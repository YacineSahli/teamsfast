//! TeamsFast App: owns all UI state, drains backend events, applies actions.

use crate::backend::{self, Command, Event};
use crate::ui::conversation::{self, Action, ConvCtx};
use crate::ui::panels::{new_chat_dialog, search_panel, NewChatState};
use crate::ui::sidebar::{sidebar, SideView};
use crate::ui::widgets::avatar;
use egui::{Color32, RichText};
use ost::api::{ChatInfo, MessageInfo, SearchHitInfo, TeamInfo};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
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
    self_name: String,
    self_id: Option<String>,

    chats: Vec<ChatInfo>,
    selected: Option<String>,
    selected_title: String,
    messages: Vec<MessageInfo>,
    members: HashMap<String, String>,
    older_link: Option<String>,
    loading_older: bool,

    draft: String,
    edit: Option<(String, String)>,
    reply: Option<(String, String, String)>,
    uploads: Vec<(String, u64, u64)>,

    teams: Vec<TeamInfo>,
    side_view: SideView,
    loading_teams: bool,
    sidebar_search: String,

    search_open: bool,
    searching: bool,
    search_hits: Vec<SearchHitInfo>,
    search_more: bool,
    search_query: String,
    search_note: Option<String>,

    new_chat: NewChatState,

    trouter_log: Vec<String>,
    trouter_on: bool,
    pending_open_refresh: bool,
    last_open_refresh: Instant,
    typing: Option<(String, Instant)>,

    textures: HashMap<String, (egui::TextureHandle, [usize; 2])>,
    lightbox: Option<String>,
    pending_images: HashSet<String>,
    queued_textures: Vec<(String, Vec<u8>, [usize; 2])>,
    #[allow(dead_code)]
    failed_images: HashSet<String>,
    egui_ctx: Option<egui::Context>,

    tray: Option<fastframe_tray::Tray>,
    window_hidden: bool,

    cmd: tokio::sync::mpsc::UnboundedSender<Command>,
    events: Receiver<Event>,
}

fn textures_key(url: &str) -> Option<String> {
    Some(url.to_string())
}

