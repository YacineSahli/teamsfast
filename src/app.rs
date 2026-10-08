//! TeamsFast App: owns all UI state, drains backend events, applies actions.

use crate::backend::{self, Command, Event};
use crate::theme::{self, Palette};
use crate::ui::conversation::{self, Action, ConvCtx};
use crate::ui::panels::{new_chat_dialog, search_panel, NewChatState};
use crate::ui::settings::{SettingsInfo, SettingsUi};
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

/// The active section (left icon rail).
#[derive(PartialEq, Clone, Copy)]
enum MainView {
    Chat,
    Teams,
    Calendar,
    Files,
    ToDo,
    Planner,
    Shifts,
    Notes,
    Activity,
}

/// An own message on its way out (or failed, awaiting Retry).
#[derive(Clone)]
pub struct PendingSend {
    pub cmid: String,
    pub chat_id: String,
    pub text: String,
    pub error: Option<String>,
}

/// Forward-to-chat picker.
pub struct ForwardState {
    pub text: String,
    pub filter: String,
}

/// A ringing incoming call: caller identity for the banner plus the raw
/// invitation JSON the backend needs to accept/decline.
#[derive(Clone)]
pub struct IncomingRing {
    pub name: String,
    pub mri: String,
    pub has_video: bool,
    pub raw: String,
    pub at: Instant,
}

/// Teams rings for roughly 30 s before voicemail; drop the banner past 45 s
/// (caller-cancel pushes are not capture-verified yet — see NOTES).
pub const RING_TIMEOUT: Duration = Duration::from_secs(45);

/// Drain a frame channel, keeping only the newest frame (live video is
/// latest-wins; backlog is dropped).
fn drain_latest(
    rx: &std::sync::mpsc::Receiver<ost::calling::video::VideoFrame>,
) -> Option<ost::calling::video::VideoFrame> {
    let mut latest = None;
    while let Ok(f) = rx.try_recv() {
        latest = Some(f);
    }
    latest
}

/// Upload/refresh a video texture from an I420 frame; recreates the
/// texture when the resolution changes.
fn apply_frame(
    ctx: &egui::Context,
    slot: Option<egui::TextureHandle>,
    id: &str,
    frame: &ost::calling::video::VideoFrame,
) -> egui::TextureHandle {
    let rgba = frame.to_rgba();
    let img = egui::ColorImage::from_rgba_unmultiplied(
        [frame.width as usize, frame.height as usize],
        &rgba,
    );
    if let Some(mut t) = slot {
        if t.size() == [frame.width as usize, frame.height as usize] {
            t.set(img, egui::TextureOptions::LINEAR);
            return t;
        }
    }
    ctx.load_texture(id, img, egui::TextureOptions::LINEAR)
}

/// QA (TEAMSFAST_FAKEVIDEO=1): animated color bars + scanline so the
/// remote tile can be pixel-checked without a real peer.
fn fake_frame(t_ms: u128) -> ost::calling::video::VideoFrame {
    let (w, h) = (320u32, 240u32);
    let (wu, hu) = (w as usize, h as usize);
    let ysz = wu * hu;
    let mut data = vec![128u8; ysz + ysz / 2];
    let shift = ((t_ms / 40) as u32 % w) as usize;
    let bar_w = (w / 6) as usize;
    for x in 0..wu {
        let bar = ((x + shift) / bar_w) % 6;
        let yv = (60 + bar * 30) as u8;
        let (u, v) = if bar % 2 == 0 { (180u8, 60u8) } else { (90u8, 170u8) };
        for row in 0..hu {
            data[row * wu + x] = yv;
        }
        let cx = x / 2;
        for r in 0..hu / 2 {
            data[ysz + r * (wu / 2) + cx] = u;
            data[ysz + (wu / 2) * (hu / 2) + r * (wu / 2) + cx] = v;
        }
    }
    let line = ((t_ms / 8) as u32 % h) as usize;
    for x in 0..wu {
        data[line * wu + x] = 235;
    }
    ost::calling::video::VideoFrame { width: w, height: h, data }
}

/// Synthetic invitation for the TEAMSFAST_RING=1 QA hook (parseable, with
/// dead links so an accidental accept fails harmlessly).
pub fn qa_incoming_ring() -> IncomingRing {    let raw = serde_json::json!({
        "callInvitation": {
            "callModalities": ["Audio"],
            "links": {
                "acceptance": "https://qa.invalid/accept",
                "end": "https://qa.invalid/end",
                "mediaAnswer": "https://qa.invalid/media"
            },
            "mediaContent": { "contentType": "application/sdp", "blob": "" }
        },
        "participants": {
            "from": { "id": "8:orgid:00000000-0000-0000-0000-000000000000",
                      "displayName": "QA Incoming Caller" }
        },
        "debugContent": { "callId": "qa-ring" }
    })
    .to_string();
    IncomingRing {
        name: "QA Incoming Caller".into(),
        mri: "8:orgid:00000000-0000-0000-0000-000000000000".into(),
        has_video: false,
        raw,
        at: Instant::now(),
    }
}

/// One update-flow message from the update thread.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    Checking,
    /// A newer release exists (version, release page URL).
    Available { version: String, url: String },
    /// Download progress in percent.
    Progress(u8),
    /// Downloaded + verified; waiting for the user to restart.
    Ready { version: String },
    /// This copy may not self-update, or the check/download failed.
    Blocked(String),
    /// The helper was asked to install; quit now.
    Restarting,
}

/// UI-side view of the update flow. The thread owns the Updater and the
/// Prepared update; the UI only ever sees events and sends commands.
#[derive(Default)]
pub struct UpdateState {
    pub dialog: Option<UpdateDialog>,
    /// Commands to the update thread (Restart / Dismiss).
    pub cmd_tx: Option<std::sync::mpsc::Sender<UpdateCommand>>,
    pub events: Option<std::sync::mpsc::Receiver<UpdateEvent>>,
    /// Checked once per launch.
    checked: bool,
    receipt_acknowledged: bool,
}

/// What the update window shows.
pub struct UpdateDialog {
    pub kind: UpdateDialogKind,
    pub version: String,
    pub url: String,
}

pub enum UpdateDialogKind {
    Progress(u8),
    Ready,
    Failed(String),
    Unsupported(String),
}

/// UI → update-thread commands.
pub enum UpdateCommand {
    Restart,
    Dismiss,
}

/// Spawn the update worker: check → download (with progress) → park until
/// the user restarts or dismisses.
fn spawn_update_worker(tx: std::sync::mpsc::Sender<UpdateEvent>, rx: std::sync::mpsc::Receiver<UpdateCommand>) {
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            let send = |ev: UpdateEvent| {
                let _ = tx.send(ev);
            };
            let updater = match crate::updates::updater() {
                Ok(u) => u,
                Err(e) => {
                    send(UpdateEvent::Blocked(format!("update client: {e:#}")));
                    return;
                }
            };
            let release = match updater.check() {
                Ok(Some(r)) => r,
                Ok(None) => return, // up to date: silent
                Err(e) => {
                    log::debug!("update check: {e:#}");
                    return; // offline at launch: silent
                }
            };
            send(UpdateEvent::Available {
                version: release.version.clone(),
                url: release.url.clone(),
            });
            match updater.installation() {
                Ok(_) => {}
                Err(reason) => {
                    send(UpdateEvent::Blocked(reason.to_string()));
                    return;
                }
            }
            let prepared = match updater.download(&release, |received, total| {
                let pct = if total > 0 {
                    ((received * 100) / total).min(100) as u8
                } else {
                    0
                };
                let _ = tx.send(UpdateEvent::Progress(pct));
            }) {
                Ok(p) => p,
                Err(e) => {
                    send(UpdateEvent::Blocked(format!("download: {e:#}")));
                    return;
                }
            };
            let version = release.version.clone();
            send(UpdateEvent::Ready { version });
            // Park until the UI decides.
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    UpdateCommand::Restart => {
                        let args: Vec<String> = std::env::args()
                            .skip(1)
                            .filter(|a| {
                                !a.starts_with("--update-")
                                    && a != fastframe_update::APPLY_UPDATE_FLAG
                            })
                            .collect();
                        match updater.handoff(prepared, args) {
                            Ok(()) => send(UpdateEvent::Restarting),
                            Err(e) => send(UpdateEvent::Blocked(format!("restart: {e:#}"))),
                        }
                        return;
                    }
                    UpdateCommand::Dismiss => return,
                }
            }
        })
        .ok();
}

/// "Create channel / join team" dialog state.
#[derive(Default)]
pub struct TeamDialogState {
    pub open: bool,
    /// Channel tab.
    pub channel_team: Option<String>,
    pub channel_name: String,
    /// Join tab.
    pub join_query: String,
    pub searching: bool,
    pub public_teams: Vec<ost::api::PublicTeamInfo>,
    /// Create-team tab.
    pub new_team_name: String,
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
    offline: Option<String>,
    last_offline_retry: Instant,
    /// Our presence availability ("" until first poll succeeds).
    presence: String,
    last_presence_poll: Instant,
    /// Own sends in flight (or failed, with the error text).
    pending_sends: Vec<PendingSend>,
    /// Read receipts for the open chat: message id → user mris.
    receipts: HashMap<String, Vec<String>>,
    /// Forward picker state: the text being forwarded + filter.
    forward: Option<ForwardState>,
    /// Device-code sign-in state (URL + code once the backend has them).
    login_code: Option<(String, String)>,
    /// Active call (label + start time) while the backend call task runs.
    call: Option<(String, Instant)>,
    /// Ringing incoming call (banner with Accept/Decline).
    incoming: Option<IncomingRing>,
    /// Live call controls (mute/camera watches + video frame receivers)
    /// for the call stage UI; one per placed call.
    call_media: Option<ost::calling::CallControlsHandle>,
    /// Call stage panel open (tiles + controls).
    call_view_open: bool,
    /// Remote/local video textures for the stage (recreated on size change).
    remote_video_tex: Option<egui::TextureHandle>,
    local_video_tex: Option<egui::TextureHandle>,
    /// QA: TEAMSFAST_FAKEVIDEO=1 animates the remote tile without a peer.
    fake_video: bool,

    /// "Join with link" dialog state.
    join_open: bool,
    join_source: String,
    /// Subject hint for the next meeting join's banner label.
    call_label_hint: Option<String>,
    /// Teams-management dialog (create channel, join/create team).
    team_dialog: TeamDialogState,
    /// Contact card for a member (mri, display name).
    contact: Option<(String, String)>,

    // ---- sections ----
    main_view: MainView,
    meetings: Vec<ost::api::MeetingInfo>,
    calendar_loading: bool,
    drive_files: Vec<ost::api::SharedFile>,
    files_loading: bool,
    todo_lists: Vec<ost::api::TodoListInfo>,
    todo_tasks: HashMap<String, Vec<ost::api::TodoTaskInfo>>,
    todo: crate::ui::sections::TodoPanelState,
    todo_loading: bool,
    planner: Vec<crate::ui::sections::PlannerBoard>,
    planner_loading: bool,
    /// Files shared in a chat (chat id + name for display).
    chat_files: Option<(String, String, Vec<ost::api::SharedFile>)>,
    chat_files_loading: bool,
    /// Rename-channel dialog (team, channel, current name).
    channel_rename: Option<(String, String, String)>,
    /// OneNote state.
    notebooks: Vec<crate::ui::sections::NotebookTree>,
    notes_page: Option<ost::api::NotePage>,
    notes: crate::ui::sections::NotesPanelState,
    notes_loading: bool,
    shifts: Vec<ost::api::ShiftInfo>,
    shifts_loading: bool,
    activity: Vec<crate::ui::sections::ActivityEntry>,

