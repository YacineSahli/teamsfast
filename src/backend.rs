//! TeamsFast backend: a tokio worker thread on its own OS thread.
//!
//! The UI pushes [`Command`]s through an unbounded tokio channel; the worker
//! awaits them on a current-thread runtime, runs protocol calls against the
//! `ost` core, and sends [`Event`]s back over a std channel, waking the
//! window with `egui::Context::request_repaint` after every event (the
//! zapfast Command/Event/Waker shape, minimised for the spike).
//!
//! Live updates: `trouter::connect_and_run` is a normal async task on this
//! runtime (it reconnects internally), and the event-hub drainer runs via
//! `spawn_blocking` — `event_hub::drain_wait` is a blocking Condvar wait and
//! its own docs say hosts must not call it on the async runtime.

use anyhow::Result;
use ost::api::client::TeamsClient;
use ost::api::{
    list_chat_members_data, list_chats_data, read_messages_data, send_message_with_client, whoami_data,
    ChatInfo, MessageInfo,
};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use tokio::sync::mpsc::UnboundedReceiver;

pub enum Command {
    /// Run the ost device-code login (code is printed on the terminal).
    StartLogin,
    /// Try to build a TeamsClient from cached tokens.
    CheckReady,
    LoadChats,
    OpenChat(String),
    /// (chat_id, text)
    Send(String, String),
    /// Connect the Trouter websocket and stream events.
    StartTrouter,
}

pub enum Event {
    Status(String),
    LoginResult(Result<(), String>),
    Ready,
    /// Our own display name (from Graph whoami), for sender attribution.
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
    },
    Sent(String),
    Trouter(String),
    TrouterConnected,
    Error(String),
}

pub fn spawn(tx: Sender<Event>) -> tokio::sync::mpsc::UnboundedSender<Command> {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
    std::thread::Builder::new()
        .name("teamsfast-backend".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(worker(tx, cmd_rx));
        })
        .expect("spawn backend thread");
    cmd_tx
}

