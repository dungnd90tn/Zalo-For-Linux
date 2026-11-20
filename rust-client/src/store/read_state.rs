//! Message-ID read marks and payload-free receipts share the message transaction.
use super::*;
use crate::domain::read_state::*;
use anyhow::ensure;
use std::collections::{HashMap, HashSet};
const MAX_PENDING: i64 = 1000;
const PENDING_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
fn numeric(value: &str) -> Result<()> {
    ensure!(
        crate::domain::reply::valid_source_id(value),
        FailureReason::Validation
    );
    Ok(())
}
fn scope(account: &str, thread: &str) -> Result<()> {
    numeric(account)?;
    let (kind, id) = thread.split_once(':').ok_or(FailureReason::Validation)?;
    ensure!(matches!(kind, "d" | "g"), FailureReason::Validation);
    numeric(id)
}
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS receipt_references(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,reference TEXT NOT NULL CHECK(length(CAST(reference AS BLOB))<=4096),PRIMARY KEY(account_id,thread_id,message_id),FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS message_unread(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,PRIMARY KEY(account_id,thread_id,message_id),FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS message_local_reads(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,confirmed INTEGER NOT NULL DEFAULT 0 CHECK(confirmed IN (0,1)),PRIMARY KEY(account_id,thread_id,message_id),FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS read_receipt_outbox(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,PRIMARY KEY(account_id,thread_id,message_id),FOREIGN KEY(account_id,thread_id,message_id) REFERENCES receipt_references(account_id,thread_id,message_id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS server_read_watermarks(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,last_message_id TEXT NOT NULL,PRIMARY KEY(account_id,thread_id));
    CREATE TABLE IF NOT EXISTS delivery_receipts(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,client_id TEXT NOT NULL,reader_id TEXT NOT NULL,state INTEGER NOT NULL CHECK(state IN (1,2)),observed_at_ms INTEGER NOT NULL,proven INTEGER NOT NULL DEFAULT 0 CHECK(proven IN (0,1)),PRIMARY KEY(account_id,thread_id,message_id,client_id,reader_id));
    CREATE INDEX IF NOT EXISTS delivery_receipts_age ON delivery_receipts(account_id,observed_at_ms) WHERE proven=0;
    CREATE TABLE IF NOT EXISTS notification_preferences(account_id TEXT PRIMARY KEY,enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),preview INTEGER NOT NULL CHECK(preview IN (0,1)));")?;
    Ok(())
}
type Bindings<'a> = HashMap<(&'a str, &'a str), &'a ReceiptBinding>;

fn valid_reference(account: &str, message: &Message, binding: &ReceiptBinding) -> Result<()> {
    let reference = &binding.reference;
    scope(account, &binding.thread_id)?;
    numeric(&reference.message_id)?;
    if reference.sender_id != "0" {
        numeric(&reference.sender_id)?;
    }
    ensure!(
        binding.account_id == account
            && message.account_id == account
            && message.thread_id == binding.thread_id
            && message.id == binding.message_id
            && message.id == reference.message_id
            && message.sender == reference.sender_id
            && reference.sent_at_ms >= -1
            && (reference.sent_at_ms <= 0 || reference.sent_at_ms == message.sent_at_ms)
            && reference.st >= -1
            && reference.at >= -1
            && reference.command >= -1
            && !reference.message_type.is_empty()
            && reference.message_type.len() <= 128
            && !reference.message_type.chars().any(char::is_control),
        FailureReason::Validation
    );
    Ok(())
}

pub(super) fn validate_bindings<'a, 'b>(
    account: &str,
    messages: impl Iterator<Item = &'a Message>,
    bindings: &'b [ReceiptBinding],
) -> Result<Bindings<'b>> {
    ensure!(bindings.len() <= MAX_PAGE_SIZE, FailureReason::Validation);
    let by_key: HashMap<_, _> = messages
        .map(|m| ((m.thread_id.as_str(), m.id.as_str()), m))
        .collect();
    let mut result = HashMap::with_capacity(bindings.len());
    for binding in bindings {
        let key = (binding.thread_id.as_str(), binding.message_id.as_str());
        let message = by_key.get(&key).ok_or(FailureReason::Validation)?;
        valid_reference(account, message, binding)?;
        ensure!(
            result.insert(key, binding).is_none(),
            FailureReason::Validation
        );
    }
    Ok(result)
}

fn normal_thread(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=0)
          AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)",
        params![account, thread],
        |row| row.get(0),
    )?)
}

