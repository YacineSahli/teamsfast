//! TeamsFast backend: a tokio worker thread on its own OS thread.
//!
//! The UI pushes [`Command`]s; domain modules under `backend/` implement the
//! protocol work against the `ost` core and report [`Event`]s back. The
//! worker owns the `TeamsClient`; trouter runs as an async task on the same
//! runtime; the event-hub drainer runs on the blocking pool (its Condvar
//! must never block the runtime).

pub mod archive;
pub mod conv;
pub mod directory;
pub mod headless;
pub mod join;
pub mod live_parse;
pub mod live;
pub mod media;
pub mod sections;

pub use headless::list_chats_headless;

use anyhow::Result;
use ost::api::client::TeamsClient;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use tokio::sync::mpsc::UnboundedReceiver;

use ost::api::{list_chats_data, ChatInfo, TeamInfo};

#[derive(Debug, Clone)]
pub enum Command {
    StartLogin,
    CheckReady,
    LoadChats,
    /// Open a chat or channel conversation.
    OpenChat(String),
    /// Light refresh of the open conversation: history page only, no
    /// roster, no UI-side state reset (live-activity refresh path).
    RefreshChat(String),
    /// Send with the caller's clientmessageid (pending-bubble reconciliation).
    Send { chat_id: String, text: String, cmid: String },
    Reply {
        chat_id: String,
        parent_id: String,
        parent_sender: String,
        parent_text: String,
        text: String,
    },
    EditMessage {
        chat_id: String,
        message_id: String,
        text: String,
    },
    DeleteMessage { chat_id: String, message_id: String },
    /// `emoji` is an emoji character; `remove` takes a reaction back.
    React {
        chat_id: String,
        message_id: String,
        emoji: String,
        remove: bool,
    },
    /// One older page of the conversation.
    LoadOlder(String),
    MarkRead { chat_id: String, message_id: String },
    Search { query: String, from: usize },
    LoadTeams,
    FetchImage(String),
    /// Persist a decoded image's size for layout reservation next open.
    SaveImageSize { url: String, w: usize, h: usize },
    UploadFile { chat_id: String, path: PathBuf },
    /// Download a shared file to ~/Downloads and open it.
    DownloadFile { url: String, name: String },
    CreateOneToOne(String),
    CreateGroup { topic: String, members: Vec<String> },
    StartTrouter,
    /// Wipe tokens (keyring + file) and disconnect — the UI returns to the
    /// sign-in screen.
    SignOut,
    /// Empty the local encrypted archive (chats + messages).
    ClearArchive,
    /// Row counts of the local archive (for the settings screen).
    ArchiveStats,
    /// Fetch our presence (poll).
    PollPresence,
    /// Set our presence ("available" | "brb" | "busy" | "dnd" | "away" | "offline").
    SetPresence(String),
    /// Chat row ops: server-side mute / hide / leave.
    SetChatMuted { chat_id: String, muted: bool },
    SetChatHidden(String),
    LeaveChat(String),
    /// Roll the local read horizon back (chat shows unread).
    MarkUnread(String),
    /// Fetch who-read-what for a chat (consumption horizons).
    ReadReceipts(String),
    // ---- sections ----
    /// Section loaders off the rail (product call) — kept for the return.
    #[allow(dead_code)]
    LoadCalendar,
    /// Meetings for an arbitrary window (week/month navigation).
    LoadCalendarRange { start_unix: u64, days: u64 },
    LoadFiles,
    LoadTodo,
    LoadTodoTasks(String),
    SetTodoDone {
        list_id: String,
        task_id: String,
        done: bool,
    },
    AddTodoTask {
        list_id: String,
        title: String,
    },
    /// Download a OneDrive item to ~/Downloads and open it.
    DownloadDriveFile {
        drive_id: String,
        item_id: String,
        name: String,
    },
    // ---- calls ----
    /// Place a 1:1 audio call to the peer of this chat thread.
    StartCall(String),
    /// Place a 1:1 call with the camera on (video modality in the invite;
    /// H.264 frames streamed from the default V4L2 device).
    StartVideoCall(String),
    /// Ring the Teams echo/test bot (settings "Test call"); `video` adds
    /// the camera (QA: TEAMSFAST_VIDEOCALL=1).
    TestCall { video: bool },
    /// Join a meeting from a URL / thread id / meet ID.
    JoinMeeting {
        source: String,
        camera: bool,
        mic: bool,
    },
    /// Open the camera for the pre-join dialog preview.
    JoinPreviewStart,
    /// Stop the preview capture (dialog closed).
    JoinPreviewStop,
    /// Accept the ringing incoming call (raw invitation JSON from
    /// [`Event::IncomingCall`]).
    AcceptIncoming(String),
    /// Decline the ringing incoming call (raw invitation JSON).
    DeclineIncoming(String),
    /// Hang up the active call.
    HangUp,
    // ---- teams management ----
    CreateChannel { team_id: String, name: String },
    SearchPublicTeams(String),
    JoinTeam(String),
    CreateTeam(String),
    RenameChannel {
        team_id: String,
        channel_id: String,
        name: String,
    },
    DeleteChannel {
        team_id: String,
        channel_id: String,
    },
    /// Recent files shared in one chat.
    ChatFiles(String),
    // ---- onenote ---- (off the rail; kept for the return)
    #[allow(dead_code)]
    LoadNotes,
    ReadNotePage { page_id: String },
    AppendNote { page_id: String, text: String },
    // ---- meet now ----
    MeetNow,
    // ---- planner + shifts ----
    #[allow(dead_code)]
    LoadPlanner,
    #[allow(dead_code)]
    LoadShifts,
    SetPlannerDone {
        task_id: String,
        etag: String,
        done: bool,
    },
}

#[derive(Debug, Clone)]
pub enum MsgAction {
    Edited,
    Deleted,
    Reacted,
    Replied,
    /// Read-state events arrive via the server horizon today.
    #[allow(dead_code)]
    MarkedRead,
}

