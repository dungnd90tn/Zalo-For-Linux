//! Scoped recall tombstones dominate late history and outgoing echo replay.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS recall_tombstones(
           account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
           actor_id TEXT NOT NULL,source_client_id TEXT,occurred_at_ms INTEGER NOT NULL,
           observed_at_ms INTEGER NOT NULL,
           materialized INTEGER NOT NULL CHECK(materialized IN (0,1)),
           PRIMARY KEY(account_id,thread_id,message_id)
         );
         CREATE INDEX IF NOT EXISTS recall_pending
         ON recall_tombstones(account_id,observed_at_ms) WHERE materialized=0;
         CREATE INDEX IF NOT EXISTS message_reply_recall_target ON message_reply(
             account_id,thread_id,json_extract(quote_projection,'$.message_id'),
             json_extract(quote_projection,'$.owner_id')
         ) WHERE json_valid(quote_projection);
         CREATE INDEX IF NOT EXISTS outgoing_reply_recall_target ON outgoing_reply(
             account_id,json_extract(quote_projection,'$.message_id'),
             json_extract(quote_projection,'$.owner_id')
         ) WHERE json_valid(quote_projection);
         CREATE TABLE IF NOT EXISTS recall_outbox(
           account_id TEXT NOT NULL,client_id TEXT NOT NULL,thread_id TEXT NOT NULL,
           message_id TEXT NOT NULL,source_client_id TEXT NOT NULL,
           source_sent_at_ms INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('sending','unknown','rejected','acknowledged','recalled')),
           PRIMARY KEY(account_id,client_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS recall_unresolved_target
         ON recall_outbox(account_id,thread_id,message_id)
         WHERE state IN ('sending','unknown','acknowledged');",
    )?;
    Ok(())
}

pub(super) fn materialized(conn: &Connection, message: &Message) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM recall_tombstones WHERE account_id=?1
           AND thread_id=?2 AND message_id=?3 AND actor_id=?4 AND materialized=1)",
        params![
            message.account_id,
            message.thread_id,
            message.id,
            message.sender
        ],
        |row| row.get(0),
    )?)
}

pub(super) fn redact(message: &mut Message) {
    crate::domain::RECALLED_TEXT.clone_into(&mut message.text);
    message.source_client_id = None;
    message.mentions.clear();
    message.reply_blocked = true;
    message.recalled = true;
    message.quote = None;
    message.sticker = None;
    message.media = None;
}

pub(crate) struct RecallChange {
    #[cfg(feature = "messaging")]
    pub thread_id: String,
    #[cfg(feature = "messaging")]
    pub message_id: String,
    #[cfg(feature = "messaging")]
    pub actor_id: String,
    #[cfg(feature = "messaging")]
    pub message: Option<Message>,
    #[cfg(feature = "messaging")]
    pub conversation: Option<Conversation>,
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::reply::valid_source_id;
    use crate::protocol::recall::{RecallNotice, RecallReference};
    use anyhow::ensure;

