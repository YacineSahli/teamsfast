//! Directory domain: message search, teams/channels, new chats.

use super::{Event, Session};
use std::sync::mpsc::Sender;

/// `Command::Search` — Graph search over messages.
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
                hits: page.hits,
                more: page.more,
            });
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("403") || msg.contains("Forbidden") {
                let _ = tx.send(Event::SearchUnavailable(
                    "Message search isn't available with this tenant's Teams                      permissions (Graph refused the Teams client token)."
                        .into(),
                ));
            } else {
                let _ = tx.send(Event::Error(format!("search: {msg}")));
            }
            let _ = tx.send(Event::SearchResults {
                query: query.to_string(),
                hits: Vec::new(),
                more: false,
            });
        }
    }
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