pub enum Event {
    Status(String),
    LoginResult(Result<(), String>),
    /// Device-code login: the URL + code to show in the GUI.
    LoginCode { url: String, code: String },
    Ready,
    /// Cached-history activity (mentions/replies) mined once at startup.
    ActivitySeeded(Vec<crate::model::ActivityEntry>),
    /// Known image sizes from the archive (url -> w,h) for placeholder
    /// reservation; sent once at startup.
    ImageSizes(HashMap<String, (usize, usize)>),
    /// Our own display name (Graph whoami).
    SelfName(String),
    /// Our own Entra object id (for own-message detection).
    SelfId(String),
    /// Network unavailable (or token refresh failed) but cached content
    /// exists — the UI stays usable instead of demanding a sign-in.
    Offline(String),
    NeedLogin(String),
    Chats(Vec<ChatInfo>),
    /// Cache fallback for the chat list — the UI merges it only when it
    /// has nothing fresher (startup / offline), never clobbering a
    /// live-updated list with a stale archive snapshot.
    ChatsCached(Vec<ChatInfo>),
    Messages {
        chat_id: String,
        messages: Vec<MessageInfo>,
        /// Roster of the open chat: mri → display name (best effort).
        members: HashMap<String, String>,
        /// Roster-resolved name for a placeholder-titled chat.
        resolved_name: Option<String>,
        /// True = prepend these (older page); false = replace the view.
        prepend: bool,
        /// True = the page is a SMALL confirmation slice from an action
        /// (send/reply/edit/delete/react): merge by id instead of
        /// replacing, so loaded older pages survive. Only ids present in
        /// the slice update — other-client deletions heal on full refresh.
        merge: bool,
        /// Cursor for "load older"; None when history is exhausted.
        older_link: Option<String>,
    },
    /// An action (edit/delete/react/reply/mark-read) succeeded; refetch.
    /// chat_id rides along for future per-chat scoping; unread today.
    ActionOk {
        #[allow(dead_code)]
        chat_id: String,
        action: MsgAction,
    },
    SearchResults {
        #[allow(dead_code)]
        query: String,
        hits: Vec<ost::api::SearchHitInfo>,
        more: bool,
    },
    Teams(Vec<TeamInfo>),
    CreateChatOk(ChatInfo),
    /// A decoded image ready for texturing on the UI thread.
    ImageReady {
        url: String,
        rgba: Vec<u8>,
        size: [usize; 2],
    },
    /// An image failed to load (show a broken-image glyph).
    ImageFailed(String),
    UploadProgress {
        #[allow(dead_code)]
        chat_id: String,
        name: String,
        sent: u64,
        total: u64,
    },
    UploadDone {
        chat_id: String,
        #[allow(dead_code)]
        name: String,
    },
    DownloadDone {
        #[allow(dead_code)]
        name: String,
        path: PathBuf,
    },
    TrouterConnected,
    /// Message search refused by Graph on this tenant (token lacks Chat.Read).
    SearchUnavailable(String),
    /// Parsed live message push (notifications + chat-list updates).
    IncomingMessage {
        chat_id: String,
        sender: String,
        preview: String,
    },
    /// Someone is typing in a chat.
    Typing { chat_id: String, user: String },
    /// Unread state per chat: (is-unread, approximate count from cache).
    /// Count 0 with unread=true means the messages aren't cached yet.
    Unread(std::collections::HashMap<String, (bool, u32)>),
    /// Archive row counts for the settings screen.
    ArchiveStats(usize, usize),
    /// Our own presence availability (e.g. "Available", "Away").
    MyPresence(String),
    /// A send definitively failed (network error or the server dropped it);
    /// the UI marks the pending bubble with a Retry.
    SendFailed {
        chat_id: String,
        cmid: String,
        error: String,
    },
    /// Read receipts for a chat: (user mri, last-read message id).
    ReadReceipts {
        chat_id: String,
        receipts: Vec<(String, String)>,
    },
    // ---- sections ----
    /// Meetings for the coming week (soonest first).
    Calendar(Vec<ost::api::MeetingInfo>),
    /// Recent OneDrive files.
    Files(Vec<ost::api::SharedFile>),
    /// Our To Do lists.
    TodoLists(Vec<ost::api::TodoListInfo>),
    /// Tasks of one To Do list.
    TodoTasks {
        list_id: String,
        tasks: Vec<ost::api::TodoTaskInfo>,
    },
    // ---- calls ----
    /// A call was placed and is connecting.
    CallStarted { label: String },
    /// Human-readable call progress (connecting / ringing / active).
    CallStatus(String),
    /// The call ended (summary line).
    CallEnded(String),
    /// The call could not be placed.
    CallFailed(String),
    /// An incoming call is ringing; `raw` is the invitation JSON (accept/
    /// decline re-parse it backend-side for the signaling links).
    IncomingCall {
        caller_name: String,
        caller_mri: String,
        has_video: bool,
        raw: String,
    },
    /// A call-signalling callback announced an end (caller cancelled while
    /// ringing; the accepted-call case is handled conservatively in the UI).
    CallGone,
    /// Live call controls for the call stage UI (mute/camera toggles,
    /// remote + local frame receivers). One per placed call.
    CallMedia(ost::calling::CallControlsHandle),
    /// Pre-join preview frames (None state = camera unavailable).
    JoinPreview(std::sync::mpsc::Receiver<ost::calling::video::VideoFrame>),
    JoinPreviewUnavailable,
    /// The roster resolved a chat's display name that differs from the
    /// chat-list entry (e.g. bot-carrying 1:1s) — sidebar update.
    ChatRenamed { chat_id: String, name: String },
    /// Public-team search results (join picker).
    PublicTeams(Vec<ost::api::PublicTeamInfo>),
    /// Planner boards: (team, plan, buckets, tasks).
    Planner(Vec<crate::ui::sections::PlannerBoard>),
    /// This week's shifts.
    Shifts(Vec<ost::api::ShiftInfo>),
    /// Files shared in one chat.
    ChatFiles {
        chat_id: String,
        files: Vec<ost::api::SharedFile>,
    },
    /// OneNote notebooks with sections+pages (personal notes).
    Notes(Vec<crate::ui::sections::NotebookTree>),
    /// One page's content.
    NotePage(ost::api::NotePage),
    /// Meet-now created; join with its link.
    MeetNowReady { join_url: String },
    Error(String),
}