impl TeamsFastApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let cmd = backend::spawn(tx);
        cmd.send(Command::CheckReady).ok();
        Self {
            state: State::Boot,
            status: "Starting…".into(),
            error: None,
            self_name: String::new(),
            self_id: None,
            chats: Vec::new(),
            selected: None,
            selected_title: String::new(),
            messages: Vec::new(),
            members: HashMap::new(),
            older_link: None,
            loading_older: false,
            draft: String::new(),
            edit: None,
            reply: None,
            uploads: Vec::new(),
            teams: Vec::new(),
            side_view: SideView::Chats,
            loading_teams: false,
            sidebar_search: String::new(),
            search_open: false,
            searching: false,
            search_hits: Vec::new(),
            search_more: false,
            search_query: String::new(),
            search_note: None,
            new_chat: NewChatState::default(),
            trouter_log: Vec::new(),
            trouter_on: false,
            pending_open_refresh: false,
            last_open_refresh: Instant::now() - Duration::from_secs(10),
            typing: None,
            textures: HashMap::new(),
            lightbox: None,
            pending_images: HashSet::new(),
            failed_images: HashSet::new(),
            queued_textures: Vec::new(),
            egui_ctx: Some(cc.egui_ctx.clone()),
            tray: fastframe_tray::Tray::spawn(crate::tray::config(), {
                let waker = cc.egui_ctx.clone();
                move || waker.request_repaint()
            }),
            window_hidden: false,
            cmd,
            events: rx,
        }
    }

    fn sync_tray(&mut self, ui: &mut egui::Ui) {
        use crate::tray::{action, TrayAction};
        let Some(tray) = self.tray.as_ref() else {
            return;
        };
        for ev in tray.events() {
            let Some(act) = action(ev, self.window_hidden) else {
                continue;
            };
            match act {
                TrayAction::Show => {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.window_hidden = false;
                }
                TrayAction::Hide => {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.window_hidden = true;
                }
                TrayAction::Quit => {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    // ---------------------------------------------------------------- events

    fn drain_events(&mut self) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Status(s) => self.status = s,
                Event::SearchUnavailable(note) => {
                    self.search_note = Some(note);
                    self.searching = false;
                    self.search_open = true;
                }
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
                Event::Chats(mut chats) => {
                    let n = chats.len();
                    // Keep roster-resolved names from this session.
                    let known: Vec<(String, String)> = self
                        .chats
                        .iter()
                        .filter(|c| c.name != "[Direct message]" && !c.name.is_empty())
                        .map(|c| (c.id.clone(), c.name.clone()))
                        .collect();
                    for (id, name) in known {
                        if let Some(c) = chats.iter_mut().find(|c| c.id == id) {
                            if c.name == "[Direct message]" || c.name.is_empty() {
                                c.name = name;
                            }
                        }
                    }
                    self.chats = chats;
                    self.status = format!("{n} chats");
                }
                Event::Messages {
                    chat_id,
                    messages,
                    members,
                    resolved_name,
                    prepend,
                    older_link,
                } => {
                    if self.selected.as_deref() != Some(chat_id.as_str()) {
                        continue;
                    }
                    if prepend {
                        let mut merged = messages;
                        merged.extend(self.messages.drain(..));
                        let mut seen = HashSet::new();
                        merged.retain(|m| seen.insert(m.id.clone()));
                        self.messages = merged;
                        self.loading_older = false;
                    } else {
                        self.messages = messages;
                    }
                    if !members.is_empty() {
                        self.members = members;
                    }
                    self.older_link = older_link;
                    if let Some(name) = resolved_name {
                        self.rename_chat(&chat_id, name);
                    }
                    // Freshly loaded tail: mark read.
                    if let (Some(sel), Some(last)) =
                        (self.selected.clone(), self.messages.last().map(|m| m.id.clone()))
                    {
                        self.cmd
                            .send(Command::MarkRead {
                                chat_id: sel,
                                message_id: last,
                            })
                            .ok();
                    }
                }
                Event::ActionOk { action, .. } => {
                    self.status = format!("{action:?} ✓");
                }
                Event::SearchResults { hits, more, .. } => {
                    self.search_hits = hits;
                    self.search_more = more;
                    self.searching = false;
                    self.search_open = true;
                }
                Event::SearchUnavailable(note) => {
                    self.search_note = Some(note);
                    self.searching = false;
                    self.search_open = true;
                }
                Event::Teams(teams) => {
                    self.teams = teams;
                    self.loading_teams = false;
                }
                Event::CreateChatOk(chat) => {
                    let mut row = ost::api::ChatInfo {
                        id: chat.id.clone(),
                        name: chat.name.clone(),
                        is_group: chat.is_group,
                        last_message_time: None,
                        last_message_sender: None,
                        last_message_preview: None,
                    };
                    if row.name == "[Direct message]" {
                        row.name = "New chat".into();
                    }
                    self.chats.insert(0, row);
                    self.open_chat(chat.id);
                }
                Event::ImageReady { url, rgba, size } => {
                    self.pending_images.remove(&url);
                    self.queued_textures.push((url, rgba, size));
                }
                Event::ImageFailed(url) => {
                    self.pending_images.remove(&url);
                }
                Event::UploadProgress {
                    name,
                    sent,
                    total,
                    ..
                } => match self.uploads.iter_mut().find(|(n, _, _)| n == &name) {
                    Some(slot) => {
                        slot.1 = sent;
                        slot.2 = total;
                    }
                    None => self.uploads.push((name, sent, total)),
                },
                Event::UploadDone { chat_id, name } => {
                    self.uploads.retain(|(n, _, _)| n != &name);
                    if self.selected.as_deref() == Some(chat_id.as_str()) {
                        let id = chat_id.clone();
                        self.open_chat(id);
                    }
                    self.status = format!("sent {name}");
                }
                Event::DownloadDone { path, .. } => {
                    self.status = format!("saved {}", path.display());
                }
                Event::Trouter(json) => {
                    self.trouter_log.push(json);
                    if self.trouter_log.len() > 200 {
                        self.trouter_log.drain(..100);
                    }
                }
                Event::TrouterConnected => {
                    self.trouter_on = true;
                    self.status = "Connected (live)".into();
                }
                Event::IncomingMessage {
                    chat_id,
                    sender,
                    preview,
                } => self.on_incoming(chat_id, sender, preview),
                Event::Typing { chat_id, user } => {
                    if self.selected.as_deref() == Some(chat_id.as_str()) {
                        self.typing = Some((user, Instant::now()));
                    }
                }
                Event::Error(e) => self.error = Some(e),
            }
        }
        // Image → texture conversion needs the egui context; do parked ones now.
        if !self.queued_textures.is_empty() {
            if let Some(gctx) = self.egui_ctx.as_ref() {
                for (url, rgba, size) in self.queued_textures.drain(..) {
                    let img =
                        egui::ColorImage::from_rgba_unmultiplied([size[0], size[1]], &rgba);
                    let tex = gctx.load_texture(format!("img:{url}"), img, Default::default());
                    self.textures.insert(url, (tex, size));
                }
            }
        }
        if let Some((_, since)) = &self.typing {
            if since.elapsed() > Duration::from_secs(5) {
                self.typing = None;
            }
        }
        // Debounced open-chat refresh on live activity.
        if self.pending_open_refresh
            && self.last_open_refresh.elapsed() >= Duration::from_millis(900)
        {
            self.pending_open_refresh = false;
            self.last_open_refresh = Instant::now();
            if let Some(chat) = self.selected.clone() {
                self.open_chat(chat);
            }
        }
    }

    fn on_incoming(&mut self, chat_id: String, sender: String, preview: String) {
        if let Some(c) = self.chats.iter_mut().find(|c| c.id == chat_id) {
            c.last_message_preview = Some(preview.clone());
            c.last_message_sender = Some(sender.clone());
            c.last_message_time = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| (d.as_millis() as u64).to_string())
                    .unwrap_or_default(),
            );
        }
        // Re-sort by recency so the active chat bubbles to the top.
        self.chats.sort_by(|a, b| {
            let k = |c: &ChatInfo| {
                c.last_message_time
                    .as_deref()
                    .and_then(|t| t.trim().parse::<u64>().ok())
                    .unwrap_or(0)
            };
            k(b).cmp(&k(a))
        });
        let open = self.selected.as_deref() == Some(chat_id.as_str());
        if open {
            self.pending_open_refresh = true;
        } else {
            let title = self
                .chats
                .iter()
                .find(|c| c.id == chat_id)
                .map(|c| {
                    if c.name.is_empty() || c.name == "[Direct message]" {
                        "Direct message".to_string()
                    } else {
                        c.name.clone()
                    }
                })
                .unwrap_or_else(|| "Teams".into());
            let body = format!("{sender}: {preview}");
            std::thread::Builder::new()
                .name("notify".into())
                .spawn(move || {
                    let _ = notify_rust::Notification::new()
                        .summary(&title)
                        .body(&body)
                        .timeout(6000)
                        .show();
                })
                .ok();
        }
    }

    fn rename_chat(&mut self, id: &str, name: String) {
        if let Some(c) = self.chats.iter_mut().find(|c| c.id == id) {
            if c.name.is_empty() || c.name == "[Direct message]" || c.name == "Direct message" {
                c.name = name.clone();
            }
        }
        if self.selected.as_deref() == Some(id) {
            self.selected_title = name;
        }
    }

    fn open_chat(&mut self, id: String) {
        self.selected = Some(id.clone());
        self.messages.clear();
        self.members.clear();
        self.older_link = None;
        self.edit = None;
        self.reply = None;
        self.selected_title = self
            .chats
            .iter()
            .find(|c| c.id == id)
            .map(|c| {
                if c.name == "[Direct message]" || c.name.is_empty() {
                    "Direct message".into()
                } else {
                    c.name.clone()
                }
            })
            .unwrap_or_else(|| id.clone());
        self.cmd.send(Command::OpenChat(id)).ok();
    }

    // ---------------------------------------------------------------- actions

    fn apply(&mut self, a: Action) {
        match a {
            Action::OpenChat(id) => self.open_chat(id),
            Action::Send(text) => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd.send(Command::Send { chat_id: chat, text }).ok();
                }
            }
            Action::SendReply {
                parent_id,
                sender,
                snippet,
                text,
            } => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd
                        .send(Command::Reply {
                            chat_id: chat,
                            parent_id,
                            parent_sender: sender,
                            parent_text: snippet,
                            text,
                        })
                        .ok();
                    self.reply = None;
                }
            }
            Action::ApplyEdit { message_id, text } => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd
                        .send(Command::EditMessage {
                            chat_id: chat,
                            message_id,
                            text,
                        })
                        .ok();
                    self.edit = None;
                }
            }
            Action::StartEdit { message_id, current } => {
                self.edit = Some((message_id, current.clone()));
                self.reply = None;
                self.draft = current;
            }
            Action::Reply {
                message_id,
                sender,
                snippet,
            } => {
                // Hover toolbar asked to show the reply banner.
                self.edit = None;
                self.reply = Some((message_id, sender, snippet));
            }
            Action::CancelEditReply => {
                self.edit = None;
                self.reply = None;
                self.draft.clear();
            }
            Action::DeleteMessage(id) => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd
                        .send(Command::DeleteMessage {
                            chat_id: chat,
                            message_id: id,
                        })
                        .ok();
                }
            }
            Action::React {
                message_id,
                emoji,
                remove,
            } => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd
                        .send(Command::React {
                            chat_id: chat,
                            message_id,
                            emoji,
                            remove,
                        })
                        .ok();
                }
            }
            Action::LoadOlder => {
                if let Some(chat) = self.selected.clone() {
                    self.loading_older = true;
                    self.cmd.send(Command::LoadOlder(chat)).ok();
                }
            }
            Action::MarkRead(id) => {
                if let Some(chat) = self.selected.clone() {
                    self.cmd
                        .send(Command::MarkRead {
                            chat_id: chat,
                            message_id: id,
                        })
                        .ok();
                }
            }
            Action::Attach => {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    if let Some(chat) = self.selected.clone() {
                        self.cmd
                            .send(Command::UploadFile { chat_id: chat, path })
                            .ok();
                    }
                }
            }
            Action::FetchImage(url) => {
                self.cmd.send(Command::FetchImage(url)).ok();
            }
            Action::OpenImage(url) => {
                self.lightbox = textures_key(&url);
            }
            Action::DownloadFile { name, url } => {
                self.cmd.send(Command::DownloadFile { url, name }).ok();
            }
            Action::OpenSearch => self.search_open = true,
            Action::ShowNewChat => self.new_chat.open = true,
            Action::CreateOneToOne(peer) => {
                self.cmd.send(Command::CreateOneToOne(peer)).ok();
            }
            Action::CreateGroup { topic, members } => {
                self.cmd
                    .send(Command::CreateGroup { topic, members })
                    .ok();
            }
            Action::Refresh => {
                self.cmd.send(Command::LoadChats).ok();
            }
        }
    }

    // ---------------------------------------------------------------- layout

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong(RichText::new("TeamsFast").size(15.0));
            ui.separator();
            match self.state {
                State::Boot => {
                    ui.spinner();
                }
                State::NeedLogin => {
                    if ui.button("Sign in").clicked() {
                        self.error = None;
                        self.status =
                            "Sign-in started — see the terminal for the device code.".into();
                        self.cmd.send(Command::StartLogin).ok();
                    }
                }
                State::Ready => {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 4.0, Color32::from_rgb(0x6f, 0xd1, 0x94));
                    if ui.button("Refresh").clicked() {
                        self.apply(Action::Refresh);
                    }
                    if ui.button("New chat").clicked() {
                        self.apply(Action::ShowNewChat);
                    }
                    if ui.button("Search").clicked() {
                        self.apply(Action::OpenSearch);
                    }
                }
            }
            ui.separator();
            ui.label(&self.status);
            if self.state == State::Ready
                && !self.trouter_on
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
            if !self.self_name.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&self.self_name).weak().small());
                    avatar(ui, &self.self_name, 24.0);
                });
            }
        });
        if let Some(err) = &self.error {
            ui.colored_label(Color32::from_rgb(0xe0, 0x7a, 0x7a), RichText::new(err).small());
        }
        if self.state == State::NeedLogin && self.error.is_some() {
            ui.label("Click Sign in, then open the URL printed in the terminal and enter the device code.");
        }
    }
}

