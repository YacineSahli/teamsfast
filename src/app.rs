//! TeamsFast App: owns all UI state, drains backend events, applies actions.

use crate::backend::{self, Command, Event};
use base64::Engine as _;
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
use crate::theme::RichTextBold;

#[derive(PartialEq, Clone, Copy)]
enum State {
    Boot,
    NeedLogin,
    Ready,
}

/// The active section (left icon rail). The Planner/Shifts/Notes arms are
/// deliberately kept — the rail removal was a product call, not a cleanup.
#[derive(PartialEq, Clone, Copy)]
#[allow(dead_code)]
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

/// In-memory snapshot of a recently viewed conversation — what `open_chat`
/// paints instantly on a re-click, before any backend round-trip.
struct CachedConv {
    messages: Vec<MessageInfo>,
    members: HashMap<String, String>,
    older_link: Option<String>,
}

/// Keep the last N viewed conversations in RAM.
const CHAT_MEM_CAP: usize = 12;

/// A ringing incoming call: caller identity for the banner plus the raw
/// invitation JSON the backend needs to accept/decline.
#[derive(Clone)]
pub struct IncomingRing {
    pub name: String,
    /// Part of the ring payload for the incoming-call capture work; no
    /// reader yet (see "incoming-call ringing hardening" in AGENTS.md).
    #[allow(dead_code)]
    pub mri: String,
    pub has_video: bool,
    pub raw: String,
    pub at: Instant,
}

/// Teams rings for roughly 30 s before voicemail; drop the banner past 45 s
/// (caller-cancel pushes are not capture-verified yet — see NOTES).
pub const RING_TIMEOUT: Duration = Duration::from_secs(45);

/// Screen-share lifecycle for the live call.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ScreenShare {
    Off,
    /// Portal dialog open (or synthetic source spinning up).
    Starting,
    /// Frames flowing into the call's video send path.
    On,
}

/// Type-erased stop hook for a running share: halts the capture and frees
/// its threads. `Arc` so it can live in the App and the starter thread.
pub type ShareStop = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Outcome of a share-start thread (drained on a later frame).
pub enum ShareEvent {
    Started(ShareStop),
    Failed(String),
}

/// Drain a frame channel, keeping only the newest frame (live video is
/// latest-wins; backlog is dropped).
fn pal_ok_r(app: &TeamsFastApp) -> u8 {
    app.palette.ok.r()
}
fn pal_ok_g(app: &TeamsFastApp) -> u8 {
    app.palette.ok.g()
}
fn pal_ok_b(app: &TeamsFastApp) -> u8 {
    app.palette.ok.b()
}

/// True when the active call has reached `connected` (admitted for
/// meeting joins). Used by the stage's lobby view.
fn connected_stage(app: &TeamsFastApp) -> bool {
    app.call_connected_at.is_some()
        && app
            .call_media
            .as_ref()
            .map(|m| m.phase.borrow().to_string() == "connected")
            .unwrap_or(false)
}

/// Small confirmation slice from an action (send/reply/edit/delete/react):
/// merge by id so loaded older pages survive. Ids present in the slice
/// take the slice's (fresher) version; absent ids survive regardless —
/// other-client deletions heal on the next full refresh/open. Result
/// stays oldest-first.
fn merge_message_tail(existing: &mut Vec<MessageInfo>, page: Vec<MessageInfo>) {
    let page_ids: std::collections::HashSet<&str> =
        page.iter().map(|m| m.id.as_str()).collect();
    let mut merged: Vec<MessageInfo> = existing
        .drain(..)
        .filter(|m| !page_ids.contains(m.id.as_str()))
        .collect();
    merged.extend(page);
    merged.sort_by(|a, b| {
        let (ta, tb) = (
            crate::model::to_epoch_ms(&a.timestamp).unwrap_or(0),
            crate::model::to_epoch_ms(&b.timestamp).unwrap_or(0),
        );
        ta.cmp(&tb).then_with(|| a.id.cmp(&b.id))
    });
    *existing = merged;
}

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
    fake_frame_sized(320, 240, t_ms)
}

