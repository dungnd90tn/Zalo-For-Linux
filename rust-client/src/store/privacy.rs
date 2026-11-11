//! Session-only, opt-in projection policy. No on-disk schema or history is changed.
use super::*;
#[cfg(feature = "messaging")]
use crate::domain::fold_search;
#[cfg(any(feature = "messaging", test))]
const MAX_HIDDEN: usize = 20_000;
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA temp_store=MEMORY; CREATE TEMP TABLE IF NOT EXISTS live_privacy(account_id TEXT PRIMARY KEY,ready INTEGER NOT NULL,unlocked_thread TEXT);
        CREATE TEMP TABLE IF NOT EXISTS live_hidden(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,PRIMARY KEY(account_id,thread_id));
        CREATE TABLE IF NOT EXISTS privacy_hidden_cache(account_id TEXT NOT NULL,thread_id TEXT NOT NULL,PRIMARY KEY(account_id,thread_id));")?;
    Ok(())
}
impl MessageStore {
    /// Fail-closed policy with nothing known (tests only; the runtime starts from `provisional_live_privacy`).
    #[cfg(test)]
    pub(crate) fn begin_live_privacy(&self, account: &str) -> Result<()> {
        validate_id(account)?;
        self.conn.execute("INSERT INTO temp.live_privacy(account_id,ready) VALUES(?1,0) ON CONFLICT(account_id) DO UPDATE SET ready=0,unlocked_thread=NULL",[account])?;
        Ok(())
    }
    /// Applies the last hidden-thread set Zalo confirmed for this account (thread IDs only, kept on disk) as the
    /// policy while a fresh check runs, so the interface is usable at once and those threads stay hidden. With no
    /// earlier confirmation the set is empty. Any unlock lease is dropped.
    #[cfg(any(feature = "messaging", test))]
    pub(crate) fn provisional_live_privacy(&mut self, account: &str) -> Result<Vec<String>> {
        validate_id(account)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM temp.live_hidden WHERE account_id=?1",
            [account],
        )?;
        tx.execute("INSERT OR IGNORE INTO temp.live_hidden SELECT account_id,thread_id FROM privacy_hidden_cache WHERE account_id=?1",[account])?;
        tx.execute("INSERT INTO temp.live_privacy(account_id,ready) VALUES(?1,1) ON CONFLICT(account_id) DO UPDATE SET ready=1,unlocked_thread=NULL",[account])?;
        let threads = {
            let mut statement = tx.prepare_cached(
                "SELECT thread_id FROM temp.live_hidden WHERE account_id=?1 ORDER BY thread_id",
            )?;
            let rows = statement.query_map([account], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(threads)
    }
    #[cfg(any(feature = "messaging", test))]
    pub(crate) fn replace_live_privacy(
        &mut self,
        account: &str,
        threads: &[String],
        unlocked: Option<&str>,
    ) -> Result<()> {
        validate_id(account)?;
        if threads.len() > MAX_HIDDEN {
            bail!(FailureReason::Validation);
        }
        for thread in threads {
            validate_id(thread)?;
        }
        if unlocked.is_some_and(|id| !threads.iter().any(|thread| thread == id)) {
            bail!(FailureReason::Validation);
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM temp.live_hidden WHERE account_id=?1",
            [account],
        )?;
        tx.execute(
            "DELETE FROM privacy_hidden_cache WHERE account_id=?1",
            [account],
        )?;
        {
            let mut insert =
                tx.prepare_cached("INSERT OR IGNORE INTO temp.live_hidden VALUES(?1,?2)")?;
            let mut remember =
                tx.prepare_cached("INSERT OR IGNORE INTO privacy_hidden_cache VALUES(?1,?2)")?;
            for thread in threads {
                insert.execute(params![account, thread])?;
                remember.execute(params![account, thread])?;
            }
        }
        tx.execute("INSERT INTO temp.live_privacy VALUES(?1,1,?2) ON CONFLICT(account_id) DO UPDATE SET ready=1,unlocked_thread=excluded.unlocked_thread",params![account,unlocked])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn live_thread_visible(&self, account: &str, thread: &str) -> Result<bool> {
        validate_id(account)?;
        validate_id(thread)?;
        Ok(self.conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=0) AND
            (NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2) OR EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1 AND unlocked_thread=?2))",params![account,thread],|r|r.get(0))?)
    }
    #[cfg(any(feature = "messaging", test))]
    pub(crate) fn unlock_live_thread(&self, account: &str, thread: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE temp.live_privacy SET unlocked_thread=?2 WHERE account_id=?1",
            params![account, thread],
        )?;
        Ok(())
    }
    /// Explicit search is the only locked-contact projection. It contains no message/preview/time/count.
    #[cfg(feature = "messaging")]
    pub(crate) fn hidden_candidates(
        &self,
        account: &str,
        query: &str,
    ) -> Result<Vec<crate::app::live::HiddenCandidate>> {
        validate_id(account)?;
        if query.chars().count() > 128 || query.chars().any(char::is_control) {
            bail!(FailureReason::Validation);
        }
        let folded = fold_search(query);
        if folded.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement=self.conn.prepare_cached("SELECT h.thread_id,COALESCE(d.display_name,c.title,'') FROM temp.live_hidden h
            LEFT JOIN directory_entries d ON d.account_id=h.account_id AND d.thread_id=h.thread_id
            LEFT JOIN conversations c ON c.account_id=h.account_id AND c.thread_id=h.thread_id
            WHERE h.account_id=?1 AND EXISTS(SELECT 1 FROM temp.live_privacy p WHERE p.account_id=?1 AND p.ready=1)
            ORDER BY COALESCE(d.display_name,c.title,''),h.thread_id LIMIT 20000")?;
        let rows = statement.query_map(params![account], |r| {
            Ok(crate::app::live::HiddenCandidate {
                thread_id: r.get(0)?,
                title: r.get(1)?,
            })
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let candidate = row?;
            if fold_search(&candidate.title).contains(&folded)
                || candidate.thread_id.contains(&folded)
            {
                candidates.push(candidate);
                if candidates.len() == 50 {
                    break;
                }
            }
        }
        Ok(candidates)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reinitializing_same_connection_preserves_session_privacy_policy() {
        let mut store = MessageStore::in_memory().unwrap();
        store.begin_live_privacy("11").unwrap();
        store
            .replace_live_privacy("11", &["d:22".into(), "g:22".into()], Some("d:22"))
            .unwrap();
        store.begin_live_privacy("33").unwrap();
        let store = MessageStore::initialize(store.conn).unwrap();
        assert!(store.live_thread_visible("11", "d:22").unwrap());
        assert!(!store.live_thread_visible("11", "g:22").unwrap());
        assert!(!store.live_thread_visible("33", "d:44").unwrap());
        assert_eq!(
            store
                .conn
                .query_row("SELECT COUNT(*) FROM temp.live_hidden", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn unknown_hidden_scope_and_unlock_filter_reads_without_changing_history() {
        let mut s = MessageStore::in_memory().unwrap();
        let make = |account: &str, thread: &str, id: &str| crate::domain::Message {
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "22".into(),
            text: "private body".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        };
        s.insert_batch(&[
            make("11", "d:22", "1"),
            make("11", "g:22", "2"),
            make("33", "d:22", "3"),
        ])
        .unwrap();
        s.begin_live_privacy("11").unwrap();
        assert!(s.conversations("11", 50).unwrap().is_empty());
        assert!(s.page("11", "d:22", None, 50).unwrap().is_empty());
        assert_eq!(s.page("33", "d:22", None, 50).unwrap().len(), 1);
        s.replace_live_privacy("11", &["d:22".into()], None)
            .unwrap();
        assert_eq!(s.conversations("11", 50).unwrap()[0].id, "g:22");
        assert!(s.page("11", "d:22", None, 50).unwrap().is_empty());
        assert!(s.open_conversation("11", "d:22", "denied").is_err());
        s.unlock_live_thread("11", Some("d:22")).unwrap();
        assert_eq!(s.page("11", "d:22", None, 50).unwrap().len(), 1);
        assert_eq!(
            s.conversations("11", 50).unwrap().len(),
            1,
            "unlock must not add private conversation to normal recents"
        );
        s.unlock_live_thread("11", None).unwrap();
        assert!(s.page("11", "d:22", None, 50).unwrap().is_empty());
        s.replace_live_privacy("11", &[], None).unwrap();
        assert_eq!(
            s.page("11", "d:22", None, 50).unwrap().len(),
            1,
            "hide never deletes history"
        );
        assert_eq!(
            s.conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            9
        );
    }
}

#[cfg(test)]
mod provisional_tests {
    use super::*;
    #[test]
    fn last_confirmed_hidden_set_applies_at_once_on_the_next_start() {
        let make = |thread: &str, id: &str| crate::domain::Message {
            account_id: "11".into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "22".into(),
            text: "body".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        };
        let mut s = MessageStore::in_memory().unwrap();
        s.insert_batch(&[make("d:22", "1"), make("d:33", "2")])
            .unwrap();
        // Never confirmed: nothing is hidden, nothing blocks the list.
        assert!(s.provisional_live_privacy("11").unwrap().is_empty());
        assert_eq!(s.conversations("11", 50).unwrap().len(), 2);
        s.replace_live_privacy("11", &["d:22".into()], Some("d:22"))
            .unwrap();
        // A later start: the remembered set hides d:22 before any network check, and the lease is gone.
        assert_eq!(
            s.provisional_live_privacy("11").unwrap(),
            vec!["d:22".to_owned()]
        );
        let listed: Vec<_> = s
            .conversations("11", 50)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(listed, vec!["d:33".to_owned()]);
        assert!(!s.live_thread_visible("11", "d:22").unwrap());
        assert!(s.provisional_live_privacy("33").unwrap().is_empty());
    }
}

#[cfg(all(test, feature = "messaging"))]
#[path = "privacy_tests.rs"]
mod qa;
