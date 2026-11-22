//! Ordinary reply metadata; raw quotes stay in the storage worker.
use super::*;
use crate::domain::{MessageQuote, QuoteContent};
use serde_json::json;

const MAX_PROJECTION: usize = 256 * 1024;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_reply(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
        source_client_id TEXT CHECK(source_client_id IS NULL OR length(source_client_id)<=24),
        quote_metadata TEXT CHECK(quote_metadata IS NULL OR length(CAST(quote_metadata AS BLOB))<=131072),
        quote_projection TEXT CHECK(quote_projection IS NULL OR length(CAST(quote_projection AS BLOB))<=262144),
        quote_source INTEGER NOT NULL DEFAULT 0 CHECK(quote_source IN (0,1)),
        reply_blocked INTEGER NOT NULL DEFAULT 0 CHECK(reply_blocked IN (0,1)),
        PRIMARY KEY(account_id,thread_id,message_id),
        FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS outgoing_reply(
        account_id TEXT NOT NULL,client_id TEXT NOT NULL,
        quote_metadata TEXT NOT NULL CHECK(length(CAST(quote_metadata AS BLOB))<=131072),
        quote_projection TEXT NOT NULL CHECK(length(CAST(quote_projection AS BLOB))<=262144),
        PRIMARY KEY(account_id,client_id),
        FOREIGN KEY(account_id,client_id) REFERENCES outgoing(account_id,client_id) ON DELETE CASCADE);")?;
    Ok(())
}
fn decode(value: &str) -> Result<MessageQuote> {
    if value.len() > MAX_PROJECTION {
        bail!(FailureReason::ConflictingMetadata);
    }
    let quote: MessageQuote =
        serde_json::from_str(value).map_err(|_| FailureReason::ConflictingMetadata)?;
    quote
        .validate()
        .map_err(|_| FailureReason::ConflictingMetadata)?;
    Ok(quote)
}
pub(super) fn synthetic(quote: &MessageQuote) -> Result<QuoteContent> {
    QuoteContent::new(
        &quote.owner_id,
        &json!({"ownerId":quote.owner_id,"globalMsgId":quote.message_id,
        "cliMsgId":quote.client_id,"cliMsgType":quote.message_type,"ts":quote.sent_at_ms,"msg":quote.text,"ttl":0}),
    )
}
type Row = (Option<String>, Option<String>, Option<String>, bool, bool);
fn canonical_reference_fields(value: &mut serde_json::Value, account: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.remove("cliMsgId");
    for key in ["ownerId", "fromUid", "globalMsgId", "msgId"] {
        let Some(value) = object.get_mut(key) else {
            continue;
        };
        let text = value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_u64().map(|id| id.to_string()));
        if let Some(mut text) = text {
            if text == "0" && matches!(key, "ownerId" | "fromUid") {
                text = account.into();
            }
            if crate::domain::reply::valid_source_id(&text) {
                *value = serde_json::Value::String(text);
            }
        }
    }
    if let Some(value) = object.get_mut("ts") {
        if let Some(number) = value
            .as_i64()
            .or_else(|| value.as_str()?.parse::<i64>().ok())
            .filter(|number| *number >= 0)
        {
            *value = json!(number);
        }
    }
}
pub(super) fn merge_quote(old: &MessageQuote, incoming: &MessageQuote) -> Result<MessageQuote> {
    let mut merged = incoming.clone();
    merged.client_id = incoming.client_id.clone().or_else(|| old.client_id.clone());
    let mut previous = old.clone();
    previous.client_id = merged.client_id.clone();
    if previous.message_type == 0
        && previous.text == expiration::PREVIEW
        && merged.message_type == 0
        && merged.text == expiration::PREVIEW
    {
        previous.ttl = merged.ttl;
    }
    if !previous.same_reference(&merged)
        || old
            .client_id
            .as_ref()
            .zip(incoming.client_id.as_ref())
            .is_some_and(|(a, b)| a != b)
    {
        bail!(FailureReason::ConflictingMetadata);
    }
    Ok(merged)
}

pub(super) fn redact_quote(
    conn: &Connection,
    account: &str,
    thread: &str,
    quote: &mut MessageQuote,
) -> Result<bool> {
    if delete_for_me::redact_quote(conn, account, thread, quote)? {
        return Ok(true);
    }
    if expiration::redact_quote(conn, account, thread, quote)? {
        return Ok(true);
    }
    #[cfg(feature = "messaging")]
    return recall::redact_quote(conn, account, thread, quote);
    #[cfg(not(feature = "messaging"))]
    Ok(false)
}

