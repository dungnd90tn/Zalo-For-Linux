//! SQLite storage owned by one background thread. All pages have a fixed upper bound.
use crate::domain::{validate_id, Conversation, Cursor, Message, MAX_PAGE_SIZE, MAX_PREVIEW_CHARS};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const CURSOR_PAGE_SQL: &str =
    "SELECT account_id,thread_id,message_id,sender,body,sent_at_ms FROM messages
    WHERE account_id=?1 AND thread_id=?2 AND (sent_at_ms,message_id)<(?3,?4)
    ORDER BY sent_at_ms DESC,message_id DESC LIMIT ?5";
const CONVERSATIONS_SQL: &str = "SELECT c.thread_id,c.title,c.last_message_at_ms,
    COALESCE(substr(m.body,1,?3),''),COALESCE(m.sender,'') FROM conversations c
    LEFT JOIN messages m ON m.rowid=(SELECT recent.rowid FROM messages recent
        WHERE recent.account_id=c.account_id AND recent.thread_id=c.thread_id
        ORDER BY recent.sent_at_ms DESC,recent.message_id DESC LIMIT 1)
    WHERE c.account_id=?1 ORDER BY c.last_message_at_ms DESC,c.thread_id LIMIT ?2";
const CONVERSATION_SQL: &str = "SELECT c.thread_id,c.title,c.last_message_at_ms,
    COALESCE(substr(m.body,1,?3),''),COALESCE(m.sender,'') FROM conversations c
    LEFT JOIN messages m ON m.rowid=(SELECT recent.rowid FROM messages recent
        WHERE recent.account_id=c.account_id AND recent.thread_id=c.thread_id
        ORDER BY recent.sent_at_ms DESC,recent.message_id DESC LIMIT 1)
    WHERE c.account_id=?1 AND c.thread_id=?2";
