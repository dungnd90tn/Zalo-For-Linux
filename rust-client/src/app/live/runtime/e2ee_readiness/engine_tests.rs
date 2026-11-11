//! Independent QA cases using real engine-created sessions and disposable storage.
use super::super::e2ee_init_peer::tests::{seed_identity, wire_bundle};
use super::tests::{check, Mode};
use super::*;
use rusqlite::{types::Value as SqlValue, Connection as Sqlite};
use std::{path::Path, sync::Arc};

fn durable_crypto(path: &Path) -> Vec<Vec<Vec<SqlValue>>> {
    let conn = Sqlite::open(path).unwrap();
    [
        "e2ee_records",
        "e2ee_outbox",
        "e2ee_peer_init_outbox",
        "e2ee_accounts",
        "e2ee_peer_epochs",
        "e2ee_verified_outbox",
        "messages",
        "outgoing",
        "receive_cursors",
    ]
    .iter()
    .map(|table| {
        let mut query = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
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

#[tokio::test]
async fn readiness_rejects_actual_opaque_session_iid_mismatch_without_committing_a_ratchet() {
    let mut bundles = vec![wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await];
    let root = std::env::temp_dir().join(format!(
        "zalo-readiness-opaque-iid-{:x}",
        rand::random::<u64>()
    ));
    let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
    let path = profile.paths.data_dir.join("messages.sqlite");
    seed_identity(&path);
    let db = Database::open(profile, "11".into()).await.unwrap();
    let (initial, _, _) = check(&db, &bundles, Mode::Normal, 31_000_000).await;
    assert!(initial.is_ok());
    // Simulate a ledger/session disagreement: keep the genuine opaque session
    // and identity bytes, but expose a different freshly advertised registration.
    let old = bundles[0]["preKey"]["iKey"]["id"].as_u64().unwrap();
    let changed = if old == 16383 { 1 } else { old + 1 };
    bundles[0]["preKey"]["iKey"]["id"] = json!(changed);
    Sqlite::open(&path).unwrap().execute(
        "UPDATE e2ee_peer_epochs SET registration_id=?1 WHERE account='11' AND uid='22' AND platform=0 AND active=1",
        [changed as i64],
    ).unwrap();
    let (ready, seen, _) = check(&db, &bundles, Mode::Normal, 31_000_128).await;
    assert!(seen.iter().all(|(command, _)| *command != 543));
    let ready = ready.unwrap();
    let before = durable_crypto(&path);
    let error = super::super::e2ee_send::prepare_checked(
        &db,
        "11",
        "22",
        "Tin tổng hợp không được commit",
        1_700_000_001_000,
        Arc::new(ready),
    )
    .await
    .err()
    .expect("Opaque session IID must still be checked");
    // The real engine checks its opaque session before returning ciphertext, so
    // this mismatch is rejected before the additional prepared-IID guard.
    assert_eq!(
        error.to_string(),
        "Signal engine rejected operation: IdentityChanged"
    );
    assert!(
        durable_crypto(&path) == before,
        "The temporary reservation is removed and no ratchet/ciphertext is committed"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn readiness_unknown_typed_init_survives_restart_and_blocks_cached_session_reuse() {
    let bundles = vec![wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await];
    let root = std::env::temp_dir().join(format!(
        "zalo-readiness-unknown-init-{:x}",
        rand::random::<u64>()
    ));
    let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
    let path = profile.paths.data_dir.join("messages.sqlite");
    seed_identity(&path);
    let mut db = Database::open(profile, "11".into()).await.unwrap();
    let (first, seen, _) = check(&db, &bundles, Mode::StateDuringInit, 32_000_000).await;
    assert!(first.is_err());
    assert_eq!(
        seen.iter().filter(|(command, _)| *command == 543).count(),
        1
    );
    let before = durable_crypto(&path);
    assert!(
        before[0].len() > 1,
        "The real handshake already persisted session records"
    );
    assert_eq!(
        before[2].len(),
        1,
        "Initialization intent must be typed and durable"
    );
    // Simulate a completed process shutdown, including its storage actor. The
    // production Drop deliberately detaches; a reopen must wait for lock release.
    db.jobs.take();
    db.thread.take().unwrap().join().unwrap();
    drop(db);
    let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
    let db = Database::open(profile, "11".into()).await.unwrap();
    let (retry, seen, events) = check(&db, &bundles, Mode::Normal, 32_000_128).await;
    assert!(
        retry.is_err(),
        "A cached session is not evidence that its unknown initialization succeeded"
    );
    assert!(
        seen.is_empty(),
        "Pending local initialization blocks before any new query or handshake"
    );
    assert!(events
        .iter()
        .all(|event| !matches!(event, LiveEvent::Sent { .. })));
    assert!(
        durable_crypto(&path) == before,
        "Unknown initialization recovery cannot mutate crypto records or the outbox"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(root);
}