use ost::api::MessageInfo;

/// Spawn the worker; returns the command sender plus a wake receiver that
/// yields one `()` per event sent to the UI. The UI parks a thread on it
/// calling `request_repaint`, so backend activity paints immediately while
/// the render loop itself stays event-driven (no polling repaint).
pub fn spawn(
    tx: Sender<Event>,
) -> (
    tokio::sync::mpsc::UnboundedSender<Command>,
    std::sync::mpsc::Receiver<()>,
) {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
    // Forwarder: worker → poke → UI sender. One extra channel hop per
    // event (events are low-rate) buys instant repaints with zero idle cost.
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<Event>();
    let (poke_tx, poke_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("event-wake".into())
        .spawn(move || {
            for ev in ev_rx {
                let _ = poke_tx.send(());
                if tx.send(ev).is_err() {
                    return; // UI gone
                }
            }
        })
        .expect("spawn event-wake thread");
    std::thread::Builder::new()
        .name("teamsfast-backend".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(worker(cmd_rx, ev_tx));
        })
        .expect("spawn backend thread");
    (cmd_tx, poke_rx)
}

/// Sort key for the chat list: last-activity time, newest first.
pub fn chat_recency(chat: &ChatInfo) -> u64 {
    chat.last_message_time
        .as_deref()
        .and_then(crate::model::to_epoch_ms)
        .unwrap_or(0)
}

/// Drop Teams' activity feeds (`48:*` ids): they accept no sends.
pub fn filter_pseudo_chats(chats: &mut Vec<ChatInfo>) {
    chats.retain(|c| !c.id.starts_with("48:"));
}

fn sort_chats(chats: &mut [ChatInfo]) {
    chats.sort_by(|a, b| chat_recency(b).cmp(&chat_recency(a)));
}

/// Session state shared by the domain handlers.
pub(crate) struct Session {
    pub client: Option<TeamsClient>,
    pub self_id: Option<String>,
    /// Backward page cursor per open conversation.
    pub older_links: HashMap<String, String>,
    /// Local archive (opens even without network, for cached content).
    pub archive: Option<archive::Archive>,
    /// Roster (members, resolved 1:1 name) per chat with fetch time —
    /// skips the Graph name-fill fan-out on re-opens within its TTL.
    pub roster_cache:
        HashMap<String, (std::time::Instant, HashMap<String, String>, Option<String>)>,
    /// Activity seeding runs once per process (it re-runs on every
    /// offline-retry recovery otherwise).
    activity_seeded: bool,
    /// Consecutive presence-poll failures — some tenants 403 the Graph
    /// presence API outright; the poll backs off to one attempt per 10 min
    /// instead of burning an RTT every minute for a permanent error.
    presence_failures: u32,
    presence_last_try: Option<std::time::Instant>,
    /// Hang-up switch + liveness for the active call (None when idle).
    /// `done` is set by the call task's RAII guard — on normal exit AND
    /// on panic unwind — so a call that ended without an explicit HangUp
    /// (remote hang-up, safety-cap timer, crashed task) can be reaped
    /// instead of wedging "a call is already in progress" forever.
    pub call_stop: Option<CallSlot>,
    /// Pre-join camera capture (join dialog preview). Dropping the UI-side
    /// receiver unwinds the whole chain: forwarder exits on send-failure,
    /// drops the camera rx, the capture thread exits on ITS send-failure.
    pub join_cam: Option<ost::calling::camera::CameraCapture>,
}

