//! Full immutable media content and a separately bounded presentation snapshot.
use super::FailureReason;
use crate::domain::{
    MediaDescriptor, MediaKind, MediaMessageContent, Message, MAX_INFO_CONTENT_BYTES,
    MAX_MEDIA_DESCRIPTOR_BYTES, MAX_PREVIEW_CHARS,
};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};

const MAX_PROJECTION_BYTES: usize = MAX_MEDIA_DESCRIPTOR_BYTES;
const MAX_PREVIEW_BYTES: usize = 4 * MAX_PREVIEW_CHARS;

pub(super) fn initialize(conn: &Connection, version: i64) -> Result<()> {
    // Rebuild only the v6 sidecar CHECK constraint, inside the caller's transaction.
    // INSERT...SELECT retains the original immutable JSON and presentation bytes.
    if version == 6 {
        conn.execute_batch("ALTER TABLE message_media RENAME TO message_media_v6;")?;
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_media(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('chat.photo','chat.webcontent','chat.recommended','group.poll')),
        metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=4194304),
        projection TEXT NOT NULL CHECK(length(CAST(projection AS BLOB))<=16384),
        preview TEXT NOT NULL CHECK(length(CAST(preview AS BLOB))<=1024),
        PRIMARY KEY(account_id,thread_id,message_id),
        FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);")?;
    if version == 6 {
        conn.execute_batch("INSERT INTO message_media(account_id,thread_id,message_id,kind,metadata,projection,preview)
            SELECT account_id,thread_id,message_id,kind,metadata,projection,preview FROM message_media_v6;
            DROP TABLE message_media_v6;")?;
    }
    Ok(())
}

pub(super) fn exists(conn: &Connection, message: &Message) -> Result<bool> {
    Ok(conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)")?.query_row(params![message.account_id,message.thread_id,message.id],|row|row.get(0))?)
}

fn preview(descriptor: &MediaDescriptor) -> String {
    descriptor
        .preview_text()
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect()
}

