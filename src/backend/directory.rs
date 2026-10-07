//! Directory domain: message search, teams/channels, new chats.

use super::{Event, Session};
use std::sync::mpsc::Sender;

/// `Command::Search` — Graph search over messages, merged with the local
/// archive index (offline results always included, deduped by message id;
/// they also cover the tenant-blocked / offline cases alone).
pub async fn search(ses: &Session, tx: &Sender<Event>, query: &str, from: usize) {
    if query.trim().is_empty() {
        let _ = tx.send(Event::SearchResults {
            query: query.to_string(),
            hits: Vec::new(),
            more: false,
        });
        return;
    }
    let Some(c) = ses.client.as_ref() else {
        let _ = tx.send(Event::NeedLogin("not signed in".into()));
        return;
    };
    match ost::api::search_messages_data(c, query, from, 25).await {
        Ok(page) => {
            let _ = tx.send(Event::SearchResults {
                query: query.to_string(),
                hits: merge_local(ses, query, page.hits),
                more: page.more,
            });
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("403") || msg.contains("Forbidden") {
                let local = local_hits(ses, query, 25);
                let note = if local.is_empty() {
                    "Online message search isn't available with this tenant's Teams \
                     permissions (Graph refused the Teams client token)."
                        .to_string()
                } else {
                    format!(
                        "Online search is blocked by this tenant; showing {} local results.",
                        local.len()
                    )
                };
                let _ = tx.send(Event::SearchUnavailable(note));
                let _ = tx.send(Event::SearchResults {
                    query: query.to_string(),
                    hits: local,
                    more: false,
                });
            } else {
                let _ = tx.send(Event::Error(format!("search: {msg}")));
                let _ = tx.send(Event::SearchResults {
                    query: query.to_string(),
                    hits: local_hits(ses, query, 25),
                    more: false,
                });
            }
        }
    }
}

/// Archive hits as search results (newest first).
fn local_hits(ses: &Session, query: &str, limit: usize) -> Vec<ost::api::SearchHitInfo> {
    let Some(archive) = ses.archive.as_ref() else {
        return Vec::new();
    };
    archive
        .search_messages(query, limit)
        .unwrap_or_default()
        .into_iter()
        .map(|(chat_id, m)| ost::api::SearchHitInfo {
            message_id: m.id.clone(),
            chat_id,
            team_id: None,
            channel_id: None,
            sender: if m.sender.is_empty() { "Unknown".into() } else { m.sender },
            timestamp: m.timestamp,
            preview: crate::model::local_search_preview(&m.content),
            subject: None,
        })
        .collect()
}

/// Local + online hits, deduped by message id, locals first.
fn merge_local(
    ses: &Session,
    query: &str,
    online: Vec<ost::api::SearchHitInfo>,
) -> Vec<ost::api::SearchHitInfo> {
    let local = local_hits(ses, query, 25);
    if local.is_empty() {
        return online;
    }
    let mut out = local;
    let seen: std::collections::HashSet<String> =
        out.iter().map(|h| h.message_id.clone()).collect();
    for h in online {
        if !seen.contains(&h.message_id) {
            out.push(h);
        }
    }
    out
}

/// `Command::LoadTeams` — teams with their channels.
pub async fn teams(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    match ost::api::list_teams_data(c).await {
        Ok(teams) => {
            let _ = tx.send(Event::Teams(teams));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("teams: {e:#}")));
            let _ = tx.send(Event::Teams(Vec::new()));
        }
    }
}

/// `Command::CreateOneToOne` — start a 1:1 by email/UPN.
pub async fn create_one_to_one(ses: &Session, tx: &Sender<Event>, user: &str) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    match ost::api::create_one_to_one_chat_data(c, user).await {
        Ok(chat) => {
            let _ = tx.send(Event::CreateChatOk(chat));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("new chat: {e:#}")));
        }
    }
}

/// `Command::CreateGroup` — create a group chat.
pub async fn create_group(ses: &Session, tx: &Sender<Event>, topic: &str, members: &[String]) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    let topic_opt = if topic.trim().is_empty() {
        None
    } else {
        Some(topic)
    };
    match ost::api::create_group_chat_data(c, members, topic_opt).await {
        Ok(chat) => {
            let _ = tx.send(Event::CreateChatOk(chat));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("new group: {e:#}")));
        }
    }
}

/// `Command::SetChatMuted` — server-side mute toggle.
pub async fn set_muted(ses: &Session, tx: &Sender<Event>, chat_id: &str, muted: bool) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    match ost::api::set_chat_muted_with_client(c, chat_id, muted).await {
        Ok(()) => {
            let _ = tx.send(Event::Status(format!(
                "{} {chat_id}",
                if muted { "muted" } else { "unmuted" }
            )));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("mute: {e:#}")));
        }
    }
}

/// `Command::SetChatHidden` — hide the chat server-side.
pub async fn set_hidden(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    match ost::api::set_chat_hidden_with_client(c, chat_id, true).await {
        Ok(()) => {
            let _ = tx.send(Event::Status("chat hidden".into()));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("hide: {e:#}")));
        }
    }
}

/// `Command::LeaveChat` — remove our membership.
pub async fn leave_chat(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = ses.client.as_ref() else {
        return;
    };
    match ost::api::leave_chat_with_client(c, chat_id).await {
        Ok(()) => {
            let _ = tx.send(Event::Status("left the chat".into()));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("leave: {e:#}")));
        }
    }
}

/// `Command::MarkUnread` — roll our read horizon back so the chat shows
/// unread (and stays unread across restarts).
pub async fn mark_unread(ses: &Session, chat_id: &str) {
    if let Some(a) = ses.archive.as_ref() {
        let _ = a.set_read(chat_id, 0);
    }
}
