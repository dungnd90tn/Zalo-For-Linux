//! Synthetic processed-photo metadata: no source image paths, pixels or network.
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use zalo_rust::{
    domain::{MediaKind, MediaMessageContent, Message},
    store::{
        attachments::AttachmentSpec,
        photos::{PhotoResource, PhotoSpec, PhotoState},
        MessageStore,
    },
};
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "zalo-photo-outbox-{}.sqlite3",
            zalo_rust::domain::local_message_id().unwrap()
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}
fn spec() -> PhotoSpec {
    PhotoSpec {
        caption: "Ảnh tổng hợp\nGiữ nguyên chú thích".into(),
        width: 800,
        height: 600,
        size: 1000,
        checksum: "a".repeat(32),
    }
}
fn resource(id: u64, thread: &str, file: &PhotoSpec) -> PhotoResource {
    let org = "https://files.zdn.vn/photo-original.jpg";
    let thumb = "https://files.zdn.vn/photo-thumb.jpg";
    let hd = "https://files.zdn.vn/photo-hd.jpg";
    let mut parameters = json!({"photoId":"synthetic-photo","clientId":id,"desc":file.caption,"width":file.width,"height":file.height,"rawUrl":org,"thumbUrl":thumb,"hdUrl":hd,"language":"VN","imei":"synthetic-device","ttl":0});
    if let Some(peer) = thread.strip_prefix("d:") {
        parameters["toid"] = json!(peer);
        parameters["normalUrl"] = json!(org);
    } else {
        parameters["grid"] = json!(thread.strip_prefix("g:").unwrap());
        parameters["oriUrl"] = json!(org);
    }
    PhotoResource{photo_id:"synthetic-photo".into(),parameters,content:MediaMessageContent::new(MediaKind::Photo,&json!({"title":"","description":file.caption,"href":org,"thumb":thumb,"params":{"width":file.width,"height":file.height,"hd":hd,"rawUrl":org}})).unwrap()}
}
fn ready(store: &mut MessageStore, thread: &str, time: i64) -> (u64, PhotoResource) {
    let file = spec();
    let id = store.reserve_photo("11", thread, &file, time).unwrap();
    let resource = resource(id, thread, &file);
    store.photo_resource_ready("11", id, &resource).unwrap();
    (id, resource)
}
fn own(server: &str, content: &MediaMessageContent, ts: i64) -> Message {
    Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: server.into(),
        sender: "11".into(),
        text: content.kind().legacy_placeholder().into(),
        source_client_id: None,
        reply_blocked: true,
        recalled: false,
        quote: None,
        sticker: None,
        media: Some(content.descriptor().clone()),
        sent_at_ms: ts,
    }
}
#[test]
fn restart_unknown_blocks_same_jpeg_even_with_new_caption_and_shared_active_guard() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let uploading = store.reserve_photo("11", "d:22", &spec(), 1000).unwrap();
    let file = AttachmentSpec {
        file_name: "a.txt".into(),
        size: 6,
        checksum: "b".repeat(32),
    };
    assert!(store.reserve_attachment("11", "d:22", &file, 1001).is_err());
    assert!(store.reserve_photo("11", "g:77", &spec(), 1001).is_err());
    assert!(store.reserve_attachment("33", "d:22", &file, 1001).is_ok());
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_photos("11").unwrap();
    assert_eq!(
        store.photo_state("11", uploading).unwrap(),
        Some(PhotoState::Interrupted)
    );
    let (ready_id, _) = ready(&mut store, "d:22", 1002);
    store.recover_photos("11").unwrap();
    assert_eq!(
        store.photo_state("11", ready_id).unwrap(),
        Some(PhotoState::Interrupted)
    );
    let (id, resource) = ready(&mut store, "d:22", 1003);
    assert_eq!(
        serde_json::from_slice::<Value>(&store.begin_photo_dispatch("11", id).unwrap()).unwrap(),
        resource.parameters
    );
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_photos("11").unwrap();
    assert_eq!(
        store.photo_state("11", id).unwrap(),
        Some(PhotoState::Unknown)
    );
    assert!(store.begin_photo_dispatch("11", id).is_err());
    let mut changed = spec();
    changed.caption = "Chú thích khác".into();
    assert!(store.reserve_photo("11", "d:22", &changed, 1004).is_err());
    let file_id = store.reserve_attachment("11", "d:22", &file, 1004).unwrap();
    assert!(file_id > id);
    assert!(store.reserve_photo("11", "d:22", &changed, 1005).is_err());
    store.interrupt_attachment("11", file_id).unwrap();
    changed.checksum = "c".repeat(32);
    assert!(store.reserve_photo("11", "d:22", &changed, 1005).is_ok());
    assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
}
#[test]
fn bound_photo_payload_rejects_wrong_kind_scope_caption_and_tampering_before_dispatch() {
    for group in [false, true] {
        let db = Database::new();
        let mut store = MessageStore::open(&db.0).unwrap();
        let thread = if group { "g:77" } else { "d:22" };
        let file = spec();
        let id = store.reserve_photo("11", thread, &file, 1000).unwrap();
        let mut resource = resource(id, thread, &file);
        assert!(store.photo_resource_ready("33", id, &resource).is_err());
        for (key, wrong) in [
            ("desc", json!("khác")),
            ("clientId", json!(id + 1)),
            ("width", json!(801)),
            ("imei", json!("")),
            ("ttl", json!(1)),
            ("fileId", json!("file-resource")),
            ("rawUrl", json!("http://files.zdn.vn/photo-original.jpg")),
        ] {
            let old = resource.parameters.get(key).cloned();
            resource.parameters[key] = wrong;
            assert!(
                store.photo_resource_ready("11", id, &resource).is_err(),
                "invalid {key}"
            );
            if let Some(old) = old {
                resource.parameters[key] = old;
            } else {
                resource.parameters.as_object_mut().unwrap().remove(key);
            }
        }
        let original = resource.content.clone();
        resource.content=MediaMessageContent::new(MediaKind::File,&json!({"title":"file.txt","href":"https://files.zdn.vn/photo-original.jpg","params":{"fileSize":1000,"fileExt":"txt"}})).unwrap();
        assert!(store.photo_resource_ready("11", id, &resource).is_err());
        resource.content = original;
        assert!(store.record_photo_ack("11", id, "9001", Some(20)).is_err());
        store.photo_resource_ready("11", id, &resource).unwrap();
        resource.parameters["desc"] = json!("new caption");
        assert!(store.photo_resource_ready("11", id, &resource).is_err());
        Connection::open(&db.0)
            .unwrap()
            .execute(
                "UPDATE photo_outbox SET caption='tampered' WHERE account_id='11' AND client_id=?1",
                [id.to_string()],
            )
            .unwrap();
        assert!(store.begin_photo_dispatch("11", id).is_err());
        assert_eq!(
            store.photo_state("11", id).unwrap(),
            Some(PhotoState::ResourceReady)
        );
    }
}
#[test]
fn acknowledged_photo_repairs_once_after_sql_failure_without_another_send() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let (id, resource) = ready(&mut store, "d:22", 1000);
    store.begin_photo_dispatch("11", id).unwrap();
    store.record_photo_ack("11", id, "9001", Some(20)).unwrap();
    Connection::open(&db.0).unwrap().execute_batch("CREATE TRIGGER reject_photo_echo BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(store.accept_photo("11", id).is_err());
    assert_eq!(
        store.photo_state("11", id).unwrap(),
        Some(PhotoState::Acknowledged)
    );
    assert!(store.reserve_photo("11", "d:22", &spec(), 1001).is_err());
    assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
    drop(store);
    Connection::open(&db.0)
        .unwrap()
        .execute_batch("DROP TRIGGER reject_photo_echo;")
        .unwrap();
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_photos("11").unwrap();
    for _ in 0..2 {
        store.reconcile_photos("11").unwrap();
    }
    let page = store.page("11", "d:22", None, 10).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, "9001");
    assert_eq!(page[0].media.as_ref(), Some(resource.content.descriptor()));
    assert_eq!(
        store.photo_state("11", id).unwrap(),
        Some(PhotoState::Accepted)
    );
    assert!(store.begin_photo_dispatch("11", id).is_err());
    assert_eq!(
        store
            .message_media("11", "d:22", "9001")
            .unwrap()
            .unwrap()
            .as_json(),
        resource.content.as_json()
    );
}
#[test]
fn real_photo_echo_and_http_ack_orders_preserve_raw_and_reject_cross_kind_settlement() {
    for echo_first in [false, true] {
        let db = Database::new();
        let mut store = MessageStore::open(&db.0).unwrap();
        let (id, resource) = ready(&mut store, "d:22", 1000);
        store.begin_photo_dispatch("11", id).unwrap();
        let file=MediaMessageContent::new(MediaKind::File,&json!({"title":"a.txt","href":"https://files.zdn.vn/photo-original.jpg","params":{"fileSize":1000,"fileExt":"txt"}})).unwrap();
        store
            .receive_batch_with_media(
                "11",
                &[(
                    own("8000", &file, 10),
                    Some(id.to_string()),
                    None,
                    Some(file),
                )],
                Some(("fixture", "100")),
            )
            .unwrap();
        assert_eq!(
            store.photo_state("11", id).unwrap(),
            Some(PhotoState::Sending)
        );
        let mut raw: Value = serde_json::from_str(resource.content.as_json()).unwrap();
        raw["opaque"] = json!({"keep":[1,"synthetic"]});
        let wire = MediaMessageContent::new(MediaKind::Photo, &raw).unwrap();
        if !echo_first {
            store.record_photo_ack("11", id, "9001", Some(19)).unwrap();
            store.accept_photo("11", id).unwrap();
        }
        let hint = if echo_first {
            id.to_string()
        } else {
            "foreign-hint".into()
        };
        store
            .receive_batch_with_media(
                "11",
                &[(own("9001", &wire, 20), Some(hint), None, Some(wire.clone()))],
                Some(("fixture", "101")),
            )
            .unwrap();
        store.record_photo_ack("11", id, "9001", Some(19)).unwrap();
        let (accepted, _) = store.accept_photo("11", id).unwrap();
        assert_eq!(accepted.sent_at_ms, 20);
        assert_eq!(
            store.photo_state("11", id).unwrap(),
            Some(PhotoState::Accepted)
        );
        assert_eq!(
            store
                .message_media("11", "d:22", "9001")
                .unwrap()
                .unwrap()
                .as_json(),
            wire.as_json()
        );
        let mut wrong = own("9001", &wire, 20);
        wrong.thread_id = "g:77".into();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(wrong, Some(id.to_string()), None, Some(wire.clone()))],
                Some(("fixture", "102"))
            )
            .is_err());
        raw["description"] = json!("wrong caption");
        let wrong = MediaMessageContent::new(MediaKind::Photo, &raw).unwrap();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(
                    own("9001", &wrong, 20),
                    Some(id.to_string()),
                    None,
                    Some(wrong)
                )],
                Some(("fixture", "102"))
            )
            .is_err());
        assert_eq!(store.receive_cursor("11", "fixture").unwrap(), "101");
    }
}
const TABLES: &[&str] = &[
    "messages",
    "message_reply",
    "outgoing",
    "outgoing_reply",
    "attachment_outbox",
    "e2ee_outbox",
    "e2ee_peer_init_outbox",
    "e2ee_records",
    "receive_cursors",
];
fn rows(path: &Path) -> Vec<Vec<Vec<SqlValue>>> {
    let conn = Connection::open(path).unwrap();
    TABLES
        .iter()
        .map(|table| {
            let mut query = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
                .unwrap();
            let cols = query.column_count();
            let values = query
                .query_map([], |row| (0..cols).map(|col| row.get(col)).collect())
                .unwrap();
            values.collect::<rusqlite::Result<_>>().unwrap()
        })
        .collect()
}
#[test]
fn schema_eleven_migration_preserves_file_quote_and_crypto_bytes_and_rolls_back() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let mut source = own("9001", &resource(1, "d:22", &spec()).content, 100);
    source.media = None;
    source.text = "Nguồn".into();
    source.sender = "22".into();
    source.source_client_id = Some("8001".into());
    source.reply_blocked = false;
    store.insert_batch(&[source]).unwrap();
    store
        .prepare_outgoing_reply("11", "d:22", "Trả lời", "9001", 1000)
        .unwrap();
    store
        .reserve_attachment(
            "11",
            "g:77",
            &AttachmentSpec {
                file_name: "a.txt".into(),
                size: 6,
                checksum: "b".repeat(32),
            },
            1001,
        )
        .unwrap();
    drop(store);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("DROP TABLE photo_outbox;INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata) VALUES('11',1,'d:22',1,2,'synthetic init',100,'unknown',X'010203FF',X'7B7D');INSERT INTO e2ee_peer_init_outbox VALUES('11',1);INSERT INTO e2ee_records VALUES('11','synthetic-ratchet',X'FF0102');INSERT INTO receive_cursors VALUES('11','fixture','100');PRAGMA user_version=11;CREATE TRIGGER reject_photo_upgrade BEFORE INSERT ON store_markers BEGIN SELECT RAISE(ABORT,'synthetic migration failure'); END;").unwrap();
    let before = rows(&db.0);
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(rows(&db.0), before);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='photo_outbox'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER reject_photo_upgrade;")
        .unwrap();
    for _ in 0..2 {
        drop(MessageStore::open(&db.0).unwrap());
        assert_eq!(rows(&db.0), before);
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            18
        );
    }
    conn.execute_batch("PRAGMA user_version=19;").unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(rows(&db.0), before);
}

#[test]
fn photo_bounds_reject_invalid_processed_metadata() {
    for field in 0..5 {
        let mut store = MessageStore::in_memory().unwrap();
        let mut file = spec();
        match field {
            0 => file.width = 0,
            1 => file.height = 2561,
            2 => file.size = 5 * 1024 * 1024 + 1,
            3 => file.size = 0,
            _ => file.checksum = "not-md5".into(),
        }
        assert!(store.reserve_photo("11", "d:22", &file, 1000).is_err());
    }
}
