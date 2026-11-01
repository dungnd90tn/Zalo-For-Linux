//! Inactive Signal provisioning coordinator. No run_inner routing/startup action.
//! Future integration is deliberately absent; tests exercise this module directly.
#![allow(dead_code)]
use super::Connection;
use crate::{
    protocol::{
        messaging::DecodedFrame,
        signal::{
            self,
            provisioning::{self as wire, CandidateIdentity, Reconciliation, UploadState},
            Scope, SignalError, SignalRequest,
        },
    },
    store::e2ee::{provisioning::PublicUploadReceipt, GenerationGate},
};
use rand::{rngs::OsRng, RngCore};
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};

const MAX_REQUESTS: usize = 16;
const MAX_PENDING_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Never resets on reconnect. Upload IDs come from the durable database claim;
/// query IDs occupy a separate CSPRNG namespace for each process/coordinator.
struct QueryIds {
    prefix: String,
    next: u64,
}
impl QueryIds {
    fn new() -> Self {
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        let prefix = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Self { prefix, next: 0 }
    }
    fn next(&mut self) -> Result<String, SignalError> {
        let n = self.next;
        self.next = self.next.checked_add(1).ok_or(SignalError::Resource)?;
        Ok(format!("signal_req_q{}_{n}", self.prefix))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SocketEpoch(u64);
impl SocketEpoch {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}
struct SocketClock(u64);
impl SocketClock {
    fn next(&mut self) -> Result<SocketEpoch, SignalError> {
        self.0 = self.0.checked_add(1).ok_or(SignalError::Resource)?;
        Ok(SocketEpoch(self.0))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryPurpose {
    Upload,
    MetadataBefore,
    Identity,
    MetadataAfter,
}
struct Pending {
    request: SignalRequest,
    purpose: QueryPurpose,
    socket: SocketEpoch,
    deadline: Instant,
    bytes: usize,
}
struct Upload {
    receipt: PublicUploadReceipt,
    reconciliation: Reconciliation,
}
/// The only server-evidence capability. No Clone/Serialize, constructor or raw
/// reply factory is exposed. It owns the exact immutable DB upload claim.
pub(crate) struct CorrelatedReplyProof {
    binding: AttemptBinding,
    scope: Scope,
    socket: SocketEpoch,
    timestamp: Option<u64>,
    acknowledged: bool,
    identity_confirmed: bool,
}
impl CorrelatedReplyProof {
    pub(crate) fn account_binding(&self) -> &str {
        &self.binding.account
    }
    pub(crate) fn local(&self) -> &crate::store::e2ee::LocalNamespace {
        &self.binding.local
    }
    pub(crate) fn operation_id(&self) -> u64 {
        self.binding.operation_id
    }
    pub(crate) fn attempt_id(&self) -> u64 {
        self.binding.attempt_id
    }
    pub(crate) fn attempt_generation(&self) -> u64 {
        self.binding.generation
    }
    pub(crate) fn attempt_socket_epoch(&self) -> u64 {
        self.binding.socket_epoch
    }
    pub(crate) fn request_id(&self) -> &str {
        &self.binding.request_id
    }
    pub(crate) fn payload_hash(&self) -> &[u8; 32] {
        &self.binding.payload_hash
    }
    pub(crate) fn attempt_hash(&self) -> &[u8; 32] {
        &self.binding.attempt_hash
    }
    pub(crate) fn account(&self) -> &str {
        self.scope.account()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.scope.generation()
    }
    pub(crate) fn socket_epoch(&self) -> u64 {
        self.socket.0
    }
    pub(crate) fn timestamp(&self) -> Option<u64> {
        self.timestamp
    }
    pub(crate) fn acknowledged(&self) -> bool {
        self.acknowledged
    }
    pub(crate) fn identity_confirmed(&self) -> bool {
        self.identity_confirmed
    }
    pub(crate) fn current(&self, gate: &GenerationGate) -> bool {
        gate.is_current(self.scope.generation())
    }
}
struct AttemptBinding {
    account: String,
    local: crate::store::e2ee::LocalNamespace,
    operation_id: u64,
    attempt_id: u64,
    generation: u64,
    socket_epoch: u64,
    request_id: String,
    payload_hash: [u8; 32],
    attempt_hash: [u8; 32],
}
impl AttemptBinding {
    fn from_receipt(receipt: &PublicUploadReceipt) -> Self {
        Self {
            account: receipt.account().into(),
            local: receipt.local().clone(),
            operation_id: receipt.operation_id(),
            attempt_id: receipt.attempt_id(),
            generation: receipt.generation(),
            socket_epoch: receipt.socket_epoch(),
            request_id: receipt.request_id().into(),
            payload_hash: *receipt.payload_hash(),
            attempt_hash: *receipt.attempt_hash(),
        }
    }
}
fn proof(
    upload: &Upload,
    scope: &Scope,
    socket: SocketEpoch,
    timestamp: Option<u64>,
    identity_confirmed: bool,
) -> CorrelatedReplyProof {
    CorrelatedReplyProof {
        binding: AttemptBinding::from_receipt(&upload.receipt),
        scope: scope.clone(),
        socket,
        timestamp,
        acknowledged: upload.reconciliation.prekeys() == wire::PrekeyOutcome::Acknowledged,
        identity_confirmed,
    }
}
/// A bounded write description, not permission to make another upload attempt.
struct Write {
    command: u16,
    body: serde_json::Value,
    scope: Scope,
    socket: SocketEpoch,
    request_id: String,
}
impl Write {
    async fn send(
        self,
        connection: &mut Connection,
        coordinator: &Coordinator,
        gate: &GenerationGate,
    ) -> anyhow::Result<()> {
        coordinator.current(gate)?;
        if self.scope.account() != coordinator.scope.account()
            || self.scope.generation() != coordinator.scope.generation()
            || coordinator.socket != Some(self.socket)
            || !connection.decoder.authenticated()
        {
            return Err(SignalError::Scope.into());
        }
        let pending = coordinator
            .pending
            .get(&self.request_id)
            .ok_or(SignalError::Correlation)?;
        if pending.socket != self.socket
            || pending.deadline <= Instant::now()
            || pending.request.command() as u16 != self.command
        {
            return Err(SignalError::OutOfOrder.into());
        }
        connection.send(self.command, 0, self.body).await
    }
}
enum ReadOutcome {
    Unhandled(DecodedFrame),
    Progress,
    Proof(CorrelatedReplyProof),
    NoData,
}
/// Inert until explicitly driven by a future runtime integration. There is no
/// startup constructor call, background task, HTTP sender or ratchet mutation.
struct Coordinator {
    scope: Scope,
    ids: QueryIds,
    clock: SocketClock,
    socket: Option<SocketEpoch>,
    pending: BTreeMap<String, Pending>,
    pending_bytes: usize,
    upload: Option<Upload>,
    cancelled: bool,
}
impl Coordinator {
    fn new(scope: Scope) -> Self {
        Self {
            scope,
            ids: QueryIds::new(),
            clock: SocketClock(0),
            socket: None,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            upload: None,
            cancelled: false,
        }
    }
    fn connected(&mut self, gate: &GenerationGate) -> Result<SocketEpoch, SignalError> {
        self.current(gate)?;
        self.disconnected();
        let epoch = self.clock.next()?;
        self.socket = Some(epoch);
        Ok(epoch)
    }
    fn current(&self, gate: &GenerationGate) -> Result<(), SignalError> {
        if self.cancelled || !gate.is_current(self.scope.generation()) {
            Err(SignalError::Scope)
        } else {
            Ok(())
        }
    }
    fn enqueue(
        &mut self,
        request: SignalRequest,
        purpose: QueryPurpose,
        now: Instant,
    ) -> Result<Write, SignalError> {
        let socket = self.socket.ok_or(SignalError::OutOfOrder)?;
        let bytes = request.frame(&self.scope)?.len();
        if self.pending.len() >= MAX_REQUESTS
            || self
                .pending_bytes
                .checked_add(bytes)
                .is_none_or(|sum| sum > MAX_PENDING_BYTES)
        {
            return Err(SignalError::Resource);
        }
        if self.pending.contains_key(request.request_id()) {
            return Err(SignalError::Duplicate);
        }
        let command = request.command() as u16;
        let body = request.wire_data(&self.scope)?;
        let id = request.request_id().to_owned();
        self.pending.insert(
            id.clone(),
            Pending {
                request,
                purpose,
                socket,
                deadline: now + REQUEST_TIMEOUT,
                bytes,
            },
        );
        self.pending_bytes += bytes;
        Ok(Write {
            command,
            body,
            scope: self.scope.clone(),
            socket,
            request_id: id,
        })
    }
    fn upload(
        &mut self,
        receipt: PublicUploadReceipt,
        gate: &GenerationGate,
        now: Instant,
    ) -> Result<Write, SignalError> {
        self.current(gate)?;
        if self.upload.is_some()
            || receipt.account() != self.scope.account()
            || receipt.generation() != self.scope.generation()
            || !receipt.can_submit(gate)
            || self.socket.map(|x| x.0) != Some(receipt.socket_epoch())
        {
            return Err(SignalError::Scope);
        }
        let identity = CandidateIdentity::from_public_payload(receipt.payload())?;
        if identity.payload_hash() != receipt.payload_hash() {
            return Err(SignalError::Invalid);
        }
        let request = wire::upload_request(&self.scope, receipt.request_id(), receipt.payload())?;
        let write = self.enqueue(request, QueryPurpose::Upload, now)?;
        self.upload = Some(Upload {
            receipt,
            reconciliation: Reconciliation::new(identity),
        });
        Ok(write)
    }
    fn state(&self) -> Option<UploadState> {
        self.upload.as_ref().map(|u| u.reconciliation.state())
    }
    fn disconnected(&mut self) {
        self.socket = None;
        self.pending.clear();
        self.pending_bytes = 0;
        if let Some(upload) = &mut self.upload {
            upload.reconciliation.unknown();
        }
    }
    fn cancel(&mut self) {
        self.cancelled = true;
        self.disconnected();
        if let Some(upload) = &mut self.upload {
            upload.reconciliation.cancel();
        }
    }
    fn expire(&mut self, now: Instant) -> bool {
        if self.pending.values().any(|p| p.deadline <= now) {
            self.disconnected();
            true
        } else {
            false
        }
    }
    fn reconcile(&mut self, gate: &GenerationGate, now: Instant) -> Result<Write, SignalError> {
        self.current(gate)?;
        if !matches!(
            self.state(),
            Some(UploadState::Unknown | UploadState::AcceptedNeedsMetadata)
        ) || !self.pending.is_empty()
        {
            return Err(SignalError::OutOfOrder);
        }
        let id = self.ids.next()?;
        let request = wire::own_query(&self.scope, &id)?;
        self.enqueue(request, QueryPurpose::MetadataBefore, now)
    }
    /// Reads evidence through the production Connection/FrameDecoder. A caller
    /// cannot supply a JSON value, fake authenticated boolean or arbitrary hash.
    async fn read_next(
        &mut self,
        connection: &mut Connection,
        epoch: SocketEpoch,
        gate: &GenerationGate,
    ) -> anyhow::Result<ReadOutcome> {
        self.current(gate)?;
        if self.socket != Some(epoch) {
            return Err(SignalError::Scope.into());
        }
        let frame = if let Some(deadline) = self.pending.values().map(|p| p.deadline).min() {
            match tokio::time::timeout_at(deadline, connection.next()).await {
                Ok(frame) => frame?,
                Err(_) => {
                    self.disconnected();
                    return Err(SignalError::OutOfOrder.into());
                }
            }
        } else {
            connection.next().await?
        };
        self.current(gate)?;
        if !connection.decoder.authenticated() {
            return Err(SignalError::Authentication.into());
        }
        let Some(frame) = frame else {
            return Ok(ReadOutcome::NoData);
        };
        self.accept(frame, epoch, gate).map_err(Into::into)
    }
    /// Private: reachable only after read_next obtains authenticated decoded data.
    fn accept(
        &mut self,
        frame: DecodedFrame,
        epoch: SocketEpoch,
        gate: &GenerationGate,
    ) -> Result<ReadOutcome, SignalError> {
        self.current(gate)?;
        if self.socket != Some(epoch) {
            return Err(SignalError::Scope);
        }
        let Some(id) = frame
            .payload
            .pointer("/data/reqId")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(ReadOutcome::Unhandled(frame));
        };
        let Some(expected) = self.pending.get(id) else {
            return Err(SignalError::Correlation);
        };
        if expected.socket != epoch
            || frame.sub_cmd != 0
            || frame.cmd != expected.request.command() as u16
        {
            return Err(SignalError::Correlation);
        }
        if expected.deadline <= Instant::now() {
            self.disconnected();
            return Err(SignalError::OutOfOrder);
        }
        // Validate before consuming so a wrong request/command cannot eat the
        // legitimate response. Valid service rejection consumes exactly once.
        let parsed = expected
            .request
            .reply(&self.scope, frame.cmd, &frame.payload);
        let own = if matches!(
            expected.purpose,
            QueryPurpose::MetadataBefore | QueryPurpose::MetadataAfter
        ) {
            Some(signal::own_key_reply(
                &expected.request,
                &self.scope,
                &frame.payload,
            ))
        } else {
            None
        };
        let bundles = if expected.purpose == QueryPurpose::Identity {
            Some(signal::bundles_reply(
                &expected.request,
                &self.scope,
                &frame.payload,
                self.scope.account(),
                signal::DeviceSelection::All,
            ))
        } else {
            None
        };
        let pending = self.pending.remove(id).ok_or(SignalError::Correlation)?;
        self.pending_bytes -= pending.bytes;
        let Some(upload) = &mut self.upload else {
            return Err(SignalError::OutOfOrder);
        };
        match pending.purpose {
            QueryPurpose::Upload => {
                let data = match parsed {
                    Ok(data) => data,
                    Err(error) => {
                        upload.reconciliation.unknown();
                        return Err(error);
                    }
                };
                let timestamp = match upload.reconciliation.acknowledged(&data) {
                    Ok(timestamp) => timestamp,
                    Err(error) => {
                        upload.reconciliation.unknown();
                        return Err(error);
                    }
                };
                let evidence = proof(upload, &self.scope, epoch, timestamp, false);
                if timestamp.is_some() {
                    self.upload.take();
                }
                Ok(ReadOutcome::Proof(evidence))
            }
            QueryPurpose::MetadataBefore => {
                upload
                    .reconciliation
                    .begin_metadata(own.ok_or(SignalError::Invalid)??)?;
                Ok(ReadOutcome::Progress)
            }
            QueryPurpose::Identity => {
                upload
                    .reconciliation
                    .identity(&bundles.ok_or(SignalError::Invalid)??, self.scope.account())?;
                Ok(ReadOutcome::Progress)
            }
            QueryPurpose::MetadataAfter => {
                let timestamp = upload
                    .reconciliation
                    .finish_metadata(own.ok_or(SignalError::Invalid)??)?;
                let upload = self.upload.take().ok_or(SignalError::OutOfOrder)?;
                Ok(ReadOutcome::Proof(proof(
                    &upload,
                    &self.scope,
                    epoch,
                    Some(timestamp),
                    true,
                )))
            }
        }
    }
    fn identity_query(
        &mut self,
        gate: &GenerationGate,
        now: Instant,
    ) -> Result<Write, SignalError> {
        self.current(gate)?;
        if !self.pending.is_empty()
            || !self
                .upload
                .as_ref()
                .is_some_and(|upload| upload.reconciliation.needs_identity())
        {
            return Err(SignalError::OutOfOrder);
        }
        let id = self.ids.next()?;
        let request = wire::identity_query(&self.scope, &id)?;
        self.enqueue(request, QueryPurpose::Identity, now)
    }
    fn final_metadata_query(
        &mut self,
        gate: &GenerationGate,
        now: Instant,
    ) -> Result<Write, SignalError> {
        self.current(gate)?;
        if !self.pending.is_empty()
            || !self
                .upload
                .as_ref()
                .is_some_and(|upload| upload.reconciliation.needs_final_metadata())
        {
            return Err(SignalError::OutOfOrder);
        }
        let id = self.ids.next()?;
        let request = wire::own_query(&self.scope, &id)?;
        self.enqueue(request, QueryPurpose::MetadataAfter, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::live::signal_crypto::{provisioning::PendingProvisioner, Cancellation},
        store::{
            e2ee::provisioning::{CandidateRequest, PublicAttemptContext},
            MessageStore,
        },
    };
    use serde_json::{json, Value};
    use std::path::Path;
    pub(super) async fn claimed(epoch: u64) -> (MessageStore, PublicUploadReceipt, Value) {
        let mut store = MessageStore::open(Path::new(":memory:")).unwrap();
        let gate = GenerationGate::new(7).unwrap();
        let plan = store
            .e2ee_reserve_local_candidate(
                "11",
                &CandidateRequest {
                    platform: crate::store::e2ee::Platform::Pc,
                    expected_current: None,
                    stable_intent: "fixture-new-pc".into(),
                    prekey_count: 1,
                    generation: 7,
                },
                &gate,
            )
            .unwrap();
        let mut provisioner =
            PendingProvisioner::new(zalo_signal_engine::AccountId::new("11").unwrap(), 7).unwrap();
        let candidate = provisioner
            .prepare(plan, &gate, &Cancellation::default())
            .await
            .unwrap();
        let staged = store.e2ee_stage_verified_bundle(&candidate, &gate).unwrap();
        let receipt = store
            .e2ee_claim_bundle_upload(
                "11",
                staged.operation_id(),
                &PublicAttemptContext {
                    generation: 7,
                    socket_epoch: epoch,
                },
                &gate,
            )
            .unwrap();
        let payload = serde_json::from_slice(receipt.payload()).unwrap();
        (store, receipt, payload)
    }
    pub(super) fn coordinator() -> (Coordinator, GenerationGate, SocketEpoch) {
        let gate = GenerationGate::new(7).unwrap();
        let mut c = Coordinator::new(Scope::new("11", 7).unwrap());
        let epoch = c.connected(&gate).unwrap();
        (c, gate, epoch)
    }
    fn reply(write: &Write, data: Value) -> DecodedFrame {
        DecodedFrame {
            cmd: write.command,
            sub_cmd: 0,
            payload: json!({"error_code":0,"data":{"reqId":write.body["reqId"],"data":data}}),
        }
    }
    #[test]
    fn request_ids_socket_scope_and_byte_count_are_bounded_without_reconnect_reuse() {
        let (mut c, gate, old) = coordinator();
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..MAX_REQUESTS {
            let id = c.ids.next().unwrap();
            assert!(ids.insert(id.clone()));
            let req = wire::own_query(&c.scope, &id).unwrap();
            c.enqueue(req, QueryPurpose::MetadataBefore, Instant::now())
                .unwrap();
        }
        let id = c.ids.next().unwrap();
        assert!(matches!(
            c.enqueue(
                wire::own_query(&c.scope, &id).unwrap(),
                QueryPurpose::MetadataBefore,
                Instant::now()
            ),
            Err(SignalError::Resource)
        ));
        let new = c.connected(&gate).unwrap();
        assert!(new != old);
        let fresh = c.ids.next().unwrap();
        assert!(!ids.contains(&fresh));
        let another = QueryIds::new().next().unwrap();
        assert_ne!(fresh, another);
        c.pending_bytes = MAX_PENDING_BYTES;
        assert!(matches!(
            c.enqueue(
                wire::own_query(&c.scope, &fresh).unwrap(),
                QueryPurpose::MetadataBefore,
                Instant::now()
            ),
            Err(SignalError::Resource)
        ));
        let mut clock = SocketClock(u64::MAX);
        assert!(clock.next().is_err());
        c.ids.next = u64::MAX;
        assert!(c.ids.next().is_err());
        c.cancel();
        assert!(c.connected(&gate).is_err());
        assert!(c.state().is_none());
    }
    #[tokio::test]
    async fn upload_proof_is_exact_attempt_bound_one_use_and_missing_timestamp_never_fabricates_binding(
    ) {
        let (mut c, gate, epoch) = coordinator();
        let (mut store, receipt, _) = claimed(epoch.0).await;
        let hash = *receipt.payload_hash();
        let attempt = *receipt.attempt_hash();
        let id = receipt.request_id().to_owned();
        let write = c.upload(receipt, &gate, Instant::now()).unwrap();
        let mut wrong = reply(&write, json!({"ts":42}));
        wrong.sub_cmd = 1;
        assert!(matches!(
            c.accept(wrong, epoch, &gate),
            Err(SignalError::Correlation)
        ));
        assert_eq!(c.pending.len(), 1);
        let mut wrong = reply(&write, json!({"ts":42}));
        wrong.payload["data"]["reqId"] = json!("signal_req_wrong");
        assert!(matches!(
            c.accept(wrong, epoch, &gate),
            Err(SignalError::Correlation)
        ));
        assert_eq!(c.pending.len(), 1);
        let duplicate = reply(&write, json!({"err":0}));
        let ReadOutcome::Proof(proof) = c
            .accept(reply(&write, json!({"err":0})), epoch, &gate)
            .unwrap()
        else {
            panic!("expected NoTS ACK proof")
        };
        assert_eq!(proof.timestamp(), None);
        assert!(proof.acknowledged());
        assert!(!proof.identity_confirmed());
        assert!(proof.current(&gate));
        assert_eq!(proof.payload_hash(), &hash);
        assert_eq!(proof.attempt_hash(), &attempt);
        assert_eq!(proof.request_id(), id);
        assert_eq!(proof.account_binding(), "11");
        assert_eq!(c.state(), Some(UploadState::AcceptedNeedsMetadata));
        assert!(matches!(
            c.accept(duplicate, epoch, &gate),
            Err(SignalError::Correlation)
        ));
        assert!(store
            .e2ee_claim_bundle_upload(
                "11",
                proof.operation_id(),
                &PublicAttemptContext {
                    generation: 7,
                    socket_epoch: epoch.0
                },
                &gate
            )
            .is_err());
        let _ = store.e2ee_recover_bundle_uploads("11").unwrap();
        assert!(store
            .e2ee_claim_bundle_upload(
                "11",
                proof.operation_id(),
                &PublicAttemptContext {
                    generation: 7,
                    socket_epoch: epoch.0
                },
                &gate
            )
            .is_err());
        gate.cancel();
        assert!(!proof.current(&gate));
        assert!(c.reconcile(&gate, Instant::now()).is_err());
    }
    #[tokio::test]
    async fn unknown_upload_full_identity_and_stable_real_metadata_can_reconcile_but_iid_collision_cannot(
    ) {
        for collide in [false, true] {
            let (mut c, gate, epoch) = coordinator();
            let (_store, receipt, payload) = claimed(epoch.0).await;
            c.upload(receipt, &gate, Instant::now()).unwrap();
            c.disconnected();
            assert_eq!(c.state(), Some(UploadState::Unknown));
            let new = c.connected(&gate).unwrap();
            assert!(new != epoch);
            assert!(c.identity_query(&gate, Instant::now()).is_err());
            assert!(c.final_metadata_query(&gate, Instant::now()).is_err());
            let request = c.reconcile(&gate, Instant::now()).unwrap();
            let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
            c.accept(
                reply(&request, json!({"keyId":reg,"submitTs":1700000000000u64})),
                new,
                &gate,
            )
            .unwrap();
            let request = c.identity_query(&gate, Instant::now()).unwrap();
            assert_eq!(request.body["data"]["uids"][0]["deviceId"], -1);
            let mut pc = payload["key"].clone();
            pc["opKeys"] = json!([]);
            if collide {
                pc["preKey"]["iKey"]["key"] = json!(base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    [5u8; 33]
                ));
            }
            let mut mobile = payload["key"].clone();
            mobile["deviceId"] = json!(0);
            let mut web = mobile.clone();
            web["deviceId"] = json!(1);
            let response = reply(
                &request,
                json!({"keys":[{"uid":"11","keys":[mobile,pc,web]}]}),
            );
            if collide {
                assert!(c.accept(response, new, &gate).is_err());
                assert_eq!(c.state(), Some(UploadState::NeedsRecovery));
                continue;
            }
            c.accept(response, new, &gate).unwrap();
            let request = c.final_metadata_query(&gate, Instant::now()).unwrap();
            let ReadOutcome::Proof(proof) = c
                .accept(
                    reply(&request, json!({"keyId":reg,"submitTs":1700000000000u64})),
                    new,
                    &gate,
                )
                .unwrap()
            else {
                panic!("expected reconciled proof")
            };
            assert!(proof.identity_confirmed());
            assert!(!proof.acknowledged());
            assert_eq!(proof.timestamp(), Some(1700000000000));
            assert_eq!(proof.attempt_socket_epoch(), epoch.0);
            assert_eq!(proof.socket_epoch(), new.0);
        }
    }
    #[tokio::test]
    async fn stale_socket_cancel_deadline_and_changed_metadata_cannot_mint_proofs() {
        let (mut c, gate, epoch) = coordinator();
        let (_store, receipt, _) = claimed(epoch.0).await;
        let write = c.upload(receipt, &gate, Instant::now()).unwrap();
        assert!(matches!(
            c.accept(
                reply(&write, json!({"ts":42})),
                SocketEpoch(epoch.0 + 1),
                &gate
            ),
            Err(SignalError::Scope)
        ));
        assert!(c.expire(Instant::now() + REQUEST_TIMEOUT));
        assert_eq!(c.state(), Some(UploadState::Unknown));
        assert!(c.pending.is_empty());
        gate.replace(8).unwrap();
        assert!(c.connected(&gate).is_err());
        let value: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/signal/wire.json"))
                .unwrap();
        let payload = serde_json::to_vec(&value["requests"][0]["data"]["data"]).unwrap();
        let identity = CandidateIdentity::from_public_payload(&payload).unwrap();
        let mut state = Reconciliation::new(identity);
        state.unknown();
        state
            .begin_metadata(signal::OwnKey::Present {
                registration_id: 1234,
                submitted_at: 1,
            })
            .unwrap();
        let request = signal::get_bundle(
            &Scope::new("11", 7).unwrap(),
            0,
            "11",
            signal::DeviceSelection::All,
            signal::BundleOptions {
                reinit: false,
                notify: false,
                auto_upgrade: false,
            },
        )
        .unwrap();
        let input = json!({"error_code":0,"data":{"reqId":request.request_id(),"data":{"keys":[{"uid":"11","keys":[value["requests"][0]["data"]["data"]["key"]]}]}}});
        let bundles = signal::bundles_reply(
            &request,
            &Scope::new("11", 7).unwrap(),
            &input,
            "11",
            signal::DeviceSelection::All,
        )
        .unwrap();
        state.identity(&bundles, "11").unwrap();
        assert!(state
            .finish_metadata(signal::OwnKey::Present {
                registration_id: 1234,
                submitted_at: 2
            })
            .is_err());
        assert_eq!(state.state(), UploadState::NeedsRecovery);
        assert!(state
            .finish_metadata(signal::OwnKey::Present {
                registration_id: 1234,
                submitted_at: 1
            })
            .is_err());
        assert!(!state.needs_identity());
        assert!(!state.needs_final_metadata());
    }
}

#[cfg(test)]
mod tls_tests {
    use super::tests::{claimed, coordinator};
    use super::*;
    use crate::{
        profile::Profile,
        protocol::{
            bootstrap::ServerConfig, http::tls_tests::material, session::synthetic_session,
        },
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConnection, StreamOwned,
    };
    use serde_json::{json, Value};
    use std::{
        net::{SocketAddr, TcpListener, TcpStream},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::JoinHandle,
    };
    use tokio_tungstenite::tungstenite;
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Mode {
        Ack,
        NoTimestamp,
        LostAck,
    }
    pub(super) struct Fixture {
        address: SocketAddr,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        pub(super) requests: Arc<Mutex<Vec<u16>>>,
        profile: Arc<Profile>,
        root: std::path::PathBuf,
    }
    impl Fixture {
        pub(super) fn new(mode: Mode) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = requests.clone();
            let cert = material();
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert.leaf.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key.clone())),
            )
            .unwrap();
            let thread = std::thread::spawn(move || {
                let mut uploaded: Option<Value> = None;
                while !flag.load(Ordering::Acquire) {
                    let tcp = match listener.accept() {
                        Ok((tcp, _)) => tcp,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(_) => break,
                    };
                    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    tcp.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                    let tls = StreamOwned::new(
                        ServerConnection::new(Arc::new(config.clone())).unwrap(),
                        tcp,
                    );
                    let Ok(mut socket) = tungstenite::accept(tls) else {
                        continue;
                    };
                    let auth: Value = serde_json::from_str(include_str!(
                        "../../../../tests/fixtures/messaging/auth_frames.json"
                    ))
                    .unwrap();
                    let auth = STANDARD
                        .decode(auth["cases"][3]["frame"].as_str().unwrap())
                        .unwrap();
                    if socket.send(tungstenite::Message::Binary(auth)).is_err() {
                        continue;
                    }
                    loop {
                        let raw = match socket.read() {
                            Ok(tungstenite::Message::Binary(raw)) => raw,
                            _ => break,
                        };
                        if raw.len() < 4 {
                            break;
                        }
                        let cmd = u16::from_le_bytes([raw[1], raw[2]]);
                        captured.lock().unwrap().push(cmd);
                        let value: Value = serde_json::from_slice(&raw[4..]).unwrap();
                        let data = match cmd {
                            540 => {
                                uploaded = Some(value["data"].clone());
                                if mode == Mode::LostAck {
                                    break;
                                } else if mode == Mode::Ack {
                                    json!({"err":0,"ts":1700000000000u64})
                                } else {
                                    json!({"err":0})
                                }
                            }
                            564 => {
                                json!({"keyId":uploaded.as_ref().unwrap()["key"]["preKey"]["iKey"]["id"],"submitTs":1700000000000u64})
                            }
                            541 => {
                                let mut pc = uploaded.as_ref().unwrap()["key"].clone();
                                pc["opKeys"] = json!([]);
                                let mut mobile = pc.clone();
                                mobile["deviceId"] = json!(0);
                                json!({"keys":[{"uid":"11","keys":[mobile,pc]}]})
                            }
                            _ => break,
                        };
                        let inner =
                            json!({"error_code":0,"data":{"reqId":value["reqId"],"data":data}});
                        let outer = json!({"error_code":0,"encrypt":0,"data":serde_json::to_string(&inner).unwrap()});
                        let frame =
                            crate::protocol::messaging::encode_frame(cmd, 0, &outer).unwrap();
                        if socket.send(tungstenite::Message::Binary(frame)).is_err() {
                            break;
                        }
                    }
                }
            });
            let root = std::env::temp_dir().join(format!(
                "signal-proof-tls-{}-{}",
                std::process::id(),
                crate::domain::local_message_id().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let profile = Arc::new(Profile::at_root(&root, "fixture").unwrap());
            Self {
                address,
                stop,
                thread: Some(thread),
                requests,
                profile,
                root,
            }
        }
        pub(super) async fn connect(&self) -> Connection {
            let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"]})).unwrap();
            let session = synthetic_session(self.profile.clone(), config);
            let request = Connection::handshake_request(&session, 0).unwrap();
            let mut roots = rustls::RootCertStore::empty();
            roots
                .add(CertificateDer::from(material().root.clone()))
                .unwrap();
            let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
            let tcp = tokio::net::TcpStream::connect(self.address).await.unwrap();
            let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
                request,
                tcp,
                Some(Connection::socket_config()),
                Some(tokio_tungstenite::Connector::Rustls(Arc::new(tls))),
            )
            .await
            .unwrap();
            Connection::connected(socket)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _ = TcpStream::connect(self.address);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap()
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[tokio::test]
    async fn actual_private_connection_mints_only_authenticated_correlated_upload_or_full_identity_proofs(
    ) {
        for mode in [Mode::Ack, Mode::NoTimestamp, Mode::LostAck] {
            let fixture = Fixture::new(mode);
            let (mut coordinator, gate, mut epoch) = coordinator();
            let (mut store, receipt, _) = claimed(epoch.0).await;
            let payload_hash = *receipt.payload_hash();
            let attempt_hash = *receipt.attempt_hash();
            let request_id = receipt.request_id().to_owned();
            let operation = receipt.operation_id();
            let mut connection = fixture.connect().await;
            assert!(
                matches!(coordinator.read_next(&mut connection,epoch,&gate).await.unwrap(),ReadOutcome::Unhandled(frame)if frame.cmd==1)
            );
            let write = coordinator.upload(receipt, &gate, Instant::now()).unwrap();
            write
                .send(&mut connection, &coordinator, &gate)
                .await
                .unwrap();
            let mut early_ack = None;
            if mode == Mode::LostAck {
                assert!(coordinator
                    .read_next(&mut connection, epoch, &gate)
                    .await
                    .is_err());
                coordinator.disconnected();
                store.e2ee_recover_bundle_uploads("11").unwrap();
                assert!(store
                    .e2ee_claim_bundle_upload(
                        "11",
                        operation,
                        &crate::store::e2ee::provisioning::PublicAttemptContext {
                            generation: 7,
                            socket_epoch: epoch.0 + 1
                        },
                        &gate
                    )
                    .is_err());
                drop(connection);
                epoch = coordinator.connected(&gate).unwrap();
                connection = fixture.connect().await;
                assert!(
                    matches!(coordinator.read_next(&mut connection,epoch,&gate).await.unwrap(),ReadOutcome::Unhandled(frame)if frame.cmd==1)
                );
            } else {
                let ReadOutcome::Proof(proof) = coordinator
                    .read_next(&mut connection, epoch, &gate)
                    .await
                    .unwrap()
                else {
                    panic!("missing authenticated upload proof")
                };
                early_ack = Some(proof);
            }
            let proof = if mode == Mode::Ack {
                early_ack.unwrap()
            } else {
                if let Some(ack) = early_ack {
                    assert!(ack.acknowledged());
                    assert_eq!(ack.timestamp(), None);
                }
                coordinator
                    .reconcile(&gate, Instant::now())
                    .unwrap()
                    .send(&mut connection, &coordinator, &gate)
                    .await
                    .unwrap();
                assert!(matches!(
                    coordinator
                        .read_next(&mut connection, epoch, &gate)
                        .await
                        .unwrap(),
                    ReadOutcome::Progress
                ));
                coordinator
                    .identity_query(&gate, Instant::now())
                    .unwrap()
                    .send(&mut connection, &coordinator, &gate)
                    .await
                    .unwrap();
                assert!(matches!(
                    coordinator
                        .read_next(&mut connection, epoch, &gate)
                        .await
                        .unwrap(),
                    ReadOutcome::Progress
                ));
                coordinator
                    .final_metadata_query(&gate, Instant::now())
                    .unwrap()
                    .send(&mut connection, &coordinator, &gate)
                    .await
                    .unwrap();
                let ReadOutcome::Proof(proof) = coordinator
                    .read_next(&mut connection, epoch, &gate)
                    .await
                    .unwrap()
                else {
                    panic!("missing full identity proof")
                };
                proof
            };
            assert_eq!(proof.payload_hash(), &payload_hash);
            assert_eq!(proof.attempt_hash(), &attempt_hash);
            assert_eq!(proof.request_id(), request_id);
            assert_eq!(proof.timestamp(), Some(1700000000000));
            assert_eq!(proof.acknowledged(), mode != Mode::LostAck);
            assert_eq!(proof.identity_confirmed(), mode != Mode::Ack);
            drop(connection);
            assert_eq!(
                fixture
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|&&command| command == 540)
                    .count(),
                1
            );
            gate.cancel();
            assert!(!proof.current(&gate));
        }
    }
}

#[cfg(test)]
mod write_boundary_tests {
    use super::*;
    #[tokio::test]
    async fn stale_or_cancelled_write_is_rejected_before_any_socket_bytes() {
        let fixture = super::tls_tests::Fixture::new(super::tls_tests::Mode::NoTimestamp);
        let (mut coordinator, gate, epoch) = super::tests::coordinator();
        let (_store, receipt, _) = super::tests::claimed(epoch.0).await;
        let mut connection = fixture.connect().await;
        coordinator
            .read_next(&mut connection, epoch, &gate)
            .await
            .unwrap();
        let stale = coordinator.upload(receipt, &gate, Instant::now()).unwrap();
        coordinator.connected(&gate).unwrap();
        assert!(stale
            .send(&mut connection, &coordinator, &gate)
            .await
            .is_err());
        let cancelled = coordinator.reconcile(&gate, Instant::now()).unwrap();
        gate.cancel();
        assert!(cancelled
            .send(&mut connection, &coordinator, &gate)
            .await
            .is_err());
        drop(connection);
        assert!(fixture.requests.lock().unwrap().is_empty());
    }
}
