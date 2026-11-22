//! Disappearing text and its payload-free replay markers.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const PREVIEW: &str = "[Tin nhắn tự xóa]";
const EXPIRY_BATCH: i64 = 256;

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis().min(i64::MAX as u128) as i64)
}

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_expirations(
           account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
           sender_id TEXT NOT NULL,deadline_ms INTEGER NOT NULL CHECK(deadline_ms>0),
           purged INTEGER NOT NULL DEFAULT 0 CHECK(purged IN(0,1)),
           PRIMARY KEY(account_id,thread_id,message_id)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS message_expiry_due
           ON message_expirations(account_id,deadline_ms) WHERE purged=0;
         CREATE TABLE IF NOT EXISTS outgoing_expirations(
           account_id TEXT NOT NULL,client_id TEXT NOT NULL,ttl_ms INTEGER NOT NULL CHECK(ttl_ms>0),
           PRIMARY KEY(account_id,client_id),
           FOREIGN KEY(account_id,client_id) REFERENCES outgoing(account_id,client_id) ON DELETE CASCADE
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

pub(super) fn deadline_for_outgoing(
    conn: &Connection,
    account: &str,
    client_id: &str,
    sent_at_ms: i64,
) -> Result<Option<i64>> {
    let ttl = conn
        .prepare_cached(
            "SELECT ttl_ms FROM outgoing_expirations WHERE account_id=?1 AND client_id=?2",
        )?
        .query_row(params![account, client_id], |row| row.get::<_, i64>(0))
        .optional()?;
    ttl.map(|ttl| {
        sent_at_ms
            .checked_add(ttl)
            .ok_or_else(|| FailureReason::Validation.into())
    })
    .transpose()
}

fn deadline(conn: &Connection, message: &Message) -> Result<Option<(i64, bool)>> {
    let row: Option<(String, i64, bool)> = conn
        .prepare_cached(
            "SELECT sender_id,deadline_ms,purged FROM message_expirations
                        WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?
        .query_row(
            params![message.account_id, message.thread_id, message.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    match row {
        Some((sender, ..)) if sender != message.sender => bail!(FailureReason::ConflictingMessage),
        Some((_, deadline, purged)) => Ok(Some((deadline, purged))),
        None => Ok(None),
    }
}

/// Establish the deadline before any message or its quoted copies are projected.
/// A replay can confirm or shorten an existing lifetime, including after a restart.
pub(super) fn record(conn: &Connection, message: &Message) -> Result<Option<(i64, bool)>> {
    let previous = deadline(conn, message)?;
    let mut incoming = message.expires_at_ms;
    let mut expired_outgoing = false;
    if message.sender == message.account_id {
        let outgoing: Option<(String, i64, bool)> = conn.prepare_cached(
            "SELECT o.client_id,o.created_at_ms,o.state='expired'
             FROM outgoing o JOIN outgoing_expirations e USING(account_id,client_id)
             WHERE o.account_id=?1 AND o.thread_id=?2
             AND (o.server_id=?3 OR (o.client_id=?4 AND o.server_id IS NULL AND (o.state='expired' OR o.body=?5)))
             ORDER BY o.server_id IS NULL LIMIT 1",
        )?.query_row(params![message.account_id,message.thread_id,message.id,message.source_client_id,message.text], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        }).optional()?;
        if let Some((client_id, created_at_ms, expired)) = outgoing {
            let sent = if expired {
                created_at_ms
            } else {
                message.sent_at_ms
            };
            incoming = earliest(
                incoming,
                deadline_for_outgoing(conn, &message.account_id, &client_id, sent)?,
            );
            expired_outgoing = expired;
        }
    }
    let previous_deadline = previous.map(|(deadline, _)| deadline);
    let effective = earliest(previous_deadline, incoming);
    if let Some(deadline) = effective.filter(|_| effective != previous_deadline) {
        conn.execute(
            "INSERT INTO message_expirations(account_id,thread_id,message_id,sender_id,deadline_ms)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET deadline_ms=excluded.deadline_ms",
            params![message.account_id, message.thread_id, message.id, message.sender, deadline],
        )?;
        message_reply::redact_copies(
            conn,
            &message.account_id,
            &message.thread_id,
            &message.id,
            &message.sender,
            PREVIEW,
        )?;
    }
    if expired_outgoing {
        // A late own echo cannot put a scrubbed outbox body back into history,
        // even if the wall clock has been moved backwards since it expired.
        conn.execute(
            "UPDATE message_expirations AS e SET purged=NOT EXISTS(SELECT 1 FROM messages m
                 WHERE m.account_id=e.account_id AND m.thread_id=e.thread_id AND m.message_id=e.message_id),
                 deadline_ms=MIN(deadline_ms,?4)
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND purged=0",
            params![message.account_id,message.thread_id,message.id,now_ms().max(1)],
        )?;
    }
    Ok(effective.map(|deadline| {
        (
            deadline,
            expired_outgoing || previous.is_some_and(|(_, purged)| purged),
        )
    }))
}

/// The database owns time gating for all ingest paths; callers may still
/// encounter an already expired row before the asynchronous purge runs.
pub(super) fn expired(expiration: Option<(i64, bool)>, now_ms: i64) -> bool {
    expiration.is_some_and(|(deadline, purged)| purged || deadline <= now_ms)
}

pub(super) fn supports_text(message: &Message, structured: bool) -> bool {
    !structured
        && message.sticker.is_none()
        && message.media.is_none()
        && message.quote.is_none()
        && message.mentions.is_empty()
}

pub(super) fn structured_forward(marker: Option<&ForwardMarker>) -> bool {
    #[cfg(feature = "messaging")]
    return marker.is_some_and(|marker| !matches!(marker, ForwardMarker::Ordinary));
    #[cfg(not(feature = "messaging"))]
    return marker.is_some();
}

pub(super) fn earliest(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a.into_iter().chain(b).min()
}

pub(super) fn exists(conn: &Connection, account: &str, thread: &str, id: &str) -> Result<bool> {
    Ok(conn.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM message_expirations WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",
    )?.query_row(params![account,thread,id], |row| row.get(0))?)
}

/// Never duplicate the contents of disappearing messages into durable quote bodies.
pub(super) fn redact_quote(
    conn: &Connection,
    account: &str,
    thread: &str,
    quote: &mut MessageQuote,
) -> Result<bool> {
    let expiring = quote.ttl.is_some_and(|ttl| ttl > 0)
        || conn
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM message_expirations
         WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender_id=?4)",
            )?
            .query_row(
                params![account, thread, quote.message_id, quote.owner_id],
                |row| row.get(0),
            )?;
    if expiring {
        PREVIEW.clone_into(&mut quote.text);
        quote.message_type = 0;
    }
    Ok(expiring)
}

