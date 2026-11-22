//! Durable one-shot guard for an explicit first vote. It never stores the choice.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS poll_vote_guards (
            account_id TEXT NOT NULL,
            poll_id INTEGER NOT NULL CHECK(poll_id BETWEEN 1 AND 9007199254740991),
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            joined INTEGER NOT NULL CHECK(joined IN (0,1)),
            PRIMARY KEY (account_id,poll_id)
        ) WITHOUT ROWID;",
    )?;
    Ok(())
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::{
        poll::{PollVoteSettlement, PollVoteState, MAX_POLL_ID},
        reply::valid_source_id,
    };
    use anyhow::ensure;
    use rusqlite::{Transaction, TransactionBehavior};

    fn validate_poll(account: &str, poll_id: u64) -> Result<()> {
        ensure!(
            valid_source_id(account) && (1..=MAX_POLL_ID).contains(&poll_id),
            FailureReason::Validation
        );
        Ok(())
    }

    impl MessageStore {
        pub(crate) fn poll_vote_state(
            &self,
            account: &str,
            poll_id: u64,
        ) -> Result<Option<PollVoteState>> {
            validate_poll(account, poll_id)?;
            Ok(self
                .conn
                .query_row(
                    "SELECT joined FROM poll_vote_guards WHERE account_id=?1 AND poll_id=?2",
                    params![account, poll_id],
                    |row| row.get::<_, bool>(0),
                )
                .optional()?
                .map(|joined| {
                    if joined {
                        PollVoteState::Joined
                    } else {
                        PollVoteState::Pending
                    }
                }))
        }

        /// Called after network preflight and before dispatch; source and guard are checked together.
        pub(crate) fn reserve_poll_vote(
            &mut self,
            account: &str,
            thread: &str,
            message: &str,
            poll_id: u64,
        ) -> Result<bool> {
            validate_poll(account, poll_id)?;
            let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
            if self.poll_detail_target(account, thread, message)? != Some(poll_id) {
                return Ok(false);
            }
            let inserted = tx.execute(
                "INSERT INTO poll_vote_guards(account_id,poll_id,thread_id,message_id,joined)
                 VALUES(?1,?2,?3,?4,0) ON CONFLICT(account_id,poll_id) DO NOTHING",
                params![account, poll_id, thread, message],
            )?;
            tx.commit()?;
            Ok(inserted == 1)
        }

        pub(crate) fn settle_poll_vote(
            &mut self,
            account: &str,
            thread: &str,
            message: &str,
            poll_id: u64,
            settlement: PollVoteSettlement,
        ) -> Result<()> {
            validate_poll(account, poll_id)?;
            match settlement {
                PollVoteSettlement::NotDispatched => {
                    self.conn.execute(
                        "DELETE FROM poll_vote_guards
                         WHERE account_id=?1 AND poll_id=?2 AND thread_id=?3 AND message_id=?4 AND joined=0",
                        params![account, poll_id, thread, message],
                    )?;
                }
                PollVoteSettlement::Joined => {
                    // Fresh positive participation can also arrive through an exact canonical alias.
                    // The original source key stays valid after the view or message is gone.
                    let tx =
                        Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
                    let canonical =
                        self.poll_detail_target(account, thread, message)? == Some(poll_id);
                    tx.execute(
                        "UPDATE poll_vote_guards SET joined=1 WHERE account_id=?1 AND poll_id=?2
                         AND joined=0 AND (?5 OR (thread_id=?3 AND message_id=?4))",
                        params![account, poll_id, thread, message, canonical],
                    )?;
                    tx.commit()?;
                }
            }
            Ok(())
        }
    }
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::domain::poll::{PollVoteSettlement, PollVoteState};
    use serde_json::json;

    fn insert_notice(store: &mut MessageStore, account: &str, thread: &str, id: &str) {
        let content = MediaMessageContent::new(
            MediaKind::Poll,
            &json!({"params":{"pollId":9,"question":"Thời điểm nào?"}}),
        )
        .unwrap();
        let message = Message {
            expires_at_ms: None,
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "81".into(),
            text: MediaKind::Poll.legacy_placeholder().into(),
            sent_at_ms: 1_700_000_000_000,
            source_client_id: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: Some(content.descriptor().clone()),
        };
        store
            .receive_batch_with_media(account, &[(message, None, None, Some(content))], None)
            .unwrap();
    }

    #[test]
    fn first_vote_guard_survives_upgrade_restart_and_notice_aliases() {
        struct Disposable(std::path::PathBuf);
        impl Drop for Disposable {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let path = Disposable(std::env::temp_dir().join(format!(
            "zalo-poll-vote-{}.sqlite3",
            crate::domain::local_message_id().unwrap()
        )));
        let mut store = MessageStore::open(&path.0).unwrap();
        insert_notice(&mut store, "11", "g:21", "801");
        insert_notice(&mut store, "11", "g:22", "802");
        insert_notice(&mut store, "12", "g:21", "801");
        store
            .conn
            .execute("DROP TABLE poll_vote_guards", [])
            .unwrap();
        store.conn.pragma_update(None, "user_version", 24).unwrap();
        drop(store);

        let mut store = MessageStore::open(&path.0).unwrap();
        let schema: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(schema, SCHEMA_VERSION);
        assert!(!store.reserve_poll_vote("11", "g:21", "801", 9).unwrap());
        store.replace_live_privacy("11", &[], None).unwrap();
        assert!(!store.reserve_poll_vote("11", "g:21", "801", 10).unwrap());
        assert!(store.reserve_poll_vote("11", "g:21", "801", 9).unwrap());
        assert!(!store.reserve_poll_vote("11", "g:22", "802", 9).unwrap());
        store
            .settle_poll_vote("11", "g:22", "802", 9, PollVoteSettlement::NotDispatched)
            .unwrap();
        assert_eq!(
            store.poll_vote_state("11", 9).unwrap(),
            Some(PollVoteState::Pending)
        );
        assert_eq!(store.poll_vote_state("12", 9).unwrap(), None);
        drop(store);

        let mut store = MessageStore::open(&path.0).unwrap();
        assert_eq!(
            store.poll_vote_state("11", 9).unwrap(),
            Some(PollVoteState::Pending)
        );
        store.replace_live_privacy("11", &[], None).unwrap();
        assert!(!store.reserve_poll_vote("11", "g:22", "802", 9).unwrap());
        store
            .settle_poll_vote("11", "g:22", "888", 9, PollVoteSettlement::Joined)
            .unwrap();
        assert_eq!(
            store.poll_vote_state("11", 9).unwrap(),
            Some(PollVoteState::Pending)
        );
        store
            .settle_poll_vote("11", "g:22", "802", 9, PollVoteSettlement::Joined)
            .unwrap();
        store
            .settle_poll_vote("11", "g:21", "801", 9, PollVoteSettlement::NotDispatched)
            .unwrap();
        assert_eq!(
            store.poll_vote_state("11", 9).unwrap(),
            Some(PollVoteState::Joined)
        );
        assert!(!store.reserve_poll_vote("11", "g:21", "801", 9).unwrap());

        store.replace_live_privacy("12", &[], None).unwrap();
        assert!(store.reserve_poll_vote("12", "g:21", "801", 9).unwrap());
        store
            .settle_poll_vote("12", "g:21", "801", 9, PollVoteSettlement::NotDispatched)
            .unwrap();
        assert_eq!(store.poll_vote_state("12", 9).unwrap(), None);
        assert!(store.reserve_poll_vote("12", "g:21", "801", 9).unwrap());
    }
}
