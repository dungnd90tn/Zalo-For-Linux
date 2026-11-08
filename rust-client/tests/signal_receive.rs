//! Receiving a personal ciphertext through the coordinator: plan, engine decrypt, one SQL commit.
//! The sender is a second store running the real send pipeline, so this crosses both directions.
#![cfg(feature = "signal-crypto")]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zalo_rust::{
    app::live::signal_crypto::{
        personal_ciphertext_digest, personal_ciphertext_from_wire, prekey_sender,
        stored_peer_identity, Cancellation, Coordinator, Recipient,
    },
    domain::Message,
    store::{
        e2ee::{self, Receive},
        MessageStore,
    },
};
use zalo_signal_engine as engine;

fn path(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "zalo-recv-{tag}-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn convert<A: serde::Serialize, B: serde::de::DeserializeOwned>(value: &A) -> B {
    serde_json::from_value(serde_json::to_value(value).unwrap()).unwrap()
}
fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}
fn engine_local(platform: engine::SignalPlatform) -> engine::LocalNamespace {
    engine::LocalNamespace {
        epoch: engine::RegistrationEpoch::new(1).unwrap(),
        platform,
    }
}

/// Receiver "202" (a mobile device): a real engine-created identity installed as its active local epoch.
async fn receiver_store(
    path: &std::path::Path,
) -> (MessageStore, e2ee::LocalNamespace, engine::PublicBundle) {
    let account = engine::AccountId::new("202").unwrap();
    let worker = engine::Engine::new(account.clone(), 1, engine::EngineOptions::default()).unwrap();
    let result = worker
        .submit(engine::Request {
            account: account.clone(),
            generation: 1,
            operation_id: 1,
            expected_revision: 0,
            operation: engine::Operation::CreateBundle {
                local: engine_local(engine::SignalPlatform::Mobile),
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
    let mut store = MessageStore::open(path).unwrap();
    let local = store
        .e2ee_register_local(
            "202",
            None,
            &e2ee::VerifiedBinding {
                platform: e2ee::Platform::Mobile,
                registration_id: bundle.registration_id,
                identity_key: bundle.identity_key,
                session_timestamp: 100,
            },
        )
        .unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    for mutation in delta.into_mutations() {
        let engine::Mutation::Put(record) = mutation else {
            panic!("unexpected delete")
        };
        let key: e2ee::RecordKey = convert(record.key());
        connection
            .execute(
                "INSERT INTO e2ee_records(account,key,value) VALUES('202',?1,?2)",
                rusqlite::params![serde_json::to_string(&key).unwrap(), record.value()],
            )
            .unwrap();
    }
    (store, local, bundle)
}

/// Sender "101" (PC) encrypts `text` for the receiver's bundle with the real send pipeline.
async fn encrypt_for(
    path: &std::path::Path,
    text: &str,
    bundle: &engine::PublicBundle,
) -> (String, [u8; 33], u16) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/e2ee/session.json")).unwrap();
    let public: [u8; 33] = hex(fixture["alice_identity_public"].as_str().unwrap())
        .try_into()
        .unwrap();
    let private = hex(fixture["alice_identity_private"].as_str().unwrap());
    let mut store = MessageStore::open(path).unwrap();
    let local = store
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
    let value = serde_json::to_vec(&serde_json::json!({"publicKey":public.as_slice(),"privateKey":private,"registrationId":1001})).unwrap();
    let key = serde_json::to_string(&e2ee::RecordKey::LocalIdentity {
        local: local.clone(),
    })
    .unwrap();
    rusqlite::Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO e2ee_records(account,key,value) VALUES('101',?1,?2)",
            rusqlite::params![key, value],
        )
        .unwrap();
    let peer = store
        .e2ee_register_peer(
            "101",
            &local,
            "202",
            None,
            &e2ee::VerifiedBinding {
                platform: e2ee::Platform::Mobile,
                registration_id: bundle.registration_id,
                identity_key: bundle.identity_key,
                session_timestamp: 100,
            },
        )
        .unwrap();
    let recipient = Recipient {
        trust: engine::TrustDecision {
            peer: convert(&peer),
            registration_id: bundle.registration_id,
            identity_key: bundle.identity_key,
            session_timestamp: 100,
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
    };
    let id = store
        .e2ee_reserve_send("101", &local, "d:202", text, 1000)
        .unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator =
        Coordinator::new(engine::AccountId::new("101").unwrap(), 1, convert(&local)).unwrap();
    let plan = coordinator
        .plan_personal(&mut store, id, text, vec![recipient], &gate, &cancel)
        .unwrap();
    let prepared = coordinator
        .prepare_personal(plan, &gate, &cancel)
        .await
        .unwrap();
    store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
    let outgoing = store.e2ee_outgoing("101", id).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&outgoing.ciphertext.unwrap()).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
    (
        records[0]["message"].as_str().unwrap().to_owned(),
        public,
        1001,
    )
}

#[tokio::test]
async fn a_first_contact_prekey_message_is_decrypted_and_committed_once_with_its_message() {
    let (recv_path, send_path) = (path("recv"), path("send"));
    let (mut store, local, bundle) = receiver_store(&recv_path).await;
    let text = "Xin chào \"Zalo\" 😀\nHai dòng";
    let (wire, sender_identity, sender_registration) = encrypt_for(&send_path, text, &bundle).await;

    // The receiver learns the sender's identity from the prekey header, records it on first use, then decrypts.
    let ciphertext = personal_ciphertext_from_wire(&wire).unwrap();
    assert!(matches!(
        ciphertext.message_type,
        engine::PersonalMessageType::PreKey
    ));
    let (registration, identity) = prekey_sender(&ciphertext.body).unwrap();
    assert_eq!(
        (registration, identity),
        (sender_registration, sender_identity)
    );
    let peer = store
        .e2ee_register_peer(
            "202",
            &local,
            "101",
            None,
            &e2ee::VerifiedBinding {
                platform: e2ee::Platform::Pc,
                registration_id: registration,
                identity_key: identity,
                session_timestamp: 200,
            },
        )
        .unwrap();
    let trust = |peer: &e2ee::PeerNamespace| engine::TrustDecision {
        peer: convert(peer),
        registration_id: registration,
        identity_key: identity,
        session_timestamp: 200,
        basis: engine::TrustBasis::AuthenticatedFirstUse,
    };
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let mut coordinator =
        Coordinator::new(engine::AccountId::new("202").unwrap(), 1, convert(&local)).unwrap();
    let plan = coordinator
        .plan_receive(
            &mut store,
            trust(&peer),
            ciphertext,
            1_700_000_000_000,
            "d:101",
            "srv-1",
            "signal-personal",
            &gate,
            &cancel,
        )
        .unwrap();
    // Nothing is stored until the commit.
    assert!(store.conversations("202", 5).unwrap().is_empty());
    let verified = coordinator
        .prepare_receive(plan, &gate, &cancel)
        .await
        .unwrap();
    assert_eq!(verified.plaintext(), text.as_bytes());

    let message = Message {
        account_id: "202".into(),
        thread_id: "d:101".into(),
        id: "srv-1".into(),
        sender: "101".into(),
        text: String::from_utf8(verified.plaintext().to_vec()).unwrap(),
        sticker: None,
        media: None,
        sent_at_ms: 1_700_000_000_000,
    };
    let receipt = store
        .e2ee_commit_receive(
            Receive {
                delta: verified.delta(),
                messages: std::slice::from_ref(&message),
                queue: verified.queue(),
                cursor: 1,
                context: verified.context().clone(),
                ciphertext_digest: *verified.ciphertext_digest(),
            },
            &gate,
        )
        .unwrap();
    assert!(!receipt.is_replay());
    // The committed delivery is recognised by its ciphertext, and the engine kept the sender's identity key
    // where an ordinary (header-less) message can read it back.
    let digest = personal_ciphertext_digest(&personal_ciphertext_from_wire(&wire).unwrap());
    assert!(store.e2ee_ciphertext_seen("202", &digest).unwrap());
    assert_eq!(
        stored_peer_identity(&store, "202", &local, &peer).unwrap(),
        sender_identity
    );
    let list = store.conversations("202", 5).unwrap();
    assert_eq!(
        (list[0].id.as_str(), list[0].last_text.as_str()),
        ("d:101", text)
    );

    // The one-time prekey is consumed and the ratchet advanced: the same ciphertext cannot be accepted again.
    let again = personal_ciphertext_from_wire(&wire).unwrap();
    let replay = coordinator.plan_receive(
        &mut store,
        trust(&peer),
        again,
        1_700_000_000_000,
        "d:101",
        "srv-2",
        "signal-personal",
        &gate,
        &cancel,
    );
    // Either planning refuses (the consumed prekey is gone) or the engine refuses the replay.
    if let Ok(plan) = replay {
        assert!(coordinator
            .prepare_receive(plan, &gate, &cancel)
            .await
            .is_err());
    }
    assert_eq!(store.conversations("202", 5).unwrap().len(), 1);
    for p in [recv_path, send_path] {
        let _ = std::fs::remove_file(p);
    }
}
