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
                ON messages(chat_id, ts);
            CREATE TABLE IF NOT EXISTS read_state (
                chat_id TEXT PRIMARY KEY,
                last_read_ms INTEGER NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    /// Delete every cached row (chats, messages, read state). The database
    /// file and key survive; the next fetch repopulates.
    pub fn wipe(&self) -> Result<usize> {
        let n = self.conn.execute("DELETE FROM messages", [])?
            + self.conn.execute("DELETE FROM chats", [])?
            + self.conn.execute("DELETE FROM read_state", [])?;
        Ok(n)
    }

    /// Row counts for the settings screen.
    pub fn stats(&self) -> Result<(usize, usize)> {
        let chats: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chats", [], |r| r.get(0))?;
        let messages: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
        Ok((chats as usize, messages as usize))
    }

    /// Record our read position for a chat (epoch milliseconds).
    pub fn set_read(&self, chat_id: &str, last_read_ms: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO read_state (chat_id, last_read_ms) VALUES (?1, ?2)",
            rusqlite::params![chat_id, last_read_ms as i64],
        )?;
        Ok(())
    }

    /// Our last recorded read position for a chat, if any.
    pub fn read_horizon(&self, chat_id: &str) -> Result<Option<u64>> {
        let ms: Option<i64> = self.conn.query_row(
            "SELECT last_read_ms FROM read_state WHERE chat_id = ?1",
            rusqlite::params![chat_id],
            |r| r.get(0),
        ).map_or(None, Some);
        Ok(ms.map(|m| m.max(0) as u64))
    }

    /// All read positions (chat → epoch ms), for badge computation.
    pub fn read_horizons(&self) -> Result<std::collections::HashMap<String, u64>> {
        let mut stmt =
            self.conn.prepare("SELECT chat_id, last_read_ms FROM read_state")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        let mut out = std::collections::HashMap::new();
        for r in rows.filter_map(Result::ok) {
            out.insert(r.0, r.1.max(0) as u64);
        }
        Ok(out)
    }

    /// Number of cached messages newer than `after_iso` in a chat — the
    /// unread-count approximation for badges.
    pub fn count_after(&self, chat_id: &str, after_iso: &str) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE chat_id = ?1 AND ts > ?2",
            rusqlite::params![chat_id, after_iso],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Local full-text search over cached messages, newest first. Backs the
    /// offline/tenant-blocked search path.
    pub fn search_messages(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, MessageInfo)>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let pat = format!(
            "%{}%",
            q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        let mut stmt = self.conn.prepare(
            "SELECT chat_id, id, ts, sender, sender_mri, content, raw, reply_to, client_message_id
             FROM messages
             WHERE content LIKE ?1 ESCAPE '\\'
             ORDER BY ts DESC LIMIT ?2",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![pat, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                MessageInfo {
                    id: row.get(1)?,
                    timestamp: row.get(2)?,
                    sender: row.get(3)?,
                    sender_mri: row.get(4)?,
                    content: row.get(5)?,
                    raw: row.get(6)?,
                    reactions: Vec::new(),
                    reply_to: row.get(7)?,
                    client_message_id: row.get(8)?,
                },
            ))
        })?;
        let mut out = Vec::new();
        while let Some(r) = rows.next() {
            out.push(r?);
        }
        Ok(out)
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
                client_message_id TEXT, PRIMARY KEY (chat_id, id));
            CREATE TABLE IF NOT EXISTS read_state (chat_id TEXT PRIMARY KEY,
                last_read_ms INTEGER NOT NULL);",
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

    #[test]
    fn read_state_and_wipe() {
        let (archive, dir) = temp_archive("read");
        let msgs: Vec<MessageInfo> = (0..3)
            .map(|i| MessageInfo {
                id: format!("m{i}"),
                sender_mri: "8:orgid:a".into(),
                sender: "A".into(),
                timestamp: format!("2026-10-06T10:0{i}:00.0000000Z"),
                content: "x".into(),
                raw: "<div>x</div>".into(),
                reactions: Vec::new(),
                reply_to: None,
                client_message_id: None,
            })
            .collect();
        archive.save_messages("19:c", &msgs).unwrap();
        archive.set_read("19:c", 1_760_000_000_000).unwrap();
        assert_eq!(
            archive.read_horizon("19:c").unwrap(),
            Some(1_760_000_000_000)
        );
        assert_eq!(archive.read_horizon("19:none").unwrap(), None);
        assert_eq!(
            archive.count_after("19:c", "2026-10-06T10:01:00.0000000Z").unwrap(),
            1
        );
        let mut all = archive.read_horizons().unwrap();
        assert_eq!(all.remove("19:c"), Some(1_760_000_000_000));
        assert!(all.is_empty());
        let (chats, messages) = archive.stats().unwrap();
        assert_eq!((chats, messages), (0, 3));
        let n = archive.wipe().unwrap();
        assert!(n >= 4);
        assert_eq!(archive.stats().unwrap(), (0, 0));
        assert_eq!(archive.read_horizon("19:c").unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