/// The worker-side handle to one running call task.
pub struct CallSlot {
    pub stop: tokio::sync::watch::Sender<bool>,
    pub done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Held by the spawned call task; Drop marks the slot done (unwind-safe).
struct CallDoneGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for CallDoneGuard {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Reap a finished call slot, if any. Returns true when the caller may
/// place a new call; false means one is genuinely still running.
fn call_slot_free(ses: &mut Session) -> bool {
    match &ses.call_stop {
        Some(slot) if !slot.done.load(std::sync::atomic::Ordering::Relaxed) => false,
        _ => {
            ses.call_stop = None;
            true
        }
    }
}

impl Session {
    fn new() -> Self {
        let archive = match archive::Archive::open() {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("archive unavailable ({e:#}); running without persistence");
                None
            }
        };
        Self {
            client: None,
            self_id: None,
            older_links: HashMap::new(),
            archive,
            roster_cache: HashMap::new(),
            activity_seeded: false,
            presence_failures: 0,
            presence_last_try: None,
            call_stop: None,
            join_cam: None,
        }
    }
}

/// Offline fallback: deliver the CACHED chat list (so the sidebar
/// paints instead of sitting empty) plus the offline banner. Returns
/// true when cached content was delivered.
fn send_cached_chats(ses: &Session, tx: &Sender<Event>, why: &str, banner: bool) -> bool {
    let Some(archive) = ses.archive.as_ref() else {
        return false;
    };
    let Ok(mut chats) = archive.load_chats() else {
        return false;
    };
    if chats.is_empty() {
        return false;
    }
    sort_chats(&mut chats);
    let _ = tx.send(Event::ChatsCached(chats));
    if banner {
        let _ = tx.send(Event::Offline(format!(
            "Offline — showing cached data ({why})"
        )));
    } else {
        // Mid-session endpoint failure: a status note, not the offline
        // banner — the banner drives a 20s CheckReady retry loop meant
        // for real offline, and it used to latch (nothing cleared it).
        let _ = tx.send(Event::Status(format!(
            "chat list unreachable — showing cached ({why})"
        )));
    }
    true
}

/// Token/session setup failed. With cached content that is an OFFLINE
/// situation (stay usable), not a sign-in demand.
fn offline_or_login(ses: &Session, tx: &Sender<Event>, error: String) {
    if !send_cached_chats(ses, tx, &error, true) {
        let _ = tx.send(Event::NeedLogin(error));
    }
}

/// Seed the Activity feed from cached history: mentions of us (@items
/// referencing our Entra id) and replies. Live events keep appending on
/// top. Quiet at startup is what made the Activity tab read as broken.
/// Once per process — offline-retry recoveries re-fire `Ready`, and the
/// seed never changes within a session.
fn seed_activity(ses: &mut Session, tx: &Sender<Event>) {
    use crate::model::ActivityEntry;
    if ses.activity_seeded {
        return;
    }
    let Some(archive) = ses.archive.as_ref() else {
        return;
    };
    let Ok(chats) = archive.load_chats() else {
        return;
    };
    let self_id = ses.self_id.clone().unwrap_or_default();
    let mut entries: Vec<ActivityEntry> = Vec::new();
    for chat in chats.iter().take(15) {
        // Mentions/replies only, prefiltered in SQLite — deserializing
        // 15×50 multi-KB rows to scan them in Rust was the old shape.
        let Ok(msgs) = archive.activity_candidates(&chat.id, 50) else {
            continue;
        };
        for m in msgs {
            let mention = !self_id.is_empty() && m.raw.contains("<at") && m.raw.contains(&self_id);
            let reply = m.reply_to.is_some();
            if !mention && !reply {
                continue;
            }
            let preview: String = m
                .content
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(140)
                .collect();
            entries.push(ActivityEntry {
                kind: if mention { "mention" } else { "reply" },
                chat_id: chat.id.clone(),
                chat_name: if chat.name.is_empty() || chat.name == "[Direct message]" {
                    "Direct message".to_string()
                } else {
                    chat.name.clone()
                },
                who: m.sender.clone(),
                preview,
                at_ms: crate::model::to_epoch_ms(&m.timestamp).unwrap_or(0),
            });
        }
    }
    ses.activity_seeded = true;
    if entries.is_empty() {
        return;
    }
    entries.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
    entries.truncate(200);
    let _ = tx.send(Event::ActivitySeeded(entries));
}

/// Commands that must run REGARDLESS of connection state: auth/recovery,
/// call + preview teardown, cache-backed loads with their own offline
/// fallback, and local-only archive writes. Everything else hits the
/// network and is gated by the central offline guard below.
fn runs_offline(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::StartLogin
            | Command::CheckReady
            | Command::LoadChats
            | Command::StartTrouter
            | Command::SignOut
            | Command::ClearArchive
            | Command::ArchiveStats
            | Command::HangUp
            | Command::JoinPreviewStart
            | Command::JoinPreviewStop
            | Command::SaveImageSize { .. }
            // Purely local: zeroes the archive read-horizon (audit find).
            | Command::MarkUnread(_)
    )
}

