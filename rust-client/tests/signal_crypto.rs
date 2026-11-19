#![cfg(feature = "signal-crypto")]
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zalo_rust::{
    app::live::signal_crypto::{
        authorize_dispatch, required_personal_records, AccountEngine, Cancellation, Coordinator,
        Recipient,
    },
    store::{e2ee, MessageStore},
};
use zalo_signal_engine as engine;

#[path = "signal_crypto/first_use_tests.rs"]
mod first_use_tests;

fn local(platform: engine::SignalPlatform) -> engine::LocalNamespace {
    engine::LocalNamespace {
        epoch: engine::RegistrationEpoch::new(1).unwrap(),
        platform,
    }
}
fn skey(key: engine::RecordKey) -> e2ee::RecordKey {
    serde_json::from_value(serde_json::to_value(key).unwrap()).unwrap()
}
fn bytes(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}
fn ephemeral_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "zalo-signal-coordinator-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
struct Receiver {
    engine: engine::Engine,
    local: engine::LocalNamespace,
    bundle: engine::PublicBundle,
    records: Vec<engine::Record>,
}
async fn receiver(platform: engine::SignalPlatform) -> Receiver {
    let account = engine::AccountId::new("202").unwrap();
    let local = local(platform);
    let worker = engine::Engine::new(account.clone(), 1, engine::EngineOptions::default()).unwrap();
    let result = worker
        .submit(engine::Request {
            account: account.clone(),
            generation: 1,
            operation_id: 1,
            expected_revision: 0,
            operation: engine::Operation::CreateBundle {
                local: local.clone(),
                prekey_count: 3,
            },
            snapshot: engine::Snapshot::new(account, 0, vec![]).unwrap(),
        })
        .unwrap()
        .await
        .unwrap();
    let (_, _, _, _, output, delta) = result.into_parts();
    let engine::Output::Bundle(bundle) = output else {
        panic!("bundle")
    };
    let records = delta
        .into_mutations()
        .into_iter()
        .map(|mutation| match mutation {
            engine::Mutation::Put(record) => record,
            _ => panic!("unexpected delete"),
        })
        .collect();
    Receiver {
        engine: worker,
        local,
        bundle,
        records,
    }
}
fn fixture_store(path: &std::path::Path) -> (MessageStore, e2ee::LocalNamespace, [u8; 33]) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/e2ee/session.json")).unwrap();
    let public: [u8; 33] = bytes(fixture["alice_identity_public"].as_str().unwrap())
        .try_into()
        .unwrap();
    let private = bytes(fixture["alice_identity_private"].as_str().unwrap());
    let mut store = MessageStore::open(path).unwrap();
    let namespace = store
        .e2ee_register_local(
            "101",
            None,
            &e2ee::VerifiedBinding {
                platform: e2ee::Platform::Pc,
                registration_id: 1001,
                identity_key: public,
                session_timestamp: 100,
            },
        )
        .unwrap();
    // Explicit pre-existing synthetic account fixture. This is not a production
    // provisioning path and does not pretend to solve local-epoch activation.
    let value=serde_json::to_vec(&serde_json::json!({"publicKey":public.as_slice(),"privateKey":private,"registrationId":1001})).unwrap();
    let key = serde_json::to_string(&e2ee::RecordKey::LocalIdentity {
        local: namespace.clone(),
    })
    .unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO e2ee_records(account,key,value) VALUES('101',?1,?2)",
            rusqlite::params![key, value],
        )
        .unwrap();
    (store, namespace, public)
}
fn recipient(
    store: &mut MessageStore,
    local: &e2ee::LocalNamespace,
    receiver: &Receiver,
    corrupt: bool,
) -> Recipient {
    let b = &receiver.bundle;
    let platform = match receiver.local.platform {
        engine::SignalPlatform::Mobile => e2ee::Platform::Mobile,
        engine::SignalPlatform::Web => e2ee::Platform::Web,
        engine::SignalPlatform::Pc => e2ee::Platform::Pc,
    };
    let peer = store
        .e2ee_register_peer(
            "101",
            local,
            "202",
            None,
            &e2ee::VerifiedBinding {
                platform,
                registration_id: b.registration_id,
                identity_key: b.identity_key,
                session_timestamp: 100,
            },
        )
        .unwrap();
    let peer: engine::PeerNamespace =
        serde_json::from_value(serde_json::to_value(peer).unwrap()).unwrap();
    let mut signature = b.signature;
    if corrupt {
        signature[0] ^= 1
    }
    Recipient {
        trust: engine::TrustDecision {
            peer,
            registration_id: b.registration_id,
            identity_key: b.identity_key,
            session_timestamp: 100,
            basis: engine::TrustBasis::AuthenticatedFirstUse,
        },
        bundle: Some(engine::PeerBundle {
            registration_id: b.registration_id,
            identity_key: b.identity_key,
            signed_prekey_id: b.signed_prekey_id,
            signed_prekey: b.signed_prekey,
            signature,
            prekey: b.prekeys.first().copied(),
        }),
    }
}
async fn decrypt(receiver: &Receiver, message: &str, alice: [u8; 33]) -> Vec<u8> {
    let raw = STANDARD.decode(message).unwrap();
    let message = engine::PersonalCiphertext {
        message_type: match raw[0] {
            3 => engine::PersonalMessageType::PreKey,
            1 => engine::PersonalMessageType::Whisper,
            _ => panic!("message type"),
        },
        body: raw[1..].to_vec(),
    };
    let records = receiver
        .records
        .iter()
        .map(|record| {
            engine::Record::from_parts(record.key().clone(), record.value().to_vec()).unwrap()
        })
        .collect();
    let result = receiver
        .engine
        .submit(engine::Request {
            account: engine::AccountId::new("202").unwrap(),
            generation: 1,
            operation_id: 2,
            expected_revision: 0,
            operation: engine::Operation::DecryptPersonal {
                local: receiver.local.clone(),
                trust: engine::TrustDecision {
                    peer: engine::PeerNamespace {
                        uid: engine::AccountId::new("101").unwrap(),
                        platform: engine::SignalPlatform::Pc,
                        epoch: engine::RegistrationEpoch::new(1).unwrap(),
                    },
                    registration_id: 1001,
                    identity_key: alice,
                    session_timestamp: 100,
                    basis: engine::TrustBasis::AuthenticatedFirstUse,
                },
                message,
                message_timestamp: 200,
            },
            snapshot: engine::Snapshot::new(engine::AccountId::new("202").unwrap(), 0, records)
                .unwrap(),
        })
        .unwrap()
        .await
        .unwrap();
    let (_, _, _, _, output, _) = result.into_parts();
    let engine::Output::Plaintext(body) = output else {
        panic!("plaintext")
    };
    body
}