    /// Unread state per chat: (is-unread, approximate count).
    unread: HashMap<String, (bool, u32)>,
    /// Settings window state + cached archive row counts for its display.
    settings_ui: SettingsUi,
    archive_stats: (usize, usize),
    /// True while the settings window is open (open-edge detection).
    settings_was_open: bool,
    /// Hide the window on the first frame (start-in-tray).
    pending_hide: bool,
    /// A real quit was requested (tray Quit): the close-to-tray cancel
    /// must stand down so the process can exit.
    quitting: bool,
    /// Update flow state + the launch receipt to acknowledge.
    update: UpdateState,
    update_receipt: Option<fastframe_update::Receipt>,
    launch_error: Option<String>,
    /// Notification clicks: chat ids delivered by notify threads.
    notif_clicks: Receiver<String>,
    /// The sender side cloned into every notification thread.
    notif_tx: Sender<String>,

    textures: HashMap<String, (egui::TextureHandle, [usize; 2])>,
    emoji_textures: HashMap<String, egui::TextureHandle>,
    reaction_popup: Option<(String, Vec<String>)>,
    lightbox: Option<String>,
    pending_images: HashSet<String>,
    queued_textures: Vec<(String, Vec<u8>, [usize; 2])>,
    #[allow(dead_code)]
    failed_images: HashSet<String>,
    egui_ctx: Option<egui::Context>,
    catalog: crate::theme::Catalog,
    palette: Palette,
    selected_theme: Option<String>,
    settings: theme::Settings,
    pending_theme: Option<String>,

    tray: Option<fastframe_tray::Tray>,
    window_hidden: bool,

    cmd: tokio::sync::mpsc::UnboundedSender<Command>,
    events: Receiver<Event>,
}

fn textures_key(url: &str) -> Option<String> {
    Some(url.to_string())
}

/// Is the current local time inside the quiet window? Handles windows that
/// cross midnight ("22:00"–"07:00"); unparsable times disable the check.
fn in_quiet_hours(from: &str, to: &str) -> bool {
    let parse = |s: &str| -> Option<u32> {
        let (h, m) = s.trim().split_once(':')?;
        let h: u32 = h.parse().ok()?;
        let m: u32 = m.parse().ok()?;
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    let (Some(from), Some(to)) = (parse(from), parse(to)) else {
        return false;
    };
    let now = {
        let t = jiff::Zoned::now();
        t.hour() as u32 * 60 + t.minute() as u32
    };
    if from <= to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

/// Open a folder in the desktop file manager (created first).
fn open_folder(path: PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&path) {
        log::warn!("create {}: {e:#}", path.display());
    }
    if let Err(e) = open::that_detached(&path) {
        log::warn!("open {}: {e:#}", path.display());
    }
}

/// File picker. GNOME's xdg portal rejects unregistered dev binaries
/// (rfd/ashpd fails with UnknownMethod), so prefer `zenity` when installed
/// and fall back to rfd elsewhere. Cancel (zenity exit 1) yields None.
fn pick_file() -> Option<std::path::PathBuf> {
    match std::process::Command::new("zenity")
        .args(["--file-selection", "--title=Attach a file"])
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if p.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(p))
            }
        }
        Ok(out) if out.status.code() == Some(1) => None, // user cancelled
        _ => rfd::FileDialog::new().pick_file(),
    }
}

/// Spawn the tray on a blocking-pool thread of a runtime that lives for the
/// whole process. ksni's blocking API needs: no async-worker context (its
/// nested block_on is illegal there) plus a tokio reactor for its spawned
/// tasks — a blocking-pool thread of a persistent multi-thread runtime
/// satisfies both.
fn spawn_tray(waker: egui::Context) -> Option<fastframe_tray::Tray> {
    let (tx, rx) = std::sync::mpsc::channel();
    let started = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            // No tokio runtime on this thread: ksni/zbus bring their own
            // (zbus >= 5.19 fixed the executor's ambient-reactor panic).
            let tray =
                fastframe_tray::Tray::spawn(crate::tray::config(), move || {
                    waker.request_repaint()
                });
            let _ = tx.send(tray);
        });
    if started.is_err() {
        return None;
    }
    rx.recv_timeout(Duration::from_secs(3)).unwrap_or(None)
}

/// Teams management dialog body.
#[allow(clippy::too_many_lines)]
fn teams_dialog(
    ui: &mut egui::Ui,
    st: &mut TeamDialogState,
    teams: &[TeamInfo],
    actions: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical()
        .id_salt("team-dialog-scroll")
        .max_height(420.0)
        .show(ui, |ui| {
            ui.strong("Create a channel");
            ui.horizontal(|ui| {
                let sel = st
                    .channel_team
                    .as_deref()
                    .and_then(|id| teams.iter().find(|t| t.id == id))
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "Pick team".into());
                egui::ComboBox::from_id_salt("channel-team")
                    .selected_text(sel)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for t in teams {
                            let is = st.channel_team.as_deref() == Some(t.id.as_str());
                            if ui.selectable_label(is, &t.name).clicked() {
                                st.channel_team = Some(t.id.clone());
                            }
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut st.channel_name)
                        .hint_text("Channel name")
                        .desired_width(ui.available_width() - 78.0),
                );
                let enabled = !st.channel_name.trim().is_empty()
                    && st.channel_team.is_some();
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(RichText::new("Create").small()),
                    )
                    .clicked()
                {
                    let team = st.channel_team.clone().unwrap();
                    let name = st.channel_name.trim().to_string();
                    st.channel_name.clear();
                    actions.push(Action::CreateChannel {
                        team_id: team,
                        name,
                    });
                }
            });
            ui.add_space(10.0);
            ui.separator();
            ui.strong("Join a public team");
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut st.join_query)
                        .hint_text("Search public teams…")
                        .desired_width(ui.available_width() - 70.0),
                );
                let enter =
                    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Search").clicked() || enter)
                    && !st.join_query.trim().is_empty()
                {
                    actions.push(Action::SearchPublicTeams(
                        st.join_query.trim().to_string(),
                    ));
                }
            });
            if st.searching {
                ui.spinner();
            }
            for t in &st.public_teams {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 80.0);
                        ui.strong(&t.name);
                        if let Some(d) = &t.description {
                            ui.add(
                                egui::Label::new(RichText::new(d).small().weak())
                                    .truncate()
                                    .selectable(false),
                            );
                        }
                    });
                    if ui.small_button("Join").clicked() {
                        actions.push(Action::JoinTeam {
                            team_id: t.id.clone(),
                            name: t.name.clone(),
                        });
                    }
                });
                ui.separator();
            }
            ui.add_space(4.0);
            ui.separator();
            ui.strong("Create a team");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut st.new_team_name)
                        .hint_text("Team name")
                        .desired_width(ui.available_width() - 78.0),
                );
                if ui
                    .add_enabled(
                        !st.new_team_name.trim().is_empty(),
                        egui::Button::new("Create").small(),
                    )
                    .clicked()
                {
                    actions.push(Action::CreateTeam(st.new_team_name.trim().to_string()));
                    st.new_team_name.clear();
                }
            });
        });
}