/// [`fake_frame`] at an arbitrary canvas — the synthetic screen-share
/// source wants a screen-shaped frame (1280x720) to exercise the send
/// loop's resolution-change encoder re-init.
fn fake_frame_sized(w: u32, h: u32, t_ms: u128) -> ost::calling::video::VideoFrame {
    let (wu, hu) = (w as usize, h as usize);
    let ysz = wu * hu;
    let mut data = vec![128u8; ysz + ysz / 2];
    let shift = ((t_ms / 40) as u32 % w) as usize;
    let bar_w = (w / 6).max(1) as usize;
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

/// Open the portal ScreenCast session (blocks on the compositor's source
/// picker) and wrap it in a type-erased stop hook.
#[cfg(feature = "screenshare")]
fn portal_screen_source(
    tx: std::sync::mpsc::SyncSender<ost::calling::video::VideoFrame>,
) -> Result<ShareStop, String> {
    match ost::calling::screen::start_screen_capture(tx, true) {
        Ok(session) => {
            // ScreenShareSession is Send-but-not-Sync and not nameable in
            // builds without the feature — erase it behind the stop hook.
            let sess = std::sync::Arc::new(std::sync::Mutex::new(Some(session)));
            Ok(std::sync::Arc::new(move || {
                if let Ok(mut guard) = sess.lock() {
                    if let Some(mut s) = guard.take() {
                        s.stop();
                    }
                }
            }))
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

/// QA source (TEAMSFAST_SCREENSHARE=1): 1280x720 animated bars at ~10fps.
/// Drives the exact share path (slot swap, encoder re-init, preview tee)
/// without the interactive portal dialog.
fn synthetic_screen_source(
    tx: std::sync::mpsc::SyncSender<ost::calling::video::VideoFrame>,
) -> ShareStop {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("qa-screen-share".into())
        .spawn(move || {
            let t0 = Instant::now();
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                // Bounded channel + latest-wins drain in the send loop:
                // a full channel just drops, no backlog.
                let _ = tx.try_send(fake_frame_sized(1280, 720, t0.elapsed().as_millis()));
                std::thread::sleep(Duration::from_millis(100));
            }
        })
        .ok();
    let hook: ShareStop =
        std::sync::Arc::new(move || stop.store(true, std::sync::atomic::Ordering::SeqCst));
    hook
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
    /// Some updater paths emit an explicit in-progress beat.
    #[allow(dead_code)]
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
    /// Kept for updater paths that surface failure as a state.
    #[allow(dead_code)]
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
    /// Last-viewed conversations (messages + members + older cursor) so a
    /// re-open paints instantly from RAM while the backend refreshes.
    /// Small LRU: the newest `CHAT_MEM_CAP` chats stay, older ones evict.
    chat_mem: HashMap<String, CachedConv>,
    /// Last message id we sent a mark-read POST for, per chat — refreshes
    /// re-deliver the same tail and would otherwise re-POST every time.
    marked_read: HashMap<String, String>,
    /// When read receipts were last fetched per chat (throttle).
    last_receipts: HashMap<String, Instant>,

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

    trouter_on: bool,
    pending_open_refresh: bool,
    last_open_refresh: Instant,
    /// A debounced RefreshChat is on the worker and its Messages reply
    /// hasn't landed. While set, further debounced refreshes hold off —
    /// bursts re-arm the flag instead of queueing one fetch per message
    /// (the worker is serial; each queued page-fetch delays sends).
    refresh_in_flight: bool,
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
    /// When the call first reported `connected` — the banner timer counts
    /// from here, not from placement (ringing time is not call time).
    call_connected_at: Option<Instant>,
    /// Incoming-call ringer stop flag; Some while the banner rings.
    ringer: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// True when the active call is a MEETING join (lobby UI applies).
    call_is_meeting: bool,
    /// Join/lobby machine over the active meeting call
    /// (joining → lobby → admitted; pure transitions from teams-core).
    lobby: ost::api::LobbyState,
    lobby_placed_at: Option<Instant>,
    /// Ringing incoming call (banner with Accept/Decline).
    incoming: Option<IncomingRing>,
    /// Live call controls (mute/camera watches + video frame receivers)
    /// for the call stage UI; one per placed call.
    call_media: Option<ost::calling::CallControlsHandle>,
    /// Call stage panel open (tiles + controls).
    call_view_open: bool,
    /// Known image sizes (url -> w,h): placeholders reserve the final
    /// box so loading images doesn't reflow the conversation.
    image_sizes: HashMap<String, [usize; 2]>,
    /// Remote/local video textures for the stage (recreated on size change).
    remote_video_tex: Option<egui::TextureHandle>,
    local_video_tex: Option<egui::TextureHandle>,
    /// QA: TEAMSFAST_FAKEVIDEO=1 animates the remote tile without a peer.
    fake_video: bool,
    /// Screen-share state for the live call (stage Share button).
    screen_share: ScreenShare,
    /// Stop hook for the running share (portal session or the synthetic
    /// QA generator); type-erased so builds without the `screenshare`
    /// feature still compile.
    screen_share_stop: Option<ShareStop>,
    /// Last share failure to show in the stage (cleared on retry).
    share_note: Option<String>,
    /// Completions from share threads — the portal dialog blocks on user
    /// interaction, so the outcome arrives on a later frame.
    share_events: (std::sync::mpsc::Sender<ShareEvent>, std::sync::mpsc::Receiver<ShareEvent>),
    /// QA auto-share fired for this call (TEAMSFAST_SCREENSHARE=1).
    share_auto_fired: bool,

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
    /// Calendar view mode + navigation state (week/month grids).
    cal_view: crate::ui::sections::CalView,
    cal_week_offset: i32,
    cal_month_offset: i32,
    /// List view steps in whole weeks through the fetched window.
    cal_list_offset: i32,
    /// Window the current `meetings` set covers (unix secs), for lazy
    /// fetches when week/month navigation leaves it.
    calendar_fetched: Option<(u64, u64)>,
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
    /// OneNote state (section off the rail by product decision; kept for
    /// a future return — the dead_code is intent, not neglect).
    #[allow(dead_code)]
    notebooks: Vec<crate::ui::sections::NotebookTree>,
    #[allow(dead_code)]
    notes_page: Option<ost::api::NotePage>,
    #[allow(dead_code)]
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
    /// Wayland toplevels cannot unmap themselves (winit no-ops
    /// Visible(false)), so "hide" minimizes there instead of hiding.
    on_wayland: bool,

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
    pal: &crate::theme::Palette,
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
                        egui::Button::new(RichText::new("Create").small())
                            .min_size(egui::vec2(68.0, 26.0))
                            .fill(pal.accent)
                            .corner_radius(egui::CornerRadius::same(8)),
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
                        .desired_width(ui.available_width() - 78.0),
                );
                let enter =
                    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let search = ui.add(
                    egui::Button::new(RichText::new("Search").small())
                        .min_size(egui::vec2(68.0, 26.0))
                        .corner_radius(egui::CornerRadius::same(8)),
                );
                if (search.clicked() || enter) && !st.join_query.trim().is_empty()
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
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Join").small())
                                .min_size(egui::vec2(68.0, 26.0))
                                .corner_radius(egui::CornerRadius::same(8)),
                        )
                        .clicked()
                    {
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
                        egui::Button::new(RichText::new("Create").small())
                            .min_size(egui::vec2(68.0, 26.0))
                            .fill(pal.accent)
                            .corner_radius(egui::CornerRadius::same(8)),
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
    /// Start screen sharing on the live call: portal ScreenCast (feature
    /// build) or the synthetic source (TEAMSFAST_SCREENSHARE=1 QA). The
    /// portal dialog blocks on user interaction, so the start runs on a
    /// plain thread and the outcome lands via `share_events` on a later
    /// frame. Requires a video call: the share rides the video leg (no
    /// modality re-negotiation).
    fn begin_screen_share(&mut self) {
        if self.screen_share != ScreenShare::Off {
            return;
        }
        let Some(media) = self.call_media.as_ref() else { return; };
        if media.local_preview.is_none() {
            self.share_note =
                Some("Screen sharing needs a video call (the share rides the video leg).".into());
            return;
        }
        let slot = media.screen_source.clone();
        let (tx_frames, rx_frames) =
            std::sync::mpsc::sync_channel::<ost::calling::video::VideoFrame>(4);
        let tx_ev = self.share_events.0.clone();
        self.screen_share = ScreenShare::Starting;
        self.share_note = None;
        let spawned = std::thread::Builder::new()
            .name("screen-share-start".into())
            .spawn(move || {
                let synthetic =
                    std::env::var("TEAMSFAST_SCREENSHARE").as_deref() == Ok("1");
                let res: Result<ShareStop, String> = if synthetic {
                    Ok(synthetic_screen_source(tx_frames))
                } else {
                    #[cfg(feature = "screenshare")]
                    {
                        portal_screen_source(tx_frames)
                    }
                    #[cfg(not(feature = "screenshare"))]
                    {
                        let _ = tx_frames;
                        Err("This build lacks screen sharing — rebuild with --features screenshare.".into())
                    }
                };
                match res {
                    Ok(stop) => {
                        if let Ok(mut guard) = slot.lock() {
                            *guard = Some(rx_frames);
                        }
                        let _ = tx_ev.send(ShareEvent::Started(stop));
                    }
                    Err(e) => {
                        let _ = tx_ev.send(ShareEvent::Failed(e));
                    }
                }
            });
        if spawned.is_err() {
            self.screen_share = ScreenShare::Off;
            self.share_note = Some("could not spawn the share thread".into());
        }
    }

    /// Stop the running share (portal session or synthetic generator) and
    /// clear the send-loop slot. Safe to call when not sharing.
    fn stop_screen_share(&mut self) {
        if let Some(stop) = self.screen_share_stop.take() {
            stop();
        }
        if let Some(media) = self.call_media.as_ref() {
            media.set_screen_source(None);
        }
        self.screen_share = ScreenShare::Off;
    }

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
        let (cmd, wake_rx) = backend::spawn(tx);
        cmd.send(Command::CheckReady).ok();
        if settings_load.auto_live {
            cmd.send(Command::StartTrouter).ok();
        }
        // Park a thread on the event-wake channel: every backend event
        // requests a repaint directly, so the render loop can be fully
        // event-driven (the old unconditional 500ms repaint is gone).
        {
            let ctx = cc.egui_ctx.clone();
            std::thread::Builder::new()
                .name("event-wake-ui".into())
                .spawn(move || {
                    while wake_rx.recv().is_ok() {
                        ctx.request_repaint();
                    }
                })
                .ok();
        }
        // Interface zoom from settings.
        cc.egui_ctx.set_zoom_factor(settings_load.zoom);
        let (notif_tx, notif_rx) = std::sync::mpsc::channel::<String>();
        let mut app = Self {
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
            // QA hook: TEAMSFAST_DRAFT seeds the composer draft ("|" is a
            // line break) so multi-line composer states render headlessly.
            draft: std::env::var("TEAMSFAST_DRAFT")
                .unwrap_or_default()
                .replace('|', "\n"),
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
            trouter_on: false,
            pending_open_refresh: false,
            last_open_refresh: Instant::now() - Duration::from_secs(10),
            refresh_in_flight: false,
            chat_mem: HashMap::new(),
            marked_read: HashMap::new(),
            last_receipts: HashMap::new(),
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
            on_wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
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
            call_connected_at: None,
            ringer: None,
            call_is_meeting: false,
            lobby: ost::api::LobbyState::Idle,
            lobby_placed_at: None,
            incoming: None,
            call_media: None,
            call_view_open: false,
            image_sizes: HashMap::new(),
            remote_video_tex: None,
            local_video_tex: None,
            fake_video: std::env::var("TEAMSFAST_FAKEVIDEO").as_deref() == Ok("1"),
            screen_share: ScreenShare::Off,
            screen_share_stop: None,
            share_note: None,
            share_events: std::sync::mpsc::channel(),
            share_auto_fired: false,
            join_open: false,
            join_source: String::new(),
            call_label_hint: None,
            team_dialog: TeamDialogState::default(),
            contact: None,
            main_view: MainView::Chat,
            meetings: Vec::new(),
            calendar_loading: false,
            cal_view: crate::ui::sections::CalView::List,
            cal_week_offset: 0,
            cal_month_offset: 0,
            cal_list_offset: 0,
            calendar_fetched: None,
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
        };
        // Apply the SAVED theme at startup: the builtin light choice used
        // to reset to dark on every restart (apply_selected_theme only ran
        // on settings CHANGES; file themes had a pending-catalog path,
        // builtins had none).
        app.apply_selected_theme();
        app
    }

    /// Stop the incoming-call ringer (accept / decline / caller gone /
    /// banner expiry).
    fn stop_ringer(&mut self) {
        if let Some(flag) = self.ringer.take() {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Ring for an incoming call: repeating tone until answered/declined/
    /// gone (capped at the banner's ring window). Detached thread — the
    /// tone owns its playback stream.
    fn start_ringer(&mut self) {
        self.stop_ringer();
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.ringer = Some(flag.clone());
        std::thread::Builder::new()
            .name("ringer".into())
            .spawn(move || {
                ost::calling::audio::play_ring_until(
                    425.0,
                    1000,
                    2000,
                    flag,
                    (RING_TIMEOUT.as_millis() as u64).max(1000),
                );
            })
            .ok();
    }

    /// Short call-ended beep (detached).
    fn play_end_beep(&mut self) {
        std::thread::Builder::new()
            .name("end-beep".into())
            .spawn(|| {
                let _ = ost::calling::audio::play_tone_freq(425.0, 450);
            })
            .ok();
    }

    /// Hide the window: unmap on X11, minimize on Wayland (a Wayland
    /// toplevel cannot unmap itself — `Visible(false)` is a no-op there,
    /// which left both the ✕ button and the tray Hide dead on Wayland).
    fn hide_window(&mut self, ctx: &egui::Context) {
        if self.on_wayland {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        self.window_hidden = true;
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
                    // Restore a Wayland "hidden" window: it was minimized,
                    // not unmapped (unmap is impossible there).
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.window_hidden = false;
                }
                TrayAction::Hide => {
                    self.hide_window(ui.ctx());
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
        // Share-thread completions first: a Started that races a call-end
        // in the same frame must not leave a stop hook behind.
        while let Ok(ev) = self.share_events.1.try_recv() {
            match ev {
                ShareEvent::Started(stop) => {
                    // The call may have ended while the portal dialog was
                    // open — stop immediately in that case.
                    if self.call_media.is_some() {
                        self.screen_share_stop = Some(stop);
                        self.screen_share = ScreenShare::On;
                    } else {
                        stop();
                    }
                }
                ShareEvent::Failed(e) => {
                    self.screen_share = ScreenShare::Off;
                    self.share_note = Some(e);
                }
            }
        }
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
                Event::ActivitySeeded(seeded) => {
                    // Merge with anything live events already added; drop
                    // duplicates by (chat, preview, second).
                    for e in seeded {
                        let dup = self.activity.iter().any(|x| {
                            x.chat_id == e.chat_id
                                && x.preview == e.preview
                                && x.at_ms / 1000 == e.at_ms / 1000
                        });
                        if !dup {
                            self.activity.push(e);
                        }
                    }
                    self.activity
                        .sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
                    self.activity.truncate(200);
                }
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
                    self.call_is_meeting = true;
                    self.lobby = ost::api::lobby_next(self.lobby, ost::api::LobbyEvent::Start);
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
                    self.start_ringer();
                }
                Event::CallGone => {
                    // Caller cancelled while ringing: drop the banner. The
                    // accepted-call case stays manual until the callback
                    // shapes are capture-verified (see NOTES).
                    self.incoming = None;
                    self.stop_ringer();
                }
                Event::CallMedia(handle) => {
                    // Video calls open the stage; voice calls stay on the
                    // banner — a large placeholder tile on an audio call
                    // just reads as an empty video screen. The local-
                    // preview sink exists only when the camera is on.
                    // Meeting joins keep the stage the join already opened
                    // (the lobby room is the content until admitted).
                    self.call_view_open = handle.local_preview.is_some()
                        || (self.call_is_meeting && self.call_view_open);
                    self.call_media = Some(handle);
                    self.remote_video_tex = None;
                    self.local_video_tex = None;
                    // QA hook: TEAMSFAST_SCREENSHARE=1 auto-starts the
                    // synthetic source on video calls so the sharing UI,
                    // preview tee, and encoder re-init can be pixel-checked
                    // headlessly (TEAMSFAST_VIDEOCALL=1 places the call).
                    if std::env::var("TEAMSFAST_SCREENSHARE").as_deref() == Ok("1")
                        && !self.share_auto_fired
                        && self.call_media.as_ref().is_some_and(|m| m.local_preview.is_some())
                    {
                        self.share_auto_fired = true;
                        self.begin_screen_share();
                    }
                }
                Event::CallStarted { label } => {
                    if self.call_is_meeting {
                        self.lobby =
                            ost::api::lobby_next(self.lobby, ost::api::LobbyEvent::Placed);
                        self.lobby_placed_at = Some(Instant::now());
                        // The meeting join deserves the full stage: the
                        // lobby/room is the content until admitted.
                        self.call_view_open = true;
                    }
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
                    self.play_end_beep();
                    self.stop_screen_share();
                    self.call = None;
                    self.call_connected_at = None;
                    self.call_is_meeting = false;
                    self.lobby = ost::api::LobbyState::Idle;
                    self.lobby_placed_at = None;
                    self.call_media = None;
                    self.call_view_open = false;
                    self.remote_video_tex = None;
                    self.local_video_tex = None;
                    self.status = summary;
                }
                Event::CallFailed(e) => {
                    self.play_end_beep();
                    self.stop_screen_share();
                    self.call = None;
                    self.call_connected_at = None;
                    self.call_is_meeting = false;
                    self.lobby = ost::api::LobbyState::Idle;
                    self.lobby_placed_at = None;
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
                    // QA hook: TEAMSFAST_NEWCHAT=1 opens the new-conversation modal.
                    if std::env::var("TEAMSFAST_NEWCHAT").as_deref() == Ok("1") {
                        self.new_chat.open = true;
                    }
                    // QA hook: TEAMSFAST_TESTCALL=1 places an echo test call;
                    // TEAMSFAST_VIDEOCALL=1 does the same with the camera on.
                    // QA hook: TEAMSFAST_MOCKLOBBY=1 pins the meeting-join
                    // lobby view on the echo call (a real lobby needs a
                    // gated meeting + admission).
                    if std::env::var("TEAMSFAST_MOCKLOBBY").as_deref() == Ok("1") {
                        self.call_is_meeting = true;
                        self.lobby = ost::api::LobbyState::Lobby;
                        self.lobby_placed_at = Some(Instant::now());
                        self.call_view_open = true;
                    }
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
                    // QA hook: TEAMSFAST_ACTIVITY=1 seeds a synthetic feed
                    // row (the live feed only fills from unread/mentions,
                    // which headless QA runs usually have none of).
                    if std::env::var("TEAMSFAST_ACTIVITY").as_deref() == Ok("1") {
                        self.activity.push(crate::ui::sections::ActivityEntry {
                            kind: "mention",
                            chat_id: String::new(),
                            chat_name: "Alerting".into(),
                            who: "Marta Kowalska".into(),
                            preview: "heads up on the deploy window @you".into(),
                            at_ms: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0),
                        });
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
                            "activity" => Some(MainView::Activity),
                            _ => None,
                        };
                        if let Some(view) = view {
                            self.switch_view(view);
                        }
                    }
                    // QA hook: TEAMSFAST_CALVIEW=<list|week|month>.
                    if let Ok(v) = std::env::var("TEAMSFAST_CALVIEW") {
                        self.cal_view = match v.trim().to_ascii_lowercase().as_str() {
                            "week" => crate::ui::sections::CalView::Week,
                            "month" => crate::ui::sections::CalView::Month,
                            _ => crate::ui::sections::CalView::List,
                        };
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
                    // No "N chats" status — the count is visible in the list;
                    // the top bar carries transient state only.
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
                    merge,
                    older_link,
                } => {
                    if self.selected.as_deref() != Some(chat_id.as_str()) {
                        continue;
                    }
                    // A reply for the open chat means any in-flight
                    // debounced refresh has landed.
                    self.refresh_in_flight = false;
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
                    } else if merge {
                        merge_message_tail(&mut self.messages, messages);
                    } else {
                        self.messages = messages;
                    }
                    if !members.is_empty() {
                        self.members = members;
                    }
                    // Merge slices carry no cursor (the pre-action one stays
                    // valid); only overwrite when the page actually has one.
                    if older_link.is_some() {
                        self.older_link = older_link;
                    }
                    if let Some(name) = resolved_name {
                        self.rename_chat(&chat_id, name);
                    }
                    // QA hook: TEAMSFAST_TABLE=1 appends two synthetic
                    // messages to the opened chat — a chat-HTML pipe table
                    // (exercises the <br> grouping) and an Adaptive Card
                    // carrying the real k8s-scan Jira table payload — so
                    // table rendering is screenshot-verifiable without
                    // scrolling a live chat to an old message. Network
                    // refreshes full-replace the list, so re-append
                    // whenever the seed is missing (checked by id, not a
                    // one-shot flag).
                    if std::env::var("TEAMSFAST_TABLE").as_deref() == Ok("1")
                        && !self.messages.is_empty()
                        && !self.messages.iter().any(|m| m.id == "qa-table-card")
                    {
                        let now_ts = "2026-10-16T21:30:00.000Z".to_string();
                        let qa = |id: &str, sender: &str, content: &str, raw: String| MessageInfo {
                            id: id.to_string(),
                            sender_mri: "8:orgid:00000000-0000-0000-0000-0000000000f1".into(),
                            sender: sender.to_string(),
                            timestamp: now_ts.clone(),
                            content: content.to_string(),
                            raw,
                            reactions: Vec::new(),
                            reply_to: None,
                            client_message_id: None,
                        };
                        self.messages.push(qa(
                            "qa-table-chat",
                            "QA Bot",
                            "line one line two line three Workload Container Action",
                            "<p>line one<br>line two<br>line three</p><p>before the table<br>||Workload||Container||Action||<br>|qa-default|api|*Reduce*<br>|qa-default|ast...</p>".into(),
                        ));
                        let card_json = r#"{"summary":"k8s right-sizing (QA)","attachments":[{"content":{"type":"AdaptiveCard","body":[{"type":"TextBlock","wrap":true,"text":" Resource request/limit right-sizing findings (7-day Grafana p95/max lookback)\n\nContainers where usage already exceeds the current request are marked *Increase* rather than reduced.\n\n 1. QA Production\n\n||Workload||Container||CPU req → suggested||Mem req → suggested||Action||\n|qa-default|api|0.500 → 0.20|512Mi → 384Mi|Reduce|\n|qa-default|management-api|0.200 → 0.05|500Mi → 384Mi|Reduce|\n|qa-default|ast..."}]}}]}"#;
                        let b64 = base64::engine::general_purpose::STANDARD
                            .encode(card_json.as_bytes());
                        self.messages.push(qa(
                            "qa-table-card",
                            "QA Bot",
                            "k8s right-sizing (QA)",
                            format!(r#"<Swift b64="{b64}"/>"#),
                        ));
                    }
                    // Who has read what (quiet fetch, no error surfaced) —
                    // throttled: refreshes land in bursts and receipts move
                    // slowly, each fetch is a worker round-trip.
                    if self
                        .last_receipts
                        .get(&chat_id)
                        .map(|t| t.elapsed() >= Duration::from_secs(4))
                        .unwrap_or(true)
                    {
                        self.last_receipts.insert(chat_id.clone(), Instant::now());
                        self.cmd.send(Command::ReadReceipts(chat_id.clone())).ok();
                    }
                    // Freshly loaded tail: mark read — unless ghost mode
                    // holds read receipts back. Skip when the tail id is
                    // unchanged: every refresh re-delivers the same last
                    // message and would re-POST the receipt each time.
                    if !self.settings.ghost_mode
                        && let Some(last) = self.messages.last().map(|m| m.id.clone())
                        && self.marked_read.get(&chat_id) != Some(&last)
                    {
                        self.marked_read.insert(chat_id.clone(), last.clone());
                        self.cmd
                            .send(Command::MarkRead {
                                chat_id: chat_id.clone(),
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
                    let dim = [size[0], size[1]];
                    if self.image_sizes.get(&url) != Some(&dim) {
                        self.image_sizes.insert(url.clone(), dim);
                        self.cmd
                            .send(Command::SaveImageSize {
                                url: url.clone(),
                                w: size[0],
                                h: size[1],
                            })
                            .ok();
                    }
                    self.queued_textures.push((url, rgba, size));
                }
                Event::ImageSizes(sizes) => {
                    self.image_sizes = sizes
                        .into_iter()
                        .map(|(u, (w, h))| (u, [w, h]))
                        .collect();
                }
                Event::ImageFailed(_url) => {
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
                Event::TrouterConnected => {
                    self.trouter_on = true;
                    // Connectivity is the status dot's job — no top-bar text.
                    self.status = String::new();
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
                    // entries only when the map lacks the chat. The chat
                    // currently open was just read — its server horizon is
                    // stale until the mark-read round-trip lands, so never
                    // re-badge it from the refresh (that drew an unread
                    // dot on the conversation the user was LOOKING at).
                    let open = self.selected.clone();
                    for (id, v) in map {
                        if Some(&id) == open.as_ref() {
                            continue;
                        }
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
                    // QA hook: TEAMSFAST_CALDUMP=1 logs every meeting's
                    // span triple — lane/size forensics without a debugger.
                    if std::env::var("TEAMSFAST_CALDUMP").as_deref() == Ok("1") {
                        for m in &meetings {
                            let (d, s, dur) = crate::ui::sections::meeting_span_pub(m);
                            log::info!("CALDUMP date={d} start_min={s} dur_min={dur} subject={:?} start={:?} canceled={}", m.subject, m.start, m.canceled);
                        }
                    }
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
                Event::Error(e) => {
                    // A failed worker op also un-wedges the debounced
                    // refresh guard (a hung fetch must not stop refreshes).
                    self.refresh_in_flight = false;
                    self.error = Some(e);
                }
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

        // Drive the meeting-lobby machine: connected = admitted; a join
        // still placing after the grace window reads as the lobby.
        if self.call.is_some()
            && self.call_is_meeting
            && let Some(media) = self.call_media.as_ref()
        {
            let phase = media.phase.borrow().to_string();
            if phase.starts_with("connected") {
                self.lobby = ost::api::lobby_next(self.lobby, ost::api::LobbyEvent::Admit);
            } else if matches!(
                self.lobby,
                ost::api::LobbyState::Joining | ost::api::LobbyState::Lobby
            ) && self
                .lobby_placed_at
                .map(|t| t.elapsed() >= Duration::from_secs(12))
                .unwrap_or(false)
            {
                self.lobby =
                    ost::api::lobby_next(self.lobby, ost::api::LobbyEvent::LobbySignal);
            }
        }

        // Presence poll every 60 s while signed in.
        if self.state == State::Ready
            && self.last_presence_poll.elapsed() >= Duration::from_secs(60)
        {
            self.last_presence_poll = Instant::now();
            self.cmd.send(Command::PollPresence).ok();
        }

        // Debounced open-chat refresh on live activity — history only; the
        // view never blanks (the full open_chat re-run it replaced also
        // refetched the roster on every incoming message).
        // 400ms debounce (was 900): the merge-capable page fetch makes the
        // visible-bubble latency mostly debounce + one RTT; bursts re-arm
        // instead of queueing (see refresh_in_flight).
        // A fetch that dies without an event (transport error paths in the
        // worker) must not wedge the guard — self-heal after 5s.
        if self.refresh_in_flight && self.last_open_refresh.elapsed() >= Duration::from_secs(5)
        {
            self.refresh_in_flight = false;
        }
        if self.pending_open_refresh
            && !self.refresh_in_flight
            && self.last_open_refresh.elapsed() >= Duration::from_millis(400)
        {
            self.pending_open_refresh = false;
            let waited = self.last_open_refresh.elapsed();
            self.last_open_refresh = Instant::now();
            self.refresh_in_flight = true;
            log::debug!("open-chat refresh: firing after {}ms of activity", waited.as_millis());
            self.refresh_chat();
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
                // Same key as the backend list: to_epoch_ms, not a bare u64
                // parse — the conversations list carries ISO timestamps,
                // which a u64 parse sends to 0 (every ISO chat sank below
                // the few numeric ones; group chats floated above older
                // chats).
                let k = |c: &ChatInfo| crate::backend::chat_recency(c);
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
        // Park the conversation we are leaving (its live state is the
        // freshest copy that exists) so a re-click paints instantly.
        if let Some(prev) = self.selected.clone()
            && prev != id
        {
            let cached = CachedConv {
                messages: std::mem::take(&mut self.messages),
                members: std::mem::take(&mut self.members),
                older_link: self.older_link.take(),
            };
            self.chat_mem.insert(prev.clone(), cached);
            if self.chat_mem.len() > CHAT_MEM_CAP
                && let Some(k) = self.chat_mem.keys().find(|k| **k != prev).cloned()
            {
                self.chat_mem.remove(&k);
            }
        }
        self.selected = Some(id.clone());
        self.edit = None;
        self.reply = None;
        // Paint immediately from RAM when this chat was viewed recently —
        // the backend refresh then reconciles when it lands. Without this
        // the pane blanked until the worker round-tripped, which read as
        // "clicking a chat is slow" even with a warm archive.
        if let Some(cached) = self.chat_mem.remove(&id) {
            self.messages = cached.messages;
            self.members = cached.members;
            self.older_link = cached.older_link;
        } else {
            self.messages.clear();
            self.members.clear();
            self.older_link = None;
        }
        self.loading_older = false;
        self.receipts.clear();
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

    /// Light live-activity refresh of the open conversation: history page
    /// only, nothing cleared — the view stays on screen until the fresh
    /// page replaces it (the old path re-opened the chat, blanking it and
    /// re-fetching the roster every time a message arrived).
    fn refresh_chat(&mut self) {
        if let Some(chat) = self.selected.clone() {
            self.cmd.send(Command::RefreshChat(chat)).ok();
        }
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
            Action::LoadCalendarRange { start_unix, days } => {
                self.calendar_fetched = Some((start_unix, start_unix + days * 86_400));
                // Gates ensure_fetched re-fires until the reply lands.
                self.calendar_loading = true;
                self.cmd
                    .send(Command::LoadCalendarRange { start_unix, days })
                    .ok();
            }
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
                self.stop_ringer();
                if let Some(ring) = self.incoming.take() {
                    self.cmd.send(Command::AcceptIncoming(ring.raw)).ok();
                }
            }
            Action::DeclineIncoming => {
                self.stop_ringer();
                if let Some(ring) = self.incoming.take() {
                    self.cmd.send(Command::DeclineIncoming(ring.raw)).ok();
                }
            }
            Action::JoinMeeting { source, label } => {
                self.join_open = false;
                self.status = "Resolving meeting…".into();
                self.call_is_meeting = true;
                self.lobby = ost::api::lobby_next(self.lobby, ost::api::LobbyEvent::Start);
                self.call_label_hint = label;
                self.cmd.send(Command::JoinMeeting(source)).ok();
            }
            Action::ShowJoinDialog => self.join_open = true,
            Action::Status(text) => self.status = text,
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
            Action::CloseChatFiles => {
                self.chat_files = None;
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
            Action::NewChannelInTeam(team_id) => {
                self.team_dialog.open = true;
                self.team_dialog.channel_team = Some(team_id);
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
                self.cal_week_offset = 0;
                self.cal_month_offset = 0;
                self.cal_list_offset = 0;
                let start = jiff::Zoned::now().with().hour(0).minute(0).second(0).subsec_nanosecond(0).build().map(|z| z.timestamp().as_second() as u64).unwrap_or(0);
                self.calendar_fetched = Some((start, start + 35 * 86_400));
                self.cmd
                    .send(Command::LoadCalendarRange {
                        start_unix: start,
                        days: 35,
                    })
                    .ok();
            }
            MainView::Files => {
                self.files_loading = true;
                self.cmd.send(Command::LoadFiles).ok();
            }
            MainView::ToDo => {
                self.todo_loading = true;
                self.cmd.send(Command::LoadTodo).ok();
            }
            // Shifts/Notes/Planner removed from the UI (2026-10); arms kept
            // as no-ops so the enum variants stay valid for cached state.
            MainView::Planner | MainView::Shifts | MainView::Notes | MainView::Activity => {}
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
                                            .bold()
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
        // OneNote, Shifts and Planner removed (2026-10) — thin usage;
        // backend section handlers stay for a future return.
        let sections: [(MainView, &'static str, crate::theme::Icon, &str); 6] = [
            (MainView::Chat, "Chat", crate::theme::Icon::MessageSquare, "Chats (Ctrl+1)"),
            (MainView::Teams, "Teams", crate::theme::Icon::Users, "Teams & channels (Ctrl+2)"),
            (MainView::Calendar, "Calendar", crate::theme::Icon::Calendar, "Calendar (Ctrl+3)"),
            (MainView::Files, "Files", crate::theme::Icon::FileText, "Files (Ctrl+4)"),
            (MainView::ToDo, "To Do", crate::theme::Icon::SquareCheck, "To Do (Ctrl+5)"),
            (MainView::Activity, "Activity", crate::theme::Icon::Activity, "Activity (Ctrl+6)"),
        ];
        for (view, _name, icon, tip) in sections {
            let sel = self.main_view == view;
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
            // Hover tint fades in/out (the last snapping surface).
            let t = crate::ui::widgets::hover_t(ui.ctx(), resp.id, resp.hovered() && !sel);
            if sel {
                // Stronger wash on light themes: accent@31% over a light
                // panel washed the glyph out (~2.8:1 — under the 3:1
                // icon-guideline floor); dark themes keep the softer tint.
                let [wr, wg, wb, _] = self.palette.window.to_srgba_unmultiplied();
                let light = u16::from(wr) + u16::from(wg) + u16::from(wb) > 384;
                ui.painter().rect_filled(rect, 8, {
                    let [r, g, b, _] = self.palette.accent.to_srgba_unmultiplied();
                    Color32::from_rgba_unmultiplied(r, g, b, if light { 0x86 } else { 0x50 })
                });
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.left_top(),
                        egui::vec2(3.0, rect.height()),
                    ),
                    2.0,
                    self.palette.accent,
                );
            } else if t > 0.01 {
                ui.painter().rect_filled(
                    rect,
                    8,
                    crate::theme::tint_over(self.palette.panel, self.palette.surface_hover, t),
                );
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
            // Unread indicator on the Chat icon: count pill (the bare
            // accent dot read as a rendering bug — a badge says "how many").
            if view == MainView::Chat {
                let total: u32 = self
                    .unread
                    .values()
                    .map(|(u, _)| u32::from(*u))
                    .sum();
                if total > 0 {
                    let text = if total > 99 {
                        "99+".to_string()
                    } else {
                        total.to_string()
                    };
                    let gal = ui.painter().layout_no_wrap(
                        text.clone(),
                        egui::FontId::proportional(9.0),
                        self.palette.on_accent,
                    );
                    let w = (gal.size().x + 8.0).max(14.0);
                    let c = rect.right_top() + egui::vec2(-2.0, 2.0);
                    let pill = egui::Rect::from_center_size(c, egui::vec2(w, 13.0));
                    ui.painter()
                        .rect_filled(pill, 6.5, self.palette.accent);
                    ui.painter().galley(
                        egui::pos2(
                            pill.center().x - gal.size().x / 2.0,
                            pill.center().y - gal.size().y / 2.0,
                        ),
                        gal,
                        self.palette.on_accent,
                    );
                }
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            // bottom_up: the first space lands BELOW the gear — an 8px
            // margin from the window edge instead of a flush-cramped icon.
            ui.add_space(8.0);
            let gear = ui.add(
                egui::Button::new(
                    crate::theme::Icon::Settings
                        .image(self.palette.secondary, 18.0),
                )
                .fill(Color32::TRANSPARENT)
                // Ghost like its rail siblings: transparent fill alone
                // still draws the inactive bg_stroke — the gear was the
                // only rail icon inside a bordered box.
                .stroke(egui::Stroke::NONE),
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
                    let (rect, dot) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 4.0, self.palette.ok);
                    // The dot was unlabeled — a critic read it as stray
                    // decoration. It is the connection indicator.
                    let _ = dot.on_hover_text("Connected to Teams");
                    ui.add_space(2.0);
                    // Ghost icon buttons: transparent at rest — the visual
                    // style's inactive fill otherwise paints four chips.
                    let ghost_btn =
                        |ui: &mut egui::Ui, icon: crate::theme::Icon, tint: Color32| {
                            ui.add(
                                egui::Button::new(icon.image(tint, 16.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                        };
                    let tint = self.palette.text;
                    let refresh = ghost_btn(ui, crate::theme::Icon::Refresh, tint)
                        .on_hover_text("Refresh chats");
                    if refresh.clicked() {
                        self.apply(Action::Refresh);
                    }
                    let new_chat = ghost_btn(ui, crate::theme::Icon::Plus, tint)
                        .on_hover_text("New chat (Ctrl+N)");
                    if new_chat.clicked() {
                        self.apply(Action::ShowNewChat);
                    }
                    let search = ghost_btn(ui, crate::theme::Icon::Search, tint)
                        .on_hover_text("Search messages (Ctrl+F)");
                    if search.clicked() {
                        self.apply(Action::OpenSearch);
                    }
                    // Settings lives on the rail (pinned gear at the bottom,
                    // where VSCode/Discord put it) — the second gear here was
                    // a duplicate entry point.
                }
            }
            ui.separator();
            ui.label(&self.status);
            if self.state == State::Ready
                && !self.trouter_on
                && crate::ui::widgets::btn::secondary(ui, &self.palette, "Go live")
                    .clicked()
            {
                self.cmd.send(Command::StartTrouter).ok();
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
            self.hide_window(ui.ctx());
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
                        .fill(crate::theme::tint_over(
                            self.palette.panel,
                            self.palette.warning,
                            0.14,
                        ))
                        .stroke(egui::Stroke::new(
                            1.0,
                            crate::theme::tint_over(
                                self.palette.panel,
                                self.palette.warning,
                                0.47,
                            ),
                        )),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("⚠").color(self.palette.warning));
                        ui.label(
                            RichText::new(
                                "Offline — showing cached data. Messages you send now can't be delivered.",
                            )
                            .small(),
                        );
                        if crate::ui::widgets::btn::secondary(ui, &self.palette, "Retry now")
                            .clicked()
                        {
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
                self.stop_ringer();
            } else {
                let busy = self.call.is_some();
                let palette = &self.palette;
                let mut actions: Vec<Action> = Vec::new();
                // Pulse the border so the ring is noticeable even when the
                // user is reading chats elsewhere.
                let pulse = ((ui.input(|i| i.time) * 3.0).sin() * 0.5 + 0.5) as f32;
                let ring_alpha = (110.0 + 130.0 * pulse) as u8;
                // FLOATING CARD, not a band: the old top Panel pushed the
                // rail, search and chat list down on every ring — an
                // incoming call must never reflow the app. Slides in from
                // above (ease-out cubic, anchored at the ring's start).
                let screen = ui.input(|i| i.raw.screen_rect).unwrap_or(egui::Rect::ZERO);
                let card_w = 460.0;
                let target_y = 40.0;
                let t_in = (ring.at.elapsed().as_secs_f32() / 0.35).min(1.0);
                let ease = 1.0 - (1.0 - t_in) * (1.0 - t_in) * (1.0 - t_in);
                let y = -90.0 + (target_y + 90.0) * ease;
                // Anchored over the CONTENT pane at the top-right (16px from
                // the window edge): native call toasts never cover the
                // navigation rail or the "+ New" button, which the centered
                // placement did at every window size.
                let x = (screen.right() - 16.0 - card_w).max(screen.left() + 8.0);
                egui::Area::new(egui::Id::new("ringing_card"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(x, y))
                    .show(ui.ctx(), |ui| {
                        ui.allocate_ui(egui::vec2(card_w, 70.0), |ui| {
                            egui::Frame::default()
                                // Neutral elevated fill — the red pulse
                                // hairline carries the urgency (a maroon
                                // slab fought the app's restrained surfaces).
                                .fill(self.palette.surface)
                                .stroke(egui::Stroke::new(
                                    1.2,
                                    crate::theme::tint_over(
                                        self.palette.panel,
                                        self.palette.danger,
                                        f32::from(ring_alpha) / 255.0,
                                    ),
                                ))
                                .corner_radius(egui::CornerRadius::same(14))
                                .shadow(egui::Shadow {
                                    offset: [0, 10],
                                    blur: 30,
                                    spread: 0,
                                    color: Color32::from_black_alpha(100),
                                })
                                .inner_margin(egui::Margin::symmetric(14, 10))
                                .show(ui, |ui| {
                        let mut accept = false;
                        let mut decline = false;
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            // Caller avatar disc.
                            let (av, _) = ui.allocate_exact_size(
                                egui::vec2(36.0, 36.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                av.center(),
                                18.0,
                                crate::model::avatar_color(&ring.name),
                            );
                            ui.painter().text(
                                av.center(),
                                egui::Align2::CENTER_CENTER,
                                crate::model::initials(&ring.name),
                                egui::FontId::proportional(13.0),
                                Color32::WHITE,
                            );
                            ui.add_space(2.0);
                            let video = if ring.has_video { " · video" } else { "" };
                            let ring_secs = ring.at.elapsed().as_secs();
                            // Two-line identity: strong caller name, then a
                            // quiet status line (one flat line read as a
                            // status strip, not an interruption).
                            ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&ring.name)
                                            .bold()
                                            .size(13.5),
                                    );
                                ui.label(
                                    RichText::new(format!(
                                        "incoming call{video} · {ring_secs}s"
                                    ))
                                    .small()
                                    .color(self.palette.secondary),
                                );
                            });
                            // Actions pinned right, comfortable targets.
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // RTL: Accept added first = rightmost
                                    // (standard call order); 16px from the
                                    // window edge, 12px between buttons.
                                    ui.add_space(16.0);
                                    // Matched pair: the dark-theme green is
                                    // pale (dark text) — darken the fill so
                                    // BOTH call actions sit at saturated
                                    // depth with white text like Decline.
                                    let ok_luma = 0.2126 * palette.ok.r() as f32
                                        + 0.7152 * palette.ok.g() as f32
                                        + 0.0722 * palette.ok.b() as f32;
                                    let (accept_fill, accept_fg) = if ok_luma > 128.0 {
                                        (palette.ok.gamma_multiply(0.72), Color32::WHITE)
                                    } else {
                                        (palette.ok, Color32::WHITE)
                                    };
                                    let accept_btn = egui::Button::new(
                                        RichText::new("Accept")
                                            .small()
                                            .bold()
                                            .color(accept_fg),
                                    )
                                    .fill(accept_fill)
                                    .min_size(egui::vec2(88.0, 30.0))
                                    .corner_radius(egui::CornerRadius::same(8));
                                    if ui
                                        .add_enabled(!busy, accept_btn)
                                        .clicked()
                                    {
                                        accept = true;
                                    }
                                    ui.add_space(8.0);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Decline")
                                                    .small()
                                                    .bold()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(palette.danger)
                                            .min_size(egui::vec2(88.0, 30.0))
                                            .corner_radius(egui::CornerRadius::same(8)),
                                        )
                                        .clicked()
                                    {
                                        decline = true;
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
                        });
                    });
                for a in actions {
                    self.apply(a);
                }
            }
        }

        if let Some((label, _at)) = self.call.clone() {
            let phase_probe = self
                .call_media
                .as_ref()
                .map(|m| m.phase.borrow().to_string())
                .unwrap_or_default();
            if phase_probe.starts_with("connected") {
                self.call_connected_at.get_or_insert(Instant::now());
            }
            let secs = self.call_connected_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            let mm = secs / 60;
            let ss = secs % 60;
            let mut toggle_stage = false;
            egui::Panel::top("call")
                .frame(
                    egui::Frame::default()
                        .fill(crate::theme::tint_over(
                            self.palette.panel,
                            self.palette.ok,
                            0.12,
                        ))
                        .stroke(egui::Stroke::new(
                            1.0,
                            crate::theme::tint_over(
                                self.palette.panel,
                                self.palette.ok,
                                0.47,
                            ),
                        )),
                )
                .show(ui, |ui| {
                    let phase = self
                        .call_media
                        .as_ref()
                        .map(|m| m.phase.borrow().to_string())
                        .unwrap_or_else(|| "calling…".into());
                    // Meeting joins speak lobby: joining → waiting in the
                    // lobby → (connected = admitted). 1:1 calls keep
                    // calling…/ringing…/connected.
                    let status = if self.call_is_meeting {
                        match self.lobby {
                            ost::api::LobbyState::Lobby => "waiting in the lobby…".to_string(),
                            ost::api::LobbyState::Admitted => phase.clone(),
                            _ => "joining meeting…".to_string(),
                        }
                    } else {
                        phase.clone()
                    };
                    let connected = self.call_connected_at.is_some();
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        // Caller avatar + name: the ringing UI for outgoing
                        // calls was a bare status line before.
                        let init = crate::model::initials(&label);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(22.0, 30.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(rect.center(), 11.0, crate::model::avatar_color(&label));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            init,
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                        if !connected {
                            ui.spinner();
                        }
                        if connected {
                            ui.label(
                                RichText::new(format!(
                                    "{label} — {status} {mm:02}:{ss:02}"
                                ))
                                .small(),
                            );
                        } else {
                            ui.label(RichText::new(format!("{label} — {status}")).small());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Same 96x30 + 8px window margin as the ring
                            // banner buttons.
                            ui.add_space(8.0);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Hang up").small().color(Color32::WHITE),
                                    )
                                    .fill(self.palette.danger)
                                    .min_size(egui::vec2(96.0, 30.0)),
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
                            // Mute from the banner too — voice calls may
                            // never open the stage.
                            if let Some(media) = self.call_media.as_ref() {
                                let mic_on = *media.mic_on.borrow();
                                let mic = ui
                                    .add(
                                        egui::Button::new(
                                            crate::theme::Icon::Mic.image(
                                                if mic_on {
                                                    self.palette.text
                                                } else {
                                                    self.palette.danger
                                                },
                                                15.0,
                                            ),
                                        )
                                        .fill(if mic_on {
                                            Color32::TRANSPARENT
                                        } else {
                                            egui::Color32::from_rgba_unmultiplied(
                                                self.palette.danger.r(),
                                                self.palette.danger.g(),
                                                self.palette.danger.b(),
                                                60,
                                            )
                                        })
                                        .min_size(egui::vec2(30.0, 30.0)),
                                    )
                                    .on_hover_text(if mic_on { "Mute" } else { "Unmute (muted)" });
                                if mic.clicked() {
                                    let _ = media.mic_on.send(!mic_on);
                                }
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
        // Share clicks are collected inside the stage panel (self is
        // borrowed there) and applied once the borrow ends.
        let mut share_clicked = false;
        if self.call_view_open {
            if let (Some((label, _at)), Some(media)) = (self.call.clone(), self.call_media.as_ref())
            {
                // Same connected-gated epoch as the banner — two different
                // durations on screen at once read as a bug.
                let secs = self
                    .call_connected_at
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);
                let (mm, ss) = (secs / 60, secs % 60);
                let phase = media.phase.borrow().to_string();
                egui::Panel::right("call_stage")
                    // exact: default_size collapsed to ~142px on this egui
                    // in some frames, squeezing the tile text.
                    .exact_size(340.0)
                    .show(ui, |ui| {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("●").color(self.palette.ok).size(12.0));
                            // Name + full state already show in the banner;
                            // the stage header carries phase + timer only.
                            ui.label(
                                RichText::new(format!("{phase}  {mm:02}:{ss:02}")).bold(),
                            );
                        });
                        ui.add_space(6.0);
                        let avail_w = (ui.available_width() - 16.0).max(120.0);
                        // Remote tile (or placeholder until frames arrive),
                        // framed so bare video doesn't sit raw on the panel.
                        let tile_h = 300.0_f32;
                        // Meeting joins show the lobby room until admitted.
                        let in_lobby = self.call_is_meeting && !connected_stage(self);
                        if in_lobby {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(avail_w, tile_h * 0.72),
                                egui::Sense::hover(),
                            );
                            let p = ui.painter();
                            p.rect_filled(rect, 8.0, Color32::from_black_alpha(110));
                            // Pulsing dots (Teams-style wait), then the
                            // copy — painter-drawn so everything composites
                            // INSIDE the tile.
                            let t = ui.input(|i| i.time);
                            let cx = rect.center().x;
                            let cy = rect.center().y - 44.0;
                            for i in 0..3 {
                                let phase = (t * 2.2 + i as f64 * 0.45).sin();
                                let alpha = (140.0 + 70.0 * phase) as u8;
                                let r = 4.5 + 1.2 * phase as f32;
                                p.circle_filled(
                                    egui::pos2(cx - 18.0 + i as f32 * 18.0, cy),
                                    r,
                                    Color32::from_rgba_unmultiplied(
                                        pal_ok_r(self),
                                        pal_ok_g(self),
                                        pal_ok_b(self),
                                        alpha,
                                    ),
                                );
                            }
                            p.text(
                                egui::pos2(cx, cy + 26.0),
                                egui::Align2::CENTER_CENTER,
                                if self.lobby == ost::api::LobbyState::Lobby {
                                    "Waiting in the lobby"
                                } else {
                                    "Joining meeting"
                                },
                                egui::FontId::proportional(15.0),
                                self.palette.text,
                            );
                            p.text(
                                egui::pos2(cx, cy + 46.0),
                                egui::Align2::CENTER_CENTER,
                                "Someone in the meeting will let you in.",
                                egui::FontId::proportional(12.0),
                                self.palette.secondary,
                            );
                            ui.add_space(2.0);
                            ui.label(RichText::new(&label).weak().small());
                        } else {
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
                        } // end non-lobby stage
                        // Local camera preview (video calls only), in flow
                        // right under the remote tile. While a screen share
                        // is live this tee carries the SHARED screen, so the
                        // tile doubles as the "what everyone sees" monitor.
                        if media.local_preview.is_some() {
                            ui.add_space(4.0);
                            if self.screen_share == ScreenShare::On {
                                ui.label(
                                    RichText::new("Sharing screen")
                                        .small()
                                        .color(self.palette.accent),
                                );
                            } else if let Some(note) = self.share_note.as_deref() {
                                ui.label(
                                    RichText::new(format!("Share failed: {note}"))
                                        .small()
                                        .color(self.palette.danger),
                                );
                            }
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
                                        if self.screen_share != ScreenShare::Off {
                                            "sharing…"
                                        } else {
                                            "camera…"
                                        },
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
                                            .fill(self.palette.surface_hover)
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
                                        // Screen share (video calls — the share
                                        // rides the video leg). Accent-filled
                                        // while live, matching the native
                                        // client's share-state affordance.
                                        let sharing = self.screen_share == ScreenShare::On;
                                        let starting = self.screen_share == ScreenShare::Starting;
                                        let share = ui.add_enabled(
                                            !starting,
                                            egui::Button::new(
                                                crate::theme::Icon::Monitor.image(
                                                    if sharing {
                                                        self.palette.on_accent
                                                    } else {
                                                        self.palette.secondary
                                                    },
                                                    15.0,
                                                ),
                                            )
                                            .fill(if sharing {
                                                self.palette.accent
                                            } else {
                                                self.palette.surface_hover
                                            })
                                            .min_size(egui::vec2(36.0, 30.0)),
                                        );
                                        if share
                                            .on_hover_text(if starting {
                                                "Opening picker…"
                                            } else if sharing {
                                                "Stop sharing"
                                            } else {
                                                "Share screen"
                                            })
                                            .clicked()
                                        {
                                            share_clicked = true;
                                        }
                                    }
                                    let mic_on = *media.mic_on.borrow();
                                    let mic = ui.add(
                                        egui::Button::new(
                                            crate::theme::Icon::Mic
                                                .image(self.palette.secondary, 15.0),
                                        )
                                        .fill(self.palette.surface_hover)
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
        if share_clicked {
            if self.screen_share == ScreenShare::On {
                self.stop_screen_share();
            } else if self.screen_share == ScreenShare::Off {
                self.begin_screen_share();
            }
        }

        if self.search_open {
            egui::Panel::right("search")
                .default_size(330.0)
                .resizable(true)
                .show(ui, |ui| {
                    let mut actions: Vec<Action> = Vec::new();
                    let close = search_panel(
                        ui,
                        &self.search_hits,
                        self.search_more,
                        self.searching,
                        &self.cmd,
                        &self.search_query,
                        self.search_note.as_deref(),
                        &self.palette,
                        &mut actions,
                    );
                    if close {
                        self.search_open = false;
                    }
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
            // QA hook: force the typing indicator without a live peer.
            // Read once — this line runs every frame and env::var takes a
            // lock + allocates.
            static FORCED_TYPING: std::sync::OnceLock<Option<String>> =
                std::sync::OnceLock::new();
            let forced_typing = FORCED_TYPING
                .get_or_init(|| std::env::var("TEAMSFAST_TYPING").ok())
                .clone();
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
                typing_user: match forced_typing.as_deref() {
                    None | Some("") => self.typing.as_ref().map(|(u, _)| u.as_str()),
                    Some(name) => Some(name),
                },
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
                        // A call needs a human on the other end — bot-only
                        // threads (connector DMs) get no call buttons.
                        // "Any 28: rider" would disable every human 1:1:
                        // Cortana rides along in those rosters.
                        && !self.members.keys().all(|m| m.starts_with("28:"))
                },
                call_label: self.call.as_ref().map(|(l, _)| l.as_str()),
                textures: &self.textures,
                image_sizes: &self.image_sizes,
                pending_images: &mut self.pending_images,
                emoji_textures: &mut self.emoji_textures,
                pal: &self.palette,
                overlays_open: self.new_chat.open
                    || self.forward.is_some()
                    || self.settings_ui.open
                    || self.join_open
                    || self.team_dialog.open,
                actions: &mut actions,
            };
            egui::Panel::bottom("composer").show(ui, |ui| {
                conversation::conversation_composer(ui, &mut ctx, &mut self.draft);
            });
            let bench = std::env::var("TEAMSFAST_SCROLLBENCH").as_deref() == Ok("1");
            let t_bench = (bench).then(std::time::Instant::now);
            egui::CentralPanel::default().show(ui, |ui| {
                conversation::conversation_messages(ui, &mut ctx);
            });
            if let Some(t0) = t_bench {
                log::info!("SCROLLBENCH frame build: {}us", t0.elapsed().as_micros());
            }
            drop(ctx);
            for a in actions {
                self.apply(a);
            }
        } else if chat_family {
            egui::CentralPanel::default().show(ui, |ui| {
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
                                        .bold()
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
                                                RichText::new("Sign in").bold(),
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
                                                .bold()
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
                                // centered_and_justified centers horizontally
                                // only on egui 0.36 — draw the two lines with
                                // the painter so they center vertically too.
                                let rect = ui.max_rect();
                                let c = rect.center();
                                let pal = &self.palette;
                                // Per-view copy: the chat hint ("Ctrl+N")
                                // leaked into every other section's empty
                                // right pane and read as a bug there.
                                let (title, sub, ghost) = match self.main_view {
                                    MainView::Teams => (
                                        "Select a team",
                                        "Pick a team on the left to browse its channels.",
                                        crate::theme::Icon::Users,
                                    ),
                                    MainView::Calendar => (
                                        "Nothing to preview",
                                        "Pick a meeting to see its details.",
                                        crate::theme::Icon::Calendar,
                                    ),
                                    MainView::Files => (
                                        "Select a file",
                                        "Files from your conversations and OneDrive appear here.",
                                        crate::theme::Icon::FileText,
                                    ),
                                    MainView::ToDo => (
                                        "Select a list",
                                        "Pick a list on the left to see its tasks.",
                                        crate::theme::Icon::SquareCheck,
                                    ),
                                    MainView::Activity => (
                                        "You're all caught up",
                                        "Mentions, reactions and replies will show up here.",
                                        crate::theme::Icon::Activity,
                                    ),
                                    // Planner/Shifts/Notes are off the rail;
                                    // if one ever returns, the chat copy is
                                    // the safe fallback.
                                    _ => (
                                        "Select a conversation",
                                        "Pick a chat on the left, or press Ctrl+N for a new one.",
                                        crate::theme::Icon::MessageSquare,
                                    ),
                                };
                                let icon_rect = egui::Rect::from_center_size(
                                    egui::pos2(c.x, c.y - 46.0),
                                    egui::Vec2::splat(40.0),
                                );
                                ui.put(
                                    icon_rect,
                                    egui::Image::from_bytes(ghost.uri(), ghost.bytes())
                                        .tint(pal.dim)
                                        .fit_to_exact_size(egui::Vec2::splat(40.0)),
                                );
                                let mut job = egui::text::LayoutJob::simple(
                                    title.into(),
                                    egui::FontId::proportional(16.0),
                                    pal.text,
                                    f32::INFINITY,
                                );
                                job.sections[0].format.coords =
                                    egui::epaint::text::VariationCoords::new([(b"wght", 700.0)]);
                                let gal =
                                    ui.ctx().fonts_mut(|f| f.layout_job(job));
                                let gw = gal.mesh_bounds.width();
                                let gh = gal.size().y;
                                ui.painter().galley(
                                    egui::pos2(c.x - gw / 2.0, c.y - 10.0 - gh / 2.0),
                                    gal,
                                    pal.text,
                                );
                                let sub = ui.painter().layout(
                                    sub.to_string(),
                                    egui::FontId::proportional(13.0),
                                    pal.secondary,
                                    f32::INFINITY,
                                );
                                let sw = sub.mesh_bounds.width();
                                ui.painter().galley(
                                    egui::pos2(c.x - sw / 2.0, c.y + 12.0 - sub.size().y / 2.0),
                                    sub,
                                    pal.secondary,
                                );
                            }
                        }
                    });
                });
            });
        } else {
            // Section views own the whole central area.
            egui::CentralPanel::default().show(ui, |ui| {
                let mut actions: Vec<Action> = Vec::new();
                match self.main_view {
                    MainView::Calendar => crate::ui::sections::calendar_panel(
                        ui,
                        &self.meetings,
                        self.calendar_loading,
                        &self.palette,
                        &mut actions,
                        &mut self.cal_view,
                        &mut self.cal_week_offset,
                        &mut self.cal_month_offset,
                        &mut self.cal_list_offset,
                        self.calendar_fetched,
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
                            self.chat_files.is_some(),
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
                    // Planner/Shifts/Notes removed from the UI; the arms
                    // remain as a plain note so a stale view enum can't
                    // crash.
                    MainView::Planner | MainView::Shifts | MainView::Notes => {
                        crate::ui::sections::body_note(
                            ui,
                            false,
                            &self.palette,
                            "This section was removed.",
                        );
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
            // Modal, not a floating window: the default window chrome
            // (draggable bar, plain fields, mismatched buttons) read as
            // unfinished next to the join dialog. Same modal frame.
            let pal = self.palette.clone();
            let modal = egui::Modal::new(egui::Id::new("new_chat_modal")).frame(
                egui::Frame::default()
                    .fill(pal.surface)
                    .stroke(egui::Stroke::new(1.0, pal.outline))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(70),
                    })
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(16)),
            );
            let resp = modal.show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                let mut actions = Vec::new();
                new_chat_dialog(ui, &mut self.new_chat, &pal, &mut actions);
                for a in actions {
                    self.apply(a);
                }
            });
            if resp.should_close() {
                self.new_chat.open = false;
            }
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
            (egui::Key::Num6, MainView::Activity),
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
            // Char-boundary safe preview: a byte slice here panics on
            // emoji/CJK at the cut (the calendar en-dash class of crash).
            let chars: Vec<char> = fwd.text.chars().collect();
            let preview = if chars.len() > 80 {
                format!("{}…", chars[..80].iter().collect::<String>())
            } else {
                fwd.text.clone()
            };
            egui::Window::new("Forward message")
                .default_width(380.0)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        egui::RichText::new(format!("“{}”", preview))
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
                    if crate::ui::widgets::btn::secondary(ui, &self.palette, "Cancel").clicked() {
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
                                    RichText::new("Rename").bold().color(self.palette.on_accent),
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
                        if crate::ui::widgets::btn::secondary(ui, &self.palette, "Cancel")
                            .clicked()
                        {
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
                    ui.horizontal(|ui| {
                        if crate::ui::widgets::btn::primary(ui, &self.palette, "Chat")
                            .clicked()
                        {
                            let mri = mri.clone();
                            let name = name.clone();
                            self.apply(Action::ChatWith { mri, name });
                        }
                        if crate::ui::widgets::btn::secondary(ui, &self.palette, "Close")
                            .clicked()
                        {
                            self.contact = None;
                        }
                    });
                });
        }

        // Teams management dialog: create channel / join public team /
        // create team.
        if self.team_dialog.open {
            let modal = egui::Modal::new(egui::Id::new("teams_mgmt_modal")).frame(
                egui::Frame::default()
                    .fill(self.palette.surface)
                    .stroke(egui::Stroke::new(1.0, self.palette.outline))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(70),
                    })
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(16)),
            );
            let mut close_teams = false;
            let resp = modal.show(ui.ctx(), |ui| {
                ui.set_width(430.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Teams").bold().size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    crate::theme::Icon::X.image(self.palette.secondary, 13.0),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(22.0, 22.0)),
                            )
                            .clicked()
                        {
                            close_teams = true;
                        }
                    });
                });
                ui.add_space(6.0);
                let mut actions: Vec<Action> = Vec::new();
                teams_dialog(ui, &mut self.team_dialog, &self.teams, &self.palette, &mut actions);
                for a in actions {
                    self.apply(a);
                }
            });
            if resp.should_close() || close_teams {
                self.team_dialog.open = false;
            }
        }

        // Join-with-link dialog (modal: centered, scrim, Esc/backdrop close).
        if self.join_open {
            let mut join_now = false;
            let mut close = false;
            // Modal::frame REPLACES the default popup frame — rebuild the
            // full look (fill/stroke/shadow) plus the 16px padding.
            let modal = egui::Modal::new(egui::Id::new("join_meeting_modal")).frame(
                egui::Frame::default()
                    .fill(self.palette.surface)
                    .stroke(egui::Stroke::new(1.0, self.palette.outline))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(70),
                    })
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(16)),
            );
            let resp = modal.show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Join a meeting").bold().size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    crate::theme::Icon::X.image(self.palette.secondary, 13.0),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(22.0, 22.0)),
                            )
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
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
                        .desired_width(ui.available_width())
                        // Match the app's standard field height (a 26px
                        // input next to 30px buttons sat undersized).
                        .min_size(egui::vec2(0.0, 30.0)),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(10.0);
                // Actions right-aligned; primary last. Join stays disabled
                // until there is something to join — the New-chat dialog's
                // convention (a saturated button over an empty field read
                // as enabled).
                let can_join = !self.join_source.trim().is_empty();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(can_join, |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Join")
                                        .bold()
                                        .color(self.palette.on_accent),
                                )
                                .fill(self.palette.accent)
                                .min_size(egui::vec2(84.0, 26.0)),
                            )
                            .clicked()
                            || (enter && can_join)
                        {
                            join_now = true;
                        }
                    });
                    if crate::ui::widgets::btn::secondary(ui, &self.palette, "Cancel")
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
                    if crate::ui::widgets::btn::secondary(ui, &self.palette, "Close")
                        .clicked()
                    {
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

        // Repaint policy: the render loop is event-driven — every backend
        // event wakes it directly (event-wake thread). What remains is
        // ticking for TIME-based state, and only while that state is live:
        // call timers, ring pulse/timeout, typing expiry, offline retry,
        // refresh debounce, upload progress, presence poll, and a slow
        // once-a-minute heartbeat while connected (relative timestamps,
        // theme catalog scans) so nothing goes stale when idle.
        let tick = if self.call.is_some() {
            Some(Duration::from_millis(500))
        } else if self.incoming.is_some()
            || self.typing.is_some()
            || self.offline.is_some()
            || self.pending_open_refresh
            || !self.uploads.is_empty()
        {
            Some(Duration::from_millis(250))
        } else if self.state == State::Ready {
            Some(Duration::from_secs(60))
        } else {
            None
        };
        if let Some(t) = tick {
            ui.ctx().request_repaint_after(t);
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn msg(id: &str, ts: &str) -> MessageInfo {
        MessageInfo {
            id: id.into(),
            sender_mri: "8:orgid:a".into(),
            sender: "A".into(),
            timestamp: ts.into(),
            content: format!("content-{id}"),
            raw: format!("<div>content-{id}</div>"),
            reactions: Vec::new(),
            reply_to: None,
            client_message_id: None,
        }
    }

    #[test]
    fn merge_keeps_older_overwrites_overlap_orders_chronologically() {
        let mut existing = vec![
            msg("m1", "2026-10-16T10:00:00.0000000Z"),
            msg("m2", "2026-10-16T10:01:00.0000000Z"),
            msg("m3", "2026-10-16T10:02:00.0000000Z"),
        ];
        // Page = newest slice: m2 EDITED (same id, so replaced), m4 new.
        // m3 is NOT in the page, so by design it SURVIVES (a mid-tail
        // deletion from another client heals on the next full open).
        let mut edited = msg("m2", "2026-10-16T10:01:00.0000000Z");
        edited.content = "edited".into();
        let page = vec![edited, msg("m4", "2026-10-16T10:05:00.0000000Z")];
        merge_message_tail(&mut existing, page);
        let ids: Vec<&str> = existing.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3", "m4"], "older kept, overlap replaced, new appended");
        assert_eq!(existing[1].content, "edited", "overlap takes the page version");

        // A page slice that re-delivers m3 replaces its content (same id)
        // — deletions are never inferable from a slice; they heal on the
        // next FULL refresh (replace) instead.
        let mut m3_new = msg("m3", "2026-10-16T10:02:00.0000000Z");
        m3_new.content = "resurrected".into();
        merge_message_tail(&mut existing, vec![m3_new]);
        assert_eq!(existing.len(), 4, "slice never drops absent ids");
        assert_eq!(existing[2].content, "resurrected");
    }
}
