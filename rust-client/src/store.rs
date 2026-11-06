//! SQLite storage owned by one background thread. All pages have a fixed upper bound.
use crate::domain::{
    canonical_plain_text, validate_id, Conversation, Cursor, InfoMessageContent,
    MediaMessageContent, Message, MAX_INFO_BATCH_BYTES, MAX_PAGE_SIZE, MAX_PREVIEW_CHARS,
};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
pub mod directory;
pub mod e2ee;
pub mod message_content;
mod message_info;
mod message_media;
mod privacy;

pub const MAX_REFRESH_MESSAGES: usize = 50;

/// One received message with optional outbox hint and complete rich sidecars.
/// The immutable metadata commits with its message and receive cursor.
pub type ReceivedMedia = (
    Message,
    Option<String>,
    Option<InfoMessageContent>,
    Option<MediaMessageContent>,
);

/// Closed, payload-free storage diagnostics. Never log an arbitrary error chain:
/// database driver messages can include SQL or data from a failing trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    ConflictingMessage,
    ConflictingEcho,
    OutgoingMismatch,
    ConflictingMetadata,
    Validation,
    Sqlite,
    Other,
}
impl std::fmt::Display for FailureReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ConflictingMessage => "Conflicting stored message",
            Self::ConflictingEcho => "Conflicting outgoing acknowledgement or echo",
            Self::OutgoingMismatch => "Echo does not match its outgoing message",
            Self::ConflictingMetadata => "Conflicting stored message metadata",
            Self::Validation => "Invalid storage input",
            Self::Sqlite => "Database operation failed",
            Self::Other => "Storage operation failed",
        })
    }
}
impl std::error::Error for FailureReason {}

/// Structural mismatch only: no text, identifiers, URLs, or keys are retained.
#[derive(Debug, Clone, Copy)]
pub struct OutgoingMismatchFields {
    thread: bool,
    body: bool,
    content_type: bool,
}
impl OutgoingMismatchFields {
    /// Bit 0: thread; bit 1: body; bit 2: rich content instead of plain text.
    pub fn mask(self) -> u8 {
        u8::from(self.thread) | (u8::from(self.body) << 1) | (u8::from(self.content_type) << 2)
    }
}
impl std::fmt::Display for OutgoingMismatchFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Outgoing echo fields do not match")
    }
}
impl std::error::Error for OutgoingMismatchFields {}

pub fn classify_failure(error: &anyhow::Error) -> FailureReason {
    // anyhow typed contexts participate in downcasting, but are not necessarily
    // exposed as that concrete type by std::error::Error::source/chain.
    if let Some(reason) = error.downcast_ref::<FailureReason>() {
        return *reason;
    }
    if let Some(reason) = error
        .chain()
        .find_map(|source| source.downcast_ref::<FailureReason>())
    {
        return *reason;
    }
    if error.chain().any(|source| source.is::<rusqlite::Error>()) {
        FailureReason::Sqlite
    } else {
        FailureReason::Other
    }
}

const CURSOR_PAGE_SQL: &str =
    "SELECT m.account_id,m.thread_id,m.message_id,m.sender,m.body,m.sent_at_ms,
    CASE WHEN length(CAST(content.metadata AS BLOB))<=256 THEN content.metadata ELSE NULL END FROM messages m
    LEFT JOIN message_content content ON content.account_id=m.account_id AND content.thread_id=m.thread_id AND content.message_id=m.message_id
    WHERE m.account_id=?1 AND m.thread_id=?2 AND (m.sent_at_ms,m.message_id)<(?3,?4)
    ORDER BY m.sent_at_ms DESC,m.message_id DESC LIMIT ?5";
const CONVERSATIONS_SQL: &str = "SELECT c.thread_id,c.title,c.last_message_at_ms,
    CASE WHEN media.message_id IS NOT NULL THEN CASE WHEN length(CAST(media.preview AS BLOB))<=1024 THEN media.preview END WHEN content.kind='sticker' THEN '[Nhãn dán]' ELSE COALESCE(substr(m.body,1,?3),'') END,COALESCE(m.sender,'') FROM conversations c
    LEFT JOIN messages m ON m.rowid=(SELECT recent.rowid FROM messages recent
        WHERE recent.account_id=c.account_id AND recent.thread_id=c.thread_id
        ORDER BY recent.sent_at_ms DESC,recent.message_id DESC LIMIT 1)
    LEFT JOIN message_content content ON content.account_id=m.account_id AND content.thread_id=m.thread_id AND content.message_id=m.message_id
    LEFT JOIN message_media media ON media.account_id=m.account_id AND media.thread_id=m.thread_id AND media.message_id=m.message_id
    WHERE c.account_id=?1
    AND NOT EXISTS(SELECT 1 FROM temp.live_privacy p WHERE p.account_id=c.account_id AND p.ready=0)
    AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h WHERE h.account_id=c.account_id AND h.thread_id=c.thread_id)
    ORDER BY c.last_message_at_ms DESC,c.thread_id LIMIT ?2";
