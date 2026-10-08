//! Conversation domain: open, send, reply, edit, delete, react, paging,
//! mark-read. All handlers are best-effort and report failures as events.

use super::{MsgAction, Session};
use ost::api::client::TeamsClient;
use ost::api::{list_chat_members_data, read_messages_page};
use std::collections::HashMap;
use std::sync::mpsc::Sender;

use super::Event;

fn client<'a>(ses: &'a Session) -> Option<&'a TeamsClient> {
    ses.client.as_ref()
}

/// Open a conversation: roster (names) + newest history page.
pub async fn open_chat(ses: &mut Session, tx: &Sender<Event>, chat_id: String) {
    let Some(c) = client(ses) else {
        let _ = tx.send(Event::NeedLogin("not signed in".into()));
        return;
    };

    let mut members: HashMap<String, String> = HashMap::new();
    let mut resolved_name: Option<String> = None;
    if let Ok((_, roster)) = list_chat_members_data(c, &chat_id).await {
        for m in roster {
            if !m.display_name.is_empty() {
                members.insert(m.mri.clone(), m.display_name.clone());
            }
            let is_me = match ses.self_id.as_deref() {
                Some(me) => m.user_id.as_deref() == Some(me) || m.mri == format!("8:orgid:{me}"),
                None => false,
            };
            if !is_me && m.mri.starts_with("8:orgid:") && !m.display_name.is_empty() {
                resolved_name = Some(m.display_name.clone());
            }
        }
    }

    // The roster's 1:1 name beats the chat-list heuristic (which falls
    // back to the last sender and can show our own name). Heal the cached
    // list so the sidebar matches the header.
    if let Some(name) = resolved_name.as_ref().filter(|n| !n.trim().is_empty()) {
        if let Some(archive) = ses.archive.as_ref()
            && let Ok(mut chats) = archive.load_chats()
            && let Some(chat) = chats.iter_mut().find(|c| c.id == chat_id)
            && chat.name != *name
        {
            chat.name = name.clone();
            let _ = archive.save_chats(&chats);
            let _ = tx.send(Event::ChatRenamed {
                chat_id: chat_id.clone(),
                name: name.clone(),
            });
        }
    }

    // Cached history paints instantly (also works offline).
    if let Some(archive) = ses.archive.as_ref()
        && let Ok(cached) = archive.load_messages(&chat_id, 50)
        && !cached.is_empty()
    {
        let _ = tx.send(Event::Messages {
            older_link: None,
            prepend: false,
            chat_id: chat_id.clone(),
            messages: cached,
            members: HashMap::new(),
            resolved_name: None,
        });
    }

    match read_messages_page(c, &chat_id, 50, None).await {
        Ok(page) => {
            let cursor = page.backward_link.clone().unwrap_or_default();
            if let Some(archive) = ses.archive.as_ref()
                && let Err(e) = archive.save_messages(&chat_id, &page.messages)
            {
                log::warn!("archive save messages: {e:#}");
            }
            ses.older_links.insert(chat_id.clone(), cursor.clone());
            let _ = tx.send(Event::Messages {
                older_link: Some(cursor),
                prepend: false,
                chat_id,
                messages: page.messages,
                members,
                resolved_name,
            });
        }
        Err(e) => {
            // Keep whatever the archive rendered; report the failure.
            let _ = tx.send(Event::Error(format!("history: {e:#}")));
        }
    }
}

/// One older page, prepended by the UI.
pub async fn load_older(ses: &mut Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    let Some(link) = ses.older_links.get(chat_id).cloned() else {
        return;
    };
    if link.is_empty() {
        return;
    }
    match read_messages_page(c, chat_id, 50, Some(&link)).await {
        Ok(page) => {
            let cursor = page.backward_link.clone().unwrap_or_default();
            ses.older_links.insert(chat_id.to_string(), cursor.clone());
            let _ = tx.send(Event::Messages {
                older_link: Some(cursor),
                prepend: true,
                chat_id: chat_id.to_string(),
                messages: page.messages,
                members: HashMap::new(),
                resolved_name: None,
            });
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("older history: {e:#}")));
        }
    }
}

