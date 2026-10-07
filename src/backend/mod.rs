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
use std::sync::mpsc::{Receiver, Sender};
use tokio::sync::mpsc::UnboundedReceiver;

use ost::api::{list_chats_data, ChatInfo, TeamInfo};

#[derive(Debug, Clone)]
pub enum Command {
    StartLogin,
    CheckReady,
    LoadChats,
    /// Open a chat or channel conversation.
    OpenChat(String),
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
    LoadCalendar,
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
    /// Ring the Teams echo/test bot (settings "Test call").
    TestCall,
    /// Join a meeting from a URL / thread id / meet ID.
    JoinMeeting(String),
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
    // ---- planner + shifts ----
    LoadPlanner,
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
    MarkedRead,
}

pub enum Event {
    Status(String),
    LoginResult(Result<(), String>),
    /// Device-code login: the URL + code to show in the GUI.
    LoginCode { url: String, code: String },
    Ready,
    /// Our own display name (Graph whoami).
    SelfName(String),
    /// Our own Entra object id (for own-message detection).
    SelfId(String),
    /// Network unavailable (or token refresh failed) but cached content
    /// exists — the UI stays usable instead of demanding a sign-in.
    Offline(String),
    NeedLogin(String),
    Chats(Vec<ChatInfo>),
    Messages {
        chat_id: String,
        messages: Vec<MessageInfo>,
        /// Roster of the open chat: mri → display name (best effort).
        members: HashMap<String, String>,
        /// Roster-resolved name for a placeholder-titled chat.
        resolved_name: Option<String>,
        /// True = prepend these (older page); false = replace the view.
        prepend: bool,
        /// Cursor for "load older"; None when history is exhausted.
        older_link: Option<String>,
    },
    /// An action (edit/delete/react/reply/mark-read) succeeded; refetch.
    ActionOk { chat_id: String, action: MsgAction },
    SearchResults {
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
        chat_id: String,
        name: String,
        sent: u64,
        total: u64,
    },
    UploadDone { chat_id: String, name: String },
    DownloadDone { name: String, path: PathBuf },
    Trouter(String),
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
    Error(String),
}

use ost::api::MessageInfo;

/// Spawn the worker; returns the command sender for the UI.
pub fn spawn(tx: Sender<Event>) -> tokio::sync::mpsc::UnboundedSender<Command> {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
    std::thread::Builder::new()
        .name("teamsfast-backend".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(worker(cmd_rx, tx));
        })
        .expect("spawn backend thread");
    cmd_tx
}

/// Sort key for the chat list: last-activity time, newest first.
pub fn chat_recency(chat: &ChatInfo) -> u64 {
    chat.last_message_time
        .as_deref()
        .and_then(|t| t.trim().parse::<u64>().ok())
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
    /// Hang-up switch for the active call (None when idle).
    pub call_stop: Option<tokio::sync::watch::Sender<bool>>,
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
            call_stop: None,
        }
    }
}

/// Token/session setup failed. With cached content that is an OFFLINE
/// situation (stay usable), not a sign-in demand.
fn offline_or_login(ses: &Session, tx: &Sender<Event>, error: String) {
    let has_cache = ses
        .archive
        .as_ref()
        .and_then(|a| a.load_chats().ok())
        .map(|c| !c.is_empty())
        .unwrap_or(false);
    if has_cache {
        let _ = tx.send(Event::Offline(format!(
            "Offline — showing cached data ({error})"
        )));
    } else {
        let _ = tx.send(Event::NeedLogin(error));
    }
}

async fn worker(mut rx: UnboundedReceiver<Command>, tx: Sender<Event>) {
    let mut ses = Session::new();
    let mut trouter_started = false;

    macro_rules! send {
        ($ev:expr) => {{
            let _ = tx.send($ev);
        }};
    }

    while let Some(cmd) = rx.recv().await {
        // `.await` on recv parks the root task properly, so spawned tasks
        // (trouter, drainer) run while the queue is idle.
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
                    Err(e) => send!(Event::Error(format!("chat list: {e:#}"))),
                }
            }
            Command::OpenChat(chat_id) => {
                conv::open_chat(&mut ses, &tx, chat_id).await;
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
                let Some(c) = ses.client.as_ref() else {
                    continue;
                };
                match ost::api::get_presence_data(c).await {
                    Ok(p) => send!(Event::MyPresence(p.availability)),
                    Err(e) => log::debug!("presence poll: {e:#}"),
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
                start_call(&mut ses, &tx, chat_id, false).await;
            }
            Command::TestCall => {
                start_call(&mut ses, &tx, String::new(), true).await;
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
            Command::JoinMeeting(source) => {
                if let Some((thread, label)) = join::resolve_join_target(&ses, &tx, &source).await
                {
                    start_call_labelled(&mut ses, &tx, thread, false, label).await;
                }
            }
            Command::HangUp => {
                if let Some(stop) = ses.call_stop.take() {
                    let _ = stop.send(true);
                    send!(Event::CallStatus("hanging up…".into()));
                }
            }
        }
    }
}

/// Place a call through teams-core's call driver: its own trouter session,
/// ICE/TURN, SRTP/Opus and cpal audio, all inside one spawned task. The
/// watch channel hangs up cooperatively.
async fn start_call(ses: &mut Session, tx: &Sender<Event>, chat_id: String, echo: bool) {
    let label = if echo { "Echo test".to_string() } else { chat_id.clone() };
    start_call_labelled(ses, tx, chat_id, echo, label).await;
}

/// [`start_call`] with a display label (meeting subject, chat name, …).
async fn start_call_labelled(
    ses: &mut Session,
    tx: &Sender<Event>,
    chat_id: String,
    echo: bool,
    label: String,
) {
    if ses.call_stop.is_some() {
        let _ = tx.send(Event::CallFailed("a call is already in progress".into()));
        return;
    }
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    ses.call_stop = Some(stop_tx);
    let tx_evt = tx.clone();
    let thread = if echo { None } else { Some(chat_id) };
    tokio::spawn(async move {
        let _ = tx_evt.send(Event::CallStarted { label });
        let _ = tx_evt.send(Event::CallStatus("connecting…".into()));
        // Safety cap ~55 min; hang-up ends it earlier.
        let res = ost::calling::run_call_with_stop(
            3300, false, echo, thread, false, false, false, stop_rx,
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
        let horizon = horizons.get(&chat.id).copied().unwrap_or(0);
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
