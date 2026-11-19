//! Startup recovery must never advance another account's photo ledger.
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::json;
use zalo_rust::{
    domain::{MediaKind, MediaMessageContent},
    store::{
        photos::{PhotoResource, PhotoSpec, PhotoState},
        MessageStore,
    },
};

fn spec() -> PhotoSpec {
    PhotoSpec {
        caption: "Ảnh tổng hợp".into(),
        width: 32,
        height: 16,
        size: 6,
        checksum: "a".repeat(32),
    }
}
fn resource(id: u64) -> PhotoResource {
    let url = "https://photos.example.test/synthetic";
    PhotoResource{photo_id:"resource-7".into(),parameters:json!({"photoId":"resource-7","clientId":id,"desc":"Ảnh tổng hợp","width":32,"height":16,"rawUrl":url,"thumbUrl":url,"hdUrl":url,"normalUrl":url,"language":"VN","imei":"synthetic-device","ttl":0,"toid":"22"}),content:MediaMessageContent::new(MediaKind::Photo,&json!({"title":"","description":"Ảnh tổng hợp","href":url,"thumb":url,"params":{"width":32,"height":16,"hd":url,"rawUrl":url}})).unwrap()}
}
fn account_rows(path: &std::path::Path, account: &str) -> Vec<Vec<SqlValue>> {
    let conn = Connection::open(path).unwrap();
    let mut q = conn
        .prepare("SELECT * FROM photo_outbox WHERE account_id=?1 ORDER BY client_id")
        .unwrap();
    let columns = q.column_count();
    q.query_map([account], |r| (0..columns).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn photo_startup_recovers_only_selected_account_and_preserves_foreign_ledger_bytes() {
    for stage in 0..5 {
        let path = std::env::temp_dir().join(format!(
            "zalo-photo-recovery-{}.sqlite",
            zalo_rust::domain::local_message_id().unwrap()
        ));
        let mut store = MessageStore::open(&path).unwrap();
        let mut ids = Vec::new();
        for account in ["11", "44"] {
            let id = store.reserve_photo(account, "d:22", &spec(), 1000).unwrap();
            ids.push(id);
            if stage > 0 {
                store
                    .photo_resource_ready(account, id, &resource(id))
                    .unwrap();
            }
            if stage > 1 {
                store.begin_photo_dispatch(account, id).unwrap();
            }
            if stage == 3 {
                store
                    .record_photo_ack(account, id, "9001", Some(100))
                    .unwrap();
            }
            if stage == 4 {
                store.fail_photo_dispatch(account, id, true).unwrap();
            }
        }
        let other = account_rows(&path, "44");
        drop(store);
        let mut reopened = MessageStore::open(&path).unwrap();
        reopened.recover_photos("11").unwrap();
        reopened.reconcile_photos("11").unwrap();
        let expected = match stage {
            0 | 1 => PhotoState::Interrupted,
            3 => PhotoState::Accepted,
            _ => PhotoState::Unknown,
        };
        assert_eq!(reopened.photo_state("11", ids[0]).unwrap(), Some(expected));
        assert_eq!(
            account_rows(&path, "44"),
            other,
            "Account B's states and exact resource bytes must remain untouched"
        );
        if stage == 2 || stage == 4 {
            assert!(reopened.begin_photo_dispatch("11", ids[0]).is_err());
            assert!(reopened.reserve_photo("11", "d:22", &spec(), 2000).is_err());
            let mut different = spec();
            different.checksum = "b".repeat(32);
            assert!(
                reopened
                    .reserve_photo("11", "d:22", &different, 2000)
                    .is_ok(),
                "An unknown file does not block a different explicit file"
            );
        }
        assert!(reopened.page("44", "d:22", None, 10).unwrap().is_empty());
        if stage == 3 {
            assert_eq!(reopened.page("11", "d:22", None, 10).unwrap().len(), 1);
        }
        drop(reopened);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
