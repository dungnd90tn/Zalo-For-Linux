//! Typed, one-shot outgoing voice intents. Audio stays in the compose session.
use super::*;
use crate::domain::MediaDescriptor;
use anyhow::ensure;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS voice_outbox(
            account_id TEXT NOT NULL, client_id INTEGER NOT NULL,
            thread_id TEXT NOT NULL, checksum BLOB NOT NULL CHECK(length(checksum)=32),
            state TEXT NOT NULL CHECK(state IN ('prepared','sending','unknown','accepted','rejected')),
            server_id TEXT, PRIMARY KEY(account_id,client_id),
            CHECK((state='accepted')=(server_id IS NOT NULL)));
         CREATE UNIQUE INDEX IF NOT EXISTS voice_active_account
            ON voice_outbox(account_id) WHERE state IN ('prepared','sending');
         CREATE UNIQUE INDEX IF NOT EXISTS voice_server_id
            ON voice_outbox(account_id,thread_id,server_id) WHERE server_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS voice_unresolved
            ON voice_outbox(account_id,thread_id,checksum) WHERE state='unknown';",
    )?;
    Ok(())
}

fn settle(conn: &Connection, account: &str, client: i64, server: &str) -> Result<()> {
    ensure!(
        conn.execute(
            "UPDATE voice_outbox SET state='accepted',server_id=?3
             WHERE account_id=?1 AND client_id=?2
               AND state IN ('sending','unknown','accepted')
               AND (server_id IS NULL OR server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM forward_outbox
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM message_forward
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND message_id=?3 AND kind='forwarded')
               AND NOT EXISTS(SELECT 1 FROM attachment_outbox
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM photo_outbox
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM sticker_outbox
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM outgoing
                   WHERE account_id=?1 AND thread_id=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM e2ee_outbox
                   WHERE account=?1 AND thread=voice_outbox.thread_id AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM messages m
                   LEFT JOIN message_media p USING(account_id,thread_id,message_id)
                   WHERE m.account_id=?1 AND m.thread_id=voice_outbox.thread_id
                     AND m.message_id=?3 AND (m.sender<>?1 OR COALESCE(p.kind,'')<>'chat.voice'))",
            params![account, client, server],
        )? == 1,
        FailureReason::ConflictingEcho,
    );
    Ok(())
}

pub(super) fn reconcile_received(
    conn: &Connection,
    message: &mut Message,
    hint: Option<&str>,
    content: &MediaMessageContent,
) -> Result<bool> {
    if message.sender != message.account_id
        || !matches!(content.descriptor(), MediaDescriptor::Voice(voice) if voice.url.is_some())
    {
        return Ok(false);
    }
    let client = hint.and_then(|hint| {
        let client = hint.parse::<i64>().ok()?;
        (client > 0 && client.to_string() == hint).then_some(client)
    });
    let mut query = conn.prepare_cached(
        "SELECT client_id,thread_id,server_id,state FROM voice_outbox
         WHERE account_id=?1 AND (client_id=?2 OR (thread_id=?3 AND server_id=?4))
         LIMIT 2",
    )?;
    let mut rows = query.query(params![
        message.account_id,
        client,
        message.thread_id,
        message.id
    ])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let client: i64 = row.get(0)?;
    let thread: String = row.get(1)?;
    let server: Option<String> = row.get(2)?;
    let state: String = row.get(3)?;
    ensure!(
        rows.next()?.is_none()
            && thread == message.thread_id
            && matches!(state.as_str(), "sending" | "unknown" | "accepted")
            && server.is_none_or(|server| server == message.id),
        FailureReason::ConflictingEcho,
    );
    settle(conn, &message.account_id, client, &message.id)?;
    message.source_client_id = Some(client.to_string());
    message.reply_blocked = true;
    Ok(state != "accepted")
}

