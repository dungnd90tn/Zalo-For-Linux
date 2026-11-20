//! Durable user-initiated group membership changes. No request is replayed.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS group_action_outbox(
            action_id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT NOT NULL, thread_id TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('invite','leave')),
            member_id TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            state TEXT NOT NULL CHECK(state IN (
                'sending','not_sent','invite_accepted','approval_pending',
                'invitation_sent','already_invited','already_member','left','rejected','unknown'
            )),
            CHECK((kind='invite' AND member_id<>'') OR (kind='leave' AND member_id=''))
        );
        CREATE INDEX IF NOT EXISTS group_action_account ON group_action_outbox(account_id,thread_id);
        CREATE UNIQUE INDEX IF NOT EXISTS group_action_unsettled
            ON group_action_outbox(account_id,thread_id,kind,member_id)
            WHERE state IN ('sending','unknown','invite_accepted','approval_pending','invitation_sent','already_invited');
        CREATE TABLE IF NOT EXISTS group_left(
            account_id TEXT NOT NULL, thread_id TEXT NOT NULL,
            verified_at_ms INTEGER NOT NULL,
            PRIMARY KEY(account_id,thread_id)
        );",
    )?;
    Ok(())
}

fn group_id<'a>(account: &str, thread: &'a str) -> Option<&'a str> {
    use crate::domain::reply::valid_source_id;
    thread
        .strip_prefix("g:")
        .filter(|id| valid_source_id(account) && valid_source_id(id))
}

impl MessageStore {
    /// Confirmed membership loss is distinct from absence in a cached directory.
    /// Keep the saved conversation and drafts so it can be read in the future.
    pub fn group_has_left(&self, account: &str, thread: &str) -> Result<bool> {
        left(&self.conn, account, thread)
    }

    pub(crate) fn group_write_blocked(&self, account: &str, thread: &str) -> Result<bool> {
        write_blocked(&self.conn, account, thread)
    }
}

pub(super) fn write_blocked(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
    if group_id(account, thread).is_none() {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM group_left WHERE account_id=?1 AND thread_id=?2)
         OR EXISTS(SELECT 1 FROM group_action_outbox WHERE account_id=?1 AND thread_id=?2
           AND kind='leave' AND state IN ('sending','unknown'))",
        params![account, thread],
        |row| row.get(0),
    )?)
}