#[tokio::test]
async fn actual_engine_two_device_fanout_one_sql_commit_and_restart_dispatch() {
    let path = ephemeral_path();
    let mobile = receiver(engine::SignalPlatform::Mobile).await;
    let pc = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, alice) = fixture_store(&path);
    let recipients = vec![
        recipient(&mut store, &local, &mobile, false),
        recipient(&mut store, &local, &pc, false),
    ];
    let keys = recipients
        .iter()
        .map(|recipient| {
            skey(engine::RecordKey::Session {
                local: local_engine(&local),
                peer: recipient.trust.peer.clone(),
            })
        })
        .collect::<Vec<_>>();
    let base = store.e2ee_snapshot("101", &keys).unwrap();
    assert!(base.records.is_empty());
    let text = "Xin chào\n\"Zalo\" \\ 😀";
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", text, 1000)
        .unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let plan = coordinator
        .plan_personal(&mut store, id, text, recipients, &gate, &cancel)
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    assert!(store
        .e2ee_snapshot("101", &keys)
        .unwrap()
        .records
        .is_empty());
    let receipt = store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    assert_eq!(receipt.revision(), base.revision + 1);
    // Preparation cancellation after COMMIT does not revoke durable READY.
    // Restart dispatch below still requires a current auth GenerationGate.
    cancel.cancel();
    assert_eq!(store.e2ee_snapshot("101", &keys).unwrap().records.len(), 2);
    let outgoing = store.e2ee_outgoing("101", id).unwrap();
    let exact = outgoing.ciphertext.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&exact).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    assert_eq!(records.len(), 2);
    for record in &records {
        assert!(record.get("textProperties").is_none());
        let target = if record["platform"] == 0 {
            &mobile
        } else {
            &pc
        };
        assert_eq!(
            decrypt(target, record["message"].as_str().unwrap(), alice).await,
            text.as_bytes()
        );
    }
    drop(store);
    let mut reopened = MessageStore::open(&path).unwrap();
    let dispatch = reopened.e2ee_begin_dispatch("101", id, 1, &gate).unwrap();
    assert_eq!(dispatch.verified_codec(), Some(1));
    assert!(dispatch.ciphertext() == exact);
    let authorized = authorize_dispatch(dispatch, &gate).unwrap();
    assert!(authorized.exact_json() == exact);
    assert!(authorized.can_attempt(&gate));
    assert_eq!(authorized.parameters().path, "/api/e2ee/pc/message/sms");
    assert!(reopened.e2ee_begin_dispatch("101", id, 1, &gate).is_err());
    gate.cancel();
    assert!(!authorized.can_attempt(&gate));
    drop(reopened);
    let _ = std::fs::remove_file(path);
}
fn local_engine(local: &e2ee::LocalNamespace) -> engine::LocalNamespace {
    serde_json::from_value(serde_json::to_value(local).unwrap()).unwrap()
}

