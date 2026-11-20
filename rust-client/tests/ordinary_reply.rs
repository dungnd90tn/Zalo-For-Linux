//! Independent ordinary reply persistence and routing checks with disposable data.
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::json;
use std::path::{Path, PathBuf};
use zalo_rust::{
    domain::{Message, QuoteContent},
    store::MessageStore,
};

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "zalo-ordinary-reply-{}.sqlite3",
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
fn message(
    account: &str,
    thread: &str,
    id: &str,
    sender: &str,
    text: &str,
    client: Option<&str>,
) -> Message {
    Message {
        mentions: Vec::new(),
        reply_blocked: false,
        recalled: false,
        account_id: account.into(),
        thread_id: thread.into(),
        id: id.into(),
        sender: sender.into(),
        text: text.into(),
        source_client_id: client.map(str::to_owned),
        quote: None,
        sticker: None,
        media: None,
        sent_at_ms: 1_700_000_000_123,
    }
}
fn rows(conn: &Connection, table: &str) -> Vec<Vec<SqlValue>> {
    assert!([
        "messages",
        "conversations",
        "outgoing",
        "outgoing_counters",
        "receive_cursors",
        "e2ee_records",
        "e2ee_outbox",
        "e2ee_peer_init_outbox"
    ]
    .contains(&table));
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn reply_target_is_reread_under_its_exact_account_and_thread() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    store
        .insert_batch(&[
            message(
                "11",
                "d:22",
                "9001",
                "22",
                "Tin riêng tổng hợp",
                Some("8001"),
            ),
            message(
                "11",
                "g:77",
                "9001",
                "33",
                "Tin nhóm tổng hợp",
                Some("8002"),
            ),
            message(
                "44",
                "d:22",
                "9001",
                "22",
                "Tài khoản tổng hợp khác",
                Some("8003"),
            ),
            message("11", "d:22", "9002", "22", "Tin cũ thiếu client ID", None),
        ])
        .unwrap();
    let direct = store
        .prepare_outgoing_reply("11", "d:22", "Trả lời riêng", "9001", 1000)
        .unwrap();
    let group = store
        .prepare_outgoing_reply("11", "g:77", "Trả lời nhóm", "9001", 1001)
        .unwrap();
    let direct_quote = store.outgoing_quote("11", direct).unwrap().unwrap();
    let group_quote = store.outgoing_quote("11", group).unwrap().unwrap();
    assert_eq!(
        (
            direct_quote.owner_id.as_str(),
            direct_quote.text.as_str(),
            direct_quote.client_id.as_deref()
        ),
        ("22", "Tin riêng tổng hợp", Some("8001"))
    );
    assert_eq!(
        (
            group_quote.owner_id.as_str(),
            group_quote.text.as_str(),
            group_quote.client_id.as_deref()
        ),
        ("33", "Tin nhóm tổng hợp", Some("8002"))
    );
    assert!(store.outgoing_quote("44", direct).unwrap().is_none());
    let before = rows(&Connection::open(&db.0).unwrap(), "outgoing");
    for (account, thread, target) in [
        ("55", "d:22", "9001"),
        ("11", "g:88", "9001"),
        ("11", "d:22", "9002"),
        ("11", "d:22", "9999"),
    ] {
        assert!(store
            .prepare_outgoing_reply(account, thread, "Không được tạo lượt gửi", target, 1002)
            .is_err());
    }
    assert_eq!(rows(&Connection::open(&db.0).unwrap(), "outgoing"), before);
}

#[test]
fn quote_survives_unknown_restart_ack_repair_and_own_echo_without_quote() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    store
        .insert_batch(&[message(
            "11",
            "d:22",
            "9001",
            "22",
            "Tin gốc tiếng Việt\n😀",
            Some("8001"),
        )])
        .unwrap();
    let id = store
        .prepare_outgoing_reply("11", "d:22", "Trả lời đã lưu", "9001", 1000)
        .unwrap();
    let quote = store.outgoing_quote("11", id).unwrap().unwrap();
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    store.recover_outgoing("11").unwrap();
    assert_eq!(store.outgoing_quote("11", id).unwrap(), Some(quote.clone()));
    store.record_outgoing_ack("11", id, "99001").unwrap();
    store.reconcile_outgoing("11").unwrap();
    let echo = message(
        "11",
        "d:22",
        "99001",
        "11",
        "Trả lời đã lưu",
        Some(&id.to_string()),
    );
    store
        .receive_batch(
            "11",
            &[(echo, Some(id.to_string()))],
            Some(("fixture", "101")),
        )
        .unwrap();
    drop(store);
    let store = MessageStore::open(&db.0).unwrap();
    let history = store.page("11", "d:22", None, 10).unwrap();
    let echoes: Vec<_> = history
        .iter()
        .filter(|message| message.id == "99001")
        .collect();
    assert_eq!(echoes.len(), 1);
    assert_eq!(echoes[0].quote, Some(quote));
    assert_eq!(
        echoes[0].source_client_id.as_deref(),
        Some(id.to_string().as_str())
    );
}

