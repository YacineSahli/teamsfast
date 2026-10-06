//! TeamsFast spike UI: chat list, conversation, composer, live-event log.
//!
//! egui 0.36 layout: the `App::ui` trait method hands us the root `Ui`;
//! side/top/bottom regions are `egui::Panel::left/top/bottom`, and the
//! main area is `egui::CentralPanel` — all shown *inside* that root Ui.

use crate::backend::{self, Command, Event};
use egui::{Color32, RichText, ScrollArea};
use ost::api::{ChatInfo, MessageInfo};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone, Copy)]
enum State {
    Boot,
    NeedLogin,
    Ready,
}

pub struct TeamsFastApp {
    state: State,
    status: String,
    error: Option<String>,
    chats: Vec<ChatInfo>,
    selected: Option<String>,
    messages: Vec<MessageInfo>,
    draft: String,
    trouter_log: Vec<String>,
    trouter_on: bool,
    /// Roster names for the open chat (mri → display name).
    open_members: HashMap<String, String>,
    /// Our own display name (Graph whoami).
    self_name: String,
    /// Live events arrived; refresh the open chat at most every so often.
    pending_open_refresh: bool,
    last_open_refresh: Instant,
    cmd: tokio::sync::mpsc::UnboundedSender<Command>,
    events: Receiver<Event>,
}

impl TeamsFastApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let cmd = backend::spawn(tx);
        cmd.send(Command::CheckReady).ok();

        Self {
            state: State::Boot,
            status: "Starting…".into(),
            error: None,
            chats: Vec::new(),
            selected: None,
            messages: Vec::new(),
            draft: String::new(),
            trouter_log: Vec::new(),
            trouter_on: false,
            open_members: HashMap::new(),
            self_name: String::new(),
            pending_open_refresh: false,
            last_open_refresh: Instant::now() - Duration::from_secs(10),
            cmd,
            events: rx,
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Status(s) => self.status = s,
                Event::SelfName(name) => self.self_name = name,
                Event::LoginResult(Ok(())) => self.status = "Signed in".into(),
                Event::LoginResult(Err(e)) => {
                    self.status = "Sign-in failed".into();
                    self.error = Some(e);
                }
                Event::Ready => {
                    self.state = State::Ready;
                    self.status = "Connected".into();
                    self.cmd.send(Command::LoadChats).ok();
                }
                Event::NeedLogin(e) => {
                    self.state = State::NeedLogin;
                    self.status = "Not signed in".into();
                    self.error = Some(e);
                }
                Event::Chats(chats) => {
                    let n = chats.len();
                    self.chats = chats;
                    self.status = format!("{n} chats");
                }
                Event::Messages { chat_id, messages, members, resolved_name } => {
                    if self.selected.as_deref() == Some(chat_id.as_str()) {
                        self.messages = messages;
                        self.open_members = members;
                    }
                    // Placeholder title (e.g. "[Direct message]" on an
                    // @unq.gbl.spaces thread): use the roster-resolved name.
                    if let (Some(name), Some(chat)) =
                        (resolved_name, self.chats.iter_mut().find(|c| c.id == chat_id))
                    {
                        if chat.name.is_empty()
                            || chat.name == "[Direct message]"
                            || chat.name == "Direct message"
                        {
                            chat.name = name;
                        }
                    }
                }
                Event::Sent(_) => self.status = "Sent".into(),
                Event::Trouter(json) => {
                    self.trouter_log.push(json);
                    if self.trouter_log.len() > 200 {
                        self.trouter_log.drain(..100);
                    }
                    // Debounce: live events can storm (presence, typing…).
                    // Flag now, refetch the open chat at most ~1/s.
                    if self.selected.is_some() {
                        self.pending_open_refresh = true;
                    }
                }
                Event::TrouterConnected => {
                    self.trouter_on = true;
                    self.status = "Connected (live)".into();
                }
                Event::Error(e) => self.error = Some(e),
            }
        }

        // Debounced open-chat refresh on live activity.
        if self.pending_open_refresh && self.last_open_refresh.elapsed() >= Duration::from_millis(900)
        {
            self.pending_open_refresh = false;
            self.last_open_refresh = Instant::now();
            if let Some(chat) = self.selected.clone() {
                self.cmd.send(Command::OpenChat(chat)).ok();
            }
        }
    }

    /// Display name for a message sender, falling back to the roster when
    /// history carries none (external/federated senders arrive as "?").
    fn sender_name(&self, m: &MessageInfo) -> String {
        if m.sender.is_empty() || m.sender == "?" {
            if let Some(name) = self.open_members.get(&m.sender_mri) {
                return name.clone();
            }
        }
        m.sender.clone()
    }

    fn top_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("TeamsFast");
            ui.separator();
            match self.state {
                State::Boot => {
                    ui.spinner();
                }
                State::NeedLogin => {
                    // The Sign-in button lives below the bar, under the error line.
                }
                State::Ready => {
                    ui.colored_label(Color32::from_rgb(0x6f, 0xd1, 0x94), "●");
                    if ui.button("Refresh").clicked() {
                        self.cmd.send(Command::LoadChats).ok();
                    }
                }
            }
            ui.separator();
            ui.label(&self.status);
            if self.state == State::Ready && !self.trouter_on
                && ui.small_button("Go live").clicked()
            {
                self.cmd.send(Command::StartTrouter).ok();
            }
            if self.trouter_on {
                ui.label(
                    RichText::new("live")
                        .small()
                        .color(Color32::from_rgb(0x6f, 0xd1, 0x94)),
                );
            }
        });
        if let Some(err) = &self.error {
            ui.colored_label(Color32::from_rgb(0xe0, 0x7a, 0x7a), RichText::new(err).small());
        }
    }
}

