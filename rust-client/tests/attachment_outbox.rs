//! Single-file durability checks use synthetic metadata, never local file paths.
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use zalo_rust::{
    domain::{MediaDescriptor, MediaKind, MediaMessageContent, Message},
    store::{
        attachments::{AttachmentResource, AttachmentSpec, AttachmentState},
        MessageStore, SCHEMA_VERSION,
    },
};

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "zalo-file-outbox-{}.sqlite3",
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
fn spec() -> AttachmentSpec {
    AttachmentSpec {
        file_name: "Tài liệu.txt".into(),
        size: 6,
        checksum: "a".repeat(32),
    }
}
fn resource(client: u64, thread: &str) -> AttachmentResource {
    let file = spec();
    let address = "https://files.zdn.vn/synthetic-file.txt";
    let mut parameters = json!({"fileId":"synthetic-file","checksum":file.checksum,"checksumSha":"","extension":"txt","totalSize":file.size,"fileName":file.file_name,"clientId":client,"fType":1,"fileCount":0,"fdata":"{}","fileUrl":address,"ttl":0});
    if let Some(peer) = thread.strip_prefix("d:") {
        parameters["toid"] = json!(peer);
        parameters["imei"] = json!("synthetic-device");
    } else {
        parameters["grid"] = json!(thread.strip_prefix("g:").unwrap());
    }
    AttachmentResource{file_id:"synthetic-file".into(),file_url:address.into(),checksum:file.checksum.clone(),parameters,content:MediaMessageContent::new(MediaKind::File,&json!({"title":file.file_name,"description":"","href":address,"params":{"fileSize":file.size,"checksum":file.checksum,"checksumSha":"","fileExt":"txt","fdata":"{}","fType":1}})).unwrap()}
}
fn ready(store: &mut MessageStore, thread: &str, time: i64) -> (u64, AttachmentResource) {
    let id = store
        .reserve_attachment("11", thread, &spec(), time)
        .unwrap();
    let resource = resource(id, thread);
    store
        .attachment_resource_ready("11", id, &resource)
        .unwrap();
    (id, resource)
}
fn own(id: &str, content: &MediaMessageContent, ts: i64) -> Message {
    Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: id.into(),
        sender: "11".into(),
        text: MediaKind::File.legacy_placeholder().into(),
        source_client_id: None,
        mentions: Vec::new(),
        reply_blocked: true,
        recalled: false,
        quote: None,
        sticker: None,
        media: Some(content.descriptor().clone()),
        sent_at_ms: ts,
    }
}

#[test]
fn unicode_extension_matches_native_projection_without_changing_wire_case() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let mut file = spec();
    file.file_name = "Tổng hợp.Ả".into();
    let id = store.reserve_attachment("11", "d:22", &file, 1000).unwrap();
    let mut resource = resource(id, "d:22");
    resource.parameters["fileName"] = json!(file.file_name);
    resource.parameters["extension"] = json!("Ả");
    resource.content = MediaMessageContent::new(
        MediaKind::File,
        &json!({"title":file.file_name,"description":"","href":resource.file_url,"params":{"fileSize":file.size,"checksum":file.checksum,"checksumSha":"","fileExt":"Ả","fdata":"{}","fType":1}}),
    )
    .unwrap();
    let MediaDescriptor::File(descriptor) = resource.content.descriptor() else {
        panic!("Expected a file descriptor");
    };
    assert_eq!(descriptor.extension, "ả");
    resource.parameters["extension"] = json!("PDF");
    assert!(store
        .attachment_resource_ready("11", id, &resource)
        .is_err());
    assert_eq!(
        store.attachment_state("11", id).unwrap(),
        Some(AttachmentState::Uploading)
    );
    resource.parameters["extension"] = json!("Ả");
    store
        .attachment_resource_ready("11", id, &resource)
        .unwrap();
    let payload = store.begin_attachment_dispatch("11", id).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&payload).unwrap()["extension"],
        "Ả"
    );
}