pub(super) fn redact_copies(
    conn: &Connection,
    account: &str,
    thread: &str,
    id: &str,
    actor: &str,
    replacement: &str,
) -> Result<()> {
    let args = params![account, thread, id, actor, replacement];
    conn.execute(
        "UPDATE message_reply SET quote_projection=json_set(quote_projection,'$.text',?5,'$.message_type',0),
           quote_metadata=NULL,quote_source=0
         WHERE account_id=?1 AND thread_id=?2 AND json_valid(quote_projection)
           AND json_extract(quote_projection,'$.message_id')=?3
           AND json_extract(quote_projection,'$.owner_id')=?4",
        args,
    )?;
    conn.execute(
        "UPDATE outgoing_reply SET quote_projection=json_set(quote_projection,'$.text',?5,'$.message_type',0),
           quote_metadata=json_object('globalMsgId',?3,'ownerId',?4,'cliMsgType',0,'msg',?5)
         WHERE account_id=?1 AND json_valid(quote_projection)
           AND client_id IN (SELECT client_id FROM outgoing WHERE account_id=?1 AND thread_id=?2)
           AND json_extract(quote_projection,'$.message_id')=?3
           AND json_extract(quote_projection,'$.owner_id')=?4",
        args,
    )?;
    Ok(())
}

