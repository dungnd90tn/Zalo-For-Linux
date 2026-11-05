//! Immutable full info content committed with its message and replay cursor.
use super::FailureReason;
use crate::domain::{InfoMessageContent, Message, MAX_INFO_CONTENT_BYTES};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_info(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind='webchat.info'),
        metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=4194304),
        PRIMARY KEY(account_id,thread_id,message_id),
        FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);")?;
    Ok(())
}

pub(super) fn load(
    conn: &Connection,
    account: &str,
    thread: &str,
    id: &str,
) -> Result<Option<InfoMessageContent>> {
    load_raw(conn, account, thread, id)?
        .map(|value| {
            let decoded =
                serde_json::from_str(&value).map_err(|_| FailureReason::ConflictingMetadata)?;
            InfoMessageContent::new(&decoded).map_err(|_| FailureReason::ConflictingMetadata.into())
        })
        .transpose()
}

fn load_raw(conn: &Connection, account: &str, thread: &str, id: &str) -> Result<Option<String>> {
    // Check bytes in SQLite before allocating a stored value. Corrupt metadata
    // is an error, never silently mistaken for an absent/plain message.
    let value: Option<Option<String>> = conn.query_row(
        "SELECT CASE WHEN kind='webchat.info' AND length(CAST(metadata AS BLOB))<=?4 THEN metadata END
         FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        params![account, thread, id, MAX_INFO_CONTENT_BYTES as i64],
        |row| row.get(0),
    ).optional()?;
    value
        .map(|value| value.ok_or_else(|| FailureReason::ConflictingMetadata.into()))
        .transpose()
}

