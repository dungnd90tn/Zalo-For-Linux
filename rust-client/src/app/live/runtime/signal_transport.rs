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
    /// Consumes the proof. Only an identity-confirmed proof yields activation evidence.
    pub(crate) fn into_activation_evidence(
        self,
    ) -> Option<crate::store::e2ee::provisioning::ActivationEvidence> {
        self.identity_confirmed
            .then(|| crate::store::e2ee::provisioning::ActivationEvidence {
                account: self.binding.account.clone(),
                operation_id: self.binding.operation_id,
                attempt_id: self.binding.attempt_id,
                generation: self.scope.generation(),
                payload_hash: self.binding.payload_hash,
                attempt_hash: self.binding.attempt_hash,
                identity_confirmed: true,
                timestamp: self.timestamp,
            })
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
    /// Re-attaches a previously sent upload whose outcome is unknown. Query-only: nothing is sent,
    /// the outbox row is `unknown` so no new claim is possible, and only reconciliation can follow.
    fn restore(
        &mut self,
        receipt: PublicUploadReceipt,
        gate: &GenerationGate,
    ) -> Result<(), SignalError> {
        self.current(gate)?;
        if self.upload.is_some() || receipt.account() != self.scope.account() {
            return Err(SignalError::Scope);
        }
        let identity = CandidateIdentity::from_public_payload(receipt.payload())?;
        if identity.payload_hash() != receipt.payload_hash() {
            return Err(SignalError::Invalid);
        }
        let mut reconciliation = Reconciliation::new(identity);
        reconciliation.unknown();
        self.upload = Some(Upload {
            receipt,
            reconciliation,
        });
        Ok(())
    }
    /// Authority to discard the staged candidate, minted only after a read showed it is not installed.
    fn abandon_evidence(&self) -> Option<crate::store::e2ee::provisioning::AbandonEvidence> {
        let upload = self.upload.as_ref()?;
        upload.reconciliation.proves_not_installed().then(|| {
            crate::store::e2ee::provisioning::AbandonEvidence {
                account: upload.receipt.account().into(),
                operation_id: upload.receipt.operation_id(),
                payload_hash: *upload.receipt.payload_hash(),
                attempt_hash: *upload.receipt.attempt_hash(),
            }
        })
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
        self.read_next_tapped(connection, epoch, gate, &mut |_| {})
            .await
    }
    /// Like `read_next`, but shows each authenticated frame to `tap` first (shape logging only).
    async fn read_next_tapped(
        &mut self,
        connection: &mut Connection,
        epoch: SocketEpoch,
        gate: &GenerationGate,
        tap: &mut (dyn FnMut(&DecodedFrame) + Send),
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
        tap(&frame);
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
                // Keep the upload even when the ACK carries a timestamp: an ACK alone never activates
                // a registration; the own 541 bundle must match first (see `e2ee_activate_pending`).
                Ok(ReadOutcome::Proof(proof(
                    upload,
                    &self.scope,
                    epoch,
                    timestamp,
                    false,
                )))
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
                    timestamp,
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

// ---------------------------------------------------------------------------------------------
// Opt-in live driver: replace this account's registered PC key (E2EE_LIVE_PLAN.md, stage 2).
//
// Off unless `ZALO_E2EE_REPLACE_PC_KEY` is an absolute path (it is also where the report goes). It
// runs once per process, right after authentication and before any offline queue is requested.
// Order: reserve → engine CreateBundle → stage durably → claim the upload (marks it `sending`) →
// command 540 → read-only 564/541/564 reconciliation → activate locally. A failure after the claim
// leaves the row `unknown`; a later run only *reads* the account to settle it and never uploads again.
// ---------------------------------------------------------------------------------------------
const DRIVER_GENERATION: u64 = 1;
// The original uploads 20 one-time prekeys; 100 made the 540 frame exceed the server's limit (close 1009).
const DRIVER_PREKEYS: u8 = 20;
const DRIVER_INTENT: &str = "replace-pc-key";
const DRIVER_STEP_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn replace_target() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_REPLACE_PC_KEY")?);
    path.is_absolute().then_some(path)
}
pub(super) fn claim_run() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}

pub(super) struct Report(std::fs::File);
impl Report {
    pub(super) fn open(path: &std::path::Path) -> Option<Self> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(path).ok()?;
        let _ = writeln!(
            file,
            "# zalo-native PC key replacement. Stages and shapes only; no keys or ids."
        );
        Some(Self(file))
    }
    pub(super) fn line(&mut self, text: &str) {
        use std::io::Write;
        let _ = writeln!(self.0, "{text}");
        let _ = self.0.flush();
    }
}

