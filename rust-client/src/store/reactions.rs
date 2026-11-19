//! Scoped reaction events and a durable, one-shot local send reservation.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reaction_events (
            account_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            source_client_id TEXT NOT NULL,
            actor_id TEXT NOT NULL,
            client_id TEXT,
            event_id TEXT,
            occurred_at_ms INTEGER NOT NULL,
            kind INTEGER NOT NULL CHECK(kind BETWEEN -1 AND 4096),
            state TEXT NOT NULL CHECK(state IN ('sending','unknown','accepted','observed')),
            CHECK(client_id IS NOT NULL OR event_id IS NOT NULL),
            FOREIGN KEY(account_id,thread_id,message_id)
                REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE
        );
        CREATE UNIQUE INDEX IF NOT EXISTS reaction_event_id
            ON reaction_events(account_id,thread_id,message_id,event_id);
        CREATE UNIQUE INDEX IF NOT EXISTS reaction_local_client
            ON reaction_events(account_id,client_id)
            WHERE client_id IS NOT NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS reaction_unresolved
            ON reaction_events(account_id,thread_id,message_id)
            WHERE state IN ('sending','unknown');
        CREATE INDEX IF NOT EXISTS reaction_removals
            ON reaction_events(account_id,thread_id,message_id,actor_id,occurred_at_ms)
            WHERE kind=-1;",
    )?;
    Ok(())
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::protocol::reactions::{
        self as wire, ReactionAction, ReactionKind, ReactionNotice, ReactionOutcome,
        ReactionSummary,
    };
    use anyhow::ensure;
    use std::collections::BTreeSet;

    const MAX_JS_INTEGER: i64 = 9_007_199_254_740_991;
    const MAX_UNRESOLVED: i64 = 100;

    // reply_blocked/source_client_id encode the previously verified ordinary text
    // proof. Match both server IDs and reject recall, encryption and rich rows.
    fn source_client(
        conn: &Connection,
        account: &str,
        thread: &str,
        message: &str,
    ) -> Result<Option<String>> {
        Ok(conn.query_row(
            "SELECT r.source_client_id FROM message_reply r
             JOIN messages m USING(account_id,thread_id,message_id)
             WHERE r.account_id=?1 AND r.thread_id=?2 AND r.message_id=?3
               AND r.reply_blocked=0 AND r.source_client_id IS NOT NULL AND m.sent_at_ms>0
               AND (substr(r.thread_id,1,2)='g:' OR m.sender=?1 OR m.sender=substr(r.thread_id,3))
               AND NOT EXISTS(SELECT 1 FROM e2ee_threads e WHERE e.account_id=?1 AND e.thread_id=?2)
               AND NOT EXISTS(SELECT 1 FROM recall_tombstones t WHERE t.account_id=?1 AND t.thread_id=?2 AND t.message_id=?3 AND t.materialized=1)
               AND NOT EXISTS(SELECT 1 FROM message_content c WHERE c.account_id=?1 AND c.thread_id=?2 AND c.message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_info i WHERE i.account_id=?1 AND i.thread_id=?2 AND i.message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_media p WHERE p.account_id=?1 AND p.thread_id=?2 AND p.message_id=?3)",
            params![account, thread, message],
            |row| row.get(0),
        ).optional()?)
    }

    fn summary(
        conn: &Connection,
        account: &str,
        thread: &str,
        message: &str,
    ) -> Result<Vec<ReactionSummary>> {
        // Until a matching server echo arrives, a local ACK is ordered after the
        // observed notices by its monotonic client ID. Device clock skew must
        // never make a freshly acknowledged removal look older than its target.
        let mut query = conn.prepare_cached(
            "SELECT e.kind, count(*), max(e.actor_id=?1)
             FROM reaction_events e
             WHERE e.account_id=?1 AND e.thread_id=?2 AND e.message_id=?3 AND e.kind>=0
               AND e.state IN ('accepted','observed')
               AND NOT EXISTS(
                   SELECT 1 FROM reaction_events r
                   WHERE r.account_id=e.account_id AND r.thread_id=e.thread_id
                     AND r.message_id=e.message_id AND r.actor_id=e.actor_id AND r.kind=-1
                     AND r.state IN ('accepted','observed')
                     AND (r.state='accepted',
                          CASE WHEN r.state='accepted' THEN CAST(r.client_id AS INTEGER) ELSE r.occurred_at_ms END,
                          length(COALESCE(r.event_id,r.client_id)),COALESCE(r.event_id,r.client_id))
                         >= (e.state='accepted',
                             CASE WHEN e.state='accepted' THEN CAST(e.client_id AS INTEGER) ELSE e.occurred_at_ms END,
                             length(COALESCE(e.event_id,e.client_id)),COALESCE(e.event_id,e.client_id))
               )
             GROUP BY e.kind ORDER BY count(*) DESC, e.kind",
        )?;
        let rows = query.query_map(params![account, thread, message], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })?;
        let mut summary = Vec::new();
        for row in rows {
            let (code, count, mine) = row?;
            if let Some(kind) = ReactionKind::from_code(code) {
                summary.push(ReactionSummary {
                    kind,
                    count: u32::try_from(count).unwrap_or(u32::MAX),
                    mine,
                });
            }
        }
        Ok(summary)
    }

    pub(in super::super) fn apply(
        conn: &Connection,
        account: &str,
        notices: &[ReactionNotice],
    ) -> Result<BTreeSet<String>> {
        ensure!(
            notices.len() <= wire::MAX_NOTICES,
            FailureReason::Validation
        );
        let mut changed = BTreeSet::new();
        for notice in notices {
            if source_client(conn, account, &notice.thread, &notice.message_id)?.as_deref()
                != Some(notice.source_client_id.as_str())
            {
                continue;
            }
            if notice.actor == account {
                let Some(client) = &notice.client_id else {
                    // Without the client ID, the same action might already
                    // have been counted from this device's HTTP acknowledgement.
                    continue;
                };
                let reserved: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM reaction_events WHERE account_id=?1 AND client_id=?2)",
                    params![account, client],
                    |row| row.get(0),
                )?;
                if reserved {
                    // Exact action and source only. A conflicting echo must not
                    // consume the local reservation or increment the total.
                    if conn.execute(
                        "UPDATE OR IGNORE reaction_events
                         SET state='observed',event_id=?7,occurred_at_ms=?8
                         WHERE account_id=?1 AND client_id=?2 AND actor_id=?1
                           AND thread_id=?3 AND message_id=?4 AND source_client_id=?5 AND kind=?6
                           AND state IN ('sending','unknown','accepted')",
                        params![
                            account,
                            client,
                            notice.thread,
                            notice.message_id,
                            notice.source_client_id,
                            notice.code,
                            notice.event_id,
                            notice.timestamp
                        ],
                    )? != 0
                    {
                        changed.insert(notice.thread.clone());
                    }
                    continue;
                }
            }
            if conn.execute(
                "INSERT INTO reaction_events(account_id,thread_id,message_id,source_client_id,actor_id,event_id,occurred_at_ms,kind,state)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'observed')
                 ON CONFLICT(account_id,thread_id,message_id,event_id) DO NOTHING",
                params![account, notice.thread, notice.message_id, notice.source_client_id,
                    notice.actor, notice.event_id, notice.timestamp, notice.code],
            )? != 0 {
                changed.insert(notice.thread.clone());
            }
        }
        Ok(changed)
    }

    impl MessageStore {
        pub(crate) fn apply_reaction_notices(
            &mut self,
            account: &str,
            notices: &[ReactionNotice],
        ) -> Result<BTreeSet<String>> {
            let tx = self.conn.transaction()?;
            let changed = apply(&tx, account, notices)?;
            tx.commit()?;
            Ok(changed)
        }

        pub(crate) fn reaction_summaries(
            &self,
            account: &str,
            thread: &str,
            ids: &[String],
        ) -> Result<Vec<(String, Vec<ReactionSummary>)>> {
            ensure!(ids.len() <= MAX_REFRESH_MESSAGES, FailureReason::Validation);
            if !self.live_thread_visible(account, thread)? {
                return Ok(Vec::new());
            }
            let mut summaries = Vec::new();
            for id in ids {
                if source_client(&self.conn, account, thread, id)?.is_some() {
                    summaries.push((id.clone(), summary(&self.conn, account, thread, id)?));
                }
            }
            Ok(summaries)
        }

        pub(crate) fn reserve_reaction(
            &mut self,
            account: &str,
            thread: &str,
            message: &str,
            action: ReactionAction,
            now: i64,
        ) -> Result<std::result::Result<(u64, String), ReactionOutcome>> {
            use ReactionOutcome::*;
            if !(1..=MAX_JS_INTEGER).contains(&now) || !self.live_thread_visible(account, thread)? {
                return Ok(Err(Unavailable));
            }
            let tx = self.conn.transaction()?;
            let Some(source) = source_client(&tx, account, thread, message)?
                .filter(|source| wire::reference_supported(message, source))
            else {
                return Ok(Err(Unavailable));
            };
            let closed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM group_left WHERE account_id=?1 AND thread_id=?2)",
                params![account, thread],
                |row| row.get(0),
            )?;
            if closed {
                return Ok(Err(Unavailable));
            }
            let (pending, same): (i64, bool) = tx.query_row(
                "SELECT count(*), COALESCE(MAX(thread_id=?2 AND message_id=?3),0)
                 FROM reaction_events WHERE account_id=?1 AND state IN ('sending','unknown')",
                params![account, thread, message],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if same {
                return Ok(Err(Unknown));
            }
            if pending >= MAX_UNRESOLVED
                || (action == ReactionAction::RemoveAll
                    && !summary(&tx, account, thread, message)?
                        .iter()
                        .any(|item| item.mine))
            {
                return Ok(Err(Unavailable));
            }
            let client_id: i64 = tx.query_row(
                "INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2)
                 ON CONFLICT(account_id) DO UPDATE SET last_id=MAX(last_id+1,excluded.last_id)
                 RETURNING last_id",
                params![account, now],
                |row| row.get(0),
            )?;
            ensure!(client_id <= MAX_JS_INTEGER, FailureReason::Validation);
            tx.execute(
                "INSERT INTO reaction_events(account_id,thread_id,message_id,source_client_id,actor_id,client_id,occurred_at_ms,kind,state)
                 VALUES(?1,?2,?3,?4,?1,?5,?6,?7,'sending')",
                params![account, thread, message, source, client_id.to_string(), now, action.code()],
            )?;
            tx.commit()?;
            Ok(Ok((client_id as u64, source)))
        }

        pub(crate) fn settle_reaction(
            &self,
            account: &str,
            client: u64,
            outcome: ReactionOutcome,
        ) -> Result<ReactionOutcome> {
            use ReactionOutcome::*;
            let client = client.to_string();
            let state: String = self.conn.query_row(
                "SELECT state FROM reaction_events WHERE account_id=?1 AND client_id=?2",
                params![account, client],
                |row| row.get(0),
            )?;
            // The authenticated socket can observe the action before the HTTP result.
            if matches!(state.as_str(), "observed" | "accepted") {
                return Ok(Accepted);
            }
            if matches!(outcome, Rejected | Unavailable) {
                self.conn.execute(
                    "DELETE FROM reaction_events WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                    params![account, client],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE reaction_events SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                    params![account, client, if outcome == Accepted {"accepted"} else {"unknown"}],
                )?;
            }
            Ok(outcome)
        }
    }
}