#[test]
fn consistent_replay_enriches_metadata_but_conflict_does_not_advance_cursor() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let old = message("11", "g:77", "9001", "22", "Tin mới có trích dẫn", None);
    store
        .receive_batch("11", &[(old.clone(), None)], Some(("fixture", "100")))
        .unwrap();
    let content = QuoteContent::new("11", &json!({
        "ownerId":"33","globalMsgId":"7001","cliMsgId":"6001","cliMsgType":1,"ts":100,"msg":"Trích dẫn tổng hợp",
        "opaque":{"nested":[1,2,3]},"ttl":0
    })).unwrap();
    let quote = content.projection("11").unwrap();
    let mut enriched = old;
    enriched.source_client_id = Some("8001".into());
    enriched.quote = Some(quote.clone());
    let published = store
        .receive_batch_with_replies(
            "11",
            &[(enriched.clone(), None, None, None, Some(content.clone()))],
            Some(("fixture", "101")),
        )
        .unwrap();
    assert_eq!(
        published.len(),
        1,
        "New metadata must refresh an already visible message"
    );
    let mut conflict = enriched;
    conflict.source_client_id = Some("8002".into());
    assert!(store
        .receive_batch("11", &[(conflict, None)], Some(("fixture", "102")))
        .is_err());
    drop(store);
    let store = MessageStore::open(&db.0).unwrap();
    let loaded = &store.page("11", "g:77", None, 10).unwrap()[0];
    assert_eq!(loaded.source_client_id.as_deref(), Some("8001"));
    assert_eq!(loaded.quote, Some(quote.clone()));
    assert_eq!(
        store
            .message_quote_content("11", "g:77", "9001")
            .unwrap()
            .unwrap()
            .as_json(),
        content.as_json()
    );
    let conn = Connection::open(&db.0).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT last_id FROM receive_cursors WHERE account_id='11' AND queue='fixture'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "101"
    );
}

#[test]
fn explicit_ineligibility_revokes_reply_but_missing_client_metadata_does_not() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    let source = message("11", "d:22", "9001", "22", "Nguồn tổng hợp", Some("8001"));
    store
        .receive_batch("11", &[(source.clone(), None)], None)
        .unwrap();
    let mut absent = source.clone();
    absent.source_client_id = None;
    store
        .receive_batch("11", &[(absent.clone(), None)], None)
        .unwrap();
    assert!(store.page("11", "d:22", None, 10).unwrap()[0]
        .reply_reference()
        .is_some());
    let mut ineligible = absent.clone();
    // The protocol marks nonzero/unknown TTL, encrypted or styled sources explicitly.
    ineligible.reply_blocked = true;
    let published = store
        .receive_batch("11", &[(ineligible, None)], None)
        .unwrap();
    assert_eq!(published.len(), 1);
    drop(store);
    let mut store = MessageStore::open(&db.0).unwrap();
    let loaded = &store.page("11", "d:22", None, 10).unwrap()[0];
    assert!(loaded.reply_blocked && loaded.reply_reference().is_none());
    assert_eq!(
        loaded.source_client_id.as_deref(),
        Some("8001"),
        "Actual correlation data is retained"
    );
    store.receive_batch("11", &[(absent, None)], None).unwrap();
    store.receive_batch("11", &[(source, None)], None).unwrap();
    assert!(store.page("11", "d:22", None, 10).unwrap()[0]
        .reply_reference()
        .is_none());
    assert!(store
        .prepare_outgoing_reply("11", "d:22", "Không gửi", "9001", 1000)
        .unwrap_err()
        .is::<zalo_rust::domain::reply::ReplyUnavailable>());
}