#[tokio::test]
async fn one_account_worker_hands_pending_creation_to_existing_active_personal_session() {
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, alice) = fixture_store(&path);
    let recipient = recipient(&mut store, &local, &bob, false);
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let worker = AccountEngine::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
    let mut provisioner = worker.provisioner();
    let mut coordinator = worker.coordinator(local_engine(&local));
    let pending = store
        .e2ee_reserve_local_candidate(
            "101",
            &e2ee::provisioning::CandidateRequest {
                platform: e2ee::Platform::Pc,
                expected_current: Some(1),
                stable_intent: "synthetic-shared-worker".into(),
                prekey_count: 20,
                generation: 1,
            },
            &gate,
        )
        .unwrap();
    let candidate = provisioner.prepare(pending, &gate, &cancel).await.unwrap();
    store.e2ee_stage_verified_bundle(&candidate, &gate).unwrap();
    let client_id = store
        .e2ee_reserve_send("101", &local, "d:202", "shared worker", 1000)
        .unwrap();
    let plan = coordinator
        .plan_personal(
            &mut store,
            client_id,
            "shared worker",
            vec![recipient],
            &gate,
            &cancel,
        )
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    let exact = store
        .e2ee_outgoing("101", client_id)
        .unwrap()
        .ciphertext
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&exact).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    assert_eq!(
        decrypt(&bob, records[0]["message"].as_str().unwrap(), alice).await,
        b"shared worker"
    );
    store
        .e2ee_validate_local_identity("101", &local, 1001, &alice)
        .unwrap();
    assert!(
        store
            .e2ee_pending_candidate("101")
            .unwrap()
            .unwrap()
            .state()
            == e2ee::provisioning::PendingState::Ready
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}

fn copy_recipient(recipient: &Recipient, prekey: (u32, [u8; 33])) -> Recipient {
    let trust = &recipient.trust;
    let bundle = recipient.bundle.as_ref().unwrap();
    Recipient {
        trust: engine::TrustDecision {
            peer: trust.peer.clone(),
            registration_id: trust.registration_id,
            identity_key: trust.identity_key,
            session_timestamp: trust.session_timestamp,
            basis: trust.basis,
        },
        bundle: Some(engine::PeerBundle {
            registration_id: bundle.registration_id,
            identity_key: bundle.identity_key,
            signed_prekey_id: bundle.signed_prekey_id,
            signed_prekey: bundle.signed_prekey,
            signature: bundle.signature,
            prekey: Some(prekey),
        }),
    }
}