#[test]
fn restart_classifies_known_unsent_and_unknown_without_reuploading_or_text_placeholder_dedup() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let uploading = store
        .reserve_attachment("11", "d:22", &spec(), 1000)
        .unwrap();
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_attachments("11").unwrap();
    assert_eq!(
        store.attachment_state("11", uploading).unwrap(),
        Some(AttachmentState::Interrupted)
    );
    let (resource_ready, _) = ready(&mut store, "d:22", 1001);
    store.recover_attachments("11").unwrap();
    assert_eq!(
        store.attachment_state("11", resource_ready).unwrap(),
        Some(AttachmentState::Interrupted)
    );
    let (sending, resource) = ready(&mut store, "d:22", 1002);
    let exact = store.begin_attachment_dispatch("11", sending).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&exact).unwrap(),
        resource.parameters
    );
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_attachments("11").unwrap();
    assert_eq!(
        store.attachment_state("11", sending).unwrap(),
        Some(AttachmentState::Unknown)
    );
    assert!(store.begin_attachment_dispatch("11", sending).is_err());
    assert!(store
        .reserve_attachment("11", "d:22", &spec(), 1003)
        .is_err());
    let mut other = spec();
    other.file_name = "Tệp khác.txt".into();
    let next = store
        .reserve_attachment("11", "d:22", &other, 1004)
        .unwrap();
    assert!(next > sending);
    assert!(
        store
            .reserve_attachment("11", "g:77", &spec(), 1005)
            .is_err(),
        "Only one active upload for an account"
    );
    assert!(store
        .reserve_attachment("33", "d:22", &spec(), 1005)
        .is_ok());
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM outgoing", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0,
        "A file never uses the text placeholder outbox"
    );
    assert!(
        store.page("11", "d:22", None, 10).unwrap().is_empty(),
        "Resource readiness is not message acceptance"
    );
}

#[test]
fn exact_payload_is_immutable_and_wrong_resource_or_account_cannot_claim_dispatch() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let (id, resource) = ready(&mut store, "g:77", 1000);
    assert!(
        store
            .record_attachment_ack("11", id, "9001", Some(10))
            .is_err(),
        "Resource response is not final send acceptance"
    );
    let mut changed = self::resource(id, "g:77");
    changed.parameters["fileId"] = json!("other");
    assert!(store.attachment_resource_ready("11", id, &changed).is_err());
    assert!(store.begin_attachment_dispatch("33", id).is_err());
    let conn = Connection::open(&db.0).unwrap();
    conn.execute("UPDATE attachment_outbox SET file_url='https://files.zdn.vn/changed' WHERE account_id='11' AND client_id=?1",[id.to_string()]).unwrap();
    assert!(
        store.begin_attachment_dispatch("11", id).is_err(),
        "Changing resource metadata cannot retain a dispatch permit"
    );
    conn.execute(
        "UPDATE attachment_outbox SET file_url=?2 WHERE account_id='11' AND client_id=?1",
        rusqlite::params![id.to_string(), resource.file_url],
    )
    .unwrap();
    let exact = store.begin_attachment_dispatch("11", id).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&exact).unwrap(),
        resource.parameters
    );
    assert!(store.begin_attachment_dispatch("11", id).is_err());
    store.fail_attachment_dispatch("11", id, true).unwrap();
    store.interrupt_attachment("11", id).unwrap();
    assert_eq!(
        store.attachment_state("11", id).unwrap(),
        Some(AttachmentState::Unknown)
    );
}

#[test]
fn server_acceptance_survives_echo_sql_failure_and_repairs_exactly_one_file_card() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let (id, resource) = ready(&mut store, "d:22", 1000);
    store.begin_attachment_dispatch("11", id).unwrap();
    store
        .record_attachment_ack("11", id, "9001", Some(1700000000123))
        .unwrap();
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_attachment_echo BEFORE INSERT ON message_media BEGIN SELECT RAISE(ABORT,'synthetic file echo failure'); END;").unwrap();
    assert!(store.accept_attachment("11", id).is_err());
    assert_eq!(
        store.attachment_state("11", id).unwrap(),
        Some(AttachmentState::Acknowledged)
    );
    assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_attachments("11").unwrap();
    conn.execute_batch("DROP TRIGGER reject_attachment_echo;")
        .unwrap();
    store.reconcile_attachments("11").unwrap();
    store.reconcile_attachments("11").unwrap();
    let history = store.page("11", "d:22", None, 10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].media.as_ref(),
        Some(resource.content.descriptor())
    );
    assert_eq!(history[0].sent_at_ms, 1700000000123);
    assert_eq!(
        store.attachment_state("11", id).unwrap(),
        Some(AttachmentState::Accepted)
    );
    assert_eq!(
        store.message_media("11", "d:22", "9001").unwrap(),
        Some(resource.content)
    );
}