pub(super) fn projection(conn: &Connection, message: &Message) -> Result<Option<MediaDescriptor>> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .prepare_cached(
            "SELECT kind,
        CASE WHEN length(CAST(projection AS BLOB))<=?4 THEN projection END,
        CASE WHEN length(CAST(preview AS BLOB))<=?5 THEN preview END
        FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?
        .query_row(
            params![
                message.account_id,
                message.thread_id,
                message.id,
                MAX_PROJECTION_BYTES as i64,
                MAX_PREVIEW_BYTES as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    row.map(|(kind, projection, stored_preview)| {
        let projection = projection.ok_or(FailureReason::ConflictingMetadata)?;
        let descriptor: MediaDescriptor =
            serde_json::from_str(&projection).map_err(|_| FailureReason::ConflictingMetadata)?;
        descriptor
            .validate()
            .map_err(|_| FailureReason::ConflictingMetadata)?;
        ensure!(
            kind == descriptor.kind().as_str()
                && stored_preview.as_deref() == Some(preview(&descriptor).as_str())
                && message.sticker.is_none()
                && message.text == format!("[Tin nhắn {kind} chưa được hỗ trợ]"),
            FailureReason::ConflictingMetadata
        );
        Ok(descriptor)
    })
    .transpose()
}

type Stored = (String, Option<String>, Option<String>, Option<String>);
fn raw(conn: &Connection, account: &str, thread: &str, id: &str) -> Result<Option<Stored>> {
    Ok(conn
        .prepare_cached(
            "SELECT kind,
        CASE WHEN length(CAST(metadata AS BLOB))<=?4 THEN metadata END,
        CASE WHEN length(CAST(projection AS BLOB))<=?5 THEN projection END,
        CASE WHEN length(CAST(preview AS BLOB))<=?6 THEN preview END
        FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        )?
        .query_row(
            params![
                account,
                thread,
                id,
                MAX_INFO_CONTENT_BYTES as i64,
                MAX_PROJECTION_BYTES as i64,
                MAX_PREVIEW_BYTES as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?)
}

pub(super) fn load(
    conn: &Connection,
    account: &str,
    thread: &str,
    id: &str,
) -> Result<Option<MediaMessageContent>> {
    raw(conn, account, thread, id)?
        .map(|(kind, metadata, projection, stored_preview)| {
            let kind = MediaKind::from_wire(&kind).ok_or(FailureReason::ConflictingMetadata)?;
            let metadata = metadata.ok_or(FailureReason::ConflictingMetadata)?;
            let value =
                serde_json::from_str(&metadata).map_err(|_| FailureReason::ConflictingMetadata)?;
            let content = MediaMessageContent::new(kind, &value)
                .map_err(|_| FailureReason::ConflictingMetadata)?;
            let expected = serde_json::to_string(content.descriptor())
                .map_err(|_| FailureReason::ConflictingMetadata)?;
            ensure!(
                metadata == content.as_json()
                    && projection.as_deref() == Some(expected.as_str())
                    && stored_preview.as_deref() == Some(preview(content.descriptor()).as_str()),
                FailureReason::ConflictingMetadata
            );
            Ok(content)
        })
        .transpose()
}

pub(super) fn persist(
    conn: &Connection,
    message: &Message,
    content: Option<&MediaMessageContent>,
    existing_message: bool,
) -> Result<()> {
    let Some(content) = content else {
        ensure!(
            message.media.is_none() && !exists(conn, message)?,
            FailureReason::ConflictingMetadata
        );
        return Ok(());
    };
    content
        .descriptor()
        .validate()
        .map_err(|_| FailureReason::Validation)?;
    let kind = content.kind().as_str();
    ensure!(
        message.media.as_ref() == Some(content.descriptor())
            && message.sticker.is_none()
            && message.text == format!("[Tin nhắn {kind} chưa được hỗ trợ]"),
        FailureReason::ConflictingMetadata
    );
    let projection =
        serde_json::to_string(content.descriptor()).map_err(|_| FailureReason::Validation)?;
    ensure!(
        projection.len() <= MAX_PROJECTION_BYTES,
        FailureReason::Validation
    );
    let preview = preview(content.descriptor());
    if let Some((old_kind, metadata, old_projection, old_preview)) =
        raw(conn, &message.account_id, &message.thread_id, &message.id)?
    {
        ensure!(
            old_kind == kind
                && metadata.as_deref() == Some(content.as_json())
                && old_projection.as_deref() == Some(projection.as_str())
                && old_preview.as_deref() == Some(preview.as_str()),
            FailureReason::ConflictingMetadata
        );
        return Ok(());
    }
    if existing_message {
        // The caller already checked the exact sender/body/timestamp tuple.
        // An unknown or malformed older rich sidecar still blocks conversion.
        let occupied:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3) OR EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",params![message.account_id,message.thread_id,message.id],|r|r.get(0))?;
        ensure!(!occupied, FailureReason::ConflictingMetadata);
    }
    conn.execute("INSERT INTO message_media(account_id,thread_id,message_id,kind,metadata,projection,preview) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![message.account_id,message.thread_id,message.id,kind,content.as_json(),projection,preview])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::MessageMediaBinding,
        store::{
            directory::{DirectoryBatch, DirectoryRecord},
            MessageStore,
        },
    };
    use serde_json::json;

    fn media_content(kind: MediaKind, revision: u64) -> MediaMessageContent {
        MediaMessageContent::new(kind,&json!({
            "title":"Ảnh tổng hợp", "description":"Nội dung nguồn", "href":"https://example.com/article",
            "thumb":"https://photo-talk.zadn.vn/synthetic.jpg",
            "params":"{\"width\":640,\"height\":480}",
            "unknown":{"revision":revision,"values":[null,true,{"retain":"all"}]}
        })).unwrap()
    }
    fn message(kind: MediaKind, id: &str) -> (Message, MediaMessageContent) {
        let content = media_content(kind, 1);
        (
            Message {
                account_id: "11".into(),
                thread_id: "d:22".into(),
                id: id.into(),
                sender: "22".into(),
                text: format!("[Tin nhắn {} chưa được hỗ trợ]", kind.as_str()),
                sticker: None,
                media: Some(content.descriptor().clone()),
                sent_at_ms: 1000,
            },
            content,
        )
    }
    fn binding(message: &Message, content: MediaMessageContent) -> MessageMediaBinding {
        MessageMediaBinding {
            account_id: message.account_id.clone(),
            thread_id: message.thread_id.clone(),
            message_id: message.id.clone(),
            content,
        }
    }
    #[test]
    fn legacy_media_enrichment_preserves_body_and_publishes_typed_history_and_preview() {
        for kind in [
            MediaKind::Photo,
            MediaKind::WebContent,
            MediaKind::Recommended,
            MediaKind::Poll,
        ] {
            let mut store = MessageStore::in_memory().unwrap();
            let (m, content) = message(kind, "one");
            let mut legacy = m.clone();
            legacy.media = None;
            store
                .receive_batch("11", &[(legacy.clone(), None)], Some(("510_1", "100")))
                .unwrap();
            let received = store
                .receive_batch_with_media(
                    "11",
                    &[(m.clone(), None, None, Some(content.clone()))],
                    Some(("510_1", "101")),
                )
                .unwrap();
            assert_eq!(received.len(), 1);
            assert_eq!(received[0].0, m);
            assert_eq!(
                received[0].1.last_text,
                m.preview_text()
                    .chars()
                    .take(MAX_PREVIEW_CHARS)
                    .collect::<String>()
            );
            assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![m.clone()]);
            assert_eq!(
                store.message_media("11", "d:22", "one").unwrap(),
                Some(content.clone())
            );
            let stored: (String, String, i64) = store
                .conn
                .query_row("SELECT body,sender,sent_at_ms FROM messages", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .unwrap();
            assert_eq!(stored, (legacy.text, legacy.sender, legacy.sent_at_ms));
            assert!(store
                .receive_batch_with_media(
                    "11",
                    &[(m, None, None, Some(content))],
                    Some(("510_1", "102"))
                )
                .unwrap()
                .is_empty());
        }
    }
    #[test]
    fn same_projection_different_full_payload_or_projection_cannot_overwrite_history() {
        let mut store = MessageStore::in_memory().unwrap();
        let (m, original) = message(MediaKind::Photo, "one");
        store
            .receive_batch_with_media(
                "11",
                &[(m.clone(), None, None, Some(original.clone()))],
                Some(("510_1", "100")),
            )
            .unwrap();
        let changed = media_content(MediaKind::Photo, 2);
        assert_eq!(original.descriptor(), changed.descriptor());
        let (next, next_content) = message(MediaKind::WebContent, "two");
        assert!(store
            .receive_batch_with_media(
                "11",
                &[
                    (next, None, None, Some(next_content)),
                    (m.clone(), None, None, Some(changed))
                ],
                Some(("510_1", "200"))
            )
            .is_err());
        assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![m.clone()]);
        let mut mismatch = m.clone();
        mismatch.media = Some(media_content(MediaKind::WebContent, 1).descriptor().clone());
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(mismatch, None, None, Some(original.clone()))],
                None
            )
            .is_err());
        let mut plain = m;
        plain.media = None;
        assert!(store.receive_batch("11", &[(plain, None)], None).is_err());
        assert_eq!(
            store.message_media("11", "d:22", "one").unwrap(),
            Some(original)
        );
    }
    #[test]
    fn recommended_and_poll_preserve_full_raw_and_never_ack_plaintext_or_accept_conflicts() {
        for kind in [MediaKind::Recommended, MediaKind::Poll] {
            let mut store = MessageStore::in_memory().unwrap();
            let (mut m, raw) = message(kind, "rich");
            m.sender = "11".into();
            let client = store.prepare_outgoing("11", "d:22", &m.text, 1000).unwrap();
            store
                .receive_batch_with_media(
                    "11",
                    &[(m.clone(), Some(client.to_string()), None, Some(raw.clone()))],
                    Some(("510_1", "100")),
                )
                .unwrap();
            let outgoing = || {
                store
                    .conn
                    .query_row("SELECT state,server_id,body FROM outgoing", [], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .unwrap()
            };
            let (state, server, body) = outgoing();
            assert_eq!(state, "sending");
            assert_eq!(server, None);
            assert_eq!(body, m.text);
            let changed = media_content(kind, 2);
            assert_eq!(raw.descriptor(), changed.descriptor());
            assert_ne!(raw.as_json(), changed.as_json());
            assert!(store
                .receive_batch_with_media(
                    "11",
                    &[(m.clone(), None, None, Some(changed))],
                    Some(("510_1", "101"))
                )
                .is_err());
            assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
            assert_eq!(
                store.message_media("11", "d:22", "rich").unwrap(),
                Some(raw)
            );
            assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![m]);
        }
    }
    #[test]
    fn enrichment_never_changes_ordinary_text_sender_timestamp_or_other_sidecars() {
        for change in 0..4 {
            let mut store = MessageStore::in_memory().unwrap();
            let (m, content) = message(MediaKind::Photo, "one");
            let mut previous = m.clone();
            previous.media = None;
            match change {
                0 => previous.text = "ordinary text".into(),
                1 => previous.sender = "33".into(),
                2 => previous.sent_at_ms += 1,
                _ => {}
            }
            store.insert_batch(&[previous.clone()]).unwrap();
            if change == 3 {
                store
                    .conn
                    .execute(
                        "INSERT INTO message_content VALUES('11','d:22','one','sticker','{}')",
                        [],
                    )
                    .unwrap();
            }
            assert!(store
                .receive_batch_with_media(
                    "11",
                    &[(m, None, None, Some(content))],
                    Some(("510_1", "100"))
                )
                .is_err());
            assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "1");
            assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![previous]);
        }
    }
    #[test]
    fn media_sql_failure_rolls_back_whole_batch_and_matching_self_media_never_acks_outbox() {
        let mut store = MessageStore::in_memory().unwrap();
        let (mut m, content) = message(MediaKind::Photo, "one");
        m.sender = "11".into();
        let client = store.prepare_outgoing("11", "d:22", &m.text, 1000).unwrap();
        let before: (String, Option<String>, String) = store
            .conn
            .query_row("SELECT state,server_id,body FROM outgoing", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        store.conn.execute_batch("CREATE TRIGGER reject_media BEFORE INSERT ON message_media BEGIN SELECT RAISE(ABORT,'synthetic media failure'); END;").unwrap();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(
                    m.clone(),
                    Some(client.to_string()),
                    None,
                    Some(content.clone())
                )],
                Some(("510_1", "100"))
            )
            .is_err());
        assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
        assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "1");
        store
            .conn
            .execute_batch("DROP TRIGGER reject_media")
            .unwrap();
        store
            .receive_batch_with_media(
                "11",
                &[(m, Some(client.to_string()), None, Some(content))],
                Some(("510_1", "100")),
            )
            .unwrap();
        let after: (String, Option<String>, String) = store
            .conn
            .query_row("SELECT state,server_id,body FROM outgoing", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(before, after);
    }
    #[test]
    fn directory_and_socket_media_delivery_order_and_binding_are_strict() {
        for directory_first in [true, false] {
            let mut store = MessageStore::in_memory().unwrap();
            let (m, content) = message(MediaKind::WebContent, "one");
            let mut batch = DirectoryBatch {
                account_id: "11".into(),
                entries: vec![DirectoryRecord {
                    remote_id: "22".into(),
                    is_group: false,
                    display_name: "Original name".into(),
                    avatar_url: None,
                    is_friend: None,
                    is_blocked: None,
                    member_count: None,
                }],
                messages: vec![m.clone()],
                message_info: vec![],
                message_media: vec![binding(&m, content.clone())],
                observed_at_ms: 1000,
            };
            if directory_first {
                store.apply_directory_batch(&batch).unwrap();
            }
            store
                .receive_batch_with_media(
                    "11",
                    &[(m.clone(), None, None, Some(content.clone()))],
                    Some(("510_1", "100")),
                )
                .unwrap();
            store.apply_directory_batch(&batch).unwrap();
            assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![m.clone()]);
            batch.entries[0].display_name = "Must roll back".into();
            batch.message_media[0].content = media_content(MediaKind::WebContent, 2);
            assert!(store.apply_directory_batch(&batch).is_err());
            assert_eq!(
                store
                    .directory_record("11", "d:22")
                    .unwrap()
                    .unwrap()
                    .display_name,
                "Original name"
            );
            batch.message_media[0] = binding(&m, content.clone());
            batch.message_media[0].account_id = "33".into();
            assert!(store.apply_directory_batch(&batch).is_err());
        }
    }
    #[test]
    fn restart_scoping_keyset_corruption_and_v5_upgrade_preserve_all_data() {
        let path = std::env::temp_dir().join(format!(
            "zalo-media-store-{}.db",
            crate::domain::local_message_id().unwrap()
        ));
        let (m, content) = message(MediaKind::Photo, "one");
        {
            let mut store = MessageStore::open(&path).unwrap();
            let mut other = m.clone();
            other.account_id = "33".into();
            store
                .receive_batch_with_media(
                    "11",
                    &[(m.clone(), None, None, Some(content.clone()))],
                    Some(("510_1", "100")),
                )
                .unwrap();
            store
                .receive_batch_with_media("33", &[(other, None, None, Some(content.clone()))], None)
                .unwrap();
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            assert_eq!(store.page("11", "d:22", None, 1).unwrap(), vec![m.clone()]);
            assert_eq!(
                store.message_media("33", "d:22", "one").unwrap(),
                Some(content)
            );
            store
                .conn
                .execute(
                    "UPDATE message_media SET projection='{}' WHERE account_id='11'",
                    [],
                )
                .unwrap();
            assert!(store.page("11", "d:22", None, 1).is_err());
            assert!(store.message_media("11", "d:22", "one").is_err());
            assert!(store.delete("11", "d:22", "one").unwrap());
            assert!(store.message_media("11", "d:22", "one").unwrap().is_none());
            assert!(store.message_media("33", "d:22", "one").unwrap().is_some());
        }
        std::fs::remove_file(path).unwrap();
        let mut previous = MessageStore::in_memory().unwrap();
        let mut legacy = m;
        legacy.media = None;
        previous
            .receive_batch("11", &[(legacy.clone(), None)], Some(("510_1", "100")))
            .unwrap();
        let client = previous
            .prepare_outgoing("11", "d:22", "preserved draft", 1001)
            .unwrap();
        previous
            .conn
            .execute_batch("DROP TABLE message_media;PRAGMA user_version=5;")
            .unwrap();
        let store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(store.page("11", "d:22", None, 10).unwrap(), vec![legacy]);
        assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT body FROM outgoing WHERE client_id=?1",
                    [client.to_string()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "preserved draft"
        );
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }

    fn downgrade_media_to_v6(conn: &Connection) {
        conn.execute_batch("BEGIN IMMEDIATE;
            ALTER TABLE message_media RENAME TO media_fixture;
            CREATE TABLE message_media(
                account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('chat.photo','chat.webcontent')),
                metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=4194304),
                projection TEXT NOT NULL CHECK(length(CAST(projection AS BLOB))<=16384),
                preview TEXT NOT NULL CHECK(length(CAST(preview AS BLOB))<=1024),
                PRIMARY KEY(account_id,thread_id,message_id),
                FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
            INSERT INTO message_media SELECT * FROM media_fixture;
            DROP TABLE media_fixture; PRAGMA user_version=6; COMMIT;").unwrap();
    }

    fn media_rows(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
        conn.prepare("SELECT account_id,thread_id,message_id,kind,metadata,projection,preview FROM message_media ORDER BY account_id,thread_id,message_id")
            .unwrap().query_map([], |row| (0..7).map(|index| row.get(index)).collect())
            .unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn populated_v6_upgrade_preserves_exact_sidecars_scope_and_pending_state() {
        let mut previous = MessageStore::in_memory().unwrap();
        let mut originals = Vec::new();
        for (account, kind) in [("11", MediaKind::Photo), ("33", MediaKind::WebContent)] {
            let (mut m, content) = message(kind, "shared");
            m.account_id = account.into();
            previous
                .receive_batch_with_media(
                    account,
                    &[(m.clone(), None, None, Some(content))],
                    Some(("510_1", "99")),
                )
                .unwrap();
            originals.push(m);
        }
        let client = previous
            .prepare_outgoing("11", "d:22", "preserved pending draft", 1001)
            .unwrap();
        downgrade_media_to_v6(&previous.conn);
        let rows = media_rows(&previous.conn);
        let mut store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(media_rows(&store.conn), rows);
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        for m in originals {
            assert_eq!(
                store.page(&m.account_id, "d:22", None, 10).unwrap(),
                vec![m.clone()]
            );
            assert_eq!(store.receive_cursor(&m.account_id, "510_1").unwrap(), "99");
        }
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT body FROM outgoing WHERE account_id='11' AND client_id=?1",
                    [client.to_string()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "preserved pending draft"
        );
        assert!(store
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
        for kind in [MediaKind::Recommended, MediaKind::Poll] {
            let (m, raw) = message(kind, kind.as_str());
            store
                .receive_batch_with_media("11", &[(m.clone(), None, None, Some(raw.clone()))], None)
                .unwrap();
            assert_eq!(store.message_media("11", "d:22", &m.id).unwrap(), Some(raw));
        }
        assert!(store.delete("11", "d:22", "shared").unwrap());
        assert!(store
            .message_media("11", "d:22", "shared")
            .unwrap()
            .is_none());
        assert!(store
            .message_media("33", "d:22", "shared")
            .unwrap()
            .is_some());
    }

    #[test]
    fn failed_v6_copy_rolls_back_schema_data_and_version() {
        let path = std::env::temp_dir().join(format!(
            "zalo-media-migration-{}.sqlite3",
            crate::domain::local_message_id().unwrap()
        ));
        let mut previous = MessageStore::open(&path).unwrap();
        let (m, raw) = message(MediaKind::Photo, "unchanged");
        previous
            .receive_batch_with_media("11", &[(m, None, None, Some(raw))], Some(("510_1", "91")))
            .unwrap();
        downgrade_media_to_v6(&previous.conn);
        previous
            .conn
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        previous
            .conn
            .execute(
                "UPDATE message_media SET projection=?1",
                ["x".repeat(MAX_PROJECTION_BYTES + 1)],
            )
            .unwrap();
        previous
            .conn
            .execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        let rows = media_rows(&previous.conn);
        drop(previous);
        assert!(MessageStore::open(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(media_rows(&conn), rows);
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='message_media_v6'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT last_id FROM receive_cursors WHERE account_id='11' AND queue='510_1'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "91"
        );
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn combined_info_media_budget_is_checked_before_receive_or_directory_writes() {
        let info = crate::domain::InfoMessageContent::new(
            &json!({"action":"msginfo.actionlist","unknown":"x".repeat(MAX_INFO_CONTENT_BYTES/2)}),
        )
        .unwrap();
        let media=MediaMessageContent::new(MediaKind::Photo,&json!({"title":"Ảnh","href":"https://photo-talk.zadn.vn/synthetic.jpg","unknown":"x".repeat(MAX_INFO_CONTENT_BYTES/2)})).unwrap();
        let (mut photo, _) = message(MediaKind::Photo, "photo");
        photo.media = Some(media.descriptor().clone());
        let mut notice = photo.clone();
        notice.id = "info".into();
        notice.text = "Thông tin nhóm".into();
        notice.media = None;
        let mut store = MessageStore::in_memory().unwrap();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[
                    (notice.clone(), None, Some(info.clone()), None),
                    (photo.clone(), None, None, Some(media.clone()))
                ],
                Some(("510_1", "100"))
            )
            .is_err());
        let batch = DirectoryBatch {
            account_id: "11".into(),
            entries: vec![],
            messages: vec![notice.clone(), photo.clone()],
            message_info: vec![crate::domain::MessageInfoBinding {
                account_id: "11".into(),
                thread_id: notice.thread_id,
                message_id: notice.id,
                content: info,
            }],
            message_media: vec![binding(&photo, media)],
            observed_at_ms: 1000,
        };
        assert!(store.apply_directory_batch(&batch).is_err());
        assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
        assert!(store.conversations("11", 10).unwrap().is_empty());
        assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "1");
    }
    #[test]
    fn displayed_id_refresh_is_scoped_bounded_and_never_returns_partial_corrupt_metadata() {
        let mut store = MessageStore::in_memory().unwrap();
        let (photo, raw) = message(MediaKind::Photo, "shared");
        store
            .receive_batch_with_media(
                "11",
                &[(photo.clone(), None, None, Some(raw.clone()))],
                None,
            )
            .unwrap();
        let mut foreign = photo.clone();
        foreign.account_id = "33".into();
        store
            .receive_batch_with_media("33", &[(foreign, None, None, Some(raw.clone()))], None)
            .unwrap();
        let mut other = photo.clone();
        other.thread_id = "d:44".into();
        store
            .receive_batch_with_media("11", &[(other, None, None, Some(raw))], None)
            .unwrap();
        let ids = vec!["missing".into(), "shared".into()];
        assert_eq!(
            store.refresh_messages("11", "d:22", &ids).unwrap(),
            vec![photo]
        );
        assert!(store
            .refresh_messages("other", "d:22", &ids)
            .unwrap()
            .is_empty());
        assert!(store
            .refresh_messages(
                "11",
                "d:22",
                &vec!["shared".into(); super::super::MAX_REFRESH_MESSAGES + 1]
            )
            .is_err());
        assert!(store
            .refresh_messages("11", "d:22", &["shared".into(), "shared".into()])
            .is_err());
        let (second, raw) = message(MediaKind::Photo, "valid");
        store
            .receive_batch_with_media("11", &[(second, None, None, Some(raw))], None)
            .unwrap();
        store.conn.execute("UPDATE message_media SET projection='not-json' WHERE account_id='11' AND thread_id='d:22' AND message_id='shared'",[]).unwrap();
        assert!(store
            .refresh_messages("11", "d:22", &["valid".into(), "shared".into()])
            .is_err());
        assert!(store
            .refresh_messages("33", "d:22", &["shared".into()])
            .unwrap()[0]
            .media
            .is_some());
    }
}