impl TeamsFastApp {
    /// Drain the call's video channels into the stage textures (latest
    /// wins). No-op without an active call.
    fn update_call_stage_frames(&mut self, ctx: &egui::Context) {
        let Some(media) = self.call_media.as_ref() else {
            return;
        };
        let t_ms = self
            .call
            .as_ref()
            .map(|(_, at)| at.elapsed().as_millis())
            .unwrap_or(0);
        let remote = match drain_latest(&media.remote_frames) {
            Some(f) => Some(f),
            None if self.fake_video => Some(fake_frame(t_ms)),
            None => None,
        };
        if let Some(f) = remote {
            self.remote_video_tex = Some(apply_frame(
                ctx,
                self.remote_video_tex.take(),
                "teamsfast-remote-video",
                &f,
            ));
        }
        if let Some(rx) = media.local_preview.as_ref() {
            if let Some(f) = drain_latest(rx) {
                self.local_video_tex = Some(apply_frame(
                    ctx,
                    self.local_video_tex.take(),
                    "teamsfast-local-video",
                    &f,
                ));
            }
        }
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        receipt: Option<fastframe_update::Receipt>,
        launch_error: Option<String>,
    ) -> Self {
        let settings_load = theme::load_settings();
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let cmd = backend::spawn(tx);
        cmd.send(Command::CheckReady).ok();
        if settings_load.auto_live {
            cmd.send(Command::StartTrouter).ok();
        }
        // Interface zoom from settings.
        cc.egui_ctx.set_zoom_factor(settings_load.zoom);
        let (notif_tx, notif_rx) = std::sync::mpsc::channel::<String>();
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
            emoji_textures: HashMap::new(),
            reaction_popup: None,
            lightbox: None,
            pending_images: HashSet::new(),
            failed_images: HashSet::new(),
            queued_textures: Vec::new(),
            egui_ctx: Some(cc.egui_ctx.clone()),
            tray: spawn_tray(cc.egui_ctx.clone()),
            window_hidden: false,
            pending_theme: settings_load.theme.clone(),
            offline: None,
            last_offline_retry: Instant::now() - Duration::from_secs(60),
            presence: String::new(),
            last_presence_poll: Instant::now() - Duration::from_secs(3600),
            pending_sends: Vec::new(),
            receipts: HashMap::new(),
            forward: None,
            login_code: None,
            call: None,
            incoming: None,
            call_media: None,
            call_view_open: false,
            remote_video_tex: None,
            local_video_tex: None,
            fake_video: std::env::var("TEAMSFAST_FAKEVIDEO").as_deref() == Ok("1"),
            join_open: false,
            join_source: String::new(),
            call_label_hint: None,
            team_dialog: TeamDialogState::default(),
            contact: None,
            main_view: MainView::Chat,
            meetings: Vec::new(),
            calendar_loading: false,
            drive_files: Vec::new(),
            files_loading: false,
            todo_lists: Vec::new(),
            todo_tasks: HashMap::new(),
            todo: Default::default(),
            todo_loading: false,
            planner: Vec::new(),
            planner_loading: false,
            chat_files: None,
            chat_files_loading: false,
            channel_rename: None,
            notebooks: Vec::new(),
            notes_page: None,
            notes: Default::default(),
            notes_loading: false,
            shifts: Vec::new(),
            shifts_loading: false,
            activity: Vec::new(),
            catalog: theme::theme_catalog(cc.egui_ctx.clone()),
            palette: Palette::dark(),
            selected_theme: settings_load.theme.clone(),
            unread: HashMap::new(),
            settings_ui: SettingsUi::default(),
            archive_stats: (0, 0),
            settings_was_open: false,
            pending_hide: settings_load.start_in_tray,
            quitting: false,
            notif_clicks: notif_rx,
            notif_tx,
            cmd,
            events: rx,
            settings: settings_load,
            update: UpdateState::default(),
            update_receipt: receipt,
            launch_error,
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
                    // Mark a real quit so the close-to-tray handler lets
                    // the close through instead of hiding again.
                    self.quitting = true;
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
                Event::Offline(err) => {
                    self.offline = Some(err.clone());
                    self.status = "Offline — showing cached data".into();
                    self.error = None;
                    self.state = State::Ready;
                    // Retry the session periodically; on success Ready fires.
                }
                Event::SelfId(id) => self.self_id = Some(id),
                Event::LoginResult(Ok(())) => {
                    self.status = "Signed in".into();
                    self.login_code = None;
                }
                Event::LoginResult(Err(e)) => {
                    self.status = "Sign-in failed".into();
                    self.error = Some(e);
                    self.login_code = None;
                }
                Event::LoginCode { url, code } => {
                    self.login_code = Some((url, code));
                    self.status = "Finish the sign-in in your browser…".into();
                }
                Event::Planner(boards) => {
                    self.planner = boards;
                    self.planner_loading = false;
                }
                Event::Shifts(shifts) => {
                    self.shifts = shifts;
                    self.shifts_loading = false;
                }
                Event::Notes(books) => {
                    self.notebooks = books;
                    self.notes_loading = false;
                }
                Event::NotePage(page) => {
                    self.notes_page = Some(page);
                }
                Event::MeetNowReady { join_url } => {
                    self.status = "meeting created — joining…".into();
                    self.call_label_hint = Some("Meet now".into());
                    self.cmd.send(Command::JoinMeeting(join_url)).ok();
                }
                Event::ChatFiles { chat_id, files } => {
                    let name = self
                        .chats
                        .iter()
                        .find(|c| c.id == chat_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| chat_id.clone());
                    self.chat_files = Some((chat_id, name, files));
                    self.chat_files_loading = false;
                }
                Event::PublicTeams(teams) => {
                    self.team_dialog.public_teams = teams;
                    self.team_dialog.searching = false;
                }
                // ---- calls ----
                Event::IncomingCall {
                    caller_name,
                    caller_mri,
                    has_video,
                    raw,
                } => {
                    self.incoming = Some(IncomingRing {
                        name: caller_name,
                        mri: caller_mri,
                        has_video,
                        raw,
                        at: Instant::now(),
                    });
                }
                Event::CallGone => {
                    // Caller cancelled while ringing: drop the banner. The
                    // accepted-call case stays manual until the callback
                    // shapes are capture-verified (see NOTES).
                    self.incoming = None;
                }
                Event::CallMedia(handle) => {
                    // Video calls open the stage; voice calls stay on the
                    // banner — a large placeholder tile on an audio call
                    // just reads as an empty video screen. The local-
                    // preview sink exists only when the camera is on.
                    self.call_view_open = handle.local_preview.is_some();
                    self.call_media = Some(handle);
                    self.remote_video_tex = None;
                    self.local_video_tex = None;
                }
                Event::CallStarted { label } => {
                    // A hint (meeting subject from the calendar) wins over
                    // the generic "Meeting"/thread-id label.
                    let label = self
                        .call_label_hint
                        .take()
                        .unwrap_or(label);
                    self.call = Some((label, Instant::now()));
                }
                Event::CallStatus(s) => {
                    if let Some((label, at)) = self.call.take() {
                        // With live controls the banner shows the driver's
                        // phase; appending static text duplicates it.
                        if self.call_media.is_some() {
                            self.call = Some((label, at));
                        } else {
                            self.call = Some((format!("{label} — {s}"), at));
                        }
                    }
                }
                Event::CallEnded(summary) => {
                    self.call = None;
                    self.call_media = None;
                    self.call_view_open = false;
                    self.remote_video_tex = None;
                    self.local_video_tex = None;
                    self.status = summary;
                }
                Event::CallFailed(e) => {
                    self.call = None;
                    self.call_media = None;
                    self.call_view_open = false;
                    self.remote_video_tex = None;
                    self.local_video_tex = None;
                    self.error = Some(format!("call: {e}"));
                }
                Event::Ready => {
                    self.state = State::Ready;
                    self.status = "Connected".into();
                    self.cmd.send(Command::LoadChats).ok();
                    // QA hook: TEAMSFAST_OPEN=<chat_id> opens it on launch.
                    if let Ok(id) = std::env::var("TEAMSFAST_OPEN") {
                        if !id.trim().is_empty() {
                            self.open_chat(id.trim().to_string());
                        }
                    }
                    // QA hook: TEAMSFAST_SETTINGS=1 opens the settings window.
                    if std::env::var("TEAMSFAST_SETTINGS").as_deref() == Ok("1") {
                        self.settings_ui.open = true;
                    }
                    // QA hook: TEAMSFAST_TESTCALL=1 places an echo test call;
                    // TEAMSFAST_VIDEOCALL=1 does the same with the camera on.
                    if std::env::var("TEAMSFAST_TESTCALL").as_deref() == Ok("1") {
                        self.cmd.send(Command::TestCall { video: false }).ok();
                    }
                    if std::env::var("TEAMSFAST_VIDEOCALL").as_deref() == Ok("1") {
                        self.cmd.send(Command::TestCall { video: true }).ok();
                    }
                    // QA hook: TEAMSFAST_HANGUP_AFTER=<secs> hangs up N
                    // seconds after Ready — bounds headless call QA so the
                    // call summary actually prints.
                    if let Ok(secs) = std::env::var("TEAMSFAST_HANGUP_AFTER") {
                        if let Ok(secs) = secs.trim().parse::<u64>() {
                            let cmd = self.cmd.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_secs(secs));
                                cmd.send(crate::backend::Command::HangUp).ok();
                            });
                        }
                    }
                    // QA hook: TEAMSFAST_RING=1 shows the ringing banner
                    // with a synthetic caller (pixel QA for incoming calls).
                    if std::env::var("TEAMSFAST_RING").as_deref() == Ok("1") {
                        self.incoming = Some(qa_incoming_ring());
                    }
                    // QA hook: TEAMSFAST_CHATFILES=1 fetches the open chat's
                    // shared files (pair with TEAMSFAST_OPEN).
                    if std::env::var("TEAMSFAST_CHATFILES").as_deref() == Ok("1") {
                        if let Some(id) = std::env::var("TEAMSFAST_OPEN").ok() {
                            if !id.trim().is_empty() {
                                self.switch_view(MainView::Files);
                                self.chat_files_loading = true;
                                self.cmd.send(Command::ChatFiles(id)).ok();
                            }
                        }
                    }
                    // QA hooks: TEAMSFAST_TEAMS=1 / TEAMSFAST_JOINDLG=1
                    // open their dialogs for screenshot QA.
                    if std::env::var("TEAMSFAST_TEAMS").as_deref() == Ok("1") {
                        self.apply(Action::ShowTeamDialog);
                    }
                    if std::env::var("TEAMSFAST_JOINDLG").as_deref() == Ok("1") {
                        self.join_open = true;
                    }
                    // QA hook: TEAMSFAST_VIEW=<chat|teams|calendar|files|todo|activity>
                    if let Ok(v) = std::env::var("TEAMSFAST_VIEW") {
                        let view = match v.trim().to_ascii_lowercase().as_str() {
                            "teams" => Some(MainView::Teams),
                            "calendar" | "cal" => Some(MainView::Calendar),
                            "files" => Some(MainView::Files),
                            "todo" => Some(MainView::ToDo),
                            "planner" => Some(MainView::Planner),
                            "shifts" => Some(MainView::Shifts),
                            "notes" | "onenote" => Some(MainView::Notes),
                            "activity" => Some(MainView::Activity),
                            _ => None,
                        };
                        if let Some(view) = view {
                            self.switch_view(view);
                        }
                    }
                }
                Event::NeedLogin(e) => {
                    self.state = State::NeedLogin;
                    self.status = "Not signed in".into();
                    self.error = Some(e);
                }
                Event::ChatRenamed { chat_id, name } => {
                    // Roster-resolved name beats the list heuristic (a
                    // bot-carrying 1:1 falls back to the last sender and
                    // can show our own name).
                    if let Some(c) = self.chats.iter_mut().find(|c| c.id == chat_id) {
                        c.name = name.clone();
                    }
                    if self.selected.as_deref() == Some(chat_id.as_str())
                        && !self.selected_title.starts_with("# ")
                    {
                        self.selected_title = name;
                    }
                    self.sort_chats();
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
                    self.sort_chats();
                    self.status = format!("{n} chats");
                    // A conversation opened before the list loaded (launch
                    // hook, notification click): give it its real title.
                    if let Some(sel) = self.selected.clone() {
                        if let Some(c) = self.chats.iter().find(|c| c.id == sel) {
                            let name = if c.name.is_empty()
                                || c.name == "[Direct message]"
                            {
                                "Direct message".to_string()
                            } else {
                                c.name.clone()
                            };
                            // Don't clobber a resolved channel title
                            // ("# x · team").
                            if !self.selected_title.starts_with("# ") {
                                self.selected_title = name;
                            }
                        }
                    }
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
                    eprintln!("DBG ui: Messages APPLIED n={}", messages.len());
                    // Reconcile own pending sends: once the server shows the
                    // message (by clientmessageid) the bubble is real.
                    let cmids: std::collections::HashSet<&str> = messages
                        .iter()
                        .filter_map(|m| m.client_message_id.as_deref())
                        .collect();
                    self.pending_sends
                        .retain(|p| !(p.chat_id == chat_id && cmids.contains(p.cmid.as_str())));
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
                    // Who has read what (quiet fetch, no error surfaced).
                    self.cmd.send(Command::ReadReceipts(chat_id.clone())).ok();
                    // Freshly loaded tail: mark read — unless ghost mode
                    // holds read receipts back.
                    if !self.settings.ghost_mode
                        && let (Some(sel), Some(last)) = (
                            self.selected.clone(),
                            self.messages.last().map(|m| m.id.clone()),
                        )
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
                    // A channel opened before its team loaded: fix the title.
                    if let Some(sel) = self.selected.clone() {
                        let hit = self
                            .teams
                            .iter()
                            .flat_map(|t| t.channels.iter().map(move |ch| (t.name.clone(), ch.id.clone(), ch.name.clone())))
                            .find(|(_, id, _)| *id == sel);
                        if let Some((team, _, ch)) = hit {
                            self.selected_title = format!("# {ch} · {team}");
                        }
                    }
                }
                Event::CreateChatOk(chat) => {
                    let mut row = ost::api::ChatInfo {
                        id: chat.id.clone(),
                        name: chat.name.clone(),
                        is_group: chat.is_group,
                        last_message_time: None,
                        last_message_sender: None,
                        last_message_preview: None,
                        last_read_ms: None,
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
                    // Keep the url in pending_images: no infinite refetch
                    // loop for URLs that never decode.
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
                Event::Unread(map) => {
                    // Server-side refresh: merge, keeping locally-bumped
                    // entries only when the map lacks the chat.
                    for (id, v) in map {
                        self.unread.insert(id, v);
                    }
                }
                Event::ArchiveStats(chats, messages) => {
                    self.archive_stats = (chats, messages);
                }
                Event::MyPresence(availability) => {
                    self.presence = availability;
                }
                Event::SendFailed {
                    chat_id,
                    cmid,
                    error,
                } => match self
                    .pending_sends
                    .iter_mut()
                    .find(|p| p.cmid == cmid && p.chat_id == chat_id)
                {
                    Some(p) => p.error = Some(error),
                    None => self.error = Some(error),
                },
                Event::ReadReceipts { chat_id, receipts } => {
                    if self.selected.as_deref() == Some(chat_id.as_str()) {
                        self.receipts.clear();
                        for (user, mid) in receipts {
                            self.receipts
                                .entry(mid)
                                .or_default()
                                .push(user);
                        }
                    }
                }
                // ---- sections ----
                Event::Calendar(meetings) => {
                    // QA hook: TEAMSFAST_JOIN=1 auto-joins the first meeting
                    // that has a join link.
                    if std::env::var("TEAMSFAST_JOIN").as_deref() == Ok("1") {
                        if let Some(m) = meetings.iter().find(|m| m.join_url.is_some()) {
                            let url = m.join_url.clone().unwrap();
                            let label = Some(if m.subject.is_empty() {
                                "Meeting".to_string()
                            } else {
                                m.subject.clone()
                            });
                            self.call_label_hint = label;
                            self.cmd.send(Command::JoinMeeting(url)).ok();
                        }
                    }
                    self.meetings = meetings;
                    self.calendar_loading = false;
                }
                Event::Files(files) => {
                    self.drive_files = files;
                    self.files_loading = false;
                }
                Event::TodoLists(lists) => {
                    self.todo_lists = lists;
                    self.todo_loading = false;
                }
                Event::TodoTasks { list_id, tasks } => {
                    self.todo_tasks.insert(list_id, tasks);
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
        // Offline: retry the session every 20 s (network may be back).
        if self.offline.is_some() && self.last_offline_retry.elapsed() >= Duration::from_secs(20)
        {
            self.last_offline_retry = Instant::now();
            self.cmd.send(Command::CheckReady).ok();
        }

        // Presence poll every 60 s while signed in.
        if self.state == State::Ready
            && self.last_presence_poll.elapsed() >= Duration::from_secs(60)
        {
            self.last_presence_poll = Instant::now();
            self.cmd.send(Command::PollPresence).ok();
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
        self.sort_chats();
        let open = self.selected.as_deref() == Some(chat_id.as_str());
        if open {
            self.pending_open_refresh = true;
        } else {
            // Unread badge for the chat.
            let entry = self.unread.entry(chat_id.clone()).or_insert((false, 0));
            entry.0 = true;
            entry.1 = entry.1.saturating_add(1);
            // Activity feed: mentions first-class, other messages too.
            let chat_name = self
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
            let kind = if preview.contains('@') { "mention" } else { "message" };
            self.activity.insert(
                0,
                crate::ui::sections::ActivityEntry {
                    kind,
                    chat_id: chat_id.clone(),
                    chat_name,
                    who: sender.clone(),
                    preview: preview.clone(),
                    at_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                },
            );
            self.activity.truncate(200);
            let level = self
                .settings
                .notification_levels
                .get(&chat_id)
                .map(|s| s.as_str())
                .unwrap_or("all");
            let notify_ok = !self.settings.muted_chats.contains(&chat_id)
                && level != "off"
                && (level != "mentions" || preview.contains('@'));
            if notify_ok {
                self.notify_desktop(chat_id, sender, preview);
            }
        }
    }

    /// Desktop notification for an incoming message (respects settings).
    /// Clicking it opens that conversation and raises the window.
    fn notify_desktop(&self, chat_id: String, sender: String, preview: String) {
        if !self.settings.notify {
            return;
        }
        if let Some((from, to)) = &self.settings.quiet_hours {
            if in_quiet_hours(from, to) {
                return;
            }
        }
        if self.settings.skip_focused
            && !self.window_hidden
            && self
                .egui_ctx
                .as_ref()
                .map(|c| c.input(|i| i.focused))
                .unwrap_or(false)
        {
            return;
        }
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
        let body = if self.settings.notify_preview {
            format!("{sender}: {preview}")
        } else {
            sender
        };
        let click_tx = self.notif_tx.clone();
        std::thread::Builder::new()
            .name("notify".into())
            .spawn(move || {
                let shown = notify_rust::Notification::new()
                    .summary(&title)
                    .body(&body)
                    .action("default", "Open")
                    .timeout(6000)
                    .show();
                if let Ok(handle) = shown {
                    // Blocks until the notification is clicked or dismissed;
                    // "default" = body click on every freedesktop server.
                    handle.wait_for_action(|action| {
                        if action == "default" {
                            let _ = click_tx.send(chat_id);
                        }
                    });
                }
            })
            .ok();
    }

    /// Apply the selected theme file's palette (fallback: built-in dark or
    /// light, per settings).
    fn apply_selected_theme(&mut self) {
        let builtin = if self.settings.builtin == "light" {
            Palette::light()
        } else {
            Palette::dark()
        };
        self.palette = self
            .selected_theme
            .as_deref()
            .and_then(|name| self.catalog.find(name))
            .map(|t| t.palette.clone())
            .unwrap_or(builtin);
        if let Some(ctx) = self.egui_ctx.as_ref() {
            self.palette.apply_visuals(ctx);
        }
    }

    fn select_theme(&mut self, filename: String) {
        self.selected_theme = Some(filename.clone());
        self.settings.theme = Some(filename);
        theme::save_settings(&self.settings);
        self.apply_selected_theme();
    }

    /// Sort chats: pinned first, then by last activity (newest first).
    fn sort_chats(&mut self) {
        self.chats.sort_by(|a, b| {
            let pa = self.settings.pinned_chats.iter().any(|p| p == &a.id);
            let pb = self.settings.pinned_chats.iter().any(|p| p == &b.id);
            pb.cmp(&pa).then_with(|| {
                let k = |c: &ChatInfo| {
                    c.last_message_time
                        .as_deref()
                        .and_then(|t| t.trim().parse::<u64>().ok())
                        .unwrap_or(0)
                };
                k(b).cmp(&k(a))
            })
        });
    }

    fn rename_chat(&mut self, id: &str, name: String) {        if let Some(c) = self.chats.iter_mut().find(|c| c.id == id) {
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
        self.receipts.clear();
        self.older_link = None;
        self.edit = None;
        self.reply = None;
        // Opening the conversation reads it: badge off (the mark-read the
        // history load triggers persists the new horizon).
        self.unread.insert(id.clone(), (false, 0));
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
            .unwrap_or_default();
        if self.selected_title.is_empty() {
            // Channels live in the teams tree, not the chats list.
            let ch = self
                .teams
                .iter()
                .flat_map(|t| t.channels.iter().map(move |ch| (t.name.clone(), ch)))
                .find(|(_, ch)| ch.id == id);
            self.selected_title = match ch {
                Some((team, ch)) => format!("# {} · {}", ch.name, team),
                None => id.clone(),
            };
        }
        self.cmd.send(Command::OpenChat(id)).ok();
    }

    // ---------------------------------------------------------------- actions

    fn apply(&mut self, a: Action) {
        match a {
            Action::OpenChat(id) => self.open_chat(id),
            Action::Send(text) => {
                if let Some(chat) = self.selected.clone() {
                    let cmid = ost::api::new_client_message_id();
                    self.pending_sends.push(PendingSend {
                        cmid: cmid.clone(),
                        chat_id: chat.clone(),
                        text: text.clone(),
                        error: None,
                    });
                    self.cmd.send(Command::Send { chat_id: chat, text, cmid }).ok();
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
                if self.settings.ghost_mode {
                    return; // privacy: hold the receipt back
                }
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
                if let Some(path) = pick_file() {
                    if let Some(chat) = self.selected.clone() {
                        self.cmd
                            .send(Command::UploadFile { chat_id: chat, path })
                            .ok();
                    }
                } else {
                    self.status = "Attach cancelled or no file picker available".into();
                }
            }
            Action::FetchImage(url) => {
                self.cmd.send(Command::FetchImage(url)).ok();
            }
            Action::OpenImage(url) => {
                self.lightbox = textures_key(&url);
            }
            Action::ShowReactions { emoji, names } => {
                self.reaction_popup = Some((emoji, names));
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
            // ---- settings / account / storage ----
            Action::OpenSettings => self.settings_ui.open = true,
            Action::SetTheme(file) => match file {
                Some(f) => self.select_theme(f),
                None => {
                    self.selected_theme = None;
                    self.settings.theme = None;
                    self.settings.builtin = "dark".into();
                    theme::save_settings(&self.settings);
                    self.apply_selected_theme();
                }
            },
            Action::SetBuiltinLight(light) => {
                self.selected_theme = None;
                self.settings.theme = None;
                self.settings.builtin = if light { "light" } else { "dark" }.into();
                theme::save_settings(&self.settings);
                self.apply_selected_theme();
            }
            Action::SignIn => {
                self.error = None;
                self.status = "Sign-in started — see the terminal for the device code.".into();
                self.cmd.send(Command::StartLogin).ok();
            }
            Action::SignOut => {
                self.chats.clear();
                self.messages.clear();
                self.members.clear();
                self.teams.clear();
                self.unread.clear();
                self.selected = None;
                self.selected_title.clear();
                self.state = State::NeedLogin;
                self.status = "Signing out…".into();
                self.cmd.send(Command::SignOut).ok();
            }
            Action::ClearArchive => {
                self.chats.clear();
                self.messages.clear();
                self.unread.clear();
                self.archive_stats = (0, 0);
                self.status = "Local archive cleared".into();
                self.cmd.send(Command::ClearArchive).ok();
            }
            Action::OpenThemeFolder => open_folder(theme::themes_dir()),
            Action::OpenStateFolder => open_folder(theme::state_dir()),
            // ---- chat list row ops ----
            Action::TogglePin(id) => {
                if self.settings.pinned_chats.contains(&id) {
                    self.settings.pinned_chats.retain(|c| c != &id);
                } else {
                    self.settings.pinned_chats.push(id);
                }
                theme::save_settings(&self.settings);
                self.sort_chats();
            }
            Action::ToggleMute { chat_id, muted } => {
                if muted {
                    if !self.settings.muted_chats.contains(&chat_id) {
                        self.settings.muted_chats.push(chat_id.clone());
                    }
                } else {
                    self.settings.muted_chats.retain(|c| c != &chat_id);
                }
                theme::save_settings(&self.settings);
                self.cmd
                    .send(Command::SetChatMuted {
                        chat_id,
                        muted,
                    })
                    .ok();
            }
            Action::MarkUnread(id) => {
                self.unread.insert(id.clone(), (true, 0));
                self.cmd.send(Command::MarkUnread(id)).ok();
            }
            Action::HideChat(id) => {
                self.chats.retain(|c| c.id != id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.selected = None;
                    self.messages.clear();
                }
                self.cmd.send(Command::SetChatHidden(id)).ok();
            }
            Action::LeaveChat(id) => {
                self.chats.retain(|c| c.id != id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.selected = None;
                    self.messages.clear();
                }
                self.cmd.send(Command::LeaveChat(id)).ok();
            }
            // ---- message ops ----
            Action::Forward(text) => {
                self.forward = Some(ForwardState {
                    text,
                    filter: String::new(),
                });
            }
            Action::TogglePinMessage {
                chat_id,
                message_id,
            } => {
                let pins = self
                    .settings
                    .pinned_messages
                    .entry(chat_id)
                    .or_default();
                if let Some(pos) = pins.iter().position(|m| m == &message_id) {
                    pins.remove(pos);
                } else {
                    pins.push(message_id);
                }
                theme::save_settings(&self.settings);
            }
            Action::RetrySend(cmid) => {
                if let Some(p) = self.pending_sends.iter().find(|p| p.cmid == cmid) {
                    let cmd = Command::Send {
                        chat_id: p.chat_id.clone(),
                        text: p.text.clone(),
                        cmid: p.cmid.clone(),
                    };
                    if let Some(p) = self
                        .pending_sends
                        .iter_mut()
                        .find(|p| p.cmid == cmid)
                    {
                        p.error = None;
                    }
                    self.cmd.send(cmd).ok();
                }
            }
            Action::DismissSend(cmid) => {
                self.pending_sends.retain(|p| p.cmid != cmid);
            }
            Action::JumpLatest => {
                if let Some(chat) = self.selected.clone() {
                    self.open_chat(chat);
                }
            }
            // ---- sections ----
            Action::ReloadSection => self.reload_section(),
            Action::OpenLink(url) => {
                if let Err(e) = open::that_detached(&url) {
                    self.error = Some(format!("open link: {e:#}"));
                }
            }
            Action::CopyText(text) => {
                if let Some(ctx) = self.egui_ctx.as_ref() {
                    ctx.copy_text(text);
                }
            }
            Action::DownloadDriveFile {
                drive_id,
                item_id,
                name,
            } => {
                self.cmd
                    .send(Command::DownloadDriveFile {
                        drive_id,
                        item_id,
                        name,
                    })
                    .ok();
            }
            Action::OpenTodoList(list_id) => {
                if !self.todo_tasks.contains_key(&list_id) {
                    self.cmd.send(Command::LoadTodoTasks(list_id)).ok();
                }
            }
            Action::AddTodoTask { list_id, title } => {
                self.cmd
                    .send(Command::AddTodoTask { list_id, title })
                    .ok();
            }
            Action::SetTodoDone {
                list_id,
                task_id,
                done,
            } => {
                self.cmd
                    .send(Command::SetTodoDone {
                        list_id,
                        task_id,
                        done,
                    })
                    .ok();
            }
            Action::ClearActivity => self.activity.clear(),
            // ---- calls ----
            Action::StartCall(chat_id) => {
                self.cmd.send(Command::StartCall(chat_id)).ok();
            }
            Action::StartVideoCall(chat_id) => {
                self.cmd.send(Command::StartVideoCall(chat_id)).ok();
            }
            Action::TestCall => {
                self.cmd.send(Command::TestCall { video: false }).ok();
            }
            Action::AcceptIncoming => {
                if let Some(ring) = self.incoming.take() {
                    self.cmd.send(Command::AcceptIncoming(ring.raw)).ok();
                }
            }
            Action::DeclineIncoming => {
                if let Some(ring) = self.incoming.take() {
                    self.cmd.send(Command::DeclineIncoming(ring.raw)).ok();
                }
            }
            Action::JoinMeeting { source, label } => {
                self.join_open = false;
                self.status = "Resolving meeting…".into();
                self.call_label_hint = label;
                self.cmd.send(Command::JoinMeeting(source)).ok();
            }
            Action::ShowJoinDialog => self.join_open = true,
            Action::ShowContact { mri, name } => {
                self.contact = Some((mri, name));
            }
            Action::RenameChannel {
                team_id,
                channel_id,
                name,
            } => {
                self.channel_rename = Some((team_id, channel_id, name));
            }
            Action::DeleteChannel { team_id, channel_id } => {
                self.status = "deleting channel…".into();
                self.cmd
                    .send(Command::DeleteChannel { team_id, channel_id })
                    .ok();
                self.loading_teams = true;
                self.cmd.send(Command::LoadTeams).ok();
            }
            Action::ReadNotePage(page_id) => {
                self.notes_page = None;
                self.cmd.send(Command::ReadNotePage { page_id }).ok();
            }
            Action::AppendNote { page_id, text } => {
                self.cmd
                    .send(Command::AppendNote { page_id, text })
                    .ok();
            }
            Action::MeetNow => {
                self.status = "creating meeting…".into();
                self.cmd.send(Command::MeetNow).ok();
            }
            Action::ShowChatFiles(chat_id) => {
                self.switch_view(MainView::Files);
                self.chat_files_loading = true;
                self.cmd.send(Command::ChatFiles(chat_id)).ok();
            }
            Action::SetPlannerDone {
                task_id,
                etag,
                done,
            } => {
                self.planner_loading = true;
                self.cmd
                    .send(Command::SetPlannerDone {
                        task_id,
                        etag,
                        done,
                    })
                    .ok();
            }
            Action::ChatWith { mri, name } => {
                // 1:1 thread id is deterministic from both MRIs.
                let Some(me) = self.self_id.clone() else {
                    self.status = "Still loading your profile…".into();
                    return;
                };
                let thread =
                    ost::api::one_to_one_thread_id(&format!("8:orgid:{me}"), &mri);
                self.contact = None;
                if !self.chats.iter().any(|c| c.id == thread) {
                    // Add a placeholder row so the title resolves instantly.
                    self.chats.insert(
                        0,
                        ChatInfo {
                            id: thread.clone(),
                            name: name.clone(),
                            is_group: false,
                            last_message_time: None,
                            last_message_sender: None,
                            last_message_preview: None,
                        last_read_ms: None,
                        },
                    );
                }
                self.open_chat(thread);
            }
            Action::ShowTeamDialog => {
                self.team_dialog.open = true;
                if self.team_dialog.channel_team.is_none() {
                    self.team_dialog.channel_team =
                        self.teams.first().map(|t| t.id.clone());
                }
            }
            Action::CreateChannel { team_id, name } => {
                self.status = format!("creating #{name}…");
                self.cmd
                    .send(Command::CreateChannel {
                        team_id,
                        name: name.clone(),
                    })
                    .ok();
                self.team_dialog.open = false;
                self.loading_teams = true;
                self.cmd.send(Command::LoadTeams).ok();
            }
            Action::SearchPublicTeams(query) => {
                self.team_dialog.searching = true;
                self.cmd.send(Command::SearchPublicTeams(query)).ok();
            }
            Action::JoinTeam { team_id, name } => {
                self.status = format!("joining {name}…");
                self.cmd.send(Command::JoinTeam(team_id)).ok();
                self.loading_teams = true;
                self.cmd.send(Command::LoadTeams).ok();
            }
            Action::CreateTeam(name) => {
                self.status = format!("creating team {name}…");
                self.cmd.send(Command::CreateTeam(name)).ok();
                self.team_dialog.open = false;
                self.loading_teams = true;
                self.cmd.send(Command::LoadTeams).ok();
            }
            Action::SetNotifyLevel { chat_id, level } => {
                if level == "all" {
                    self.settings.notification_levels.remove(&chat_id);
                } else {
                    self.settings
                        .notification_levels
                        .insert(chat_id, level);
                }
                theme::save_settings(&self.settings);
            }
            Action::HangUp => {
                self.cmd.send(Command::HangUp).ok();
            }
        }
    }

    /// Switch the active section (loads its data on first visit).
    fn switch_view(&mut self, view: MainView) {
        self.main_view = view;
        self.reload_section();
    }

    /// (Re)fetch the visible section's data.
    fn reload_section(&mut self) {
        match self.main_view {
            MainView::Chat => {}
            MainView::Teams => {
                if self.teams.is_empty() {
                    self.loading_teams = true;
                    self.cmd.send(Command::LoadTeams).ok();
                }
            }
            MainView::Calendar => {
                self.calendar_loading = true;
                self.cmd.send(Command::LoadCalendar).ok();
            }
            MainView::Files => {
                self.files_loading = true;
                self.cmd.send(Command::LoadFiles).ok();
            }
            MainView::ToDo => {
                self.todo_loading = true;
                self.cmd.send(Command::LoadTodo).ok();
            }
            MainView::Planner => {
                self.planner_loading = true;
                self.cmd.send(Command::LoadPlanner).ok();
            }
            MainView::Shifts => {
                self.shifts_loading = true;
                self.cmd.send(Command::LoadShifts).ok();
            }
            MainView::Notes => {
                self.notes_loading = true;
                self.cmd.send(Command::LoadNotes).ok();
            }
            MainView::Activity => {}
        }
    }

    /// Update flow: one check per launch, event drain, dialog, receipt.
    fn update_logic(&mut self, ui: &mut egui::Ui) {
        // Acknowledge a helper-installed relaunch once the window is up.
        if !self.update.receipt_acknowledged
            && let Some(receipt) = self.update_receipt.take()
        {
            self.update.receipt_acknowledged = true;
            if let Err(e) = receipt.acknowledge() {
                log::warn!("update receipt: {e:#}");
            }
            self.status = format!("Updated to {} ✓", crate::updates::CONFIG.current_version);
        }
        if let Some(err) = self.launch_error.take() {
            self.error = Some(format!("update: {err}"));
        }
        // One check per launch, when enabled.
        if !self.update.checked {
            self.update.checked = true;
            if self.settings.update_checks {
                let (tx_ev, rx_ev) = std::sync::mpsc::channel::<UpdateEvent>();
                let (tx_cmd, rx_cmd) = std::sync::mpsc::channel::<UpdateCommand>();
                spawn_update_worker(tx_ev, rx_cmd);
                self.update.events = Some(rx_ev);
                self.update.cmd_tx = Some(tx_cmd);
            }
        }
        // Drain events.
        let mut restart_now = false;
        if let Some(events) = &self.update.events {
            while let Ok(ev) = events.try_recv() {
                match ev {
                    UpdateEvent::Checking => {}
                    UpdateEvent::Available { version, url } => {
                        self.update.dialog = Some(UpdateDialog {
                            kind: UpdateDialogKind::Progress(0),
                            version,
                            url,
                        });
                    }
                    UpdateEvent::Progress(pct) => {
                        if let Some(d) = &mut self.update.dialog {
                            d.kind = UpdateDialogKind::Progress(pct);
                        }
                    }
                    UpdateEvent::Ready { version } => {
                        if let Some(d) = &mut self.update.dialog {
                            d.kind = UpdateDialogKind::Ready;
                            let _ = &version;
                        }
                        // QA hook: TEAMSFAST_AUTOUPDATE=1 restarts without
                        // waiting for the button (headless update tests).
                        if std::env::var("TEAMSFAST_AUTOUPDATE").as_deref() == Ok("1") {
                            if let Some(tx) = &self.update.cmd_tx {
                                let _ = tx.send(UpdateCommand::Restart);
                            }
                            self.update.dialog = None;
                            self.status = "installing update…".into();
                        }
                    }
                    UpdateEvent::Blocked(msg) => {
                        if let Some(d) = &mut self.update.dialog {
                            // Download/verify failures read as errors;
                            // install-detection refusals as informational.
                            d.kind = UpdateDialogKind::Unsupported(msg);
                        }
                    }
                    UpdateEvent::Restarting => restart_now = true,
                }
            }
        }
        if restart_now {
            // The helper waits for us to exit.
            self.quitting = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Dialog window.
        if let Some(dialog) = self.update.dialog.as_ref() {
            let version = dialog.version.clone();
            let url = dialog.url.clone();
            let open = &mut true;
            let mut cmd: Option<UpdateCommand> = None;
            egui::Window::new("Update available")
                .open(open)
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .show(ui.ctx(), |ui| match &dialog.kind {
                    UpdateDialogKind::Progress(pct) => {
                        ui.label(format!("TeamsFast {version} is downloading…"));
                        ui.add(egui::ProgressBar::new(*pct as f32 / 100.0).show_percentage());
                    }
                    UpdateDialogKind::Ready => {
                        ui.label(format!("TeamsFast {version} is ready to install."));
                        ui.label(
                            RichText::new("Restart to finish; the previous version is restored if anything fails.")
                                .small()
                                .weak(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Restart to update")
                                            .strong()
                                            .color(self.palette.on_accent),
                                    )
                                    .fill(self.palette.accent)
                                    .min_size(egui::vec2(120.0, 24.0)),
                                )
                                .clicked()
                            {
                                cmd = Some(UpdateCommand::Restart);
                            }
                        });
                    }
                    UpdateDialogKind::Failed(msg) => {
                        ui.label(format!("Update to {version} failed:"));
                        ui.label(RichText::new(msg).small().color(self.palette.danger));
                    }
                    UpdateDialogKind::Unsupported(msg) => {
                        ui.label(format!("TeamsFast {version} is available."));
                        ui.label(RichText::new(msg).small().weak());
                        if ui.hyperlink_to("Open the release page", &url).clicked() {
                            let _ = open::that_detached(&url);
                        }
                    }
                });
            if !*open {
                cmd = Some(UpdateCommand::Dismiss);
            }
            match cmd {
                Some(UpdateCommand::Restart) => {
                    self.update.dialog = None;
                    self.status = "installing update…".into();
                    // The worker hands off; the Restarting event quits us.
                }
                Some(UpdateCommand::Dismiss) => {
                    self.update.dialog = None;
                }
                None => {}
            }
        }
    }

    // ---------------------------------------------------------------- layout

    /// Left icon rail: section switcher (Chat/Teams/Calendar/Files/ToDo/
    /// Activity) with the settings gear pinned at the bottom.
    fn rail(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let sections: [(MainView, &'static str, crate::theme::Icon, &str); 9] = [
            (MainView::Chat, "Chat", crate::theme::Icon::MessageSquare, "Chats (Ctrl+1)"),
            (MainView::Teams, "Teams", crate::theme::Icon::Users, "Teams & channels (Ctrl+2)"),
            (MainView::Calendar, "Calendar", crate::theme::Icon::Calendar, "Calendar (Ctrl+3)"),
            (MainView::Files, "Files", crate::theme::Icon::FileText, "Files (Ctrl+4)"),
            (MainView::ToDo, "To Do", crate::theme::Icon::SquareCheck, "To Do (Ctrl+5)"),
            (MainView::Planner, "Planner", crate::theme::Icon::ListTodo, "Planner boards (Ctrl+6)"),
            (MainView::Shifts, "Shifts", crate::theme::Icon::Clock, "Shifts this week (Ctrl+7)"),
            (MainView::Notes, "OneNote", crate::theme::Icon::Archive, "OneNote notebooks (Ctrl+8)"),
            (MainView::Activity, "Activity", crate::theme::Icon::Activity, "Activity (Ctrl+9)"),
        ];
        for (view, _name, icon, tip) in sections {
            let sel = self.main_view == view;
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
            if sel {
                ui.painter().rect_filled(rect, 8, {
                    let [r, g, b, _] = self.palette.accent.to_srgba_unmultiplied();
                    Color32::from_rgba_unmultiplied(r, g, b, 0x50)
                });
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.left_top(),
                        egui::vec2(3.0, rect.height()),
                    ),
                    2.0,
                    self.palette.accent,
                );
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 8, self.palette.surface_hover);
            }
            let tint = if sel { self.palette.text } else { self.palette.secondary };
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child.vertical_centered(|ui| {
                ui.add_space(9.0);
                ui.add(
                    egui::Image::from_bytes(icon.uri(), icon.bytes())
                        .tint(tint)
                        .fit_to_exact_size(egui::Vec2::splat(18.0)),
                );
            });
            let resp = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tip);
            if resp.clicked() {
                self.switch_view(view);
            }
            // Unread indicator on the Chat icon.
            if view == MainView::Chat {
                let total: u32 = self
                    .unread
                    .values()
                    .map(|(u, _)| u32::from(*u))
                    .sum();
                if total > 0 {
                    let c = rect.right_top() + egui::vec2(-4.0, 4.0);
                    ui.painter().circle_filled(c, 6.0, self.palette.accent);
                }
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            let gear = ui.add(
                egui::Button::new(
                    crate::theme::Icon::Settings
                        .image(self.palette.secondary, 18.0),
                )
                .fill(Color32::TRANSPARENT),
            )
            .on_hover_text("Settings (Ctrl+,)");
            if gear.clicked() {
                self.settings_ui.open = true;
            }
        });
    }

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
                        .circle_filled(rect.center(), 4.0, self.palette.ok);
                    ui.add_space(2.0);
                    let refresh = ui
                        .add(egui::Button::new(
                            crate::theme::Icon::Refresh
                                .image(self.palette.text, 16.0),
                        ))
                        .on_hover_text("Refresh chats");
                    if refresh.clicked() {
                        self.apply(Action::Refresh);
                    }
                    let new_chat = ui
                        .add(egui::Button::new(
                            crate::theme::Icon::Plus.image(self.palette.text, 16.0),
                        ))
                        .on_hover_text("New chat (Ctrl+N)");
                    if new_chat.clicked() {
                        self.apply(Action::ShowNewChat);
                    }
                    let search = ui
                        .add(egui::Button::new(
                            crate::theme::Icon::Search
                                .image(self.palette.text, 16.0),
                        ))
                        .on_hover_text("Search messages (Ctrl+F)");
                    if search.clicked() {
                        self.apply(Action::OpenSearch);
                    }
                    let gear = ui
                        .add(egui::Button::new(
                            crate::theme::Icon::Settings
                                .image(self.palette.text, 16.0),
                        ))
                        .on_hover_text("Settings (Ctrl+,)");
                    if gear.clicked() {
                        self.settings_ui.open = true;
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
                    // Name chip: presence dot + name; click = status menu.
                    let dot = crate::model::presence_color(&self.presence);
                    let label = if self.presence.is_empty() {
                        self.self_name.clone()
                    } else {
                        format!("● {}", self.self_name)
                    };
                    ui.menu_button(
                        RichText::new(label).small().color(self.palette.text),
                        |ui| {
                            ui.label(RichText::new("Set status").small().weak());
                            ui.separator();
                            // Coloured dot + label (popup emoji renders
                            // monochrome on this stack — see gotchas).
                            const STATUSES: [(&str, &str, bool); 6] = [
                                ("available", "Available", false),
                                ("brb", "Be right back", false),
                                ("busy", "Busy", true),
                                ("dnd", "Do not disturb", true),
                                ("away", "Away", false),
                                ("offline", "Appear offline", false),
                            ];
                            for (key, text, red) in STATUSES {
                                let wire = match key {
                                    "available" => "Available",
                                    "brb" => "BeRightBack",
                                    "busy" => "Busy",
                                    "dnd" => "DoNotDisturb",
                                    "away" => "Away",
                                    _ => "Offline",
                                };
                                let sel =
                                    self.presence.eq_ignore_ascii_case(wire);
                                let color = if red {
                                    self.palette.danger
                                } else {
                                    match key {
                                        "offline" => self.palette.dim,
                                        "away" | "brb" => self.palette.warning,
                                        _ => self.palette.ok,
                                    }
                                };
                                if ui
                                    .selectable_label(
                                        sel,
                                        RichText::new(format!("●  {text}"))
                                            .small()
                                            .color(color),
                                    )
                                    .clicked()
                                {
                                    self.cmd
                                        .send(Command::SetPresence(key.to_string()))
                                        .ok();
                                    ui.close();
                                }
                            }
                        },
                    )
                    .response
                    .on_hover_text(format!(
                        "Status: {}",
                        if self.presence.is_empty() { "unknown" } else { &self.presence }
                    ));
                    let _ = dot;
                });
            }
        });
        if let Some(err) = &self.error {
            ui.colored_label(self.palette.danger, RichText::new(err).small());
        }
    }
}