pub(super) fn left(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
    if group_id(account, thread).is_none() {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM group_left WHERE account_id=?1 AND thread_id=?2)",
        params![account, thread],
        |row| row.get(0),
    )?)
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::reply::valid_source_id;
    use anyhow::ensure;

    const MAX_UNSETTLED: i64 = 100;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GroupActionOutcome {
        Pending,
        NotSent,
        InviteAccepted,
        ApprovalPending,
        InvitationSent,
        AlreadyInvited,
        AlreadyMember,
        Left,
        Rejected,
        Unknown,
    }

    impl GroupActionOutcome {
        fn storage_name(self) -> &'static str {
            match self {
                Self::Pending => "sending",
                Self::NotSent => "not_sent",
                Self::InviteAccepted => "invite_accepted",
                Self::ApprovalPending => "approval_pending",
                Self::InvitationSent => "invitation_sent",
                Self::AlreadyInvited => "already_invited",
                Self::AlreadyMember => "already_member",
                Self::Left => "left",
                Self::Rejected => "rejected",
                Self::Unknown => "unknown",
            }
        }

        fn from_storage_name(name: &str) -> Result<Self> {
            Ok(match name {
                "sending" => Self::Pending,
                "not_sent" => Self::NotSent,
                "invite_accepted" => Self::InviteAccepted,
                "approval_pending" => Self::ApprovalPending,
                "invitation_sent" => Self::InvitationSent,
                "already_invited" => Self::AlreadyInvited,
                "already_member" => Self::AlreadyMember,
                "left" => Self::Left,
                "rejected" => Self::Rejected,
                "unknown" => Self::Unknown,
                _ => bail!(FailureReason::ConflictingMetadata),
            })
        }

        fn valid_response(self, invite: bool) -> bool {
            match self {
                Self::NotSent | Self::Rejected | Self::Unknown => true,
                Self::InviteAccepted
                | Self::ApprovalPending
                | Self::InvitationSent
                | Self::AlreadyInvited
                | Self::AlreadyMember => invite,
                Self::Left => !invite,
                Self::Pending => false,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct GroupActionIntent {
        /// Local only. The Zalo mutation endpoints do not carry a client ID.
        pub id: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GroupInviteTarget {
        pub contact: String,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum GroupActionFailure {
        Unavailable,
        AlreadyLeft,
        Unsettled,
    }

    impl std::fmt::Display for GroupActionFailure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Unavailable => "The group action is not available",
                Self::AlreadyLeft => "This account has left the group",
                Self::Unsettled => "There is a group action whose outcome is still unsettled",
            })
        }
    }

    impl std::error::Error for GroupActionFailure {}

    fn group_available(conn: &Connection, account: &str, thread: &str) -> Result<()> {
        let remote = group_id(account, thread).ok_or(GroupActionFailure::Unavailable)?;
        ensure!(
            !left(conn, account, thread)?,
            GroupActionFailure::AlreadyLeft
        );
        let available: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)
             AND (
                 EXISTS(SELECT 1 FROM directory_entries WHERE account_id=?1 AND thread_id=?2 AND is_group=1 AND remote_id=?3)
                 OR EXISTS(SELECT 1 FROM conversations WHERE account_id=?1 AND thread_id=?2)
             )",
            params![account,thread,remote], |row| row.get(0),
        )?;
        ensure!(available, GroupActionFailure::Unavailable);
        Ok(())
    }

    fn invitee_available(conn: &Connection, account: &str, member: &str) -> Result<()> {
        ensure!(
            valid_source_id(member) && member != account,
            GroupActionFailure::Unavailable
        );
        let target = format!("d:{member}");
        let available: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM directory_entries
               WHERE account_id=?1 AND thread_id=?2 AND remote_id=?3 AND is_group=0
                 AND is_friend=1 AND COALESCE(is_blocked,0)=0)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)",
            params![account, target, member],
            |row| row.get(0),
        )?;
        ensure!(available, GroupActionFailure::Unavailable);
        Ok(())
    }

    impl MessageStore {
        pub(crate) fn validate_group_scope(&self, account: &str, thread: &str) -> Result<()> {
            group_available(&self.conn, account, thread)
        }

        pub(crate) fn group_invite_targets(
            &self,
            account: &str,
            thread: &str,
            query: &str,
        ) -> Result<Vec<GroupInviteTarget>> {
            group_available(&self.conn, account, thread)?;
            ensure!(
                query.chars().count() <= 128 && !query.chars().any(char::is_control),
                FailureReason::Validation
            );
            let folded = crate::domain::fold_search(query);
            let mut query = self.conn.prepare_cached(
                "SELECT d.remote_id,d.display_name FROM directory_entries d
                 WHERE d.account_id=?1 AND d.is_group=0 AND d.remote_id<>?1
                 AND d.is_friend=1 AND COALESCE(d.is_blocked,0)=0
                 AND (?2='' OR instr(d.search_name,?2)>0)
                 AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h
                    WHERE h.account_id=d.account_id AND h.thread_id=d.thread_id)
                 ORDER BY d.display_name,d.thread_id LIMIT 25",
            )?;
            let rows = query.query_map(params![account, folded], |row| {
                Ok(GroupInviteTarget {
                    contact: row.get(0)?,
                    name: row.get(1)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        }

        pub(crate) fn group_pending_outcome(
            &self,
            account: &str,
            thread: &str,
            member: Option<&str>,
        ) -> Result<Option<GroupActionOutcome>> {
            ensure!(
                group_id(account, thread).is_some(),
                FailureReason::Validation
            );
            let scoped:Option<String>=self.conn.query_row(
                "SELECT state FROM group_action_outbox WHERE account_id=?1 AND thread_id=?2
                 AND kind=?3 AND member_id=?4
                 AND state IN ('sending','unknown','invite_accepted','approval_pending','invitation_sent','already_invited')",
                params![account,thread,if member.is_some(){"invite"}else{"leave"},member.unwrap_or("")],
                |row|row.get(0),
            ).optional()?;
            scoped
                .as_deref()
                .map(GroupActionOutcome::from_storage_name)
                .transpose()
        }

        pub(crate) fn prepare_group_invite(
            &mut self,
            account: &str,
            thread: &str,
            target: &str,
            now_ms: i64,
        ) -> Result<GroupActionIntent> {
            self.prepare_group_action(account, thread, Some(target), now_ms)
        }

        pub(crate) fn prepare_group_leave(
            &mut self,
            account: &str,
            thread: &str,
            now_ms: i64,
        ) -> Result<GroupActionIntent> {
            self.prepare_group_action(account, thread, None, now_ms)
        }

        fn prepare_group_action(
            &mut self,
            account: &str,
            thread: &str,
            member: Option<&str>,
            now_ms: i64,
        ) -> Result<GroupActionIntent> {
            ensure!(now_ms > 0, FailureReason::Validation);
            let tx = self.conn.transaction()?;
            group_available(&tx, account, thread)?;
            if let Some(target) = member {
                invitee_available(&tx, account, target)?;
            }
            let kind = if member.is_some() { "invite" } else { "leave" };
            let member = member.unwrap_or("");
            let (count,blocked):(i64,bool)=tx.query_row(
                "SELECT COUNT(*),COALESCE(MAX(thread_id=?2 AND (
                   (kind=?3 AND member_id=?4)
                   OR state='sending'
                   OR (kind='leave' AND state='unknown')
                 )),0)
                 FROM group_action_outbox o WHERE account_id=?1
                 AND NOT EXISTS(SELECT 1 FROM group_left l WHERE l.account_id=o.account_id AND l.thread_id=o.thread_id)
                 AND state IN ('sending','unknown','invite_accepted','approval_pending','invitation_sent','already_invited')",
                params![account,thread,kind,member],
                |row| Ok((row.get(0)?,row.get(1)?)),
            )?;
            ensure!(
                count < MAX_UNSETTLED && !blocked,
                GroupActionFailure::Unsettled
            );
            tx.execute(
                "INSERT INTO group_action_outbox(account_id,thread_id,kind,member_id,created_at_ms,state)
                 VALUES(?1,?2,?3,?4,?5,'sending')",
                params![account,thread,kind,member,now_ms],
            )?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok(GroupActionIntent { id: id as u64 })
        }

        /// Atomically records the authoritative outcome and any membership loss,
        /// then returns the winning state. An older result cannot undo a left marker.
        pub(crate) fn finish_group_action(
            &mut self,
            account: &str,
            id: u64,
            outcome: GroupActionOutcome,
        ) -> Result<GroupActionOutcome> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let id = i64::try_from(id).map_err(|_| FailureReason::Validation)?;
            let tx = self.conn.transaction()?;
            let (thread,kind,created,state):(String,String,i64,String)=tx.query_row(
                "SELECT thread_id,kind,created_at_ms,state FROM group_action_outbox WHERE account_id=?1 AND action_id=?2",
                params![account,id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            )?;
            ensure!(
                outcome.valid_response(kind == "invite"),
                FailureReason::ConflictingMetadata
            );
            let state = GroupActionOutcome::from_storage_name(&state)?;
            if !matches!(
                state,
                GroupActionOutcome::Pending | GroupActionOutcome::Unknown
            ) {
                return Ok(state);
            }
            tx.execute(
                "UPDATE group_action_outbox SET state=?3 WHERE account_id=?1 AND action_id=?2",
                params![account, id, outcome.storage_name()],
            )?;
            if outcome == GroupActionOutcome::Left {
                tx.execute(
                    "INSERT OR IGNORE INTO group_left(account_id,thread_id,verified_at_ms)
                     VALUES(?1,?2,?3)",
                    params![account, thread, created],
                )?;
                tx.execute(
                    "DELETE FROM message_unread WHERE account_id=?1 AND thread_id=?2",
                    params![account, thread],
                )?;
                tx.execute(
                    "DELETE FROM read_receipt_outbox WHERE account_id=?1 AND thread_id=?2",
                    params![account, thread],
                )?;
            }
            tx.commit()?;
            Ok(outcome)
        }

        pub(crate) fn group_action_outcome(
            &self,
            account: &str,
            id: u64,
        ) -> Result<GroupActionOutcome> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let id = i64::try_from(id).map_err(|_| FailureReason::Validation)?;
            let state: String = self.conn.query_row(
                "SELECT state FROM group_action_outbox WHERE account_id=?1 AND action_id=?2",
                params![account, id],
                |row| row.get(0),
            )?;
            GroupActionOutcome::from_storage_name(&state)
        }

        pub(crate) fn recover_group_actions(&self, account: &str) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE group_action_outbox SET state='unknown' WHERE account_id=?1 AND state='sending'",
                [account],
            )?;
            Ok(())
        }

        /// A positive, account-authenticated roster entry settles only that
        /// exact member. A missing entry never relaxes uncertainty.
        pub(crate) fn confirm_group_member(
            &self,
            account: &str,
            thread: &str,
            member: &str,
        ) -> Result<()> {
            ensure!(
                group_id(account, thread).is_some() && valid_source_id(member) && member != account,
                FailureReason::Validation
            );
            self.conn.execute(
                "UPDATE group_action_outbox SET state='already_member'
                   WHERE account_id=?1 AND thread_id=?2 AND kind='invite' AND member_id=?3
                   AND state IN ('unknown','invite_accepted','approval_pending','invitation_sent','already_invited')",
                params![account,thread,member],
            )?;
            Ok(())
        }
    }
}

