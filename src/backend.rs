//! TeamsFast backend: a tokio worker thread on its own OS thread.
//!
//! The UI pushes [`Command`]s through an mpsc channel; the worker runs them
//! against the `ost` protocol core and sends [`Event`]s back, waking the
//! window with `egui::Context::request_repaint` after every event (the
//! zapfast Command/Event/Waker shape, minimised for the spike).

use anyhow::Result;
use ost::api::client::TeamsClient;
use ost::api::{list_chats_data, read_messages_data, send_message_with_client, ChatInfo, MessageInfo};
use std::sync::mpsc::{Receiver, Sender};

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
    NeedLogin(String),
    Chats(Vec<ChatInfo>),
    Messages { chat_id: String, messages: Vec<MessageInfo> },
    Sent(String),
    Trouter(String),
    TrouterConnected,
    Error(String),
}

pub struct Backend {
    pub tx: Sender<Event>,
}

pub fn spawn(tx: Sender<Event>) -> Sender<Command> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();
    std::thread::Builder::new()
        .name("teamsfast-backend".into())
        .spawn(move || worker(tx, cmd_rx))
        .expect("spawn backend thread");
    cmd_tx
}

fn worker(tx: Sender<Event>, rx: Receiver<Command>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    rt.block_on(async move {
        let mut client: Option<TeamsClient> = None;
        let mut trouter_started = false;

        while let Ok(cmd) = rx.recv() {
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
                    // After login (or if we were already logged in), check readiness.
                    match TeamsClient::new().await {
                        Ok(c) => {
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
                        Ok(chats) => {
                            let _ = tx.send(Event::Chats(chats));
                        }
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("chat list: {e:#}")));
                        }
                    }
                }
                Command::OpenChat(chat_id) => {
                    let Some(c) = client.as_ref() else { continue };
                    match read_messages_data(c, &chat_id, 50).await {
                        Ok(messages) => {
                            let _ = tx.send(Event::Messages { chat_id, messages });
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
                                let _ = tx.send(Event::Messages { chat_id, messages });
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
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        // connect_and_run only returns on Ctrl+C (Shutdown);
                        // otherwise it reconnects with backoff internally.
                        tokio::spawn(async move {
                            let _ = ost::trouter::connect_and_run().await;
                        });
                        let _ = tx2.send(Event::TrouterConnected);
                        loop {
                            for ev in ost::event_hub::drain_wait(32, 250) {
                                let _ = tx2.send(Event::Trouter(ev));
                            }
                        }
                    });
                }
            }
        }
    });
}
