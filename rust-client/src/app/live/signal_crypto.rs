//! Optional trusted crypto preparation. No socket, HTTP, ACK or startup activation.
//! SQL transactions never span an engine await; all working ratchets are disposable.
use crate::{
    protocol::signal,
    store::{e2ee, MessageStore},
};
use anyhow::{anyhow, ensure, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::Notify,
    time::{timeout_at, Instant},
};
use zalo_signal_engine as engine;

pub mod provisioning;

const MAX_DEVICES: usize = 5;
const BATCH_TIMEOUT: Duration = Duration::from_secs(15);
pub use crate::protocol::signal::engine_adapter::AuthorizedSms;
/// Consume one committed dispatch capability. The future HTTP adapter must
/// consume this non-Clone envelope once; preparing it never sends anything.
pub fn authorize_dispatch(
    receipt: e2ee::DispatchReceipt,
    gate: &e2ee::GenerationGate,
) -> Result<AuthorizedSms> {
    Ok(signal::engine_adapter::sms_from_dispatch(receipt, gate)?)
}

/// Cancels uncommitted preparation. It is not a durable CancelSend command and
/// cannot revoke committed READY rows or an already-authorized HTTP attempt.
#[derive(Clone, Default)]
pub struct Cancellation {
    inner: Arc<CancelState>,
}
#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    notify: Notify,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.notify.notify_waiters();
    }
    fn check(&self) -> Result<()> {
        ensure!(
            !self.inner.cancelled.load(Ordering::Acquire),
            "Crypto preparation cancelled"
        );
        Ok(())
    }
    async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.inner.cancelled.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

/// Upstream authenticated device/bundle policy supplies this decision. Existing
/// bindings are always read; a missing record never silently approves a new key.
pub struct Recipient {
    pub trust: engine::TrustDecision,
    pub bundle: Option<engine::PeerBundle>,
}

/// Minted only after the coordinator checks its own submitted job context.
/// The wire adapter reads ciphertext directly from this sealed engine result.
pub struct VerifiedPersonalResult {
    result: engine::OperationResult,
    address: signal::Address,
}
impl VerifiedPersonalResult {
    pub(crate) fn result(&self) -> &engine::OperationResult {
        &self.result
    }
    pub(crate) fn address(&self) -> &signal::Address {
        &self.address
    }
}

/// Non-constructible preparation capability. Only the store's verified commit
/// path may turn it into durable, restart-recoverable dispatch provenance.
pub struct VerifiedPreparedSend {
    delta: e2ee::Delta,
    client_id: u64,
    exact_sms: Vec<u8>,
    metadata: Vec<u8>,
    local: e2ee::LocalNamespace,
    peers: Vec<e2ee::PeerNamespace>,
    cancellation: Cancellation,
    deadline: Instant,
}
impl VerifiedPreparedSend {
    pub(crate) fn delta(&self) -> &e2ee::Delta {
        &self.delta
    }
    pub(crate) fn client_id(&self) -> u64 {
        self.client_id
    }
    pub(crate) fn exact_sms(&self) -> &[u8] {
        &self.exact_sms
    }
    pub(crate) fn metadata(&self) -> &[u8] {
        &self.metadata
    }
    pub(crate) fn local(&self) -> &e2ee::LocalNamespace {
        &self.local
    }
    pub(crate) fn peers(&self) -> &[e2ee::PeerNamespace] {
        &self.peers
    }
    pub(crate) fn check_current(&self, gate: &e2ee::GenerationGate) -> Result<()> {
        self.check_preparation()?;
        ensure!(
            gate.is_current(self.delta.generation),
            "Prepared crypto operation expired or changed generation"
        );
        Ok(())
    }
    pub(crate) fn check_preparation(&self) -> Result<()> {
        self.cancellation.check()?;
        ensure!(
            Instant::now() < self.deadline,
            "Prepared crypto operation expired"
        );
        Ok(())
    }
}

