//! Account-scoped directory metadata applied by the existing SQLite owner only.
use super::{insert_validated_with_media, MessageStore};
use crate::domain::{
    fold_search, validate_id, Message, MessageInfoBinding, MessageMediaBinding,
    MAX_INFO_BATCH_BYTES, MAX_PAGE_SIZE,
};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

pub const DIRECTORY_LIMIT: usize = 200;
pub const MAX_DIRECTORY_ROWS: usize = 30_000;
pub const MAX_SENDER_NAMES: usize = 64;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderName {
    pub sender_id: String,
    pub display_name: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCursor {
    pub name: String,
    pub thread_id: String,
}
impl DirectoryCursor {
    pub fn from_record(record: &DirectoryRecord) -> Self {
        Self {
            name: record.display_name.clone(),
            thread_id: record.thread_id(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryRecord {
    pub remote_id: String,
    pub is_group: bool,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub is_friend: Option<bool>,
    pub is_blocked: Option<bool>,
    pub member_count: Option<u32>,
}
impl DirectoryRecord {
    pub fn thread_id(&self) -> String {
        format!(
            "{}:{}",
            if self.is_group { "g" } else { "d" },
            self.remote_id
        )
    }
    fn validate(&self) -> Result<()> {
        if self.remote_id.is_empty()
            || self.remote_id.len() > 24
            || self.remote_id.starts_with('0')
            || !self.remote_id.bytes().all(|b| b.is_ascii_digit())
            || self.display_name.trim().is_empty()
            || self.display_name.len() > 512
            || self.display_name.chars().any(char::is_control)
            || self
                .avatar_url
                .as_ref()
                .is_some_and(|s| s.len() > 2048 || s.chars().any(char::is_control))
            || self.member_count.is_some_and(|n| n > 100_000)
        {
            bail!("Invalid directory record");
        }
        Ok(())
    }
}
pub struct DirectoryBatch {
    pub account_id: String,
    pub entries: Vec<DirectoryRecord>,
    pub messages: Vec<Message>,
    pub message_info: Vec<MessageInfoBinding>,
    pub message_media: Vec<MessageMediaBinding>,
    pub observed_at_ms: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryCommit {
    pub metadata_rows: usize,
    pub inserted_messages: usize,
}
/// Called within MessageStore's existing schema transaction, never a second writer.
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS directory_entries(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,remote_id TEXT NOT NULL,
        is_group INTEGER NOT NULL,display_name TEXT NOT NULL,search_name TEXT NOT NULL,avatar_url TEXT,
        is_friend INTEGER,is_blocked INTEGER,member_count INTEGER,
        observed_at_ms INTEGER NOT NULL,PRIMARY KEY(account_id,thread_id));
        CREATE INDEX IF NOT EXISTS directory_names ON directory_entries(account_id,is_group,display_name,thread_id);
        CREATE INDEX IF NOT EXISTS directory_search ON directory_entries(account_id,is_group,search_name,thread_id);")?;
    Ok(())
}
impl MessageStore {
    /// Resolve only the requested user identities via the account/thread primary
    /// key. Group records with the same remote ID never name a message sender.
    pub fn sender_names(&self, account: &str, sender_ids: &[String]) -> Result<Vec<SenderName>> {
        validate_id(account)?;
        if sender_ids.len() > MAX_SENDER_NAMES {
            bail!("Sender-name request exceeds its resource limit");
        }
        for sender in sender_ids {
            validate_id(sender)?;
        }
        let mut seen = HashSet::with_capacity(sender_ids.len());
        let mut names = Vec::with_capacity(sender_ids.len());
        let mut query=self.conn.prepare_cached("SELECT CASE WHEN length(CAST(display_name AS BLOB))<=512 THEN display_name ELSE NULL END FROM directory_entries WHERE account_id=?1 AND thread_id=?2 AND remote_id=?3 AND is_group=0")?;
        for sender in sender_ids {
            if !seen.insert(sender) {
                continue;
            }
            let name: Option<Option<String>> = query
                .query_row(params![account, format!("d:{sender}"), sender], |row| {
                    row.get(0)
                })
                .optional()?;
            if let Some(display_name) = name
                .flatten()
                .filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control))
            {
                names.push(SenderName {
                    sender_id: sender.clone(),
                    display_name,
                });
            }
        }
        Ok(names)
    }
    /// Metadata and messages commit together. A conflict rolls back both; partial
    /// network snapshots never delete cached contacts, groups, messages or outbox.
    pub fn apply_directory_batch(&mut self, batch: &DirectoryBatch) -> Result<DirectoryCommit> {
        validate_id(&batch.account_id)?;
        if batch.observed_at_ms < 1
            || batch.entries.len() > DIRECTORY_LIMIT
            || batch.messages.len() > MAX_PAGE_SIZE
            || batch.message_info.len() > MAX_PAGE_SIZE
            || batch.message_media.len() > MAX_PAGE_SIZE
        {
            bail!("Directory batch exceeds its resource limit");
        }
        let mut touched = HashSet::new();
        for entry in &batch.entries {
            entry.validate()?;
            if !touched.insert(entry.thread_id()) {
                bail!("Duplicate directory identity in one batch");
            }
        }
        let mut message_keys = HashMap::with_capacity(batch.messages.len());
        for message in &batch.messages {
            message.validate()?;
            if message.account_id != batch.account_id {
                bail!("Directory message belongs to another account");
            }
            touched.insert(message.thread_id.clone());
            message_keys.insert((message.thread_id.as_str(), message.id.as_str()), message);
        }
        let mut infos = HashMap::with_capacity(batch.message_info.len());
        let mut info_bytes = 0usize;
        for info in &batch.message_info {
            validate_id(&info.account_id)?;
            validate_id(&info.thread_id)?;
            validate_id(&info.message_id)?;
            let key = (info.thread_id.as_str(), info.message_id.as_str());
            info_bytes = info_bytes.saturating_add(info.content.as_json().len());
            if info.account_id != batch.account_id
                || info_bytes > MAX_INFO_BATCH_BYTES
                || infos.insert(key, &info.content).is_some()
                || !message_keys
                    .get(&key)
                    .is_some_and(|message| message.sticker.is_none() && message.media.is_none())
            {
                bail!("Invalid directory message metadata binding");
            }
        }
        let mut media_map = HashMap::with_capacity(batch.message_media.len());
        for media in &batch.message_media {
            validate_id(&media.account_id)?;
            validate_id(&media.thread_id)?;
            validate_id(&media.message_id)?;
            let key = (media.thread_id.as_str(), media.message_id.as_str());
            info_bytes = info_bytes.saturating_add(media.content.as_json().len());
            if media.account_id != batch.account_id
                || info_bytes > MAX_INFO_BATCH_BYTES
                || media_map.insert(key, &media.content).is_some()
                || infos.contains_key(&key)
                || !message_keys.get(&key).is_some_and(|message| {
                    message.sticker.is_none()
                        && message.media.as_ref() == Some(media.content.descriptor())
                })
            {
                bail!("Invalid directory media metadata binding");
            }
        }
        let tx = self.conn.transaction()?;
        let count: usize = tx.query_row(
            "SELECT count(*) FROM directory_entries WHERE account_id=?1",
            [&batch.account_id],
            |r| r.get(0),
        )?;
        let mut new_rows = 0;
        for entry in &batch.entries {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM directory_entries WHERE account_id=?1 AND thread_id=?2)",params![batch.account_id,entry.thread_id()],|r|r.get(0))?;
            if !exists {
                new_rows += 1;
            }
        }
        if count + new_rows > MAX_DIRECTORY_ROWS {
            bail!("Directory account cache exceeds its row budget");
        }
        let mut metadata_rows = 0;
        {
            let mut statement = tx.prepare_cached("INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,avatar_url,is_friend,is_blocked,member_count,observed_at_ms,search_name)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                ON CONFLICT(account_id,thread_id) DO UPDATE SET
                display_name=excluded.display_name,search_name=excluded.search_name,avatar_url=COALESCE(excluded.avatar_url,directory_entries.avatar_url),is_friend=COALESCE(excluded.is_friend,directory_entries.is_friend),
                is_blocked=COALESCE(excluded.is_blocked,directory_entries.is_blocked),member_count=COALESCE(excluded.member_count,directory_entries.member_count),observed_at_ms=excluded.observed_at_ms
                WHERE excluded.observed_at_ms>=directory_entries.observed_at_ms")?;
            for entry in &batch.entries {
                metadata_rows += statement.execute(params![
                    batch.account_id,
                    entry.thread_id(),
                    entry.remote_id,
                    entry.is_group,
                    entry.display_name,
                    entry.avatar_url,
                    entry.is_friend,
                    entry.is_blocked,
                    entry.member_count,
                    batch.observed_at_ms,
                    fold_search(&entry.display_name)
                ])?;
            }
        }
        let inserted_messages = insert_validated_with_media(
            &tx,
            batch.messages.iter().map(|message| {
                (
                    message,
                    infos
                        .get(&(message.thread_id.as_str(), message.id.as_str()))
                        .copied(),
                    media_map
                        .get(&(message.thread_id.as_str(), message.id.as_str()))
                        .copied(),
                )
            }),
        )?;
        {
            let mut rename = tx.prepare_cached("UPDATE conversations SET title=(SELECT display_name FROM directory_entries d WHERE d.account_id=?1 AND d.thread_id=?2)
                WHERE account_id=?1 AND thread_id=?2 AND EXISTS(SELECT 1 FROM directory_entries d WHERE d.account_id=?1 AND d.thread_id=?2)")?;
            for thread in touched {
                rename.execute(params![batch.account_id, thread])?;
            }
        }
        tx.commit()?;
        Ok(DirectoryCommit {
            metadata_rows,
            inserted_messages,
        })
    }
    /// Indexed keyset pages. Separate group/direct cursors avoid ID collisions.
    pub fn directory_page(
        &self,
        account: &str,
        is_group: bool,
        query: &str,
        after: Option<&DirectoryCursor>,
        limit: usize,
    ) -> Result<Vec<DirectoryRecord>> {
        validate_id(account)?;
        if limit == 0 || limit > DIRECTORY_LIMIT {
            bail!("Invalid directory page size");
        }
        if query.chars().count() > 128 || query.chars().any(char::is_control) {
            bail!("Invalid directory search");
        }
        let (name, thread) = after
            .map(|c| (c.name.as_str(), c.thread_id.as_str()))
            .unwrap_or(("", ""));
        if name.len() > 512 || thread.len() > 256 {
            bail!("Invalid directory cursor");
        }
        let mut statement = self.conn.prepare_cached("SELECT remote_id,is_group,display_name,avatar_url,is_friend,is_blocked,member_count FROM directory_entries
            WHERE account_id=?1 AND is_group=?2 AND NOT EXISTS(SELECT 1 FROM temp.live_privacy p WHERE p.account_id=directory_entries.account_id AND p.ready=0) AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h WHERE h.account_id=directory_entries.account_id AND h.thread_id=directory_entries.thread_id) AND (display_name,thread_id)>(?3,?4) AND (?5='' OR instr(search_name,?5)>0 OR instr(remote_id,?5)>0)
            ORDER BY display_name,thread_id LIMIT ?6")?;
        let records = statement
            .query_map(
                params![
                    account,
                    is_group,
                    name,
                    thread,
                    fold_search(query),
                    limit as i64
                ],
                |row| {
                    Ok(DirectoryRecord {
                        remote_id: row.get(0)?,
                        is_group: row.get(1)?,
                        display_name: row.get(2)?,
                        avatar_url: row.get(3)?,
                        is_friend: row.get(4)?,
                        is_blocked: row.get(5)?,
                        member_count: row.get(6)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }
    pub fn directory_record(&self, account: &str, thread: &str) -> Result<Option<DirectoryRecord>> {
        validate_id(account)?;
        validate_id(thread)?;
        if !self.live_thread_visible(account, thread)? {
            return Ok(None);
        }
        Ok(self.conn.query_row("SELECT remote_id,is_group,display_name,avatar_url,is_friend,is_blocked,member_count FROM directory_entries WHERE account_id=?1 AND thread_id=?2",params![account,thread],|row|Ok(DirectoryRecord {
            remote_id:row.get(0)?,is_group:row.get(1)?,display_name:row.get(2)?,avatar_url:row.get(3)?,is_friend:row.get(4)?,is_blocked:row.get(5)?,member_count:row.get(6)?,
        })).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sender_names_are_bounded_account_scoped_user_records_not_group_titles() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut user = entry("Nguyễn Hà");
        let mut group = user.clone();
        group.is_group = true;
        group.display_name = "Nhóm có cùng ID".into();
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "11".into(),
                entries: vec![user.clone(), group],
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                observed_at_ms: 1,
            })
            .unwrap();
        user.display_name = "Tên tài khoản khác".into();
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "33".into(),
                entries: vec![user],
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                observed_at_ms: 1,
            })
            .unwrap();
        let ids = vec!["12".into(), "12".into(), "99".into()];
        assert_eq!(
            store.sender_names("11", &ids).unwrap(),
            vec![SenderName {
                sender_id: "12".into(),
                display_name: "Nguyễn Hà".into()
            }]
        );
        assert_eq!(
            store.sender_names("33", &ids).unwrap()[0].display_name,
            "Tên tài khoản khác"
        );
        assert!(store.sender_names("44", &ids).unwrap().is_empty());
        assert!(store
            .sender_names("11", &vec!["12".into(); MAX_SENDER_NAMES + 1])
            .is_err());
        for bad in ["", "\n", &"x".repeat(257)] {
            assert!(store.sender_names("11", &[bad.into()]).is_err());
        }
        let plan:Vec<String>=store.conn.prepare("EXPLAIN QUERY PLAN SELECT display_name FROM directory_entries WHERE account_id=?1 AND thread_id=?2 AND remote_id=?3 AND is_group=0").unwrap().query_map(params!["11","d:12","12"],|row|row.get(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert!(plan
            .iter()
            .any(|row| row.contains("SEARCH") && row.contains("account_id=? AND thread_id=?")));
        assert!(!plan.iter().any(|row| row.contains("SCAN")));
    }
    #[test]
    fn sender_rename_and_restart_leave_message_identity_and_keyset_history_unchanged() {
        let path = std::env::temp_dir().join(format!(
            "zalo-sender-names-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        {
            let mut store = MessageStore::open(&path).unwrap();
            store.apply_directory_batch(&batch("11", "Tên cũ")).unwrap();
            let before = store.page("11", "d:12", None, 10).unwrap();
            assert_eq!(
                store.sender_names("11", &["12".into()]).unwrap()[0].display_name,
                "Tên cũ"
            );
            let mut rename = batch("11", "Tên mới");
            rename.observed_at_ms += 1;
            store.apply_directory_batch(&rename).unwrap();
            assert_eq!(
                store.sender_names("11", &["12".into()]).unwrap()[0].display_name,
                "Tên mới"
            );
            assert_eq!(store.page("11", "d:12", None, 10).unwrap(), before);
            let cursor = before[0].cursor();
            assert!(store
                .page("11", "d:12", Some(&cursor), 10)
                .unwrap()
                .is_empty());
        }
        {
            let store = MessageStore::open(&path).unwrap();
            assert_eq!(
                store.sender_names("11", &["12".into()]).unwrap()[0].display_name,
                "Tên mới"
            );
            assert!(store.sender_names("33", &["12".into()]).unwrap().is_empty());
        }
        std::fs::remove_file(path).unwrap();
    }
    fn entry(name: &str) -> DirectoryRecord {
        DirectoryRecord {
            remote_id: "12".into(),
            is_group: false,
            display_name: name.into(),
            avatar_url: None,
            is_friend: Some(true),
            is_blocked: Some(false),
            member_count: None,
        }
    }
    fn message(account: &str, id: &str, text: &str) -> Message {
        Message {
            account_id: account.into(),
            thread_id: "d:12".into(),
            id: id.into(),
            sender: "12".into(),
            text: text.into(),
            sticker: None,
            media: None,
            sent_at_ms: 100,
        }
    }
    fn batch(account: &str, name: &str) -> DirectoryBatch {
        DirectoryBatch {
            account_id: account.into(),
            entries: vec![entry(name)],
            messages: vec![message(account, "501", "actual synthetic preview")],
            message_info: vec![],
            message_media: vec![],
            observed_at_ms: 1000,
        }
    }

    fn info_batch(name: &str) -> DirectoryBatch {
        let mut batch = batch("11", name);
        let message = &batch.messages[0];
        batch.message_info.push(MessageInfoBinding {
            account_id:message.account_id.clone(),thread_id:message.thread_id.clone(),message_id:message.id.clone(),
            content:crate::domain::InfoMessageContent::new(&serde_json::json!({"action":"msginfo.actionlist","unknown":{"Actions":[{"actionId":7}]},"params":"{\"msg\":{\"vi\":\"Thông tin nhóm\"}}"})).unwrap(),
        });
        batch
    }

    #[test]
    fn directory_info_and_socket_replay_in_both_orders_without_type_conversion() {
        for preview_first in [true, false] {
            let mut store = MessageStore::in_memory().unwrap();
            let preview = info_batch("Retained contact name");
            let m = preview.messages[0].clone();
            let info = preview.message_info[0].content.clone();
            if preview_first {
                assert_eq!(
                    store
                        .apply_directory_batch(&preview)
                        .unwrap()
                        .inserted_messages,
                    1
                );
                assert!(store
                    .receive_batch_with_info(
                        "11",
                        &[(m.clone(), None, Some(info.clone()))],
                        Some(("510_1", "100"))
                    )
                    .unwrap()
                    .is_empty());
            } else {
                assert_eq!(
                    store
                        .receive_batch_with_info(
                            "11",
                            &[(m.clone(), None, Some(info.clone()))],
                            Some(("510_1", "100"))
                        )
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    store
                        .apply_directory_batch(&preview)
                        .unwrap()
                        .inserted_messages,
                    0
                );
            }
            assert_eq!(
                store.message_info("11", &m.thread_id, &m.id).unwrap(),
                Some(info)
            );
            assert_eq!(
                store.page("11", &m.thread_id, None, 10).unwrap(),
                vec![m.clone()]
            );
            let mut conflict = info_batch("Must roll back rename");
            conflict.message_info[0].content=crate::domain::InfoMessageContent::new(&serde_json::json!({"action":"msginfo.actionlist","unknown":{"Actions":[{"actionId":8}]}})).unwrap();
            assert!(store.apply_directory_batch(&conflict).is_err());
            assert_eq!(
                store
                    .directory_record("11", &m.thread_id)
                    .unwrap()
                    .unwrap()
                    .display_name,
                "Retained contact name"
            );
            assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
        }
    }

    #[test]
    fn directory_info_binding_validation_and_sql_failure_preserve_all_rows() {
        let mut store = MessageStore::in_memory().unwrap();
        for invalid in 0..4 {
            let mut preview = info_batch("Must not persist");
            match invalid {
                0 => preview.message_info[0].account_id = "33".into(),
                1 => preview.message_info[0].thread_id = "g:12".into(),
                2 => preview.message_info[0].message_id = "unrelated".into(),
                _ => preview.message_info.push(preview.message_info[0].clone()),
            }
            assert!(store.apply_directory_batch(&preview).is_err());
            assert!(store
                .directory_page("11", false, "", None, 10)
                .unwrap()
                .is_empty());
            assert!(store.conversations("11", 10).unwrap().is_empty());
        }
        store.conn.execute_batch("CREATE TRIGGER reject_directory_info BEFORE INSERT ON message_info BEGIN SELECT RAISE(ABORT,'synthetic info failure'); END;").unwrap();
        let preview = info_batch("Must be atomic");
        assert!(store.apply_directory_batch(&preview).is_err());
        assert!(store
            .directory_page("11", false, "", None, 10)
            .unwrap()
            .is_empty());
        assert!(store.conversations("11", 10).unwrap().is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_directory_info")
            .unwrap();
        assert_eq!(
            store
                .apply_directory_batch(&preview)
                .unwrap()
                .inserted_messages,
            1
        );
    }
    #[test]
    fn scoped_metadata_replays_and_titles_preserve_existing_messages_and_outbox() {
        let mut store = MessageStore::in_memory().unwrap();
        store
            .insert_batch(&[message("11", "500", "existing history")])
            .unwrap();
        let pending = store
            .prepare_outgoing("11", "d:12", "preserved draft", 200)
            .unwrap();
        assert_eq!(
            store
                .apply_directory_batch(&batch("11", "Tên tài khoản A"))
                .unwrap()
                .inserted_messages,
            1
        );
        assert_eq!(
            store
                .apply_directory_batch(&batch("11", "Tên tài khoản A"))
                .unwrap()
                .inserted_messages,
            0
        );
        store
            .apply_directory_batch(&batch("22", "Tên tài khoản B"))
            .unwrap();
        assert_eq!(
            store
                .directory_record("11", "d:12")
                .unwrap()
                .unwrap()
                .display_name,
            "Tên tài khoản A"
        );
        assert_eq!(
            store
                .directory_record("22", "d:12")
                .unwrap()
                .unwrap()
                .display_name,
            "Tên tài khoản B"
        );
        assert_eq!(
            store.conversations("11", 10).unwrap()[0].title,
            "Tên tài khoản A"
        );
        let pending_count:i64=store.conn.query_row("SELECT COUNT(*) FROM outgoing WHERE account_id='11' AND client_id=?1 AND body='preserved draft'",[pending.to_string()],|row|row.get(0)).unwrap();
        assert_eq!(pending_count, 1);
        let history_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE account_id='11'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history_count, 2);
    }
    #[test]
    fn conflicts_roll_back_metadata_and_cross_account_messages_are_rejected() {
        let mut store = MessageStore::in_memory().unwrap();
        store
            .apply_directory_batch(&batch("11", "Original"))
            .unwrap();
        let mut conflicting = batch("11", "Must roll back");
        conflicting.messages[0].text = "conflicting payload".into();
        assert!(store.apply_directory_batch(&conflicting).is_err());
        assert_eq!(
            store
                .directory_record("11", "d:12")
                .unwrap()
                .unwrap()
                .display_name,
            "Original"
        );
        conflicting.messages[0].account_id = "22".into();
        assert!(store.apply_directory_batch(&conflicting).is_err());
        let mut stale = batch("11", "Older metadata");
        stale.observed_at_ms = 999;
        stale.messages.clear();
        assert_eq!(
            store.apply_directory_batch(&stale).unwrap().metadata_rows,
            0
        );
        assert_eq!(
            store
                .directory_record("11", "d:12")
                .unwrap()
                .unwrap()
                .display_name,
            "Original"
        );
    }
    #[test]
    fn directory_keyset_pages_keep_groups_and_direct_ids_separate() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut first = batch("11", "An");
        first.messages.clear();
        store.apply_directory_batch(&first).unwrap();
        first.entries[0].remote_id = "13".into();
        first.entries[0].display_name = "Bình".into();
        store.apply_directory_batch(&first).unwrap();
        first.entries[0].remote_id = "12".into();
        first.entries[0].is_group = true;
        first.entries[0].display_name = "Nhóm".into();
        store.apply_directory_batch(&first).unwrap();
        let page = store.directory_page("11", false, "", None, 1).unwrap();
        assert_eq!(page[0].remote_id, "12");
        let next = store
            .directory_page(
                "11",
                false,
                "",
                Some(&DirectoryCursor::from_record(&page[0])),
                1,
            )
            .unwrap();
        assert_eq!(next[0].remote_id, "13");
        assert_eq!(
            store
                .directory_page("11", true, "", None, 200)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .directory_page("22", false, "", None, 200)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn search_is_literal_vietnamese_normalized_and_covers_more_than_one_page() {
        let mut store = MessageStore::in_memory().unwrap();
        let entries: Vec<_> = (1000..1251)
            .map(|id| {
                let mut e = entry(&format!("User {id}"));
                e.remote_id = id.to_string();
                e
            })
            .collect();
        for chunk in entries.chunks(200) {
            store
                .apply_directory_batch(&DirectoryBatch {
                    account_id: "11".into(),
                    entries: chunk.to_vec(),
                    messages: vec![],
                    message_info: vec![],
                    message_media: vec![],
                    observed_at_ms: 100,
                })
                .unwrap();
        }
        let mut record = entry("Đặng THIẾT KẾ");
        record.remote_id = "9999".into();
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "11".into(),
                entries: vec![record.clone()],
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                observed_at_ms: 101,
            })
            .unwrap();
        for query in ["  DANG thiet ke  ", "Đa\u{0306}ng", "9999"] {
            let found = store.directory_page("11", false, query, None, 100).unwrap();
            assert_eq!(found, vec![record.clone()]);
        }
        assert!(store
            .directory_page("11", false, "%", None, 100)
            .unwrap()
            .is_empty());
        assert!(store
            .directory_page("22", false, "dang", None, 100)
            .unwrap()
            .is_empty());
        let mut partial = record.clone();
        partial.is_friend = None;
        partial.is_blocked = None;
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "11".into(),
                entries: vec![partial],
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                observed_at_ms: 102,
            })
            .unwrap();
        assert_eq!(
            store.directory_record("11", "d:9999").unwrap(),
            Some(record)
        );
    }
    #[test]
    fn version_two_database_migration_preserves_history_and_later_incoming_titles() {
        let mut previous = MessageStore::in_memory().unwrap();
        previous
            .insert_batch(&[message("11", "old", "Preserved v2 history")])
            .unwrap();
        previous
            .conn
            .execute_batch("DROP TABLE directory_entries;PRAGMA user_version=2;")
            .unwrap();
        let mut store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            store.page("11", "d:12", None, 10).unwrap()[0].text,
            "Preserved v2 history"
        );
        let mut metadata = batch("11", "Cached offline name");
        metadata.messages.clear();
        metadata.entries[0].remote_id = "13".into();
        store.apply_directory_batch(&metadata).unwrap();
        let mut later = message("11", "later", "Live after discovery");
        later.thread_id = "d:13".into();
        store.insert_batch(&[later]).unwrap();
        assert!(store
            .conversations("11", 10)
            .unwrap()
            .iter()
            .any(|c| c.id == "d:13" && c.title == "Cached offline name"));
    }
    #[test]
    fn account_row_budget_allows_updates_and_rolls_back_growth_atomically() {
        let mut store = MessageStore::in_memory().unwrap();
        store.conn.execute_batch("WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<30000) INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,search_name,observed_at_ms) SELECT '11','d:'||n,n,0,'Synthetic','synthetic',1 FROM ids;").unwrap();
        let mut update = batch("11", "Updated at capacity");
        update.messages.clear();
        assert_eq!(
            store.apply_directory_batch(&update).unwrap().metadata_rows,
            1
        );
        let mut extra = entry("New over budget");
        extra.remote_id = "30001".into();
        update.entries[0].display_name = "Must rollback".into();
        update.entries.push(extra);
        assert!(store.apply_directory_batch(&update).is_err());
        assert_eq!(
            store
                .directory_record("11", "d:12")
                .unwrap()
                .unwrap()
                .display_name,
            "Updated at capacity"
        );
        assert!(store.directory_record("11", "d:30001").unwrap().is_none());
        assert_eq!(
            store
                .apply_directory_batch(&batch("22", "Separate budget"))
                .unwrap()
                .metadata_rows,
            1
        );
    }
}