pub(super) fn persist(
    conn: &Connection,
    message: &Message,
    content: Option<&QuoteContent>,
) -> Result<bool> {
    let mut incoming = content
        .map(|raw| raw.projection(&message.account_id))
        .transpose()?;
    let mut projected = message.quote.clone();
    let (content, quote_redacted) = {
        let mut redacted = false;
        for candidate in [&mut incoming, &mut projected].into_iter().flatten() {
            redacted |= redact_quote(conn, &message.account_id, &message.thread_id, candidate)?;
        }
        (content.filter(|_| !redacted), redacted)
    };
    incoming = match (projected.take(), incoming) {
        (Some(projected), Some(raw)) => Some(merge_quote(&raw, &projected)?),
        (Some(projected), None) => Some(projected),
        (None, incoming) => incoming,
    };
    let old: Option<Row> = conn.query_row("SELECT source_client_id,quote_metadata,quote_projection,quote_source,reply_blocked FROM message_reply WHERE account_id=?1 AND thread_id=?2 AND message_id=?3", params![message.account_id,message.thread_id,message.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
    let (old_client, old_raw, old_projection, old_wire, old_blocked) =
        old.clone().unwrap_or((None, None, None, false, false));
    let blocked = old_blocked || message.reply_blocked;
    if old_client
        .as_ref()
        .zip(message.source_client_id.as_ref())
        .is_some_and(|(a, b)| a != b)
    {
        bail!(FailureReason::ConflictingMetadata);
    }
    let client = message.source_client_id.clone().or(old_client);
    let previous = old_projection.as_deref().map(decode).transpose()?;
    if let (Some(previous), Some(value)) = (previous.as_ref(), incoming.as_ref()) {
        incoming = Some(merge_quote(previous, value)?);
    }
    let mut raw = old_raw.filter(|_| !quote_redacted);
    let mut projection = old_projection;
    let mut wire = old_wire && !quote_redacted;
    if let Some(quote) = incoming {
        if let Some(content) = content {
            if old_wire && raw.as_deref() != Some(content.as_json()) {
                let mut old: serde_json::Value = serde_json::from_str(
                    raw.as_deref().ok_or(FailureReason::ConflictingMetadata)?,
                )?;
                let mut new: serde_json::Value = serde_json::from_str(content.as_json())?;
                canonical_reference_fields(&mut old, &message.account_id);
                canonical_reference_fields(&mut new, &message.account_id);
                if old != new {
                    bail!(FailureReason::ConflictingMetadata);
                }
            }
            // A wire replay may add an optional client ID but never erase one.
            if !old_wire || content.projection(&message.account_id)?.client_id.is_some() {
                raw = Some(content.as_json().into());
            }
            wire = true;
        } else if raw.is_none() && !quote_redacted {
            raw = Some(synthetic(&quote)?.as_json().into());
        }
        projection = Some(serde_json::to_string(&quote)?);
    }
    if client.is_none() && projection.is_none() && !blocked {
        return Ok(false);
    }
    if old.as_ref().is_some_and(|old| {
        old == &(
            client.clone(),
            raw.clone(),
            projection.clone(),
            wire,
            blocked,
        )
    }) {
        return Ok(false);
    }
    conn.execute("INSERT INTO message_reply(account_id,thread_id,message_id,source_client_id,quote_metadata,quote_projection,quote_source,reply_blocked) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
        ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET source_client_id=excluded.source_client_id,quote_metadata=excluded.quote_metadata,quote_projection=excluded.quote_projection,quote_source=excluded.quote_source,reply_blocked=excluded.reply_blocked", params![message.account_id,message.thread_id,message.id,client,raw,projection,wire,blocked])?;
    Ok(true)
}
pub(super) fn hydrate(conn: &Connection, message: &mut Message) -> Result<()> {
    let mut statement = conn.prepare_cached(
        "SELECT r.source_client_id,r.quote_projection,r.reply_blocked,
            EXISTS(SELECT 1 FROM recall_tombstones t WHERE t.account_id=?1 AND t.thread_id=?2
                 AND t.message_id=?3 AND t.actor_id=?4 AND t.materialized=1),
            (SELECT e.deadline_ms FROM message_expirations e
                WHERE e.account_id=?1 AND e.thread_id=?2 AND e.message_id=?3 AND e.sender_id=?4)
         FROM (SELECT 1) LEFT JOIN message_reply r
         ON r.account_id=?1 AND r.thread_id=?2 AND r.message_id=?3",
    )?;
    let (client, quote, blocked, recalled, deadline): (
        Option<String>,
        Option<String>,
        Option<bool>,
        bool,
        Option<i64>,
    ) = statement.query_row(
        params![
            message.account_id,
            message.thread_id,
            message.id,
            message.sender
        ],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    message.expires_at_ms = expiration::earliest(message.expires_at_ms, deadline);
    message.reply_blocked |= message.expires_at_ms.is_some();
    if recalled {
        super::recall::redact(message);
    } else if let Some(blocked) = blocked {
        message.source_client_id = client;
        message.quote = quote.as_deref().map(decode).transpose()?;
        message.reply_blocked |= blocked;
        super::mention::hydrate(conn, message)?;
    }
    Ok(())
}

pub(super) fn outgoing(
    conn: &Connection,
    account: &str,
    client_id: u64,
) -> Result<Option<MessageQuote>> {
    let projection: Option<String> = conn
        .query_row(
            "SELECT quote_projection FROM outgoing_reply WHERE account_id=?1 AND client_id=?2",
            params![account, client_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    projection.as_deref().map(decode).transpose()
}

impl MessageStore {
    /// Read-only worker access to full quote data; history only hydrates projection.
    pub fn message_quote_content(
        &self,
        account: &str,
        thread: &str,
        message: &str,
    ) -> Result<Option<QuoteContent>> {
        validate_id(account)?;
        validate_id(thread)?;
        validate_id(message)?;
        if !self.live_thread_visible(account, thread)? {
            return Ok(None);
        }
        let raw:Option<Option<String>> = self.conn.query_row("SELECT quote_metadata FROM message_reply WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,message],|row|row.get(0)).optional()?;
        raw.flatten()
            .map(|raw| QuoteContent::new(account, &serde_json::from_str(&raw)?))
            .transpose()
    }
    pub fn outgoing_quote(&self, account: &str, client_id: u64) -> Result<Option<MessageQuote>> {
        validate_id(account)?;
        let thread: Option<String> = self
            .conn
            .query_row(
                "SELECT thread_id FROM outgoing WHERE account_id=?1 AND client_id=?2",
                params![account, client_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(thread) = thread else {
            return Ok(None);
        };
        if !self.live_thread_visible(account, &thread)? {
            return Ok(None);
        }
        outgoing(&self.conn, account, client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Message {
        Message {
            expires_at_ms: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "9001".into(),
            sender: "22".into(),
            text: "Nguồn tổng hợp".into(),
            source_client_id: Some("8001".into()),
            quote: None,
            sticker: None,
            media: None,
            sent_at_ms: 100,
        }
    }
    #[test]
    fn reply_preparation_respects_privacy_and_never_reserves_plaintext_for_e2ee() {
        let mut store = MessageStore::in_memory().unwrap();
        store.insert_batch(&[source()]).unwrap();
        store.begin_live_privacy("11").unwrap();
        assert!(store
            .prepare_outgoing_reply("11", "d:22", "Bản nháp", "9001", 1000)
            .unwrap_err()
            .is::<crate::domain::reply::ReplyUnavailable>());
        store
            .replace_live_privacy("11", &["d:22".into()], None)
            .unwrap();
        assert!(store
            .prepare_outgoing_reply("11", "d:22", "Bản nháp", "9001", 1000)
            .is_err());
        store.unlock_live_thread("11", Some("d:22")).unwrap();
        store
            .conn
            .execute("INSERT INTO e2ee_threads VALUES('11','d:22')", [])
            .unwrap();
        assert!(store
            .prepare_outgoing_reply("11", "d:22", "Bản nháp", "9001", 1000)
            .is_err());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM outgoing", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        store.conn.execute("DELETE FROM e2ee_threads", []).unwrap();
        let id = store
            .prepare_outgoing_reply("11", "d:22", "Bản nháp", "9001", 1000)
            .unwrap();
        assert!(store.outgoing_quote("11", id).unwrap().is_some());
        store.unlock_live_thread("11", None).unwrap();
        assert!(store.outgoing_quote("11", id).unwrap().is_none());
    }
}
