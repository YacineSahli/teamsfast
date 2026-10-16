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

    // Cached history paints FIRST — before any network round-trip. The
    // roster fetch below is an RTT (often several: blank roster names fan
    // out to Graph user lookups), and making the user wait for it before
    // a cached conversation appeared read as "clicking a chat is slow".
    // Message rows carry their own sender names, so the cached paint is
    // complete without the roster (which arrives with the network refresh).
    if let Some(archive) = ses.archive.as_ref()
        && let Ok(cached) = archive.load_messages(&chat_id, 50)
        && !cached.is_empty()
    {
        log::debug!("open_chat: cached {} messages painted before network", cached.len());
        let _ = tx.send(Event::Messages {
            older_link: None,
            prepend: false,
            merge: false,
            chat_id: chat_id.clone(),
            messages: cached,
            members: HashMap::new(),
            resolved_name: None,
        });
    }

    // Roster names rarely change, but fetching them fans out to up to 60
    // Graph user lookups — cache per chat for a few minutes and skip the
    // whole fetch on re-opens within the window. History still runs
    // concurrently with the roster on a cold open.
    const ROSTER_TTL: std::time::Duration = std::time::Duration::from_secs(300);
    let now = std::time::Instant::now();
    let cached_roster = ses
        .roster_cache
        .get(&chat_id)
        .filter(|(at, _, _)| now.duration_since(*at) < ROSTER_TTL)
        .cloned();
    let needs_roster = cached_roster.is_none();
    let roster_fut = async {
        if !needs_roster {
            return None;
        }
        Some(list_chat_members_data(c, &chat_id).await)
    };
    let is_onetoone = chat_id.starts_with("19:")
        && !chat_id.contains("@thread")
        && !chat_id.contains("meeting");
    let (roster, page) = tokio::join!(roster_fut, read_messages_page(c, &chat_id, 50, None));

    let mut members: HashMap<String, String> = HashMap::new();
    let mut resolved_name: Option<String> = None;
    // Roster-name resolution is ONLY meaningful for 1:1 chats: in a
    // group/meeting the roster is full of members and picking one would
    // name the chat after a random participant (e.g. "Standup" showing
    // "Souhail Khalfallah"). Groups take their topic from the chat list.
    if let Some(roster) = roster {
        if let Ok((_, roster)) = roster {
            for m in roster {
                if !m.display_name.is_empty() {
                    members.insert(m.mri.clone(), m.display_name.clone());
                }
                if is_onetoone {
                    let is_me = match ses.self_id.as_deref() {
                        Some(me) => {
                            m.user_id.as_deref() == Some(me)
                                || m.mri == format!("8:orgid:{me}")
                        }
                        None => false,
                    };
                    if !is_me
                        && m.mri.starts_with("8:orgid:")
                        && !m.display_name.is_empty()
                    {
                        resolved_name = Some(m.display_name.clone());
                    }
                }
            }
        }
        // Members drive sender labels in every chat type — cache both.
        ses.roster_cache.insert(
            chat_id.clone(),
            (now, members.clone(), resolved_name.clone()),
        );
    } else if let Some((_, m, r)) = cached_roster {
        members = m;
        resolved_name = r;
    }

    // The roster's 1:1 name beats the chat-list heuristic (which falls
    // back to the last sender and can show our own name). Heal the cached
    // row in place — a full list rewrite here cost a save per open.
    if let Some(name) = resolved_name.as_ref().filter(|n| !n.trim().is_empty()) {
        if let Some(archive) = ses.archive.as_ref() {
            let needs_heal = archive
                .chat_name(&chat_id)
                .ok()
                .flatten()
                .map(|current| current != *name)
                .unwrap_or(false);
            if needs_heal {
                let _ = archive.rename_chat(&chat_id, name);
                let _ = tx.send(Event::ChatRenamed {
                    chat_id: chat_id.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    match page {
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
                merge: false,
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
            // Persist the page: scrolled history stops re-fetching every
            // session and stays readable offline (load_older from cache).
            if let Some(archive) = ses.archive.as_ref()
                && let Err(e) = archive.save_messages(chat_id, &page.messages)
            {
                log::warn!("archive save older page: {e:#}");
            }
            let _ = tx.send(Event::Messages {
                older_link: Some(cursor),
                prepend: true,
                merge: false,
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
            // Small confirmation slice, merged into the view: the send's
            // feedback should not wait on (or re-download) a full page, and
            // the pre-action older cursor stays valid.
            if let Ok(page) = read_messages_page(c, chat_id, 10, None).await {
                let landed = ost::api::find_by_client_id(&page.messages, cmid).is_some();
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
                    older_link: None,
                    prepend: false,
                    merge: true,
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
            refetch_tail(ses, tx, chat_id).await;
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
            refetch_tail(ses, tx, chat_id).await;
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
            refetch_tail(ses, tx, chat_id).await;
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
            refetch_tail(ses, tx, chat_id).await;
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

/// Post-action confirmation slice: small newest page merged into the view.
/// The action's feedback must not wait on (or re-download) a full page, and
/// older pages the user already loaded survive. Only ids present in the
/// slice are updated — other-client deletions heal on the full refresh.
pub(crate) async fn refetch_tail(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    // The action itself already succeeded (ActionOk is out); a failed
    // confirmation slice only means the view heals on the next full
    // refresh — say so instead of failing silently.
    match read_messages_page(c, chat_id, 10, None).await {
        Ok(page) => {
            if let Some(archive) = ses.archive.as_ref()
                && let Err(e) = archive.save_messages(chat_id, &page.messages)
            {
                log::warn!("archive save messages: {e:#}");
            }
            let _ = tx.send(Event::Messages {
                older_link: None,
                prepend: false,
                merge: true,
                chat_id: chat_id.to_string(),
                messages: page.messages,
                members: HashMap::new(),
                resolved_name: None,
            });
        }
        Err(e) => {
            log::warn!("action confirmation fetch failed: {e:#}");
            let _ = tx.send(Event::Status("action confirmed; view refresh deferred".into()));
        }
    }
}

/// Refetch the newest page for a chat (the light live-activity refresh)
/// without touching the roster — member names were already delivered when
/// the chat was opened.
pub(crate) async fn refetch(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    let t0 = std::time::Instant::now();
    // Full replace: the live-activity refresh (RefreshChat) must also heal
    // deletions made by other clients, which a merged slice cannot see.
    // The Err branch is load-bearing: the UI's refresh_in_flight guard is
    // cleared by Messages OR Error — a silent failure wedges refreshes
    // until the 5s self-heal (this exact shape caused the original stall).
    match read_messages_page(c, chat_id, 50, None).await {
        Ok(page) => {
            log::debug!("refresh page fetch: {}ms", t0.elapsed().as_millis());
            if let Some(archive) = ses.archive.as_ref()
                && let Err(e) = archive.save_messages(chat_id, &page.messages)
            {
                log::warn!("archive save messages: {e:#}");
            }
            let _ = tx.send(Event::Messages {
                older_link: page.backward_link,
                prepend: false,
                merge: false,
                chat_id: chat_id.to_string(),
                messages: page.messages,
                members: HashMap::new(),
                resolved_name: None,
            });
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("chat refresh: {e:#}")));
        }
    }
}
