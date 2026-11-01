//! SQLite storage owned by one background thread. All pages have a fixed upper bound.
use crate::domain::{validate_id, Conversation, Cursor, Message, MAX_PAGE_SIZE, MAX_PREVIEW_CHARS};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
pub mod directory;
pub mod e2ee;

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
                VALUES (?1,?2,COALESCE((SELECT display_name FROM directory_entries WHERE account_id=?1 AND thread_id=?2),?2),?3) ON CONFLICT(account_id,thread_id) DO UPDATE SET
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
        if version > 4 {
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
            CREATE TABLE IF NOT EXISTS outgoing (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, client_id TEXT NOT NULL,
                body TEXT NOT NULL, created_at_ms INTEGER NOT NULL, state TEXT NOT NULL,
                server_id TEXT, PRIMARY KEY(account_id,client_id));
            CREATE INDEX IF NOT EXISTS outgoing_thread ON outgoing(account_id,thread_id,state);
            CREATE INDEX IF NOT EXISTS outgoing_server ON outgoing(account_id,thread_id,server_id);
            CREATE TABLE IF NOT EXISTS outgoing_counters(account_id TEXT PRIMARY KEY,last_id INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS receive_cursors(account_id TEXT NOT NULL,queue TEXT NOT NULL,last_id TEXT NOT NULL,
                PRIMARY KEY(account_id,queue));
            ")?;
        directory::initialize(&conn)?;
        e2ee::initialize(&conn)?;
        conn.execute_batch("PRAGMA user_version=4; COMMIT;")?;
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

    /// Persist the idempotency key before dispatch. Unknown delivery is never blindly replayed.
    pub fn prepare_outgoing(
        &mut self,
        account: &str,
        thread: &str,
        text: &str,
        now_ms: i64,
    ) -> Result<u64> {
        validate_id(account)?;
        validate_id(thread)?;
        if text.trim().is_empty() || text.len() > crate::domain::MAX_TEXT_BYTES || now_ms < 1 {
            bail!("Invalid outgoing message");
        }
        let tx = self.conn.transaction()?;
        let unresolved: i64 = tx.query_row(
            "SELECT COUNT(*) FROM outgoing WHERE account_id=?1 AND state IN ('sending','unknown','acknowledged')",
            [account],
            |r| r.get(0),
        )?;
        if unresolved >= 100 {
            bail!("Too many unresolved sends");
        }
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND body=?3 AND state IN ('sending','unknown','acknowledged'))",params![account,thread,text],|r|r.get(0))?;
        if duplicate {
            bail!("This draft has an unresolved send. Wait for its server echo before sending it again.");
        }
        let previous: Option<i64> = tx
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |r| r.get(0),
            )
            .optional()?;
        let next = now_ms.max(
            previous
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Client ID exhausted"))?,
        );
        if next > 9_007_199_254_740_991 {
            bail!("Client ID exhausted");
        }
        tx.execute("INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",params![account,next])?;
        tx.execute("INSERT INTO outgoing(account_id,thread_id,client_id,body,created_at_ms,state) VALUES(?1,?2,?3,?4,?5,'sending')",params![account,thread,next.to_string(),text,now_ms])?;
        tx.commit()?;
        Ok(next as u64)
    }
    pub fn recover_outgoing(&self, account: &str) -> Result<()> {
        validate_id(account)?;
        self.conn.execute(
            "UPDATE outgoing SET state='unknown' WHERE account_id=?1 AND state='sending'",
            [account],
        )?;
        Ok(())
    }
    pub fn fail_outgoing(&self, account: &str, client_id: u64, unknown: bool) -> Result<()> {
        validate_id(account)?;
        self.conn.execute(
            "UPDATE outgoing SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",
            params![
                account,
                client_id.to_string(),
                if unknown { "unknown" } else { "rejected" }
            ],
        )?;
        Ok(())
    }
    /// Record the server ID independently: local message insertion can fail without losing the ACK.
    pub fn record_outgoing_ack(
        &self,
        account: &str,
        client_id: u64,
        server_id: &str,
    ) -> Result<()> {
        validate_id(account)?;
        validate_id(server_id)?;
        let changed=self.conn.execute("UPDATE outgoing SET state='acknowledged',server_id=?3 WHERE account_id=?1 AND client_id=?2 AND (server_id IS NULL OR server_id=?3)",params![account,client_id.to_string(),server_id])?;
        if changed != 1 {
            bail!("Conflicting or missing outgoing acknowledgement");
        }
        Ok(())
    }
    pub fn reconcile_outgoing(&mut self, account: &str) -> Result<()> {
        validate_id(account)?;
        let rows: Vec<(String, String)> = {
            let mut statement=self.conn.prepare("SELECT client_id,server_id FROM outgoing WHERE account_id=?1 AND state='acknowledged' LIMIT 100")?;
            let rows = statement.query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (client, server) in rows {
            self.accept_outgoing(account, client.parse()?, &server, None)?;
        }
        Ok(())
    }
    pub fn accept_outgoing(
        &mut self,
        account: &str,
        client_id: u64,
        server_id: &str,
        timestamp: Option<i64>,
    ) -> Result<(Message, Conversation)> {
        validate_id(account)?;
        validate_id(server_id)?;
        let tx = self.conn.transaction()?;
        let(thread,text,created,previous_id):(String,String,i64,Option<String>)=tx.query_row("SELECT thread_id,body,created_at_ms,server_id FROM outgoing WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','accepted')",params![account,client_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        if previous_id.as_deref().is_some_and(|id| id != server_id) {
            bail!("Conflicting server acknowledgement");
        }
        let existing_ts:Option<i64>=tx.query_row("SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,server_id],|r|r.get(0)).optional()?;
        let message = Message {
            account_id: account.into(),
            thread_id: thread.clone(),
            id: server_id.into(),
            sender: account.into(),
            text,
            sent_at_ms: existing_ts.or(timestamp).unwrap_or(created),
        };
        message.validate()?;
        insert_validated(&tx, std::slice::from_ref(&message))?;
        tx.execute("UPDATE outgoing SET state='accepted',server_id=?3 WHERE account_id=?1 AND client_id=?2",params![account,client_id.to_string(),server_id])?;
        let conversation = tx.query_row(
            CONVERSATION_SQL,
            params![account, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        tx.commit()?;
        Ok((message, conversation))
    }
    /// Incoming rows and replay cursor commit together. A failure cannot skip server data.
    pub fn receive_batch(
        &mut self,
        account: &str,
        messages: &[(Message, Option<String>)],
        cursor: Option<(&str, &str)>,
    ) -> Result<Vec<(Message, Conversation)>> {
        validate_id(account)?;
        if messages.len() > MAX_PAGE_SIZE {
            bail!("Receive batch exceeds limit");
        }
        for (m, _) in messages {
            m.validate()?;
            if m.account_id != account {
                bail!("Incoming account mismatch");
            }
        }
        if let Some((queue, id)) = cursor {
            validate_id(queue)?;
            validate_id(id)?;
            let number = id.parse::<u64>()?;
            if number == 0 || number > 9_007_199_254_740_991 {
                bail!("Invalid receive cursor");
            }
        }
        let tx = self.conn.transaction()?;
        let mut result = Vec::with_capacity(messages.len());
        for (m, client_id) in messages {
            let mut adjusted = 0;
            if m.sender == account {
                if let Some(client_id) = client_id {
                    let original:Option<(String,String,Option<String>)>=tx.query_row("SELECT thread_id,body,server_id FROM outgoing WHERE account_id=?1 AND client_id=?2",params![account,client_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
                    if let Some((thread, body, previous_id)) = original {
                        if previous_id.as_deref().is_some_and(|id| id != m.id) {
                            bail!("Conflicting server echo");
                        }
                        if thread != m.thread_id || body != m.text {
                            bail!("Echo does not match outgoing message");
                        }
                        tx.execute("UPDATE outgoing SET state='accepted',server_id=?3 WHERE account_id=?1 AND client_id=?2",params![account,client_id,m.id])?;
                        adjusted=tx.execute("UPDATE messages SET sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?1 AND body=?5 AND sent_at_ms<>?4",params![account,m.thread_id,m.id,m.sent_at_ms,m.text])?;
                    }
                }
            }
            let inserted = insert_validated(&tx, std::slice::from_ref(m))?;
            if adjusted > 0 {
                tx.execute("UPDATE conversations SET last_message_at_ms=(SELECT MAX(sent_at_ms) FROM messages WHERE account_id=?1 AND thread_id=?2) WHERE account_id=?1 AND thread_id=?2",params![account,m.thread_id])?;
            }
            if inserted > 0 || adjusted > 0 {
                let c = tx.query_row(
                    CONVERSATION_SQL,
                    params![account, m.thread_id, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )?;
                result.push((m.clone(), c));
            }
        }
        if let Some((queue, id)) = cursor {
            tx.execute("INSERT INTO receive_cursors(account_id,queue,last_id) VALUES(?1,?2,?3) ON CONFLICT(account_id,queue) DO UPDATE SET last_id=CASE WHEN length(excluded.last_id)>length(last_id) OR (length(excluded.last_id)=length(last_id) AND excluded.last_id>last_id) THEN excluded.last_id ELSE last_id END",params![account,queue,id])?;
        }
        tx.commit()?;
        Ok(result)
    }
    pub fn receive_cursor(&self, account: &str, queue: &str) -> Result<String> {
        validate_id(account)?;
        validate_id(queue)?;
        Ok(self
            .conn
            .query_row(
                "SELECT last_id FROM receive_cursors WHERE account_id=?1 AND queue=?2",
                params![account, queue],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "1".into()))
    }

    pub fn open_conversation(
        &self,
        account: &str,
        thread: &str,
        title: &str,
    ) -> Result<Conversation> {
        validate_id(account)?;
        validate_id(thread)?;
        if title.is_empty() || title.len() > 512 {
            bail!("Invalid conversation title");
        }
        self.conn.execute("INSERT INTO conversations(account_id,thread_id,title,last_message_at_ms) VALUES(?1,?2,?3,0) ON CONFLICT(account_id,thread_id) DO NOTHING",params![account,thread,title])?;
        Ok(self.conn.query_row(
            CONVERSATION_SQL,
            params![account, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?)
    }
    pub fn set_title(&self, account: &str, thread: &str, title: &str) -> Result<()> {
        validate_id(account)?;
        validate_id(thread)?;
        if title.is_empty() || title.len() > 512 {
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

#[cfg(test)]
mod live_storage_tests {
    use super::*;
    fn outgoing(s: &mut MessageStore) -> u64 {
        s.prepare_outgoing("11", "d:22", "hello", 1700000000000)
            .unwrap()
    }
    fn echo(id: &str, ts: i64) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: id.into(),
            sender: "11".into(),
            text: "hello".into(),
            sent_at_ms: ts,
        }
    }
    #[test]
    fn client_ids_are_persisted_monotonic_scoped_safe_integers_and_unknown_is_never_replayed() {
        let mut s = MessageStore::in_memory().unwrap();
        let first = outgoing(&mut s);
        assert_eq!(first, 1700000000000);
        s.fail_outgoing("11", first, true).unwrap();
        assert!(s
            .prepare_outgoing("11", "d:22", "hello", 1700000000000)
            .is_err());
        let second = s.prepare_outgoing("11", "d:22", "different", 1).unwrap();
        assert_eq!(second, first + 1);
        assert_eq!(s.prepare_outgoing("33", "d:22", "hello", 1).unwrap(), 1);
        s.recover_outgoing("11").unwrap();
        assert!(s.prepare_outgoing("11", "d:22", "different", 1).is_err());
        assert!(s
            .prepare_outgoing("11", "d:22", "overflow", 9_007_199_254_740_992)
            .is_err());
    }
    #[test]
    fn echo_before_and_after_http_ack_never_duplicates_and_cannot_rebind_server_id() {
        for echo_first in [true, false] {
            let mut s = MessageStore::in_memory().unwrap();
            let id = outgoing(&mut s);
            if echo_first {
                s.receive_batch("11", &[(echo("900", 20), Some(id.to_string()))], None)
                    .unwrap();
            }
            let (m, _) = s.accept_outgoing("11", id, "900", Some(19)).unwrap();
            assert_eq!(m.sent_at_ms, if echo_first { 20 } else { 19 });
            s.receive_batch("11", &[(echo("900", 20), Some(id.to_string()))], None)
                .unwrap();
            assert_eq!(
                s.page("11", "d:22", None, 10).unwrap(),
                vec![echo("900", 20)]
            );
            assert!(s.accept_outgoing("11", id, "901", None).is_err());
            assert!(s
                .receive_batch("11", &[(echo("901", 21), Some(id.to_string()))], None)
                .is_err());
            assert_eq!(s.page("11", "d:22", None, 10).unwrap().len(), 1);
        }
    }
    #[test]
    fn ack_is_kept_when_message_insert_fails_and_reconciliation_never_sends_network() {
        let mut s = MessageStore::in_memory().unwrap();
        let id = outgoing(&mut s);
        s.record_outgoing_ack("11", id, "900").unwrap();
        s.conn.execute_batch("CREATE TRIGGER fail_live BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(s.accept_outgoing("11", id, "900", None).is_err());
        let row: (String, String) = s
            .conn
            .query_row(
                "SELECT state,server_id FROM outgoing WHERE account_id='11'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("acknowledged".into(), "900".into()));
        assert!(s.prepare_outgoing("11", "d:22", "hello", 1).is_err());
        s.conn.execute_batch("DROP TRIGGER fail_live").unwrap();
        s.reconcile_outgoing("11").unwrap();
        assert_eq!(s.page("11", "d:22", None, 10).unwrap().len(), 1);
    }
    #[test]
    fn receive_cursor_is_atomic_account_scoped_and_never_moves_back_on_duplicate_replay() {
        let mut s = MessageStore::in_memory().unwrap();
        let m = echo("900", 20);
        assert_eq!(
            s.receive_batch("11", &[(m.clone(), None)], Some(("510_0", "100")))
                .unwrap()
                .len(),
            1
        );
        assert!(s
            .receive_batch(
                "11",
                &[(
                    Message {
                        text: "conflict".into(),
                        ..m.clone()
                    },
                    None
                )],
                Some(("510_0", "200"))
            )
            .is_err());
        assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
        assert!(s
            .receive_batch("33", &[(m.clone(), None)], Some(("510_0", "200")))
            .is_err());
        assert_eq!(s.receive_cursor("33", "510_0").unwrap(), "1");
        assert!(s
            .receive_batch("11", &[(m, None)], Some(("510_0", "99")))
            .unwrap()
            .is_empty());
        assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
    }
    #[test]
    fn reopening_target_preserves_title_latest_timestamp_and_preview() {
        let mut s = MessageStore::in_memory().unwrap();
        s.insert_batch(&[echo("900", 20)]).unwrap();
        s.set_title("11", "d:22", "Known friend").unwrap();
        let c = s.open_conversation("11", "d:22", "Zalo ID 22").unwrap();
        assert_eq!(c.title, "Known friend");
        assert_eq!(c.last_message_at_ms, 20);
        assert_eq!(c.last_text, "hello");
    }
}