async fn worker(mut rx: UnboundedReceiver<Command>, tx: Sender<Event>) {
    let mut ses = Session::new();
    let mut trouter_started = false;

    macro_rules! send {
        ($ev:expr) => {{
            let _ = tx.send($ev);
        }};
    }

    // Persisted image sizes -> the UI reserves final boxes on open.
    if let Some(archive) = ses.archive.as_ref()
        && let Ok(sizes) = archive.load_image_sizes()
        && !sizes.is_empty()
    {
        let _ = tx.send(Event::ImageSizes(sizes));
    }

    while let Some(cmd) = rx.recv().await {
        // `.await` on recv parks the root task properly, so spawned tasks
        // (trouter, drainer) run while the queue is idle.

        // Central offline guard: ~40 handlers had a silent
        // `client(ses) else return` — a click that did NOTHING read as a
        // dead UI. Network commands now answer with one status note; the
        // per-handler guards stay as backstops (this `continue` fires
        // first, so no handler double-reports).
        if ses.client.is_none() && !runs_offline(&cmd) {
            send!(Event::Status("offline — needs a connection".into()));
            continue;
        }
        match cmd {
            Command::StartLogin => {
                send!(Event::Status("Requesting a sign-in code…".into()));
                let tx_code = tx.clone();
                let res = ost::auth::login_with_code_sink(true, move |url, code| {
                    // Show the code in the GUI (still printed by `login` for
                    // the terminal users via tracing).
                    let _ = tx_code.send(Event::LoginCode {
                        url: url.to_string(),
                        code: code.to_string(),
                    });
                })
                .await
                .map_err(|e| format!("{e:#}"));
                send!(Event::LoginResult(res));
                match ready_session(&mut ses).await {
                    Ok((name, id)) => {
                        if let Some(n) = name {
                            send!(Event::SelfName(n));
                        }
                        if let Some(id) = id {
                            send!(Event::SelfId(id));
                        }
                        send!(Event::Ready);
                        seed_activity(&mut ses, &tx);
                    }
                    Err(e) => offline_or_login(&ses, &tx, e),
                }
            }
            Command::CheckReady => match ready_session(&mut ses).await {
                Ok((name, id)) => {
                    if let Some(n) = name {
                        send!(Event::SelfName(n));
                    }
                    if let Some(id) = id {
                        send!(Event::SelfId(id));
                    }
                    send!(Event::Ready);
                    seed_activity(&mut ses, &tx);
                }
                Err(e) => offline_or_login(&ses, &tx, e),
            },
            Command::LoadChats => {
                let Some(c) = ses.client.as_ref() else {
                    send!(Event::Offline("offline — showing cached data".into()));
                    continue;
                };
                match list_chats_data(c, 40).await {
                    Ok(mut chats) => {
                        filter_pseudo_chats(&mut chats);
                        sort_chats(&mut chats);
                        if let Some(archive) = ses.archive.as_ref() {
                            if let Err(e) = archive.save_chats(&chats) {
                                log::warn!("archive save chats: {e:#}");
                            }
                        }
                        let unread = unread_map(&ses, &chats);
                        send!(Event::Chats(chats));
                        send!(Event::Unread(unread));
                    }
                    Err(e) => {
                        // Offline (or the endpoint chain failed): the
                        // cached list keeps the sidebar alive; the raw
                        // error string only when there is nothing cached.
                        if !send_cached_chats(&ses, &tx, "chat list fetch failed", false) {
                            send!(Event::Error(format!("chat list: {e:#}")));
                        }
                    }
                }
            }
            Command::OpenChat(chat_id) => {
                conv::open_chat(&mut ses, &tx, chat_id).await;
            }
            Command::RefreshChat(chat_id) => {
                conv::refetch(&ses, &tx, &chat_id).await;
            }
            Command::Send { chat_id, text, cmid } => {
                conv::send(&mut ses, &tx, &chat_id, &text, &cmid).await;
            }
            Command::Reply {
                chat_id,
                parent_id,
                parent_sender,
                parent_text,
                text,
            } => {
                conv::reply(&ses, &tx, &chat_id, &parent_id, &parent_sender, &parent_text, &text)
                    .await;
            }
            Command::EditMessage {
                chat_id,
                message_id,
                text,
            } => {
                conv::edit(&ses, &tx, &chat_id, &message_id, &text).await;
            }
            Command::DeleteMessage {
                chat_id,
                message_id,
            } => {
                conv::delete(&ses, &tx, &chat_id, &message_id).await;
            }
            Command::React {
                chat_id,
                message_id,
                emoji,
                remove,
            } => {
                conv::react(&ses, &tx, &chat_id, &message_id, &emoji, remove).await;
            }
            Command::LoadOlder(chat_id) => {
                conv::load_older(&mut ses, &tx, &chat_id).await;
            }
            Command::MarkRead { chat_id, message_id } => {
                conv::mark_read(&ses, &tx, &chat_id, &message_id).await;
            }
            Command::Search { query, from } => {
                directory::search(&ses, &tx, &query, from).await;
            }
            Command::LoadTeams => {
                directory::teams(&ses, &tx).await;
            }
            Command::FetchImage(url) => {
                media::fetch_image(&ses, &tx, &url).await;
            }
            Command::SaveImageSize { url, w, h } => {
                if let Some(archive) = ses.archive.as_ref()
                    && let Err(e) = archive.save_image_size(&url, w, h)
                {
                    log::debug!("archive save image size: {e:#}");
                }
            }
            Command::UploadFile { chat_id, path } => {
                media::upload(&ses, &tx, &chat_id, &path).await;
            }
            Command::DownloadFile { url, name } => {
                media::download(&ses, &tx, &url, &name).await;
            }
            Command::CreateOneToOne(user) => {
                directory::create_one_to_one(&ses, &tx, &user).await;
            }
            Command::CreateGroup { topic, members } => {
                directory::create_group(&ses, &tx, &topic, &members).await;
            }
            Command::StartTrouter => {
                if trouter_started {
                    continue;
                }
                trouter_started = true;
                live::start(&tx);
                send!(Event::TrouterConnected);
            }
            Command::SignOut => {
                // Wipe tokens from every store (keyring included) and drop
                // the session; the cache file only survives when the
                // keyring is unavailable (never lose tokens rule).
                match ost::config::Config::delete_for(ost::config::DEFAULT_PROFILE) {
                    Ok(_) => log::info!("tokens wiped for sign-out"),
                    Err(e) => log::warn!("token wipe failed: {e:#}"),
                }
                ost::config::Config::invalidate_cache();
                ses.client = None;
                ses.self_id = None;
                ses.older_links.clear();
                send!(Event::NeedLogin("Signed out".into()));
            }
            Command::ClearArchive => {
                if let Some(a) = ses.archive.as_mut() {
                    match a.wipe() {
                        Ok(n) => send!(Event::Status(format!(
                            "local archive cleared ({n} rows)"
                        ))),
                        Err(e) => send!(Event::Error(format!("clear archive: {e:#}"))),
                    }
                }
            }
            Command::ArchiveStats => {
                if let Some(a) = ses.archive.as_ref()
                    && let Ok((chats, messages)) = a.stats()
                {
                    send!(Event::ArchiveStats(chats, messages));
                }
            }
            Command::PollPresence => {
                // Back off after repeated failures: a tenant that forbids
                // the presence API would otherwise pay an RTT every minute
                // forever. Three strikes → one attempt per 10 minutes; a
                // success resets immediately.
                if ses.presence_failures >= 3
                    && ses
                        .presence_last_try
                        .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(600))
                {
                    continue;
                }
                ses.presence_last_try = Some(std::time::Instant::now());
                let Some(c) = ses.client.as_ref() else {
                    continue;
                };
                let res = ost::api::get_presence_data(c).await;
                match res {
                    Ok(p) => {
                        ses.presence_failures = 0;
                        send!(Event::MyPresence(p.availability));
                    }
                    Err(e) => {
                        ses.presence_failures += 1;
                        log::debug!(
                            "presence poll (strike {}): {e:#}",
                            ses.presence_failures
                        )
                    }
                }
            }
            Command::SetPresence(status) => {
                let Some(c) = ses.client.as_ref() else {
                    continue;
                };
                match ost::api::set_presence_with_client(c, &status).await {
                    Ok(()) => {
                        send!(Event::Status(format!("status set: {status}")));
                        if let Ok(p) = ost::api::get_presence_data(c).await {
                            send!(Event::MyPresence(p.availability));
                        }
                    }
                    Err(e) => send!(Event::Error(format!("set status: {e:#}"))),
                }
            }
            Command::SetChatMuted { chat_id, muted } => {
                directory::set_muted(&ses, &tx, &chat_id, muted).await;
            }
            Command::SetChatHidden(chat_id) => {
                directory::set_hidden(&ses, &tx, &chat_id).await;
            }
            Command::LeaveChat(chat_id) => {
                directory::leave_chat(&ses, &tx, &chat_id).await;
            }
            Command::MarkUnread(chat_id) => {
                directory::mark_unread(&ses, &chat_id).await;
            }
            Command::ReadReceipts(chat_id) => {
                conv::fetch_receipts(&ses, &tx, &chat_id).await;
            }
            // ---- sections ----
            Command::LoadCalendar => {
                sections::calendar(&ses, &tx).await;
            }
            Command::LoadCalendarRange { start_unix, days } => {
                sections::calendar_range(&ses, &tx, start_unix, days).await;
            }
            Command::LoadFiles => {
                sections::files(&ses, &tx).await;
            }
            Command::LoadTodo => {
                sections::todo_lists(&ses, &tx).await;
            }
            Command::LoadTodoTasks(list_id) => {
                sections::todo_tasks(&ses, &tx, &list_id).await;
            }
            Command::SetTodoDone {
                list_id,
                task_id,
                done,
            } => {
                sections::todo_set_done(&ses, &tx, &list_id, &task_id, done).await;
            }
            Command::AddTodoTask { list_id, title } => {
                sections::todo_add(&ses, &tx, &list_id, &title).await;
            }
            Command::DownloadDriveFile {
                drive_id,
                item_id,
                name,
            } => {
                sections::download_drive_file(&ses, &tx, &drive_id, &item_id, &name).await;
            }
            // ---- calls ----
            Command::StartCall(chat_id) => {
                start_call(&mut ses, &tx, chat_id, false, false, true).await;
            }
            Command::StartVideoCall(chat_id) => {
                start_call(&mut ses, &tx, chat_id, false, true, true).await;
            }
            Command::TestCall { video } => {
                start_call(&mut ses, &tx, String::new(), true, video, true).await;
            }
            Command::CreateChannel { team_id, name } => {
                sections::create_channel(&ses, &tx, &team_id, &name).await;
            }
            Command::SearchPublicTeams(query) => {
                sections::search_public_teams(&ses, &tx, &query).await;
            }
            Command::JoinTeam(team_id) => {
                sections::join_team(&ses, &tx, &team_id).await;
            }
            Command::CreateTeam(name) => {
                sections::create_team(&ses, &tx, &name).await;
            }
            Command::RenameChannel {
                team_id,
                channel_id,
                name,
            } => {
                sections::rename_channel(&ses, &tx, &team_id, &channel_id, &name).await;
            }
            Command::DeleteChannel { team_id, channel_id } => {
                sections::delete_channel(&ses, &tx, &team_id, &channel_id).await;
            }
            Command::ChatFiles(chat_id) => {
                sections::chat_files(&ses, &tx, &chat_id).await;
            }
            Command::LoadNotes => {
                sections::load_notes(&ses, &tx).await;
            }
            Command::ReadNotePage { page_id } => {
                sections::read_note_page(&ses, &tx, &page_id).await;
            }
            Command::AppendNote { page_id, text } => {
                sections::append_note(&ses, &tx, &page_id, &text).await;
            }
            Command::MeetNow => {
                sections::meet_now(&ses, &tx).await;
            }
            Command::LoadPlanner => {
                sections::load_planner(&ses, &tx).await;
            }
            Command::LoadShifts => {
                sections::load_shifts(&ses, &tx).await;
            }
            Command::SetPlannerDone {
                task_id,
                etag,
                done,
            } => {
                sections::planner_set_done(&ses, &tx, &task_id, &etag, done).await;
            }
            Command::JoinMeeting { source, camera, mic } => {
                if let Some((thread, label)) = join::resolve_join_target(&ses, &tx, &source).await
                {
                    start_call_labelled(&mut ses, &tx, thread, false, label, camera, mic).await;
                }
            }
            Command::JoinPreviewStart => {
                if ses.join_cam.is_some() {
                    continue; // already previewing
                }
                let started = tokio::task::spawn_blocking(|| {
                    ost::calling::camera::CameraCapture::start(None, 320, 240, 15)
                })
                .await;
                match started {
                    Ok(Ok((cap, cam_rx))) => {
                        ses.join_cam = Some(cap);
                        let (ui_tx, ui_rx) = std::sync::mpsc::sync_channel(2);
                        // Forwarder: YUV frames -> VideoFrame for the UI.
                        // Exits when the UI side drops its receiver.
                        std::thread::spawn(move || {
                            while let Ok(f) = cam_rx.recv() {
                                let vf = ost::calling::video::VideoFrame {
                                    width: f.width,
                                    height: f.height,
                                    data: f.data,
                                };
                                if ui_tx.send(vf).is_err() {
                                    break;
                                }
                            }
                        });
                        send!(Event::JoinPreview(ui_rx));
                    }
                    _ => {
                        // Camera absent/busy: the dialog shows the ghost
                        // tile; joining can still flip the camera on (the
                        // call degrades to black frames per teams-core).
                        send!(Event::JoinPreviewUnavailable);
                    }
                }
            }
            Command::JoinPreviewStop => {
                ses.join_cam = None; // chain unwinds via the UI rx drop
            }
            Command::AcceptIncoming(raw) => {
                accept_incoming(&mut ses, &tx, raw).await;
            }
            Command::DeclineIncoming(raw) => {
                decline_incoming(raw).await;
            }
            Command::HangUp => {
                if let Some(slot) = ses.call_stop.take() {
                    let _ = slot.stop.send(true);
                    send!(Event::CallStatus("hanging up…".into()));
                }
            }
        }
    }
}

