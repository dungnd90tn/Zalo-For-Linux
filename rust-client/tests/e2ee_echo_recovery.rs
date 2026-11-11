//! Independent disk fixtures for repairing a persisted ACK without dispatching it.
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use zalo_rust::store::{e2ee::MAX_ACK_REPAIR_BATCH, MessageStore};

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zalo-echo-recovery-{}.sqlite3",
            zalo_rust::domain::local_message_id().unwrap()
        ));
        drop(MessageStore::open(&path).unwrap());
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
fn seed(path: &Path, account: &str, client: i64, state: &str, server: Option<&str>) {
    Connection::open(path).unwrap().execute(
        "INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata,server_id) VALUES(?1,?2,'d:22',1,2,?3,?2,?4,?5,?6,?7)",
        params![account,client,format!("Tin tổng hợp {client}"),state,b"exact-ciphertext".as_slice(),b"exact-metadata".as_slice(),server],
    ).unwrap();
}
fn count(path: &Path, account: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM messages WHERE account_id=?1",
            [account],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn restart_repairs_only_current_account_acknowledged_chat_and_keeps_exact_bytes() {
    let database = Database::new();
    seed(&database.0, "11", 100, "acknowledged", Some("9001"));
    seed(&database.0, "33", 101, "acknowledged", Some("9002"));
    seed(&database.0, "11", 102, "acknowledged", Some("init-102"));
    seed(&database.0, "11", 103, "sending", None);
    seed(&database.0, "11", 104, "unknown", Some("9004"));
    seed(&database.0, "11", 105, "ready", None);
    Connection::open(&database.0)
        .unwrap()
        .execute_batch(
            "INSERT INTO e2ee_records VALUES('11','synthetic-ratchet',X'01020304');
         INSERT INTO receive_cursors VALUES('11','synthetic-queue','12345');",
        )
        .unwrap();
    let mut store = MessageStore::open(&database.0).unwrap();
    let repaired = store
        .e2ee_repair_acknowledged("11", None, MAX_ACK_REPAIR_BATCH)
        .unwrap();
    assert_eq!(repaired.repaired, 1);
    drop(store);
    assert_eq!(count(&database.0, "11"), 1);
    assert_eq!(count(&database.0, "33"), 0);
    let conn = Connection::open(&database.0).unwrap();
    let message: (String, i64) = conn
        .query_row(
            "SELECT body,sent_at_ms FROM messages WHERE account_id='11' AND message_id='9001'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(message, ("Tin tổng hợp 100".into(), 100));
    for (client, expected) in [
        (100, "committed"),
        (102, "acknowledged"),
        (103, "sending"),
        (104, "unknown"),
        (105, "ready"),
    ] {
        let row:(String,Vec<u8>,Vec<u8>)=conn.query_row("SELECT state,ciphertext,metadata FROM e2ee_outbox WHERE account='11' AND client_id=?1",[client],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(
            row,
            (
                expected.into(),
                b"exact-ciphertext".to_vec(),
                b"exact-metadata".to_vec()
            )
        );
    }
    assert_eq!(
        conn.query_row(
            "SELECT value FROM e2ee_records WHERE account='11' AND key='synthetic-ratchet'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        conn.query_row(
            "SELECT last_id FROM receive_cursors WHERE account_id='11' AND queue='synthetic-queue'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "12345"
    );
    drop(conn);
    let mut reopened = MessageStore::open(&database.0).unwrap();
    assert_eq!(
        reopened
            .e2ee_repair_acknowledged("11", None, MAX_ACK_REPAIR_BATCH)
            .unwrap()
            .repaired,
        0
    );
    assert_eq!(count(&database.0, "11"), 1);
}

#[test]
fn batches_advance_by_key_without_losing_rows_across_restart() {
    let database = Database::new();
    for client in 1..=MAX_ACK_REPAIR_BATCH as i64 + 3 {
        seed(
            &database.0,
            "11",
            client,
            "acknowledged",
            Some(&(9000 + client).to_string()),
        );
    }
    let mut store = MessageStore::open(&database.0).unwrap();
    let first = store
        .e2ee_repair_acknowledged("11", None, MAX_ACK_REPAIR_BATCH)
        .unwrap();
    assert_eq!(first.repaired, MAX_ACK_REPAIR_BATCH);
    assert_eq!(first.last_client_id, Some(MAX_ACK_REPAIR_BATCH as u64));
    drop(store);
    let mut store = MessageStore::open(&database.0).unwrap();
    let second = store
        .e2ee_repair_acknowledged("11", first.last_client_id, MAX_ACK_REPAIR_BATCH)
        .unwrap();
    assert_eq!(second.repaired, 3);
    assert_eq!(
        store
            .e2ee_repair_acknowledged("11", second.last_client_id, MAX_ACK_REPAIR_BATCH)
            .unwrap()
            .repaired,
        0
    );
    assert_eq!(count(&database.0, "11"), MAX_ACK_REPAIR_BATCH as i64 + 3);
}

#[test]
fn local_insert_failure_keeps_ack_for_later_restart_without_rollback_of_prior_repairs() {
    let database = Database::new();
    seed(&database.0, "11", 1, "acknowledged", Some("9001"));
    seed(&database.0, "11", 2, "acknowledged", Some("9002"));
    let conn = Connection::open(&database.0).unwrap();
    conn.execute_batch("CREATE TRIGGER recovery_insert_failure BEFORE INSERT ON messages WHEN NEW.message_id='9002' BEGIN SELECT RAISE(ABORT,'synthetic insertion failure'); END;").unwrap();
    drop(conn);
    let mut store = MessageStore::open(&database.0).unwrap();
    assert!(store
        .e2ee_repair_acknowledged("11", None, MAX_ACK_REPAIR_BATCH)
        .is_err());
    drop(store);
    assert_eq!(count(&database.0, "11"), 1);
    let conn = Connection::open(&database.0).unwrap();
    let state: String = conn
        .query_row(
            "SELECT state FROM e2ee_outbox WHERE account='11' AND client_id=2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "acknowledged");
    conn.execute_batch("DROP TRIGGER recovery_insert_failure")
        .unwrap();
    drop(conn);
    let mut store = MessageStore::open(&database.0).unwrap();
    assert_eq!(
        store
            .e2ee_repair_acknowledged("11", None, MAX_ACK_REPAIR_BATCH)
            .unwrap()
            .repaired,
        1
    );
    assert_eq!(count(&database.0, "11"), 2);
}