#[tokio::test]
async fn forced_renewal_preserves_trust_and_durable_unconfirmed_intent() {
    const INIT: &str = r#"{"subDevice":[]}"#;
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Mobile).await;
    let (mut store, local, alice) = fixture_store(&path);
    let initial = recipient(&mut store, &local, &bob, false);
    let mut invalid = copy_recipient(&initial, bob.bundle.prekeys[1]);
    invalid.bundle.as_mut().unwrap().signature[0] ^= 1;
    let renewal = copy_recipient(&initial, bob.bundle.prekeys[2]);
    let namespace = initial.trust.peer.clone();
    let session_key = skey(engine::RecordKey::Session {
        local: local_engine(&local),
        peer: namespace.clone(),
    });
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();

    let first = store
        .e2ee_reserve_send("101", &local, "d:202", INIT, 1000)
        .unwrap();
    store
        .e2ee_mark_peer_init("101", &local, "202", first)
        .unwrap();
    let plan = coordinator
        .plan_personal(&mut store, first, INIT, vec![initial], &gate, &cancel)
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    store.e2ee_begin_dispatch("101", first, 1, &gate).unwrap();
    store
        .e2ee_confirm_peer_init("101", &local, "202", first, "init-first")
        .unwrap();
    let before = store
        .e2ee_snapshot("101", std::slice::from_ref(&session_key))
        .unwrap();
    assert_eq!(before.records.len(), 1);

    // Renewal must verify the new signed bundle even when a usable session exists.
    // An invalid signature must not retire the trusted binding or alter its ratchet.
    let failed = store
        .e2ee_reserve_send("101", &local, "d:202", INIT, 1001)
        .unwrap();
    store
        .e2ee_mark_peer_init("101", &local, "202", failed)
        .unwrap();
    let plan = coordinator
        .plan_personal(&mut store, failed, INIT, vec![invalid], &gate, &cancel)
        .unwrap();
    assert!(coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .is_err());
    assert!(store.e2ee_discard_reserved("101", failed).unwrap());
    assert!(!store.e2ee_peer_init_pending("101", &local, "202").unwrap());
    let after_failure = store
        .e2ee_snapshot("101", std::slice::from_ref(&session_key))
        .unwrap();
    assert_eq!(after_failure.records[0].value, before.records[0].value);

    let renewed = store
        .e2ee_reserve_send("101", &local, "d:202", INIT, 1002)
        .unwrap();
    store
        .e2ee_mark_peer_init("101", &local, "202", renewed)
        .unwrap();
    let plan = coordinator
        .plan_personal(&mut store, renewed, INIT, vec![renewal], &gate, &cancel)
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    let after_renewal = store
        .e2ee_snapshot("101", std::slice::from_ref(&session_key))
        .unwrap();
    assert_ne!(after_renewal.records[0].value, before.records[0].value);
    let exact = store
        .e2ee_outgoing("101", renewed)
        .unwrap()
        .ciphertext
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&exact).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        decrypt(&bob, records[0]["message"].as_str().unwrap(), alice).await,
        INIT.as_bytes()
    );
    store.e2ee_begin_dispatch("101", renewed, 1, &gate).unwrap();
    store.e2ee_send_failed("101", renewed, false).unwrap();
    drop(store);

    // A committed renewal without a correlated service ACK survives restart as
    // pending, even though its replacement ratchet is already stored locally.
    let mut reopened = MessageStore::open(&path).unwrap();
    assert!(reopened
        .e2ee_peer_init_pending("101", &local, "202")
        .unwrap());
    assert!(!reopened
        .e2ee_peer_init_pending("101", &local, "101")
        .unwrap());
    let active = reopened
        .e2ee_active_peer("101", &local, "202", e2ee::Platform::Mobile)
        .unwrap()
        .unwrap();
    assert!(local_engine_peer(&active.namespace) == namespace);
    assert_eq!(active.registration_id, bob.bundle.registration_id);
    assert_eq!(active.session_timestamp, 100);
    reopened
        .e2ee_validate_peer_identity(
            "101",
            &local,
            &active.namespace,
            bob.bundle.registration_id,
            &bob.bundle.identity_key,
            100,
        )
        .unwrap();
    assert_eq!(
        reopened
            .e2ee_snapshot("101", &[session_key])
            .unwrap()
            .records[0]
            .value,
        after_renewal.records[0].value
    );
    let outgoing = reopened.e2ee_outgoing("101", renewed).unwrap();
    assert!(outgoing.state == e2ee::SendState::Unknown);
    assert_eq!(outgoing.ciphertext.unwrap(), exact);
    assert!(reopened
        .e2ee_begin_dispatch("101", renewed, 1, &gate)
        .is_err());
    assert!(reopened
        .e2ee_confirm_peer_init("101", &local, "101", renewed, "init-renewed")
        .is_err());
    assert!(reopened
        .e2ee_peer_init_pending("101", &local, "202")
        .unwrap());
    reopened
        .e2ee_confirm_peer_init("101", &local, "202", renewed, "init-renewed")
        .unwrap();
    assert!(!reopened
        .e2ee_peer_init_pending("101", &local, "202")
        .unwrap());
    assert!(reopened.page("101", "d:202", None, 10).unwrap().is_empty());
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

