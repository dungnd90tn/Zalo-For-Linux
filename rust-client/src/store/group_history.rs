//! Scoped cloud history adds older projections without advancing live delivery.
use super::*;
use crate::protocol::messaging::{numeric_id, IncomingBatch, Target, ThreadKind};
use anyhow::ensure;
use rusqlite::TransactionBehavior;
use std::collections::HashSet;

fn verify_anchor(conn: &Connection, account: &str, thread: &str, anchor: &Cursor) -> Result<()> {
    let target = Target::from_thread(thread).map_err(|_| FailureReason::Validation)?;
    ensure!(
        target.kind == ThreadKind::Group
            && numeric_id(account).is_ok()
            && numeric_id(&anchor.id).is_ok()
            && anchor.sent_at_ms > 0
            && !group_actions::write_blocked(conn, account, thread)?,
        FailureReason::Validation
    );
    let oldest: Option<(String, i64)> = conn
        .query_row(
            "SELECT m.message_id,m.sent_at_ms FROM messages m
             WHERE m.account_id=?1 AND m.thread_id=?2
             AND EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)
             ORDER BY m.sent_at_ms ASC,m.message_id ASC LIMIT 1",
            params![account, thread],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    ensure!(
        oldest
            .as_ref()
            .is_some_and(|(id, time)| id == &anchor.id && *time == anchor.sent_at_ms),
        FailureReason::Validation
    );
    Ok(())
}

impl MessageStore {
    pub(crate) fn group_history_anchor(
        &self,
        account: &str,
        thread: &str,
        anchor: &Cursor,
    ) -> Result<()> {
        verify_anchor(&self.conn, account, thread, anchor)
    }

