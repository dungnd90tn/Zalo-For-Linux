//! Durable suppression for one ordinary message scoped to its stored sender.
use super::*;

pub(crate) const DELETED_FOR_ME_TEXT: &str = "[Tin nhắn đã xóa ở phía tôi]";

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS delete_for_me_intents(
            account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
            sender_id TEXT NOT NULL,source_client_id TEXT,operation_id INTEGER,
            state TEXT NOT NULL CHECK(state IN ('sending','unknown','acknowledged','deleted')),
            CHECK(operation_id IS NOT NULL OR state='deleted'),
            PRIMARY KEY(account_id,thread_id,message_id),
            UNIQUE(account_id,operation_id)
        ) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// A duplicate with conflicting identity is still a conflicting message.
/// A replay without a client ID can match the immutable server ID and sender.
pub(super) fn suppresses(conn: &Connection, message: &Message) -> Result<bool> {
    let binding: Option<(String, Option<String>)> = conn
        .prepare_cached(
            "SELECT sender_id,source_client_id FROM delete_for_me_intents
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND state='deleted'",
        )?
        .query_row(
            params![message.account_id, message.thread_id, message.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((sender, client_id)) = binding else {
        return Ok(false);
    };
    if sender != message.sender
        || client_id
            .as_ref()
            .zip(message.source_client_id.as_ref())
            .is_some_and(|(old, new)| old != new)
    {
        bail!(FailureReason::ConflictingMessage);
    }
    Ok(true)
}

pub(super) fn redact_quote(
    conn: &Connection,
    account: &str,
    thread: &str,
    quote: &mut MessageQuote,
) -> Result<bool> {
    let deleted: bool = conn
        .prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM delete_for_me_intents
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender_id=?4
               AND state='deleted')",
        )?
        .query_row(
            params![account, thread, quote.message_id, quote.owner_id],
            |row| row.get(0),
        )?;
    if deleted {
        DELETED_FOR_ME_TEXT.clone_into(&mut quote.text);
        quote.message_type = 0;
    }
    Ok(deleted)
}