pub async fn send(ses: &mut Session, tx: &Sender<Event>, chat_id: &str, text: &str, cmid: &str) {
    let Some(c) = client(ses) else {
        let _ = tx.send(Event::SendFailed {
            chat_id: chat_id.to_string(),
            cmid: cmid.to_string(),
            error: "not signed in".into(),
        });
        return;
    };
    // Idempotent send: the clientmessageid lets us verify the message
    // actually landed — topic-type channels accept the POST but drop the
    // message, which would otherwise look like a silent failure.
    let sent = ost::api::send_message_with_client_id(c, chat_id, text, cmid).await;
    match sent {
        Ok(_) => {
            let _ = tx.send(Event::ActionOk {
                chat_id: chat_id.to_string(),
                action: MsgAction::Replied,
            });
            if let Ok(page) = read_messages_page(c, chat_id, 50, None).await {
                let landed = ost::api::find_by_client_id(&page.messages, cmid).is_some();
                let cursor = page.backward_link.clone().unwrap_or_default();
                ses.older_links.insert(chat_id.to_string(), cursor.clone());
                if !landed {
                    let _ = tx.send(Event::SendFailed {
                        chat_id: chat_id.to_string(),
                        cmid: cmid.to_string(),
                        error:
                            "The message may not have been delivered: this conversation (a topic channel) did not show it after posting."
                                .into(),
                    });
                }
                let _ = tx.send(Event::Messages {
                    older_link: Some(cursor),
                    prepend: false,
                    chat_id: chat_id.to_string(),
                    messages: page.messages,
                    members: HashMap::new(),
                    resolved_name: None,
                });
            }
        }
        Err(e) => {
            let _ = tx.send(Event::SendFailed {
                chat_id: chat_id.to_string(),
                cmid: cmid.to_string(),
                error: format!("{e:#}"),
            });
        }
    }
}

/// `Command::ReadReceipts` — who has read up to which message.
pub async fn fetch_receipts(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::read_receipts_data(c, chat_id).await {
        Ok(receipts) => {
            let pairs: Vec<(String, String)> = receipts
                .into_iter()
                .filter(|r| !r.user.is_empty())
                .map(|r| (r.user, r.message_id))
                .collect();
            let _ = tx.send(Event::ReadReceipts {
                chat_id: chat_id.to_string(),
                receipts: pairs,
            });
        }
        Err(e) => {
            // Receipts are a nicety — never surface as a hard error.
            log::debug!("receipts: {e:#}");
        }
    }
}

pub async fn reply(
    ses: &Session,
    tx: &Sender<Event>,
    chat_id: &str,
    parent_id: &str,
    parent_sender: &str,
    parent_text: &str,
    text: &str,
) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::reply_message_with_client(c, chat_id, parent_id, parent_sender, parent_text, text)
        .await
    {
        Ok(()) => {
            let _ = tx.send(Event::ActionOk {
                chat_id: chat_id.to_string(),
                action: MsgAction::Replied,
            });
            refetch(ses, tx, chat_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("reply: {e:#}")));
        }
    }
}

pub async fn edit(ses: &Session, tx: &Sender<Event>, chat_id: &str, message_id: &str, text: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::edit_message_with_client(c, chat_id, message_id, text).await {
        Ok(()) => {
            let _ = tx.send(Event::ActionOk {
                chat_id: chat_id.to_string(),
                action: MsgAction::Edited,
            });
            refetch(ses, tx, chat_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("edit: {e:#}")));
        }
    }
}

pub async fn delete(ses: &Session, tx: &Sender<Event>, chat_id: &str, message_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::delete_message_with_client(c, chat_id, message_id).await {
        Ok(()) => {
            let _ = tx.send(Event::ActionOk {
                chat_id: chat_id.to_string(),
                action: MsgAction::Deleted,
            });
            refetch(ses, tx, chat_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("delete: {e:#}")));
        }
    }
}

pub async fn react(
    ses: &Session,
    tx: &Sender<Event>,
    chat_id: &str,
    message_id: &str,
    emoji: &str,
    remove: bool,
) {
    let Some(c) = client(ses) else {
        return;
    };
    let res = if remove {
        ost::api::remove_reaction_with_client(c, chat_id, message_id, emoji).await
    } else {
        ost::api::send_reaction_with_client(c, chat_id, message_id, emoji).await
    };
    match res {
        Ok(()) => {
            let _ = tx.send(Event::ActionOk {
                chat_id: chat_id.to_string(),
                action: MsgAction::Reacted,
            });
            refetch(ses, tx, chat_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("reaction: {e:#}")));
        }
    }
}

pub async fn mark_read(ses: &Session, tx: &Sender<Event>, chat_id: &str, message_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::mark_read_with_client(c, chat_id, message_id).await {
        Ok(()) => {
            // Advance the local read horizon so unread badges clear even
            // before the next chat-list refresh.
            if let Some(archive) = ses.archive.as_ref() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let _ = archive.set_read(chat_id, now_ms);
            }
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("mark-read: {e:#}")));
        }
    }
}

/// Refetch the newest page for a chat (after an action) without touching the
/// roster — member names were already delivered when the chat was opened.
async fn refetch(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    if let Ok(page) = read_messages_page(c, chat_id, 50, None).await {
        if let Some(archive) = ses.archive.as_ref()
            && let Err(e) = archive.save_messages(chat_id, &page.messages)
        {
            log::warn!("archive save messages: {e:#}");
        }
        let _ = tx.send(Event::Messages {
            older_link: page.backward_link,
            prepend: false,
            chat_id: chat_id.to_string(),
            messages: page.messages,
            members: HashMap::new(),
            resolved_name: None,
        });
    }
}
