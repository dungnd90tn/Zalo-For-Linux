//! Independent QA cases using real engine-created sessions and disposable storage.
use super::super::e2ee_init_peer::tests::{seed_identity, wire_bundle};
use super::tests::{check, Mode};
use super::*;
use rusqlite::{types::Value as SqlValue, Connection as Sqlite};
use std::{path::Path, sync::Arc};

async fn stored_session(db: &Database, uid: &str, device: e2ee::Platform) -> (String, Value) {
    let uid = uid.to_owned();
    db.call(move |store| {
        let local = e2ee::LocalNamespace {
            epoch: store.e2ee_current_local("11")?.unwrap().to_string(),
            platform: e2ee::Platform::Pc,
        };
        let active = store.e2ee_active_peer("11", &local, &uid, device)?.unwrap();
        let namespace = active.namespace.epoch.clone();
        let record = store
            .e2ee_snapshot(
                "11",
                &[e2ee::RecordKey::Session {
                    local,
                    peer: active.namespace,
                }],
            )?
            .records
            .into_iter()
            .next()
            .unwrap();
        let encoded: String = serde_json::from_slice(&record.value)?;
        Ok((namespace, serde_json::from_str(&encoded)?))
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn readiness_renews_only_the_unknown_iid_device_and_archives_its_previous_session() {
    let bundles = vec![
        wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await,
        wire_bundle(zalo_signal_engine::SignalPlatform::Web, 1).await,
    ];
    let root =
        std::env::temp_dir().join(format!("zalo-renewal-archive-{:x}", rand::random::<u64>()));
    let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
    let path = profile.paths.data_dir.join("messages.sqlite");
    seed_identity(&path);
    let db = Database::open(profile, "11".into()).await.unwrap();
    assert!(check(&db, &bundles, Mode::Normal, 33_000_000)
        .await
        .0
        .is_ok());
    let mobile = stored_session(&db, "22", e2ee::Platform::Mobile).await;
    let web = stored_session(&db, "22", e2ee::Platform::Web).await;
    let before = durable_crypto(&path);
    let (renewed, seen, _) = check(&db, &bundles, Mode::UnknownIid, 33_000_128).await;
    let renewed = renewed.expect("Verified matching bundle must permit bounded session renewal");
    let inits: Vec<_> = seen.iter().filter(|(cmd, _)| *cmd == 543).collect();
    assert_eq!(inits.len(), 1);
    assert_eq!(inits[0].1["uidTo"], "22");
    assert_eq!(inits[0].1["data"].as_array().unwrap().len(), 1);
    assert_eq!(inits[0].1["data"][0]["deviceId"], 0);
    let after_mobile = stored_session(&db, "22", e2ee::Platform::Mobile).await;
    assert_eq!(
        after_mobile.0, mobile.0,
        "Renewal keeps the trusted namespace"
    );
    assert_eq!(stored_session(&db, "22", e2ee::Platform::Web).await, web);
    let old_sessions = mobile.1["sessions"].as_object().unwrap();
    let sessions = after_mobile.1["sessions"].as_object().unwrap();
    assert_eq!(sessions.len(), old_sessions.len() + 1);
    for key in old_sessions.keys() {
        assert!(sessions[key]["indexInfo"]["closed"].as_i64().unwrap() >= 0);
    }
    assert_eq!(
        sessions
            .values()
            .filter(|session| session["indexInfo"]["closed"] == -1)
            .count(),
        1
    );
    assert_eq!(
        durable_crypto(&path)[4],
        before[4],
        "Trust bindings cannot change during same-identity renewal"
    );
    let prepared = super::super::e2ee_send::prepare_checked(
        &db,
        "11",
        "22",
        "Tin tổng hợp sau làm mới phiên",
        1_700_000_003_000,
        Arc::new(renewed),
    )
    .await
    .unwrap();
    assert_eq!(prepared.devices, 2);
    let (_, seen, _) = check(&db, &bundles, Mode::Normal, 33_000_256).await;
    assert!(
        seen.iter().all(|(cmd, _)| *cmd != 543),
        "Known metadata does not repeat renewal"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn unknown_metadata_never_authorizes_changed_registration_or_an_invalid_signed_bundle() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bundles = vec![wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await];
    let root =
        std::env::temp_dir().join(format!("zalo-renewal-reject-{:x}", rand::random::<u64>()));
    let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
    let path = profile.paths.data_dir.join("messages.sqlite");
    seed_identity(&path);
    let db = Database::open(profile, "11".into()).await.unwrap();
    assert!(check(&db, &bundles, Mode::Normal, 34_000_000)
        .await
        .0
        .is_ok());
    for case in 0..3 {
        let mut invalid = bundles.clone();
        match case {
            0 => {
                let old = invalid[0]["preKey"]["iKey"]["id"].as_u64().unwrap();
                invalid[0]["preKey"]["iKey"]["id"] = json!((old + 1) % 16384);
            }
            1 => invalid[0]["preKey"]["signature"] = json!(STANDARD.encode([0u8; 64])),
            _ => invalid[0] = wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await,
        }
        let before = durable_crypto(&path);
        let (result, seen, _) =
            check(&db, &invalid, Mode::UnknownIid, 34_000_128 + case * 128).await;
        assert!(result.is_err());
        assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
        assert_eq!(
            durable_crypto(&path),
            before,
            "Refused renewal cannot modify durable crypto state"
        );
    }
    // An incoming prekey can make the opaque session newer than the binding
    // timestamp. The original then keeps that session open and archives the
    // proposed one: readiness must not mistake this for a completed renewal.
    let (key, timestamp) = db
        .call(|store| {
            let local = e2ee::LocalNamespace {
                epoch: store.e2ee_current_local("11")?.unwrap().to_string(),
                platform: e2ee::Platform::Pc,
            };
            let active = store
                .e2ee_active_peer("11", &local, "22", e2ee::Platform::Mobile)?
                .unwrap();
            Ok((
                serde_json::to_string(&e2ee::RecordKey::SessionTimestamp {
                    local,
                    peer: active.namespace,
                })?,
                active.session_timestamp + 1,
            ))
        })
        .await
        .unwrap();
    let conn = Sqlite::open(&path).unwrap();
    assert_eq!(
        conn.execute(
            "UPDATE e2ee_records SET value=?1 WHERE account='11' AND key=?2",
            rusqlite::params![serde_json::to_vec(&timestamp).unwrap(), key]
        )
        .unwrap(),
        1
    );
    drop(conn);
    let before = durable_crypto(&path);
    let (result, seen, _) = check(&db, &bundles, Mode::UnknownIid, 34_000_640).await;
    assert!(
        result.is_err(),
        "An older binding timestamp cannot authorize session renewal"
    );
    assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
    assert_eq!(durable_crypto(&path), before);
    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn interrupted_renewal_and_own_device_init_never_authorize_chat_or_repeat_the_handshake() {
    for (mode, target) in [
        (Mode::UnknownIidStateDuringInit, "22"),
        (Mode::OwnStateDuringInit, "11"),
    ] {
        let bundles = vec![
            wire_bundle(zalo_signal_engine::SignalPlatform::Mobile, 0).await,
            wire_bundle(zalo_signal_engine::SignalPlatform::Web, 1).await,
        ];
        let root = std::env::temp_dir().join(format!(
            "zalo-renewal-interrupt-{:x}",
            rand::random::<u64>()
        ));
        let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
        let path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        assert!(check(&db, &bundles, Mode::Normal, 35_000_000)
            .await
            .0
            .is_ok());
        let (result, seen, events) = check(&db, &bundles, mode, 35_000_128).await;
        assert!(
            result.is_err(),
            "A session state transition invalidates the initialization wait"
        );
        let inits: Vec<_> = seen.iter().filter(|(cmd, _)| *cmd == 543).collect();
        assert_eq!(inits.len(), 1);
        assert_eq!(inits[0].1["uidTo"], target);
        assert!(events
            .iter()
            .all(|event| !matches!(event, LiveEvent::Sent { .. })));
        let before = durable_crypto(&path);
        let (retry, seen, _) = check(&db, &bundles, Mode::Normal, 35_000_256).await;
        assert!(retry.is_err());
        assert!(
            seen.is_empty(),
            "Unknown initialization blocks before any repeated network operation"
        );
        assert_eq!(durable_crypto(&path), before);
        drop(db);
        let _ = std::fs::remove_dir_all(root);
    }
}

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