#[cfg(feature = "messaging")]
pub(super) fn state(
    conn: &Connection,
    account: &str,
    thread: &str,
    id: &str,
) -> Result<Option<String>> {
    Ok(conn
        .prepare_cached(
            "SELECT state FROM delete_for_me_intents
         WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?
        .query_row(params![account, thread, id], |row| row.get(0))
        .optional()?)
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::reply::valid_source_id;
    use anyhow::ensure;

    const MAX_CLIENT_ID: i64 = 9_007_199_254_740_991;
    const MAX_UNRESOLVED: i64 = 100;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum DeleteForMeFailure {
        Unavailable,
        Unresolved,
    }

    impl std::fmt::Display for DeleteForMeFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(match self {
                Self::Unavailable => "Message is unavailable for deletion",
                Self::Unresolved => "Message deletion is unresolved",
            })
        }
    }

    impl std::error::Error for DeleteForMeFailure {}

    pub(crate) struct PreparedDeleteForMe {
        pub client_id: u64,
        pub sender_id: String,
        pub source_client_id: String,
    }

    pub(crate) struct DeleteForMeChange {
        pub thread_id: String,
        pub message_id: String,
        pub sender_id: String,
        pub conversation: Conversation,
    }

    struct Source {
        sender: String,
        client: Option<String>,
    }

    fn source(conn: &Connection, account: &str, thread: &str, id: &str) -> Result<Source> {
        let (kind, remote) = thread
            .split_once(':')
            .ok_or(DeleteForMeFailure::Unavailable)?;
        ensure!(
            valid_source_id(account)
                && valid_source_id(remote)
                && valid_source_id(id)
                && matches!(kind, "d" | "g")
                && (kind != "d" || account != remote),
            DeleteForMeFailure::Unavailable
        );
        let visible: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
            params![account, thread],
            |row| row.get(0),
        )?;
        ensure!(
            visible && !group_actions::write_blocked(conn, account, thread)?,
            DeleteForMeFailure::Unavailable
        );
        match state(conn, account, thread, id)?.as_deref() {
            Some("deleted") => bail!(DeleteForMeFailure::Unavailable),
            Some(_) => bail!(DeleteForMeFailure::Unresolved),
            None => {}
        }
        let recalling: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM recall_outbox WHERE account_id=?1 AND thread_id=?2
             AND message_id=?3 AND state IN ('sending','unknown','acknowledged'))",
            params![account, thread, id],
            |row| row.get(0),
        )?;
        ensure!(!recalling, DeleteForMeFailure::Unresolved);
        eligible_message(conn, account, thread, id, kind, remote)
    }

    fn eligible_message(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        kind: &str,
        remote: &str,
    ) -> Result<Source> {
        let (sender, body, client): (String, String, Option<String>) = conn.query_row(
            "SELECT m.sender,m.body,r.source_client_id FROM messages m
             LEFT JOIN message_reply r USING(account_id,thread_id,message_id)
             WHERE m.account_id=?1 AND m.thread_id=?2 AND m.message_id=?3 AND m.sent_at_ms>0
               AND r.quote_projection IS NULL AND COALESCE(r.reply_blocked,0)=0
               AND NOT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_mention WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')
               AND NOT EXISTS(SELECT 1 FROM list_silent_messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM recall_tombstones WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND materialized=1)",
            params![account,thread,id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?.ok_or(DeleteForMeFailure::Unavailable)?;
        ensure!(
            valid_source_id(&sender)
                && (kind != "d" || sender == remote || sender == account)
                && client.as_deref().is_none_or(valid_source_id)
                && !canonical_plain_text(&body).is_empty()
                && !body.starts_with("[Tin nhắn "),
            DeleteForMeFailure::Unavailable
        );
        Ok(Source { sender, client })
    }

    fn project(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        sender: &str,
    ) -> Result<DeleteForMeChange> {
        let current_sender: Option<String> = conn.query_row(
            "SELECT sender FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![account,thread,id], |row| row.get(0),
        ).optional()?;
        ensure!(
            current_sender
                .as_deref()
                .is_none_or(|current| current == sender),
            FailureReason::ConflictingMessage
        );
        message_reply::redact_copies(conn, account, thread, id, sender, DELETED_FOR_ME_TEXT)?;
        conn.execute(
            "UPDATE outgoing SET body=?4 WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND state='accepted' AND ?5=?1",
            params![account,thread,id,DELETED_FOR_ME_TEXT,sender],
        )?;
        for table in ["pending_reaction_events", "delivery_receipts", "messages"] {
            conn.execute(
                &format!(
                    "DELETE FROM {table} WHERE account_id=?1 AND thread_id=?2 AND message_id=?3"
                ),
                params![account, thread, id],
            )?;
        }
        conn.execute(
            "UPDATE conversations AS c SET last_message_at_ms=COALESCE((
               SELECT MAX(m.sent_at_ms) FROM messages m WHERE m.account_id=c.account_id AND m.thread_id=c.thread_id
               AND NOT EXISTS(SELECT 1 FROM list_silent_messages q WHERE q.account_id=m.account_id
                 AND q.thread_id=m.thread_id AND q.message_id=m.message_id)),0)
             WHERE c.account_id=?1 AND c.thread_id=?2",
            params![account,thread],
        )?;
        let conversation = conn.query_row(
            CONVERSATION_SQL,
            params![account, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        Ok(DeleteForMeChange {
            thread_id: thread.into(),
            message_id: id.into(),
            sender_id: sender.into(),
            conversation,
        })
    }

    impl MessageStore {
        pub(crate) fn delete_for_me_on_device(
            &mut self,
            account: &str,
            thread: &str,
            message_id: &str,
        ) -> Result<DeleteForMeChange> {
            let tx = self.conn.transaction()?;
            let target = source(&tx, account, thread, message_id)?;
            tx.execute(
                "INSERT INTO delete_for_me_intents VALUES(?1,?2,?3,?4,?5,NULL,'deleted')",
                params![account, thread, message_id, target.sender, target.client],
            )?;
            let change = project(&tx, account, thread, message_id, &target.sender)?;
            tx.commit()?;
            Ok(change)
        }

        pub(crate) fn prepare_delete_for_me(
            &mut self,
            account: &str,
            thread: &str,
            message_id: &str,
            now_ms: i64,
        ) -> Result<PreparedDeleteForMe> {
            ensure!(
                (1..=MAX_CLIENT_ID).contains(&now_ms),
                FailureReason::Validation
            );
            let tx = self.conn.transaction()?;
            let target = source(&tx, account, thread, message_id)?;
            let source_client_id = target.client.ok_or(DeleteForMeFailure::Unavailable)?;
            let count: i64 = tx.query_row(
                "SELECT count(*) FROM delete_for_me_intents WHERE account_id=?1 AND state<>'deleted'",
                [account], |row| row.get(0),
            )?;
            ensure!(count < MAX_UNRESOLVED, DeleteForMeFailure::Unresolved);
            let prev: Option<i64> = tx
                .query_row(
                    "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                    [account],
                    |row| row.get(0),
                )
                .optional()?;
            let client = prev
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(FailureReason::Validation)?
                .max(now_ms);
            ensure!(
                (1..=MAX_CLIENT_ID).contains(&client),
                FailureReason::Validation
            );
            tx.execute(
                "INSERT INTO outgoing_counters VALUES(?1,?2)
                 ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
                params![account, client],
            )?;
            tx.execute(
                "INSERT INTO delete_for_me_intents VALUES(?1,?2,?3,?4,?5,?6,'sending')",
                params![
                    account,
                    thread,
                    message_id,
                    target.sender,
                    source_client_id,
                    client
                ],
            )?;
            tx.commit()?;
            Ok(PreparedDeleteForMe {
                client_id: client as u64,
                sender_id: target.sender,
                source_client_id,
            })
        }

        pub(crate) fn delete_for_me_dispatch_pending(
            &self,
            account: &str,
            client_id: u64,
        ) -> Result<bool> {
            let client_id = i64::try_from(client_id)?;
            let thread: Option<String> = self.conn.query_row(
                "SELECT t.thread_id FROM delete_for_me_intents t
                 JOIN messages m USING(account_id,thread_id,message_id)
                 JOIN message_reply r USING(account_id,thread_id,message_id)
                 WHERE t.account_id=?1 AND t.operation_id=?2 AND t.state='sending'
                   AND m.sender=t.sender_id AND r.source_client_id=t.source_client_id
                   AND EXISTS(SELECT 1 FROM temp.live_privacy p WHERE p.account_id=?1 AND p.ready=1)
                   AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h WHERE h.account_id=?1 AND h.thread_id=t.thread_id)
                   AND NOT EXISTS(SELECT 1 FROM e2ee_threads e WHERE e.account_id=?1 AND e.thread_id=t.thread_id)
                   AND NOT EXISTS(SELECT 1 FROM recall_tombstones q WHERE q.account_id=?1
                     AND q.thread_id=t.thread_id AND q.message_id=t.message_id AND q.materialized=1)",
                params![account,client_id],
                |row| row.get(0),
            ).optional()?;
            match thread {
                Some(thread) => Ok(!group_actions::write_blocked(&self.conn, account, &thread)?),
                None => Ok(false),
            }
        }

        pub(crate) fn record_delete_for_me_ack(&self, account: &str, client_id: u64) -> Result<()> {
            ensure!(self.conn.execute(
                "UPDATE delete_for_me_intents SET state=CASE WHEN state='deleted' THEN state ELSE 'acknowledged' END
                 WHERE account_id=?1 AND operation_id=?2 AND state IN ('sending','unknown','acknowledged','deleted')",
                params![account,i64::try_from(client_id)?],
            )? == 1, FailureReason::ConflictingEcho);
            Ok(())
        }

        pub(crate) fn accept_delete_for_me_ack(
            &mut self,
            account: &str,
            client_id: u64,
        ) -> Result<DeleteForMeChange> {
            let tx = self.conn.transaction()?;
            let client_id = i64::try_from(client_id)?;
            let (thread, id, sender): (String, String, String) = tx.query_row(
                "SELECT thread_id,message_id,sender_id FROM delete_for_me_intents
                 WHERE account_id=?1 AND operation_id=?2 AND state IN ('acknowledged','deleted')",
                params![account, client_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            tx.execute(
                "UPDATE delete_for_me_intents SET state='deleted' WHERE account_id=?1 AND operation_id=?2",
                params![account,client_id],
            )?;
            let change = project(&tx, account, &thread, &id, &sender)?;
            tx.commit()?;
            Ok(change)
        }

        pub(crate) fn fail_delete_for_me(
            &self,
            account: &str,
            client_id: u64,
            unknown: bool,
        ) -> Result<()> {
            let sql = if unknown {
                "UPDATE delete_for_me_intents SET state='unknown'
                 WHERE account_id=?1 AND operation_id=?2 AND state='sending'"
            } else {
                "DELETE FROM delete_for_me_intents
                 WHERE account_id=?1 AND operation_id=?2 AND state='sending'"
            };
            self.conn
                .execute(sql, params![account, i64::try_from(client_id)?])?;
            Ok(())
        }

        pub(crate) fn recover_delete_for_me(&mut self, account: &str) -> Result<()> {
            self.conn.execute(
                "UPDATE delete_for_me_intents SET state='unknown' WHERE account_id=?1 AND state='sending'",
                [account],
            )?;
            let accepted: Vec<u64> = {
                let mut select = self.conn.prepare_cached(
                    "SELECT operation_id FROM delete_for_me_intents
                     WHERE account_id=?1 AND state='acknowledged' LIMIT ?2",
                )?;
                let rows = select.query_map(params![account, MAX_UNRESOLVED], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for client in accepted {
                self.accept_delete_for_me_ack(account, client)?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "messaging")]
pub(crate) use live::{DeleteForMeChange, DeleteForMeFailure};

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;

    fn text(account: &str, thread: &str, id: &str, sender: &str) -> Message {
        Message {
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: sender.into(),
            sent_at_ms: id.parse().unwrap(),
            source_client_id: Some(format!("9{id}")),
            text: format!("tin thử {id}"),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
        }
    }

    #[test]
    fn local_delete_recovers_preview_unread_and_scoped_quotes_without_remote_intent() {
        let mut store = MessageStore::in_memory().unwrap();
        let earlier = text("11", "d:22", "701", "22");
        let target = text("11", "d:22", "703", "22");
        let quote = MessageQuote {
            owner_id: target.sender.clone(),
            message_id: target.id.clone(),
            // The sender and server ID bind this stale optional quoted client ID.
            client_id: Some("777".into()),
            message_type: 1,
            sent_at_ms: target.sent_at_ms,
            text: target.text.clone(),
            ttl: None,
        };
        let mut mentioning_quote = earlier;
        mentioning_quote.quote = Some(quote);
        let sibling_group = text("11", "g:22", "703", "22");
        let sibling_account = text("33", "d:22", "703", "22");
        store
            .insert_batch(&[
                mentioning_quote.clone(),
                target.clone(),
                sibling_group,
                sibling_account,
            ])
            .unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        store.conn.execute(
            "INSERT INTO message_unread VALUES('11','d:22','701'),('11','d:22','703'),('33','d:22','703')",
            [],
        ).unwrap();

        let change = store.delete_for_me_on_device("11", "d:22", "703").unwrap();
        assert_eq!(
            (
                change.thread_id.as_str(),
                change.message_id.as_str(),
                change.sender_id.as_str()
            ),
            ("d:22", "703", "22")
        );
        assert_eq!(
            change.conversation.last_message_at_ms,
            mentioning_quote.sent_at_ms
        );
        assert_eq!(change.conversation.last_text, mentioning_quote.text);
        assert_eq!(store.thread_read_state("11", "d:22").unwrap().unread, 1);
        assert_eq!(store.thread_read_state("33", "d:22").unwrap().unread, 1);
        assert_eq!(store.page("11", "g:22", None, 20).unwrap().len(), 1);
        assert_eq!(store.page("33", "d:22", None, 20).unwrap().len(), 1);
        assert!(store
            .search_messages("11", "d:22", "tin thử 703", 20)
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM outgoing_counters", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        let hydrated = store.page("11", "d:22", None, 20).unwrap();
        assert_eq!(hydrated.len(), 1);
        assert_eq!(
            hydrated[0].quote.as_ref().unwrap().text,
            DELETED_FOR_ME_TEXT
        );
        assert!(store
            .message_quote_content("11", "d:22", "701")
            .unwrap()
            .is_none());

        assert!(store
            .receive_batch(
                "11",
                &[(target.clone(), None), (mentioning_quote, None)],
                Some(("510_0", "900")),
            )
            .unwrap()
            .is_empty());
        assert_eq!(store.receive_cursor("11", "510_0").unwrap(), "900");
        let mut conflicting = target;
        conflicting.sender = "99".into();
        assert!(store.insert_batch(&[conflicting]).is_err());
        assert_eq!(store.page("11", "d:22", None, 20).unwrap().len(), 1);
    }

    #[test]
    fn synced_delete_requires_exact_ack_and_recovers_after_projection_failure() {
        let mut store = MessageStore::in_memory().unwrap();
        let target = text("11", "g:22", "703", "55");
        store.insert_batch(std::slice::from_ref(&target)).unwrap();
        assert!(store
            .prepare_delete_for_me("11", "g:22", "703", 1000)
            .is_err());
        store.replace_live_privacy("11", &[], None).unwrap();
        let prepared = store
            .prepare_delete_for_me("11", "g:22", "703", 1000)
            .unwrap();
        let op = prepared.client_id;
        assert_eq!(prepared.sender_id, target.sender);
        assert_eq!(prepared.source_client_id, target.source_client_id.unwrap());
        assert!(store.delete_for_me_dispatch_pending("11", op).unwrap());
        assert!(store.accept_delete_for_me_ack("11", op).is_err());
        assert!(store.record_delete_for_me_ack("33", op).is_err());
        store.fail_delete_for_me("11", op, true).unwrap();
        assert!(!store.delete_for_me_dispatch_pending("11", op).unwrap());
        assert_eq!(store.page("11", "g:22", None, 20).unwrap().len(), 1);
        let error = store
            .delete_for_me_on_device("11", "g:22", "703")
            .err()
            .unwrap();
        assert_eq!(
            error.downcast_ref::<DeleteForMeFailure>(),
            Some(&DeleteForMeFailure::Unresolved)
        );

        store.record_delete_for_me_ack("11", op).unwrap();
        store.conn.execute_batch(
            "CREATE TRIGGER broken_delete BEFORE DELETE ON messages BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
        ).unwrap();
        assert!(store.accept_delete_for_me_ack("11", op).is_err());
        assert_eq!(store.page("11", "g:22", None, 20).unwrap().len(), 1);
        store
            .conn
            .execute_batch("DROP TRIGGER broken_delete")
            .unwrap();
        store.recover_delete_for_me("11").unwrap();
        assert!(store.page("11", "g:22", None, 20).unwrap().is_empty());
        store.accept_delete_for_me_ack("11", op).unwrap();
    }
}
