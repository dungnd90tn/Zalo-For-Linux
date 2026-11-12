//! Independent real-engine checks for activating first-use trust only at commit.
use super::*;

const INIT: &str = "{\"subDevice\":[]}";

fn candidate(
    store: &mut MessageStore,
    local: &e2ee::LocalNamespace,
    receiver: &Receiver,
) -> Recipient {
    let bundle = &receiver.bundle;
    let platform = match receiver.local.platform {
        engine::SignalPlatform::Mobile => e2ee::Platform::Mobile,
        engine::SignalPlatform::Web => e2ee::Platform::Web,
        engine::SignalPlatform::Pc => e2ee::Platform::Pc,
    };
    let (peer, timestamp) = store
        .e2ee_reserve_first_use_peer(
            "101",
            local,
            "202",
            &e2ee::VerifiedBinding {
                platform,
                registration_id: bundle.registration_id,
                identity_key: bundle.identity_key,
                session_timestamp: 100,
            },
        )
        .unwrap();
    Recipient {
        trust: engine::TrustDecision {
            peer: serde_json::from_value(serde_json::to_value(peer).unwrap()).unwrap(),
            registration_id: bundle.registration_id,
            identity_key: bundle.identity_key,
            session_timestamp: timestamp,
            basis: engine::TrustBasis::AuthenticatedFirstUse,
        },
        bundle: Some(engine::PeerBundle {
            registration_id: bundle.registration_id,
            identity_key: bundle.identity_key,
            signed_prekey_id: bundle.signed_prekey_id,
            signed_prekey: bundle.signed_prekey,
            signature: bundle.signature,
            prekey: bundle.prekeys.first().copied(),
        }),
    }
}

fn untrusted(
    path: &std::path::Path,
    store: &mut MessageStore,
    local: &e2ee::LocalNamespace,
    id: u64,
) {
    assert!(store
        .e2ee_active_peer("101", local, "202", e2ee::Platform::Mobile)
        .unwrap()
        .is_none());
    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM e2ee_peer_epochs WHERE active=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM e2ee_records WHERE account='101'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "Only the pre-existing local identity may persist"
    );
    let outgoing = store.e2ee_outgoing("101", id).unwrap();
    assert!(outgoing.state == e2ee::SendState::Reserved);
    assert!(outgoing.ciphertext.is_none());
}