#[test]
fn own_file_echo_before_or_after_http_ack_settles_only_matching_typed_resource() {
    for echo_first in [true, false] {
        let db = Database::new();
        let mut store = MessageStore::open(&db.0).unwrap();
        let (id, resource) = ready(&mut store, "d:22", 1000);
        store.begin_attachment_dispatch("11", id).unwrap();
        let mut raw: Value = serde_json::from_str(resource.content.as_json()).unwrap();
        raw["wire_only"] = json!({"retain":[1,2,3]});
        let wire = MediaMessageContent::new(MediaKind::File, &raw).unwrap();
        if !echo_first {
            store
                .record_attachment_ack("11", id, "9001", Some(19))
                .unwrap();
            store.accept_attachment("11", id).unwrap();
        }
        let result = store
            .receive_batch_with_media(
                "11",
                &[(
                    own("9001", &wire, 20),
                    Some(id.to_string()),
                    None,
                    Some(wire.clone()),
                )],
                Some(("fixture", "100")),
            )
            .unwrap();
        assert_eq!(result.len(), 1);
        store
            .record_attachment_ack("11", id, "9001", Some(19))
            .unwrap();
        store.accept_attachment("11", id).unwrap();
        assert_eq!(store.page("11", "d:22", None, 10).unwrap().len(), 1);
        assert_eq!(
            store.message_media("11", "d:22", "9001").unwrap(),
            Some(wire.clone())
        );
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(
                    own("9001", &wire, 20),
                    Some("foreign-hint".into()),
                    None,
                    Some(wire.clone())
                )],
                Some(("fixture", "101"))
            )
            .unwrap()
            .is_empty());
        let mut wrong = own("9002", &wire, 21);
        wrong.thread_id = "g:77".into();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(wrong, Some(id.to_string()), None, Some(wire.clone()))],
                Some(("fixture", "102"))
            )
            .is_err());
        assert_eq!(store.receive_cursor("11", "fixture").unwrap(), "101");
        let mut wrong_resource: Value = serde_json::from_str(wire.as_json()).unwrap();
        wrong_resource["href"] = json!("https://files.zdn.vn/other.txt");
        let wrong_resource = MediaMessageContent::new(MediaKind::File, &wrong_resource).unwrap();
        assert!(store
            .receive_batch_with_media(
                "11",
                &[(
                    own("9001", &wrong_resource, 20),
                    None,
                    None,
                    Some(wrong_resource)
                )],
                Some(("fixture", "102"))
            )
            .is_err());
    }
}

const TABLES: &[&str] = &[
    "messages",
    "message_reply",
    "outgoing",
    "outgoing_reply",
    "receive_cursors",
    "e2ee_outbox",
    "e2ee_peer_init_outbox",
    "e2ee_records",
];
fn rows(path: &Path) -> Vec<Vec<Vec<SqlValue>>> {
    let conn = Connection::open(path).unwrap();
    TABLES
        .iter()
        .map(|table| {
            let mut query = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
                .unwrap();
            let columns = query.column_count();
            let values = query
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap();
            values.collect::<rusqlite::Result<_>>().unwrap()
        })
        .collect()
}
fn seed_v10(path: &Path) {
    let mut store = MessageStore::open(path).unwrap();
    let source = Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: "9001".into(),
        sender: "22".into(),
        text: "Nguồn tổng hợp".into(),
        source_client_id: Some("8001".into()),
        mentions: Vec::new(),
        reply_blocked: false,
        recalled: false,
        quote: None,
        sticker: None,
        media: None,
        sent_at_ms: 100,
    };
    store.insert_batch(&[source]).unwrap();
    store
        .prepare_outgoing_reply("11", "d:22", "Bản nháp trả lời", "9001", 1000)
        .unwrap();
    drop(store);
    Connection::open(path).unwrap().execute_batch("DROP TABLE attachment_outbox; INSERT INTO receive_cursors VALUES('11','fixture','100');INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata) VALUES('11',1,'d:22',1,2,'synthetic init',100,'unknown',X'010203FF',X'7B7D');INSERT INTO e2ee_peer_init_outbox VALUES('11',1);INSERT INTO e2ee_records VALUES('11','synthetic-ratchet',X'FF0102');PRAGMA user_version=10;").unwrap();
}

#[test]
fn schema_ten_upgrade_and_failed_migration_preserve_quote_history_and_crypto_intents() {
    let db = Database::new();
    seed_v10(&db.0);
    let before = rows(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_attachment_upgrade BEFORE INSERT ON store_markers BEGIN SELECT RAISE(ABORT,'synthetic migration failure'); END;").unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(rows(&db.0), before);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        10
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='attachment_outbox'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER reject_attachment_upgrade;")
        .unwrap();
    for _ in 0..2 {
        drop(MessageStore::open(&db.0).unwrap());
        assert_eq!(rows(&db.0), before);
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(rows(&db.0), before);
}
