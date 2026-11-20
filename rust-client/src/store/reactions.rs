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
            WHERE kind=-1;
        CREATE TABLE IF NOT EXISTS pending_reaction_events (
            account_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            source_client_id TEXT NOT NULL,
            actor_id TEXT NOT NULL,
            client_id TEXT,
            event_id TEXT NOT NULL,
            occurred_at_ms INTEGER NOT NULL,
            kind INTEGER NOT NULL CHECK(kind BETWEEN -1 AND 4096),
            PRIMARY KEY(account_id,thread_id,message_id,event_id)
        ) WITHOUT ROWID;",
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
    const MAX_PENDING_NOTICES: i64 = 4096;

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
        apply_batch(conn, account, notices, false)
    }

    fn apply_batch(
        conn: &Connection,
        account: &str,
        notices: &[ReactionNotice],
        stage_missing: bool,
    ) -> Result<BTreeSet<String>> {
        ensure!(
            notices.len() <= wire::MAX_NOTICES,
            FailureReason::Validation
        );
        let mut changed = BTreeSet::new();
        let mut staged = false;
        for notice in notices {
            if notice.actor == account && notice.client_id.is_none() {
                // Without the client ID, the same action might already have
                // been counted from this device's HTTP acknowledgement.
                continue;
            }
            match source_client(conn, account, &notice.thread, &notice.message_id)? {
                Some(source) if source == notice.source_client_id => {
                    if record_observed(conn, account, notice)? {
                        changed.insert(notice.thread.clone());
                    }
                }
                None if stage_missing => staged |= stage_if_missing(conn, account, notice)?,
                _ => {}
            }
        }
        if staged {
            let pending: i64 = conn.query_row(
                "SELECT count(*) FROM pending_reaction_events WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )?;
            // A replay can retry this page when ordinary history has released
            // pending slots. Never evict a committed notice to fit a new page.
            ensure!(pending <= MAX_PENDING_NOTICES, FailureReason::Validation);
        }
        Ok(changed)
    }

    fn stage_if_missing(conn: &Connection, account: &str, notice: &ReactionNotice) -> Result<bool> {
        Ok(conn.execute(
            "INSERT INTO pending_reaction_events
               (account_id,thread_id,message_id,source_client_id,actor_id,client_id,event_id,occurred_at_ms,kind)
             SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9
             WHERE NOT EXISTS(SELECT 1 FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
               AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)
             ON CONFLICT(account_id,thread_id,message_id,event_id) DO NOTHING",
            params![account,notice.thread,notice.message_id,notice.source_client_id,notice.actor,
                notice.client_id,notice.event_id,notice.timestamp,notice.code],
        )? != 0)
    }

    fn record_observed(conn: &Connection, account: &str, notice: &ReactionNotice) -> Result<bool> {
        if notice.actor == account {
            if let Some(client) = &notice.client_id {
                let reserved: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM reaction_events WHERE account_id=?1 AND client_id=?2)",
                    params![account, client],
                    |row| row.get(0),
                )?;
                if reserved {
                    // Exact action and source only. A conflicting echo must not
                    // consume the local reservation or increment the total.
                    return Ok(conn.execute(
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
                    )? != 0);
                }
            }
        }
        Ok(conn.execute(
                "INSERT INTO reaction_events(account_id,thread_id,message_id,source_client_id,actor_id,event_id,occurred_at_ms,kind,state)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'observed')
                 ON CONFLICT(account_id,thread_id,message_id,event_id) DO NOTHING",
                params![account, notice.thread, notice.message_id, notice.source_client_id,
                    notice.actor, notice.event_id, notice.timestamp, notice.code],
            )? != 0)
    }

    pub(in super::super) fn reconcile_pending(
        conn: &Connection,
        account: &str,
        thread: &str,
        message: &str,
    ) -> Result<bool> {
        let mut statement = conn.prepare_cached(
            "SELECT source_client_id,actor_id,client_id,event_id,occurred_at_ms,kind
             FROM pending_reaction_events WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?;
        let mut rows = statement.query(params![account, thread, message])?;
        let Some(mut row) = rows.next()? else {
            return Ok(false);
        };
        let source = source_client(conn, account, thread, message)?;
        let mut changed = false;
        loop {
            let source_client_id: String = row.get(0)?;
            if source.as_deref() == Some(&source_client_id) {
                changed |= record_observed(
                    conn,
                    account,
                    &ReactionNotice {
                        thread: thread.into(),
                        message_id: message.into(),
                        source_client_id,
                        actor: row.get(1)?,
                        client_id: row.get(2)?,
                        event_id: row.get(3)?,
                        timestamp: row.get(4)?,
                        code: row.get(5)?,
                    },
                )?;
            }
            let Some(next) = rows.next()? else { break };
            row = next;
        }
        drop(rows);
        conn.execute(
            "DELETE FROM pending_reaction_events WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![account, thread, message],
        )?;
        Ok(changed)
    }

    impl MessageStore {
        #[cfg(test)]
        pub(crate) fn apply_reaction_notices(
            &mut self,
            account: &str,
            notices: &[ReactionNotice],
        ) -> Result<BTreeSet<String>> {
            self.commit_reaction_page(account, notices, None)
        }

        pub(crate) fn commit_reaction_page(
            &mut self,
            account: &str,
            notices: &[ReactionNotice],
            cursor: Option<(u16, u64)>,
        ) -> Result<BTreeSet<String>> {
            let tx = self.conn.transaction()?;
            let changed = apply_batch(&tx, account, notices, true)?;
            if let Some((command, last_id)) = cursor {
                ensure!(
                    matches!(command, wire::DIRECT_PULL | wire::GROUP_PULL)
                        && (1..=MAX_JS_INTEGER as u64).contains(&last_id),
                    FailureReason::Validation
                );
                advance_receive_cursor(
                    &tx,
                    account,
                    &format!("{command}_0"),
                    &last_id.to_string(),
                )?;
            }
            tx.commit()?;
            Ok(changed)
        }

        pub(crate) fn reaction_summaries(
            &mut self,
            account: &str,
            thread: &str,
            ids: &[String],
        ) -> Result<Vec<(String, Vec<ReactionSummary>)>> {
            ensure!(ids.len() <= MAX_REFRESH_MESSAGES, FailureReason::Validation);
            if !self.live_thread_visible(account, thread)? {
                return Ok(Vec::new());
            }
            let tx = self.conn.transaction()?;
            let mut summaries = Vec::new();
            for id in ids {
                if source_client(&tx, account, thread, id)?.is_some() {
                    reconcile_pending(&tx, account, thread, id)?;
                    summaries.push((id.clone(), summary(&tx, account, thread, id)?));
                }
            }
            tx.commit()?;
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
            reconcile_pending(&tx, account, thread, message)?;
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
pub(super) use live::{apply, reconcile_pending};

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
            mentions: Vec::new(),
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

    #[test]
    fn schema_seventeen_upgrade_and_reaction_cursor_failure_preserve_history_atomically() {
        let mut original = MessageStore::in_memory().unwrap();
        let mut cached = source();
        cached.id = "912".into();
        original
            .receive_batch("11", &[(cached.clone(), None)], Some(("510_0", "73")))
            .unwrap();
        original
            .apply_reaction_notices(
                "11",
                &[ReactionNotice {
                    message_id: "912".into(),
                    ..notice("90", 5, 2)
                }],
            )
            .unwrap();
        original
            .conn
            .execute_batch("DROP TABLE pending_reaction_events; PRAGMA user_version=17;")
            .unwrap();
        let mut upgraded = MessageStore::initialize(original.conn).unwrap();
        assert_eq!(
            upgraded
                .conn
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            19
        );
        assert_eq!(upgraded.receive_cursor("11", "510_0").unwrap(), "73");
        assert_eq!(upgraded.page("11", "d:22", None, 10).unwrap(), [cached]);
        assert_eq!(
            upgraded
                .reaction_summaries("11", "d:22", &["912".into()])
                .unwrap()[0]
                .1[0]
                .count,
            1
        );
        upgraded.conn.execute_batch(
            "CREATE TRIGGER prohibit_test_cursor BEFORE INSERT ON receive_cursors
             WHEN NEW.queue='610_0' BEGIN SELECT RAISE(ABORT, 'synthetic cursor write failure'); END;",
        ).unwrap();
        let actions = [notice("91", 3, 4)];
        assert!(upgraded
            .commit_reaction_page("11", &actions, Some((610, 100)))
            .is_err());
        let count_pending = |store: &MessageStore| {
            store
                .conn
                .query_row("SELECT count(*) FROM pending_reaction_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(count_pending(&upgraded), 0);
        assert_eq!(upgraded.receive_cursor("11", "610_0").unwrap(), "1");
        upgraded
            .conn
            .execute_batch("DROP TRIGGER prohibit_test_cursor;")
            .unwrap();
        assert!(upgraded
            .commit_reaction_page("11", &actions, Some((610, 100)))
            .unwrap()
            .is_empty());
        assert_eq!(count_pending(&upgraded), 1);
        assert_eq!(upgraded.receive_cursor("11", "610_0").unwrap(), "100");
        upgraded
            .receive_batch("11", &[(source(), None)], None)
            .unwrap();
        assert_eq!(count_pending(&upgraded), 0);
        assert_eq!(
            upgraded
                .reaction_summaries("11", "d:22", &["910".into()])
                .unwrap()[0]
                .1[0]
                .kind,
            ReactionKind::Like
        );
    }

    #[test]
    fn deferred_capacity_stops_the_page_without_evicting_old_actions() {
        let mut store = MessageStore::in_memory().unwrap();
        store
            .conn
            .execute_batch(
                "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<4096)
             INSERT INTO pending_reaction_events
             SELECT '11','d:22','911','990','22',NULL,CAST(n AS TEXT),n,3 FROM seq;",
            )
            .unwrap();
        assert!(store
            .commit_reaction_page("11", &[notice("93", 5, 12)], Some((610, 100)))
            .is_err());
        assert_eq!(store.receive_cursor("11", "610_0").unwrap(), "1");
        let counts: (i64, i64) = store
            .conn
            .query_row(
                "SELECT count(*),sum(message_id='911') FROM pending_reaction_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (4096, 4096));
    }
}
