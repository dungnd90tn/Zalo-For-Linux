//! Independent projection/privacy regressions: the durable journal is not a UI filter.
use super::*;
use crate::domain::{InfoMessageContent, MediaKind, MediaMessageContent, Message};
use crate::store::directory::{DirectoryBatch, DirectoryRecord};
use rusqlite::types::Value as SqlValue;
use serde_json::json;

fn message(account: &str, thread: &str, id: &str, time: i64) -> Message {
    Message {
        account_id: account.into(),
        thread_id: thread.into(),
        id: id.into(),
        sender: "22".into(),
        text: "Nội dung riêng tư không phải kết quả tìm kiếm".into(),
        sticker: None,
        media: None,
        sent_at_ms: time,
    }
}

fn persistent_rows(store: &MessageStore) -> Vec<(String, Vec<Vec<SqlValue>>)> {
    let tables: Vec<String> = store
        .conn
        .prepare("SELECT name FROM main.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut statement = store
                .conn
                .prepare(&format!(
                    "SELECT * FROM main.\"{}\" ORDER BY rowid",
                    table.replace('"', "\"\"")
                ))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

#[test]
fn visibility_is_filtered_before_limits_and_locked_search_has_no_preview() {
    let mut store = MessageStore::in_memory().unwrap();
    let mut hidden = Vec::new();
    let mut entries = Vec::new();
    for number in 1000..1205 {
        let id = number.to_string();
        let thread = format!("d:{id}");
        store
            .insert_batch(&[message("11", &thread, &id, number)])
            .unwrap();
        hidden.push(thread);
        entries.push(DirectoryRecord {
            remote_id: id,
            is_group: false,
            display_name: format!("A private {number}"),
            avatar_url: None,
            is_friend: Some(true),
            is_blocked: None,
            member_count: None,
        });
    }
    for (account, thread, id, time) in [
        ("11", "g:1000", "group", 50),
        ("11", "d:9000", "visible", 1),
        ("33", "d:1000", "other-account", 3000),
        ("11", "d:8000", "no-directory", 2000),
    ] {
        store
            .insert_batch(&[message(account, thread, id, time)])
            .unwrap();
    }
    store.set_title("11", "d:8000", "Nguyễn Thị Ánh").unwrap();
    hidden.push("d:8000".into());
    entries.push(DirectoryRecord {
        remote_id: "9000".into(),
        is_group: false,
        display_name: "Z visible contact".into(),
        avatar_url: None,
        is_friend: Some(true),
        is_blocked: None,
        member_count: None,
    });
    for entries in entries.chunks(200) {
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "11".into(),
                entries: entries.to_vec(),
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                observed_at_ms: 4000,
            })
            .unwrap();
    }
    store.begin_live_privacy("11").unwrap();
    assert!(store.conversations("11", 1).unwrap().is_empty());
    assert!(store
        .directory_page("11", false, "", None, 1)
        .unwrap()
        .is_empty());
    assert!(store.hidden_candidates("11", "nguyen").unwrap().is_empty());
    assert_eq!(store.page("33", "d:1000", None, 1).unwrap().len(), 1);
    store.replace_live_privacy("11", &hidden, None).unwrap();
    assert_eq!(store.conversations("11", 1).unwrap()[0].id, "g:1000");
    let directory = store.directory_page("11", false, "", None, 1).unwrap();
    assert_eq!(directory.len(), 1);
    assert_eq!(directory[0].remote_id, "9000");
    assert!(store
        .directory_page("11", false, "private", None, 200)
        .unwrap()
        .is_empty());
    let candidates = store.hidden_candidates("11", "NGUYEN THI ANH").unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].thread_id, "d:8000");
    assert_eq!(candidates[0].title, "Nguyễn Thị Ánh");
    assert!(store.hidden_candidates("33", "nguyen").unwrap().is_empty());
    assert!(store.hidden_candidates("11", "").unwrap().is_empty());
    assert_eq!(store.hidden_candidates("11", "private").unwrap().len(), 50);
    store.unlock_live_thread("11", Some("d:1000")).unwrap();
    assert_eq!(store.page("11", "d:1000", None, 1).unwrap().len(), 1);
    assert_eq!(store.conversations("11", 1).unwrap()[0].id, "g:1000");
    assert_eq!(
        store.directory_page("11", false, "", None, 1).unwrap()[0].remote_id,
        "9000"
    );
}