const CONVERSATION_SQL: &str = "SELECT c.thread_id,c.title,c.last_message_at_ms,
    CASE WHEN media.message_id IS NOT NULL THEN CASE WHEN length(CAST(media.preview AS BLOB))<=1024 THEN media.preview END WHEN content.kind='sticker' THEN '[Nhãn dán]' ELSE COALESCE(substr(m.body,1,?3),'') END,COALESCE(m.sender,'') FROM conversations c
    LEFT JOIN messages m ON m.rowid=(SELECT recent.rowid FROM messages recent
        WHERE recent.account_id=c.account_id AND recent.thread_id=c.thread_id
        ORDER BY recent.sent_at_ms DESC,recent.message_id DESC LIMIT 1)
    LEFT JOIN message_content content ON content.account_id=m.account_id AND content.thread_id=m.thread_id AND content.message_id=m.message_id
    LEFT JOIN message_media media ON media.account_id=m.account_id AND media.thread_id=m.thread_id AND media.message_id=m.message_id
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
    insert_validated_with_info(conn, messages.iter().map(|message| (message, None)))
}
fn insert_validated_with_info<'a>(
    conn: &Connection,
    messages: impl IntoIterator<Item = (&'a Message, Option<&'a InfoMessageContent>)>,
) -> Result<usize> {
    insert_validated_with_media(
        conn,
        messages
            .into_iter()
            .map(|(message, info)| (message, info, None)),
    )
}
fn insert_validated_with_media<'a>(
    conn: &Connection,
    messages: impl IntoIterator<
        Item = (
            &'a Message,
            Option<&'a InfoMessageContent>,
            Option<&'a MediaMessageContent>,
        ),
    >,
) -> Result<usize> {
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
        for (m, info, media) in messages {
            if (info.is_some() && (media.is_some() || m.media.is_some()))
                || (m.sticker.is_some() && media.is_some())
            {
                bail!(FailureReason::Validation);
            }
            let previous: Option<(String, String, i64)> = existing
                .query_row(params![m.account_id, m.thread_id, m.id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            if let Some(previous) = previous {
                if previous != (m.sender.clone(), m.text.clone(), m.sent_at_ms) {
                    bail!(FailureReason::ConflictingMessage);
                }
                message_content::persist(conn, m, true)?;
                message_info::persist(conn, m, info, true)?;
                message_media::persist(conn, m, media, true)?;
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
            message_content::persist(conn, m, false)?;
            message_info::persist(conn, m, info, false)?;
            message_media::persist(conn, m, media, false)?;
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
        if version > 7 {
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
        message_content::initialize(&conn)?;
        message_info::initialize(&conn)?;
        message_media::initialize(&conn, version)?;
        conn.execute_batch("PRAGMA user_version=7; COMMIT;")?;
        privacy::initialize(&conn)?;
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
        if !self.live_thread_visible(account, thread)? {
            bail!(FailureReason::Validation);
        }
        if text.len() > crate::domain::MAX_TEXT_BYTES || now_ms < 1 {
            bail!("Invalid outgoing message");
        }
        let text = canonical_plain_text(text);
        if text.is_empty() {
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
        let duplicate = {
            // Released unresolved intents may contain the original untrimmed
            // draft. Compare canonically without modifying or redispatching them.
            // The account count above is <100; stream one capped body at a time.
            let mut statement=tx.prepare_cached("SELECT CASE WHEN length(CAST(body AS BLOB))<=?3 THEN body ELSE NULL END FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND state IN ('sending','unknown','acknowledged') LIMIT 100")?;
            let rows = statement.query_map(
                params![account, thread, crate::domain::MAX_TEXT_BYTES as i64],
                |row| row.get::<_, Option<String>>(0),
            )?;
            let mut duplicate = false;
            for body in rows {
                let body = body?.ok_or(FailureReason::Validation)?;
                if canonical_plain_text(&body) == text {
                    duplicate = true;
                    break;
                }
            }
            duplicate
        };
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
        let changed=self.conn.execute("UPDATE outgoing SET state='acknowledged',server_id=?3 WHERE account_id=?1 AND client_id=?2 AND (server_id IS NULL OR server_id=?3)
            AND NOT EXISTS(SELECT 1 FROM outgoing other WHERE other.account_id=outgoing.account_id AND other.thread_id=outgoing.thread_id AND other.server_id=?3 AND other.client_id<>outgoing.client_id)",params![account,client_id.to_string(),server_id])?;
        if changed != 1 {
            bail!(FailureReason::ConflictingEcho);
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
            bail!(FailureReason::ConflictingEcho);
        }
        let ambiguous: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND client_id<>?4)",
            params![account,thread,server_id,client_id.to_string()],
            |row| row.get(0),
        )?;
        if ambiguous {
            bail!(FailureReason::ConflictingEcho);
        }
        let existing_ts:Option<i64>=tx.query_row("SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,server_id],|r|r.get(0)).optional()?;
        let message = Message {
            account_id: account.into(),
            thread_id: thread.clone(),
            id: server_id.into(),
            sender: account.into(),
            text,
            sticker: None,
            media: None,
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
        self.receive_batch_inner(
            account,
            messages
                .iter()
                .map(|(message, client)| (message, client, None, None)),
            messages.len(),
            cursor,
        )
    }

    /// Full info content is immutable and commits in the very same transaction
    /// as its text projection, outbox updates, and durable replay cursor.
    pub fn receive_batch_with_info(
        &mut self,
        account: &str,
        messages: &[(Message, Option<String>, Option<InfoMessageContent>)],
        cursor: Option<(&str, &str)>,
    ) -> Result<Vec<(Message, Conversation)>> {
        self.receive_batch_inner(
            account,
            messages
                .iter()
                .map(|(message, client, info)| (message, client, info.as_ref(), None)),
            messages.len(),
            cursor,
        )
    }

    pub fn receive_batch_with_media(
        &mut self,
        account: &str,
        messages: &[ReceivedMedia],
        cursor: Option<(&str, &str)>,
    ) -> Result<Vec<(Message, Conversation)>> {
        self.receive_batch_inner(
            account,
            messages
                .iter()
                .map(|(m, c, i, media)| (m, c, i.as_ref(), media.as_ref())),
            messages.len(),
            cursor,
        )
    }

    fn receive_batch_inner<'a>(
        &mut self,
        account: &str,
        messages: impl Iterator<
                Item = (
                    &'a Message,
                    &'a Option<String>,
                    Option<&'a InfoMessageContent>,
                    Option<&'a MediaMessageContent>,
                ),
            > + Clone,
        message_count: usize,
        cursor: Option<(&str, &str)>,
    ) -> Result<Vec<(Message, Conversation)>> {
        validate_id(account).map_err(|_| FailureReason::Validation)?;
        if message_count > MAX_PAGE_SIZE {
            bail!(FailureReason::Validation);
        }
        let mut info_bytes = 0usize;
        for (m, _, info, media) in messages.clone() {
            m.validate().map_err(|_| FailureReason::Validation)?;
            info_bytes = info_bytes.saturating_add(info.map_or(0, |info| info.as_json().len()));
            info_bytes = info_bytes.saturating_add(media.map_or(0, |media| media.as_json().len()));
            if m.account_id != account
                || (info.is_some() && m.sticker.is_some())
                || (info.is_some() && (media.is_some() || m.media.is_some()))
                || (media.is_some() && m.sticker.is_some())
                || info_bytes > MAX_INFO_BATCH_BYTES
            {
                bail!(FailureReason::Validation);
            }
        }
        if let Some((queue, id)) = cursor {
            validate_id(queue).map_err(|_| FailureReason::Validation)?;
            validate_id(id).map_err(|_| FailureReason::Validation)?;
            let number = id.parse::<u64>().map_err(|_| FailureReason::Validation)?;
            if number == 0 || number > 9_007_199_254_740_991 {
                bail!(FailureReason::Validation);
            }
        }
        let tx = self.conn.transaction()?;
        let mut result = Vec::with_capacity(message_count);
        for (m, client_id, info, media) in messages {
            let mut adjusted = 0;
            // System/info events can carry source cliMsgId values but are not
            // acknowledgements of the user's plain-text send intent.
            if m.sender == account
                && info.is_none()
                && media.is_none()
                && m.media.is_none()
                && m.sticker.is_none()
            {
                let explicit: Option<(String, String, String, Option<String>)> = client_id
                    .as_ref()
                    .map(|client_id| {
                        tx.query_row("SELECT client_id,thread_id,body,server_id FROM outgoing WHERE account_id=?1 AND client_id=?2",params![account,client_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()
                    })
                    .transpose()?
                    .flatten();
                // When cliMsgId does not resolve, an already recorded HTTP ACK
                // provides a separate, durable binding to that message.
                // Query at most two rows because the server-ID index is not unique.
                let bound: Vec<(String, String, String, Option<String>)> = {
                    let mut statement=tx.prepare_cached("SELECT client_id,thread_id,body,server_id FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 LIMIT 2")?;
                    let rows = statement.query_map(params![account, m.thread_id, m.id], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                    })?;
                    rows.collect::<rusqlite::Result<_>>()?
                };
                if bound.len() > 1
                    || explicit
                        .as_ref()
                        .zip(bound.first())
                        .is_some_and(|(a, b)| a.0 != b.0)
                {
                    bail!(FailureReason::ConflictingEcho);
                }
                if let Some((client_id, thread, body, previous_id)) =
                    explicit.or_else(|| bound.into_iter().next())
                {
                    if previous_id.as_deref().is_some_and(|id| id != m.id) {
                        bail!(FailureReason::ConflictingEcho);
                    }
                    // Released versions sent/stored the untrimmed composer draft.
                    // Repair only its exact, already ACK-bound server echo. No
                    // arbitrary text normalization, ID reassignment, or replay send.
                    let repair_body = body != m.text
                        && previous_id.as_deref() == Some(m.id.as_str())
                        && canonical_plain_text(&body) == m.text
                        && !m.text.is_empty();
                    let mismatch = OutgoingMismatchFields {
                        thread: thread != m.thread_id,
                        body: body != m.text && !repair_body,
                        content_type: m.sticker.is_some() || info.is_some(),
                    };
                    if mismatch.mask() != 0 {
                        return Err(
                            anyhow::Error::new(mismatch).context(FailureReason::OutgoingMismatch)
                        );
                    }
                    if repair_body {
                        let rich:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",params![account,m.thread_id,m.id],|row|row.get(0))?;
                        if rich {
                            bail!(FailureReason::ConflictingMetadata);
                        }
                        let existing:Option<(String,String)>=tx.query_row("SELECT sender,body FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,m.thread_id,m.id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
                        if existing
                            .as_ref()
                            .is_some_and(|(sender, text)| sender != account || text != &body)
                        {
                            bail!(FailureReason::ConflictingMessage);
                        }
                        adjusted=tx.execute("UPDATE messages SET body=?6,sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?1 AND body=?5",params![account,m.thread_id,m.id,m.sent_at_ms,body,m.text])?;
                        tx.execute(
                            "UPDATE outgoing SET body=?3 WHERE account_id=?1 AND client_id=?2",
                            params![account, client_id, m.text],
                        )?;
                    }
                    tx.execute("UPDATE outgoing SET state='accepted',server_id=?3 WHERE account_id=?1 AND client_id=?2",params![account,client_id,m.id])?;
                    adjusted+=tx.execute("UPDATE messages SET sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?1 AND body=?5 AND sent_at_ms<>?4",params![account,m.thread_id,m.id,m.sent_at_ms,m.text])?;
                }
            }
            let sticker_enriched = m.sticker.is_some() && message_content::load(&tx, m)?.is_none();
            let media_enriched = media.is_some() && !message_media::exists(&tx, m)?;
            let inserted = insert_validated_with_media(&tx, std::iter::once((m, info, media)))?;
            if adjusted > 0 {
                tx.execute("UPDATE conversations SET last_message_at_ms=(SELECT MAX(sent_at_ms) FROM messages WHERE account_id=?1 AND thread_id=?2) WHERE account_id=?1 AND thread_id=?2",params![account,m.thread_id])?;
            }
            if inserted > 0 || adjusted > 0 || sticker_enriched || media_enriched {
                let c = tx.query_row(
                    CONVERSATION_SQL,
                    params![account, m.thread_id, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )?;
                let mut hydrated = m.clone();
                hydrated.media = message_media::projection(&tx, m)?;
                result.push((hydrated, c));
            }
        }
        if let Some((queue, id)) = cursor {
            tx.execute("INSERT INTO receive_cursors(account_id,queue,last_id) VALUES(?1,?2,?3) ON CONFLICT(account_id,queue) DO UPDATE SET last_id=CASE WHEN length(excluded.last_id)>length(last_id) OR (length(excluded.last_id)=length(last_id) AND excluded.last_id>last_id) THEN excluded.last_id ELSE last_id END",params![account,queue,id])?;
        }
        tx.commit()?;
        Ok(result)
    }

    /// Explicit bounded worker-side access; pages never load full rich payloads.
    pub fn message_info(
        &self,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<InfoMessageContent>> {
        validate_id(account).map_err(|_| FailureReason::Validation)?;
        validate_id(thread).map_err(|_| FailureReason::Validation)?;
        if !self.live_thread_visible(account, thread)? {
            return Ok(None);
        }
        validate_id(id).map_err(|_| FailureReason::Validation)?;
        message_info::load(&self.conn, account, thread, id)
    }

    /// Full private content is fetched explicitly by the SQLite owner; normal
    /// history snapshots only hydrate its bounded presentation projection.
    pub fn message_media(
        &self,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<MediaMessageContent>> {
        validate_id(account).map_err(|_| FailureReason::Validation)?;
        validate_id(thread).map_err(|_| FailureReason::Validation)?;
        if !self.live_thread_visible(account, thread)? {
            return Ok(None);
        }
        validate_id(id).map_err(|_| FailureReason::Validation)?;
        message_media::load(&self.conn, account, thread, id)
    }
    /// Rehydrate only the caller's bounded displayed IDs. Missing rows never
    /// imply deletion; no chronological page or private raw sidecar is loaded.
    pub fn refresh_messages(
        &self,
        account: &str,
        thread: &str,
        message_ids: &[String],
    ) -> Result<Vec<Message>> {
        validate_id(account).map_err(|_| FailureReason::Validation)?;
        validate_id(thread).map_err(|_| FailureReason::Validation)?;
        if !self.live_thread_visible(account, thread)? {
            return Ok(Vec::new());
        }
        if message_ids.len() > MAX_REFRESH_MESSAGES {
            bail!(FailureReason::Validation);
        }
        let mut unique = std::collections::HashSet::with_capacity(message_ids.len());
        for id in message_ids {
            validate_id(id).map_err(|_| FailureReason::Validation)?;
            if !unique.insert(id) {
                bail!(FailureReason::Validation);
            }
        }
        let mut statement=self.conn.prepare_cached("SELECT m.account_id,m.thread_id,m.message_id,m.sender,m.body,m.sent_at_ms,
            CASE WHEN length(CAST(content.metadata AS BLOB))<=256 THEN content.metadata ELSE NULL END FROM messages m
            LEFT JOIN message_content content ON content.account_id=m.account_id AND content.thread_id=m.thread_id AND content.message_id=m.message_id
            WHERE m.account_id=?1 AND m.thread_id=?2 AND m.message_id=?3")?;
        let mut result = Vec::with_capacity(message_ids.len());
        for id in message_ids {
            let message = statement
                .query_row(params![account, thread, id], |row| {
                    Ok(Message {
                        account_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        id: row.get(2)?,
                        sender: row.get(3)?,
                        text: row.get(4)?,
                        sent_at_ms: row.get(5)?,
                        sticker: message_content::decode(row.get(6)?),
                        media: None,
                    })
                })
                .optional()?;
            if let Some(mut message) = message {
                message.media = message_media::projection(&self.conn, &message)?;
                message
                    .validate()
                    .map_err(|_| FailureReason::ConflictingMetadata)?;
                result.push(message);
            }
        }
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
        if !self.live_thread_visible(account, thread)? {
            bail!(FailureReason::Validation);
        }
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
        if !self.live_thread_visible(account, thread)? {
            return Ok(Vec::new());
        }
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
                sticker: message_content::decode(r.get(6)?),
                media: None,
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
            let mut stmt = self.conn.prepare_cached("SELECT m.account_id,m.thread_id,m.message_id,m.sender,m.body,m.sent_at_ms,
                CASE WHEN length(CAST(content.metadata AS BLOB))<=256 THEN content.metadata ELSE NULL END FROM messages m
                LEFT JOIN message_content content ON content.account_id=m.account_id AND content.thread_id=m.thread_id AND content.message_id=m.message_id
                WHERE m.account_id=?1 AND m.thread_id=?2 ORDER BY m.sent_at_ms DESC,m.message_id DESC LIMIT ?3")?;
            let mapped = stmt.query_map(params![account, thread, limit as i64], row)?;
            mapped.collect::<rusqlite::Result<_>>()?
        };
        for message in &mut result {
            message.media = message_media::projection(&self.conn, message)?;
            message
                .validate()
                .map_err(|_| FailureReason::ConflictingMetadata)?;
        }
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
            sticker: None,
            media: None,
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
        assert!(plan.contains("SEARCH m USING INDEX message_page"), "{plan}");
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
            sticker: None,
            media: None,
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
    fn acknowledged_echo_with_missing_or_foreign_client_id_reconciles_server_timestamp() {
        for timestamp in [None, Some(1700000000003)] {
            for client_id in [None, Some("123456789".to_owned())] {
                let mut s = MessageStore::in_memory().unwrap();
                let id = outgoing(&mut s);
                s.record_outgoing_ack("11", id, "900").unwrap();
                s.accept_outgoing("11", id, "900", timestamp).unwrap();
                let received = s
                    .receive_batch(
                        "11",
                        &[(echo("900", 1700000000007), client_id.clone())],
                        Some(("510_0", "100")),
                    )
                    .unwrap();
                assert_eq!(received.len(), 1);
                assert_eq!(received[0].0.sent_at_ms, 1700000000007);
                assert_eq!(received[0].1.last_message_at_ms, 1700000000007);
                assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
                assert!(s
                    .receive_batch(
                        "11",
                        &[(echo("900", 1700000000007), client_id)],
                        Some(("510_0", "101")),
                    )
                    .unwrap()
                    .is_empty());
                assert_eq!(s.page("11", "d:22", None, 10).unwrap().len(), 1);
            }
        }
    }
    #[test]
    fn legacy_acknowledged_boundary_whitespace_echo_is_reconciled_after_restart() {
        let path = std::env::temp_dir().join(format!(
            "zalo-legacy-text-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        Connection::open(&path)
            .unwrap()
            .execute_batch(include_str!(
                "../tests/fixtures/e2ee/released_v4_90b25d5.sql"
            ))
            .unwrap();
        {
            let mut s = MessageStore::open(&path).unwrap();
            let id = s
                .prepare_outgoing("11", "d:22", "  chào \r\n", 1700000000000)
                .unwrap();
            // Exact pre-fix persisted outgoing bytes, including boundary whitespace.
            s.conn
                .execute("UPDATE outgoing SET body=?1", ["  chào \r\n"])
                .unwrap();
            s.record_outgoing_ack("11", id, "900").unwrap();
            s.accept_outgoing("11", id, "900", None).unwrap();
        }
        {
            let mut s = MessageStore::open(&path).unwrap();
            let mut received = echo("900", 1700000000007);
            received.text = "chào".into();
            let result = s
                .receive_batch("11", &[(received.clone(), None)], Some(("510_0", "100")))
                .unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(
                s.page("11", "d:22", None, 10).unwrap(),
                vec![received.clone()]
            );
            assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
            let body: String = s
                .conn
                .query_row("SELECT body FROM outgoing", [], |row| row.get(0))
                .unwrap();
            assert_eq!(body, "chào");
            assert!(s
                .receive_batch("11", &[(received, None)], Some(("510_0", "101")))
                .unwrap()
                .is_empty());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn legacy_plain_send_persists_only_the_shared_canonical_body_without_editing_the_draft() {
        let mut s = MessageStore::in_memory().unwrap();
        let draft = "\u{feff} \tchào  bạn\n tốt \r\n".to_owned();
        let id = s
            .prepare_outgoing("11", "d:22", &draft, 1700000000000)
            .unwrap();
        let body: String = s
            .conn
            .query_row("SELECT body FROM outgoing", [], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "chào  bạn\n tốt");
        assert_eq!(draft, "\u{feff} \tchào  bạn\n tốt \r\n");
        s.fail_outgoing("11", id, true).unwrap();
        assert!(s
            .prepare_outgoing("11", "d:22", "chào  bạn\n tốt", 1700000000001)
            .is_err());
    }
    #[test]
    fn released_unresolved_raw_body_blocks_equivalent_new_draft_without_allocating_an_id() {
        for state in ["unknown", "sending", "acknowledged"] {
            let mut s = MessageStore::in_memory().unwrap();
            let id = s.prepare_outgoing("11", "d:22", "chào  bạn", 1000).unwrap();
            let raw = "\u{feff} \tchào  bạn \r\n";
            s.conn
                .execute("UPDATE outgoing SET body=?1,state=?2", params![raw, state])
                .unwrap();
            for draft in ["chào  bạn", raw] {
                assert!(
                    s.prepare_outgoing("11", "d:22", draft, 2000).is_err(),
                    "{state} must not be blindly resent"
                );
            }
            let stored:(String,String,i64)=s.conn.query_row("SELECT body,state,(SELECT last_id FROM outgoing_counters WHERE account_id='11') FROM outgoing WHERE account_id='11' AND client_id=?1",[id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
            assert_eq!(stored, (raw.into(), state.into(), 1000));
            let count: i64 = s
                .conn
                .query_row("SELECT count(*) FROM outgoing", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
            // Boundary equivalence does not collapse meaningful internal spaces.
            assert!(s.prepare_outgoing("11", "d:22", "chào bạn", 2000).is_ok());
        }
    }
    #[test]
    fn legacy_boundary_repair_is_atomic_and_never_relaxes_noncanonical_or_unbound_content() {
        for variant in 0..9 {
            let mut s = MessageStore::in_memory().unwrap();
            let id = s
                .prepare_outgoing("11", "d:22", "hello", 1700000000000)
                .unwrap();
            let old = " \u{feff}chào  bạn\n tốt\t ";
            s.conn
                .execute("UPDATE outgoing SET body=?1", [old])
                .unwrap();
            if variant != 6 {
                s.record_outgoing_ack("11", id, "900").unwrap();
                s.accept_outgoing("11", id, "900", Some(19)).unwrap();
            }
            s.receive_batch("11", &[], Some(("510_0", "100"))).unwrap();
            let mut incoming = echo("900", 20);
            incoming.text = "chào  bạn\n tốt".into();
            let mut client = None;
            match variant {
                0=>s.conn.execute_batch("CREATE TRIGGER reject_legacy_cursor BEFORE UPDATE ON receive_cursors BEGIN SELECT RAISE(ABORT,'synthetic cursor failure'); END;").unwrap(),
                1=>incoming.text="chào bạn\n tốt".into(),
                2=>incoming.text="cha\u{0300}o  bạn\n tốt".into(),
                3=>{incoming.thread_id="d:33".into();client=Some(id.to_string());},
                4=>incoming.sender="22".into(),
                5=>{incoming.text=crate::domain::LEGACY_STICKER_PLACEHOLDER.into();incoming.sticker=Some(crate::domain::StickerDescriptor{sticker_id:1,category_id:0,sticker_type:7});},
                6=>client=Some(id.to_string()),
                7=>{s.conn.execute("UPDATE messages SET body='unrelated stored content'",[]).unwrap();},
                _=>{s.conn.execute("INSERT INTO message_content(account_id,thread_id,message_id,kind,metadata) VALUES('11','d:22','900','sticker','{}')",[]).unwrap();},
            }
            let error = s
                .receive_batch("11", &[(incoming.clone(), client)], Some(("510_0", "101")))
                .unwrap_err();
            let expected = match variant {
                0 => FailureReason::Sqlite,
                4 | 5 | 7 => FailureReason::ConflictingMessage,
                8 => FailureReason::ConflictingMetadata,
                _ => FailureReason::OutgoingMismatch,
            };
            assert_eq!(classify_failure(&error), expected, "variant {variant}");
            if expected == FailureReason::OutgoingMismatch {
                let mask = error
                    .downcast_ref::<OutgoingMismatchFields>()
                    .unwrap()
                    .mask();
                assert!((1..=7).contains(&mask));
                if variant == 3 {
                    assert_eq!(mask, 1);
                }
            }
            let body: String = s
                .conn
                .query_row("SELECT body FROM outgoing", [], |r| r.get(0))
                .unwrap();
            assert_eq!(body, old);
            assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
            if variant == 0 {
                assert_eq!(s.page("11", "d:22", None, 10).unwrap()[0].text, old);
                s.conn
                    .execute_batch("DROP TRIGGER reject_legacy_cursor")
                    .unwrap();
                s.receive_batch("11", &[(incoming, None)], Some(("510_0", "101")))
                    .unwrap();
                assert_eq!(
                    s.page("11", "d:22", None, 10).unwrap()[0].text,
                    "chào  bạn\n tốt"
                );
            }
        }
    }
    #[test]
    fn acknowledged_echo_fallback_survives_restart_and_echo_before_ack() {
        let path = std::env::temp_dir().join(format!(
            "zalo-ack-echo-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        {
            let mut s = MessageStore::open(&path).unwrap();
            let id = outgoing(&mut s);
            s.record_outgoing_ack("11", id, "900").unwrap();
            s.accept_outgoing("11", id, "900", None).unwrap();
        }
        {
            let mut s = MessageStore::open(&path).unwrap();
            s.recover_outgoing("11").unwrap();
            s.receive_batch("11", &[(echo("900", 20), None)], Some(("510_0", "100")))
                .unwrap();
            assert_eq!(
                s.page("11", "d:22", None, 10).unwrap(),
                vec![echo("900", 20)]
            );
            assert!(s
                .receive_batch(
                    "11",
                    &[(echo("900", 20), Some("foreign".into()))],
                    Some(("510_0", "101"))
                )
                .unwrap()
                .is_empty());
        }
        std::fs::remove_file(path).unwrap();
        let mut s = MessageStore::in_memory().unwrap();
        let id = outgoing(&mut s);
        s.receive_batch("11", &[(echo("900", 20), Some("foreign".into()))], None)
            .unwrap();
        s.record_outgoing_ack("11", id, "900").unwrap();
        s.accept_outgoing("11", id, "900", Some(19)).unwrap();
        assert_eq!(
            s.page("11", "d:22", None, 10).unwrap(),
            vec![echo("900", 20)]
        );
    }
    #[test]
    fn echo_fallback_cannot_retime_arbitrary_rows_or_override_conflicting_client_binding() {
        for variant in 0..5 {
            let mut s = MessageStore::in_memory().unwrap();
            let id = outgoing(&mut s);
            s.record_outgoing_ack("11", id, "900").unwrap();
            s.accept_outgoing("11", id, "900", Some(19)).unwrap();
            s.receive_batch("11", &[], Some(("510_0", "100"))).unwrap();
            let mut incoming = echo("900", 20);
            let mut client = None;
            let expected = match variant {
                0 => {
                    incoming.text = "different body".into();
                    FailureReason::OutgoingMismatch
                }
                1 => {
                    incoming.sender = "22".into();
                    FailureReason::ConflictingMessage
                }
                2 => {
                    incoming.thread_id = "d:33".into();
                    let mut old = incoming.clone();
                    old.sent_at_ms = 19;
                    s.insert_batch(&[old]).unwrap();
                    FailureReason::ConflictingMessage
                }
                3 => {
                    incoming.id = "901".into();
                    client = Some(id.to_string());
                    FailureReason::ConflictingEcho
                }
                _ => {
                    let second = s.prepare_outgoing("11", "d:33", "different", 1).unwrap();
                    client = Some(second.to_string());
                    FailureReason::ConflictingEcho
                }
            };
            let mut fresh = echo("new", 21);
            fresh.sender = "22".into();
            let error = s
                .receive_batch(
                    "11",
                    &[(fresh, None), (incoming, client)],
                    Some(("510_0", "200")),
                )
                .unwrap_err();
            assert_eq!(classify_failure(&error), expected);
            assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
            assert_eq!(
                s.page("11", "d:22", None, 10).unwrap(),
                vec![echo("900", 19)]
            );
            let state: String = s
                .conn
                .query_row(
                    "SELECT state FROM outgoing WHERE account_id='11' AND client_id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(state, "accepted");
        }
    }
    #[test]
    fn server_id_binding_is_unique_per_thread_and_ambiguous_legacy_binding_is_rejected() {
        let mut s = MessageStore::in_memory().unwrap();
        let first = outgoing(&mut s);
        s.record_outgoing_ack("11", first, "900").unwrap();
        s.accept_outgoing("11", first, "900", Some(19)).unwrap();
        let second = s.prepare_outgoing("11", "d:22", "hello", 1).unwrap();
        assert_eq!(
            classify_failure(&s.record_outgoing_ack("11", second, "900").unwrap_err()),
            FailureReason::ConflictingEcho
        );
        assert_eq!(
            classify_failure(
                &s.accept_outgoing("11", second, "900", Some(19))
                    .unwrap_err()
            ),
            FailureReason::ConflictingEcho
        );
        s.conn
            .execute(
                "UPDATE outgoing SET server_id='900' WHERE client_id=?1",
                [second.to_string()],
            )
            .unwrap();
        let error = s
            .receive_batch("11", &[(echo("900", 20), None)], Some(("510_0", "100")))
            .unwrap_err();
        assert_eq!(classify_failure(&error), FailureReason::ConflictingEcho);
        assert_eq!(
            s.page("11", "d:22", None, 10).unwrap(),
            vec![echo("900", 19)]
        );
        assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "1");
        let third = s.prepare_outgoing("11", "d:33", "other thread", 1).unwrap();
        s.record_outgoing_ack("11", third, "900").unwrap();
        s.accept_outgoing("11", third, "900", Some(20)).unwrap();
        assert_eq!(
            s.page("11", "d:33", None, 10).unwrap()[0].text,
            "other thread"
        );
    }
    #[test]
    fn typed_storage_classification_never_requires_driver_or_payload_strings() {
        let error = anyhow::Error::new(FailureReason::ConflictingEcho)
            .context("private context is not a diagnostic field");
        assert_eq!(classify_failure(&error), FailureReason::ConflictingEcho);
        let s = MessageStore::in_memory().unwrap();
        let error: anyhow::Error = s
            .conn
            .execute_batch("SELECT private_fixture_secret FROM absent_fixture_table")
            .unwrap_err()
            .into();
        assert_eq!(classify_failure(&error), FailureReason::Sqlite);
        assert_eq!(
            classify_failure(&error).to_string(),
            "Database operation failed"
        );
    }
    #[test]
    fn echo_timestamp_reconciliation_and_outbox_state_roll_back_if_cursor_commit_fails() {
        let mut s = MessageStore::in_memory().unwrap();
        let id = outgoing(&mut s);
        s.record_outgoing_ack("11", id, "900").unwrap();
        s.accept_outgoing("11", id, "900", Some(19)).unwrap();
        s.receive_batch("11", &[], Some(("510_0", "100"))).unwrap();
        s.conn.execute_batch("UPDATE outgoing SET state='acknowledged'; CREATE TRIGGER reject_echo_cursor BEFORE UPDATE ON receive_cursors BEGIN SELECT RAISE(ABORT,'synthetic cursor failure'); END;").unwrap();
        let error = s
            .receive_batch("11", &[(echo("900", 20), None)], Some(("510_0", "101")))
            .unwrap_err();
        assert_eq!(classify_failure(&error), FailureReason::Sqlite);
        assert_eq!(
            s.page("11", "d:22", None, 10).unwrap(),
            vec![echo("900", 19)]
        );
        assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
        assert_eq!(s.conversations("11", 10).unwrap()[0].last_message_at_ms, 19);
        let state: String = s
            .conn
            .query_row(
                "SELECT state FROM outgoing WHERE account_id='11'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "acknowledged");
        s.conn
            .execute_batch("DROP TRIGGER reject_echo_cursor")
            .unwrap();
        assert_eq!(
            s.receive_batch("11", &[(echo("900", 20), None)], Some(("510_0", "101")))
                .unwrap()
                .len(),
            1
        );
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