#[test]
fn a_quote_sidecar_failure_rolls_back_reservation_and_ack_echo_keeps_retryable_metadata() {
    let db = Database::new();
    let mut store = MessageStore::open(&db.0).unwrap();
    store
        .insert_batch(&[message(
            "11",
            "d:22",
            "9001",
            "22",
            "Nguồn tổng hợp",
            Some("8001"),
        )])
        .unwrap();
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_outgoing_quote BEFORE INSERT ON outgoing_reply BEGIN SELECT RAISE(ABORT,'synthetic quote failure'); END;").unwrap();
    assert!(store
        .prepare_outgoing_reply("11", "d:22", "Trả lời nguyên tử", "9001", 1000)
        .is_err());
    assert!(rows(&conn, "outgoing").is_empty());
    assert!(rows(&conn, "outgoing_counters").is_empty());
    conn.execute_batch("DROP TRIGGER reject_outgoing_quote;")
        .unwrap();
    let id = store
        .prepare_outgoing_reply("11", "d:22", "Trả lời nguyên tử", "9001", 1000)
        .unwrap();
    let quote = store.outgoing_quote("11", id).unwrap().unwrap();
    store.record_outgoing_ack("11", id, "99001").unwrap();
    conn.execute_batch("CREATE TRIGGER reject_message_quote BEFORE INSERT ON message_reply BEGIN SELECT RAISE(ABORT,'synthetic echo failure'); END;").unwrap();
    assert!(store.reconcile_outgoing("11").is_err());
    assert_eq!(store.outgoing_quote("11", id).unwrap(), Some(quote.clone()));
    assert_eq!(
        conn.query_row(
            "SELECT state FROM outgoing WHERE account_id='11' AND client_id=?1",
            [id.to_string()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "acknowledged"
    );
    conn.execute_batch("DROP TRIGGER reject_message_quote;")
        .unwrap();
    store.reconcile_outgoing("11").unwrap();
    assert_eq!(
        store
            .page("11", "d:22", None, 10)
            .unwrap()
            .iter()
            .find(|message| message.id == "99001")
            .unwrap()
            .quote,
        Some(quote)
    );
}

const OLD_TABLES: &[&str] = &[
    "messages",
    "conversations",
    "outgoing",
    "outgoing_counters",
    "receive_cursors",
    "e2ee_records",
    "e2ee_outbox",
    "e2ee_peer_init_outbox",
];
fn seed_v9(path: &Path) {
    drop(MessageStore::open(path).unwrap());
    Connection::open(path).unwrap().execute_batch("BEGIN IMMEDIATE;
        DROP TABLE attachment_outbox; DROP TABLE outgoing_reply; DROP TABLE message_reply;
        INSERT INTO conversations VALUES('11','d:22','Hội thoại tổng hợp',100);
        INSERT INTO messages VALUES('11','d:22','9001','22','Tin cũ không có cliMsgId',100);
        INSERT INTO outgoing VALUES('11','d:22','8001','Bản nháp chưa rõ kết quả',100,'unknown',NULL);
        INSERT INTO outgoing_counters VALUES('11',8001);
        INSERT INTO receive_cursors VALUES('11','fixture','101');
        INSERT INTO e2ee_records VALUES('11','synthetic-ratchet',X'01020304');
        INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state,ciphertext,metadata)
          VALUES('11',1,'d:22',1,2,'synthetic init',100,'unknown',X'010203FF',X'7B7D');
        INSERT INTO e2ee_peer_init_outbox VALUES('11',1);
        PRAGMA user_version=9; COMMIT;").unwrap();
}

#[test]
fn schema_nine_upgrade_preserves_all_old_bytes_and_does_not_invent_reply_identity() {
    let db = Database::new();
    seed_v9(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    let before: Vec<_> = OLD_TABLES.iter().map(|table| rows(&conn, table)).collect();
    drop(conn);
    for _ in 0..2 {
        let store = MessageStore::open(&db.0).unwrap();
        let old = &store.page("11", "d:22", None, 10).unwrap()[0];
        assert!(
            old.source_client_id.is_none()
                && old.quote.is_none()
                && old.reply_reference().is_none()
        );
        let conn = Connection::open(&db.0).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
        for (table, expected) in OLD_TABLES.iter().zip(&before) {
            assert_eq!(&rows(&conn, table), expected, "Changed {table}");
        }
        assert_eq!(
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }
}

#[test]
fn failed_schema_nine_upgrade_rolls_back_both_sidecars_and_refuses_future_schema() {
    let db = Database::new();
    seed_v9(&db.0);
    let conn = Connection::open(&db.0).unwrap();
    let before: Vec<_> = OLD_TABLES.iter().map(|table| rows(&conn, table)).collect();
    conn.execute_batch("CREATE TRIGGER reject_reply_upgrade BEFORE INSERT ON store_markers BEGIN SELECT RAISE(ABORT,'synthetic migration failure'); END;").unwrap();
    assert!(MessageStore::open(&db.0).is_err());
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('message_reply','outgoing_reply')", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    for (table, expected) in OLD_TABLES.iter().zip(&before) {
        assert_eq!(&rows(&conn, table), expected);
    }
    conn.execute_batch("DROP TRIGGER reject_reply_upgrade; PRAGMA user_version=21;")
        .unwrap();
    assert!(MessageStore::open(&db.0).is_err());
}