impl eframe::App for TeamsFastApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.sync_tray(ui);
        self.update_logic(ui);

        // Start-in-tray: hide on the first rendered frame.
        if self.pending_hide {
            self.pending_hide = false;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_hidden = true;
        }

        // Close-to-tray: cancel the close and hide instead — but never
        // when a real quit is in flight, and never when there is no tray
        // to hide to (the ✕ would become unquittable).
        if ui.input(|i| i.viewport().close_requested())
            && self.settings.close_to_tray
            && !self.quitting
            && self.tray.is_some()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_hidden = true;
        }

        // Notification clicks: open that conversation and raise the window.
        let mut clicked: Option<String> = None;
        while let Ok(id) = self.notif_clicks.try_recv() {
            clicked = Some(id);
        }
        if let Some(id) = clicked {
            self.apply(Action::OpenChat(id));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            self.window_hidden = false;
        }

        // Theme catalog: poll for scans/edits; re-apply the selected palette.
        if self.catalog.poll() {
            self.apply_selected_theme();
        }

        // Pending theme from startup settings (once the catalog is ready).
        if let Some(name) = self.pending_theme.take() {
            if self.catalog.themes().iter().any(|t| t.filename == name) {
                self.selected_theme = Some(name.clone());
                self.apply_selected_theme();
            } else {
                self.pending_theme = Some(name);
            }
        }

        egui::Panel::top("top").show(ui, |ui| {
            self.top_bar(ui);
        });

        // Section rail (icon strip, far left).
        if self.offline.is_some() {
            egui::Panel::top("offline")
                .frame(
                    egui::Frame::default()
                        .fill(
                            Color32::from_rgba_unmultiplied(
                                self.palette.warning.r(),
                                self.palette.warning.g(),
                                self.palette.warning.b(),
                                36,
                            ),
                        )
                        .stroke(egui::Stroke::new(
                            1.0,
                            Color32::from_rgba_unmultiplied(
                                self.palette.warning.r(),
                                self.palette.warning.g(),
                                self.palette.warning.b(),
                                120,
                            ),
                        )),
                )
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("⚠").color(self.palette.warning));
                        ui.label(
                            RichText::new(
                                "Offline — showing cached data. Messages you send now can't be delivered.",
                            )
                            .small(),
                        );
                        if ui.small_button("Retry now").clicked() {
                            self.last_offline_retry = Instant::now();
                            self.cmd.send(Command::CheckReady).ok();
                        }
                    });
                });
        }

        if let Some(ring) = self.incoming.clone() {
            // Caller-cancel pushes are not capture-verified yet, so the
            // banner self-expires past the ring window.
            if ring.at.elapsed() > RING_TIMEOUT {
                self.incoming = None;
            } else {
                let busy = self.call.is_some();
                let palette = &self.palette;
                let mut actions: Vec<Action> = Vec::new();
                // Pulse the border so the ring is noticeable even when the
                // user is reading chats elsewhere.
                let pulse = ((ui.input(|i| i.time) * 3.0).sin() * 0.5 + 0.5) as f32;
                let ring_alpha = (110.0 + 130.0 * pulse) as u8;
                egui::Panel::top("ringing")
                    .frame(
                        egui::Frame::default()
                            .fill(Color32::from_rgba_unmultiplied(
                                self.palette.danger.r(),
                                self.palette.danger.g(),
                                self.palette.danger.b(),
                                44,
                            ))
                            .stroke(egui::Stroke::new(
                                1.6,
                                Color32::from_rgba_unmultiplied(
                                    self.palette.danger.r(),
                                    self.palette.danger.g(),
                                    self.palette.danger.b(),
                                    ring_alpha,
                                ),
                            )),
                    )
                    .show_inside(ui, |ui| {
                        let mut accept = false;
                        let mut decline = false;
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("📞").size(15.0));
                            let video = if ring.has_video { " · video" } else { "" };
                            let ring_secs = ring.at.elapsed().as_secs();
                            ui.label(
                                RichText::new(format!(
                                    "{} — incoming call{} · {}s",
                                    ring.name, video, ring_secs
                                ))
                                .strong(),
                            );
                            // Actions pinned right, comfortable targets.
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Decline")
                                                    .small()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(palette.danger)
                                            .min_size(egui::vec2(96.0, 30.0)),
                                        )
                                        .clicked()
                                    {
                                        decline = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            egui::Button::new(
                                                RichText::new("Accept")
                                                    .small()
                                                    .strong()
                                                    .color(Color32::from_rgb(0x0a, 0x24, 0x16)),
                                            )
                                            .fill(palette.ok)
                                            .min_size(egui::vec2(96.0, 30.0)),
                                        )
                                        .clicked()
                                    {
                                        accept = true;
                                    }
                                    if busy {
                                        ui.label(
                                            RichText::new("busy — accept disabled")
                                                .weak()
                                                .small(),
                                        );
                                    }
                                },
                            );
                        });
                        if accept {
                            actions.push(Action::AcceptIncoming);
                        }
                        if decline {
                            actions.push(Action::DeclineIncoming);
                        }
                    });
                for a in actions {
                    self.apply(a);
                }
            }
        }

        if let Some((label, at)) = self.call.clone() {
            let secs = at.elapsed().as_secs();
            let mm = secs / 60;
            let ss = secs % 60;
            let mut toggle_stage = false;
            egui::Panel::top("call")
                .frame(
                    egui::Frame::default()
                        .fill(Color32::from_rgba_unmultiplied(
                            self.palette.ok.r(),
                            self.palette.ok.g(),
                            self.palette.ok.b(),
                            30,
                        ))
                        .stroke(egui::Stroke::new(
                            1.0,
                            Color32::from_rgba_unmultiplied(
                                self.palette.ok.r(),
                                self.palette.ok.g(),
                                self.palette.ok.b(),
                                120,
                            ),
                        )),
                )
                .show_inside(ui, |ui| {
                    let phase = self
                        .call_media
                        .as_ref()
                        .map(|m| m.phase.borrow().to_string())
                        .unwrap_or_else(|| "calling…".into());
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("●").color(self.palette.ok).size(13.0));
                        ui.label(
                            RichText::new(format!("{label} — {phase}  {mm:02}:{ss:02}")).small(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Hang up").small().color(Color32::WHITE),
                                    )
                                    .fill(self.palette.danger)
                                    .min_size(egui::vec2(84.0, 26.0)),
                                )
                                .clicked()
                            {
                                self.cmd.send(Command::HangUp).ok();
                            }
                            if self.call_media.is_some()
                                && ui
                                    .small_button(if self.call_view_open {
                                        "Hide call view"
                                    } else {
                                        "Show call view"
                                    })
                                    .clicked()
                            {
                                toggle_stage = true;
                            }
                        });
                    });
                });
            if toggle_stage {
                self.call_view_open = !self.call_view_open;
            }
        }


        egui::Panel::left("rail")
            .default_size(46.0)
            .resizable(false)
            .show(ui, |ui| {
                self.rail(ui);
            });

        // Chat-family sections keep the list sidebar; the others take the
        // whole central area.
        let chat_family = matches!(self.main_view, MainView::Chat | MainView::Teams);
        if chat_family && self.state == State::Ready {
            egui::Panel::left("side")
                .default_size(300.0)
                .resizable(true)
                .show(ui, |ui| {
                    let mut sctx = crate::ui::sidebar::SidebarCtx {
                        chats: &self.chats,
                        selected: self.selected.as_ref(),
                        teams: &self.teams,
                        view: match self.main_view {
                            MainView::Teams => SideView::Teams,
                            _ => SideView::Chats,
                        },
                        cmd: &self.cmd,
                        pal: &self.palette,
                        unread: &self.unread,
                        show_badges: self.settings.unread_badges,
                        pinned: &self
                            .settings
                            .pinned_chats
                            .iter()
                            .cloned()
                            .collect::<HashSet<String>>(),
                        muted: &self
                            .settings
                            .muted_chats
                            .iter()
                            .cloned()
                            .collect::<HashSet<String>>(),
                    notify_levels: &self.settings.notification_levels,
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
        }

        // ---- call stage (right panel): video tiles + live controls ----
        if self.call_view_open
            && self.call.is_some()
            && self.call_media.is_some()
        {
            self.update_call_stage_frames(ui.ctx());
        }
        if self.call_view_open {
            if let (Some((label, at)), Some(media)) = (self.call.clone(), self.call_media.as_ref())
            {
                let secs = at.elapsed().as_secs();
                let (mm, ss) = (secs / 60, secs % 60);
                let phase = media.phase.borrow().to_string();
                egui::Panel::right("call_stage")
                    .default_size(340.0)
                    .resizable(false)
                    .show_inside(ui, |ui| {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("●").color(self.palette.ok).size(12.0));
                            // Name + full state already show in the banner;
                            // the stage header carries phase + timer only.
                            ui.label(
                                RichText::new(format!("{phase}  {mm:02}:{ss:02}")).strong(),
                            );
                        });
                        ui.add_space(6.0);
                        let avail_w = (ui.available_width() - 16.0).max(120.0);
                        // Remote tile (or placeholder until frames arrive),
                        // framed so bare video doesn't sit raw on the panel.
                        let tile_h = 300.0_f32;
                        match self.remote_video_tex.as_ref() {
                            Some(tex) => {
                                let size = tex.size_vec2();
                                let h = (avail_w * size.y / size.x).min(tile_h);
                                let resp = ui.add_sized(
                                    [avail_w, h],
                                    egui::Image::new((tex.id(), size)),
                                );
                                ui.painter().rect_stroke(
                                    resp.rect,
                                    8,
                                    egui::Stroke::new(1.0, Color32::from_black_alpha(90)),
                                    egui::StrokeKind::Inside,
                                );
                                ui.add_space(2.0);
                                ui.label(RichText::new(&label).weak().small());
                            }
                            None => {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(avail_w, tile_h * 0.72),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 8.0, Color32::from_black_alpha(110));
                                let initial = label
                                    .chars()
                                    .next()
                                    .map(|c| c.to_uppercase().to_string())
                                    .unwrap_or_else(|| "?".into());
                                ui.painter().text(
                                    rect.center() - egui::vec2(0.0, 12.0),
                                    egui::Align2::CENTER_CENTER,
                                    initial,
                                    egui::FontId::proportional(30.0),
                                    self.palette.secondary,
                                );
                                ui.painter().text(
                                    rect.center() + egui::vec2(0.0, 18.0),
                                    egui::Align2::CENTER_CENTER,
                                    "No video yet",
                                    egui::FontId::proportional(12.0),
                                    self.palette.secondary,
                                );
                            }
                        }
                        // Local camera preview (video calls only), in flow
                        // right under the remote tile.
                        if media.local_preview.is_some() {
                            ui.add_space(4.0);
                            match self.local_video_tex.as_ref() {
                                Some(tex) => {
                                    let size = tex.size_vec2();
                                    let w = 150.0_f32.min(avail_w);
                                    let h = w * size.y / size.x;
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(w, h),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().image(
                                        tex.id(),
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        6,
                                        egui::Stroke::new(1.0, Color32::from_black_alpha(90)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                None => {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(150.0, 112.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        6.0,
                                        Color32::from_black_alpha(80),
                                    );
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "camera…",
                                        egui::FontId::proportional(11.0),
                                        self.palette.secondary,
                                    );
                                }
                            }
                        }
                        // Bottom strip: controls anchored to the panel's
                        // bottom-right (explicit rect — egui cross-align
                        // inside bottom_up would leave them flush-left).
                        let ctrl_h = 46.0;
                        let ctrl_rect = egui::Rect::from_min_max(
                            egui::pos2(ui.max_rect().left(), ui.max_rect().bottom() - ctrl_h),
                            egui::pos2(ui.max_rect().right(), ui.max_rect().bottom() - 10.0),
                        );
                        let mut ctrl_ui = ui.new_child(
                            egui::UiBuilder::new().max_rect(ctrl_rect),
                        );
                        ctrl_ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Hang up")
                                                    .small()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(self.palette.danger)
                                            .min_size(egui::vec2(84.0, 30.0)),
                                        )
                                        .clicked()
                                    {
                                        self.cmd.send(Command::HangUp).ok();
                                    }
                                    if media.local_preview.is_some() {
                                        let cam_on = *media.camera_on.borrow();
                                        let cam = ui.add(
                                            egui::Button::new(
                                                crate::theme::Icon::Video
                                                    .image(self.palette.secondary, 15.0),
                                            )
                                            .fill(Color32::from_rgba_unmultiplied(
                                                255, 255, 255, 16,
                                            ))
                                            .min_size(egui::vec2(36.0, 30.0)),
                                        );
                                        if cam
                                            .on_hover_text(if cam_on {
                                                "Camera off"
                                            } else {
                                                "Camera on"
                                            })
                                            .clicked()
                                        {
                                            let _ = media.camera_on.send(!cam_on);
                                        }
                                    }
                                    let mic_on = *media.mic_on.borrow();
                                    let mic = ui.add(
                                        egui::Button::new(
                                            crate::theme::Icon::Mic
                                                .image(self.palette.secondary, 15.0),
                                        )
                                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 16))
                                        .min_size(egui::vec2(36.0, 30.0)),
                                    );
                                    if mic
                                        .on_hover_text(if mic_on { "Mute" } else { "Unmute" })
                                        .clicked()
                                    {
                                        let _ = media.mic_on.send(!mic_on);
                                    }
                            },
                        );
                    });
                // Live tiles + the timer need a steady repaint.
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
            }
        }

        if self.search_open {
            egui::Panel::right("search")
                .default_size(330.0)
                .resizable(true)
                .show(ui, |ui| {
                    let mut actions: Vec<Action> = Vec::new();
                    search_panel(
                        ui,
                        &self.search_hits,
                        self.search_more,
                        self.searching,
                        &self.cmd,
                        &self.search_query,
                        self.search_note.as_deref(),
                        &mut actions,
                    );
                    for a in actions {
                        self.apply(a);
                    }
                });
        }

        if chat_family && self.state == State::Ready && self.selected.is_some() {
            let chat_id = self.selected.clone().unwrap();
            let mut actions: Vec<Action> = Vec::new();
            let pending_view: Vec<conversation::PendingBubble> = self
                .pending_sends
                .iter()
                .filter(|p| p.chat_id == chat_id)
                .map(|p| conversation::PendingBubble {
                    cmid: p.cmid.clone(),
                    text: p.text.clone(),
                    error: p.error.clone(),
                })
                .collect();
            let pinned_here = self
                .settings
                .pinned_messages
                .get(&chat_id)
                .cloned()
                .unwrap_or_default();
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
                pending: &pending_view,
                receipts: &self.receipts,
                pinned: &pinned_here,
                can_call: {
                    let id = chat_id.as_str();
                    id.contains("@unq.gbl.spaces")
                        && !chat_id.starts_with("19:meeting_")
                },
                call_label: self.call.as_ref().map(|(l, _)| l.as_str()),
                textures: &self.textures,
                pending_images: &mut self.pending_images,
                emoji_textures: &mut self.emoji_textures,
                pal: &self.palette,
                actions: &mut actions,
            };
            egui::Panel::bottom("composer").show_inside(ui, |ui| {
                conversation::conversation_composer(ui, &mut ctx, &mut self.draft);
            });
            egui::CentralPanel::default().show_inside(ui, |ui| {
                conversation::conversation_messages(ui, &mut ctx);
            });
            drop(ctx);
            for a in actions {
                self.apply(a);
            }
        } else if chat_family {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(24.0);
                        match self.state {
                            State::Boot => {
                                ui.spinner();
                                ui.label(RichText::new("Starting…").weak());
                            }
                            State::NeedLogin => {
                                ui.label(
                                    RichText::new("Welcome to TeamsFast")
                                        .strong()
                                        .size(18.0),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Sign in with your Microsoft account to start.",
                                    )
                                    .weak(),
                                );
                                ui.add_space(10.0);
                                if self.login_code.is_none() {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Sign in").strong(),
                                            )
                                            .min_size(egui::vec2(120.0, 30.0)),
                                        )
                                        .clicked()
                                    {
                                        self.error = None;
                                        self.status = "Requesting a sign-in code…".into();
                                        self.cmd.send(Command::StartLogin).ok();
                                    }
                                    if let Some(err) = &self.error {
                                        ui.label(
                                            RichText::new(err)
                                                .small()
                                                .color(self.palette.danger),
                                        );
                                    }
                                } else {
                                    let (url, code) = self.login_code.clone().unwrap();
                                    ui.label(RichText::new("1. Open this page:").small());
                                    ui.horizontal(|ui| {
                                        ui.monospace(
                                            RichText::new(&url).small().color(self.palette.link),
                                        );
                                        if ui.small_button("Open").clicked() {
                                            let _ = open::that_detached(&url);
                                        }
                                    });
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("2. Enter this code:").small());
                                    ui.horizontal(|ui| {
                                        ui.monospace(
                                            RichText::new(&code)
                                                .size(22.0)
                                                .strong()
                                                .color(self.palette.text),
                                        );
                                        if ui.small_button("Copy").clicked() {
                                            if let Some(ctx) = self.egui_ctx.as_ref() {
                                                ctx.copy_text(code.clone());
                                            }
                                        }
                                    });
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(
                                            "Waiting for you to finish in the browser…",
                                        )
                                        .small()
                                        .weak(),
                                    );
                                    ui.spinner();
                                }
                            }
                            State::Ready => {
                                ui.label(
                                    RichText::new("Select a conversation")
                                        .strong()
                                        .size(16.0),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(
                                        "Pick a chat on the left, or press Ctrl+N for a new one.",
                                    )
                                    .weak(),
                                );
                            }
                        }
                    });
                });
            });
        } else {
            // Section views own the whole central area.
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let mut actions: Vec<Action> = Vec::new();
                match self.main_view {
                    MainView::Calendar => crate::ui::sections::calendar_panel(
                        ui,
                        &self.meetings,
                        self.calendar_loading,
                        &self.palette,
                        &mut actions,
                    ),
                    MainView::Files => {
                        if let Some((chat_id, chat_name, files)) = &self.chat_files {
                            crate::ui::sections::chat_files_panel(
                                ui,
                                chat_id,
                                chat_name,
                                files,
                                self.chat_files_loading,
                                &self.palette,
                                &mut actions,
                            );
                            ui.add_space(6.0);
                        }
                        crate::ui::sections::files_panel(
                            ui,
                            &self.drive_files,
                            self.files_loading,
                            &self.palette,
                            &mut actions,
                        )
                    }
                    MainView::ToDo => {
                        let mut st = std::mem::take(&mut self.todo);
                        crate::ui::sections::todo_panel(
                            ui,
                            &self.todo_lists,
                            &self.todo_tasks,
                            &mut st,
                            self.todo_loading,
                            &self.palette,
                            &mut actions,
                        );
                        self.todo = st;
                    }
                    MainView::Planner => crate::ui::sections::planner_panel(
                        ui,
                        &self.planner,
                        self.planner_loading,
                        &self.palette,
                        &mut actions,
                    ),
                    MainView::Shifts => crate::ui::sections::shifts_panel(
                        ui,
                        &self.shifts,
                        self.shifts_loading,
                        &self.palette,
                    ),
                    MainView::Notes => {
                        let mut st = std::mem::take(&mut self.notes);
                        crate::ui::sections::notes_panel(
                            ui,
                            &self.notebooks,
                            self.notes_page.as_ref(),
                            &mut st,
                            self.notes_loading,
                            &self.palette,
                            &mut actions,
                        );
                        self.notes = st;
                    }
                    MainView::Activity => crate::ui::sections::activity_panel(
                        ui,
                        &self.activity,
                        &self.palette,
                        &mut actions,
                    ),
                    _ => unreachable!("chat family handled above"),
                }
                for a in actions {
                    self.apply(a);
                }
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

        // Settings window (own pass so it can borrow settings + catalog).
        let settings_just_opened = self.settings_ui.open && !self.settings_was_open;
        self.settings_was_open = self.settings_ui.open;
        if settings_just_opened {
            // Refresh the storage numbers once per window open.
            self.cmd.send(Command::ArchiveStats).ok();
        }
        if self.settings_ui.open {
            let themes: Vec<(String, String)> = self
                .catalog
                .picker_themes()
                .map(|t| (t.filename.clone(), theme::display_name(&t.filename).to_string()))
                .collect();
            let info = SettingsInfo {
                self_name: &self.self_name,
                signed_in: self.state == State::Ready,
                live: self.trouter_on,
                offline: self.offline.is_some(),
                themes: &themes,
                chats_cached: self.archive_stats.0,
                messages_cached: self.archive_stats.1,
            };
            let mut actions = Vec::new();
            let mut st = std::mem::take(&mut self.settings_ui);
            crate::ui::settings::settings_window(
                ui,
                &mut st,
                &mut self.settings,
                &info,
                &self.palette,
                &mut actions,
            );
            self.settings_ui = st;
            for a in actions {
                self.apply(a);
            }
        }

        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::N)) {
            self.apply(Action::ShowNewChat);
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F)) {
            self.apply(Action::OpenSearch);
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Comma)) {
            self.settings_ui.open = true;
        }
        // Section shortcuts: Ctrl+1..6.
        let section_keys = [
            (egui::Key::Num1, MainView::Chat),
            (egui::Key::Num2, MainView::Teams),
            (egui::Key::Num3, MainView::Calendar),
            (egui::Key::Num4, MainView::Files),
            (egui::Key::Num5, MainView::ToDo),
            (egui::Key::Num6, MainView::Planner),
            (egui::Key::Num7, MainView::Shifts),
            (egui::Key::Num8, MainView::Notes),
            (egui::Key::Num9, MainView::Activity),
        ];
        for (key, view) in section_keys {
            if ui.input(|i| i.modifiers.ctrl && i.key_pressed(key)) {
                self.switch_view(view);
            }
        }

        // Forward-to-chat picker.
        if let Some(fwd) = self.forward.as_mut() {
            let mut picked: Option<(String, String)> = None; // (chat_id, text)
            let mut close = false;
            egui::Window::new("Forward message")
                .default_width(380.0)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "“{}”",
                            if fwd.text.len() > 80 {
                                format!("{}…", &fwd.text[..80])
                            } else {
                                fwd.text.clone()
                            }
                        ))
                        .small()
                        .weak(),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut fwd.filter)
                            .hint_text("Filter chats…")
                            .desired_width(ui.available_width()),
                    );
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            let f = fwd.filter.to_lowercase();
                            for chat in self
                                .chats
                                .iter()
                                .filter(|c| {
                                    c.name.to_lowercase().contains(&f) || f.is_empty()
                                })
                                .take(30)
                            {
                                let name = if chat.name.is_empty() {
                                    "Direct message".to_string()
                                } else {
                                    chat.name.clone()
                                };
                                if ui.selectable_label(false, &name).clicked() {
                                    picked = Some((chat.id.clone(), fwd.text.clone()));
                                }
                            }
                        });
                    ui.add_space(4.0);
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            if close || picked.is_some() {
                self.forward = None;
            }
            if let Some((chat_id, text)) = picked {
                let cmid = ost::api::new_client_message_id();
                self.pending_sends.push(PendingSend {
                    cmid: cmid.clone(),
                    chat_id: chat_id.clone(),
                    text: text.clone(),
                    error: None,
                });
                self.cmd
                    .send(Command::Send {
                        chat_id,
                        text,
                        cmid,
                    })
                    .ok();
                self.status = "message forwarded".into();
            }
        }

        // Rename-channel dialog.
        if let Some((team, channel, mut current)) = self.channel_rename.clone() {
            egui::Window::new("Rename channel")
                .default_width(380.0)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new("New name:").small());
                    ui.add(
                        egui::TextEdit::singleline(&mut current)
                            .desired_width(ui.available_width()),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Rename").strong().color(self.palette.on_accent),
                                )
                                .fill(self.palette.accent)
                                .min_size(egui::vec2(70.0, 24.0)),
                            )
                            .clicked()
                            && !current.trim().is_empty()
                        {
                            let name = current.trim().to_string();
                            self.cmd
                                .send(Command::RenameChannel {
                                    team_id: team,
                                    channel_id: channel,
                                    name,
                                })
                                .ok();
                            self.channel_rename = None;
                            self.loading_teams = true;
                            self.cmd.send(Command::LoadTeams).ok();
                        }
                        if ui.button("Cancel").clicked() {
                            self.channel_rename = None;
                        }
                    });
                });
        }

        // Contact card: who they are + quick chat.
        if let Some((mri, name)) = self.contact.clone() {
            egui::Window::new("Contact")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        avatar(ui, &name, 40.0);
                        ui.vertical(|ui| {
                            ui.strong(&name);
                            ui.label(
                                RichText::new(mri.trim_start_matches("8:").to_string())
                                    .small()
                                    .weak(),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    ui.separator();
                    if ui
                        .button("Chat")
                        .clicked()
                    {
                        let mri = mri.clone();
                        let name = name.clone();
                        self.apply(Action::ChatWith { mri, name });
                    }
                    if ui.button("Close").clicked() {
                        self.contact = None;
                    }
                });
        }

        // Teams management dialog: create channel / join public team /
        // create team.
        if self.team_dialog.open {
            let modal = egui::Modal::new(egui::Id::new("teams_mgmt_modal"));
            let resp = modal.show(ui.ctx(), |ui| {
                ui.set_width(430.0);
                ui.label(RichText::new("Teams").strong().size(15.0));
                ui.add_space(6.0);
                let mut actions: Vec<Action> = Vec::new();
                teams_dialog(ui, &mut self.team_dialog, &self.teams, &mut actions);
                for a in actions {
                    self.apply(a);
                }
            });
            if resp.should_close() {
                self.team_dialog.open = false;
            }
        }

        // Join-with-link dialog (modal: centered, scrim, Esc/backdrop close).
        if self.join_open {
            let mut join_now = false;
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("join_meeting_modal"));
            let resp = modal.show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.label(RichText::new("Join a meeting").strong().size(15.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Paste a Teams meeting link, a meeting thread ID, or a meeting ID.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.join_source)
                        .hint_text("https://teams.microsoft.com/l/meetup-join/…")
                        .desired_width(ui.available_width()),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(10.0);
                // Actions right-aligned; primary last.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Join").strong().color(self.palette.on_accent),
                            )
                            .fill(self.palette.accent)
                            .min_size(egui::vec2(84.0, 26.0)),
                        )
                        .clicked()
                        || enter
                    {
                        join_now = true;
                    }
                    if ui
                        .add(egui::Button::new("Cancel").min_size(egui::vec2(76.0, 26.0)))
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            if resp.should_close() || close {
                self.join_open = false;
            }
            if join_now {
                let src = self.join_source.trim().to_string();
                if !src.is_empty() {
                    self.apply(Action::JoinMeeting { source: src, label: None });
                }
                self.join_source.clear();
            }
        }

        if let Some((emoji, names)) = self.reaction_popup.clone() {
            egui::Window::new(format!("Reactions {emoji}"))
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    if names.is_empty() {
                        ui.label("No reactions.");
                    }
                    for n in &names {
                        ui.label(n);
                    }
                    if ui.button("Close").clicked() {
                        self.reaction_popup = None;
                    }
                });
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
