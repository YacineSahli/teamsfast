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