fn route_matches(message: &Message, reference: &ReceiptReference) -> bool {
    if let Some(group) = message.thread_id.strip_prefix("g:") {
        reference.recipient_id == group
    } else if let Some(peer) = message.thread_id.strip_prefix("d:") {
        message.sender == peer
            && (reference.recipient_id == message.account_id || reference.recipient_id == peer)
    } else {
        false
    }
}

fn merge_reference(old: ReceiptReference, new: &ReceiptReference) -> ReceiptReference {
    let mut merged = old.clone();
    if merged.client_id.is_none() {
        merged.client_id = new.client_id.clone();
    }
    for (value, new_value) in [
        (&mut merged.st, new.st),
        (&mut merged.at, new.at),
        (&mut merged.command, new.command),
        (&mut merged.sent_at_ms, new.sent_at_ms),
    ] {
        if *value < 0 {
            *value = new_value;
        }
    }
    // Preserve the first complete identity. Conflicting auxiliary metadata
    // must neither change the target nor stop message/cursor persistence.
    if old.message_type != new.message_type
        || old.recipient_id != new.recipient_id
        || old
            .client_id
            .as_ref()
            .zip(new.client_id.as_ref())
            .is_some_and(|(a, b)| a != b)
    {
        return old;
    }
    merged
}

fn queue_read(conn: &Connection, account: &str, thread: &str, id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO read_receipt_outbox
         SELECT r.account_id,r.thread_id,r.message_id
         FROM receipt_references r JOIN message_local_reads l USING(account_id,thread_id,message_id)
         WHERE r.account_id=?1 AND r.thread_id=?2 AND r.message_id=?3 AND l.confirmed=0
         AND json_type(r.reference,'$.client_id')='text'
         AND NOT EXISTS(SELECT 1 FROM group_left l WHERE l.account_id=?1 AND l.thread_id=?2)
         AND NOT EXISTS(SELECT 1 FROM list_silent_messages s WHERE s.account_id=?1 AND s.thread_id=?2 AND s.message_id=?3)
         AND NOT EXISTS(SELECT 1 FROM server_read_watermarks w WHERE w.account_id=?1 AND w.thread_id=?2
          AND (length(w.last_message_id)>length(?3) OR (length(w.last_message_id)=length(?3) AND w.last_message_id>=?3)))",
        params![account, thread, id],
    )?;
    Ok(())
}