/// Replaces the account's PC key. Never returns an error to the caller: the outcome is in the report.
pub(super) async fn replace_pc_key(
    connection: &mut Connection,
    db: &super::Database,
    account: &str,
    path: &std::path::Path,
) {
    let Some(mut report) = Report::open(path) else {
        return;
    };
    match replace_inner(connection, db, account, &mut report).await {
        Ok(message) => report.line(&format!("RESULT: {message}")),
        Err(error) => {
            // A claimed upload whose fate is unknown must never be re-sent; mark it so a later run
            // only reads the account.
            let owner = account.to_owned();
            let _ = db
                .call(move |s| s.e2ee_recover_bundle_uploads(&owner))
                .await;
            report.line(&format!("RESULT: stopped: {error}"));
            report
                .line("If an upload was sent, run again: it only reads the account to settle it.");
        }
    }
}

async fn pump(
    coordinator: &mut Coordinator,
    connection: &mut Connection,
    epoch: SocketEpoch,
    gate: &GenerationGate,
    report: &mut Report,
) -> anyhow::Result<Option<CorrelatedReplyProof>> {
    let deadline = Instant::now() + DRIVER_STEP_TIMEOUT;
    loop {
        let outcome = tokio::time::timeout_at(
            deadline,
            coordinator.read_next_tapped(connection, epoch, gate, &mut |frame| {
                if frame.payload.pointer("/data/reqId").is_some() {
                    report.line(&format!(
                        "## reply command {} sub {}",
                        frame.cmd, frame.sub_cmd
                    ));
                    for line in super::e2ee_probe::shape(&frame.payload) {
                        report.line(&line);
                    }
                }
            }),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for the server"))??;
        match outcome {
            ReadOutcome::Progress => return Ok(None),
            ReadOutcome::Proof(proof) => return Ok(Some(proof)),
            ReadOutcome::Unhandled(_) | ReadOutcome::NoData => {}
        }
    }
}
async fn send_and_wait(
    write: Write,
    coordinator: &mut Coordinator,
    connection: &mut Connection,
    epoch: SocketEpoch,
    gate: &GenerationGate,
    report: &mut Report,
) -> anyhow::Result<Option<CorrelatedReplyProof>> {
    write.send(connection, coordinator, gate).await?;
    pump(coordinator, connection, epoch, gate, report).await
}
/// Read-only: 564, then own 541, then 564 again. Yields the identity-confirmed proof.
async fn reconcile_all(
    coordinator: &mut Coordinator,
    connection: &mut Connection,
    epoch: SocketEpoch,
    gate: &GenerationGate,
    report: &mut Report,
) -> anyhow::Result<CorrelatedReplyProof> {
    let write = coordinator.reconcile(gate, Instant::now())?;
    send_and_wait(write, coordinator, connection, epoch, gate, report).await?;
    let write = coordinator.identity_query(gate, Instant::now())?;
    send_and_wait(write, coordinator, connection, epoch, gate, report).await?;
    let write = coordinator.final_metadata_query(gate, Instant::now())?;
    send_and_wait(write, coordinator, connection, epoch, gate, report)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the server never confirmed the new identity"))
}

enum Outcome {
    Done(&'static str),
    /// An earlier attempt was proven not installed and discarded; start again with a fresh key.
    Retry,
}
async fn replace_inner(
    connection: &mut Connection,
    db: &super::Database,
    account: &str,
    report: &mut Report,
) -> anyhow::Result<&'static str> {
    // At most one automatic fresh start per run, and only after a read proved the old one was not installed.
    for _ in 0..2 {
        match replace_once(connection, db, account, report).await? {
            Outcome::Done(message) => return Ok(message),
            Outcome::Retry => report.line("starting again with a fresh key"),
        }
    }
    anyhow::bail!(
        "the server did not install the new key after a fresh attempt; not retrying again"
    )
}
/// Read-only: command 564, the PC key id the service holds for this account.
async fn query_own_key(
    connection: &mut Connection,
    account: &str,
    report: &mut Report,
) -> anyhow::Result<signal::OwnKey> {
    let scope = Scope::new(account, DRIVER_GENERATION)?;
    let request = signal::own_key(&scope, 9_200_001, signal::DevicePlatform::Pc)?;
    connection.send(564, 0, request.wire_data(&scope)?).await?;
    let deadline = Instant::now() + DRIVER_STEP_TIMEOUT;
    loop {
        let frame = tokio::time::timeout_at(deadline, connection.next())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for the key id"))??;
        let Some(frame) = frame.filter(|f| {
            f.cmd == 564
                && f.payload
                    .pointer("/data/reqId")
                    .and_then(serde_json::Value::as_str)
                    == Some(request.request_id())
        }) else {
            continue;
        };
        report.line("## own key id reply");
        for line in super::e2ee_probe::shape(&frame.payload) {
            report.line(&line);
        }
        return Ok(signal::own_key_reply(&request, &scope, &frame.payload)?);
    }
}
async fn replace_once(
    connection: &mut Connection,
    db: &super::Database,
    account: &str,
    report: &mut Report,
) -> anyhow::Result<Outcome> {
    use crate::{
        app::live::signal_crypto::{provisioning::PendingProvisioner, Cancellation},
        store::e2ee::provisioning::{CandidateRequest, PendingState, PublicAttemptContext},
    };
    use anyhow::Context;
    let gate = std::sync::Arc::new(GenerationGate::new(DRIVER_GENERATION)?);
    let mut coordinator = Coordinator::new(Scope::new(account, DRIVER_GENERATION)?);
    let epoch = coordinator.connected(&gate)?;
    let owner = account.to_owned();
    let (current, pending) = db
        .call(move |s| {
            Ok((
                s.e2ee_current_local(&owner)?,
                s.e2ee_pending_candidate(&owner)?
                    .map(|c| (c.state(), c.operation_id())),
            ))
        })
        .await?;
    if pending.is_none() && current.is_some() {
        // Is the PC key on the service still this client's? The original client (or another install) may
        // have replaced it since; then the local key is stale and must be rotated, not trusted.
        let local_registration = {
            let owner = account.to_owned();
            let epoch = current.unwrap_or_default();
            db.call(move |s| s.e2ee_local_registration(&owner, epoch))
                .await?
        };
        let server = query_own_key(connection, account, report).await?;
        match (server, local_registration) {
            (signal::OwnKey::Present { registration_id, .. }, Some(local))
                if registration_id == local =>
            {
                return Ok(Outcome::Done(
                    "this client already holds the account's PC key; nothing to do",
                ));
            }
            _ => report.line(
                "the PC key on the service is not this client's (another client replaced it); replacing it again",
            ),
        }
    }
    report.line(&format!(
        "start: local epoch present={}, staged candidate={}",
        current.is_some(),
        pending.is_some()
    ));
    // A previous run that died mid-upload: settle by reading only.
    let restored_run = matches!(
        pending,
        Some((PendingState::Sending | PendingState::Unknown, _))
    );
    if restored_run {
        let owner = account.to_owned();
        let restored = db
            .call(move |s| {
                s.e2ee_recover_bundle_uploads(&owner)?;
                s.e2ee_restore_unknown_upload(&owner)
            })
            .await?
            .context("the earlier upload cannot be restored")?;
        coordinator.restore(restored, &gate)?;
        report.line(
            "an earlier upload attempt has an unknown outcome; checking the account (read-only)",
        );
    } else {
        let operation = match pending {
            Some((PendingState::Ready, operation)) => operation,
            _ => {
                let owner = account.to_owned();
                let g = gate.clone();
                let plan = db
                    .call(move |s| {
                        s.e2ee_reserve_local_candidate(
                            &owner,
                            &CandidateRequest {
                                platform: crate::store::e2ee::Platform::Pc,
                                expected_current: current,
                                stable_intent: DRIVER_INTENT.into(),
                                prekey_count: DRIVER_PREKEYS,
                                generation: DRIVER_GENERATION,
                            },
                            &g,
                        )
                    })
                    .await?;
                let operation = plan.operation_id();
                report.line("reserved a new local identity; generating keys");
                let mut provisioner = PendingProvisioner::new(
                    zalo_signal_engine::AccountId::new(account)
                        .map_err(|_| anyhow::anyhow!("invalid account id"))?,
                    DRIVER_GENERATION,
                )?;
                let candidate = provisioner
                    .prepare(plan, &gate, &Cancellation::default())
                    .await?;
                let g = gate.clone();
                db.call(move |s| s.e2ee_stage_verified_bundle(&candidate, &g))
                    .await?;
                report.line("keys generated and staged durably");
                operation
            }
        };
        let owner = account.to_owned();
        let g = gate.clone();
        let socket = epoch.value();
        let receipt = db
            .call(move |s| {
                s.e2ee_claim_bundle_upload(
                    &owner,
                    operation,
                    &PublicAttemptContext {
                        generation: DRIVER_GENERATION,
                        socket_epoch: socket,
                    },
                    &g,
                )
            })
            .await?;
        report.line("upload claimed; sending command 540 (this replaces the account's PC key)");
        let write = coordinator.upload(receipt, &gate, Instant::now())?;
        let acked = send_and_wait(write, &mut coordinator, connection, epoch, &gate, report)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no reply to command 540"))?;
        report.line(&format!(
            "540 acknowledged by the server (acknowledged={}, iid equals registration id: {:?})",
            acked.acknowledged(),
            coordinator
                .upload
                .as_ref()
                .and_then(|u| u.reconciliation.ack_iid_matches())
        ));
    }
    let proof = match reconcile_all(&mut coordinator, connection, epoch, &gate, report).await {
        Ok(proof) => proof,
        Err(error) => {
            // Only a restored attempt may be discarded, and only on proof from a read of the account.
            if let (true, Some(evidence)) = (restored_run, coordinator.abandon_evidence()) {
                report.line("the account does not hold the staged key (the upload never took effect); discarding it");
                let owner = account.to_owned();
                db.call(move |s| s.e2ee_abandon_unknown_candidate(&owner, &evidence))
                    .await?;
                return Ok(Outcome::Retry);
            }
            return Err(error);
        }
    };
    report.line("the server's own 541 bundle matches the new identity");
    let evidence = proof
        .into_activation_evidence()
        .context("identity was not confirmed")?;
    let owner = account.to_owned();
    let g = gate.clone();
    let local = db
        .call(move |s| s.e2ee_activate_pending(&owner, &evidence, &g))
        .await?;
    report.line(&format!("activated local epoch {}", local.epoch));
    Ok(Outcome::Done(
        "the account's PC key was replaced and is now held by this client",
    ))
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
    /// The live service answers 564 with `{err:0,keyId}` only. Identity is still proven by the own 541
    /// bundle; the proof simply carries no timestamp.
    #[tokio::test]
    async fn reconciliation_succeeds_without_a_server_timestamp_as_the_real_service_omits_it() {
        let (mut c, gate, epoch) = coordinator();
        let (_store, receipt, payload) = claimed(epoch.0).await;
        c.upload(receipt, &gate, Instant::now()).unwrap();
        c.disconnected();
        let new = c.connected(&gate).unwrap();
        let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
        let request = c.reconcile(&gate, Instant::now()).unwrap();
        c.accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
            .unwrap();
        let request = c.identity_query(&gate, Instant::now()).unwrap();
        let mut pc = payload["key"].clone();
        pc["opKeys"] = json!([]);
        c.accept(
            reply(
                &request,
                json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[pc]}]}),
            ),
            new,
            &gate,
        )
        .unwrap();
        let request = c.final_metadata_query(&gate, Instant::now()).unwrap();
        let ReadOutcome::Proof(proof) = c
            .accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
            .unwrap()
        else {
            panic!("expected reconciled proof")
        };
        assert!(proof.identity_confirmed() && proof.timestamp().is_none());
        // A different registration id on the second read still cannot confirm.
        let (mut c, gate, epoch) = coordinator();
        let (_store, receipt, payload) = claimed(epoch.0).await;
        c.upload(receipt, &gate, Instant::now()).unwrap();
        c.disconnected();
        let new = c.connected(&gate).unwrap();
        let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
        let request = c.reconcile(&gate, Instant::now()).unwrap();
        c.accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
            .unwrap();
        let request = c.identity_query(&gate, Instant::now()).unwrap();
        let mut pc = payload["key"].clone();
        pc["opKeys"] = json!([]);
        c.accept(
            reply(
                &request,
                json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[pc]}]}),
            ),
            new,
            &gate,
        )
        .unwrap();
        let request = c.final_metadata_query(&gate, Instant::now()).unwrap();
        assert!(c
            .accept(
                reply(&request, json!({"err":0,"keyId":reg.as_u64().unwrap() + 1})),
                new,
                &gate
            )
            .is_err());
        assert_eq!(c.state(), Some(UploadState::NeedsRecovery));
    }
    /// Drives upload → ACK → reconcile with real-service shapes (no timestamps) and returns the
    /// ACK-only proof and the identity-confirmed proof.
    async fn confirmed(
        c: &mut Coordinator,
        gate: &GenerationGate,
        epoch: SocketEpoch,
        receipt: PublicUploadReceipt,
        payload: &Value,
    ) -> (CorrelatedReplyProof, CorrelatedReplyProof) {
        let write = c.upload(receipt, gate, Instant::now()).unwrap();
        let ReadOutcome::Proof(acked) = c
            .accept(reply(&write, json!({"err": 0})), epoch, gate)
            .unwrap()
        else {
            panic!("expected ACK proof")
        };
        let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
        let request = c.reconcile(gate, Instant::now()).unwrap();
        c.accept(reply(&request, json!({"err":0,"keyId":reg})), epoch, gate)
            .unwrap();
        let request = c.identity_query(gate, Instant::now()).unwrap();
        let mut pc = payload["key"].clone();
        pc["opKeys"] = json!([]);
        c.accept(
            reply(
                &request,
                json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[pc]}]}),
            ),
            epoch,
            gate,
        )
        .unwrap();
        let request = c.final_metadata_query(gate, Instant::now()).unwrap();
        let ReadOutcome::Proof(proof) = c
            .accept(reply(&request, json!({"err":0,"keyId":reg})), epoch, gate)
            .unwrap()
        else {
            panic!("expected reconciled proof")
        };
        (acked, proof)
    }
    #[tokio::test]
    async fn only_an_identity_confirmed_proof_activates_and_activation_is_atomic_and_one_shot() {
        let (mut c, gate, epoch) = coordinator();
        let (mut store, receipt, payload) = claimed(epoch.0).await;
        let operation = receipt.operation_id();
        let local = receipt.local().clone();
        let (acked, proof) = confirmed(&mut c, &gate, epoch, receipt, &payload).await;
        // An ACK alone (even a successful one) is not activation authority.
        assert!(acked.acknowledged() && acked.into_activation_evidence().is_none());
        assert_eq!(store.e2ee_current_local("11").unwrap(), None);
        let evidence = proof.into_activation_evidence().unwrap();

        // Tampered evidence is rejected and changes nothing.
        let mut forged = crate::store::e2ee::provisioning::ActivationEvidence {
            ..clone_evidence(&evidence)
        };
        forged.attempt_hash[0] ^= 1;
        assert!(store.e2ee_activate_pending("11", &forged, &gate).is_err());
        let mut forged = clone_evidence(&evidence);
        forged.payload_hash[0] ^= 1;
        assert!(store.e2ee_activate_pending("11", &forged, &gate).is_err());
        assert_eq!(store.e2ee_current_local("11").unwrap(), None);
        assert!(store.e2ee_pending_candidate("11").unwrap().is_some());

        let activated = store.e2ee_activate_pending("11", &evidence, &gate).unwrap();
        assert_eq!(activated.epoch, local.epoch);
        assert_eq!(
            store.e2ee_current_local("11").unwrap(),
            Some(local.epoch.parse().unwrap())
        );
        let identity =
            CandidateIdentity::from_public_payload(&serde_json::to_vec(&payload).unwrap()).unwrap();
        store
            .e2ee_validate_local_identity(
                "11",
                &activated,
                identity.registration_id(),
                identity.identity_key(),
            )
            .unwrap();
        assert!(store.e2ee_pending_candidate("11").unwrap().is_none());
        assert!(store.e2ee_restore_unknown_upload("11").unwrap().is_none());
        // One shot: the staged registration is gone, so replay cannot activate again or reclaim.
        assert!(store.e2ee_activate_pending("11", &evidence, &gate).is_err());
        assert!(store
            .e2ee_claim_bundle_upload(
                "11",
                operation,
                &PublicAttemptContext {
                    generation: 7,
                    socket_epoch: epoch.0
                },
                &gate
            )
            .is_err());
        // A later rotation may reserve a new candidate against the new current epoch.
        assert!(store
            .e2ee_reserve_local_candidate(
                "11",
                &CandidateRequest {
                    platform: crate::store::e2ee::Platform::Pc,
                    expected_current: store.e2ee_current_local("11").unwrap(),
                    stable_intent: "next-rotation".into(),
                    prekey_count: 1,
                    generation: 7,
                },
                &gate,
            )
            .is_ok());
    }
    fn clone_evidence(
        e: &crate::store::e2ee::provisioning::ActivationEvidence,
    ) -> crate::store::e2ee::provisioning::ActivationEvidence {
        crate::store::e2ee::provisioning::ActivationEvidence {
            account: e.account.clone(),
            operation_id: e.operation_id,
            attempt_id: e.attempt_id,
            generation: e.generation,
            payload_hash: e.payload_hash,
            attempt_hash: e.attempt_hash,
            identity_confirmed: e.identity_confirmed,
            timestamp: e.timestamp,
        }
    }
    /// Own 541 outcomes after a correct `keyId`: the live service omits the PC bundle (accepted);
    /// a listed PC bundle must be ours; two PC bundles are never accepted.
    #[tokio::test]
    async fn own_541_may_omit_the_asking_device_but_a_listed_pc_bundle_must_match() {
        for case in ["absent", "different", "duplicate"] {
            let (mut c, gate, epoch) = coordinator();
            let (_store, receipt, payload) = claimed(epoch.0).await;
            c.upload(receipt, &gate, Instant::now()).unwrap();
            c.disconnected();
            let new = c.connected(&gate).unwrap();
            let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
            let request = c.reconcile(&gate, Instant::now()).unwrap();
            c.accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
                .unwrap();
            let request = c.identity_query(&gate, Instant::now()).unwrap();
            let mut ours = payload["key"].clone();
            ours["opKeys"] = json!([]);
            let mut web = ours.clone();
            web["deviceId"] = json!(1);
            let mut other = ours.clone();
            other["preKey"]["iKey"]["key"] = json!(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                [5u8; 33]
            ));
            let keys = match case {
                "absent" => json!([web]),
                "different" => json!([web, other]),
                _ => json!([ours.clone(), ours]),
            };
            let outcome = c.accept(
                reply(
                    &request,
                    json!({"err":0,"keys":[{"uid":"11","err":0,"keys":keys}]}),
                ),
                new,
                &gate,
            );
            if case == "absent" {
                outcome.unwrap();
                let request = c.final_metadata_query(&gate, Instant::now()).unwrap();
                assert!(matches!(
                    c.accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate),
                    Ok(ReadOutcome::Proof(p)) if p.identity_confirmed()
                ));
            } else {
                assert!(outcome.is_err());
                // A different listed identity proves "not installed"; a duplicate (rejected while
                // parsing the reply) proves nothing and never confirms.
                assert_eq!(c.abandon_evidence().is_some(), case == "different");
                assert_ne!(c.state(), Some(UploadState::IdentityConfirmed));
                if case == "different" {
                    assert_eq!(c.state(), Some(UploadState::NeedsRecovery));
                }
            }
        }
    }
    #[tokio::test]
    async fn abandoning_requires_an_unknown_upload_and_matching_evidence_and_keeps_other_state() {
        use crate::store::e2ee::provisioning::AbandonEvidence;
        let (_c, gate, epoch) = coordinator();
        let (mut store, receipt, _) = claimed(epoch.0).await;
        let evidence = |hash_flip: bool| AbandonEvidence {
            account: "11".into(),
            operation_id: receipt.operation_id(),
            payload_hash: *receipt.payload_hash(),
            attempt_hash: {
                let mut h = *receipt.attempt_hash();
                h[0] ^= u8::from(hash_flip);
                h
            },
        };
        // Still `sending`: the request may be in flight, so it cannot be discarded.
        assert!(store
            .e2ee_abandon_unknown_candidate("11", &evidence(false))
            .is_err());
        store.e2ee_recover_bundle_uploads("11").unwrap();
        assert!(store
            .e2ee_abandon_unknown_candidate("11", &evidence(true))
            .is_err());
        let mut other = evidence(false);
        other.account = "12".into();
        assert!(store.e2ee_abandon_unknown_candidate("11", &other).is_err());
        assert!(store.e2ee_pending_candidate("11").unwrap().is_some());
        store
            .e2ee_abandon_unknown_candidate("11", &evidence(false))
            .unwrap();
        assert!(store.e2ee_pending_candidate("11").unwrap().is_none());
        assert_eq!(store.e2ee_current_local("11").unwrap(), None);
        // The same intent can now be reserved again, and replaying the abandonment fails.
        assert!(store
            .e2ee_abandon_unknown_candidate("11", &evidence(false))
            .is_err());
        assert!(store
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
            .is_ok());
    }
    #[tokio::test]
    async fn restart_restores_an_unknown_upload_query_only_and_it_can_still_activate() {
        let (c, gate, epoch) = coordinator();
        drop(c);
        let (mut store, receipt, payload) = claimed(epoch.0).await;
        // Simulate the process dying after the request left: the row becomes `unknown`.
        assert_eq!(store.e2ee_recover_bundle_uploads("11").unwrap(), 1);
        let restored = store.e2ee_restore_unknown_upload("11").unwrap().unwrap();
        assert_eq!(restored.attempt_hash(), receipt.attempt_hash());
        // No second upload can be claimed for it.
        assert!(store
            .e2ee_claim_bundle_upload(
                "11",
                restored.operation_id(),
                &PublicAttemptContext {
                    generation: 7,
                    socket_epoch: epoch.0
                },
                &gate
            )
            .is_err());
        let (mut c, gate, new) = coordinator();
        c.restore(restored, &gate).unwrap();
        assert_eq!(c.state(), Some(UploadState::Unknown));
        let reg = payload["key"]["preKey"]["iKey"]["id"].clone();
        let request = c.reconcile(&gate, Instant::now()).unwrap();
        c.accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
            .unwrap();
        let request = c.identity_query(&gate, Instant::now()).unwrap();
        let mut pc = payload["key"].clone();
        pc["opKeys"] = json!([]);
        c.accept(
            reply(
                &request,
                json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[pc]}]}),
            ),
            new,
            &gate,
        )
        .unwrap();
        let request = c.final_metadata_query(&gate, Instant::now()).unwrap();
        let ReadOutcome::Proof(proof) = c
            .accept(reply(&request, json!({"err":0,"keyId":reg})), new, &gate)
            .unwrap()
        else {
            panic!("expected reconciled proof")
        };
        let evidence = proof.into_activation_evidence().unwrap();
        assert!(store.e2ee_activate_pending("11", &evidence, &gate).is_ok());
    }
    mod driver {
        use super::*;
        use crate::app::live::runtime::{wire, Database};
        use futures_util::{SinkExt, StreamExt};
        use std::sync::{Arc, Mutex};

        /// A fake account service on plain websocket. It authenticates, then answers the commands the
        /// driver sends, like the real service does for a PC registration. Returns the commands seen.
        #[derive(Clone, Copy, PartialEq)]
        enum Mode {
            Normal,
            /// The key is installed, then the connection dies before the reply.
            DropAfterInstall,
            /// The request is refused (like an oversize frame) and the connection closes.
            RejectAndDrop,
        }
        async fn server(
            key: Arc<Mutex<Option<Value>>>,
            mode: Mode,
        ) -> (std::net::SocketAddr, tokio::task::JoinHandle<Vec<u16>>) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                use base64::{engine::general_purpose::STANDARD, Engine};
                use std::io::Write;
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let mut z =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}}"#)
                    .unwrap();
                let auth = wire::encode_frame(
                    1,
                    0,
                    &json!({"error_code":0,"key":STANDARD.encode([7u8; 32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())}),
                )
                .unwrap();
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Binary(auth))
                    .await
                    .unwrap();
                let mut seen = Vec::new();
                while let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) =
                    socket.next().await
                {
                    let cmd = u16::from(bytes[1]) | (u16::from(bytes[2]) << 8);
                    let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                    seen.push(cmd);
                    let reply = match cmd {
                        540 => {
                            if mode == Mode::RejectAndDrop {
                                return seen;
                            }
                            *key.lock().unwrap() = Some(body["data"]["key"].clone());
                            if mode == Mode::DropAfterInstall {
                                return seen;
                            }
                            let id = body["data"]["key"]["preKey"]["iKey"]["id"].clone();
                            json!({"err":0,"iid":id,"keyExists":0,"ts":1700000000000u64,"devices":[]})
                        }
                        564 => {
                            // Before any install the account still holds an old PC key (id 4242).
                            let id = key
                                .lock()
                                .unwrap()
                                .as_ref()
                                .map(|k| k["preKey"]["iKey"]["id"].clone())
                                .unwrap_or(json!(4242));
                            json!({"err": 0, "keyId": id})
                        }
                        // Like the live service: the asking device (PC) is left out of its own list.
                        541 => match key.lock().unwrap().clone() {
                            Some(mut web) => {
                                web["opKeys"] = json!([]);
                                web["deviceId"] = json!(1);
                                json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[web]}]})
                            }
                            None => json!({"err":0,"keys":[{"uid":"11","err":0,"keys":[]}]}),
                        },
                        _ => continue,
                    };
                    let frame = wire::encode_frame(
                        cmd,
                        0,
                        &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":reply}}}),
                    )
                    .unwrap();
                    if socket
                        .send(tokio_tungstenite::tungstenite::Message::Binary(frame))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                seen
            });
            (address, task)
        }
        async fn connect(address: std::net::SocketAddr) -> Connection {
            let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
                .await
                .unwrap();
            let mut connection = Connection::connected(socket, "11");
            assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
            connection
        }

        #[tokio::test]
        async fn replaces_the_pc_key_once_and_a_crash_mid_upload_is_only_ever_reconciled_by_reading(
        ) {
            let scratch =
                std::env::temp_dir().join(format!("zalo-driver-{:x}", rand::random::<u64>()));
            std::fs::create_dir(&scratch).unwrap();
            let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
            let db = Database::open(profile, "11".into()).await.unwrap();
            let report = scratch.join("report.txt");
            let key = Arc::new(Mutex::new(None));

            // Run 1: the process "dies" after command 540 left. Nothing may be activated.
            let (address, task) = server(key.clone(), Mode::DropAfterInstall).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            assert_eq!(task.await.unwrap(), [540]);
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(
                text.contains("upload claimed") && text.contains("RESULT: stopped"),
                "{text}"
            );
            assert_eq!(db.call(|s| s.e2ee_current_local("11")).await.unwrap(), None);
            assert!(db
                .call(|s| s.e2ee_restore_unknown_upload("11").map(|r| r.is_some()))
                .await
                .unwrap());

            // Run 2: a new process settles it by reading only (564, 541, 564), with no second 540.
            let (address, task) = server(key.clone(), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            assert_eq!(task.await.unwrap(), [564, 541, 564]);
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(
                text.contains("activated local epoch")
                    && text.contains("RESULT: the account's PC key was replaced"),
                "{text}"
            );
            assert!(db
                .call(|s| s.e2ee_current_local("11"))
                .await
                .unwrap()
                .is_some());

            // Run 3: already held, so it does nothing at all on the wire.
            let (address, task) = server(key.clone(), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            // Only a read of the key id (564) happens: it still matches, so nothing is sent or changed.
            assert_eq!(task.await.unwrap(), [564]);
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(text.contains("nothing to do"), "{text}");
            // The report never carries key material: no long base64-looking token on any line.
            for word in text.split_whitespace() {
                assert!(
                    word.len() < 32
                        || !word
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
                    "key-like token in report: {word}"
                );
            }
            std::fs::remove_dir_all(scratch).unwrap();
        }

        #[tokio::test]
        async fn an_upload_the_server_never_took_is_discarded_on_proof_and_retried_once_with_a_fresh_key(
        ) {
            let scratch =
                std::env::temp_dir().join(format!("zalo-driver-{:x}", rand::random::<u64>()));
            std::fs::create_dir(&scratch).unwrap();
            let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
            let db = Database::open(profile, "11".into()).await.unwrap();
            let report = scratch.join("report.txt");
            let key = Arc::new(Mutex::new(None));

            // Run 1: the server refuses the frame and closes (what close code 1009 looked like).
            let (address, task) = server(key.clone(), Mode::RejectAndDrop).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            assert_eq!(task.await.unwrap(), [540]);
            assert!(db
                .call(|s| s.e2ee_restore_unknown_upload("11").map(|r| r.is_some()))
                .await
                .unwrap());
            assert!(key.lock().unwrap().is_none());

            // Run 2: a read shows the account holds a different key, so the stale candidate is discarded
            // and exactly one new 540 follows.
            let (address, task) = server(key.clone(), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            let seen = task.await.unwrap();
            assert_eq!(seen, [564, 540, 564, 541, 564], "{seen:?}");
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(
                text.contains("discarding it") && text.contains("starting again with a fresh key"),
                "{text}"
            );
            assert!(text.contains("activated local epoch"), "{text}");
            assert!(db
                .call(|s| s.e2ee_current_local("11"))
                .await
                .unwrap()
                .is_some());
            std::fs::remove_dir_all(scratch).unwrap();
        }

        /// The original client (or another install) replaced the PC key after this client activated its own:
        /// the local epoch is stale, so the driver rotates to a new epoch instead of trusting it.
        #[tokio::test]
        async fn a_stale_local_key_is_replaced_again_with_a_new_epoch() {
            let scratch =
                std::env::temp_dir().join(format!("zalo-driver-{:x}", rand::random::<u64>()));
            std::fs::create_dir(&scratch).unwrap();
            let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
            let db = Database::open(profile, "11".into()).await.unwrap();
            let report = scratch.join("report.txt");
            let (address, task) = server(Arc::new(Mutex::new(None)), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            task.await.unwrap();
            let first = db
                .call(|s| s.e2ee_current_local("11"))
                .await
                .unwrap()
                .unwrap();
            // A fresh service state: it holds some other key (id 4242), not the one this client activated.
            let (address, task) = server(Arc::new(Mutex::new(None)), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            let seen = task.await.unwrap();
            assert_eq!(seen, [564, 540, 564, 541, 564], "{seen:?}");
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(text.contains("another client replaced it"), "{text}");
            let second = db
                .call(|s| s.e2ee_current_local("11"))
                .await
                .unwrap()
                .unwrap();
            assert!(second > first, "{first} -> {second}");
            std::fs::remove_dir_all(scratch).unwrap();
        }

        #[tokio::test]
        async fn a_normal_run_sends_540_once_then_reads_and_activates() {
            let scratch =
                std::env::temp_dir().join(format!("zalo-driver-{:x}", rand::random::<u64>()));
            std::fs::create_dir(&scratch).unwrap();
            let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
            let db = Database::open(profile, "11".into()).await.unwrap();
            let report = scratch.join("report.txt");
            let (address, task) = server(Arc::new(Mutex::new(None)), Mode::Normal).await;
            let mut connection = connect(address).await;
            replace_pc_key(&mut connection, &db, "11", &report).await;
            drop(connection);
            assert_eq!(task.await.unwrap(), [540, 564, 541, 564]);
            let text = std::fs::read_to_string(&report).unwrap();
            assert!(
                text.contains("## reply command 540") && text.contains("data.data.err: number = 0"),
                "{text}"
            );
            assert!(text.contains("activated local epoch"), "{text}");
            assert!(db
                .call(|s| s.e2ee_current_local("11"))
                .await
                .unwrap()
                .is_some());
            std::fs::remove_dir_all(scratch).unwrap();
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
                submitted_at: Some(1),
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
                submitted_at: Some(2)
            })
            .is_err());
        assert_eq!(state.state(), UploadState::NeedsRecovery);
        assert!(state
            .finish_metadata(signal::OwnKey::Present {
                registration_id: 1234,
                submitted_at: Some(1)
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
            Connection::connected(socket, session.account_id())
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