/// Actor-created immutable input. Owns committed records and routing decisions;
/// no SQLite connection or transaction is captured by asynchronous preparation.
pub struct PreparedPersonalInput {
    client_id: u64,
    recipients: Vec<Recipient>,
    scope: signal::Scope,
    thread: String,
    local: e2ee::LocalNamespace,
    working: engine::Snapshot,
    allowed_keys: Vec<e2ee::RecordKey>,
    durable_id: u64,
    input_digest: [u8; 32],
    sender_iid: u16,
    business: signal::BusinessText,
    deadline: Instant,
}

pub struct Coordinator {
    account: engine::AccountId,
    generation: u64,
    local: engine::LocalNamespace,
    worker: AccountEngine,
    last_job: u64,
}

/// One factory per account/auth lifecycle. Share this handle between active
/// personal work and pending creation instead of allocating another JS runtime.
#[derive(Clone)]
pub struct AccountEngine {
    inner: Arc<AccountEngineInner>,
}
struct AccountEngineInner {
    account: engine::AccountId,
    generation: u64,
    engine: engine::Engine,
    next_job: Mutex<u64>,
}
struct CompletedJob {
    result: engine::OperationResult,
    job: u64,
}
impl AccountEngine {
    pub fn new(account: engine::AccountId, generation: u64) -> Result<Self> {
        let engine = engine::Engine::new(
            account.clone(),
            generation,
            engine::EngineOptions::default(),
        )
        .map_err(crypto_error)?;
        Ok(Self {
            inner: Arc::new(AccountEngineInner {
                account,
                generation,
                engine,
                next_job: Mutex::new(0),
            }),
        })
    }
    pub fn coordinator(&self, local: engine::LocalNamespace) -> Coordinator {
        Coordinator {
            account: self.inner.account.clone(),
            generation: self.inner.generation,
            local,
            worker: self.clone(),
            last_job: 0,
        }
    }
    pub fn provisioner(&self) -> provisioning::PendingProvisioner {
        provisioning::PendingProvisioner::from_worker(self.clone())
    }
    pub fn invalidate(&self, generation: u64) -> Result<()> {
        self.inner
            .engine
            .advance_generation(generation)
            .map_err(crypto_error)
    }
    async fn execute(
        &self,
        operation: engine::Operation,
        snapshot: engine::Snapshot,
        gate: &e2ee::GenerationGate,
        cancel: &Cancellation,
        deadline: Instant,
    ) -> Result<CompletedJob> {
        cancel.check()?;
        ensure!(
            gate.is_current(self.inner.generation)
                && snapshot.account() == &self.inner.account
                && Instant::now() < deadline,
            "Account engine request scope is stale"
        );
        let revision = snapshot.revision();
        // Both allocation and submission share this short synchronous lock.
        // Cross-thread producers cannot submit job2 before job1. No JS, SQL or
        // await runs while holding it; the snapshot is already owned.
        let (job, ticket) = {
            let mut sequence = self
                .inner
                .next_job
                .lock()
                .map_err(|_| anyhow!("Account engine job allocator failed"))?;
            *sequence = sequence
                .checked_add(1)
                .ok_or_else(|| anyhow!("Account engine job counter exhausted"))?;
            let job = *sequence;
            let ticket = self
                .inner
                .engine
                .submit(engine::Request {
                    account: self.inner.account.clone(),
                    generation: self.inner.generation,
                    operation_id: job,
                    expected_revision: revision,
                    operation,
                    snapshot,
                })
                .map_err(crypto_error)?;
            (job, ticket)
        };
        let result = tokio::select! {
            result=timeout_at(deadline,ticket)=>result.map_err(|_|anyhow!("Account engine preparation deadline exceeded"))?.map_err(crypto_error)?,
            _=cancel.cancelled()=>return Err(anyhow!("Account engine preparation cancelled")),
        };
        cancel.check()?;
        ensure!(
            gate.is_current(self.inner.generation) && Instant::now() < deadline,
            "Account engine result is stale"
        );
        ensure!(
            result.account() == &self.inner.account
                && result.generation() == self.inner.generation
                && result.operation_id() == job
                && result.expected_revision() == revision,
            "Account engine result context mismatch"
        );
        Ok(CompletedJob { result, job })
    }
}
fn crypto_error(error: engine::EngineError) -> anyhow::Error {
    anyhow!("Signal engine rejected operation: {error:?}")
}
fn store_key(key: &engine::RecordKey) -> Result<e2ee::RecordKey> {
    Ok(serde_json::from_slice(&serde_json::to_vec(key)?)?)
}
fn store_local(local: &engine::LocalNamespace) -> Result<e2ee::LocalNamespace> {
    Ok(serde_json::from_slice(&serde_json::to_vec(local)?)?)
}
fn personal_keys(
    local: &engine::LocalNamespace,
    peer: &engine::PeerNamespace,
) -> Vec<engine::RecordKey> {
    vec![
        engine::RecordKey::LocalIdentity {
            local: local.clone(),
        },
        engine::RecordKey::Session {
            local: local.clone(),
            peer: peer.clone(),
        },
        engine::RecordKey::PeerIdentity {
            local: local.clone(),
            peer: peer.clone(),
        },
        engine::RecordKey::SessionTimestamp {
            local: local.clone(),
            peer: peer.clone(),
        },
    ]
}
fn required_record(snapshot: &engine::Snapshot, key: &engine::RecordKey) -> Result<()> {
    ensure!(
        snapshot.records().iter().any(|record| record.key() == key),
        "Required committed crypto binding is missing"
    );
    Ok(())
}
fn digest_part(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

impl Coordinator {
    pub fn new(
        account: engine::AccountId,
        generation: u64,
        local: engine::LocalNamespace,
    ) -> Result<Self> {
        Ok(AccountEngine::new(account, generation)?.coordinator(local))
    }
    pub fn invalidate(&self, generation: u64) -> Result<()> {
        self.worker.invalidate(generation)
    }
    fn current(&self, gate: &e2ee::GenerationGate, cancel: &Cancellation) -> Result<()> {
        cancel.check()?;
        ensure!(
            gate.is_current(self.generation),
            "Crypto generation is stale"
        );
        Ok(())
    }
    async fn run(
        &mut self,
        operation: engine::Operation,
        snapshot: &engine::Snapshot,
        gate: &e2ee::GenerationGate,
        cancel: &Cancellation,
        deadline: Instant,
    ) -> Result<engine::OperationResult> {
        self.current(gate, cancel)?;
        let completed = self
            .worker
            .execute(operation, snapshot.duplicate(), gate, cancel, deadline)
            .await?;
        self.last_job = completed.job;
        Ok(completed.result)
    }

    /// Prepare all recipient ratchets from ONE committed base revision. This
    /// neither commits nor dispatches; failure drops every intermediate overlay.
    /// Empty styles are skipped exactly as the original client does.
    pub fn plan_personal(
        &self,
        store: &mut MessageStore,
        client_id: u64,
        text: &str,
        mut recipients: Vec<Recipient>,
        gate: &e2ee::GenerationGate,
        cancel: &Cancellation,
    ) -> Result<PreparedPersonalInput> {
        let deadline = Instant::now() + BATCH_TIMEOUT;
        self.current(gate, cancel)?;
        ensure!(
            !recipients.is_empty() && recipients.len() <= MAX_DEVICES,
            "Invalid encrypted recipient count"
        );
        let reservation = store.e2ee_outgoing(self.account.as_str(), client_id)?;
        let local = store_local(&self.local)?;
        ensure!(
            reservation.state == e2ee::SendState::Reserved && reservation.local == local,
            "Encrypted send reservation is not current"
        );
        let thread = reservation
            .thread
            .strip_prefix("d:")
            .ok_or_else(|| anyhow!("Personal send requires a direct thread"))?;
        ensure!(
            store.e2ee_reserved_plaintext(self.account.as_str(), client_id)? == text,
            "Encrypted draft differs from its durable reservation"
        );
        let scope = signal::Scope::new(self.account.as_str(), self.generation)?;
        let business = signal::encode_personal_business(&scope, thread, client_id, text)?;
        recipients.sort_by(|a, b| a.trust.peer.cmp(&b.trust.peer));
        let mut unique = BTreeSet::new();
        let mut planned = BTreeSet::new();
        let mut hash = Sha256::new();
        digest_part(&mut hash, b"zalo-personal-preparation-v1");
        digest_part(&mut hash, self.account.as_str().as_bytes());
        digest_part(&mut hash, reservation.thread.as_bytes());
        hash.update(client_id.to_be_bytes());
        digest_part(&mut hash, &serde_json::to_vec(&self.local)?);
        digest_part(&mut hash, business.as_bytes());
        for recipient in &recipients {
            let trust = &recipient.trust;
            let peer: e2ee::PeerNamespace =
                serde_json::from_value(serde_json::to_value(&trust.peer)?)?;
            store.e2ee_validate_peer_identity(
                self.account.as_str(),
                &local,
                &peer,
                trust.registration_id,
                &trust.identity_key,
                trust.session_timestamp,
            )?;
            ensure!(
                (trust.peer.uid.as_str() == thread || trust.peer.uid == self.account)
                    && !(trust.peer.uid == self.account
                        && trust.peer.platform == self.local.platform),
                "Encrypted recipient scope mismatch"
            );
            ensure!(
                unique.insert((trust.peer.uid.clone(), trust.peer.platform)),
                "Duplicate encrypted recipient"
            );
            for key in personal_keys(&self.local, &trust.peer) {
                planned.insert(key);
            }
            digest_part(&mut hash, &serde_json::to_vec(&trust.peer)?);
            hash.update(trust.registration_id.to_be_bytes());
            hash.update(trust.identity_key);
            hash.update(trust.session_timestamp.to_be_bytes());
            // Logical input binds user intent and stable identity/epoch routing.
            // A fresh authenticated541 fetch may choose a different one-time key
            // for a known-unsent retry; ciphertext/delta are bound at finalCAS.
        }
        ensure!(
            recipients
                .iter()
                .any(|recipient| recipient.trust.peer.uid.as_str() == thread),
            "Encrypted fanout lacks its peer"
        );
        let input_digest: [u8; 32] = hash.finalize().into();
        let durable_id = store.e2ee_reserve_operation(
            self.account.as_str(),
            e2ee::OperationIdentity::Send { client_id },
            &input_digest,
        )?;
        let allowed_keys = planned.iter().map(store_key).collect::<Result<Vec<_>>>()?;
        let snapshot = store.e2ee_snapshot(self.account.as_str(), &allowed_keys)?;
        let working = e2ee::engine_adapter::into_engine_snapshot(snapshot)?;
        required_record(
            &working,
            &engine::RecordKey::LocalIdentity {
                local: self.local.clone(),
            },
        )?;
        let local_identity = working
            .records()
            .iter()
            .find(|record| matches!(record.key(), engine::RecordKey::LocalIdentity { .. }))
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(local_identity.value())?;
        let sender_iid = value["registrationId"]
            .as_u64()
            .filter(|value| *value <= 16383)
            .ok_or_else(|| anyhow!("Invalid local registration binding"))?
            as u16;
        let public = value["publicKey"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid local identity public key"))?;
        let public: Vec<u8> = public
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .filter(|value| *value <= 255)
                    .map(|value| value as u8)
                    .ok_or_else(|| anyhow!("Invalid local identity public key"))
            })
            .collect::<Result<_>>()?;
        let public: [u8; 33] = public
            .try_into()
            .map_err(|_| anyhow!("Invalid local identity public key length"))?;
        store.e2ee_validate_local_identity(self.account.as_str(), &local, sender_iid, &public)?;
        Ok(PreparedPersonalInput {
            client_id,
            recipients,
            scope,
            thread: thread.into(),
            local,
            working,
            allowed_keys,
            durable_id,
            input_digest,
            sender_iid,
            business,
            deadline,
        })
    }
    /// Runs exclusively on owned input, without borrowing the SQLite actor.
    pub async fn prepare_personal(
        &mut self,
        plan: PreparedPersonalInput,
        gate: &e2ee::GenerationGate,
        cancel: &Cancellation,
    ) -> Result<VerifiedPreparedSend> {
        self.current(gate, cancel)?;
        let PreparedPersonalInput {
            client_id,
            recipients,
            scope,
            thread,
            local,
            mut working,
            allowed_keys,
            durable_id,
            input_digest,
            sender_iid,
            business,
            deadline,
        } = plan;
        ensure!(
            scope.account() == self.account.as_str()
                && scope.generation() == self.generation
                && local == store_local(&self.local)?,
            "Crypto plan scope mismatch"
        );
        ensure!(
            Instant::now() < deadline,
            "Crypto preparation deadline exceeded"
        );
        let base_revision = working.revision();
        let mut aggregate = BTreeMap::new();
        let mut ciphertexts = Vec::new();
        for recipient in recipients {
            let keys = personal_keys(&self.local, &recipient.trust.peer)
                .iter()
                .map(store_key)
                .collect::<Result<Vec<_>>>()?;
            if let Some(bundle) = recipient.bundle {
                let result = self
                    .run(
                        engine::Operation::ProcessBundle {
                            local: self.local.clone(),
                            trust: copy_trust(&recipient.trust),
                            bundle,
                        },
                        &working,
                        gate,
                        cancel,
                        deadline,
                    )
                    .await?;
                merge_result(
                    result,
                    context(self, durable_id, base_revision, &keys, input_digest),
                    &mut working,
                    &mut aggregate,
                )?;
            } else {
                required_record(
                    &working,
                    &engine::RecordKey::PeerIdentity {
                        local: self.local.clone(),
                        peer: recipient.trust.peer.clone(),
                    },
                )?;
                required_record(
                    &working,
                    &engine::RecordKey::Session {
                        local: self.local.clone(),
                        peer: recipient.trust.peer.clone(),
                    },
                )?;
            }
            let result = self
                .run(
                    engine::Operation::EncryptPersonal {
                        local: self.local.clone(),
                        trust: copy_trust(&recipient.trust),
                        plaintext: business.as_bytes().to_vec(),
                    },
                    &working,
                    gate,
                    cancel,
                    deadline,
                )
                .await?;
            let address = signal::Address::new(
                &scope,
                recipient.trust.peer.uid.as_str(),
                wire_platform(recipient.trust.peer.platform),
                self.local.epoch.as_str(),
                recipient.trust.peer.epoch.as_str(),
            )?;
            let verified = VerifiedPersonalResult { result, address };
            ciphertexts.push(signal::engine_adapter::personal(&verified)?);
            merge_result(
                verified.result,
                context(self, durable_id, base_revision, &keys, input_digest),
                &mut working,
                &mut aggregate,
            )?;
        }
        self.current(gate, cancel)?;
        ensure!(
            Instant::now() < deadline,
            "Crypto preparation deadline exceeded"
        );
        let sms = signal::personal_sms(&scope, &thread, client_id, Some(sender_iid), &ciphertexts)?;
        let exact_sms = sms.exact_json()?;
        let metadata = serde_json::to_vec(
            &serde_json::json!({"codec":1,"kind":"personal","path":sms.path,"metric_command":sms.metric_command}),
        )?;
        let peers = recipients_for_keys(&allowed_keys);
        Ok(VerifiedPreparedSend {
            delta: e2ee::Delta {
                account: self.account.as_str().into(),
                expected_revision: base_revision,
                generation: self.generation,
                operation_id: durable_id,
                input_digest,
                allowed_keys,
                activate_peers: vec![],
                mutations: aggregate.into_values().collect(),
            },
            client_id,
            exact_sms,
            metadata,
            local,
            peers,
            cancellation: cancel.clone(),
            deadline,
        })
    }
}
fn copy_trust(value: &engine::TrustDecision) -> engine::TrustDecision {
    engine::TrustDecision {
        peer: value.peer.clone(),
        registration_id: value.registration_id,
        identity_key: value.identity_key,
        session_timestamp: value.session_timestamp,
        basis: value.basis,
    }
}
fn wire_platform(value: engine::SignalPlatform) -> signal::DevicePlatform {
    match value {
        engine::SignalPlatform::Mobile => signal::DevicePlatform::Mobile,
        engine::SignalPlatform::Web => signal::DevicePlatform::Web,
        engine::SignalPlatform::Pc => signal::DevicePlatform::Pc,
    }
}
fn context(
    coordinator: &Coordinator,
    durable_id: u64,
    revision: u64,
    keys: &[e2ee::RecordKey],
    input_digest: [u8; 32],
) -> e2ee::engine_adapter::ExpectedJobContext {
    e2ee::engine_adapter::ExpectedJobContext {
        account: coordinator.account.as_str().into(),
        generation: coordinator.generation,
        engine_job_id: coordinator.last_job,
        durable_operation_id: durable_id,
        expected_revision: revision,
        local: store_local(&coordinator.local).expect("validated namespace"),
        allowed_keys: keys.to_vec(),
        input_digest,
        activate_peers: vec![],
    }
}
fn merge_result(
    result: engine::OperationResult,
    context: e2ee::engine_adapter::ExpectedJobContext,
    working: &mut engine::Snapshot,
    aggregate: &mut BTreeMap<e2ee::RecordKey, e2ee::Mutation>,
) -> Result<()> {
    let (_, delta) = e2ee::engine_adapter::into_store_delta_checked(result, context)?;
    let mut records: BTreeMap<_, _> = working
        .records()
        .iter()
        .map(|record| (record.key().clone(), record.value().to_vec()))
        .collect();
    for mutation in delta.mutations {
        let key = match &mutation {
            e2ee::Mutation::Put(record) => {
                let key =
                    serde_json::from_slice::<engine::RecordKey>(&serde_json::to_vec(&record.key)?)?;
                records.insert(key, record.value.clone());
                record.key.clone()
            }
            e2ee::Mutation::Delete(key) => {
                let engine_key =
                    serde_json::from_slice::<engine::RecordKey>(&serde_json::to_vec(key)?)?;
                records.remove(&engine_key);
                key.clone()
            }
        };
        aggregate.insert(key, mutation);
    }
    *working = engine::Snapshot::new(
        working.account().clone(),
        working.revision(),
        records
            .into_iter()
            .map(|(key, value)| engine::Record::from_parts(key, value).map_err(crypto_error))
            .collect::<Result<_>>()?,
    )
    .map_err(crypto_error)?;
    Ok(())
}
fn recipients_for_keys(keys: &[e2ee::RecordKey]) -> Vec<e2ee::PeerNamespace> {
    keys.iter()
        .filter_map(|key| match key {
            e2ee::RecordKey::Session { peer, .. }
            | e2ee::RecordKey::PeerIdentity { peer, .. }
            | e2ee::RecordKey::SessionTimestamp { peer, .. } => Some(peer.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Public, unauthenticated protobuf references are used ONLY to select committed
/// keys. MAC/identity/ratchet validation remains in the existing engine. Reject
/// duplicate, noncanonical, oversized or unsupported fields before a DB lookup.
pub fn required_personal_records(
    local: &engine::LocalNamespace,
    trust: &engine::TrustDecision,
    message: &engine::PersonalCiphertext,
) -> Result<Vec<e2ee::RecordKey>> {
    ensure!(
        message.body.len() > 9
            && message.body.len() <= engine::MAX_CIPHERTEXT_BYTES
            && message.body[0] == 0x33,
        "Invalid personal cipher framing"
    );
    let mut keys = personal_keys(local, &trust.peer);
    if message.message_type == engine::PersonalMessageType::PreKey {
        let mut cursor = 1usize;
        let mut seen = BTreeSet::new();
        let mut signed = None;
        let mut one_time = None;
        while cursor < message.body.len() {
            let tag = read_varint(&message.body, &mut cursor)?;
            let field = tag >> 3;
            let wire = tag & 7;
            ensure!(
                (1..=6).contains(&field) && seen.insert(field),
                "Unsupported or duplicate prekey header field"
            );
            match (field, wire) {
                (1 | 5 | 6, 0) => {
                    let value = u32::try_from(read_varint(&message.body, &mut cursor)?)
                        .map_err(|_| anyhow!("Prekey header ID exceeds uint32"))?;
                    if field == 1 {
                        ensure!(value > 0, "Invalid one-time prekey record ID");
                        one_time = Some(value)
                    }
                    if field == 6 {
                        ensure!(value > 0, "Invalid signed prekey record ID");
                        signed = Some(value)
                    }
                    if field == 5 {
                        ensure!(
                            value <= 16383 && value == u32::from(trust.registration_id),
                            "Invalid or uncorrelated peer registration ID"
                        );
                    }
                }
                (2..=4, 2) => {
                    let length = usize::try_from(read_varint(&message.body, &mut cursor)?)?;
                    let end = cursor
                        .checked_add(length)
                        .filter(|end| *end <= message.body.len())
                        .ok_or_else(|| anyhow!("Truncated prekey header"))?;
                    ensure!(length > 0, "Empty prekey header field");
                    if field == 2 || field == 3 {
                        ensure!(
                            length == 33 && message.body[cursor] == 5,
                            "Invalid prekey public key framing"
                        );
                    }
                    if field == 3 {
                        ensure!(
                            message.body[cursor..end] == trust.identity_key,
                            "Uncorrelated peer identity in prekey header"
                        );
                    }
                    cursor = end;
                }
                _ => return Err(anyhow!("Unsupported prekey header wire type")),
            }
        }
        ensure!(
            seen.contains(&2) && seen.contains(&3) && seen.contains(&4) && seen.contains(&5),
            "Incomplete prekey header"
        );
        keys.push(engine::RecordKey::SignedPreKey {
            local: local.clone(),
            id: signed.ok_or_else(|| anyhow!("Missing signed prekey ID"))?,
        });
        if let Some(id) = one_time {
            keys.push(engine::RecordKey::PreKey {
                local: local.clone(),
                id,
            })
        }
    }
    keys.iter().map(store_key).collect()
}
fn read_varint(body: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for index in 0..10 {
        let byte = *body
            .get(*cursor)
            .ok_or_else(|| anyhow!("Truncated prekey varint"))?;
        *cursor += 1;
        ensure!(index < 9 || byte <= 1, "Prekey varint overflow");
        value |= u64::from(byte & 127) << (index * 7);
        if byte & 128 == 0 {
            ensure!(index == 0 || byte != 0, "Noncanonical prekey varint");
            return Ok(value);
        }
    }
    Err(anyhow!("Oversized prekey varint"))
}

#[cfg(test)]
mod account_worker_tests {
    use super::*;
    #[test]
    fn shared_worker_serializes_cross_thread_job_allocation_and_submission() {
        let worker = AccountEngine::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(6));
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let worker = worker.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    barrier.wait();
                    runtime.block_on(async move {
                        let completed = worker
                            .execute(
                                engine::Operation::CreateBundle {
                                    local: engine::LocalNamespace {
                                        epoch: engine::RegistrationEpoch::new(1).unwrap(),
                                        platform: engine::SignalPlatform::Pc,
                                    },
                                    prekey_count: 1,
                                },
                                engine::Snapshot::new(
                                    engine::AccountId::new("101").unwrap(),
                                    0,
                                    vec![],
                                )
                                .unwrap(),
                                &e2ee::GenerationGate::new(1).unwrap(),
                                &Cancellation::default(),
                                Instant::now() + BATCH_TIMEOUT,
                            )
                            .await
                            .unwrap();
                        assert_eq!(completed.result.operation_id(), completed.job);
                        completed.job
                    })
                })
            })
            .collect();
        let jobs: BTreeSet<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(jobs.into_iter().eq(1..=6));
    }
}