fn local_engine_peer(peer: &e2ee::PeerNamespace) -> engine::PeerNamespace {
    serde_json::from_value(serde_json::to_value(peer).unwrap()).unwrap()
}

#[tokio::test]
async fn failed_sql_retry_uses_fresh_job_same_durable_id_and_fresh_one_time_key() {
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, alice) = fixture_store(&path);
    let recipient = recipient(&mut store, &local, &bob, false);
    let session = skey(engine::RecordKey::Session {
        local: local_engine(&local),
        peer: recipient.trust.peer.clone(),
    });
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", "retry", 1000)
        .unwrap();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "retry",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
            &gate,
            &cancel,
        )
        .unwrap();
    let first = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_crypto_commit BEFORE UPDATE OF state ON e2ee_outbox WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END;").unwrap();
    assert!(store.e2ee_commit_verified_send(&first, &gate).is_err());
    assert!(store
        .e2ee_snapshot("101", std::slice::from_ref(&session))
        .unwrap()
        .records
        .is_empty());
    assert!(store.e2ee_outgoing("101", id).unwrap().state == e2ee::SendState::Reserved);
    connection
        .execute_batch("DROP TRIGGER fail_crypto_commit;")
        .unwrap();
    let reserved_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM e2ee_operation_reservations WHERE account='101'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "retry",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[1])],
            &gate,
            &cancel,
        )
        .unwrap();
    let second = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    let receipt = store.e2ee_commit_verified_send(&second, &gate).unwrap();
    assert!(!receipt.is_replay());
    assert!(store.e2ee_commit_verified_send(&first, &gate).is_err());
    let reserved_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM e2ee_operation_reservations WHERE account='101'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reserved_before, reserved_after);
    let exact = store.e2ee_outgoing("101", id).unwrap().ciphertext.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&exact).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    assert_eq!(
        decrypt(&bob, records[0]["message"].as_str().unwrap(), alice).await,
        b"retry"
    );
    assert!(coordinator
        .plan_personal(
            &mut store,
            id,
            "retry",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[2])],
            &gate,
            &cancel
        )
        .is_err());
    drop(store);
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn any_device_failure_discards_all_ratchets_and_keeps_reserved_draft() {
    let path = ephemeral_path();
    let mobile = receiver(engine::SignalPlatform::Mobile).await;
    let pc = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, _) = fixture_store(&path);
    let recipients = vec![
        recipient(&mut store, &local, &mobile, false),
        recipient(&mut store, &local, &pc, true),
    ];
    let keys = recipients
        .iter()
        .map(|recipient| {
            skey(engine::RecordKey::Session {
                local: local_engine(&local),
                peer: recipient.trust.peer.clone(),
            })
        })
        .collect::<Vec<_>>();
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", "draft", 1000)
        .unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let plan = coordinator
        .plan_personal(&mut store, id, "draft", recipients, &gate, &cancel)
        .unwrap();
    assert!(coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .is_err());
    assert!(store
        .e2ee_snapshot("101", &keys)
        .unwrap()
        .records
        .is_empty());
    assert!(store.e2ee_outgoing("101", id).unwrap().state == e2ee::SendState::Reserved);
    assert_eq!(store.e2ee_reserved_plaintext("101", id).unwrap(), "draft");
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn public_header_plans_exact_prekey_records_and_rejects_ambiguous_input() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/e2ee/session.json")).unwrap();
    let body = bytes(fixture["initial_prekey_message"].as_str().unwrap());
    let local = local(engine::SignalPlatform::Pc);
    let peer = engine::PeerNamespace {
        uid: engine::AccountId::new("101").unwrap(),
        platform: engine::SignalPlatform::Pc,
        epoch: engine::RegistrationEpoch::new(1).unwrap(),
    };
    let trust = engine::TrustDecision {
        peer,
        registration_id: 1001,
        identity_key: bytes(fixture["alice_identity_public"].as_str().unwrap())
            .try_into()
            .unwrap(),
        session_timestamp: 100,
        basis: engine::TrustBasis::PersistedBinding,
    };
    let message = engine::PersonalCiphertext {
        message_type: engine::PersonalMessageType::PreKey,
        body: body.clone(),
    };
    let keys = required_personal_records(&local, &trust, &message).unwrap();
    assert_eq!(keys.len(), 6);
    assert!(keys
        .iter()
        .any(|key| matches!(key, e2ee::RecordKey::PreKey { id: 9, .. })));
    assert!(keys
        .iter()
        .any(|key| matches!(key, e2ee::RecordKey::SignedPreKey { id: 7, .. })));
    let mut duplicate = body.clone();
    duplicate.extend([8, 9]);
    assert!(required_personal_records(
        &local,
        &trust,
        &engine::PersonalCiphertext {
            message_type: engine::PersonalMessageType::PreKey,
            body: duplicate
        }
    )
    .is_err());
    let mut overflow = body.clone();
    overflow.splice(2..4, [0x80, 0x80, 1]);
    assert!(required_personal_records(
        &local,
        &trust,
        &engine::PersonalCiphertext {
            message_type: engine::PersonalMessageType::PreKey,
            body: overflow
        }
    )
    .is_err());
    let mut zero_id = body.clone();
    zero_id[5] = 0;
    assert!(required_personal_records(
        &local,
        &trust,
        &engine::PersonalCiphertext {
            message_type: engine::PersonalMessageType::PreKey,
            body: zero_id
        }
    )
    .is_err());
    let mut truncated = body;
    truncated.pop();
    assert!(required_personal_records(
        &local,
        &trust,
        &engine::PersonalCiphertext {
            message_type: engine::PersonalMessageType::PreKey,
            body: truncated
        }
    )
    .is_err());
}