/// Place a call through teams-core's call driver: its own trouter session,
/// ICE/TURN, SRTP/Opus and cpal audio, all inside one spawned task. The
/// watch channel hangs up cooperatively. `video` turns the camera on for
/// the whole call (320x240@15 H.264 send; there is no mid-call toggle —
/// renegotiation is future work).
async fn start_call(
    ses: &mut Session,
    tx: &Sender<Event>,
    chat_id: String,
    echo: bool,
    video: bool,
    mic: bool,
) {
    let label = if echo { "Echo test".to_string() } else { chat_id.clone() };
    start_call_labelled(ses, tx, chat_id, echo, label, video, mic).await;
}

/// [`start_call`] with a display label (meeting subject, chat name, …).
async fn start_call_labelled(
    ses: &mut Session,
    tx: &Sender<Event>,
    chat_id: String,
    echo: bool,
    label: String,
    video: bool,
    mic: bool,
) {
    if !call_slot_free(ses) {
        let _ = tx.send(Event::CallFailed("a call is already in progress".into()));
        return;
    }
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    ses.call_stop = Some(CallSlot {
        stop: stop_tx,
        done: done.clone(),
    });
    let tx_evt = tx.clone();
    let thread = if echo { None } else { Some(chat_id) };
    tokio::spawn(async move {
        // First statement: Drop marks the slot done when THIS task ends
        // (normal return or panic unwind).
        let _done_guard = CallDoneGuard(done);
        let _ = tx_evt.send(Event::CallStarted { label });
        // No static status text: the CallMedia handle's live phase
        // (calling…/ringing…/connected) drives the banner.
        // Live call controls (mute/camera gates + frame sinks for the
        // stage UI); the handle rides to the UI as an event.
        // Preview tee ALWAYS on: it mirrors the encode input (camera OR
        // screen share), so a mid-call camera flip or a share from an
        // audio-placed leg still surfaces frames to the stage's monitor
        // tile. With no source the tee simply stays empty.
        let (media, media_handle) = ost::calling::call_controls(true);
        // Pre-join choice: start muted when the user toggled mic off
        // (the gate is a watch channel created true).
        if !mic {
            let _ = media_handle.mic_on.send(false);
        }
        let _ = tx_evt.send(Event::CallMedia(media_handle));
        // Safety cap ~55 min; hang-up ends it earlier.
        let res = ost::calling::run_call_with_controls(
            3300, false, echo, thread, video, false, false, stop_rx, Some(media),
        )
        .await;
        match res {
            Ok(result) => {
                let _ = tx_evt.send(Event::CallEnded(format!(
                    "call ended — accepted: {}, {} packets sent / {} received",
                    result.call_accepted, result.packets_sent, result.packets_received
                )));
            }
            Err(e) => {
                let _ = tx_evt.send(Event::CallFailed(format!("{e:#}")));
            }
        }
    });
}

