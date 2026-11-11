//! Disposable schema-eight fixtures for the additive, typed peer-init intent.
use rusqlite::{types::Value, Connection};
use std::path::{Path, PathBuf};
use zalo_rust::store::MessageStore;

const TABLES: &[&str] = &[
    "messages",
    "conversations",
    "outgoing",
    "receive_cursors",
    "e2ee_outbox",
    "e2ee_records",
];
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zalo-readiness-schema-{}.sqlite3",
            zalo_rust::domain::local_message_id().unwrap()
        ));
        drop(MessageStore::open(&path).unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE e2ee_peer_init_outbox;
            INSERT INTO conversations VALUES('11','d:22','Hội thoại tổng hợp',100);
            INSERT INTO messages VALUES('11','d:22','9001','22','Tin được giữ nguyên',100);
            INSERT INTO outgoing VALUES('11','d:22','7001','Bản nháp chưa rõ kết quả',100,'unknown',NULL);
            INSERT INTO receive_cursors VALUES('11','synthetic-queue','101');
            INSERT INTO e2ee_records VALUES('11','synthetic-ratchet',X'0102030405');
            INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata)
              VALUES('11',1,'d:22',1,2,'{\"subDevice\":[]}',100,'unknown',X'000102FF',X'7B20207D');
            INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata)
              VALUES('33',1,'d:22',1,2,'Nội dung tổng hợp',101,'sending',X'030405FF',X'7B207D');
            PRAGMA user_version=8;").unwrap();
        Self(path)
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}
fn snapshot(path: &Path) -> Vec<Vec<Vec<Value>>> {
    let conn = Connection::open(path).unwrap();
    TABLES
        .iter()
        .map(|table| {
            let mut query = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2,3"))
                .unwrap();
            let columns = query.column_count();
            let rows = query
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        })
        .collect()
}
fn version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn version_eight_upgrade_preserves_bytes_and_never_guesses_init_from_body() {
    let db = Database::new();
    let before = snapshot(&db.0);
    for _ in 0..2 {
        drop(MessageStore::open(&db.0).unwrap());
        assert_eq!(snapshot(&db.0), before);
        let conn = Connection::open(&db.0).unwrap();
        assert_eq!(version(&conn), 9);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM e2ee_peer_init_outbox", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        assert!(conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
    }
}

#[test]
fn migration_failure_rolls_back_new_intent_table_and_schema_version() {
    let db = Database::new();
    let before = snapshot(&db.0);
    Connection::open(&db.0).unwrap().execute_batch("CREATE TRIGGER reject_projection_marker BEFORE INSERT ON store_markers BEGIN SELECT RAISE(ABORT,'synthetic migration failure'); END;").unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(version(&conn), 8);
    assert_eq!(snapshot(&db.0), before);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='e2ee_peer_init_outbox'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER reject_projection_marker;")
        .unwrap();
    drop(conn);
    drop(MessageStore::open(&db.0).unwrap());
    assert_eq!(version(&Connection::open(&db.0).unwrap()), 9);
}

#[test]
fn intent_foreign_key_is_account_scoped_and_cascades_with_its_outbox() {
    let db = Database::new();
    drop(MessageStore::open(&db.0).unwrap());
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    assert!(conn
        .execute(
            "INSERT INTO e2ee_peer_init_outbox(account,client_id) VALUES('11',2)",
            []
        )
        .is_err());
    conn.execute_batch(
        "INSERT INTO e2ee_peer_init_outbox(account,client_id) VALUES('11',1),('33',1);
        DELETE FROM e2ee_outbox WHERE account='11' AND client_id=1;",
    )
    .unwrap();
    assert_eq!(
        conn.query_row("SELECT account FROM e2ee_peer_init_outbox", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "33"
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM e2ee_outbox WHERE account='33' AND client_id=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn future_schema_is_refused_without_mutating_existing_rows() {
    let db = Database::new();
    let before = snapshot(&db.0);
    Connection::open(&db.0)
        .unwrap()
        .execute_batch("PRAGMA user_version=10;")
        .unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(snapshot(&db.0), before);
    assert_eq!(version(&Connection::open(&db.0).unwrap()), 10);
}
