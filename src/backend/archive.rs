//! Local archive: chats + messages persisted in an SQLCipher database so
//! the app opens instantly on cached content and history stays readable
//! offline. The network remains the source of truth; the archive mirrors
//! it after every successful fetch.
//!
//! Key handling: a random 32-byte hex key in `archive.key` (0600) beside
//! the database. Upgrading to the OS keyring is a later step.

use anyhow::{Context, Result};
use ost::api::{ChatInfo, MessageInfo};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct Archive {
    conn: Connection,
}

pub fn default_paths() -> (PathBuf, PathBuf) {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("teamsfast");
    (state.join("archive.db"), state.join("archive.key"))
}

fn load_or_create_key(path: &Path) -> Result<String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let key = text.trim().to_string();
        if key.len() >= 32 {
            return Ok(key);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

impl Archive {
    pub fn open() -> Result<Self> {
        let (db_path, key_path) = default_paths();
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let key = load_or_create_key(&key_path)?;
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open archive {db_path:?}"))?;
        conn.pragma_update(None, "key", &key)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chats (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                is_group INTEGER NOT NULL,
                last_message_time TEXT,
                last_message_sender TEXT,
                last_message_preview TEXT
            );
            CREATE TABLE IF NOT EXISTS messages (
                chat_id TEXT NOT NULL,
                id TEXT NOT NULL,
                ts TEXT NOT NULL,
                sender TEXT NOT NULL,
                sender_mri TEXT NOT NULL,
                content TEXT NOT NULL,
                raw TEXT NOT NULL,
                reply_to TEXT,
                client_message_id TEXT,
                PRIMARY KEY (chat_id, id)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_chat_ts
                ON messages(chat_id, ts);",
        )?;
        Ok(Self { conn })
    }

    /// Replace the cached chat list.
    pub fn save_chats(&self, chats: &[ChatInfo]) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("INSERT OR REPLACE INTO chats VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for c in chats {
            stmt.execute((
                &c.id,
                &c.name,
                c.is_group,
                &c.last_message_time,
                &c.last_message_sender,
                &c.last_message_preview,
            ))?;
        }
        Ok(())
    }

    pub fn load_chats(&self) -> Result<Vec<ChatInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_group, last_message_time, last_message_sender,
                    last_message_preview
             FROM chats",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ChatInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                is_group: row.get(2)?,
                last_message_time: row.get(3)?,
                last_message_sender: row.get(4)?,
                last_message_preview: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Replace the cached messages of one conversation.
    pub fn save_messages(&self, chat_id: &str, messages: &[MessageInfo]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO messages
             (chat_id, id, ts, sender, sender_mri, content, raw, reply_to, client_message_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for m in messages {
            stmt.execute((
                chat_id,
                &m.id,
                &m.timestamp,
                &m.sender,
                &m.sender_mri,
                &m.content,
                &m.raw,
                &m.reply_to,
                &m.client_message_id,
            ))?;
        }
        Ok(())
    }

    /// Newest `limit` messages of a conversation, oldest first.
    pub fn load_messages(&self, chat_id: &str, limit: usize) -> Result<Vec<MessageInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, sender, sender_mri, content, raw, reply_to, client_message_id
             FROM messages WHERE chat_id = ?1
             ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(rusqlite::params![chat_id, limit as i64], row_to_message)?;
        let mut out = Vec::new();
        while let Some(m) = rows.next() {
            out.push(m?);
        }
        out.reverse(); // newest last → oldest-first rendering order
        Ok(out)
    }

    /// One older page strictly before `before_ts`, oldest first.
    pub fn load_older(
        &self,
        chat_id: &str,
        before_ts: &str,
        limit: usize,
    ) -> Result<Vec<MessageInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, sender, sender_mri, content, raw, reply_to, client_message_id
             FROM messages WHERE chat_id = ?1 AND ts < ?2
             ORDER BY ts DESC, id DESC LIMIT ?3",
        )?;
        let mut rows = stmt.query_map(
            rusqlite::params![chat_id, before_ts, limit as i64],
            row_to_message,
        )?;
        let mut out = Vec::new();
        while let Some(m) = rows.next() {
            out.push(m?);
        }
        out.reverse();
        Ok(out)
    }
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageInfo> {
    Ok(MessageInfo {
        id: row.get(0)?,
        sender_mri: row.get(3)?,
        sender: row.get(2)?,
        timestamp: row.get(1)?,
        content: row.get(4)?,
        raw: row.get(5)?,
        reactions: Vec::new(), // reactions come from live fetches
        reply_to: row.get(6)?,
        client_message_id: row.get(7)?,
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_archive(tag: &str) -> (Archive, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "teamsfast-test-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("k.key");
        std::fs::write(&key, "a".repeat(64)).unwrap();
        let db = dir.join("a.db");
        let conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "key", "a".repeat(64)).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chats (id TEXT PRIMARY KEY, name TEXT NOT NULL,
                is_group INTEGER NOT NULL, last_message_time TEXT,
                last_message_sender TEXT, last_message_preview TEXT);
            CREATE TABLE IF NOT EXISTS messages (chat_id TEXT NOT NULL, id TEXT NOT NULL,
                ts TEXT NOT NULL, sender TEXT NOT NULL, sender_mri TEXT NOT NULL,
                content TEXT NOT NULL, raw TEXT NOT NULL, reply_to TEXT,
                client_message_id TEXT, PRIMARY KEY (chat_id, id));",
        )
        .unwrap();
        (Archive { conn }, dir)
    }

    #[test]
    fn messages_round_trip_and_order() {
        let (archive, dir) = temp_archive("msgs");
        let msgs = vec![
            MessageInfo {
                id: "m1".into(),
                sender_mri: "8:orgid:a".into(),
                sender: "A".into(),
                timestamp: "2026-10-06T10:00:00.0000000Z".into(),
                content: "first".into(),
                raw: "<div>first</div>".into(),
                reactions: Vec::new(),
                reply_to: None,
                client_message_id: None,
            },
            MessageInfo {
                id: "m2".into(),
                sender_mri: "8:orgid:b".into(),
                sender: "B".into(),
                timestamp: "2026-10-06T11:00:00.0000000Z".into(),
                content: "second".into(),
                raw: "<div>second</div>".into(),
                reactions: Vec::new(),
                reply_to: None,
                client_message_id: None,
            },
        ];
        archive.save_messages("19:c", &msgs).unwrap();
        let loaded = archive.load_messages("19:c", 10).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "first"); // oldest first
        assert_eq!(loaded[1].content, "second");
        let older = archive
            .load_older("19:c", "2026-10-06T10:30:00.0000000Z", 10)
            .unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].content, "first");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chats_round_trip() {
        let (archive, dir) = temp_archive("chats");
        archive
            .save_chats(&[ChatInfo {
                id: "19:x".into(),
                name: "X".into(),
                is_group: true,
                last_message_time: Some("1760000000000".into()),
                last_message_sender: Some("A".into()),
                last_message_preview: Some("hey".into()),
            }])
            .unwrap();
        let chats = archive.load_chats().unwrap();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].name, "X");
        assert!(chats[0].is_group);
        let _ = std::fs::remove_dir_all(dir);
    }
}