fn conversation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: r.get(0)?,
        title: r.get(1)?,
        last_message_at_ms: r.get(2)?,
        last_text: r.get(3)?,
        last_sender: r.get(4)?,
    })
}
fn insert_validated(conn: &Connection, messages: &[Message]) -> Result<usize> {
    let mut inserted = 0;
    {
        let mut thread = conn.prepare_cached(
            "INSERT INTO conversations(account_id,thread_id,title,last_message_at_ms)
                VALUES (?1,?2,?2,?3) ON CONFLICT(account_id,thread_id) DO UPDATE SET
                last_message_at_ms=MAX(last_message_at_ms,excluded.last_message_at_ms)",
        )?;
        let mut existing = conn.prepare_cached(
            "SELECT sender,body,sent_at_ms FROM messages
                WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?;
        let mut insert = conn.prepare_cached(
            "INSERT INTO messages(account_id,thread_id,message_id,sender,body,sent_at_ms)
                VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        for m in messages {
            let previous: Option<(String, String, i64)> = existing
                .query_row(params![m.account_id, m.thread_id, m.id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            if let Some(previous) = previous {
                if previous != (m.sender.clone(), m.text.clone(), m.sent_at_ms) {
                    bail!("Conflicting payload for an existing message ID");
                }
                continue;
            }
            thread.execute(params![m.account_id, m.thread_id, m.sent_at_ms])?;
            inserted += insert.execute(params![
                m.account_id,
                m.thread_id,
                m.id,
                m.sender,
                m.text,
                m.sent_at_ms
            ])?;
        }
    }
    Ok(inserted)
}

pub struct MessageStore {
    conn: Connection,
}

impl MessageStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::initialize(conn)
    }
    pub fn in_memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }
    fn initialize(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            bail!("Database was created by a newer client");
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS conversations (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, title TEXT NOT NULL,
                last_message_at_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, thread_id));
            CREATE TABLE IF NOT EXISTS messages (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, message_id TEXT NOT NULL,
                sender TEXT NOT NULL, body TEXT NOT NULL, sent_at_ms INTEGER NOT NULL,
                PRIMARY KEY (account_id, thread_id, message_id),
                FOREIGN KEY (account_id, thread_id) REFERENCES conversations(account_id, thread_id));
            CREATE INDEX IF NOT EXISTS message_page ON messages(account_id, thread_id, sent_at_ms DESC, message_id DESC);
            CREATE INDEX IF NOT EXISTS conversation_recent ON conversations(account_id, last_message_at_ms DESC, thread_id);
            PRAGMA user_version=1; COMMIT;")?;
        Ok(Self { conn })
    }

    /// Atomic and replay-safe. Conflicting payloads under an existing server ID are refused.
    pub fn insert_batch(&mut self, messages: &[Message]) -> Result<usize> {
        if messages.len() > MAX_PAGE_SIZE {
            bail!("Batch exceeds {MAX_PAGE_SIZE} messages");
        }
        for message in messages {
            message.validate()?;
        }
        let tx = self.conn.transaction()?;
        let inserted = insert_validated(&tx, messages)?;
        tx.commit()?;
        Ok(inserted)
    }

    /// ACK metadata is read before commit: a snapshot failure rolls back the write.
    pub fn insert_with_conversation(&mut self, message: &Message) -> Result<Conversation> {
        message.validate()?;
        let tx = self.conn.transaction()?;
        insert_validated(&tx, std::slice::from_ref(message))?;
        let snapshot = tx.query_row(
            CONVERSATION_SQL,
            params![
                message.account_id,
                message.thread_id,
                MAX_PREVIEW_CHARS as i64
            ],
            conversation_row,
        )?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn set_title(&self, account: &str, thread: &str, title: &str) -> Result<()> {
        validate_id(account)?;
        validate_id(thread)?;
        if title.is_empty() || title.len() > 256 {
            bail!("Invalid conversation title");
        }
        self.conn.execute("INSERT INTO conversations(account_id,thread_id,title,last_message_at_ms) VALUES (?1,?2,?3,0)
            ON CONFLICT(account_id,thread_id) DO UPDATE SET title=excluded.title", params![account,thread,title])?;
        Ok(())
    }

    /// Newest page first in SQL, then reversed for chronological display. Cursor excludes its row.
    pub fn page(
        &self,
        account: &str,
        thread: &str,
        before: Option<&Cursor>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        validate_id(account)?;
        validate_id(thread)?;
        if limit == 0 || limit > MAX_PAGE_SIZE {
            bail!("Page size must be 1–{MAX_PAGE_SIZE}");
        }
        if let Some(cursor) = before {
            validate_id(&cursor.id)?;
        }
        let row = |r: &rusqlite::Row<'_>| {
            Ok(Message {
                account_id: r.get(0)?,
                thread_id: r.get(1)?,
                id: r.get(2)?,
                sender: r.get(3)?,
                text: r.get(4)?,
                sent_at_ms: r.get(5)?,
            })
        };
        let mut result: Vec<Message> = if let Some(cursor) = before {
            let mut stmt = self.conn.prepare_cached(CURSOR_PAGE_SQL)?;
            let mapped = stmt.query_map(
                params![account, thread, cursor.sent_at_ms, cursor.id, limit as i64],
                row,
            )?;
            mapped.collect::<rusqlite::Result<_>>()?
        } else {
            let mut stmt = self.conn.prepare_cached("SELECT account_id,thread_id,message_id,sender,body,sent_at_ms FROM messages
                WHERE account_id=?1 AND thread_id=?2 ORDER BY sent_at_ms DESC,message_id DESC LIMIT ?3")?;
            let mapped = stmt.query_map(params![account, thread, limit as i64], row)?;
            mapped.collect::<rusqlite::Result<_>>()?
        };
        result.reverse();
        Ok(result)
    }

    pub fn conversations(&self, account: &str, limit: usize) -> Result<Vec<Conversation>> {
        validate_id(account)?;
        if limit == 0 || limit > MAX_PAGE_SIZE {
            bail!("Invalid conversation page size");
        }
        let mut stmt = self.conn.prepare_cached(CONVERSATIONS_SQL)?;
        let rows = stmt.query_map(
            params![account, limit as i64, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete(&mut self, account: &str, thread: &str, id: &str) -> Result<bool> {
        validate_id(account)?;
        validate_id(thread)?;
        validate_id(id)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "DELETE FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![account, thread, id],
        )?;
        tx.execute("UPDATE conversations SET last_message_at_ms=COALESCE((SELECT MAX(sent_at_ms) FROM messages
            WHERE account_id=?1 AND thread_id=?2),0) WHERE account_id=?1 AND thread_id=?2",params![account,thread])?;
        tx.commit()?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(account: &str, thread: &str, id: &str, ts: i64) -> Message {
        Message {
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "A".into(),
            text: format!("Message {id}"),
            sent_at_ms: ts,
        }
    }
    #[test]
    fn deep_page_query_seeks_composite_range_without_temporary_sort() {
        let store = MessageStore::in_memory().unwrap();
        let mut statement = store
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CURSOR_PAGE_SQL}"))
            .unwrap();
        let plan: Vec<String> = statement
            .query_map(params!["a", "t", 10, "m", 50], |r| r.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" ");
        assert!(
            plan.contains("SEARCH messages USING INDEX message_page"),
            "{plan}"
        );
        assert!(plan.contains("(sent_at_ms,message_id)<(?,?)"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
    #[test]
    fn conversation_preview_seeks_latest_indexed_row_without_history_scan() {
        let store = MessageStore::in_memory().unwrap();
        let mut statement = store
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CONVERSATIONS_SQL}"))
            .unwrap();
        let plan: Vec<String> = statement
            .query_map(params!["a", 50, MAX_PREVIEW_CHARS as i64], |r| r.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" ");
        assert!(
            plan.contains("SEARCH c USING INDEX conversation_recent"),
            "{plan}"
        );
        assert!(
            plan.contains("SEARCH recent USING COVERING INDEX message_page"),
            "{plan}"
        );
        assert!(
            plan.contains("SEARCH m USING INTEGER PRIMARY KEY"),
            "{plan}"
        );
        assert!(!plan.contains("SCAN "), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
    #[test]
    fn conversation_preview_uses_actual_tie_order_unicode_and_account_scope() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut latest = m("a", "t", "z", 10);
        latest.text = "ế🦀".repeat(MAX_PREVIEW_CHARS);
        latest.sender = "Bạn An".into();
        store
            .insert_batch(&[m("a", "t", "a", 10), latest.clone(), m("b", "t", "z", 20)])
            .unwrap();
        store.set_title("a", "empty", "Empty").unwrap();
        let threads = store.conversations("a", 10).unwrap();
        assert_eq!(
            threads[0].last_text,
            latest
                .text
                .chars()
                .take(MAX_PREVIEW_CHARS)
                .collect::<String>()
        );
        assert_eq!(threads[0].last_sender, "Bạn An");
        assert_eq!(threads[0].last_text.chars().count(), MAX_PREVIEW_CHARS);
        assert!(threads[1].last_text.is_empty());
        assert!(threads[1].last_sender.is_empty());
        store.delete("a", "t", "z").unwrap();
        let threads = store.conversations("a", 10).unwrap();
        assert_eq!(threads[0].last_text, "Message a");
        assert_eq!(threads[0].last_sender, "A");
        assert_eq!(
            store.conversations("b", 10).unwrap()[0].last_message_at_ms,
            20
        );
    }
    #[test]
    fn durable_ack_snapshot_uses_exact_indexed_thread_and_latest_message() {
        let mut store = MessageStore::in_memory().unwrap();
        store
            .insert_batch(&[m("a", "t", "newest", 20), m("b", "t", "other", 30)])
            .unwrap();
        let snapshot = store
            .insert_with_conversation(&m("a", "t", "backfill", 10))
            .unwrap();
        assert_eq!(snapshot.last_text, "Message newest");
        assert_eq!(snapshot.last_message_at_ms, 20);
        let mut stmt = store
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CONVERSATION_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(params!["a", "t", MAX_PREVIEW_CHARS as i64], |r| r.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" ");
        assert!(plan.contains("SEARCH c USING INDEX sqlite_autoindex_conversations_1 (account_id=? AND thread_id=?)"),"{plan}");
        assert!(
            plan.contains("SEARCH recent USING COVERING INDEX message_page"),
            "{plan}"
        );
        assert!(
            !plan.contains("SCAN ") && !plan.contains("TEMP B-TREE"),
            "{plan}"
        );
    }
    #[test]
    fn failed_ack_metadata_read_rolls_back_message_instead_of_committing_failure() {
        let mut store = MessageStore::in_memory().unwrap();
        store.set_title("a", "t", "Original title").unwrap();
        store.conn.execute_batch("CREATE TRIGGER bad_snapshot AFTER INSERT ON messages BEGIN
            UPDATE conversations SET title=X'FF' WHERE account_id=NEW.account_id AND thread_id=NEW.thread_id; END;").unwrap();
        assert!(store
            .insert_with_conversation(&m("a", "t", "not-committed", 10))
            .is_err());
        assert!(store.page("a", "t", None, 10).unwrap().is_empty());
        assert_eq!(
            store.conversations("a", 10).unwrap()[0].title,
            "Original title"
        );
        assert_eq!(
            store.conversations("a", 10).unwrap()[0].last_message_at_ms,
            0
        );
    }
    #[test]
    fn keyset_pages_have_no_gaps_or_duplicates_at_equal_timestamps() {
        let mut s = MessageStore::in_memory().unwrap();
        let messages: Vec<_> = (0..11)
            .map(|i| m("a", "t", &format!("m{i:02}"), 10))
            .collect();
        assert_eq!(s.insert_batch(&messages).unwrap(), 11);
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let page = s.page("a", "t", cursor.as_ref(), 3).unwrap();
            if page.is_empty() {
                break;
            }
            cursor = Some(page[0].cursor());
            ids.extend(page.into_iter().map(|m| m.id));
        }
        ids.sort();
        assert_eq!(
            ids,
            messages.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
        );
    }
    #[test]
    fn dedupe_and_delete_are_scoped_to_account_and_thread() {
        let mut s = MessageStore::in_memory().unwrap();
        let messages = [
            m("a", "t", "same", 10),
            m("b", "t", "same", 10),
            m("a", "other", "same", 10),
            m("a", "t", "next", 10),
        ];
        assert_eq!(s.insert_batch(&messages).unwrap(), 4);
        assert_eq!(s.insert_batch(&messages).unwrap(), 0);
        assert!(s.delete("a", "t", "same").unwrap());
        assert_eq!(s.page("a", "t", None, 10).unwrap().len(), 1);
        assert_eq!(s.page("b", "t", None, 10).unwrap().len(), 1);
        assert_eq!(s.page("a", "other", None, 10).unwrap().len(), 1);
    }
    #[test]
    fn conflicting_replay_rolls_back_entire_batch() {
        let mut s = MessageStore::in_memory().unwrap();
        let existing = m("a", "t", "same", 10);
        s.insert_batch(std::slice::from_ref(&existing)).unwrap();
        let mut conflict = existing;
        conflict.text = "Changed".into();
        assert!(s.insert_batch(&[m("a", "t", "new", 11), conflict]).is_err());
        assert_eq!(s.page("a", "t", None, 10).unwrap().len(), 1);
        assert_eq!(s.conversations("a", 10).unwrap()[0].last_message_at_ms, 10);
    }
    #[test]
    fn newest_page_and_delete_update_conversation_order() {
        let mut s = MessageStore::in_memory().unwrap();
        s.insert_batch(&[m("a", "t", "old", 1), m("a", "t", "new", 20)])
            .unwrap();
        assert_eq!(s.page("a", "t", None, 1).unwrap()[0].id, "new");
        s.delete("a", "t", "new").unwrap();
        assert_eq!(s.conversations("a", 10).unwrap()[0].last_message_at_ms, 1);
        assert!(s.page("a", "t", None, MAX_PAGE_SIZE + 1).is_err());
    }
}
