//! TeamsFast spike UI: chat list, conversation, composer, live-event log.
//!
//! egui 0.36 layout: the `App::ui` trait method hands us the root `Ui`;
//! side/top/bottom regions are `egui::Panel::left/top/bottom`, and the
//! main area is `egui::CentralPanel` — all shown *inside* that root Ui.

use crate::backend::{self, Command, Event};
use egui::{Color32, RichText, ScrollArea};
use ost::api::{ChatInfo, MessageInfo};
use std::sync::mpsc::{Receiver, Sender};

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
    cmd: Sender<Command>,
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
            cmd,
            events: rx,
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Status(s) => self.status = s,
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
                Event::Messages { chat_id, messages } => {
                    if self.selected.as_deref() == Some(chat_id.as_str()) {
                        self.messages = messages;
                    }
                }
                Event::Sent(_) => self.status = "Sent".into(),
                Event::Trouter(json) => {
                    self.trouter_log.push(json);
                    if self.trouter_log.len() > 200 {
                        self.trouter_log.drain(..100);
                    }
                    // Live message activity: cheap trigger — refresh the open chat.
                    if let Some(chat) = self.selected.clone() {
                        self.cmd.send(Command::OpenChat(chat)).ok();
                    }
                }
                Event::TrouterConnected => {
                    self.trouter_on = true;
                    self.status = "Connected (live)".into();
                }
                Event::Error(e) => self.error = Some(e),
            }
        }
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
                        let label = if chat.name.is_empty() {
                            let mut s: String = chat.id.chars().take(24).collect();
                            s.push('…');
                            s
                        } else {
                            chat.name.clone()
                        };
                        if ui.selectable_label(selected, RichText::new(&label).strong()).clicked()
                        {
                            self.selected = Some(chat.id.clone());
                            self.messages.clear();
                            self.cmd.send(Command::OpenChat(chat.id.clone())).ok();
                        }
                        if let Some(p) = &chat.last_message_preview {
                            ui.label(RichText::new(p).small().weak());
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
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&m.sender);
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
