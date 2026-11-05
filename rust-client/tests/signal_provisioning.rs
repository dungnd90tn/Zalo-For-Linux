#![cfg(feature = "signal-crypto")]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zalo_rust::{
    app::live::signal_crypto::{provisioning::PendingProvisioner, Cancellation},
    store::{
        e2ee::{
            self,
            provisioning::{CandidateRequest, PendingState, PublicAttemptContext},
        },
        MessageStore,
    },
};
use zalo_signal_engine as engine;

fn path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "zalo-creation-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn request(current: Option<u64>) -> CandidateRequest {
    CandidateRequest {
        platform: e2ee::Platform::Pc,
        expected_current: current,
        stable_intent: "synthetic-registration".into(),
        prekey_count: 20,
        generation: 1,
    }
}
fn count(connection: &rusqlite::Connection, table: &str) -> i64 {
    let sql = match table {
        "pending" => "SELECT count(*) FROM e2ee_pending_records",
        "active" => "SELECT count(*) FROM e2ee_records",
        "epochs" => "SELECT count(*) FROM e2ee_local_epochs",
        "public" => "SELECT count(*) FROM e2ee_public_outbox",
        _ => panic!("unknown synthetic table"),
    };
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[tokio::test]
async fn actual_csprng_bundle_stages_atomically_and_reopens_without_activation() {
    let path = path();
    let mut store = MessageStore::open(&path).unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    let operation = plan.operation_id();
    let epoch = plan.local().epoch.clone();
    assert!(plan.snapshot().records.is_empty());
    let mut provisioner =
        PendingProvisioner::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
    let candidate = provisioner.prepare(plan, &gate, &cancel).await.unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(count(&connection, "pending"), 0);
    assert_eq!(count(&connection, "public"), 0);
    let staged = store.e2ee_stage_verified_bundle(&candidate, &gate).unwrap();
    assert!(!staged.is_replay());
    assert_eq!(staged.operation_id(), operation);
    assert_eq!(staged.local().epoch, epoch);
    assert!(store.e2ee_candidate_state("101", operation).unwrap() == PendingState::Ready);
    assert_eq!(count(&connection, "pending"), 22);
    assert_eq!(count(&connection, "public"), 1);
    assert_eq!(count(&connection, "active"), 0);
    assert_eq!(count(&connection, "epochs"), 0);
    let current: Option<i64> = connection
        .query_row(
            "SELECT current_local FROM e2ee_accounts WHERE account='101'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(current.is_none());
    let public: Vec<u8> = connection
        .query_row(
            "SELECT payload FROM e2ee_public_outbox WHERE account='101'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&public).unwrap();
    assert_eq!(json["delSS"], 0);
    assert_eq!(json["key"]["deviceId"], 2);
    assert_eq!(json["key"]["opKeys"].as_array().unwrap().len(), 20);
    assert!(json.get("reqId").is_none());
    assert!(store
        .e2ee_stage_verified_bundle(&candidate, &gate)
        .unwrap()
        .is_replay());
    assert!(store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .is_err());
    let pending_before: Vec<(String, Vec<u8>)> = {
        let mut query = connection
            .prepare("SELECT key,value FROM e2ee_pending_records ORDER BY key")
            .unwrap();
        query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    cancel.cancel();
    drop(candidate);
    drop(provisioner);
    drop(store);
    drop(connection);
    let mut store = MessageStore::open(&path).unwrap();
    let status = store.e2ee_pending_candidate("101").unwrap().unwrap();
    assert!(status.state() == PendingState::Ready);
    assert_eq!(status.operation_id(), operation);
    let receipt = store
        .e2ee_claim_bundle_upload(
            "101",
            operation,
            &PublicAttemptContext {
                generation: 1,
                socket_epoch: 7,
            },
            &gate,
        )
        .unwrap();
    assert!(receipt.payload() == public);
    assert_eq!(receipt.local().epoch, epoch);
    assert!(receipt.can_submit(&gate));
    assert!(store
        .e2ee_claim_bundle_upload(
            "101",
            operation,
            &PublicAttemptContext {
                generation: 1,
                socket_epoch: 7
            },
            &gate
        )
        .is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    let pending_after: Vec<(String, Vec<u8>)> = {
        let mut query = connection
            .prepare("SELECT key,value FROM e2ee_pending_records ORDER BY key")
            .unwrap();
        query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(pending_before == pending_after);
    assert_eq!(count(&connection, "active"), 0);
    assert_eq!(count(&connection, "epochs"), 0);
    assert_eq!(store.e2ee_recover_bundle_uploads("101").unwrap(), 1);
    assert!(store.e2ee_candidate_state("101", operation).unwrap() == PendingState::Unknown);
    assert!(store
        .e2ee_claim_bundle_upload(
            "101",
            operation,
            &PublicAttemptContext {
                generation: 1,
                socket_epoch: 8
            },
            &gate
        )
        .is_err());
    drop(store);
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pending_creation_preserves_existing_local_keys_and_binding() {
    let path = path();
    let mut store = MessageStore::open(&path).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/e2ee/session.json")).unwrap();
    let decode = |name: &str| {
        let value = fixture[name].as_str().unwrap();
        (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
            .collect::<Vec<_>>()
    };
    let public: [u8; 33] = decode("alice_identity_public").try_into().unwrap();
    let active = store
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
    let key = e2ee::RecordKey::LocalIdentity {
        local: active.clone(),
    };
    let encoded = serde_json::to_string(&key).unwrap();
    let private=serde_json::to_vec(&serde_json::json!({"publicKey":public.as_slice(),"privateKey":decode("alice_identity_private"),"registrationId":1001})).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO e2ee_records(account,key,value) VALUES('101',?1,?2)",
            rusqlite::params![encoded, private],
        )
        .unwrap();
    let before = store
        .e2ee_snapshot("101", std::slice::from_ref(&key))
        .unwrap()
        .records
        .remove(0)
        .value;
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(Some(1)), &gate)
        .unwrap();
    assert_eq!(plan.local().epoch, "2");
    let mut provisioner =
        PendingProvisioner::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
    let candidate = provisioner.prepare(plan, &gate, &cancel).await.unwrap();
    store.e2ee_stage_verified_bundle(&candidate, &gate).unwrap();
    store
        .e2ee_validate_local_identity("101", &active, 1001, &public)
        .unwrap();
    assert!(store.e2ee_snapshot("101", &[key]).unwrap().records[0].value == before);
    let current: i64 = connection
        .query_row(
            "SELECT current_local FROM e2ee_accounts WHERE account='101'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current, 1);
    assert_eq!(count(&connection, "active"), 1);
    assert_eq!(count(&connection, "epochs"), 1);
    assert_eq!(count(&connection, "pending"), 22);
    drop(store);
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn stage_sql_failure_and_precommit_cancel_leave_no_partial_private_or_public_keys() {
    let path = path();
    let mut store = MessageStore::open(&path).unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    let operation = plan.operation_id();
    let mut provisioner =
        PendingProvisioner::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
    let candidate = provisioner.prepare(plan, &gate, &cancel).await.unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_creation BEFORE INSERT ON e2ee_public_outbox BEGIN SELECT RAISE(ABORT,'synthetic storage failure'); END;").unwrap();
    assert!(store.e2ee_stage_verified_bundle(&candidate, &gate).is_err());
    assert_eq!(count(&connection, "pending"), 0);
    assert_eq!(count(&connection, "public"), 0);
    assert!(store.e2ee_candidate_state("101", operation).unwrap() == PendingState::Reserved);
    connection
        .execute_batch("DROP TRIGGER fail_creation;")
        .unwrap();
    cancel.cancel();
    assert!(store.e2ee_stage_verified_bundle(&candidate, &gate).is_err());
    assert_eq!(count(&connection, "pending"), 0);
    assert_eq!(count(&connection, "public"), 0);
    let cancel = Cancellation::default();
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    assert_eq!(plan.operation_id(), operation);
    let candidate = provisioner.prepare(plan, &gate, &cancel).await.unwrap();
    store.e2ee_stage_verified_bundle(&candidate, &gate).unwrap();
    assert_eq!(count(&connection, "pending"), 22);
    assert_eq!(count(&connection, "public"), 1);
    drop(store);
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn wrong_account_generation_and_cancelled_plans_never_generate_staged_keys() {
    let mut store = MessageStore::in_memory().unwrap();
    let gate = e2ee::GenerationGate::new(1).unwrap();
    let cancel = Cancellation::default();
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    let operation = plan.operation_id();
    let mut wrong = PendingProvisioner::new(engine::AccountId::new("202").unwrap(), 1).unwrap();
    assert!(wrong.prepare(plan, &gate, &cancel).await.is_err());
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    let mut correct = PendingProvisioner::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
    cancel.cancel();
    assert!(correct.prepare(plan, &gate, &cancel).await.is_err());
    let plan = store
        .e2ee_reserve_local_candidate("101", &request(None), &gate)
        .unwrap();
    gate.replace(2).unwrap();
    assert!(correct
        .prepare(plan, &gate, &Cancellation::default())
        .await
        .is_err());
    assert!(store.e2ee_candidate_state("101", operation).unwrap() == PendingState::Reserved);
}