/// Accept the ringing incoming call: same spawn shape as
/// [`start_call_labelled`] (watch channel + detached task), but the call
/// leg comes from the invitation instead of epconv placement.
async fn accept_incoming(ses: &mut Session, tx: &Sender<Event>, raw: String) {
    if !call_slot_free(ses) {
        // Ringing on top of an active call: decline politely rather than
        // half-answer; the caller hears the decline immediately.
        decline_incoming(raw).await;
        let _ = tx.send(Event::CallFailed("declined: a call is already in progress".into()));
        return;
    }
    let Some(notification) = ost::calling::parse_call_notification(&raw) else {
        let _ = tx.send(Event::CallFailed(
            "incoming call invitation could not be parsed".into(),
        ));
        return;
    };
    let label = notification
        .participants
        .as_ref()
        .and_then(|p| p.from.as_ref())
        .and_then(|f| f.display_name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Incoming call".into());

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    ses.call_stop = Some(CallSlot {
        stop: stop_tx,
        done: done.clone(),
    });
    let tx_evt = tx.clone();
    tokio::spawn(async move {
        // First statement: Drop marks the slot done when THIS task ends
        // (normal return or panic unwind).
        let _done_guard = CallDoneGuard(done);
        let _ = tx_evt.send(Event::CallStarted { label });
        let _ = tx_evt.send(Event::CallStatus("accepting…".into()));
        // Safety cap via hang-up only — an incoming call has no duration
        // budget; the stop watch (and the remote end) end it.
        let res = ost::calling::answer_call_with_stop(&notification, false, stop_rx).await;
        match res {
            Ok(result) => {
                let _ = tx_evt.send(Event::CallEnded(format!(
                    "call ended — accepted: {}, {} packets sent / {} received",
                    result.call_accepted, result.packets_sent, result.packets_received
                )));
            }
            Err(e) => {
                let _ = tx_evt.send(Event::CallFailed(format!("{e:#}")));
            }
        }
    });
}

/// Decline a ringing call: fire-and-forget POST to the invitation's end
/// link (best-effort — a failed decline just lets the caller time out).
async fn decline_incoming(raw: String) {
    tokio::spawn(async move {
        let Some(notification) = ost::calling::parse_call_notification(&raw) else {
            log::warn!("decline: invitation unparseable, ignoring");
            return;
        };
        if let Err(e) = ost::calling::decline_call(&notification).await {
            log::warn!("decline failed (caller will time out): {e:#}");
        }
    });
}

/// Unread state for every chat: the flag compares the chat's last activity
/// against our stored read horizon; the count asks the archive how many
/// cached messages are newer (0 when the new messages aren't cached yet).
fn unread_map(ses: &Session, chats: &[ChatInfo]) -> std::collections::HashMap<String, (bool, u32)> {
    let mut out = std::collections::HashMap::new();
    let Some(archive) = ses.archive.as_ref() else {
        return out;
    };
    let horizons = archive.read_horizons().unwrap_or_default();
    if horizons.is_empty() {
        // First run (no read state ever recorded): seed every chat as read
        // instead of flagging the whole list unread out of the box.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        for chat in chats {
            let last_ms = chat
                .last_message_time
                .as_deref()
                .and_then(crate::model::to_epoch_ms)
                .unwrap_or(0);
            let _ = archive.set_read(&chat.id, last_ms.max(now_ms));
        }
        return out;
    }
    for chat in chats {
        let last_ms = chat
            .last_message_time
            .as_deref()
            .and_then(crate::model::to_epoch_ms)
            .unwrap_or(0);
        // Read position = max(local mark-read, server consumption
        // horizon) — reading the chat on another client advances ours.
        let server_ms = chat.last_read_ms.unwrap_or(0);
        let horizon = horizons.get(&chat.id).copied().unwrap_or(0).max(server_ms);
        if last_ms == 0 || last_ms <= horizon {
            continue;
        }
        let count = ms_to_teams_iso(horizon)
            .and_then(|iso| archive.count_after(&chat.id, &iso).ok())
            .unwrap_or(0) as u32;
        out.insert(chat.id.clone(), (true, count));
    }
    out
}