#[cfg(any(feature = "messaging", test))]
impl MessageStore {
    pub(crate) fn reserve_voice(
        &mut self,
        account: &str,
        thread: &str,
        checksum: [u8; 32],
        now_ms: i64,
    ) -> Result<i64> {
        super::attachments::plain_visible(self, account, thread)?;
        ensure!(now_ms > 0, FailureReason::Validation);
        let tx = self.conn.transaction()?;
        ensure!(
            !super::attachments::upload_active(&tx, account)?,
            "An upload is already active"
        );
        let (count, duplicate): (i64, i64) = tx.query_row(
            "SELECT count(*),COALESCE(max(thread_id=?2 AND checksum=?3),0)
             FROM voice_outbox WHERE account_id=?1 AND state='unknown'",
            params![account, thread, checksum.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        ensure!(
            count < 100 && duplicate == 0,
            "Voice send has an unresolved intent"
        );
        let previous: Option<i64> = tx
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        let client = now_ms.max(
            previous
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(FailureReason::Validation)?,
        );
        ensure!(client <= 9_007_199_254_740_991, FailureReason::Validation);
        tx.execute(
            "INSERT INTO outgoing_counters VALUES(?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
            params![account, client],
        )?;
        tx.execute(
            "INSERT INTO voice_outbox(account_id,client_id,thread_id,checksum,state)
             VALUES(?1,?2,?3,?4,'prepared')",
            params![account, client, thread, checksum.as_slice()],
        )?;
        tx.commit()?;
        Ok(client)
    }

    pub(crate) fn begin_voice_dispatch(
        &self,
        account: &str,
        thread: &str,
        client: i64,
    ) -> Result<()> {
        super::attachments::plain_visible(self, account, thread)?;
        ensure!(
            self.conn.execute(
                "UPDATE voice_outbox SET state='sending'
                 WHERE account_id=?1 AND thread_id=?2 AND client_id=?3 AND state='prepared'",
                params![account, thread, client],
            )? == 1,
            FailureReason::ConflictingMetadata,
        );
        Ok(())
    }

    pub(crate) fn record_voice_ack(&self, account: &str, client: i64, server: &str) -> Result<()> {
        validate_id(account)?;
        validate_id(server)?;
        settle(&self.conn, account, client, server)
    }

    /// Returns true if the corresponding typed own echo was already committed.
    pub(crate) fn finish_voice_failure(
        &self,
        account: &str,
        client: i64,
        unknown: bool,
    ) -> Result<bool> {
        validate_id(account)?;
        let affected = self.conn.execute(
            "UPDATE voice_outbox SET state=?3
             WHERE account_id=?1 AND client_id=?2 AND state IN ('prepared','sending')",
            params![
                account,
                client,
                if unknown { "unknown" } else { "rejected" }
            ],
        )?;
        if affected != 0 {
            return Ok(false);
        }
        Ok(self.conn.query_row(
            "SELECT state='accepted' FROM voice_outbox WHERE account_id=?1 AND client_id=?2",
            params![account, client],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn recover_voices(&self, account: &str) -> Result<()> {
        validate_id(account)?;
        self.conn.execute(
            "UPDATE voice_outbox SET state=CASE WHEN state='sending' THEN 'unknown' ELSE 'rejected' END
             WHERE account_id=?1 AND state IN ('prepared','sending')",
            [account],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restart_blocks_unknown_voice_replay_but_releases_unsent_reservations() {
        let mut store = MessageStore::in_memory().unwrap();
        let id = store.reserve_voice("11", "d:22", [1; 32], 1000).unwrap();
        assert!(store.begin_voice_dispatch("11", "d:33", id).is_err());
        store.recover_voices("11").unwrap();
        let next = store.reserve_voice("11", "d:22", [1; 32], 1000).unwrap();
        assert!(next > id);
        store.begin_voice_dispatch("11", "d:22", next).unwrap();
        assert!(store.begin_voice_dispatch("11", "d:22", next).is_err());
        store.recover_voices("11").unwrap();
        assert!(store.reserve_voice("11", "d:22", [1; 32], 1000).is_err());
        let other = store.reserve_voice("11", "d:33", [1; 32], 1000).unwrap();
        assert!(!store.finish_voice_failure("11", other, false).unwrap());
    }

    #[test]
    fn dispatch_rechecks_privacy_and_encryption_before_claiming_voice_bytes() {
        for hidden in [true, false] {
            let mut store = MessageStore::in_memory().unwrap();
            let id = store.reserve_voice("11", "d:22", [1; 32], 1000).unwrap();
            if hidden {
                store
                    .replace_live_privacy("11", &["d:22".into()], None)
                    .unwrap();
            } else {
                store
                    .conn
                    .execute("INSERT INTO e2ee_threads VALUES('11','d:22')", [])
                    .unwrap();
            }
            assert!(store.begin_voice_dispatch("11", "d:22", id).is_err());
            assert!(store.record_voice_ack("11", id, "501").is_err());
        }
    }

    #[test]
    fn typed_voice_echo_settles_unknown_only_in_its_own_scope() {
        let mut store = MessageStore::in_memory().unwrap();
        let id = store.reserve_voice("11", "d:22", [1; 32], 1000).unwrap();
        store.begin_voice_dispatch("11", "d:22", id).unwrap();
        assert!(!store.finish_voice_failure("11", id, true).unwrap());
        let content = MediaMessageContent::new(
            MediaKind::Voice,
            &json!({"href":"https://f15-zpaudio.zdn.vn/fixtures/voice.mp3"}),
        )
        .unwrap();
        let mut message = Message {
            expires_at_ms: None,
            account_id: "11".into(),
            thread_id: "d:33".into(),
            id: "501".into(),
            sender: "11".into(),
            text: MediaKind::Voice.legacy_placeholder().into(),
            sent_at_ms: 1200,
            quote: None,
            sticker: None,
            media: Some(content.descriptor().clone()),
            source_client_id: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
        };
        assert!(reconcile_received(&store.conn, &mut message, Some("1000"), &content).is_err());
        assert!(store.reserve_voice("11", "d:22", [1; 32], 1000).is_err());
        message.thread_id = "d:22".into();
        message.sender = "22".into();
        assert!(!reconcile_received(&store.conn, &mut message, Some("1000"), &content).unwrap());
        message.sender = "11".into();
        assert!(reconcile_received(&store.conn, &mut message, Some("1000"), &content).unwrap());
        assert_eq!(message.source_client_id.as_deref(), Some("1000"));
        assert!(message.reply_blocked);
        assert!(store.finish_voice_failure("11", id, true).unwrap());
        assert!(store.record_voice_ack("11", id, "502").is_err());
        store.record_voice_ack("11", id, "501").unwrap();
    }

    #[test]
    fn voice_ack_cannot_claim_an_unmaterialized_plain_or_encrypted_receipt() {
        let mut store = MessageStore::in_memory().unwrap();
        let id = store.reserve_voice("11", "d:22", [1; 32], 1000).unwrap();
        store.begin_voice_dispatch("11", "d:22", id).unwrap();
        store.conn.execute("INSERT INTO outgoing VALUES('11','d:22','10','Synthetic',100,'acknowledged','501')", []).unwrap();
        store.conn.execute("INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,server_id)
           VALUES('11',20,'d:22',1,1,'Synthetic',100,'acknowledged','502')", []).unwrap();
        assert!(store.record_voice_ack("11", id, "501").is_err());
        assert!(store.record_voice_ack("11", id, "502").is_err());
        assert!(store.record_voice_ack("11", id, "503").is_ok());
    }
}