#[tokio::test]
async fn first_use_requires_a_typed_init_and_exact_candidate_with_a_bundle() {
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Mobile).await;
    let (mut store, local, _) = fixture_store(&path);
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let template = candidate(&mut store, &local, &bob);
    let copy = || copy_recipient(&template, bob.bundle.prekeys[0]);
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", INIT, 1000)
        .unwrap();
    assert!(
        coordinator
            .plan_personal(&mut store, id, INIT, vec![copy()], &gate, &cancel)
            .is_err(),
        "The ordinary composer cannot turn an inactive candidate into trust"
    );
    let recipient = copy();
    assert!(
        coordinator
            .plan_personal_init(&mut store, id, INIT, vec![recipient], &gate, &cancel)
            .is_err(),
        "An unmarked reservation is not an initialization"
    );
    store.e2ee_mark_peer_init("101", &local, "202", id).unwrap();
    let mut no_bundle = copy();
    no_bundle.bundle = None;
    assert!(coordinator
        .plan_personal_init(&mut store, id, INIT, vec![no_bundle], &gate, &cancel)
        .is_err());
    let mut wrong = copy();
    wrong.trust.identity_key[1] ^= 1;
    assert!(coordinator
        .plan_personal_init(&mut store, id, INIT, vec![wrong], &gate, &cancel)
        .is_err());
    untrusted(&path, &mut store, &local, id);
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn first_use_cancel_conflict_and_sql_abort_leave_all_candidates_inactive() {
    for fault in 0..5 {
        let path = ephemeral_path();
        let bob = receiver(engine::SignalPlatform::Mobile).await;
        let (mut store, local, _) = fixture_store(&path);
        let gate = e2ee::GenerationGate::new(1).unwrap();
        let cancel = Cancellation::default();
        let mut coordinator = Coordinator::new(
            engine::AccountId::new("101").unwrap(),
            1,
            local_engine(&local),
        )
        .unwrap();
        let recipient = candidate(&mut store, &local, &bob);
        let id = store
            .e2ee_reserve_send("101", &local, "d:202", INIT, 1000)
            .unwrap();
        store.e2ee_mark_peer_init("101", &local, "202", id).unwrap();
        let plan = coordinator
            .plan_personal_init(&mut store, id, INIT, vec![recipient], &gate, &cancel)
            .unwrap();
        let prepared = coordinator
            .prepare_personal(plan, &gate, &cancel)
            .await
            .unwrap();
        untrusted(&path, &mut store, &local, id);
        match fault {
            0 => cancel.cancel(),
            1 => { store.e2ee_reserve_prekey_ids("101", &local, 1).unwrap(); }
            2 => rusqlite::Connection::open(&path).unwrap().execute_batch(
                "CREATE TRIGGER reject_first_use_ready BEFORE UPDATE OF state ON e2ee_outbox WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'synthetic commit failure'); END;"
            ).unwrap(),
            3 => gate.replace(2).unwrap(),
            _ => {
                let fixture: serde_json::Value = serde_json::from_str(include_str!("../fixtures/e2ee/session.json")).unwrap();
                store.e2ee_register_local("101", Some(1), &e2ee::VerifiedBinding {
                    platform: e2ee::Platform::Pc, registration_id: 1001,
                    identity_key: bytes(fixture["alice_identity_public"].as_str().unwrap()).try_into().unwrap(),
                    session_timestamp: 101,
                }).unwrap();
            }
        }
        assert!(store.e2ee_commit_verified_send(&prepared, &gate).is_err());
        untrusted(&path, &mut store, &local, id);
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn first_use_commit_activates_binding_session_and_exact_init_bytes_atomically_across_restart()
{
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Mobile).await;
    let (mut store, local, alice) = fixture_store(&path);
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let recipient = candidate(&mut store, &local, &bob);
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", INIT, 1000)
        .unwrap();
    store.e2ee_mark_peer_init("101", &local, "202", id).unwrap();
    let plan = coordinator
        .plan_personal_init(&mut store, id, INIT, vec![recipient], &gate, &cancel)
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    untrusted(&path, &mut store, &local, id);
    store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    let active = store
        .e2ee_active_peer("101", &local, "202", e2ee::Platform::Mobile)
        .unwrap()
        .unwrap();
    assert_eq!(active.registration_id, bob.bundle.registration_id);
    assert_eq!(
        store
            .e2ee_snapshot(
                "101",
                &[e2ee::RecordKey::Session {
                    local: local.clone(),
                    peer: active.namespace,
                }]
            )
            .unwrap()
            .records
            .len(),
        1
    );
    let outgoing = store.e2ee_outgoing("101", id).unwrap();
    assert!(outgoing.state == e2ee::SendState::Ready);
    let exact = outgoing.ciphertext.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&exact).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    assert_eq!(
        decrypt(&bob, records[0]["message"].as_str().unwrap(), alice).await,
        INIT.as_bytes()
    );
    store.e2ee_begin_dispatch("101", id, 1, &gate).unwrap();
    store.e2ee_send_failed("101", id, false).unwrap();
    drop(store);
    let mut reopened = MessageStore::open(&path).unwrap();
    assert!(reopened
        .e2ee_peer_init_pending("101", &local, "202")
        .unwrap());
    assert!(
        reopened.e2ee_begin_dispatch("101", id, 1, &gate).is_err(),
        "An unknown init cannot be resent after restart"
    );
    let outgoing = reopened.e2ee_outgoing("101", id).unwrap();
    assert!(outgoing.state == e2ee::SendState::Unknown);
    assert_eq!(outgoing.ciphertext.unwrap(), exact);
    assert!(reopened
        .e2ee_active_peer("101", &local, "202", e2ee::Platform::Mobile)
        .unwrap()
        .is_some());
    drop(reopened);
    let _ = std::fs::remove_file(path);
}
