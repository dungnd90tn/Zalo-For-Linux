//! Independent compatibility tests using disposable synthetic v7 databases.
use rusqlite::{types::Value, Connection};
use serde_json::json;
use std::path::PathBuf;
use zalo_rust::{
    domain::{MediaKind, MediaMessageContent, Message},
    store::MessageStore,
};

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "zalo-file-upgrade-{}.sqlite3",
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

fn rows(conn: &Connection, table: &str) -> Vec<Vec<Value>> {
    assert!(["message_media", "messages", "outgoing", "receive_cursors"].contains(&table));
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2,3"))
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn seed_v7(path: &std::path::Path) {
    let mut store = MessageStore::open(path).unwrap();
    for (account, kind) in [("11", MediaKind::Photo), ("33", MediaKind::Recommended)] {
        let content = MediaMessageContent::new(
            kind,
            &json!({
                "title":"Nội dung tổng hợp", "description":"Giữ nguyên bản gốc",
                "href":"https://zalo.me/synthetic", "unknown":{"bytes":[1,2,3]}
            }),
        )
        .unwrap();
        let message = Message {
            account_id: account.into(),
            thread_id: "d:22".into(),
            id: "shared".into(),
            sender: "22".into(),
            text: kind.legacy_placeholder().into(),
            sticker: None,
            media: Some(content.descriptor().clone()),
            sent_at_ms: 100,
        };
        store
            .receive_batch_with_media(
                account,
                &[(message, None, None, Some(content))],
                Some(("510_1", "101")),
            )
            .unwrap();
        store
            .prepare_outgoing(account, "d:22", "Bản nháp chờ xác nhận", 102)
            .unwrap();
    }
    drop(store);
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE;
        ALTER TABLE message_media RENAME TO fixture_media;
        CREATE TABLE message_media(
            account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('chat.photo','chat.webcontent','chat.recommended','group.poll')),
            metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=4194304),
            projection TEXT NOT NULL CHECK(length(CAST(projection AS BLOB))<=16384),
            preview TEXT NOT NULL CHECK(length(CAST(preview AS BLOB))<=1024),
            PRIMARY KEY(account_id,thread_id,message_id),
            FOREIGN KEY(account_id,thread_id,message_id) REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE);
        INSERT INTO message_media SELECT * FROM fixture_media;
        DROP TABLE fixture_media;
        UPDATE message_media SET metadata='  '||metadata||char(10);
        PRAGMA user_version=7; COMMIT;").unwrap();
}

#[test]
fn populated_v7_keeps_exact_bytes_pending_state_and_cascades_then_accepts_files() {
    let db = Database::new();
    seed_v7(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    let tables = ["message_media", "messages", "outgoing", "receive_cursors"];
    let before: Vec<_> = tables.iter().map(|table| rows(&conn, table)).collect();
    drop(conn);
    for _ in 0..2 {
        drop(MessageStore::open(&db.0).unwrap());
        let conn = Connection::open(&db.0).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            9
        );
        for (table, expected) in tables.iter().zip(&before) {
            assert_eq!(&rows(&conn, table), expected, "changed {table}");
        }
        assert!(conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
        assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_master WHERE name LIKE 'live_%' OR name='message_media_v6'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
    let mut store = MessageStore::open(&db.0).unwrap();
    let content = MediaMessageContent::new(MediaKind::File, &json!({"title":"Tài liệu.pdf", "href":"https://files.zdn.vn/synthetic.pdf", "params":"{\"fileExt\":\"pdf\",\"fileSize\":123}"})).unwrap();
    let message = Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: "new-file".into(),
        sender: "22".into(),
        text: MediaKind::File.legacy_placeholder().into(),
        sticker: None,
        media: Some(content.descriptor().clone()),
        sent_at_ms: 200,
    };
    store
        .receive_batch_with_media("11", &[(message, None, None, Some(content.clone()))], None)
        .unwrap();
    assert_eq!(
        store.message_media("11", "d:22", "new-file").unwrap(),
        Some(content)
    );
    assert!(store.delete("11", "d:22", "shared").unwrap());
    assert!(store
        .message_media("11", "d:22", "shared")
        .unwrap()
        .is_none());
    // The synthetic noncanonical whitespace probes byte retention only; the
    // normal reader correctly refuses noncanonical raw JSON projections.
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM message_media WHERE account_id='33' AND thread_id='d:22' AND message_id='shared'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn invalid_v7_sidecar_rolls_back_copy_and_version_without_losing_rows() {
    let db = Database::new();
    seed_v7(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE message_media SET preview=replace(hex(zeroblob(600)),'0','x'); PRAGMA ignore_check_constraints=OFF;").unwrap();
    let before = rows(&conn, "message_media");
    drop(conn);
    assert!(MessageStore::open(&db.0).is_err());
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(rows(&conn, "message_media"), before);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='message_media_v6'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn newer_schema_is_refused_without_downgrade_or_row_changes() {
    let db = Database::new();
    seed_v7(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("PRAGMA user_version=10").unwrap();
    let before = rows(&conn, "message_media");
    drop(conn);
    assert!(MessageStore::open(&db.0).is_err());
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(rows(&conn, "message_media"), before);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        10
    );
}
