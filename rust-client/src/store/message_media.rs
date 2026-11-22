//! Full immutable media content and a separately bounded presentation snapshot.
use super::FailureReason;
use crate::domain::media::poll_id_in_notice;
use crate::domain::poll::MAX_POLL_NOTICE_BYTES;
use crate::domain::{
    MediaDescriptor, MediaKind, MediaMessageContent, Message, MAX_INFO_CONTENT_BYTES,
    MAX_MEDIA_DESCRIPTOR_BYTES, MAX_PREVIEW_CHARS,
};
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};

const MAX_PROJECTION_BYTES: usize = MAX_MEDIA_DESCRIPTOR_BYTES;
const MAX_PREVIEW_BYTES: usize = 4 * MAX_PREVIEW_CHARS;

pub(super) fn initialize(conn: &Connection, version: i64) -> Result<()> {
    // Extend the sidecar CHECK constraint inside the caller's transaction.
    // INSERT...SELECT retains the original immutable JSON and presentation bytes.
    const VOICE_SCHEMA_VERSION: i64 = 22;
    let migrate = version < VOICE_SCHEMA_VERSION
        && conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='message_media')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
    if migrate {
        conn.execute_batch("ALTER TABLE message_media RENAME TO message_media_legacy;")?;
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_media(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('chat.photo','chat.webcontent','chat.recommended','group.poll','share.file','chat.voice')),
        metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=4194304),
        projection TEXT NOT NULL CHECK(length(CAST(projection AS BLOB))<=16384),
        preview TEXT NOT NULL CHECK(length(CAST(preview AS BLOB))<=1024),
        PRIMARY KEY(account_id,thread_id,message_id),
        FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);")?;
    if migrate {
        conn.execute_batch("INSERT INTO message_media(account_id,thread_id,message_id,kind,metadata,projection,preview)
            SELECT account_id,thread_id,message_id,kind,metadata,projection,preview FROM message_media_legacy;
            DROP TABLE message_media_legacy;")?;
    }
    Ok(())
}