impl eframe::App for TeamsFastApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();

        egui::Panel::top("top").show(ui, |ui| {
            self.top_bar(ui);
            if self.state == State::NeedLogin && self.error.is_some() {
                if ui.button("Sign in").clicked() {
                    self.error = None;
                    self.status = "Sign-in started — see the terminal for the device code."
                        .into();
                    self.cmd.send(Command::StartLogin).ok();
                }
                ui.label(
                    "Click Sign in, then open the URL printed in the terminal \
                     and enter the device code there.",
                );
            }
        });

        egui::Panel::left("chats")
            .default_size(270.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Chats");
                ui.separator();
                ScrollArea::vertical().show(ui, |ui| {
                    for chat in &self.chats {
                        let selected = self.selected.as_deref() == Some(chat.id.as_str());
                        let label = match chat.name.as_str() {
                            "" => {
                                let mut s: String = chat.id.chars().take(24).collect();
                                s.push('…');
                                s
                            }
                            "[Direct message]" => "Direct message".into(),
                            other => other.to_string(),
                        };
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(selected, RichText::new(&label).strong())
                                .clicked()
                            {
                                self.selected = Some(chat.id.clone());
                                self.messages.clear();
                                self.cmd.send(Command::OpenChat(chat.id.clone())).ok();
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let time = format_chat_time(&chat.last_message_time);
                                if !time.is_empty() {
                                    ui.label(RichText::new(time).small().weak());
                                }
                                if chat.is_group {
                                    ui.label(RichText::new("group").small().weak());
                                }
                            });
                        });
                        if let Some(p) = &chat.last_message_preview {
                            ui.add(
                                egui::Label::new(RichText::new(p).small().weak()).truncate(),
                            );
                        }
                        ui.separator();
                    }
                });
            });

        egui::Panel::bottom("composer").show(ui, |ui| {
            ui.horizontal(|ui| {
                let can_send = self.state == State::Ready
                    && self.selected.is_some()
                    && !self.draft.trim().is_empty();
                let send_btn = ui.add_enabled(can_send, egui::Button::new("Send"));
                let response = ui.text_edit_singleline(&mut self.draft);
                let enter =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (send_btn.clicked() || enter) && can_send {
                    if let Some(chat) = self.selected.clone() {
                        let text = std::mem::take(&mut self.draft);
                        self.status = "Sending…".into();
                        self.cmd.send(Command::Send(chat, text)).ok();
                        response.request_focus();
                    }
                }
            });
        });

        egui::Panel::bottom("live")
            .default_size(90.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.collapsing(
                    RichText::new(format!("Live events ({})", self.trouter_log.len())).small(),
                    |ui| {
                        ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                            for line in &self.trouter_log {
                                ui.label(RichText::new(line).small().weak());
                            }
                        });
                    },
                );
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.selected.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label("Pick a chat on the left.");
                });
            } else {
                ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for m in &self.messages {
                        let sender = self.sender_name(m);
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&sender);
                            ui.label(RichText::new(&m.timestamp).small().weak());
                            for r in &m.reactions {
                                ui.label(
                                    RichText::new(format!("{} {}", r.emoji, r.count))
                                        .small(),
                                );
                            }
                        });
                        ui.label(&m.content);
                        ui.add_space(6.0);
                    }
                });
            }
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
}

/// `"14:32"` when the message is from today, `"3 Oct"` otherwise.
/// Teams sends epoch-milliseconds strings; unparsable values hide the column.
fn format_chat_time(t: &Option<String>) -> String {
    let ms: u64 = match t.as_deref().and_then(|s| s.trim().parse::<u64>().ok()) {
        Some(v) => v,
        None => return String::new(),
    };
    let secs = (ms / 1000) as i64;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (ly, lm, ld) = civil_from_days(secs.div_euclid(86_400));
    let (ny, nm, nd) = civil_from_days(now_secs.div_euclid(86_400));
    if ly == ny && lm == nm && ld == nd {
        let sod = secs.rem_euclid(86_400);
        format!("{:02}:{:02}", sod / 3600, (sod % 3600) / 60)
    } else {
        const MON: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!("{} {}", ld, MON[(lm - 1) as usize % 12])
    }
}

/// Howard Hinnant's `civil_from_days`: days since epoch to (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