#[tokio::test]
async fn sealed_results_reject_wrong_job_account_revision_and_planned_scope() {
    for case in 0..6 {
        let worker = engine::Engine::new(
            engine::AccountId::new("101").unwrap(),
            1,
            engine::EngineOptions::default(),
        )
        .unwrap();
        let result = worker
            .submit(engine::Request {
                account: engine::AccountId::new("101").unwrap(),
                generation: 1,
                operation_id: 1,
                expected_revision: 0,
                operation: engine::Operation::CreateBundle {
                    local: local(engine::SignalPlatform::Pc),
                    prekey_count: 1,
                },
                snapshot: engine::Snapshot::new(engine::AccountId::new("101").unwrap(), 0, vec![])
                    .unwrap(),
            })
            .unwrap()
            .await
            .unwrap();
        let allowed_keys = result
            .delta()
            .mutations()
            .iter()
            .map(|mutation| match mutation {
                engine::Mutation::Put(record) => skey(record.key().clone()),
                _ => panic!("unexpected delete"),
            })
            .collect();
        let mut context = e2ee::engine_adapter::ExpectedJobContext {
            account: "101".into(),
            generation: 1,
            engine_job_id: 1,
            durable_operation_id: 77,
            expected_revision: 0,
            local: serde_json::from_value(
                serde_json::to_value(local(engine::SignalPlatform::Pc)).unwrap(),
            )
            .unwrap(),
            allowed_keys,
            input_digest: [0; 32],
            activate_peers: vec![],
        };
        match case {
            0 => context.account = "202".into(),
            1 => context.generation = 2,
            2 => context.engine_job_id = 2,
            3 => context.expected_revision = 1,
            4 => context.local.epoch = "2".into(),
            5 => context.allowed_keys.clear(),
            _ => unreachable!(),
        }
        assert!(e2ee::engine_adapter::into_store_delta_checked(result, context).is_err());
    }
}