#[cfg(feature = "messaging")]
pub(crate) use live::GroupActionFailure;
#[cfg(feature = "messaging")]
pub use live::{GroupActionOutcome, GroupInviteTarget};

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;

    fn fixture() -> MessageStore {
        let mut store = MessageStore::in_memory().unwrap();
        store.conn.execute_batch(
            "INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,search_name,is_friend,is_blocked,observed_at_ms)
             VALUES ('11','g:22','22',1,'Nhóm tổng hợp','nhom tong hop',NULL,NULL,1),
                    ('33','g:22','22',1,'Nhóm tài khoản khác','nhom tai khoan khac',NULL,NULL,1),
                    ('11','d:44','44',0,'Bạn thử nghiệm','ban thu nghiem',1,0,1),
                    ('11','d:55','55',0,'Bạn đã chặn','ban da chan',1,1,1),
                    ('11','d:66','66',0,'Liên hệ chưa rõ','lien he chua ro',NULL,0,1);",
        ).unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        store.replace_live_privacy("33", &[], None).unwrap();
        store
    }

    #[test]
    fn invitation_recovery_waits_for_positive_member_proof() {
        let mut store = fixture();
        assert!(store.prepare_group_invite("11", "g:22", "55", 11).is_err());
        assert!(store.prepare_group_invite("11", "g:22", "66", 11).is_err());
        assert!(store.prepare_group_invite("33", "g:22", "44", 11).is_err());
        let invite = store.prepare_group_invite("11", "g:22", "44", 11).unwrap();
        assert!(store.group_action_outcome("33", invite.id).is_err());
        assert!(store
            .finish_group_action("11", invite.id, GroupActionOutcome::Left)
            .is_err());
        store.recover_group_actions("11").unwrap();
        assert_eq!(
            store.group_action_outcome("11", invite.id).unwrap(),
            GroupActionOutcome::Unknown
        );
        assert!(store.prepare_group_invite("11", "g:22", "44", 12).is_err());
        store.confirm_group_member("11", "g:22", "55").unwrap();
        assert!(store.prepare_group_invite("11", "g:22", "44", 12).is_err());
        store.confirm_group_member("11", "g:22", "44").unwrap();
        assert_eq!(
            store.group_action_outcome("11", invite.id).unwrap(),
            GroupActionOutcome::AlreadyMember
        );
        assert_eq!(
            store
                .finish_group_action("11", invite.id, GroupActionOutcome::Unknown)
                .unwrap(),
            GroupActionOutcome::AlreadyMember
        );
    }

    #[test]
    fn left_state_is_atomic_scoped_and_keeps_existing_history() {
        let mut store = fixture();
        let original = Message {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            id: "60".into(),
            sender: "55".into(),
            text: "Lịch sử tổng hợp trước khi rời nhóm".into(),
            source_client_id: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
            sent_at_ms: 10,
        };
        store.insert_batch(std::slice::from_ref(&original)).unwrap();
        let leave = store.prepare_group_leave("11", "g:22", 11).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_group_left BEFORE INSERT ON group_left
             BEGIN SELECT RAISE(ABORT,'synthetic failed commit'); END;",
            )
            .unwrap();
        assert!(store
            .finish_group_action("11", leave.id, GroupActionOutcome::Left)
            .is_err());
        assert_eq!(
            store.group_action_outcome("11", leave.id).unwrap(),
            GroupActionOutcome::Pending
        );
        assert!(!store.group_has_left("11", "g:22").unwrap());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_group_left")
            .unwrap();
        store.recover_group_actions("11").unwrap();
        assert!(store.prepare_group_leave("11", "g:22", 12).is_err());
        assert_eq!(
            store
                .finish_group_action("11", leave.id, GroupActionOutcome::Left)
                .unwrap(),
            GroupActionOutcome::Left
        );
        assert!(store.group_has_left("11", "g:22").unwrap());
        assert!(!store.group_has_left("33", "g:22").unwrap());
        assert!(store.prepare_group_invite("11", "g:22", "44", 13).is_err());
        let stored = store.page("11", "g:22", None, 10).unwrap();
        assert_eq!(stored, [original]);
        assert!(store.conversations("11", 10).unwrap()[0].group_left);
        assert!(store
            .directory_page("11", true, "", None, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .directory_page("33", true, "", None, 10)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .prepare_outgoing("11", "g:22", "Bản nháp cục bộ", 14)
            .is_err());
        let stale = crate::store::directory::DirectoryBatch {
            account_id: "11".into(),
            entries: vec![crate::store::directory::DirectoryRecord {
                remote_id: "22".into(),
                is_group: true,
                display_name: "Thành viên cũ từ máy chủ".into(),
                avatar_url: None,
                is_friend: None,
                is_blocked: None,
                member_count: Some(23),
            }],
            messages: vec![],
            message_info: vec![],
            message_media: vec![],
            message_quotes: vec![],
            observed_at_ms: 1000,
        };
        assert_eq!(
            store.apply_directory_batch(&stale).unwrap().metadata_rows,
            0
        );
        assert!(store.directory_record("11", "g:22").unwrap().is_none());
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.replace_live_privacy("11", &[], None).unwrap();
        assert!(reopened.group_has_left("11", "g:22").unwrap());
        assert_eq!(
            reopened.conversations("11", 10).unwrap()[0].title,
            "Nhóm tổng hợp"
        );
    }
    #[test]
    fn group_picker_and_safe_not_sent_join_the_same_scope() {
        let mut store = fixture();
        let targets = store.group_invite_targets("11", "g:22", "Ban thu").unwrap();
        assert_eq!(
            targets,
            [GroupInviteTarget {
                contact: "44".into(),
                name: "Bạn thử nghiệm".into()
            }]
        );
        assert!(store
            .group_invite_targets("33", "g:22", "")
            .unwrap()
            .is_empty());
        store
            .replace_live_privacy("11", &["d:44".into()], Some("d:44"))
            .unwrap();
        assert!(store
            .group_invite_targets("11", "g:22", "")
            .unwrap()
            .is_empty());
        store.replace_live_privacy("11", &[], None).unwrap();

        let leave = store.prepare_group_leave("11", "g:22", 11).unwrap();
        assert!(store.group_write_blocked("11", "g:22").unwrap());
        assert_eq!(
            store.group_pending_outcome("11", "g:22", None).unwrap(),
            Some(GroupActionOutcome::Pending)
        );
        assert!(store
            .prepare_outgoing("11", "g:22", "Còn trong nhóm", 12)
            .is_err());
        store
            .finish_group_action("11", leave.id, GroupActionOutcome::NotSent)
            .unwrap();
        assert!(!store.group_write_blocked("11", "g:22").unwrap());
        assert!(store
            .prepare_outgoing("11", "g:22", "Còn trong nhóm", 13)
            .is_ok());
        assert!(store.prepare_group_leave("11", "g:22", 14).is_ok());
    }
}