/// Epoch ms → the Teams ISO shape cached in the archive
/// (`2026-10-06T10:12:37.6750000Z`), so lexicographic SQL compares line up.
pub(crate) fn ms_to_teams_iso(ms: u64) -> Option<String> {
    let ts = jiff::Timestamp::from_millisecond(ms as i64).ok()?;
    let zdt = ts.to_zoned(jiff::tz::TimeZone::UTC);
    Some(format!(
        "{}.{:07}Z",
        zdt.strftime("%Y-%m-%dT%H:%M:%S"),
        (ms % 1000) * 10_000
    ))
}

/// Build a TeamsClient from cached tokens; returns our display name when the
/// Graph whoami works. `self_id` falls back to the JWT `oid` claim so own
/// message detection survives a transient Graph hiccup.
async fn ready_session(ses: &mut Session) -> Result<(Option<String>, Option<String>), String> {
    let c = TeamsClient::new().await.map_err(|e| format!("{e:#}"))?;
    ses.client = Some(c);
    match ses.client.as_ref() {
        Some(c) => match ost::api::whoami_data(c).await {
            Ok(me) => {
                ses.self_id = Some(me.id.clone());
                Ok((Some(me.display_name), ses.self_id.clone()))
            }
            Err(e) => {
                log::warn!("whoami failed ({e:#}); using JWT oid for own-detection");
                ses.self_id = cached_jwt_oid();
                Ok((None, ses.self_id.clone()))
            }
        },
        None => Ok((None, None)),
    }
}

/// Extract the Entra object id from the cached AAD access token (JWT `oid`).
fn cached_jwt_oid() -> Option<String> {
    use base64::Engine;
    let cfg = ost::config::Config::load_cached().ok()?;
    let token = cfg.get_graph_token()?.token;
    let payload = token.split('.').nth(1)?;
    let clean = payload.trim_end_matches('=');
    let bytes = (0..=2)
        .find_map(|pad| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(format!("{clean}{}", "=".repeat(pad)))
                .ok()
        })?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("oid").and_then(|x| x.as_str()).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(time: Option<&str>) -> ChatInfo {
        ChatInfo {
            id: "19:c@thread.v2".into(),
            name: "C".into(),
            is_group: false,
            last_message_time: time.map(String::from),
            last_message_sender: None,
            last_message_preview: None,
            last_read_ms: None,
        }
    }

    /// Sort key must understand BOTH timestamp shapes the conversations
    /// list produces. A bare u64 parse sent every ISO-stamped chat to 0 —
    /// they sank below the numeric ones (group chats floated above older
    /// chats on screen).
    #[test]
    fn chat_recency_parses_iso_and_numeric() {
        // 2026-10-14T20:00:00Z ≈ 1792699200_000 ms.
        let iso = chat(Some("2026-10-14T20:00:00.000Z"));
        let numeric = chat(Some("1760000000000")); // 2025-10-09, OLDER
        assert!(chat_recency(&iso) > 0, "ISO timestamp must parse");
        assert!(
            chat_recency(&iso) > chat_recency(&numeric),
            "newer ISO chat must outrank older numeric chat"
        );
        assert_eq!(chat_recency(&chat(None)), 0);
        assert_eq!(chat_recency(&chat(Some("garbage"))), 0);
    }
}

#[cfg(test)]
mod offline_guard_tests {
    use super::{runs_offline, Command};

    #[test]
    fn offline_capable_commands_stay_runnable() {
        // Recovery + teardown + cache-backed + local-only.
        for cmd in [
            Command::StartLogin,
            Command::CheckReady,
            Command::LoadChats,
            Command::StartTrouter,
            Command::SignOut,
            Command::ClearArchive,
            Command::ArchiveStats,
            Command::HangUp,
            Command::JoinPreviewStart,
            Command::JoinPreviewStop,
            Command::SaveImageSize {
                url: String::new(),
                w: 0,
                h: 0,
            },
        ] {
            assert!(runs_offline(&cmd), "{cmd:?} must run offline");
        }
    }

    #[test]
    fn network_commands_are_gated() {
        // Spot-check every family that hits the wire.
        for cmd in [
            Command::OpenChat(String::new()),
            Command::RefreshChat(String::new()),
            Command::Send {
                chat_id: String::new(),
                text: String::new(),
                cmid: String::new(),
            },
            Command::FetchImage(String::new()),
            Command::LoadCalendar,
            Command::LoadFiles,
            Command::LoadTodo,
            Command::StartCall(String::new()),
            Command::JoinMeeting {
                source: String::new(),
                camera: false,
                mic: true,
            },
            Command::LoadOlder(String::new()),
            Command::MarkRead {
                chat_id: String::new(),
                message_id: String::new(),
            },
        ] {
            assert!(!runs_offline(&cmd), "{cmd:?} must be gated offline");
        }
    }
}