impl eframe::App for TeamsFastApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.sync_tray(ui);

        egui::Panel::top("top").show(ui, |ui| {
            self.top_bar(ui);
        });

        egui::Panel::left("side")
            .default_size(300.0)
            .resizable(true)
            .show(ui, |ui| {
                let mut sctx = crate::ui::sidebar::SidebarCtx {
                    chats: &self.chats,
                    selected: self.selected.as_ref(),
                    teams: &self.teams,
                    view: self.side_view,
                    cmd: &self.cmd,
                };
                let mut loading = self.loading_teams;
                let mut search = std::mem::take(&mut self.sidebar_search);
                let mut side_actions: Vec<Action> = Vec::new();
                sidebar(ui, &mut sctx, &mut search, &mut loading, &mut side_actions);
                self.sidebar_search = search;
                self.loading_teams = loading;
                self.side_view = sctx.view;
                for a in side_actions {
                    self.apply(a);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.state != State::Ready {
                ui.centered_and_justified(|ui| match self.state {
                    State::Boot => ui.label("Starting…"),
                    State::NeedLogin => ui.label("Sign in to start."),
                    State::Ready => unreachable!(),
                });
                return;
            }
            if self.selected.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label("Pick a chat on the left, or press New chat (Ctrl+N).");
                });
                return;
            }
            let chat_id = self.selected.clone().unwrap();
            let mut actions: Vec<Action> = Vec::new();
            let mut ctx = ConvCtx {
                chat_name: self.selected_title.clone(),
                chat_id: chat_id.clone(),
                messages: &self.messages,
                members: &self.members,
                self_name: &self.self_name,
                self_id: self.self_id.as_deref(),
                older_link: self.older_link.as_deref(),
                loading_older: self.loading_older,
                typing_user: self.typing.as_ref().map(|(u, _)| u.as_str()),
                edit: self.edit.clone(),
                reply: self.reply.clone(),
                uploads: &self.uploads,
                textures: &self.textures,
                pending_images: &mut self.pending_images,
                actions: &mut actions,
            };
            conversation::conversation(ui, &mut ctx, &mut self.draft);
            drop(ctx);
            for a in actions {
                self.apply(a);
            }
        });

        if self.search_open {
            egui::Panel::right("search")
                .default_size(330.0)
                .resizable(true)
                .show(ui, |ui| {
                    let cmd = self.cmd.clone();
                    search_panel(
                        ui,
                        &self.search_hits,
                        self.search_more,
                        self.searching,
                        &cmd,
                        &self.search_query,
                        self.search_note.as_deref(),
                    );
                });
        }

        if self.new_chat.open {
            egui::Window::new("New conversation")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    let mut actions = Vec::new();
                    new_chat_dialog(ui, &mut self.new_chat, &mut actions);
                    for a in actions {
                        self.apply(a);
                    }
                });
        }

        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::N)) {
            self.apply(Action::ShowNewChat);
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F)) {
            self.apply(Action::OpenSearch);
        }

        if let Some(url) = self.lightbox.clone() {
            if let Some((tex, size)) = self.textures.get(&url) {
                let (tex, size) = (tex.clone(), *size);
                let close = crate::ui::media::lightbox(ui, &tex, size, &url);
                if close {
                    self.lightbox = None;
                }
            } else {
                self.lightbox = None;
            }
        }

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
}