/// Brings stored presentation snapshots up to the current reader. The full source content is immutable and is the
/// only input; a row whose content no longer parses is left as it is. Runs once per reader revision.
pub(super) fn reproject(conn: &Connection) -> Result<()> {
    reproject_photo_albums(conn)?;
    // Bump the marker when a descriptor derivation changes (v1: call logs read their params).
    if conn.execute(
        "INSERT OR IGNORE INTO store_markers(name) VALUES('media_projection_v1')",
        [],
    )? == 0
    {
        return Ok(());
    }
    let rows: Vec<(i64, String, String, String, String)> = {
        let mut statement = conn.prepare(
            "SELECT rowid,kind,metadata,projection,preview FROM message_media WHERE kind='chat.recommended'",
        )?;
        let rows = statement.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (rowid, kind, metadata, projection, stored_preview) in rows {
        let Some(kind) = MediaKind::from_wire(&kind) else {
            continue;
        };
        let Ok(value) = serde_json::from_str(&metadata) else {
            continue;
        };
        let Ok(content) = MediaMessageContent::new(kind, &value) else {
            continue;
        };
        if content.as_json() != metadata || content.descriptor().validate().is_err() {
            continue;
        }
        let Ok(next) = serde_json::to_string(content.descriptor()) else {
            continue;
        };
        let next_preview = preview(content.descriptor());
        if next.len() <= MAX_PROJECTION_BYTES
            && (next != projection || next_preview != stored_preview)
        {
            conn.execute(
                "UPDATE message_media SET projection=?2,preview=?3 WHERE rowid=?1",
                params![rowid, next, next_preview],
            )?;
        }
    }
    Ok(())
}

fn reproject_photo_albums(conn: &Connection) -> Result<()> {
    if conn.execute(
        "INSERT OR IGNORE INTO store_markers(name) VALUES('media_photo_album_v1')",
        [],
    )? == 0
    {
        return Ok(());
    }
    // Params may be encoded as a JSON string with escaped keys, so inspect
    // the source content one row at a time. Only replace the exact old projection.
    let mut query = conn.prepare(
        "SELECT rowid,metadata,projection,preview FROM message_media
         WHERE rowid>?1 AND kind='chat.photo'
         AND (instr(metadata,'group_layout_id')>0 OR instr(metadata,char(92)||'u')>0)
         ORDER BY rowid LIMIT 1",
    )?;
    let mut cursor = i64::MIN;
    while let Some((rowid, metadata, projection, stored_preview)) = query
        .query_row([cursor], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()?
    {
        cursor = rowid;
        let Ok(value) = serde_json::from_str(&metadata) else {
            continue;
        };
        let Ok(content) = MediaMessageContent::new(MediaKind::Photo, &value) else {
            continue;
        };
        if content.album_member().is_none()
            || content.as_json() != metadata
            || preview(content.descriptor()) != stored_preview
        {
            continue;
        }
        let previous = crate::domain::media::legacy_photo_projection(&value);
        if serde_json::from_str::<MediaDescriptor>(&projection)
            .ok()
            .as_ref()
            != Some(&previous)
        {
            continue;
        }
        let next = serde_json::to_string(content.descriptor())?;
        conn.execute(
            "UPDATE message_media SET projection=?2 WHERE rowid=?1",
            params![rowid, next],
        )?;
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
    let row: Option<Stored> = conn
        .prepare_cached(
            "SELECT kind,
        CASE WHEN kind='group.poll' AND length(CAST(metadata AS BLOB))<=?6 THEN metadata END,
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
                MAX_PREVIEW_BYTES as i64,
                MAX_POLL_NOTICE_BYTES as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(kind, notice, projection, stored_preview)| {
        let projection = projection.ok_or(FailureReason::ConflictingMetadata)?;
        let mut descriptor: MediaDescriptor =
            serde_json::from_str(&projection).map_err(|_| FailureReason::ConflictingMetadata)?;
        descriptor
            .validate()
            .map_err(|_| FailureReason::ConflictingMetadata)?;
        ensure!(
            kind == descriptor.kind().as_str()
                && stored_preview.as_deref() == Some(preview(&descriptor).as_str())
                && message.sticker.is_none()
                && message.text == descriptor.kind().legacy_placeholder(),
            FailureReason::ConflictingMetadata
        );
        if let MediaDescriptor::Poll(poll) = &mut descriptor {
            poll.poll_id = notice
                .as_deref()
                .and_then(|raw| poll_id_in_notice(raw, poll));
        }
        Ok(descriptor)
    })
    .transpose()
}

#[cfg(feature = "messaging")]
impl super::MessageStore {
    /// Resolve an opaque UI message key through the current account and visibility policy.
    pub(crate) fn poll_detail_target(
        &self,
        account: &str,
        thread: &str,
        message_id: &str,
    ) -> Result<Option<u64>> {
        use crate::domain::reply::valid_source_id;

        if !valid_source_id(account)
            || !thread.strip_prefix("g:").is_some_and(valid_source_id)
            || !valid_source_id(message_id)
            || self.group_write_blocked(account, thread)?
        {
            return Ok(None);
        }
        let allowed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
            params![account, thread],
            |row| row.get(0),
        )?;
        if !allowed {
            return Ok(None);
        }
        let mut messages = self.refresh_messages(account, thread, &[message_id.to_owned()])?;
        let Some(message) = messages.pop().filter(|message| !message.is_recalled()) else {
            return Ok(None);
        };
        Ok(match message.media {
            Some(MediaDescriptor::Poll(poll)) => poll.poll_id,
            _ => None,
        })
    }
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
            && message.text == content.kind().legacy_placeholder(),
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
                expires_at_ms: None,
                mentions: Vec::new(),
                reply_blocked: false,
                recalled: false,
                source_client_id: None,
                quote: None,
                account_id: "11".into(),
                thread_id: "d:22".into(),
                id: id.into(),
                sender: "22".into(),
                text: kind.legacy_placeholder().into(),
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

    #[cfg(feature = "messaging")]
    #[test]
    fn poll_detail_lookup_uses_the_saved_notice_and_live_privacy_scope() {
        let mut store = MessageStore::in_memory().unwrap();
        let (mut notice, _) = message(MediaKind::Poll, "801");
        notice.thread_id = "g:22".into();
        let content = MediaMessageContent::new(
            MediaKind::Poll,
            &json!({"action":"create", "params":"{\"pollId\":42,\"question\":\"Họp lúc nào?\"}"}),
        )
        .unwrap();
        notice.media = Some(content.descriptor().clone());
        store
            .receive_batch_with_media("11", &[(notice.clone(), None, None, Some(content))], None)
            .unwrap();
        assert_eq!(
            store.page("11", "g:22", None, 10).unwrap(),
            vec![notice.clone()]
        );
        let conflicting = MediaMessageContent::new(
            MediaKind::Poll,
            &json!({"action":"create", "params":"{\"pollId\":43,\"question\":\"Họp lúc nào?\"}"}),
        )
        .unwrap();
        notice.media = Some(conflicting.descriptor().clone());
        assert!(store
            .receive_batch_with_media("11", &[(notice, None, None, Some(conflicting))], None)
            .is_err());
        store.begin_live_privacy("11").unwrap();
        assert_eq!(store.poll_detail_target("11", "g:22", "801").unwrap(), None);
        store.replace_live_privacy("11", &[], None).unwrap();
        assert_eq!(
            store.poll_detail_target("11", "g:22", "801").unwrap(),
            Some(42)
        );
        for (account, thread, id) in [
            ("33", "g:22", "801"),
            ("11", "d:22", "801"),
            ("11", "g:23", "801"),
            ("11", "g:22", "42"),
        ] {
            assert_eq!(store.poll_detail_target(account, thread, id).unwrap(), None);
        }
        store
            .replace_live_privacy("11", &["g:22".into()], None)
            .unwrap();
        assert_eq!(store.poll_detail_target("11", "g:22", "801").unwrap(), None);
        store.unlock_live_thread("11", Some("g:22")).unwrap();
        assert_eq!(store.poll_detail_target("11", "g:22", "801").unwrap(), None);
        store.begin_live_privacy("11").unwrap();
        assert_eq!(store.poll_detail_target("11", "g:22", "801").unwrap(), None);
        store.replace_live_privacy("11", &[], None).unwrap();
        store
            .conn
            .execute("INSERT INTO group_left VALUES('11','g:22',1000)", [])
            .unwrap();
        assert_eq!(store.poll_detail_target("11", "g:22", "801").unwrap(), None);
    }
    #[test]
    fn stored_call_logs_are_reprojected_from_their_immutable_content() {
        let mut store = MessageStore::in_memory().unwrap();
        let content = MediaMessageContent::new(MediaKind::Recommended, &json!({
            "action":"recommened.calltime","description":"Cuộc gọi","href":"",
            "params":"{\"duration\":90,\"isCaller\":0,\"calltype\":0}","thumb":"","title":"sendBubbleMessage"
        }))
        .unwrap();
        let m = Message {
            expires_at_ms: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            source_client_id: None,
            quote: None,
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "call".into(),
            sender: "22".into(),
            text: "[Tin nhắn chat.recommended chưa được hỗ trợ]".into(),
            sticker: None,
            media: Some(content.descriptor().clone()),
            sent_at_ms: 1000,
        };
        store
            .receive_batch_with_media("11", &[(m, None, None, Some(content.clone()))], None)
            .unwrap();
        // What an older reader stored: the placeholder title.
        store
            .conn
            .execute_batch(
                "UPDATE message_media SET projection=replace(projection,'Cuộc gọi thoại đến','sendBubbleMessage'),
                    preview='sendBubbleMessage';
                DELETE FROM store_markers WHERE name='media_projection_v1';",
            )
            .unwrap();
        assert!(store.message_media("11", "d:22", "call").is_err());
        reproject(&store.conn).unwrap();
        assert_eq!(
            store.message_media("11", "d:22", "call").unwrap(),
            Some(content)
        );
        assert_eq!(
            store.conversations("11", 10).unwrap()[0].last_text,
            "Cuộc gọi thoại đến"
        );
    }

    #[test]
    fn old_album_projections_upgrade_without_accepting_tampered_members() {
        let mut store = MessageStore::in_memory().unwrap();
        let address = "https://f1.zdn.vn/";
        let mut raw = json!({
            "title":"\"".repeat(1024), "description":"\"".repeat(4096),
            "href":address, "thumb":address
        });
        let base = MediaMessageContent::new(MediaKind::Photo, &raw).unwrap();
        let base_bytes = serde_json::to_vec(base.descriptor()).unwrap().len();
        let fill = (MAX_PROJECTION_BYTES - base_bytes - 8) / 2;
        let address = format!("{address}{}", "x".repeat(fill));
        raw["href"] = json!(address);
        raw["thumb"] = json!(address);
        let previous = MediaMessageContent::new(MediaKind::Photo, &raw)
            .unwrap()
            .descriptor()
            .clone();
        raw["params"] = json!(
            r#"{"group_\u006cayout_id":"7001","id_in_group":0,"is_group_layout":1,"total_item_in_group":3}"#
        );
        let content = MediaMessageContent::new(MediaKind::Photo, &raw).unwrap();
        assert!(content.album_member().is_some());
        assert!(!content.as_json().contains("group_layout_id"));
        let (MediaDescriptor::Photo(old), MediaDescriptor::Photo(new)) =
            (&previous, content.descriptor())
        else {
            panic!("expected photo")
        };
        assert_eq!(old.caption.len(), 4096);
        assert!(new.caption.len() < old.caption.len());
        let (mut legacy, _) = message(MediaKind::Photo, "album-legacy");
        legacy.media = Some(content.descriptor().clone());
        let mut altered = legacy.clone();
        altered.id = "album-altered".into();
        store
            .receive_batch_with_media(
                "11",
                &[
                    (legacy.clone(), None, None, Some(content.clone())),
                    (altered.clone(), None, None, Some(content.clone())),
                ],
                None,
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE message_media SET projection=?1 WHERE message_id='album-legacy'",
                [serde_json::to_string(&previous).unwrap()],
            )
            .unwrap();
        let mut tampered = serde_json::to_value(content.descriptor()).unwrap();
        tampered["data"]["album"]["index"] = json!(1);
        store
            .conn
            .execute(
                "UPDATE message_media SET projection=?1 WHERE message_id='album-altered'",
                [tampered.to_string()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "DELETE FROM store_markers WHERE name='media_photo_album_v1'",
                [],
            )
            .unwrap();
        assert!(load(&store.conn, "11", "d:22", &legacy.id).is_err());
        reproject(&store.conn).unwrap();
        assert_eq!(
            load(&store.conn, "11", "d:22", &legacy.id).unwrap(),
            Some(content.clone())
        );
        assert_eq!(
            projection(&store.conn, &legacy).unwrap().as_ref(),
            Some(content.descriptor())
        );
        assert!(load(&store.conn, "11", "d:22", &altered.id).is_err());
        assert!(persist(&store.conn, &altered, Some(&content), true).is_err());
        persist(&store.conn, &legacy, Some(&content), true).unwrap();
    }
    #[test]
    fn legacy_media_enrichment_preserves_body_and_publishes_typed_history_and_preview() {
        for kind in [
            MediaKind::Photo,
            MediaKind::WebContent,
            MediaKind::Recommended,
            MediaKind::Poll,
            MediaKind::File,
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
                message_quotes: vec![],
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
            crate::store::SCHEMA_VERSION
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
    fn schema_21_upgrade_preserves_album_and_enriches_saved_voice_on_replay() {
        let mut previous = MessageStore::in_memory().unwrap();
        let (mut album, _) = message(MediaKind::Photo, "album");
        let album_content = MediaMessageContent::new(
            MediaKind::Photo,
            &json!({"params":{"group_layout_id":"100","id_in_group":0,"is_group_layout":1,"total_item_in_group":2}}),
        )
        .unwrap();
        album.media = Some(album_content.descriptor().clone());
        previous
            .receive_batch_with_media(
                "11",
                &[(album.clone(), None, None, Some(album_content))],
                None,
            )
            .unwrap();
        let (mut voice, _) = message(MediaKind::Voice, "voice");
        let mut old_voice = voice.clone();
        old_voice.media = None;
        previous
            .receive_batch("11", &[(old_voice, None)], None)
            .unwrap();
        let old_schema: String = previous.conn.query_row(
            "SELECT replace(sql, ',' || quote('chat.voice'), '') FROM sqlite_master WHERE name='message_media'",
            [], |row| row.get(0)
        ).unwrap();
        previous
            .conn
            .execute_batch("ALTER TABLE message_media RENAME TO media_fixture;")
            .unwrap();
        previous.conn.execute_batch(&old_schema).unwrap();
        previous.conn.execute_batch("INSERT INTO message_media SELECT * FROM media_fixture; DROP TABLE media_fixture; PRAGMA user_version=21;").unwrap();
        let rows = media_rows(&previous.conn);
        let mut store = MessageStore::initialize(previous.conn).unwrap();
        assert_eq!(media_rows(&store.conn), rows);
        let voice_content = MediaMessageContent::new(
            MediaKind::Voice,
            &json!({"href":"https://voice.talk.zdn.vn/synthetic.mp3","params":{"duration":4500}}),
        )
        .unwrap();
        voice.media = Some(voice_content.descriptor().clone());
        let received = store
            .receive_batch_with_media(
                "11",
                &[(voice.clone(), None, None, Some(voice_content))],
                None,
            )
            .unwrap();
        assert_eq!(&received[0].0, &voice);
        let page = store.page("11", "d:22", None, 20).unwrap();
        assert!(page.contains(&album));
        assert!(page.contains(&voice));
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
            crate::store::SCHEMA_VERSION
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
                "SELECT count(*) FROM sqlite_master WHERE name='message_media_legacy'",
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
            message_quotes: vec![],
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