#[test]
fn locked_receives_and_ack_keep_raw_history_cursors_and_outbox_without_projection() {
    let mut store = MessageStore::in_memory().unwrap();
    let client = store
        .prepare_outgoing("11", "d:22", "Tin đã gửi trước khi khóa", 1000)
        .unwrap();
    store.begin_live_privacy("11").unwrap();
    store
        .replace_live_privacy("11", &["d:22".into()], None)
        .unwrap();
    assert!(store
        .prepare_outgoing("11", "d:22", "Không được gửi khi khóa", 1001)
        .is_err());
    assert!(store
        .open_conversation("11", "d:22", "Không được mở bằng ID")
        .is_err());
    let raw = MediaMessageContent::new(
        MediaKind::Photo,
        &json!({
            "title":"Ảnh riêng", "href":"https://photo-talk.zadn.vn/synthetic.jpg",
            "unknown":{"preserve":[true,7,{"nested":"full raw metadata"}]}
        }),
    )
    .unwrap();
    let mut photo = message("11", "d:22", "photo", 1002);
    photo.text = "[Tin nhắn chat.photo chưa được hỗ trợ]".into();
    photo.media = Some(raw.descriptor().clone());
    let info = InfoMessageContent::new(&json!({
        "action":"msginfo.actionlist", "title":"Thông tin riêng",
        "params":"{\"msg\":{\"vi\":\"%1$s đã đổi thông tin\"}}",
        "unknown":{"preserve":"complete"}
    }))
    .unwrap();
    let notice = message("11", "d:22", "notice", 1003);
    store
        .receive_batch_with_media(
            "11",
            &[
                (photo.clone(), None, None, Some(raw.clone())),
                (notice.clone(), None, Some(info.clone()), None),
            ],
            Some(("510_1", "100")),
        )
        .unwrap();
    store.record_outgoing_ack("11", client, "accepted").unwrap();
    store
        .accept_outgoing("11", client, "accepted", Some(1004))
        .unwrap();
    assert!(store.page("11", "d:22", None, 50).unwrap().is_empty());
    assert!(store
        .refresh_messages("11", "d:22", &["photo".into()])
        .unwrap()
        .is_empty());
    assert!(store
        .message_media("11", "d:22", "photo")
        .unwrap()
        .is_none());
    assert!(store
        .message_info("11", "d:22", "notice")
        .unwrap()
        .is_none());
    assert_eq!(store.receive_cursor("11", "510_1").unwrap(), "100");
    let before = persistent_rows(&store);
    store.begin_live_privacy("11").unwrap();
    store
        .replace_live_privacy("11", &["d:22".into()], Some("d:22"))
        .unwrap();
    assert_eq!(
        store.message_media("11", "d:22", "photo").unwrap(),
        Some(raw)
    );
    assert_eq!(
        store.message_info("11", "d:22", "notice").unwrap(),
        Some(info)
    );
    assert_eq!(store.page("11", "d:22", None, 50).unwrap().len(), 3);
    store.unlock_live_thread("11", None).unwrap();
    assert_eq!(
        persistent_rows(&store),
        before,
        "privacy transitions never rewrite/delete durable rows"
    );
    assert_eq!(
        store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(
        store
            .conn
            .query_row("PRAGMA temp_store", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .conn
            .query_row(
                "SELECT state FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "accepted"
    );
}

#[test]
fn unlock_lease_is_not_persisted_and_only_hidden_ids_are_remembered_on_reopen() {
    struct Disposable(std::path::PathBuf);
    impl Drop for Disposable {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = Disposable(std::env::temp_dir().join(format!(
        "zalo-privacy-{}.sqlite3",
        crate::domain::local_message_id().unwrap()
    )));
    let before;
    {
        let mut store = MessageStore::open(&path.0).unwrap();
        store
            .insert_batch(&[message("11", "d:22", "one", 1000)])
            .unwrap();
        before = persistent_rows(&store);
        store.begin_live_privacy("11").unwrap();
        store
            .replace_live_privacy("11", &["d:22".into()], Some("d:22"))
            .unwrap();
        assert_eq!(store.page("11", "d:22", None, 1).unwrap().len(), 1);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM main.sqlite_master WHERE name LIKE 'live_%'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    let mut store = MessageStore::open(&path.0).unwrap();
    // Only the confirmed hidden thread IDs are remembered: no lease, no message or preview data.
    let after: Vec<_> = persistent_rows(&store)
        .into_iter()
        .filter(|(table, _)| table != "privacy_hidden_cache")
        .collect();
    let before: Vec<_> = before
        .into_iter()
        .filter(|(table, _)| table != "privacy_hidden_cache")
        .collect();
    assert_eq!(after, before);
    assert_eq!(
        store
            .conn
            .query_row(
                "SELECT group_concat(account_id||'/'||thread_id) FROM privacy_hidden_cache",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "11/d:22"
    );
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM temp.live_privacy", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    // The runtime opts in before publishing cached history on every new worker: the remembered set hides d:22
    // at once and the earlier unlock does not carry over.
    assert_eq!(
        store.provisional_live_privacy("11").unwrap(),
        vec!["d:22".to_owned()]
    );
    assert!(store.page("11", "d:22", None, 1).unwrap().is_empty());
    assert!(store.conversations("11", 1).unwrap().is_empty());
}
