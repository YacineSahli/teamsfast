//! TeamsFast backend: a tokio worker thread on its own OS thread.
//!
//! The UI pushes [`Command`]s; domain modules under `backend/` implement the
//! protocol work against the `ost` core and report [`Event`]s back. The
//! worker owns the `TeamsClient`; trouter runs as an async task on the same
//! runtime; the event-hub drainer runs on the blocking pool (its Condvar
//! must never block the runtime).

pub mod conv;
pub mod directory;
pub mod headless;
pub mod live_parse;
pub mod live;
pub mod media;

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
    Send { chat_id: String, text: String },
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
    Ready,
    /// Our own display name (Graph whoami).
    SelfName(String),
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
}

impl Session {
    fn new() -> Self {
        Self {
            client: None,
            self_id: None,
            older_links: HashMap::new(),
        }
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
                send!(Event::Status(
                    "Sign-in started — open the terminal that launched teamsfast, \
                     visit the URL and enter the device code."
                        .into(),
                ));
                let res = ost::auth::oauth::login(true)
                    .await
                    .map_err(|e| format!("{e:#}"));
                send!(Event::LoginResult(res));
                match ready_session(&mut ses).await {
                    Ok(name) => {
                        if let Some(n) = name {
                            send!(Event::SelfName(n));
                        }
                        send!(Event::Ready);
                    }
                    Err(e) => send!(Event::NeedLogin(e)),
                }
            }            Command::CheckReady => match ready_session(&mut ses).await {
                Ok(name) => {
                    if let Some(n) = name {
                        send!(Event::SelfName(n));
                    }
                    send!(Event::Ready);
                }
                Err(e) => send!(Event::NeedLogin(e)),
            },
            Command::LoadChats => {
                let Some(c) = ses.client.as_ref() else {
                    send!(Event::NeedLogin("not signed in".into()));
                    continue;
                };
                match list_chats_data(c, 40).await {
                    Ok(mut chats) => {
                        filter_pseudo_chats(&mut chats);
                        sort_chats(&mut chats);
                        send!(Event::Chats(chats));
                    }
                    Err(e) => send!(Event::Error(format!("chat list: {e:#}"))),
                }
            }
            Command::OpenChat(chat_id) => {
                conv::open_chat(&mut ses, &tx, chat_id).await;
            }
            Command::Send { chat_id, text } => {
                conv::send(&ses, &tx, &chat_id, &text).await;
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
        }
    }
}

/// Build a TeamsClient from cached tokens; returns our display name when the
/// Graph whoami works.
async fn ready_session(ses: &mut Session) -> Result<Option<String>, String> {
    let c = TeamsClient::new().await.map_err(|e| format!("{e:#}"))?;
    ses.client = Some(c);
    let name = match ses.client.as_ref() {
        Some(c) => ost::api::whoami_data(c)
            .await

            .ok()
            .map(|me| {
                ses.self_id = Some(me.id.clone());
                me.display_name
            }),
        None => None,
    };
    Ok(name)
}