    const MAX_CLIENT_ID: i64 = 9_007_199_254_740_991;
    const MAX_UNRESOLVED: i64 = 100;
    const MAX_PENDING_TOMBSTONES: i64 = 4096;
    const PENDING_TOMBSTONE_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
    use crate::domain::RECALLED_TEXT as PLACEHOLDER;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum RecallOutcome {
        Pending,
        Unknown,
        Rejected,
        Acknowledged,
        Recalled,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum RecallFailure {
        Unavailable,
        Expired,
        AlreadyRecalled,
        Unresolved,
    }

    impl std::fmt::Display for RecallFailure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Unavailable => "Recall target is not available",
                Self::Expired => "Recall window has expired",
                Self::AlreadyRecalled => "Message is already recalled",
                Self::Unresolved => "A recall is still unresolved",
            })
        }
    }

    impl std::error::Error for RecallFailure {}

    pub(crate) struct PreparedRecall {
        pub client_id: u64,
        pub reference: RecallReference,
    }

    fn scope<'a>(account: &str, thread: &'a str) -> Option<&'a str> {
        thread
            .strip_prefix("d:")
            .filter(|peer| valid_source_id(account) && valid_source_id(peer) && account != *peer)
    }

    fn private_or_encrypted(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             OR EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             OR EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
            params![account, thread],
            |row| row.get(0),
        )?)
    }

    fn original(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<Message>> {
        let mut message = conn.query_row(
            "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![account,thread,id],
            |row| Ok(Message {
                account_id:account.into(),thread_id:thread.into(),id:id.into(),sender:row.get(0)?,
                text:row.get(1)?,sent_at_ms:row.get(2)?,source_client_id:None,mentions: Vec::new(), reply_blocked:false,recalled:false,
                quote:None,sticker:None,media:None,
            }),
        ).optional()?;
        if let Some(message) = &mut message {
            message_reply::hydrate(conn, message)?;
        }
        Ok(message)
    }

    fn ordinary(conn: &Connection, message: &Message) -> Result<bool> {
        if message.reply_reference().is_none() || message.quote.is_some() {
            return Ok(false);
        }
        let allowed = conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')
             AND NOT EXISTS(SELECT 1 FROM list_silent_messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",
            params![message.account_id,message.thread_id,message.id],
            |row| row.get(0),
        )?;
        Ok(allowed)
    }

    struct Tombstone {
        actor: String,
        source_client: Option<String>,
        materialized: bool,
    }

    fn tombstone(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<Tombstone>> {
        Ok(conn
            .query_row(
                "SELECT actor_id,source_client_id,materialized FROM recall_tombstones
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
                |row| {
                    Ok(Tombstone {
                        actor: row.get(0)?,
                        source_client: row.get(1)?,
                        materialized: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    fn same_cli(prior: Option<&str>, incoming: Option<&str>) -> bool {
        prior.zip(incoming).is_none_or(|(a, b)| a == b)
    }

    /// Called before projecting any replay of a potentially recalled row.
    /// The caller supplies ordinary source eligibility from the wire mapper.
    pub(in super::super) fn redacted_copy(
        conn: &Connection,
        message: &Message,
        ordinary_text: bool,
    ) -> Result<Option<Message>> {
        if scope(&message.account_id, &message.thread_id).is_none() {
            return Ok(None);
        }
        let Some(prior) = tombstone(conn, &message.account_id, &message.thread_id, &message.id)?
        else {
            return Ok(None);
        };
        if message.sender != prior.actor
            || (!prior.materialized
                && (!ordinary_text
                    || !same_cli(
                        prior.source_client.as_deref(),
                        message.source_client_id.as_deref(),
                    )))
        {
            return Ok(None);
        }
        conn.execute(
            "UPDATE recall_tombstones SET materialized=1,
                source_client_id=COALESCE(source_client_id,?4)
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND materialized=0",
            params![
                message.account_id,
                message.thread_id,
                message.id,
                message.source_client_id
            ],
        )?;
        let mut sanitized = message.clone();
        sanitized.sent_at_ms = conn.query_row(
            "SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?4",
            params![message.account_id,message.thread_id,message.id,message.sender],
            |row|row.get(0),
        ).optional()?.unwrap_or(message.sent_at_ms);
        redact(&mut sanitized);
        Ok(Some(sanitized))
    }

    pub(in super::super) fn redact_replay(
        conn: &Connection,
        message: &mut Message,
        ordinary_text: bool,
    ) -> Result<bool> {
        if let Some(redacted) = redacted_copy(conn, message, ordinary_text)? {
            *message = redacted;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// A recalled own echo can prove the old text intent landed, but only
    /// when its original, authenticated ordinary metadata matches the ledger.
    pub(in super::super) fn settle_recalled_own_echo(
        conn: &Connection,
        message: &Message,
    ) -> Result<()> {
        if message.sender != message.account_id {
            return Ok(());
        }
        let Some(client) = &message.source_client_id else {
            return Ok(());
        };
        conn.execute(
            "UPDATE outgoing SET body=?5,server_id=?3,state='accepted'
             WHERE account_id=?1 AND thread_id=?2 AND client_id=?4
               AND (server_id IS NULL OR server_id=?3) AND body=?6
               AND state IN ('sending','unknown','acknowledged','accepted')
               AND NOT EXISTS(SELECT 1 FROM outgoing_reply r WHERE r.account_id=?1 AND r.client_id=?4)
               AND NOT EXISTS(SELECT 1 FROM outgoing other WHERE other.account_id=?1 AND other.thread_id=?2 AND other.server_id=?3 AND other.client_id<>?4)",
            params![message.account_id,message.thread_id,message.id,client,PLACEHOLDER,message.text],
        )?;
        Ok(())
    }

    pub(in super::super) fn redact_quote(
        conn: &Connection,
        account: &str,
        thread: &str,
        quote: &mut MessageQuote,
    ) -> Result<bool> {
        let Some(prior) = tombstone(conn, account, thread, &quote.message_id)? else {
            return Ok(false);
        };
        if quote.owner_id != prior.actor
            || (!prior.materialized
                && !same_cli(prior.source_client.as_deref(), quote.client_id.as_deref()))
        {
            return Ok(false);
        }
        PLACEHOLDER.clone_into(&mut quote.text);
        quote.message_type = 0;
        Ok(true)
    }

    fn redact_copies(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        actor: &str,
    ) -> Result<()> {
        let args = params![account, thread, id, actor, PLACEHOLDER];
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

    fn bound_pending(conn: &Connection, account: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM recall_tombstones WHERE account_id=?1 AND materialized=0
             AND observed_at_ms < unixepoch('now')*1000-?2",
            params![account, PENDING_TOMBSTONE_AGE_MS],
        )?;
        conn.execute(
            "DELETE FROM recall_tombstones WHERE rowid IN (
                SELECT rowid FROM recall_tombstones WHERE account_id=?1 AND materialized=0
                ORDER BY observed_at_ms DESC,rowid DESC LIMIT -1 OFFSET ?2
            )",
            params![account, MAX_PENDING_TOMBSTONES],
        )?;
        Ok(())
    }

    fn write(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        actor: &str,
        client: Option<&str>,
        time: i64,
    ) -> Result<Option<RecallChange>> {
        let Some(peer) = scope(account, thread) else {
            return Ok(None);
        };
        if !valid_source_id(id)
            || time <= 0
            || (actor != account && actor != peer)
            || client.is_some_and(|id| !valid_source_id(id))
        {
            return Ok(None);
        }
        let mut previous = tombstone(conn, account, thread, id)?;
        let mut original = original(conn, account, thread, id)?;
        if let Some(message) = &original {
            if message.sender != actor || !same_cli(message.source_client_id.as_deref(), client) {
                return Ok(None);
            }
        }
        if previous.as_ref().is_some_and(|prior| {
            prior.actor != actor || !same_cli(prior.source_client.as_deref(), client)
        }) {
            // A notice can precede history. If that pending notice disagreed
            // with the eventual row, let a new notice bound to the actual row
            // replace it. Never change an already materialized tombstone.
            let bound_original = match &original {
                Some(message) => ordinary(conn, message)?,
                None => false,
            };
            if !bound_original {
                return Ok(None);
            }
            conn.execute(
                "DELETE FROM recall_tombstones WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
            )?;
            previous = None;
        }
        if let Some(message) = &original {
            if previous.is_none() && !ordinary(conn, message)? {
                return Ok(None);
            }
        }
        let client = client.or_else(|| {
            original
                .as_ref()
                .and_then(|m| m.source_client_id.as_deref())
        });
        conn.execute(
            "INSERT INTO recall_tombstones VALUES(?1,?2,?3,?4,?5,?6,unixepoch('now')*1000,?7)
             ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET
              source_client_id=COALESCE(source_client_id,excluded.source_client_id),
              materialized=MAX(materialized,excluded.materialized)",
            params![account, thread, id, actor, client, time, original.is_some()],
        )?;
        if original.is_none() {
            bound_pending(conn, account)?;
        }
        if actor == account {
            conn.execute(
                "UPDATE recall_outbox SET state='recalled'
                 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3
                 AND state IN ('sending','unknown','acknowledged') AND (?4 IS NULL OR source_client_id=?4)",
                params![account,thread,id,client],
            )?;
        }
        redact_copies(conn, account, thread, id, actor)?;
        if let Some(message) = &mut original {
            conn.execute(
                "UPDATE messages SET body=?5 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?4",
                params![account,thread,id,actor,PLACEHOLDER],
            )?;
            if actor == account {
                conn.execute(
                    "UPDATE outgoing SET body=?4,state='accepted' WHERE account_id=?1 AND thread_id=?2 AND server_id=?3
                     AND state IN ('acknowledged','accepted')",
                    params![account,thread,id,PLACEHOLDER],
                )?;
            }
            for table in [
                "receipt_references",
                "message_unread",
                "message_local_reads",
                "delivery_receipts",
                "message_forward",
                "message_reply",
            ] {
                conn.execute(
                    &format!("DELETE FROM {table} WHERE account_id=?1 AND thread_id=?2 AND message_id=?3"),
                    params![account,thread,id],
                )?;
            }
            redact(message);
            message_reply::persist(conn, message, None)?;
        }
        let conversation = original
            .as_ref()
            .map(|_| {
                conn.query_row(
                    CONVERSATION_SQL,
                    params![account, thread, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )
            })
            .transpose()?;
        Ok(Some(RecallChange {
            thread_id: thread.into(),
            message_id: id.into(),
            actor_id: actor.into(),
            message: original,
            conversation,
        }))
    }

    pub(in super::super) fn apply_notice(
        conn: &Connection,
        account: &str,
        notice: &RecallNotice,
    ) -> Result<Option<RecallChange>> {
        if !valid_source_id(&notice.event_id) {
            return Ok(None);
        }
        write(
            conn,
            account,
            &notice.thread_id,
            &notice.message_id,
            &notice.actor_id,
            notice.client_id.as_deref(),
            notice.occurred_at_ms,
        )
    }

    fn allocate(conn: &Connection, account: &str, now: i64) -> Result<i64> {
        let last: Option<i64> = conn
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        let next = last
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(FailureReason::Validation)?
            .max(now);
        ensure!(
            (1..=MAX_CLIENT_ID).contains(&next),
            FailureReason::Validation
        );
        conn.execute(
            "INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
            params![account, next],
        )?;
        Ok(next)
    }

    impl MessageStore {
        pub(crate) fn prepare_recall(
            &mut self,
            account: &str,
            thread: &str,
            id: &str,
            now: i64,
            max_age: i64,
        ) -> Result<PreparedRecall> {
            ensure!(
                scope(account, thread).is_some()
                    && valid_source_id(id)
                    && (1..=MAX_CLIENT_ID).contains(&now)
                    && max_age > 0,
                RecallFailure::Unavailable
            );
            let tx = self.conn.transaction()?;
            ensure!(
                !private_or_encrypted(&tx, account, thread)?,
                RecallFailure::Unavailable
            );
            ensure!(
                tombstone(&tx, account, thread, id)?.is_none(),
                RecallFailure::AlreadyRecalled
            );
            let original = original(&tx, account, thread, id)?.ok_or(RecallFailure::Unavailable)?;
            ensure!(
                original.sender == account && ordinary(&tx, &original)?,
                RecallFailure::Unavailable
            );
            let reference = RecallReference::from_source(account, &original)
                .map_err(|_| RecallFailure::Unavailable)?;
            let age = now
                .checked_sub(reference.sent_at_ms)
                .ok_or(RecallFailure::Unavailable)?;
            ensure!((0..max_age).contains(&age), RecallFailure::Expired);
            let (count, duplicate): (i64, bool) = tx.query_row(
                "SELECT count(*),COALESCE(MAX(thread_id=?2 AND message_id=?3),0) FROM recall_outbox
                 WHERE account_id=?1 AND state IN ('sending','unknown','acknowledged')",
                params![account, thread, id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            ensure!(
                count < MAX_UNRESOLVED && !duplicate,
                RecallFailure::Unresolved
            );
            let client = allocate(&tx, account, now)?;
            tx.execute(
                "INSERT INTO recall_outbox VALUES(?1,?2,?3,?4,?5,?6,?7,'sending')",
                params![
                    account,
                    client.to_string(),
                    thread,
                    id,
                    reference.client_id,
                    reference.sent_at_ms,
                    now
                ],
            )?;
            tx.commit()?;
            Ok(PreparedRecall {
                client_id: client as u64,
                reference,
            })
        }

        pub(crate) fn record_recall_ack(&self, account: &str, client: u64) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            ensure!(self.conn.execute(
                "UPDATE recall_outbox SET state=CASE WHEN state='recalled' THEN state ELSE 'acknowledged' END
                 WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','recalled')",
                params![account,client.to_string()],
            )?==1, FailureReason::ConflictingEcho);
            Ok(())
        }

        pub(crate) fn accept_recall_ack(
            &mut self,
            account: &str,
            client: u64,
        ) -> Result<RecallChange> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let tx = self.conn.transaction()?;
            let (thread, id, source, created): (String, String, String, i64) = tx.query_row(
                "SELECT thread_id,message_id,source_client_id,created_at_ms FROM recall_outbox
                 WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','recalled')",
                params![account, client.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let change = write(&tx, account, &thread, &id, account, Some(&source), created)?
                .ok_or(FailureReason::ConflictingEcho)?;
            tx.commit()?;
            Ok(change)
        }

        pub(crate) fn fail_recall(&self, account: &str, client: u64, unknown: bool) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE recall_outbox SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                params![account,client.to_string(),if unknown {"unknown"} else {"rejected"}],
            )?;
            Ok(())
        }

        pub(crate) fn recall_outcome(&self, account: &str, client: u64) -> Result<RecallOutcome> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let state: String = self.conn.query_row(
                "SELECT state FROM recall_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
                |row| row.get(0),
            )?;
            Ok(match state.as_str() {
                "sending" => RecallOutcome::Pending,
                "unknown" => RecallOutcome::Unknown,
                "rejected" => RecallOutcome::Rejected,
                "acknowledged" => RecallOutcome::Acknowledged,
                "recalled" => RecallOutcome::Recalled,
                _ => bail!(FailureReason::ConflictingEcho),
            })
        }

        pub(crate) fn recover_recalls(&mut self, account: &str) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE recall_outbox SET state='unknown' WHERE account_id=?1 AND state='sending'",
                [account],
            )?;
            let clients: Vec<String> = {
                let mut query=self.conn.prepare_cached("SELECT client_id FROM recall_outbox WHERE account_id=?1 AND state='acknowledged' LIMIT ?2")?;
                let rows = query.query_map(params![account, MAX_UNRESOLVED], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for client in clients {
                self.accept_recall_ack(account, client.parse()?)?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "messaging")]
pub(super) use live::{
    apply_notice, redact_quote, redact_replay, redacted_copy, settle_recalled_own_echo,
};
#[cfg(feature = "messaging")]
pub(crate) use live::{RecallFailure, RecallOutcome};

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::{
        domain::{read_state::ReceiveContext, RECALLED_TEXT},
        protocol::{forward::ForwardMarker, recall::RecallNotice},
    };
    use serde_json::json;

    fn message(thread: &str, id: &str, sender: &str, body: &str) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: sender.into(),
            text: body.into(),
            sent_at_ms: 100,
            source_client_id: Some("101".into()),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            media: None,
            sticker: None,
        }
    }

    fn notice(id: &str, actor: &str) -> RecallNotice {
        RecallNotice {
            thread_id: "d:22".into(),
            message_id: id.into(),
            actor_id: actor.into(),
            client_id: None,
            event_id: "800".into(),
            occurred_at_ms: 201,
        }
    }

    fn receive(
        store: &mut MessageStore,
        messages: &[(ReceivedReply, Option<ForwardMarker>)],
        notices: &[RecallNotice],
        cursor: Option<(&str, &str)>,
    ) -> Result<ReceivedWithRecalls> {
        store.receive_batch_with_recalls(
            "11",
            messages,
            cursor,
            &[],
            ReceiveContext::OfflineSync,
            notices,
        )
    }

    #[test]
    fn explicit_marker_does_not_treat_a_literal_body_as_recalled() {
        let mut store = MessageStore::in_memory().unwrap();
        let ordinary = message("d:22", "7", "22", RECALLED_TEXT);
        store.insert_batch(&[ordinary]).unwrap();
        let loaded = store.refresh_messages("11", "d:22", &["7".into()]).unwrap();
        assert!(!loaded[0].is_recalled());
        receive(&mut store, &[], &[notice("7", "22")], None).unwrap();
        let loaded = store.refresh_messages("11", "d:22", &["7".into()]).unwrap();
        assert!(loaded[0].is_recalled());
        assert!(loaded[0].source_client_id.is_none());
    }

    #[test]
    fn own_recall_sanitizes_body_outbox_quotes_search_and_late_wire() {
        let mut store = MessageStore::in_memory().unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let client = store
            .prepare_outgoing("11", "d:22", "Bí mật tổng hợp", 100)
            .unwrap();
        let (source, _) = store.accept_outgoing("11", client, "7", Some(100)).unwrap();
        let mut reply = message("d:22", "9", "22", "Trả lời");
        reply.quote = Some(MessageQuote::from_message(&source).unwrap());
        let raw = QuoteContent::new(
            "11",
            &json!({"ownerId":"11","globalMsgId":"7","cliMsgId":client.to_string(),
            "cliMsgType":1,"ts":100,"msg":source.text,"sensitive_extension":source.text}),
        )
        .unwrap();
        receive(
            &mut store,
            &[((reply.clone(), None, None, None, Some(raw.clone())), None)],
            &[],
            None,
        )
        .unwrap();
        let previous_draft = store
            .prepare_outgoing_reply("11", "d:22", "Lời nháp cũ", "7", 160)
            .unwrap();

        let recall = store.prepare_recall("11", "d:22", "7", 171, 100).unwrap();
        assert!(recall.client_id > previous_draft);
        store.record_recall_ack("11", recall.client_id).unwrap();
        let changed = store.accept_recall_ack("11", recall.client_id).unwrap();
        assert!(changed.message.unwrap().is_recalled());
        assert_eq!(
            store.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Recalled
        );
        assert!(store
            .search_messages("11", "d:22", "Bí mật tổng hợp", 15)
            .unwrap()
            .hits
            .is_empty());
        assert!(store
            .message_delivery("11", "d:22", &["7".into()])
            .unwrap()
            .is_empty());
        let stored_body: String = store
            .conn
            .query_row(
                "SELECT body FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_body, RECALLED_TEXT);
        let quote = message_reply::outgoing(&store.conn, "11", previous_draft)
            .unwrap()
            .unwrap();
        assert_eq!(quote.text, RECALLED_TEXT);
        assert!(store
            .message_quote_content("11", "d:22", "9")
            .unwrap()
            .is_none());
        assert!(store
            .prepare_outgoing_reply("11", "d:22", "Không trích lại", "7", 172)
            .is_err());

        // Late original and reply replay must not restore either copy.
        let mut stale = source;
        stale.sent_at_ms = 101;
        receive(
            &mut store,
            &[
                (
                    (stale, Some(client.to_string()), None, None, None),
                    Some(ForwardMarker::Unavailable),
                ),
                ((reply, None, None, None, Some(raw)), None),
            ],
            &[],
            None,
        )
        .unwrap();
        let loaded = store
            .refresh_messages("11", "d:22", &["7".into(), "9".into()])
            .unwrap();
        assert_eq!(loaded[0].text, RECALLED_TEXT);
        assert_eq!(loaded[0].sent_at_ms, 100);
        assert_eq!(loaded[1].quote.as_ref().unwrap().text, RECALLED_TEXT);
        let wire = store.message_quote_content("11", "d:22", "9").unwrap();
        assert!(wire.is_none_or(|raw| !raw.as_json().contains("Bí mật tổng hợp")));
    }

    #[test]
    fn matched_notice_before_history_is_atomic_with_cursor_and_only_scoped() {
        let mut store = MessageStore::in_memory().unwrap();
        let remote = message("d:22", "7", "22", "Bí mật tổng hợp");
        let other = message("d:33", "7", "33", "Hội thoại khác");
        store.insert_batch(std::slice::from_ref(&other)).unwrap();
        let mut scoped = notice("7", "22");
        scoped.client_id = Some("101".into());
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_recall_cursor BEFORE INSERT ON receive_cursors
             BEGIN SELECT RAISE(ABORT,'synthetic rollback'); END;",
            )
            .unwrap();
        let same_frame = [(
            (remote.clone(), None, None, None, None),
            Some(ForwardMarker::Ordinary),
        )];
        assert!(receive(
            &mut store,
            &same_frame,
            std::slice::from_ref(&scoped),
            Some(("510_0", "99"))
        )
        .is_err());
        assert_eq!(store.receive_cursor("11", "510_0").unwrap(), "1");
        assert!(!materialized(&store.conn, &remote).unwrap());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_recall_cursor")
            .unwrap();

        receive(&mut store, &[], &[scoped], Some(("510_0", "99"))).unwrap();
        assert!(!materialized(&store.conn, &remote).unwrap());
        receive(&mut store, &same_frame, &[], None).unwrap();
        assert!(materialized(&store.conn, &remote).unwrap());
        let row = store.refresh_messages("11", "d:33", &["7".into()]).unwrap();
        assert_eq!(row, [other]);
    }

    #[test]
    fn acknowledgement_survives_local_failure_and_is_repaired_on_restart() {
        let mut store = MessageStore::in_memory().unwrap();
        let row = message("d:22", "7", "11", "Bí mật tổng hợp");
        store.replace_live_privacy("11", &[], None).unwrap();
        store.insert_batch(&[row]).unwrap();
        assert!(store.prepare_recall("11", "d:22", "7", 200, 100).is_err());
        let recall = store.prepare_recall("11", "d:22", "7", 199, 100).unwrap();
        store.record_recall_ack("11", recall.client_id).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_recall_projection BEFORE UPDATE OF body ON messages
             BEGIN SELECT RAISE(ABORT,'synthetic failed projection'); END;",
            )
            .unwrap();
        assert!(store.accept_recall_ack("11", recall.client_id).is_err());
        assert_eq!(
            store.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Acknowledged
        );
        store
            .conn
            .execute_batch("DROP TRIGGER reject_recall_projection")
            .unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.recover_recalls("11").unwrap();
        assert_eq!(
            reopened.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Recalled
        );
        assert!(reopened
            .refresh_messages("11", "d:22", &["7".into()])
            .unwrap()[0]
            .is_recalled());
    }
}