pub(super) fn persist(
    conn: &Connection,
    message: &Message,
    binding: &ReceiptBinding,
    fresh: bool,
    ordinary: bool,
) -> Result<bool> {
    if !ordinary || message.sender == message.account_id || message.sender == "0" {
        return Ok(false);
    }
    let key = params![message.account_id, message.thread_id, message.id];
    if route_matches(message, &binding.reference) {
        let old: Option<String> = conn.query_row(
            "SELECT reference FROM receipt_references WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            key, |r| r.get(0),
        ).optional()?;
        let mut reference = binding.reference.clone();
        reference.client_id = reference
            .client_id
            .filter(|id| crate::domain::reply::valid_source_id(id));
        if let Some(old) = old {
            reference = merge_reference(serde_json::from_str(&old)?, &reference);
        }
        let json = serde_json::to_string(&reference)?;
        ensure!(json.len() <= 4096, FailureReason::Validation);
        conn.execute(
            "INSERT INTO receipt_references VALUES(?1,?2,?3,?4)
             ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET reference=excluded.reference
             WHERE reference<>excluded.reference",
            params![message.account_id, message.thread_id, message.id, json],
        )?;
        if reference.client_id.is_some() {
            queue_read(conn, &message.account_id, &message.thread_id, &message.id)?;
        }
    }
    if !fresh
        || !binding.counts_unread
        || !normal_thread(conn, &message.account_id, &message.thread_id)?
        || super::group_actions::left(conn, &message.account_id, &message.thread_id)?
    {
        return Ok(false);
    }
    Ok(conn.execute(
        "INSERT OR IGNORE INTO message_unread SELECT ?1,?2,?3
         WHERE NOT EXISTS(SELECT 1 FROM message_local_reads WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
         AND NOT EXISTS(SELECT 1 FROM list_silent_messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
         AND NOT EXISTS(SELECT 1 FROM server_read_watermarks WHERE account_id=?1 AND thread_id=?2
          AND (length(last_message_id)>length(?3) OR (length(last_message_id)=length(?3) AND last_message_id>=?3)))", key,
    )? > 0)
}

impl MessageStore {
    pub fn thread_read_state(&self, account: &str, thread: &str) -> Result<ThreadReadState> {
        scope(account, thread)?;
        let unread = if normal_thread(&self.conn, account, thread)?
            && !self.group_has_left(account, thread)?
        {
            self.conn.query_row(
                "SELECT count(*) FROM message_unread WHERE account_id=?1 AND thread_id=?2",
                params![account, thread],
                |r| r.get(0),
            )?
        } else {
            0
        };
        Ok(ThreadReadState {
            thread_id: thread.into(),
            unread,
        })
    }
    pub fn read_state_snapshot(
        &self,
        account: &str,
        threads: &[String],
    ) -> Result<Vec<ThreadReadState>> {
        ensure!(threads.len() <= 200, FailureReason::Validation);
        threads
            .iter()
            .map(|t| self.thread_read_state(account, t))
            .collect()
    }
    pub fn mark_visible_read(
        &mut self,
        account: &str,
        thread: &str,
        ids: &[String],
    ) -> Result<Vec<ReceiptReference>> {
        scope(account, thread)?;
        ensure!(
            ids.len() <= MAX_REFRESH_MESSAGES && normal_thread(&self.conn, account, thread)?,
            FailureReason::Validation
        );
        let tx = self.conn.transaction()?;
        let mut seen = HashSet::new();
        for id in ids {
            numeric(id)?;
            if !seen.insert(id) {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO message_local_reads(account_id,thread_id,message_id)
                 SELECT account_id,thread_id,message_id FROM messages m
                 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender<>?1 AND sender<>'0'
                 AND (EXISTS(SELECT 1 FROM message_unread u WHERE u.account_id=?1 AND u.thread_id=?2 AND u.message_id=?3)
                   OR EXISTS(SELECT 1 FROM receipt_references r WHERE r.account_id=?1 AND r.thread_id=?2 AND r.message_id=?3))
                 AND NOT EXISTS(SELECT 1 FROM list_silent_messages s WHERE s.account_id=?1 AND s.thread_id=?2 AND s.message_id=?3)",
                params![account, thread, id],
            )?;
            tx.execute(
                "DELETE FROM message_unread WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
            )?;
            queue_read(&tx, account, thread, id)?;
        }
        tx.commit()?;
        self.pending_read_receipts(account, thread, MAX_REFRESH_MESSAGES)
    }
    pub fn pending_read_threads(&self, account: &str, limit: usize) -> Result<Vec<String>> {
        numeric(account)?;
        ensure!((1..=200).contains(&limit), FailureReason::Validation);
        let mut query = self.conn.prepare_cached(
            "SELECT DISTINCT o.thread_id FROM read_receipt_outbox o WHERE o.account_id=?1
              AND NOT EXISTS(SELECT 1 FROM temp.live_privacy p WHERE p.account_id=?1 AND p.ready=0)
              AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h WHERE h.account_id=?1 AND h.thread_id=o.thread_id)
              AND NOT EXISTS(SELECT 1 FROM group_left l WHERE l.account_id=?1 AND l.thread_id=o.thread_id)
             ORDER BY o.thread_id LIMIT ?2",
        )?;
        let rows = query.query_map(params![account, limit as i64], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn pending_read_receipts(
        &self,
        account: &str,
        thread: &str,
        limit: usize,
    ) -> Result<Vec<ReceiptReference>> {
        scope(account, thread)?;
        ensure!(
            limit <= MAX_REFRESH_MESSAGES && limit > 0,
            FailureReason::Validation
        );
        if !normal_thread(&self.conn, account, thread)? {
            return Ok(vec![]);
        }
        let mut query=self.conn.prepare("SELECT r.reference FROM read_receipt_outbox o JOIN receipt_references r USING(account_id,thread_id,message_id) WHERE o.account_id=?1 AND o.thread_id=?2 ORDER BY length(o.message_id),o.message_id LIMIT ?3")?;
        let rows = query.query_map(params![account, thread, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn confirm_read_receipts(
        &mut self,
        account: &str,
        thread: &str,
        ids: &[String],
    ) -> Result<()> {
        scope(account, thread)?;
        ensure!(ids.len() <= MAX_REFRESH_MESSAGES, FailureReason::Validation);
        let tx = self.conn.transaction()?;
        for id in ids {
            numeric(id)?;
            tx.execute("UPDATE message_local_reads SET confirmed=1 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND EXISTS(SELECT 1 FROM read_receipt_outbox WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",params![account,thread,id])?;
            tx.execute("DELETE FROM read_receipt_outbox WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![account,thread,id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn notification_preferences(&self, account: &str) -> Result<NotificationPreferences> {
        numeric(account)?;
        Ok(self
            .conn
            .query_row(
                "SELECT enabled,preview FROM notification_preferences WHERE account_id=?1",
                [account],
                |r| {
                    Ok(NotificationPreferences {
                        enabled: r.get(0)?,
                        preview: r.get(1)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }
    pub fn set_notification_preferences(
        &self,
        account: &str,
        prefs: NotificationPreferences,
    ) -> Result<()> {
        numeric(account)?;
        self.conn.execute("INSERT INTO notification_preferences VALUES(?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET enabled=excluded.enabled,preview=excluded.preview",params![account,prefs.enabled,prefs.preview])?;
        Ok(())
    }
    pub fn apply_server_read(
        &mut self,
        account: &str,
        watermarks: &[ServerReadWatermark],
    ) -> Result<()> {
        numeric(account)?;
        ensure!(watermarks.len() <= 200, FailureReason::Validation);
        let tx = self.conn.transaction()?;
        for mark in watermarks {
            scope(account, &mark.thread_id)?;
            numeric(&mark.last_message_id)?;
            tx.execute("INSERT INTO server_read_watermarks VALUES(?1,?2,?3) ON CONFLICT(account_id,thread_id) DO UPDATE SET last_message_id=excluded.last_message_id WHERE length(excluded.last_message_id)>length(last_message_id) OR (length(excluded.last_message_id)=length(last_message_id) AND excluded.last_message_id>last_message_id)",params![account,mark.thread_id,mark.last_message_id])?;
            tx.execute("DELETE FROM message_unread WHERE account_id=?1 AND thread_id=?2 AND (length(message_id)<length(?3) OR (length(message_id)=length(?3) AND message_id<=?3))",params![account,mark.thread_id,mark.last_message_id])?;
            tx.execute("DELETE FROM read_receipt_outbox WHERE account_id=?1 AND thread_id=?2 AND (length(message_id)<length(?3) OR (length(message_id)=length(?3) AND message_id<=?3))",params![account,mark.thread_id,mark.last_message_id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn apply_receipt_updates(
        &mut self,
        account: &str,
        updates: &[ReceiptUpdate],
        now_ms: i64,
    ) -> Result<()> {
        numeric(account)?;
        ensure!(
            updates.len() <= 200 && now_ms > 0,
            FailureReason::Validation
        );
        let tx = self.conn.transaction()?;
        for update in updates {
            scope(account, &update.thread_id)?;
            numeric(&update.message_id)?;
            ensure!(
                update.account_id == account && update.state != DeliveryState::Accepted,
                FailureReason::Validation
            );
            if let Some(client) = &update.client_id {
                numeric(client)?;
            }
            let reader = update.reader_id.as_deref().unwrap_or("");
            if !reader.is_empty() {
                numeric(reader)?;
                ensure!(reader != account, FailureReason::Validation);
                if let Some(peer) = update.thread_id.strip_prefix("d:") {
                    ensure!(reader == peer, FailureReason::Validation);
                }
            }
            let state = if update.state == DeliveryState::Seen && !reader.is_empty() {
                2
            } else {
                1
            };
            tx.execute(
                "INSERT INTO delivery_receipts(account_id,thread_id,message_id,client_id,reader_id,state,observed_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(account_id,thread_id,message_id,client_id,reader_id) DO UPDATE SET
                 state=MAX(state,excluded.state),observed_at_ms=MAX(observed_at_ms,excluded.observed_at_ms)",
                params![account,update.thread_id,update.message_id,update.client_id.as_deref().unwrap_or(""),reader,state,now_ms],
            )?;
        }
        // Keep durable status for verified own messages. Bound unresolved/foreign
        // entries so receipts that race the outgoing echo cannot grow indefinitely.
        tx.execute(
            "UPDATE delivery_receipts AS d SET proven=1 WHERE d.account_id=?1 AND d.proven=0
             AND EXISTS(
                SELECT 1 FROM messages m LEFT JOIN message_reply r USING(account_id,thread_id,message_id)
                WHERE m.account_id=d.account_id AND m.thread_id=d.thread_id AND m.message_id=d.message_id
                AND NOT EXISTS(SELECT 1 FROM recall_tombstones t WHERE t.account_id=m.account_id AND t.thread_id=m.thread_id AND t.message_id=m.message_id AND t.actor_id=m.sender AND t.materialized=1)
                AND m.sender=d.account_id AND (d.client_id='' OR d.client_id=r.source_client_id)
             )",
            [account],
        )?;
        tx.execute(
            "DELETE FROM delivery_receipts WHERE account_id=?1 AND proven=0 AND observed_at_ms<?2",
            params![account, now_ms.saturating_sub(PENDING_TTL_MS)],
        )?;
        tx.execute(
            "DELETE FROM delivery_receipts WHERE rowid IN (
                SELECT rowid FROM delivery_receipts WHERE account_id=?1 AND proven=0
                ORDER BY observed_at_ms DESC,rowid DESC LIMIT -1 OFFSET ?2
            )",
            params![account, MAX_PENDING],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn message_delivery(
        &self,
        account: &str,
        thread: &str,
        ids: &[String],
    ) -> Result<Vec<MessageDelivery>> {
        scope(account, thread)?;
        ensure!(ids.len() <= MAX_REFRESH_MESSAGES, FailureReason::Validation);
        if !normal_thread(&self.conn, account, thread)? {
            return Ok(vec![]);
        }
        let mut statuses = self.conn.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?1)
                    AND NOT EXISTS(SELECT 1 FROM recall_tombstones t WHERE t.account_id=?1 AND t.thread_id=?2 AND t.message_id=?3 AND t.actor_id=?1 AND t.materialized=1),
                    (SELECT source_client_id FROM message_reply WHERE account_id=?1 AND thread_id=?2 AND message_id=?3),
                    EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND state='accepted')
                    OR EXISTS(SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND state='accepted')
                    OR EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND state='accepted')
                    OR EXISTS(SELECT 1 FROM forward_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND state='accepted')",
        )?;
        let mut receipts = self.conn.prepare_cached(
            "SELECT COALESCE(MAX(state),0), COUNT(DISTINCT CASE WHEN state=2 THEN reader_id END)
             FROM delivery_receipts WHERE account_id=?1 AND thread_id=?2 AND message_id=?3
             AND (client_id='' OR client_id=?4)",
        )?;
        let mut result = Vec::new();
        for id in ids {
            numeric(id)?;
            let (own, client, accepted): (bool, Option<String>, bool) = statuses
                .query_row(params![account, thread, id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
            if !own {
                continue;
            }
            let (status, seen_count): (u8, u64) = receipts
                .query_row(params![account, thread, id, client], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
            let state = match status {
                2 => DeliveryState::Seen,
                1 => DeliveryState::Delivered,
                _ if accepted => DeliveryState::Accepted,
                _ => continue,
            };
            result.push(MessageDelivery {
                message_id: id.clone(),
                state,
                seen_count,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        account: &str,
        thread: &str,
        sender: &str,
        id: &str,
    ) -> (ReceivedReply, ReceiptBinding) {
        let message = Message {
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: sender.into(),
            text: "Tin nhắn tổng hợp".into(),
            sent_at_ms: 20,
            source_client_id: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
        };
        let binding = ReceiptBinding {
            account_id: account.into(),
            thread_id: thread.into(),
            message_id: id.into(),
            reference: ReceiptReference {
                message_id: id.into(),
                client_id: Some("99".into()),
                sender_id: sender.into(),
                recipient_id: thread.strip_prefix("g:").unwrap_or(account).into(),
                message_type: "webchat".into(),
                st: -1,
                at: -1,
                command: -1,
                sent_at_ms: 20,
            },
            counts_unread: true,
        };
        ((message, None, None, None, None), binding)
    }

    #[test]
    fn schema_twelve_history_upgrades_without_synthetic_unread() {
        let mut store = MessageStore::in_memory().unwrap();
        let (row, _) = input("11", "d:22", "22", "5");
        store
            .receive_batch_with_replies("11", std::slice::from_ref(&row), Some(("510_0", "99")))
            .unwrap();
        store
            .conn
            .execute_batch(
                "DROP TABLE read_receipt_outbox; DROP TABLE receipt_references;
             DROP TABLE message_unread; DROP TABLE message_local_reads;
             DROP TABLE server_read_watermarks; DROP TABLE delivery_receipts;
             DROP TABLE notification_preferences; PRAGMA user_version=12;",
            )
            .unwrap();
        let store = MessageStore::initialize(store.conn).unwrap();
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
        assert_eq!(store.receive_cursor("11", "510_0").unwrap(), "99");
        let column: String = store
            .conn
            .query_row("SELECT body FROM messages WHERE message_id='5'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(column, row.0.text);
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 0);
        assert_eq!(
            store.notification_preferences("11").unwrap(),
            NotificationPreferences::default()
        );
    }

    #[test]
    fn unread_commits_with_cursor_and_only_exact_fresh_rows() {
        let mut store = MessageStore::in_memory().unwrap();
        let (history, history_ref) = input("11", "d:22", "22", "5");
        let (fresh, fresh_ref) = input("11", "d:22", "22", "10");
        let (silent, mut silent_ref) = input("11", "d:22", "22", "11");
        silent_ref.counts_unread = false;
        let (hidden, hidden_ref) = input("11", "d:33", "33", "12");
        store
            .replace_live_privacy("11", &["d:33".into()], Some("d:33"))
            .unwrap();
        assert!(store
            .receive_batch_with_receipts(
                "11",
                std::slice::from_ref(&history),
                None,
                std::slice::from_ref(&history_ref),
                ReceiveContext::History,
            )
            .unwrap()
            .1
            .is_empty());
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_cursor BEFORE INSERT ON receive_cursors
             BEGIN SELECT RAISE(ABORT,'synthetic rollback'); END;",
            )
            .unwrap();
        assert!(store
            .receive_batch_with_receipts(
                "11",
                std::slice::from_ref(&fresh),
                Some(("510_0", "100")),
                std::slice::from_ref(&fresh_ref),
                ReceiveContext::LiveFresh,
            )
            .is_err());
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 0);
        store
            .conn
            .execute_batch("DROP TRIGGER reject_cursor;")
            .unwrap();
        let rows = [history, fresh, silent, hidden];
        let refs = [history_ref, fresh_ref, silent_ref, hidden_ref];
        assert_eq!(
            store
                .receive_batch_with_receipts(
                    "11",
                    &rows,
                    Some(("510_0", "100")),
                    &refs,
                    ReceiveContext::LiveFresh,
                )
                .unwrap()
                .1,
            [("d:22".into(), "10".into())]
        );
        assert!(store
            .receive_batch_with_receipts("11", &rows, None, &refs, ReceiveContext::LiveFresh,)
            .unwrap()
            .1
            .is_empty());
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 1);
        assert_eq!(store.thread_read_state("11", "d:33").unwrap().unread, 0);
        assert_eq!(store.thread_read_state("33", "d:22").unwrap().unread, 0);
    }

    #[test]
    fn explicit_local_read_enriches_missing_client_id_and_skips_remote_watermark() {
        let mut store = MessageStore::in_memory().unwrap();
        let (row, mut reference) = input("11", "d:22", "22", "99");
        reference.reference.client_id = None;
        let (unrelated, mut unrelated_ref) = input("11", "d:22", "22", "100");
        unrelated_ref.reference.recipient_id = "77".into();
        store
            .receive_batch_with_receipts(
                "11",
                &[row.clone(), unrelated],
                None,
                &[reference.clone(), unrelated_ref],
                ReceiveContext::LiveFresh,
            )
            .unwrap();
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 2);
        assert!(store
            .mark_visible_read("11", "d:22", &["99".into()])
            .unwrap()
            .is_empty());
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 1);
        reference.reference.client_id = Some("123".into());
        reference.reference.st = 0;
        store
            .receive_batch_with_receipts(
                "11",
                &[row],
                None,
                &[reference],
                ReceiveContext::OfflineSync,
            )
            .unwrap();
        assert_eq!(store.pending_read_threads("11", 10).unwrap(), ["d:22"]);
        let refs = store.pending_read_receipts("11", "d:22", 10).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].st, 0);
        assert_eq!(refs[0].at, -1);
        assert_eq!(refs[0].client_id.as_deref(), Some("123"));
        store
            .apply_server_read(
                "11",
                &[ServerReadWatermark {
                    thread_id: "d:22".into(),
                    last_message_id: "100".into(),
                }],
            )
            .unwrap();
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 0);
        assert!(store.pending_read_threads("11", 10).unwrap().is_empty());
        assert!(store
            .mark_visible_read("11", "d:22", &["99".into()])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn eager_delivery_requires_own_binding_and_keeps_confirmed_seen() {
        let mut store = MessageStore::in_memory().unwrap();
        let (mut row, _) = input("11", "g:22", "11", "50");
        row.0.source_client_id = Some("99".into());
        let (peer, _) = input("11", "g:22", "33", "51");
        let delivered = ReceiptUpdate {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            message_id: "50".into(),
            client_id: Some("99".into()),
            reader_id: Some("33".into()),
            state: DeliveryState::Delivered,
        };
        let seen = ReceiptUpdate {
            state: DeliveryState::Seen,
            ..delivered.clone()
        };
        let wrong = ReceiptUpdate {
            client_id: Some("101".into()),
            reader_id: Some("44".into()),
            ..seen.clone()
        };
        let peer_status = ReceiptUpdate {
            message_id: "51".into(),
            client_id: None,
            ..seen.clone()
        };
        store
            .apply_receipt_updates("11", &[seen, wrong, peer_status], 10)
            .unwrap();
        assert!(store
            .message_delivery("11", "g:22", &["50".into()])
            .unwrap()
            .is_empty());
        store.insert_batch(&[row.0, peer.0]).unwrap();
        store
            .apply_receipt_updates("11", &[delivered], PENDING_TTL_MS + 20)
            .unwrap();
        assert_eq!(
            store
                .message_delivery("11", "g:22", &["50".into(), "51".into()])
                .unwrap(),
            [MessageDelivery {
                message_id: "50".into(),
                state: DeliveryState::Seen,
                seen_count: 1
            },]
        );
        assert!(store
            .message_delivery("33", "g:22", &["50".into()])
            .unwrap()
            .is_empty());
        assert_eq!(
            store.notification_preferences("11").unwrap(),
            NotificationPreferences {
                enabled: true,
                preview: false
            }
        );
        store
            .set_notification_preferences(
                "11",
                NotificationPreferences {
                    enabled: false,
                    preview: true,
                },
            )
            .unwrap();
        assert_eq!(
            store.notification_preferences("11").unwrap(),
            NotificationPreferences {
                enabled: false,
                preview: true
            }
        );
        assert_eq!(
            store.notification_preferences("33").unwrap(),
            NotificationPreferences::default()
        );
    }
}