#[cfg(feature = "messaging")]
#[derive(Debug)]
pub struct ExpiredMessage {
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
    pub(crate) sender_id: String,
    pub(crate) conversation: Conversation,
}

impl MessageStore {
    #[cfg(feature = "messaging")]
    pub(crate) fn next_message_expiry(&self, account: &str) -> Result<Option<i64>> {
        validate_id(account)?;
        Ok(self.conn.query_row(
            "SELECT MIN(deadline) FROM (
                 SELECT MIN(deadline_ms) AS deadline FROM message_expirations WHERE account_id=?1 AND purged=0
                 UNION ALL
                 SELECT MIN(o.created_at_ms+e.ttl_ms) FROM outgoing o JOIN outgoing_expirations e USING(account_id,client_id)
                 WHERE o.account_id=?1 AND o.state IN ('sending','unknown','rejected','acknowledged')
             )",
            [account], |row| row.get(0),
        )?)
    }

    #[cfg(feature = "messaging")]
    pub(crate) fn expire_messages(
        &mut self,
        account: &str,
        now_ms: i64,
    ) -> Result<Vec<ExpiredMessage>> {
        validate_id(account)?;
        let tx = self.conn.transaction()?;
        let due = {
            let mut query = tx.prepare_cached(
                "SELECT thread_id,message_id,sender_id FROM message_expirations
                 WHERE account_id=?1 AND purged=0 AND deadline_ms<=?2 ORDER BY deadline_ms LIMIT ?3",
            )?;
            let rows = query.query_map(params![account, now_ms, EXPIRY_BATCH], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute(
            "UPDATE outgoing AS o SET body=?3,state='expired'
             WHERE account_id=?1 AND state IN ('sending','unknown','rejected','acknowledged') AND EXISTS(
               SELECT 1 FROM outgoing_expirations e WHERE e.account_id=o.account_id
               AND e.client_id=o.client_id AND o.created_at_ms+e.ttl_ms<=?2)",
            params![account, now_ms, PREVIEW],
        )?;
        let mut changes = Vec::new();
        for (thread, id, sender) in due {
            tx.execute(
                "UPDATE message_expirations SET purged=1 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
            )?;
            if account == sender {
                tx.execute("UPDATE outgoing SET body=?4,state='expired' WHERE account_id=?1 AND thread_id=?2 AND server_id=?3",
                           params![account, thread, id, PREVIEW])?;
            }
            let count = tx.execute("DELETE FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?4",
                                   params![account, thread, id, sender])?;
            if count != 0 {
                tx.execute("DELETE FROM pending_reaction_events WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                           params![account, thread, id])?;
                tx.execute("DELETE FROM delivery_receipts WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                           params![account, thread, id])?;
                tx.execute("UPDATE conversations AS c SET last_message_at_ms=COALESCE((
                   SELECT MAX(m.sent_at_ms) FROM messages m WHERE m.account_id=c.account_id AND m.thread_id=c.thread_id
                     AND NOT EXISTS(SELECT 1 FROM list_silent_messages q WHERE q.account_id=m.account_id
                       AND q.thread_id=m.thread_id AND q.message_id=m.message_id)),0)
                   WHERE c.account_id=?1 AND c.thread_id=?2", params![account, thread])?;
                let conversation = tx.query_row(
                    CONVERSATION_SQL,
                    params![account, thread, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )?;
                changes.push(ExpiredMessage {
                    thread_id: thread,
                    message_id: id,
                    sender_id: sender,
                    conversation,
                });
            }
        }
        tx.commit()?;
        Ok(changes)
    }
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;

    fn text(id: &str, body: &str, stamped: i64) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: id.into(),
            sender: "22".into(),
            text: body.into(),
            source_client_id: Some("9000".into()),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
            sent_at_ms: stamped,
            expires_at_ms: None,
        }
    }

    #[test]
    fn expiring_text_never_enters_search_or_sidebar_and_replay_cannot_restore_it() {
        let mut db = MessageStore::in_memory().unwrap();
        let now = now_ms();
        let ordinary = text("500", "Previous message", now - 100);
        let mut transient = text("501", "Secret address https://example.org", now);
        transient.expires_at_ms = Some(now + 60_000);
        let received = db
            .receive_batch(
                "11",
                &[(ordinary.clone(), None), (transient.clone(), None)],
                None,
            )
            .unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[1].0.expires_at_ms, transient.expires_at_ms);
        assert_eq!(received[1].1.last_text, PREVIEW);
        assert_eq!(
            db.page("11", "d:22", None, 10).unwrap()[1].text,
            transient.text
        );
        assert!(db
            .search_messages("11", "d:22", "Secret", 10)
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(
            db.conversation_overview("11", "d:22", 10)
                .unwrap()
                .link_count,
            0
        );
        assert_eq!(
            db.next_message_expiry("11").unwrap(),
            transient.expires_at_ms
        );

        let changes = db.expire_messages("11", now + 60_000).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].message_id, transient.id);
        assert_eq!(changes[0].conversation.last_text, ordinary.text);
        assert_eq!(db.page("11", "d:22", None, 10).unwrap(), [ordinary]);
        assert_eq!(db.next_message_expiry("11").unwrap(), None);

        transient.expires_at_ms = None;
        assert!(db
            .receive_batch("11", &[(transient, None)], Some(("510_1", "123")))
            .unwrap()
            .is_empty());
        assert_eq!(db.receive_cursor("11", "510_1").unwrap(), "123");
        assert_eq!(db.page("11", "d:22", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn quote_in_the_same_batch_keeps_only_a_neutral_projection() {
        let mut db = MessageStore::in_memory().unwrap();
        let now = now_ms();
        let mut source = text("500", "Never duplicate me", now - 100);
        let mut reply = text("501", "A non-expiring reply", now);
        reply.quote = Some(MessageQuote::from_message(&source).unwrap());
        source.expires_at_ms = Some(now + 60_000);
        db.receive_batch("11", &[(reply, None), (source, None)], None)
            .unwrap();
        let messages = db.page("11", "d:22", None, 10).unwrap();
        let quote = messages[1].quote.as_ref().unwrap();
        assert_eq!((quote.text.as_str(), quote.message_type), (PREVIEW, 0));
        let has_raw_copy: bool = db.conn.query_row(
            "SELECT quote_metadata IS NOT NULL FROM message_reply WHERE account_id='11' AND thread_id='d:22' AND message_id='501'",
            [], |row| row.get(0),
        ).unwrap();
        assert!(!has_raw_copy);
    }

    #[test]
    fn acknowledged_and_uncertain_text_scrub_outbox_at_expiration() {
        let mut db = MessageStore::in_memory().unwrap();
        let now = now_ms();
        let ttl = 60_000;
        let accepted = db
            .prepare_outgoing_disappearing("11", "d:22", "Acknowledged secret", now, ttl)
            .unwrap();
        let uncertain = db
            .prepare_outgoing_disappearing("11", "d:22", "Uncertain secret", now + 1, ttl)
            .unwrap();
        db.fail_outgoing("11", uncertain, true).unwrap();
        db.record_outgoing_ack("11", accepted, "600").unwrap();
        let (committed, sidebar) = db
            .accept_outgoing("11", accepted, "600", Some(now + 2))
            .unwrap();
        assert_eq!(committed.expires_at_ms, Some(now + 2 + ttl));
        assert_eq!(sidebar.last_text, PREVIEW);
        assert_eq!(db.next_message_expiry("11").unwrap(), Some(now + 1 + ttl));
        db.expire_messages("11", now + 2 + ttl).unwrap();
        let persisted: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM outgoing WHERE account_id='11' AND state='expired' AND body=?1",
            [PREVIEW], |row| row.get(0),
        ).unwrap();
        assert_eq!(persisted, 2);
        let mut late_echo = text("601", "Uncertain secret", now + 3);
        late_echo.sender = "11".into();
        late_echo.source_client_id = Some(uncertain.to_string());
        let received = db
            .receive_batch("11", &[(late_echo, Some(uncertain.to_string()))], None)
            .unwrap();
        assert!(received.is_empty());
        assert!(db.page("11", "d:22", None, 10).unwrap().is_empty());
    }
}