    /// The caller holds the matching confirmed privacy revision throughout the
    /// database job. Recheck its installed store scope and the original anchor
    /// inside the transaction before any row from the fetched page can commit.
    pub(crate) fn ingest_group_history(
        &mut self,
        account: &str,
        thread: &str,
        anchor: &Cursor,
        batch: IncomingBatch,
    ) -> Result<(usize, Vec<delete_for_me::DeleteForMeChange>)> {
        ensure!(
            batch.messages.len() <= MAX_PAGE_SIZE
                && batch.recalls.is_empty()
                && batch.deletes.is_empty()
                && batch.reactions.is_empty(),
            FailureReason::Validation
        );
        let mut ids = HashSet::with_capacity(batch.messages.len());
        let mut sidecar_bytes = 0usize;
        for row in &batch.messages {
            let message = &row.message;
            message.validate().map_err(|_| FailureReason::Validation)?;
            ensure!(
                !row.unsupported
                    && !message.is_recalled()
                    && message.account_id == account
                    && message.thread_id == thread
                    && numeric_id(&message.id).is_ok()
                    && numeric_id(&message.sender).is_ok()
                    && message.sent_at_ms > 0
                    && (message.id.len(), message.id.as_str())
                        < (anchor.id.len(), anchor.id.as_str())
                    && (message.sent_at_ms, message.id.as_str())
                        < (anchor.sent_at_ms, anchor.id.as_str())
                    && ids.insert(message.id.as_str()),
                FailureReason::Validation
            );
            sidecar_bytes = sidecar_bytes
                .saturating_add(row.info.as_ref().map_or(0, |info| info.as_json().len()))
                .saturating_add(row.media.as_ref().map_or(0, |media| media.as_json().len()))
                .saturating_add(
                    row.quote_content
                        .as_ref()
                        .map_or(0, |quote| quote.as_json().len()),
                );
            ensure!(
                sidecar_bytes <= MAX_INFO_BATCH_BYTES,
                FailureReason::Validation
            );
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_anchor(&tx, account, thread, anchor)?;
        let mut deleted_changes = Vec::new();
        if !batch.messages.is_empty() && delete_for_me::has_pending(&tx, account)? {
            for row in &batch.messages {
                if row.info.is_none()
                    && row.media.is_none()
                    && row.quote_content.is_none()
                    && delete_for_me::ordinary_replay(&row.message, row.forward.as_ref())
                {
                    deleted_changes.extend(delete_for_me::confirm_pending(
                        &tx,
                        &row.message,
                        false,
                    )?);
                }
            }
        }
        let mut inserted = 0;
        for row in &batch.messages {
            let source = &row.message;
            if delete_for_me::suppresses(&tx, source)? {
                continue;
            }
            if structured_webchat_conflicts_with_send_echo(&tx, source, row.forward.as_ref())? {
                continue;
            }
            let plain = row.info.is_none()
                && row.media.is_none()
                && row.quote_content.is_none()
                && source.quote.is_none()
                && source.reply_reference().is_some()
                && !matches!(
                    row.forward,
                    Some(ForwardMarker::Parsed(_) | ForwardMarker::Unavailable)
                );
            let redacted = recall::history_redacted_copy(&tx, source, plain)?;
            let message = redacted.as_ref().unwrap_or(source);
            let recalled = redacted.is_some();
            inserted += insert_validated_with_media(
                &tx,
                std::iter::once((
                    message,
                    row.info.as_ref().filter(|_| !recalled),
                    row.media.as_ref().filter(|_| !recalled),
                )),
            )?;
            if let Some(marker) = row.forward.as_ref().filter(|_| !recalled) {
                forward::persist(&tx, message, marker)?;
            }
            message_reply::persist(
                &tx,
                message,
                row.quote_content.as_ref().filter(|_| !recalled),
            )?;
            if row.list_silent {
                tx.execute(
                    "INSERT OR IGNORE INTO list_silent_messages(account_id,thread_id,message_id)
                     VALUES(?1,?2,?3)",
                    params![account, thread, message.id],
                )?;
            }
        }
        if batch.messages.iter().any(|row| row.list_silent) {
            tx.execute(
                "UPDATE conversations SET last_message_at_ms=COALESCE((
                    SELECT MAX(m.sent_at_ms) FROM messages m
                    WHERE m.account_id=?1 AND m.thread_id=?2
                    AND NOT EXISTS(SELECT 1 FROM list_silent_messages q
                        WHERE q.account_id=m.account_id AND q.thread_id=m.thread_id
                        AND q.message_id=m.message_id)),0)
                 WHERE account_id=?1 AND thread_id=?2",
                params![account, thread],
            )?;
        }
        delete_for_me::refresh_changes(&tx, account, &mut deleted_changes)?;
        tx.commit()?;
        Ok((inserted, deleted_changes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::read_state::ReceiptReference, protocol::messaging::Incoming};
    use serde_json::json;

    fn row(id: &str, time: i64, sender: &str, text: &str) -> Incoming {
        Incoming {
            forward: Some(ForwardMarker::Ordinary),
            receipt: None,
            message: Message {
                account_id: "11".into(),
                thread_id: "g:77".into(),
                id: id.into(),
                sender: sender.into(),
                text: text.into(),
                source_client_id: Some("101".into()),
                mentions: Vec::new(),
                reply_blocked: false,
                recalled: false,
                quote: None,
                sticker: None,
                media: None,
                sent_at_ms: time,
            },
            client_id: None,
            info: None,
            media: None,
            quote_content: None,
            unsupported: false,
            list_silent: false,
        }
    }

    fn page(rows: Vec<Incoming>) -> IncomingBatch {
        IncomingBatch {
            messages: rows,
            ..IncomingBatch::default()
        }
    }

    fn saved_store() -> (MessageStore, Cursor) {
        let mut store = MessageStore::in_memory().unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let anchor = row("10", 100, "22", "Tin mới").message;
        let cursor = Cursor {
            sent_at_ms: anchor.sent_at_ms,
            id: anchor.id.clone(),
        };
        store
            .receive_batch("11", &[(anchor, None)], Some(("511_0", "201")))
            .unwrap();
        (store, cursor)
    }

    #[test]
    fn remote_history_requires_current_one_group_anchor_and_confirmed_normal_privacy() {
        let (mut store, anchor) = saved_store();
        store.group_history_anchor("11", "g:77", &anchor).unwrap();
        let changed = Cursor {
            sent_at_ms: 101,
            ..anchor.clone()
        };
        assert!(store.group_history_anchor("11", "g:77", &changed).is_err());
        assert!(store.group_history_anchor("11", "d:77", &anchor).is_err());
        assert!(store.group_history_anchor("12", "g:77", &anchor).is_err());
        store
            .replace_live_privacy("11", &["g:77".into()], Some("g:77"))
            .unwrap();
        assert!(store
            .ingest_group_history("11", "g:77", &anchor, page(vec![row("7", 70, "22", "Ẩn")]))
            .is_err());
        store.replace_live_privacy("11", &[], None).unwrap();
        store
            .conn
            .execute("INSERT INTO e2ee_threads VALUES('11','g:77')", [])
            .unwrap();
        assert!(store.group_history_anchor("11", "g:77", &anchor).is_err());
        store.conn.execute("DELETE FROM e2ee_threads", []).unwrap();
        store
            .conn
            .execute("INSERT INTO group_left VALUES('11','g:77',101)", [])
            .unwrap();
        assert!(store.group_history_anchor("11", "g:77", &anchor).is_err());
        assert_eq!(store.page("11", "g:77", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn remote_history_ingests_scoped_sidecars_and_tombstones_without_advancing_live_state() {
        let (mut store, anchor) = saved_store();
        let outgoing = store
            .prepare_outgoing("11", "g:77", "Nội dung tự gửi", 120)
            .unwrap();
        store.record_outgoing_ack("11", outgoing, "9").unwrap();
        let mut own = row("9", 90, "11", "Nội dung tự gửi");
        own.message.source_client_id = Some(outgoing.to_string());
        own.client_id = Some(outgoing.to_string());
        let mut quoted = row("8", 80, "33", "Tin tìm kiếm");
        quoted.quote_content = Some(
            QuoteContent::new(
                "11",
                &json!({
                    "ownerId":"22","globalMsgId":"7","cliMsgId":"101","cliMsgType":1,
                    "ts":70,"msg":"Nội dung đã thu hồi"
                }),
            )
            .unwrap(),
        );
        quoted.receipt = Some(ReceiptBinding {
            account_id: "11".into(),
            thread_id: "g:77".into(),
            message_id: "8".into(),
            reference: ReceiptReference {
                message_id: "8".into(),
                client_id: Some("101".into()),
                sender_id: "33".into(),
                recipient_id: "77".into(),
                message_type: "webchat".into(),
                st: 3,
                at: 0,
                command: 521,
                sent_at_ms: 80,
            },
            counts_unread: true,
        });
        store
            .conn
            .execute(
                "INSERT INTO recall_tombstones VALUES('11','g:77','7','22','101',90,100,0)",
                [],
            )
            .unwrap();
        let recalled = row("7", 70, "22", "Nội dung đã thu hồi");
        assert_eq!(
            store
                .ingest_group_history("11", "g:77", &anchor, page(vec![quoted, own, recalled]))
                .unwrap()
                .0,
            3
        );

        let rows = store.page("11", "g:77", Some(&anchor), 10).unwrap();
        assert!(rows[0].is_recalled());
        assert_eq!(
            rows[1].quote.as_ref().unwrap().text,
            crate::domain::RECALLED_TEXT
        );
        assert_eq!(rows[2].text, "Nội dung tự gửi");
        assert_eq!(store.receive_cursor("11", "511_0").unwrap(), "201");
        assert_eq!(store.thread_read_state("11", "g:77").unwrap().unread, 0);
        assert!(store
            .message_delivery("11", "g:77", &["8".into()])
            .unwrap()
            .is_empty());
        let state: String = store
            .conn
            .query_row(
                "SELECT state FROM outgoing WHERE account_id='11' AND client_id=?1",
                [outgoing.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "acknowledged");
        assert!(store
            .search_messages("11", "g:77", "Nội dung đã thu hồi", 5)
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(
            store
                .search_messages("11", "g:77", "tìm kiếm", 5)
                .unwrap()
                .hits[0]
                .message_id,
            "8"
        );
        assert_eq!(
            store.conversations("11", 5).unwrap()[0].last_text,
            "Tin mới"
        );
        assert!(store.group_history_anchor("11", "g:77", &anchor).is_err());
    }

    #[test]
    fn remote_history_rejects_cross_scope_bounds_and_rolls_back_entire_page() {
        let (mut store, anchor) = saved_store();
        let mut wrong = row("8", 80, "22", "Sai nhóm");
        wrong.message.thread_id = "g:78".into();
        for invalid in [
            wrong,
            row("11", 80, "22", "Sai ID"),
            row("8", 101, "22", "Sai mốc"),
        ] {
            assert!(store
                .ingest_group_history(
                    "11",
                    "g:77",
                    &anchor,
                    page(vec![row("7", 70, "22", "Không được ghi"), invalid,])
                )
                .is_err());
        }
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER fail_history BEFORE INSERT ON messages WHEN NEW.message_id='8'
             BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            )
            .unwrap();
        assert!(store
            .ingest_group_history(
                "11",
                "g:77",
                &anchor,
                page(vec![
                    row("7", 70, "22", "Không được ghi"),
                    row("8", 80, "22", "Lỗi ghi"),
                ])
            )
            .is_err());
        assert!(store
            .page("11", "g:77", Some(&anchor), 10)
            .unwrap()
            .is_empty());
        assert_eq!(store.receive_cursor("11", "511_0").unwrap(), "201");
    }
}