#[cfg(feature = "messaging")]
pub(super) use live::apply;

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::protocol::reactions::{
        ReactionAction, ReactionKind, ReactionNotice, ReactionOutcome,
    };

    fn source() -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "910".into(),
            sender: "22".into(),
            text: "Tin tổng hợp".into(),
            sent_at_ms: 1,
            sticker: None,
            media: None,
            quote: None,
            source_client_id: Some("990".into()),
            recalled: false,
            reply_blocked: false,
        }
    }

    fn notice(event: &str, code: i64, time: i64) -> ReactionNotice {
        ReactionNotice {
            thread: "d:22".into(),
            message_id: "910".into(),
            source_client_id: "990".into(),
            event_id: event.into(),
            actor: "22".into(),
            client_id: None,
            timestamp: time,
            code,
        }
    }

    #[test]
    fn scope_replay_and_out_of_order_removal_do_not_double_count_or_leak() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut other = source();
        other.thread_id = "d:33".into();
        other.sender = "33".into();
        store.insert_batch(&[source(), other]).unwrap();
        let ids = ["910".into()];
        let mut wrong_source = notice("95", 3, 5);
        wrong_source.source_client_id = "991".into();
        store
            .apply_reaction_notices(
                "11",
                &[
                    notice("92", -1, 4),
                    notice("91", 3, 2),
                    notice("94", 5, 5),
                    notice("94", 5, 5),
                    wrong_source,
                ],
            )
            .unwrap();
        let result = store.reaction_summaries("11", "d:22", &ids).unwrap();
        let [heart] = result[0].1.as_slice() else {
            panic!("expected one peer reaction")
        };
        assert_eq!(heart.kind, ReactionKind::Heart);
        assert_eq!(heart.count, 1);
        assert!(!heart.mine);
        assert!(store.reaction_summaries("11", "d:33", &ids).unwrap()[0]
            .1
            .is_empty());
        assert!(store
            .reaction_summaries("12", "d:22", &ids)
            .unwrap()
            .is_empty());
        store
            .replace_live_privacy("11", &["d:22".into()], None)
            .unwrap();
        assert!(store
            .reaction_summaries("11", "d:22", &ids)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn ambiguous_dispatch_recovers_only_from_an_exact_authenticated_own_echo() {
        use ReactionOutcome::*;
        let mut store = MessageStore::in_memory().unwrap();
        store.insert_batch(&[source()]).unwrap();
        let add = ReactionAction::Add(ReactionKind::Heart);
        let (client, _) = store
            .reserve_reaction("11", "d:22", "910", add, 10)
            .unwrap()
            .unwrap();
        assert_eq!(
            store.settle_reaction("11", client, Unknown).unwrap(),
            Unknown
        );
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        assert_eq!(
            reopened
                .reserve_reaction("11", "d:22", "910", add, 11)
                .unwrap(),
            Err(Unknown)
        );
        let mut wrong = notice("92", 3, 12);
        wrong.actor = "11".into();
        wrong.client_id = Some(client.to_string());
        assert!(reopened
            .apply_reaction_notices("11", &[wrong])
            .unwrap()
            .is_empty());
        let mut echo = notice("93", 5, 1_000);
        echo.actor = "11".into();
        echo.client_id = Some(client.to_string());
        assert_eq!(
            reopened
                .apply_reaction_notices("11", &[echo])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            reopened.settle_reaction("11", client, Unknown).unwrap(),
            Accepted
        );
        let (remove, _) = reopened
            .reserve_reaction("11", "d:22", "910", ReactionAction::RemoveAll, 14)
            .unwrap()
            .unwrap();
        assert_eq!(
            reopened.settle_reaction("11", remove, Accepted).unwrap(),
            Accepted
        );
        let ids = ["910".into()];
        assert!(reopened.reaction_summaries("11", "d:22", &ids).unwrap()[0]
            .1
            .is_empty());
        assert_eq!(
            reopened
                .reserve_reaction("11", "d:22", "910", ReactionAction::RemoveAll, 15)
                .unwrap(),
            Err(Unavailable)
        );
    }
}
