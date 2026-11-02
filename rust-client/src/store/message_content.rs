//! Bounded typed metadata stored with the same message/cursor transaction.
use super::FailureReason;
use crate::domain::{Message, StickerDescriptor, LEGACY_STICKER_PLACEHOLDER};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};
const MAX_METADATA_BYTES: usize = 256;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_content(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind='sticker'),metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=256),
        PRIMARY KEY(account_id,thread_id,message_id),
        FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);")?;
    Ok(())
}
pub(super) fn decode(value: Option<String>) -> Option<StickerDescriptor> {
    value
        .filter(|value| value.len() <= MAX_METADATA_BYTES)
        .and_then(|value| serde_json::from_str::<StickerDescriptor>(&value).ok())
        .filter(|sticker| sticker.validate().is_ok())
}
pub(super) fn load(conn: &Connection, message: &Message) -> Result<Option<StickerDescriptor>> {
    let mut statement=conn.prepare_cached("SELECT metadata FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='sticker' AND length(CAST(metadata AS BLOB))<=256")?;
    let value: Option<String> = statement
        .query_row(
            params![message.account_id, message.thread_id, message.id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(decode(value))
}
/// Legacy unsupported-sticker rows can gain their verified catalog address;
/// their text/sender/timestamp are unchanged. Existing metadata is immutable.
pub(super) fn persist(
    conn: &Connection,
    message: &Message,
    existing_message: bool,
) -> Result<bool> {
    if !existing_message && message.sticker.is_none() {
        return Ok(false);
    }
    let previous = load(conn, message)?;
    if existing_message && previous.is_none() && message.sticker.is_some() {
        // A malformed/unknown sidecar is different from an absent sidecar.
        // Preserve its bytes and reject the enrichment with a closed reason,
        // rather than attempting INSERT and reporting a misleading SQLite fault.
        let occupied: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",params![message.account_id,message.thread_id,message.id],|row|row.get(0))?;
        ensure!(!occupied, FailureReason::ConflictingMetadata);
    }
    match (&previous, &message.sticker) {
        (Some(old), Some(new)) => {
            ensure!(old == new, FailureReason::ConflictingMetadata);
            Ok(false)
        }
        (Some(_), None) => {
            ensure!(
                message.text == LEGACY_STICKER_PLACEHOLDER,
                FailureReason::ConflictingMetadata
            );
            Ok(false)
        }
        (None, Some(sticker)) => {
            sticker.validate()?;
            ensure!(
                !existing_message || message.text == LEGACY_STICKER_PLACEHOLDER,
                "Cannot enrich an ordinary text message as a sticker"
            );
            let metadata = serde_json::to_string(sticker)?;
            ensure!(
                metadata.len() <= MAX_METADATA_BYTES,
                "Sticker metadata exceeds budget"
            );
            conn.execute("INSERT INTO message_content(account_id,thread_id,message_id,kind,metadata) VALUES(?1,?2,?3,'sticker',?4)",params![message.account_id,message.thread_id,message.id,metadata])?;
            Ok(true)
        }
        (None, None) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{MAX_STICKER_ID, STICKER_PREVIEW},
        store::MessageStore,
    };
    fn descriptor(id: u64) -> StickerDescriptor {
        StickerDescriptor {
            sticker_id: id,
            category_id: 7,
            sticker_type: 3,
        }
    }
    fn message(id: &str, sticker: Option<StickerDescriptor>) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: id.into(),
            sender: "22".into(),
            text: LEGACY_STICKER_PLACEHOLDER.into(),
            sticker,
            sent_at_ms: 1000,
        }
    }
    #[test]
    fn legacy_placeholder_enriches_once_and_conflicting_descriptor_rolls_back_batch_cursor() {
        let mut store = MessageStore::in_memory().unwrap();
        let old = message("sticker", None);
        store
            .receive_batch("11", &[(old.clone(), None)], Some(("q", "100")))
            .unwrap();
        let rich = message("sticker", Some(descriptor(9)));
        let published = store
            .receive_batch("11", &[(rich.clone(), None)], Some(("q", "200")))
            .unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].1.last_text, STICKER_PREVIEW);
        assert_eq!(
            store.page("11", "d:22", None, 10).unwrap(),
            vec![rich.clone()]
        );
        assert!(store
            .receive_batch("11", &[(rich.clone(), None)], Some(("q", "250")))
            .unwrap()
            .is_empty());
        assert!(store
            .receive_batch("11", &[(old, None)], Some(("q", "260")))
            .unwrap()
            .is_empty());
        assert_eq!(
            store.page("11", "d:22", None, 10).unwrap()[0].sticker,
            Some(descriptor(9))
        );
        let conflicting = message("sticker", Some(descriptor(10)));
        let mut next = message("next", None);
        next.text = "new text".into();
        assert!(store
            .receive_batch(
                "11",
                &[(next, None), (conflicting, None)],
                Some(("q", "300"))
            )
            .is_err());
        assert_eq!(store.receive_cursor("11", "q").unwrap(), "260");
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![rich]);
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM messages", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn enrichment_never_rewrites_text_sender_timestamp_or_unknown_placeholder() {
        for original in ["actual text", "[Tin nhắn chat.photo chưa được hỗ trợ]"] {
            let mut store = MessageStore::in_memory().unwrap();
            let mut old = message("id", None);
            old.text = original.into();
            store.insert_batch(std::slice::from_ref(&old)).unwrap();
            let mut rich = old.clone();
            rich.sticker = Some(descriptor(1));
            assert!(store.insert_batch(&[rich]).is_err());
            assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![old]);
        }
        let mut store = MessageStore::in_memory().unwrap();
        let old = message("id", None);
        store.insert_batch(std::slice::from_ref(&old)).unwrap();
        let mut rich = message("id", Some(descriptor(1)));
        rich.sent_at_ms += 1;
        assert!(store.insert_batch(&[rich]).is_err());
        let mut rich = message("id", Some(descriptor(1)));
        rich.sender = "33".into();
        assert!(store.insert_batch(&[rich]).is_err());
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![old]);
    }
    #[test]
    fn metadata_write_failure_rolls_back_message_and_cursor_then_retries_without_duplication() {
        let mut store = MessageStore::in_memory().unwrap();
        let rich = message("id", Some(descriptor(1)));
        store.conn.execute_batch("CREATE TRIGGER reject_sticker BEFORE INSERT ON message_content BEGIN SELECT RAISE(ABORT,'synthetic metadata failure'); END;").unwrap();
        assert!(store
            .receive_batch("11", &[(rich.clone(), None)], Some(("q", "100")))
            .is_err());
        assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
        assert_eq!(store.receive_cursor("11", "q").unwrap(), "1");
        assert!(store.conversations("11", 10).unwrap().is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_sticker")
            .unwrap();
        assert_eq!(
            store
                .receive_batch("11", &[(rich, None)], Some(("q", "100")))
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn restart_keyset_and_delete_preserve_scoped_metadata_without_loading_other_messages() {
        let path = std::env::temp_dir().join(format!(
            "zalo-sticker-store-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        {
            let mut store = MessageStore::open(&path).unwrap();
            let a = message("same", Some(descriptor(1)));
            let mut b = message("same", Some(descriptor(2)));
            b.thread_id = "g:22".into();
            let mut c = message("same", Some(descriptor(3)));
            c.account_id = "33".into();
            let last = message("z-last", Some(descriptor(4)));
            store.insert_batch(&[a, b, c, last]).unwrap();
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            let latest = store.page("11", "d:22", None, 1).unwrap();
            assert_eq!(latest[0].sticker, Some(descriptor(4)));
            let older = store
                .page("11", "d:22", Some(&latest[0].cursor()), 1)
                .unwrap();
            assert_eq!(older[0].sticker, Some(descriptor(1)));
            assert_eq!(
                store.page("11", "g:22", None, 1).unwrap()[0].sticker,
                Some(descriptor(2))
            );
            assert_eq!(
                store.page("33", "d:22", None, 1).unwrap()[0].sticker,
                Some(descriptor(3))
            );
            assert!(store.delete("11", "d:22", "same").unwrap());
            assert_eq!(
                store
                    .conn
                    .query_row("SELECT count(*) FROM message_content", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                3
            );
            assert_eq!(
                store.page("33", "d:22", None, 1).unwrap()[0].sticker,
                Some(descriptor(3))
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_stored_metadata_degrades_one_message_and_ids_obey_numeric_limits() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut text = message("text", None);
        text.text = "retained text".into();
        store
            .insert_batch(&[message("sticker", Some(descriptor(0))), text])
            .unwrap();
        store
            .conn
            .execute("UPDATE message_content SET metadata='{}'", [])
            .unwrap();
        let page = store.page("11", "d:22", None, 10).unwrap();
        assert_eq!(page.len(), 2);
        assert!(page.iter().all(|message| message.sticker.is_none()));
        assert!(page.iter().any(|message| message.text == "retained text"));
        let error = store
            .receive_batch(
                "11",
                &[(message("sticker", Some(descriptor(0))), None)],
                Some(("q", "100")),
            )
            .unwrap_err();
        assert_eq!(
            super::super::classify_failure(&error),
            FailureReason::ConflictingMetadata
        );
        assert_eq!(store.receive_cursor("11", "q").unwrap(), "1");
        let stored: String = store
            .conn
            .query_row("SELECT metadata FROM message_content", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, "{}");
        let mut invalid = message("invalid", Some(descriptor(MAX_STICKER_ID + 1)));
        assert!(store.insert_batch(std::slice::from_ref(&invalid)).is_err());
        invalid.sticker.as_mut().unwrap().sticker_id = 1;
        invalid.sticker.as_mut().unwrap().sticker_type = 256;
        assert!(store.insert_batch(&[invalid]).is_err());
        assert_eq!(store.page("11", "d:22", None, 10).unwrap().len(), 2);
    }
}