pub(super) fn persist(
    conn: &Connection,
    message: &Message,
    info: Option<&InfoMessageContent>,
    existing_message: bool,
) -> Result<()> {
    if info.is_none() {
        let occupied:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",params![message.account_id,message.thread_id,message.id],|row|row.get(0))?;
        ensure!(!occupied, FailureReason::ConflictingMetadata);
        return Ok(());
    }
    let previous = load_raw(conn, &message.account_id, &message.thread_id, &message.id)?;
    match (previous.as_ref(), info) {
        (Some(old), Some(new)) => ensure!(old == new.as_json(), FailureReason::ConflictingMetadata),
        (None, None) => {}
        (None, Some(info)) => {
            // Existing text/sticker messages cannot change type under the same
            // server identity even if their presentation happens to be equal.
            ensure!(
                !existing_message && message.sticker.is_none(),
                FailureReason::ConflictingMetadata
            );
            conn.execute("INSERT INTO message_info(account_id,thread_id,message_id,kind,metadata) VALUES(?1,?2,?3,'webchat.info',?4)",
                params![message.account_id,message.thread_id,message.id,info.as_json()])?;
        }
        (Some(_), None) => return Err(FailureReason::ConflictingMetadata.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{MAX_INFO_BATCH_BYTES, MAX_INFO_DEPTH},
        store::MessageStore,
    };
    use serde_json::json;

    fn message(account: &str, thread: &str, id: &str) -> Message {
        Message {
            account_id: account.into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "22".into(),
            text: "Minh đã đổi thông tin nhóm".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1000,
        }
    }
    fn info(action_id: u64) -> InfoMessageContent {
        InfoMessageContent::new(&json!({
            "action":"msginfo.actionlist", "title":"Thông tin nhóm",
            "params":"{\"msg\":{\"vi\":\"%1$s đã đổi thông tin nhóm\"}}",
            "unknown":{"Actions":[{"actionId":action_id,"data":{"nested":[null,true,7]}}]}
        }))
        .unwrap()
    }
    #[test]
    fn full_info_survives_restart_scoping_replay_and_cascade_delete() {
        let path = std::env::temp_dir().join(format!(
            "zalo-info-store-{}.db",
            crate::domain::local_message_id().unwrap()
        ));
        let a = message("11", "g:22", "same");
        let b = message("33", "g:22", "same");
        let c = message("11", "d:22", "same");
        let content = info(1);
        {
            let mut store = MessageStore::open(&path).unwrap();
            for m in [&a, &b, &c] {
                assert_eq!(
                    store
                        .receive_batch_with_info(
                            &m.account_id,
                            &[(m.clone(), None, Some(content.clone()))],
                            Some(("511_1", "100"))
                        )
                        .unwrap()
                        .len(),
                    1
                );
            }
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            for m in [&a, &b, &c] {
                assert_eq!(
                    store
                        .message_info(&m.account_id, &m.thread_id, &m.id)
                        .unwrap(),
                    Some(content.clone())
                );
                assert_eq!(
                    store.page(&m.account_id, &m.thread_id, None, 1).unwrap(),
                    vec![m.clone()]
                );
                assert!(store
                    .receive_batch_with_info(
                        &m.account_id,
                        &[(m.clone(), None, Some(content.clone()))],
                        Some(("511_1", "101"))
                    )
                    .unwrap()
                    .is_empty());
            }
            assert!(store.delete("11", "g:22", "same").unwrap());
            assert!(store.message_info("11", "g:22", "same").unwrap().is_none());
            assert_eq!(
                store.message_info("33", "g:22", "same").unwrap(),
                Some(content.clone())
            );
            assert_eq!(
                store.message_info("11", "d:22", "same").unwrap(),
                Some(content)
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn identical_preview_cannot_hide_changed_payload_or_plain_type_replay() {
        let mut store = MessageStore::in_memory().unwrap();
        let original = message("11", "g:22", "one");
        store
            .receive_batch_with_info(
                "11",
                &[(original.clone(), None, Some(info(1)))],
                Some(("511_1", "100")),
            )
            .unwrap();
        let next = message("11", "g:22", "two");
        let error = store
            .receive_batch_with_info(
                "11",
                &[
                    (next.clone(), None, Some(info(2))),
                    (original.clone(), None, Some(info(3))),
                ],
                Some(("511_1", "200")),
            )
            .unwrap_err();
        assert_eq!(
            super::super::classify_failure(&error),
            FailureReason::ConflictingMetadata
        );
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "100");
        assert_eq!(
            store.page("11", "g:22", None, 10).unwrap(),
            vec![original.clone()]
        );
        assert!(store
            .receive_batch("11", &[(original, None)], Some(("511_1", "300")))
            .is_err());
        store
            .receive_batch("11", &[(next.clone(), None)], None)
            .unwrap();
        assert!(store
            .receive_batch_with_info("11", &[(next, None, Some(info(2)))], Some(("511_1", "300")))
            .is_err());
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "100");
    }

    #[test]
    fn metadata_failure_rolls_back_message_conversation_cursor_and_retries() {
        let mut store = MessageStore::in_memory().unwrap();
        let m = message("11", "g:22", "one");
        store.conn.execute_batch("CREATE TRIGGER reject_info BEFORE INSERT ON message_info BEGIN SELECT RAISE(ABORT,'synthetic info failure'); END;").unwrap();
        assert!(store
            .receive_batch_with_info(
                "11",
                &[(m.clone(), None, Some(info(1)))],
                Some(("511_1", "100"))
            )
            .is_err());
        assert!(store.page("11", "g:22", None, 10).unwrap().is_empty());
        assert!(store.conversations("11", 10).unwrap().is_empty());
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "1");
        store
            .conn
            .execute_batch("DROP TRIGGER reject_info")
            .unwrap();
        assert_eq!(
            store
                .receive_batch_with_info("11", &[(m, None, Some(info(1)))], Some(("511_1", "100")))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rich_info_never_acks_plain_outgoing_even_when_projection_and_client_match() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut m = message("11", "d:22", "server");
        m.sender = "11".into();
        let client = store.prepare_outgoing("11", "d:22", &m.text, 1000).unwrap();
        let before: (String, Option<String>, String) = store
            .conn
            .query_row(
                "SELECT state,server_id,body FROM outgoing WHERE account_id='11'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            store
                .receive_batch_with_info(
                    "11",
                    &[(m.clone(), Some(client.to_string()), Some(info(1)))],
                    Some(("510_1", "100"))
                )
                .unwrap()
                .len(),
            1
        );
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![m]);
        assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
        let after: (String, Option<String>, String) = store
            .conn
            .query_row(
                "SELECT state,server_id,body FROM outgoing WHERE account_id='11'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn byte_depth_and_aggregate_budgets_fail_without_advancing_cursor() {
        assert!(InfoMessageContent::new(
            &json!({"action":"msginfo.actionlist","x":"x".repeat(MAX_INFO_CONTENT_BYTES)})
        )
        .is_err());
        let mut nested = json!(null);
        for _ in 0..MAX_INFO_DEPTH + 1 {
            nested = json!([nested]);
        }
        assert!(
            InfoMessageContent::new(&json!({"action":"msginfo.actionlist","x":nested})).is_err()
        );
        let content = InfoMessageContent::new(
            &json!({"action":"msginfo.actionlist","x":"x".repeat(MAX_INFO_BATCH_BYTES/2)}),
        )
        .unwrap();
        let mut store = MessageStore::in_memory().unwrap();
        assert!(store
            .receive_batch_with_info(
                "11",
                &[
                    (message("11", "g:22", "1"), None, Some(content.clone())),
                    (message("11", "g:22", "2"), None, Some(content))
                ],
                Some(("511_1", "100"))
            )
            .is_err());
        assert!(store.page("11", "g:22", None, 10).unwrap().is_empty());
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "1");
    }

    #[test]
    fn deepest_accepted_info_roundtrips_through_persisted_decoder() {
        let mut nested = json!(null);
        for _ in 0..MAX_INFO_DEPTH - 2 {
            nested = json!([nested]);
        }
        let content =
            InfoMessageContent::new(&json!({"action":"msginfo.actionlist","x":nested.clone()}))
                .unwrap();
        let mut store = MessageStore::in_memory().unwrap();
        store
            .receive_batch_with_info(
                "11",
                &[(message("11", "g:22", "one"), None, Some(content.clone()))],
                None,
            )
            .unwrap();
        assert_eq!(
            store.message_info("11", "g:22", "one").unwrap(),
            Some(content)
        );
        let too_deep = json!({"action":"msginfo.actionlist","x":[nested]});
        assert!(InfoMessageContent::new(&too_deep).is_err());
    }

    #[test]
    fn v4_additive_upgrade_preserves_stickers_outbox_history_and_cursor() {
        let mut previous = MessageStore::in_memory().unwrap();
        let m = message("11", "g:22", "old");
        previous
            .receive_batch("11", &[(m.clone(), None)], Some(("511_1", "99")))
            .unwrap();
        let client = previous
            .prepare_outgoing("11", "d:22", "retained draft", 1001)
            .unwrap();
        let mut sticker = message("11", "d:22", "sticker");
        sticker.text = crate::domain::LEGACY_STICKER_PLACEHOLDER.into();
        sticker.sticker = Some(crate::domain::StickerDescriptor {
            sticker_id: 7,
            category_id: 8,
            sticker_type: 1,
        });
        previous.insert_batch(&[sticker.clone()]).unwrap();
        previous
            .conn
            .execute_batch("DROP TABLE message_info;PRAGMA user_version=4;")
            .unwrap();
        let mut store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(store.page("11", "g:22", None, 10).unwrap(), vec![m]);
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![sticker]);
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "99");
        let body: String = store
            .conn
            .query_row(
                "SELECT body FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(body, "retained draft");
        assert_eq!(
            store
                .receive_batch_with_info(
                    "11",
                    &[(message("11", "g:22", "new"), None, Some(info(1)))],
                    Some(("511_1", "100"))
                )
                .unwrap()
                .len(),
            1
        );
    }
}