#[tokio::test]
async fn cancelled_stale_generation_and_stale_sql_revision_never_publish() {
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, _) = fixture_store(&path);
    let recipient = recipient(&mut store, &local, &bob, false);
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", "bounded", 1000)
        .unwrap();
    let mut coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "bounded",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
            &gate,
            &cancel,
        )
        .unwrap();
    cancel.cancel();
    assert!(coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .is_err());
    let cancel = Cancellation::default();
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "bounded",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
            &gate,
            &cancel,
        )
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    cancel.cancel();
    assert!(store.e2ee_commit_verified_send(&prepared, &gate).is_err());
    let cancel = Cancellation::default();
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "bounded",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
            &gate,
            &cancel,
        )
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    store.e2ee_reserve_prekey_ids("101", &local, 1).unwrap();
    assert!(store.e2ee_commit_verified_send(&prepared, &gate).is_err());
    assert!(store.e2ee_outgoing("101", id).unwrap().state == e2ee::SendState::Reserved);
    let plan = coordinator
        .plan_personal(
            &mut store,
            id,
            "bounded",
            vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
            &gate,
            &cancel,
        )
        .unwrap();
    gate.replace(2).unwrap();
    assert!(coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .is_err());
    assert!(store.e2ee_outgoing("101", id).unwrap().state == e2ee::SendState::Reserved);
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn tampered_local_identity_record_and_misbound_peer_are_rejected_before_engine() {
    let path = ephemeral_path();
    let bob = receiver(engine::SignalPlatform::Pc).await;
    let (mut store, local, _) = fixture_store(&path);
    let recipient = recipient(&mut store, &local, &bob, false);
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", "binding", 1000)
        .unwrap();
    let coordinator = Coordinator::new(
        engine::AccountId::new("101").unwrap(),
        1,
        local_engine(&local),
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let key = serde_json::to_string(&e2ee::RecordKey::LocalIdentity {
        local: local.clone(),
    })
    .unwrap();
    let original: Vec<u8> = connection
        .query_row(
            "SELECT value FROM e2ee_records WHERE account='101' AND key=?1",
            [&key],
            |row| row.get(0),
        )
        .unwrap();
    for change in [0, 1] {
        let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
        if change == 0 {
            record["registrationId"] = 999.into()
        } else {
            record["publicKey"][1] = 77.into()
        };
        connection
            .execute(
                "UPDATE e2ee_records SET value=?2 WHERE account='101' AND key=?1",
                rusqlite::params![key, serde_json::to_vec(&record).unwrap()],
            )
            .unwrap();
        assert!(coordinator
            .plan_personal(
                &mut store,
                id,
                "binding",
                vec![copy_recipient(&recipient, bob.bundle.prekeys[0])],
                &gate,
                &cancel
            )
            .is_err());
    }
    connection
        .execute(
            "UPDATE e2ee_records SET value=?2 WHERE account='101' AND key=?1",
            rusqlite::params![key, original],
        )
        .unwrap();
    let mut wrong = copy_recipient(&recipient, bob.bundle.prekeys[0]);
    wrong.trust.registration_id ^= 1;
    assert!(coordinator
        .plan_personal(&mut store, id, "binding", vec![wrong], &gate, &cancel)
        .is_err());
    assert!(store.e2ee_outgoing("101", id).unwrap().state == e2ee::SendState::Reserved);
    drop(store);
    drop(connection);
    let _ = std::fs::remove_file(path);
}