/// Sort key for the chat list: last-activity time, newest first.
/// Teams sends `originalArrivalTime` as an epoch-milliseconds string;
/// anything unparsable sorts last, preserving server order among ties.
fn chat_recency(chat: &ChatInfo) -> u64 {
    chat.last_message_time
        .as_deref()
        .and_then(|t| t.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn sort_chats(chats: &mut [ChatInfo]) {
    chats.sort_by(|a, b| chat_recency(b).cmp(&chat_recency(a)));
}

/// Drop Teams' activity feeds (Mentions, Notifications, Threads, Calllogs…).
/// They come mixed into `view=mychats` with `48:*` ids; real conversations
/// (1:1, group, meeting, channel) never do. You cannot send to a feed.
fn filter_pseudo_chats(chats: &mut Vec<ChatInfo>) {
    chats.retain(|c| !c.id.starts_with("48:"));
}

async fn worker(tx: Sender<Event>, mut rx: UnboundedReceiver<Command>) {
    let mut client: Option<TeamsClient> = None;
    let mut trouter_started = false;
    let mut self_id: Option<String> = None;

    async fn fetch_self(client: &TeamsClient) -> Option<(String, String)> {
        let me = whoami_data(client).await.ok()?;
        Some((me.id, me.display_name))
    }

    while let Some(cmd) = rx.recv().await {
        // `.await` on recv parks the root task properly, so spawned tasks
        // (trouter, drainer) run while the queue is idle.
        match cmd {
            Command::StartLogin => {
                let _ = tx.send(Event::Status(
                    "Sign-in started — open the terminal that launched teamsfast, \
                     visit the URL and enter the device code."
                        .into(),
                ));
                let res = ost::auth::oauth::login(true)
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::LoginResult(res));
                match TeamsClient::new().await {
                    Ok(c) => {
                        if let Some((id, name)) = fetch_self(&c).await {
                            self_id = Some(id);
                            let _ = tx.send(Event::SelfName(name));
                        }
                        client = Some(c);
                        let _ = tx.send(Event::Ready);
                    }
                    Err(e) => {
                        let _ = tx.send(Event::NeedLogin(format!("{e:#}")));
                    }
                }
            }
            Command::CheckReady => match TeamsClient::new().await {
                Ok(c) => {
                    if let Some((id, name)) = fetch_self(&c).await {
                        self_id = Some(id);
                        let _ = tx.send(Event::SelfName(name));
                    }
                    client = Some(c);
                    let _ = tx.send(Event::Ready);
                }
                Err(e) => {
                    let _ = tx.send(Event::NeedLogin(format!("{e:#}")));
                }
            },
            Command::LoadChats => {
                let Some(c) = client.as_ref() else {
                    let _ = tx.send(Event::NeedLogin("not signed in".into()));
                    continue;
                };
                match list_chats_data(c, 40).await {
                    Ok(mut chats) => {
                        filter_pseudo_chats(&mut chats);
                        sort_chats(&mut chats);
                        let _ = tx.send(Event::Chats(chats));
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Error(format!("chat list: {e:#}")));
                    }
                }
            }
            Command::OpenChat(chat_id) => {
                let Some(c) = client.as_ref() else { continue };
                // Roster first, best effort: names for unnamed senders ("?")
                // and a real name for placeholder-titled chats (e.g. the
                // @unq.gbl.spaces threads ost's resolver doesn't cover).
                let mut members: HashMap<String, String> = HashMap::new();
                let mut resolved_name: Option<String> = None;
                if let Ok((_, roster)) = list_chat_members_data(c, &chat_id).await {
                    for m in roster {
                        if !m.display_name.is_empty() {
                            members.insert(m.mri.clone(), m.display_name.clone());
                        }
                        let is_me = match self_id.as_deref() {
                            Some(me) => {
                                m.user_id.as_deref() == Some(me)
                                    || m.mri == format!("8:orgid:{me}")
                            }
                            None => false,
                        };
                        if !is_me && m.mri.starts_with("8:orgid:") && !m.display_name.is_empty() {
                            resolved_name = Some(m.display_name.clone());
                        }
                    }
                }
                match read_messages_data(c, &chat_id, 50).await {
                    Ok(messages) => {
                        let _ = tx.send(Event::Messages { chat_id, messages, members, resolved_name });
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Error(format!("history: {e:#}")));
                    }
                }
            }
            Command::Send(chat_id, text) => {
                let Some(c) = client.as_ref() else { continue };
                match send_message_with_client(c, &chat_id, &text).await {
                    Ok(()) => {
                        let _ = tx.send(Event::Sent(chat_id.clone()));
                        // Re-read so the sent bubble comes from server truth.
                        if let Ok(messages) = read_messages_data(c, &chat_id, 50).await {
                            let _ = tx.send(Event::Messages {
                                chat_id,
                                messages,
                                members: HashMap::new(),
                                resolved_name: None,
                            });
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Error(format!("send: {e:#}")));
                    }
                }
            }
            Command::StartTrouter => {
                if trouter_started {
                    continue;
                }
                trouter_started = true;

                // Trouter session: plain async task (all-nonblocking inside).
                tokio::spawn(async {
                    if let Err(e) = ost::trouter::connect_and_run().await {
                        log::warn!("trouter stopped: {e:#}");
                    }
                });

                // Event-hub drainer: blocking Condvar waits belong on the
                // blocking pool, never on the async runtime.
                let tx2 = tx.clone();
                tokio::task::spawn_blocking(move || loop {
                    for ev in ost::event_hub::drain_wait(32, 250) {
                        if tx2.send(Event::Trouter(ev)).is_err() {
                            return; // UI gone
                        }
                    }
                });

                let _ = tx.send(Event::TrouterConnected);
            }
        }
    }
}

// Keep Result imported for future error paths; silences unused-import churn.
#[allow(unused)]
fn _t(_: Result<()>) {}
