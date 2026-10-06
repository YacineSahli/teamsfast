//! Directory domain (AGENT 1 OWNS THIS FILE): search, teams/channels,
//! new chats. Replace each stub body.

use super::{Event, Session};
use std::sync::mpsc::Sender;

/// `Command::Search` — Graph search over messages.
///
/// Core API:
/// `ost::api::search_messages_data(&client, query, from, size) -> Result<SearchPage>`
/// `SearchPage { hits: Vec<SearchHitInfo>, total: Option<i64>, more: bool }`
/// `SearchHitInfo { message_id, chat_id, team_id, channel_id, sender, timestamp, preview, subject }`
///
/// Emit `Event::SearchResults { query, hits, more }` (use size 25, cap from
/// at 100). Empty/whitespace query: emit empty results without a network call.
pub async fn search(ses: &Session, tx: &Sender<Event>, query: &str, from: usize) {
    let _ = (ses, tx, query, from);
}

/// `Command::LoadTeams` — teams with their channels.
///
/// Core API: `ost::api::list_teams_data(&client) -> Result<Vec<TeamInfo>>`
/// `TeamInfo { id, name, channels: Vec<ChannelInfo> }`
/// `ChannelInfo { id, name, description, membership_type, web_url }`
///
/// Emit `Event::Teams(teams)`.
pub async fn teams(ses: &Session, tx: &Sender<Event>) {
    let _ = (ses, tx);
}

/// `Command::CreateOneToOne` — start a 1:1 by email/UPN.
///
/// Core API:
/// `ost::api::create_one_to_one_chat_data(&client, user) -> Result<ChatInfo>`
/// (user = email or UPN; the core resolves it to an object id).
/// Emit `Event::CreateChatOk(chat)` then the caller opens it.
pub async fn create_one_to_one(ses: &Session, tx: &Sender<Event>, user: &str) {
    let _ = (ses, tx, user);
}

/// `Command::CreateGroup` — create a group chat.
///
/// Core API:
/// `ost::api::create_group_chat_data(&client, users, topic) -> Result<ChatInfo>`
/// (users = emails/UPNs).
/// Emit `Event::CreateChatOk(chat)`; on error `Event::Error`.
pub async fn create_group(ses: &Session, tx: &Sender<Event>, topic: &str, members: &[String]) {
    let _ = (ses, tx, topic, members);
}
