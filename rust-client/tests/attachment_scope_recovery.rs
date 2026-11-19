//! Startup recovery must never advance another account's attachment ledger.
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::json;
use zalo_rust::{
    domain::{MediaKind, MediaMessageContent},
    store::{
        attachments::{AttachmentResource, AttachmentSpec, AttachmentState},
        MessageStore,
    },
};

fn spec() -> AttachmentSpec {
    AttachmentSpec {
        file_name: "synthetic.txt".into(),
        size: 6,
        checksum: "a".repeat(32),
    }
}
fn resource(id: u64) -> AttachmentResource {
    let checksum = "a".repeat(32);
    let url = "https://files.example.test/synthetic";
    AttachmentResource {file_id:"resource-7".into(),file_url:url.into(),checksum:checksum.clone(),
        parameters:json!({"fileId":"resource-7","checksum":checksum,"checksumSha":"","extension":"txt","totalSize":6,"fileName":"synthetic.txt","clientId":id,"fType":1,"fileCount":0,"fdata":"{}","fileUrl":url,"ttl":0,"toid":"22","imei":"synthetic-device"}),
        content:MediaMessageContent::new(MediaKind::File,&json!({"title":"synthetic.txt","description":"","href":url,"params":{"fileSize":6,"checksum":checksum,"checksumSha":"","fileExt":"txt","fdata":"{}","fType":1}})).unwrap()}
}
fn account_rows(path: &std::path::Path, account: &str) -> Vec<Vec<SqlValue>> {
    let conn = Connection::open(path).unwrap();
    let mut q = conn
        .prepare("SELECT * FROM attachment_outbox WHERE account_id=?1 ORDER BY client_id")
        .unwrap();
    let columns = q.column_count();
    q.query_map([account], |r| (0..columns).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn startup_recovers_only_the_selected_account_and_never_dispatches_unknown_bytes() {
    for stage in 0..5 {
        let path = std::env::temp_dir().join(format!(
            "zalo-attachment-recovery-{}.sqlite",
            zalo_rust::domain::local_message_id().unwrap()
        ));
        let mut store = MessageStore::open(&path).unwrap();
        let mut ids = Vec::new();
        for account in ["11", "44"] {
            let id = store
                .reserve_attachment(account, "d:22", &spec(), 1000)
                .unwrap();
            ids.push(id);
            if stage > 0 {
                store
                    .attachment_resource_ready(account, id, &resource(id))
                    .unwrap();
            }
            if stage > 1 {
                store.begin_attachment_dispatch(account, id).unwrap();
            }
            if stage == 3 {
                store
                    .record_attachment_ack(account, id, "9001", Some(100))
                    .unwrap();
            }
            if stage == 4 {
                store.fail_attachment_dispatch(account, id, true).unwrap();
            }
        }
        let other = account_rows(&path, "44");
        drop(store);
        let mut reopened = MessageStore::open(&path).unwrap();
        reopened.recover_attachments("11").unwrap();
        reopened.reconcile_attachments("11").unwrap();
        let expected = match stage {
            0 | 1 => AttachmentState::Interrupted,
            3 => AttachmentState::Accepted,
            _ => AttachmentState::Unknown,
        };
        assert_eq!(
            reopened.attachment_state("11", ids[0]).unwrap(),
            Some(expected)
        );
        assert_eq!(
            account_rows(&path, "44"),
            other,
            "Account B's states and exact resource bytes must remain untouched"
        );
        if stage == 2 || stage == 4 {
            assert!(reopened.begin_attachment_dispatch("11", ids[0]).is_err());
            assert!(reopened
                .reserve_attachment("11", "d:22", &spec(), 2000)
                .is_err());
            let mut different = spec();
            different.checksum = "b".repeat(32);
            assert!(
                reopened
                    .reserve_attachment("11", "d:22", &different, 2000)
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
