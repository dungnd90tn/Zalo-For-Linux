//! Pending local registration. No network, activation or fabricated binding.
//! Only the existing MessageStore owner invokes these synchronous transactions.
use super::*;

pub const MAX_PUBLIC_BUNDLE_BYTES: usize = 64 * 1024;
pub struct CandidateRequest {
    pub platform: Platform,
    pub expected_current: Option<u64>,
    pub stable_intent: String,
    pub prekey_count: u8,
    pub generation: u64,
}
/// Actor-owned reservation; no keys, identity binding or current-epoch change.
pub struct ProvisionPlan {
    account: String,
    local: LocalNamespace,
    expected_current: Option<u64>,
    generation: u64,
    operation_id: u64,
    revision: u64,
    prekey_count: u8,
    digest: [u8; 32],
}
impl ProvisionPlan {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn local(&self) -> &LocalNamespace {
        &self.local
    }
    pub fn expected_current(&self) -> Option<u64> {
        self.expected_current
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn expected_revision(&self) -> u64 {
        self.revision
    }
    pub fn prekey_count(&self) -> u8 {
        self.prekey_count
    }
    pub fn plan_digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            account: self.account.clone(),
            revision: self.revision,
            records: vec![],
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    Reserved,
    Ready,
    Sending,
    Unknown,
}
impl PendingState {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "reserved" => Self::Reserved,
            "ready" => Self::Ready,
            "sending" => Self::Sending,
            "unknown" => Self::Unknown,
            _ => bail!("Invalid provisioning state"),
        })
    }
}
/// Restart discovery exposes only reservation metadata, never pending secrets.
pub struct PendingCandidateStatus {
    account: String,
    local: LocalNamespace,
    operation_id: u64,
    state: PendingState,
    expected_current: Option<u64>,
    prekey_count: u8,
    intent: String,
}
impl PendingCandidateStatus {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn local(&self) -> &LocalNamespace {
        &self.local
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn state(&self) -> PendingState {
        self.state
    }
    pub fn expected_current(&self) -> Option<u64> {
        self.expected_current
    }
    pub fn prekey_count(&self) -> u8 {
        self.prekey_count
    }
    pub fn stable_intent(&self) -> &str {
        &self.intent
    }
}
/// Private fields prove the keys/public payload transaction committed. This
/// receipt is not authority to send; claiming an upload is a separate commit.
pub struct PendingBundleReceipt {
    account: String,
    local: LocalNamespace,
    operation_id: u64,
    revision: u64,
    replay: bool,
}
impl PendingBundleReceipt {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn local(&self) -> &LocalNamespace {
        &self.local
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_replay(&self) -> bool {
        self.replay
    }
}
/// One-use READY→SENDING claim, bound to the exact staged public bundle.
/// The future wire adapter must consume this receipt and check the auth gate.
pub struct PublicAttemptContext {
    pub generation: u64,
    pub socket_epoch: u64,
}
pub struct PublicUploadReceipt {
    account: String,
    local: LocalNamespace,
    operation_id: u64,
    generation: u64,
    attempt_id: u64,
    socket_epoch: u64,
    request_id: String,
    payload: Vec<u8>,
    payload_hash: [u8; 32],
    attempt_hash: [u8; 32],
}
/// What a confirmed registration must prove before the store activates it. Built only from a
/// reconciliation proof in the live transport (`pub(crate)` fields), never from caller booleans.
pub struct ActivationEvidence {
    pub(crate) account: String,
    pub(crate) operation_id: u64,
    pub(crate) attempt_id: u64,
    pub(crate) generation: u64,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) attempt_hash: [u8; 32],
    pub(crate) identity_confirmed: bool,
    /// A real server timestamp when one was returned; otherwise the activation time is recorded.
    pub(crate) timestamp: Option<u64>,
}
/// Proof (from a read-only look at the account) that an unresolved upload was not installed.
pub struct AbandonEvidence {
    pub(crate) account: String,
    pub(crate) operation_id: u64,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) attempt_hash: [u8; 32],
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
impl PublicUploadReceipt {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn local(&self) -> &LocalNamespace {
        &self.local
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn attempt_id(&self) -> u64 {
        self.attempt_id
    }
    pub fn socket_epoch(&self) -> u64 {
        self.socket_epoch
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn attempt_hash(&self) -> &[u8; 32] {
        &self.attempt_hash
    }
    pub fn can_submit(&self, gate: &GenerationGate) -> bool {
        gate.is_current(self.generation)
    }
}
fn current(conn: &Connection, owner: &str) -> Result<Option<u64>> {
    Ok(conn
        .query_row(
            "SELECT current_local FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get::<_, Option<i64>>(0),
        )?
        .map(|value| value as u64))
}
fn plan_digest(owner: &str, request: &CandidateRequest) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    digest_part(&mut hash, b"pending-local-v1");
    digest_part(&mut hash, owner.as_bytes());
    digest_part(&mut hash, request.stable_intent.as_bytes());
    hash.update([request.platform.wire(), request.prekey_count]);
    hash.update(request.expected_current.unwrap_or(0).to_be_bytes());
    Ok(hash.finalize().into())
}
fn private_digest(
    plan: &ProvisionPlan,
    records: &BTreeSet<(String, Vec<u8>)>,
    public: &[u8],
    bundle_hash: &[u8; 32],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    digest_part(&mut hash, b"pending-bundle-v1");
    hash.update(plan.digest);
    hash.update(plan.operation_id.to_be_bytes());
    hash.update(plan.revision.to_be_bytes());
    digest_part(&mut hash, plan.local.epoch.as_bytes());
    hash.update(bundle_hash);
    digest_part(&mut hash, public);
    for (key, value) in records {
        digest_part(&mut hash, key.as_bytes());
        digest_part(&mut hash, value)
    }
    hash.finalize().into()
}
/// The creation policy is narrow even though CSPRNG chooses the registration
/// ID after planning. This cannot write sessions, peer bindings or old epochs.
fn creation_records(
    plan: &ProvisionPlan,
    registration: u16,
    mutations: &[Mutation],
) -> Result<BTreeSet<(String, Vec<u8>)>> {
    ensure!(
        registration <= 16383
            && mutations.len() == usize::from(plan.prekey_count) + 2
            && mutations.len() <= MAX_RECORDS,
        "Invalid creation record count"
    );
    let mut expected = BTreeSet::new();
    expected.insert(
        RecordKey::LocalIdentity {
            local: plan.local.clone(),
        }
        .encoded()?,
    );
    expected.insert(
        RecordKey::SignedPreKey {
            local: plan.local.clone(),
            id: u32::from(registration) + 1,
        }
        .encoded()?,
    );
    for index in 0..u32::from(plan.prekey_count) {
        expected.insert(
            RecordKey::PreKey {
                local: plan.local.clone(),
                id: u32::from(registration) + 3 + index,
            }
            .encoded()?,
        );
    }
    let mut result = BTreeSet::new();
    let mut bytes = 0usize;
    for mutation in mutations {
        let Mutation::Put(record) = mutation else {
            bail!("Creation cannot delete records")
        };
        let key = record.key.encoded()?;
        ensure!(
            expected.remove(&key),
            "Creation writes outside its reserved namespace"
        );
        bytes = bytes
            .checked_add(key.len() + 32)
            .and_then(|total| total.checked_add(record.value.len()))
            .ok_or_else(|| anyhow::anyhow!("Creation budget overflow"))?;
        ensure!(bytes <= MAX_SNAPSHOT_BYTES, "Private bundle exceeds budget");
        result.insert((key, record.value.clone()));
    }
    ensure!(expected.is_empty(), "Incomplete creation record set");
    Ok(result)
}
struct CreationData<'a> {
    plan: &'a ProvisionPlan,
    registration: u16,
    mutations: &'a [Mutation],
    public: &'a [u8],
    bundle_hash: &'a [u8; 32],
}
struct StoredCandidate {
    plan: ProvisionPlan,
    state: String,
    stage_digest: Option<Vec<u8>>,
}
fn load_plan(
    conn: &Connection,
    owner: &str,
    operation_id: u64,
    generation: u64,
) -> Result<StoredCandidate> {
    type CandidateRow = (
        i64,
        u8,
        Option<i64>,
        u8,
        i64,
        Vec<u8>,
        String,
        Option<Vec<u8>>,
    );
    let(epoch,platform,previous,count,base,digest,state,stage_digest):CandidateRow=conn.query_row("SELECT epoch,platform,expected_current,prekey_count,base_revision,plan_digest,state,stage_digest FROM e2ee_local_candidates WHERE account=?1 AND operation_id=?2",params![owner,integer(operation_id)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
    let platform = match platform {
        0 => Platform::Mobile,
        1 => Platform::Web,
        2 => Platform::Pc,
        _ => bail!("Invalid candidate platform"),
    };
    ensure!(
        epoch > 0
            && base >= 0
            && (1..=100).contains(&count)
            && previous.is_none_or(|value| value > 0),
        "Invalid stored candidate"
    );
    let digest = digest
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid candidate digest"))?;
    Ok(StoredCandidate {
        plan: ProvisionPlan {
            account: owner.into(),
            local: LocalNamespace {
                epoch: epoch.to_string(),
                platform,
            },
            expected_current: previous.map(|value| value as u64),
            generation,
            operation_id,
            revision: base as u64,
            prekey_count: count,
            digest,
        },
        state,
        stage_digest,
    })
}
impl MessageStore {
    pub fn e2ee_reserve_local_candidate(
        &mut self,
        owner: &str,
        request: &CandidateRequest,
        gate: &GenerationGate,
    ) -> Result<ProvisionPlan> {
        account(owner)?;
        validate_id(&request.stable_intent)?;
        gate.check(request.generation)?;
        ensure!(
            request.platform == Platform::Pc,
            "Only PC local registration is supported"
        );
        ensure!(
            (1..=100).contains(&request.prekey_count),
            "Invalid provisioning prekey count"
        );
        if let Some(previous) = request.expected_current {
            epoch(&previous.to_string())?;
        }
        let digest = plan_digest(owner, request)?;
        let tx = self.conn.transaction()?;
        ensure_owner(&tx, owner)?;
        ensure!(
            current(&tx, owner)? == request.expected_current,
            "Provisioning current epoch changed"
        );
        let existing:Option<(i64,i64,Vec<u8>,String)>=tx.query_row("SELECT epoch,operation_id,plan_digest,state FROM e2ee_local_candidates WHERE account=?1 AND intent=?2",params![owner,request.stable_intent],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (local_epoch, operation_id) = if let Some((epoch, id, previous, state)) = existing {
            ensure!(
                previous == digest && state == "reserved",
                "Provisioning intent is changed or already staged"
            );
            (epoch, id)
        } else {
            ensure!(
                !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM e2ee_local_candidates WHERE account=?1)",
                    [owner],
                    |r| r.get::<_, bool>(0)
                )?,
                "An unresolved registration already exists"
            );
            let next: i64 = tx.query_row(
                "SELECT next_local FROM e2ee_accounts WHERE account=?1",
                [owner],
                |r| r.get(0),
            )?;
            let next = next
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Local epochs exhausted"))?;
            epoch(&next.to_string())?;
            let stable_key = serde_json::to_string(
                &serde_json::json!({"kind":"local_registration","epoch":next.to_string()}),
            )?;
            let id = reserve_operation_id(&tx, owner, &stable_key, &digest)?;
            tx.execute("INSERT INTO e2ee_local_candidates(account,epoch,platform,intent,operation_id,expected_current,prekey_count,state,base_revision,plan_digest) VALUES(?1,?2,?3,?4,?5,?6,?7,'reserved',0,?8)",params![owner,next,request.platform.wire(),request.stable_intent,integer(id)?,request.expected_current.map(|value|value as i64),request.prekey_count,digest.as_slice()])?;
            tx.execute(
                "UPDATE e2ee_accounts SET next_local=?2 WHERE account=?1",
                params![owner, next],
            )?;
            (next, integer(id)?)
        };
        let revision = revision(&tx, owner)?;
        tx.execute("UPDATE e2ee_local_candidates SET base_revision=?3 WHERE account=?1 AND epoch=?2 AND state='reserved'",params![owner,local_epoch,revision as i64])?;
        let lease = gate.linearize(request.generation)?;
        tx.commit()?;
        drop(lease);
        Ok(ProvisionPlan {
            account: owner.into(),
            local: LocalNamespace {
                epoch: local_epoch.to_string(),
                platform: request.platform,
            },
            expected_current: request.expected_current,
            generation: request.generation,
            operation_id: operation_id as u64,
            revision,
            prekey_count: request.prekey_count,
            digest,
        })
    }
    pub fn e2ee_pending_candidate(&self, owner: &str) -> Result<Option<PendingCandidateStatus>> {
        account(owner)?;
        let row:Option<(i64,String)>=self.conn.query_row("SELECT operation_id,intent FROM e2ee_local_candidates WHERE account=?1 AND state IN('reserved','ready','sending','unknown')",[owner],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        row.map(|(id, intent)| {
            validate_id(&intent)?;
            let stored = load_plan(&self.conn, owner, id as u64, 1)?;
            Ok(PendingCandidateStatus {
                account: owner.into(),
                local: stored.plan.local,
                operation_id: id as u64,
                state: PendingState::parse(&stored.state)?,
                expected_current: stored.plan.expected_current,
                prekey_count: stored.plan.prekey_count,
                intent,
            })
        })
        .transpose()
    }
    pub fn e2ee_candidate_state(&self, owner: &str, operation_id: u64) -> Result<PendingState> {
        account(owner)?;
        let state: String = self.conn.query_row(
            "SELECT state FROM e2ee_local_candidates WHERE account=?1 AND operation_id=?2",
            params![owner, integer(operation_id)?],
            |r| r.get(0),
        )?;
        PendingState::parse(&state)
    }
    pub fn e2ee_stage_verified_bundle(
        &mut self,
        candidate: &crate::app::live::signal_crypto::provisioning::EngineCreationCandidate,
        gate: &GenerationGate,
    ) -> Result<PendingBundleReceipt> {
        candidate.check_current(gate)?;
        let plan = candidate.plan();
        let delta = candidate.delta();
        let bundle = candidate.bundle();
        ensure!(
            delta.account == plan.account
                && delta.generation == plan.generation
                && delta.operation_id == plan.operation_id
                && delta.expected_revision == plan.revision
                && delta.input_digest == plan.digest
                && delta.activate_peers.is_empty(),
            "Creation candidate scope mismatch"
        );
        ensure!(
            bundle.prekeys.len() == usize::from(plan.prekey_count)
                && bundle.signed_prekey_id == u32::from(bundle.registration_id) + 1,
            "Creation candidate public keyset mismatch"
        );
        self.stage_pending_bundle(
            CreationData {
                plan,
                registration: bundle.registration_id,
                mutations: &delta.mutations,
                public: candidate.public_payload(),
                bundle_hash: candidate.bundle_fingerprint(),
            },
            gate,
            || candidate.check_preparation(),
        )
    }
    fn stage_pending_bundle(
        &mut self,
        data: CreationData<'_>,
        gate: &GenerationGate,
        check_preparation: impl Fn() -> Result<()>,
    ) -> Result<PendingBundleReceipt> {
        let CreationData {
            plan,
            registration,
            mutations,
            public,
            bundle_hash,
        } = data;
        gate.check(plan.generation)?;
        ensure!(plan.revision < i64::MAX as u64, "Crypto revision exhausted");
        ensure!(
            !public.is_empty() && public.len() <= MAX_PUBLIC_BUNDLE_BYTES,
            "Public registration payload exceeds budget"
        );
        let records = creation_records(plan, registration, mutations)?;
        let digest = private_digest(plan, &records, public, bundle_hash);
        let payload_hash: [u8; 32] = Sha256::digest(public).into();
        let tx = self.conn.transaction()?;
        let(previous,state,base,staged):(Vec<u8>,String,i64,Option<Vec<u8>>)=tx.query_row("SELECT plan_digest,state,base_revision,stage_digest FROM e2ee_local_candidates WHERE account=?1 AND epoch=?2 AND operation_id=?3",params![plan.account,epoch(&plan.local.epoch)?,integer(plan.operation_id)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        ensure!(
            previous == plan.digest && base as u64 == plan.revision,
            "Stale or changed provisioning plan"
        );
        if state != "reserved" {
            ensure!(
                staged.as_deref() == Some(digest.as_slice()),
                "Conflicting staged registration replay"
            );
            tx.rollback()?;
            return Ok(PendingBundleReceipt {
                account: plan.account.clone(),
                local: plan.local.clone(),
                operation_id: plan.operation_id,
                revision: plan.revision + 1,
                replay: true,
            });
        }
        ensure!(
            current(&tx, &plan.account)? == plan.expected_current,
            "Provisioning current epoch changed"
        );
        let next = plan
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Crypto revision exhausted"))?;
        ensure!(
            tx.execute(
                "UPDATE e2ee_accounts SET revision=?3 WHERE account=?1 AND revision=?2",
                params![plan.account, plan.revision as i64, next as i64]
            )? == 1,
            "Stale crypto revision"
        );
        for (key, value) in &records {
            tx.execute(
                "INSERT INTO e2ee_pending_records(account,epoch,key,value) VALUES(?1,?2,?3,?4)",
                params![plan.account, epoch(&plan.local.epoch)?, key, value],
            )?;
        }
        tx.execute("INSERT INTO e2ee_public_outbox(account,operation_id,epoch,codec,command,payload,payload_hash,bundle_hash,state) VALUES(?1,?2,?3,1,540,?4,?5,?6,'ready')",params![plan.account,integer(plan.operation_id)?,epoch(&plan.local.epoch)?,public,payload_hash.as_slice(),bundle_hash.as_slice()])?;
        tx.execute("UPDATE e2ee_local_candidates SET state='ready',stage_digest=?3,registration_id=?4 WHERE account=?1 AND epoch=?2",params![plan.account,epoch(&plan.local.epoch)?,digest.as_slice(),registration])?;
        tx.execute("INSERT INTO e2ee_operations(account,operation_id,digest,input_digest,revision) VALUES(?1,?2,?3,?4,?5)",params![plan.account,integer(plan.operation_id)?,digest.as_slice(),plan.digest.as_slice(),next as i64])?;
        let lease = gate.linearize(plan.generation)?;
        check_preparation()?;
        tx.commit()?;
        drop(lease);
        Ok(PendingBundleReceipt {
            account: plan.account.clone(),
            local: plan.local.clone(),
            operation_id: plan.operation_id,
            revision: next,
            replay: false,
        })
    }
    pub fn e2ee_claim_bundle_upload(
        &mut self,
        owner: &str,
        operation_id: u64,
        context: &PublicAttemptContext,
        gate: &GenerationGate,
    ) -> Result<PublicUploadReceipt> {
        account(owner)?;
        integer(context.socket_epoch)?;
        gate.check(context.generation)?;
        let tx = self.conn.transaction()?;
        let stored = load_plan(&tx, owner, operation_id, context.generation)?;
        let plan = stored.plan;
        ensure!(
            stored.state == "ready" && current(&tx, owner)? == plan.expected_current,
            "Registration is not safely uploadable"
        );
        type PublicRow = (Vec<u8>, Vec<u8>, Vec<u8>, i64);
        let(public,payload_hash,bundle_hash,previous):PublicRow=tx.query_row("SELECT payload,payload_hash,bundle_hash,attempt_counter FROM e2ee_public_outbox WHERE account=?1 AND operation_id=?2 AND state='ready' AND codec=1 AND command=540 AND length(payload)>0 AND length(payload)<=?3 AND length(payload_hash)=32 AND length(bundle_hash)=32",params![owner,integer(operation_id)?,MAX_PUBLIC_BUNDLE_BYTES as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let payload_hash: [u8; 32] = payload_hash
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid public payload hash"))?;
        ensure!(
            payload_hash == Sha256::digest(&public).as_slice(),
            "Public upload bytes changed"
        );
        let bundle_hash: [u8; 32] = bundle_hash
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid bundle hash"))?;
        let(count,bytes):(i64,i64)=tx.query_row("SELECT count(*),COALESCE(sum(length(CAST(key AS BLOB))+length(value)+32),0) FROM e2ee_pending_records WHERE account=?1 AND epoch=?2",params![owner,epoch(&plan.local.epoch)?],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            count == i64::from(plan.prekey_count) + 2
                && bytes >= 0
                && bytes <= MAX_SNAPSHOT_BYTES as i64,
            "Stored pending keys exceed budget"
        );
        let records = {
            let mut stmt=tx.prepare_cached("SELECT key,value FROM e2ee_pending_records WHERE account=?1 AND epoch=?2 ORDER BY key")?;
            let rows = stmt.query_map(params![owner, epoch(&plan.local.epoch)?], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?;
            rows.collect::<std::result::Result<BTreeSet<_>, _>>()?
        };
        let digest = private_digest(&plan, &records, &public, &bundle_hash);
        ensure!(
            stored.stage_digest.as_deref() == Some(digest.as_slice()),
            "Pending public/private bundle binding changed"
        );
        let attempt_id = (previous as u64)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Upload attempts exhausted"))?;
        integer(attempt_id)?;
        // Durable operation IDs and attempts are never reused. Query requests
        // use a separate nonce namespace, so reconnects cannot collide here.
        let request_id = format!("signal_req_upload_{operation_id}_{attempt_id}");
        let mut hash = Sha256::new();
        digest_part(&mut hash, b"public-upload-attempt-v1");
        digest_part(&mut hash, owner.as_bytes());
        hash.update(operation_id.to_be_bytes());
        hash.update(attempt_id.to_be_bytes());
        hash.update(context.generation.to_be_bytes());
        hash.update(context.socket_epoch.to_be_bytes());
        digest_part(&mut hash, request_id.as_bytes());
        hash.update(payload_hash);
        hash.update(digest);
        let attempt_hash: [u8; 32] = hash.finalize().into();
        ensure!(tx.execute("UPDATE e2ee_public_outbox SET state='sending',attempt_counter=?3,attempt_generation=?4,socket_epoch=?5,request_id=?6,attempt_hash=?7 WHERE account=?1 AND operation_id=?2 AND state='ready'",params![owner,integer(operation_id)?,integer(attempt_id)?,integer(context.generation)?,integer(context.socket_epoch)?,request_id,attempt_hash.as_slice()])?==1,"Upload attempt changed");
        tx.execute("UPDATE e2ee_local_candidates SET state='sending' WHERE account=?1 AND operation_id=?2 AND state='ready'",params![owner,integer(operation_id)?])?;
        let lease = gate.linearize(context.generation)?;
        tx.commit()?;
        drop(lease);
        Ok(PublicUploadReceipt {
            account: owner.into(),
            local: plan.local,
            operation_id,
            generation: context.generation,
            attempt_id,
            socket_epoch: context.socket_epoch,
            request_id,
            payload: public,
            payload_hash,
            attempt_hash,
        })
    }
    /// The registration id of a local epoch's identity. Read-only.
    pub fn e2ee_local_registration(&self, owner: &str, local_epoch: u64) -> Result<Option<u16>> {
        account(owner)?;
        Ok(self
            .conn
            .query_row(
                "SELECT registration_id FROM e2ee_local_epochs WHERE account=?1 AND epoch=?2",
                params![owner, integer(local_epoch)?],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// The current local epoch, if any. Read-only.
    pub fn e2ee_current_local(&self, owner: &str) -> Result<Option<u64>> {
        account(owner)?;
        Ok(self
            .conn
            .query_row(
                "SELECT current_local FROM e2ee_accounts WHERE account=?1",
                [owner],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(|value| value as u64))
    }
    /// A query-only handle for an upload whose outcome is unknown (the process died or the socket
    /// dropped after the request left). It can only be reconciled by reading; the outbox row stays
    /// `unknown`, so no second upload can be claimed for it.
    pub fn e2ee_restore_unknown_upload(&self, owner: &str) -> Result<Option<PublicUploadReceipt>> {
        account(owner)?;
        type Row = (
            i64,
            i64,
            Vec<u8>,
            Vec<u8>,
            i64,
            i64,
            i64,
            String,
            Vec<u8>,
            i64,
        );
        let row: Option<Row> = self.conn.query_row(
            "SELECT o.operation_id,o.epoch,o.payload,o.payload_hash,o.attempt_counter,o.attempt_generation,o.socket_epoch,o.request_id,o.attempt_hash,c.platform FROM e2ee_public_outbox o JOIN e2ee_local_candidates c ON c.account=o.account AND c.operation_id=o.operation_id WHERE o.account=?1 AND o.state='unknown' AND o.request_id IS NOT NULL AND o.attempt_hash IS NOT NULL",
            [owner],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)),
        ).optional()?;
        let Some((
            operation,
            local_epoch,
            payload,
            payload_hash,
            attempt,
            generation,
            socket,
            request_id,
            attempt_hash,
            platform,
        )) = row
        else {
            return Ok(None);
        };
        let payload_hash: [u8; 32] = payload_hash
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid public payload hash"))?;
        ensure!(
            payload_hash == Sha256::digest(&payload).as_slice() && platform == 2,
            "Stored upload changed"
        );
        let attempt_hash: [u8; 32] = attempt_hash
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid attempt hash"))?;
        Ok(Some(PublicUploadReceipt {
            account: owner.into(),
            local: LocalNamespace {
                epoch: local_epoch.to_string(),
                platform: Platform::Pc,
            },
            operation_id: operation as u64,
            generation: generation as u64,
            attempt_id: attempt as u64,
            socket_epoch: socket as u64,
            request_id,
            payload,
            payload_hash,
            attempt_hash,
        }))
    }
    /// Installs a confirmed pending bundle as the account's current local identity.
    ///
    /// One transaction: the sealed outbox row, the pending private records and the staged digest must
    /// still match the evidence; the records move into the active namespace; the new epoch becomes
    /// current only if the epoch that was current at planning time still is. The candidate and its
    /// outbox row are then retired, so a later rotation can reserve a new candidate. Requires
    /// `identity_confirmed` (the own 541 bundle matched), never just an upload ACK.
    pub fn e2ee_activate_pending(
        &mut self,
        owner: &str,
        evidence: &ActivationEvidence,
        gate: &GenerationGate,
    ) -> Result<LocalNamespace> {
        account(owner)?;
        ensure!(
            evidence.identity_confirmed && evidence.account == owner,
            "Registration is not confirmed"
        );
        gate.check(evidence.generation)?;
        let tx = self.conn.transaction()?;
        let stored = load_plan(&tx, owner, evidence.operation_id, evidence.generation)?;
        let plan = stored.plan;
        ensure!(
            matches!(stored.state.as_str(), "sending" | "unknown")
                && current(&tx, owner)? == plan.expected_current,
            "Registration is not safely activatable"
        );
        type Outbox = (Vec<u8>, Vec<u8>, Vec<u8>, i64, Option<Vec<u8>>, String);
        let (public, payload_hash, bundle_hash, attempt, attempt_hash, state): Outbox = tx.query_row(
            "SELECT payload,payload_hash,bundle_hash,attempt_counter,attempt_hash,state FROM e2ee_public_outbox WHERE account=?1 AND operation_id=?2 AND epoch=?3 AND codec=1 AND command=540",
            params![owner, integer(evidence.operation_id)?, epoch(&plan.local.epoch)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )?;
        let bundle_hash: [u8; 32] = bundle_hash
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid bundle hash"))?;
        ensure!(
            matches!(state.as_str(), "sending" | "unknown")
                && payload_hash == Sha256::digest(&public).as_slice()
                && payload_hash == evidence.payload_hash
                && attempt as u64 == evidence.attempt_id
                && attempt_hash.as_deref() == Some(evidence.attempt_hash.as_slice()),
            "Upload evidence does not match the staged registration"
        );
        let records = {
            let mut stmt = tx.prepare_cached(
                "SELECT key,value FROM e2ee_pending_records WHERE account=?1 AND epoch=?2 ORDER BY key",
            )?;
            let rows = stmt.query_map(params![owner, epoch(&plan.local.epoch)?], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?;
            rows.collect::<std::result::Result<BTreeSet<_>, _>>()?
        };
        ensure!(
            records.len() == usize::from(plan.prekey_count) + 2
                && stored.stage_digest.as_deref()
                    == Some(private_digest(&plan, &records, &public, &bundle_hash).as_slice()),
            "Pending public/private bundle binding changed"
        );
        let identity =
            crate::protocol::signal::provisioning::CandidateIdentity::from_public_payload(&public)
                .map_err(|_| anyhow::anyhow!("Staged public bundle is invalid"))?;
        ensure!(
            identity.payload_hash() == evidence.payload_hash.as_slice(),
            "Staged public bundle changed"
        );
        let registration = identity.registration_id();
        let session_ts = evidence.timestamp.unwrap_or_else(now_ms);
        ensure!(
            session_ts > 0 && session_ts <= MAX_SAFE_INTEGER,
            "Invalid activation timestamp"
        );
        let local = epoch(&plan.local.epoch)?;
        let moved = tx.execute(
            "INSERT INTO e2ee_records(account,key,value) SELECT account,key,value FROM e2ee_pending_records WHERE account=?1 AND epoch=?2",
            params![owner, local],
        )?;
        ensure!(
            moved == records.len(),
            "Pending records changed during activation"
        );
        tx.execute(
            "DELETE FROM e2ee_pending_records WHERE account=?1 AND epoch=?2",
            params![owner, local],
        )?;
        tx.execute(
            "INSERT INTO e2ee_local_epochs VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                owner,
                local,
                plan.local.platform.wire(),
                registration,
                Sha256::digest(identity.identity_key()).as_slice(),
                session_ts as i64,
                u64::from(registration) + 3 + u64::from(plan.prekey_count)
            ],
        )?;
        ensure!(
            tx.execute(
                "UPDATE e2ee_accounts SET current_local=?2,revision=revision+1 WHERE account=?1",
                params![owner, local]
            )? == 1,
            "Account state changed during activation"
        );
        tx.execute(
            "DELETE FROM e2ee_public_outbox WHERE account=?1 AND operation_id=?2",
            params![owner, integer(evidence.operation_id)?],
        )?;
        tx.execute(
            "DELETE FROM e2ee_local_candidates WHERE account=?1 AND operation_id=?2",
            params![owner, integer(evidence.operation_id)?],
        )?;
        let lease = gate.linearize(evidence.generation)?;
        tx.commit()?;
        drop(lease);
        Ok(plan.local)
    }
    /// Discards an `unknown` registration that a read of the account showed was never installed, so a
    /// fresh candidate can be reserved. Only the staged pending keys, their outbox row and the candidate
    /// go; the active epoch, its records and every history table are untouched.
    pub fn e2ee_abandon_unknown_candidate(
        &mut self,
        owner: &str,
        evidence: &AbandonEvidence,
    ) -> Result<()> {
        account(owner)?;
        ensure!(
            evidence.account == owner,
            "Evidence belongs to another account"
        );
        let tx = self.conn.transaction()?;
        type Row = (i64, Vec<u8>, Option<Vec<u8>>, String);
        let (local, payload_hash, attempt_hash, state): Row = tx.query_row(
            "SELECT epoch,payload_hash,attempt_hash,state FROM e2ee_public_outbox WHERE account=?1 AND operation_id=?2",
            params![owner, integer(evidence.operation_id)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        ensure!(
            state == "unknown"
                && payload_hash == evidence.payload_hash
                && attempt_hash.as_deref() == Some(evidence.attempt_hash.as_slice()),
            "Only an unknown upload matching the evidence can be abandoned"
        );
        tx.execute(
            "DELETE FROM e2ee_pending_records WHERE account=?1 AND epoch=?2",
            params![owner, local],
        )?;
        tx.execute(
            "DELETE FROM e2ee_public_outbox WHERE account=?1 AND operation_id=?2",
            params![owner, integer(evidence.operation_id)?],
        )?;
        ensure!(
            tx.execute(
                "DELETE FROM e2ee_local_candidates WHERE account=?1 AND operation_id=?2 AND state='unknown'",
                params![owner, integer(evidence.operation_id)?],
            )? == 1,
            "The candidate is not abandonable"
        );
        tx.commit()?;
        Ok(())
    }
    pub fn e2ee_recover_bundle_uploads(&mut self, owner: &str) -> Result<usize> {
        account(owner)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE e2ee_public_outbox SET state='unknown' WHERE account=?1 AND state='sending'",
            [owner],
        )?;
        tx.execute(
            "UPDATE e2ee_local_candidates SET state='unknown' WHERE account=?1 AND state='sending'",
            [owner],
        )?;
        tx.commit()?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> CandidateRequest {
        CandidateRequest {
            platform: Platform::Pc,
            expected_current: None,
            stable_intent: "initial-pc".into(),
            prekey_count: 2,
            generation: 1,
        }
    }
    fn fixture(plan: &ProvisionPlan) -> Vec<Mutation> {
        vec![
            Mutation::Put(Record {
                key: RecordKey::LocalIdentity {
                    local: plan.local.clone(),
                },
                value: b"independent identity fixture".to_vec(),
            }),
            Mutation::Put(Record {
                key: RecordKey::SignedPreKey {
                    local: plan.local.clone(),
                    id: 8,
                },
                value: b"independent signed key fixture".to_vec(),
            }),
            Mutation::Put(Record {
                key: RecordKey::PreKey {
                    local: plan.local.clone(),
                    id: 10,
                },
                value: b"independent prekey one".to_vec(),
            }),
            Mutation::Put(Record {
                key: RecordKey::PreKey {
                    local: plan.local.clone(),
                    id: 11,
                },
                value: b"independent prekey two".to_vec(),
            }),
        ]
    }
    fn stage(
        store: &mut MessageStore,
        plan: &ProvisionPlan,
        gate: &GenerationGate,
    ) -> Result<PendingBundleReceipt> {
        // Isolated SQL transaction fixture. The public production entry point
        // accepts only an independently validated, sealed engine candidate.
        store.stage_pending_bundle(
            CreationData {
                plan,
                registration: 7,
                mutations: &fixture(plan),
                public: b"synthetic public bundle",
                bundle_hash: &[7; 32],
            },
            gate,
            || Ok(()),
        )
    }
    #[test]
    fn candidate_reservation_is_account_scoped_bounded_and_never_creates_a_binding() {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = GenerationGate::new(1).unwrap();
        let req = request();
        let first = store
            .e2ee_reserve_local_candidate("123", &req, &gate)
            .unwrap();
        assert_eq!(first.local.epoch, "1");
        assert!(first.snapshot().records.is_empty());
        assert_eq!(first.snapshot().revision, 0);
        let again = store
            .e2ee_reserve_local_candidate("123", &req, &gate)
            .unwrap();
        assert_eq!(again.operation_id, first.operation_id);
        assert!(again.local == first.local);
        assert!(current(&store.conn, "123").unwrap().is_none());
        for table in [
            "e2ee_local_epochs",
            "e2ee_records",
            "e2ee_pending_records",
            "e2ee_public_outbox",
        ] {
            assert_eq!(
                store
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        let mut changed = request();
        changed.prekey_count = 3;
        assert!(store
            .e2ee_reserve_local_candidate("123", &changed, &gate)
            .is_err());
        changed.stable_intent = "other-intent".into();
        assert!(store
            .e2ee_reserve_local_candidate("123", &changed, &gate)
            .is_err());
        assert!(store
            .e2ee_reserve_local_candidate("999", &req, &gate)
            .is_ok());
        let mut oversized = request();
        oversized.prekey_count = 101;
        assert!(store
            .e2ee_reserve_local_candidate("456", &oversized, &gate)
            .is_err());
        gate.cancel();
        assert!(store
            .e2ee_reserve_local_candidate("456", &req, &gate)
            .is_err());
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM e2ee_accounts WHERE account='456'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn staging_private_keys_and_public_outbox_rolls_back_on_write_and_final_check_failure() {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = GenerationGate::new(1).unwrap();
        let plan = store
            .e2ee_reserve_local_candidate("123", &request(), &gate)
            .unwrap();
        store.conn.execute_batch("CREATE TRIGGER reject_public BEFORE INSERT ON e2ee_public_outbox BEGIN SELECT RAISE(ABORT,'fixture public outbox failure'); END;").unwrap();
        assert!(stage(&mut store, &plan, &gate).is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), 0);
        assert!(
            store
                .e2ee_candidate_state("123", plan.operation_id)
                .unwrap()
                == PendingState::Reserved
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM e2ee_pending_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        store
            .conn
            .execute_batch("DROP TRIGGER reject_public")
            .unwrap();
        assert!(store
            .stage_pending_bundle(
                CreationData {
                    plan: &plan,
                    registration: 7,
                    mutations: &fixture(&plan),
                    public: b"synthetic public bundle",
                    bundle_hash: &[7; 32]
                },
                &gate,
                || bail!("fixture cancellation checkpoint")
            )
            .is_err());
        assert!(gate.is_current(1));
        assert_eq!(revision(&store.conn, "123").unwrap(), 0);
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM e2ee_public_outbox", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let receipt = stage(&mut store, &plan, &gate).unwrap();
        assert_eq!(receipt.revision(), 1);
        assert!(!receipt.is_replay());
        assert!(stage(&mut store, &plan, &gate).unwrap().is_replay());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM e2ee_pending_records", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert!(current(&store.conn, "123").unwrap().is_none());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM e2ee_local_epochs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn pending_rotation_preserves_active_epoch_old_private_records_and_history() {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = GenerationGate::new(1).unwrap();
        let mut identity = [1; 33];
        identity[0] = 5;
        let old = store
            .e2ee_register_local(
                "123",
                None,
                &VerifiedBinding {
                    platform: Platform::Pc,
                    registration_id: 7,
                    identity_key: identity,
                    session_timestamp: 100,
                },
            )
            .unwrap();
        let key = RecordKey::LocalIdentity { local: old.clone() }
            .encoded()
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO e2ee_records(account,key,value) VALUES('123',?1,?2)",
                params![key, b"old private identity bytes"],
            )
            .unwrap();
        store
            .insert_batch(&[Message {
                account_id: "123".into(),
                thread_id: "d:456".into(),
                id: "old-history".into(),
                sender: "456".into(),
                text: "Synthetic retained history".into(),
                sticker: None,
                media: None,
                sent_at_ms: 1000,
            }])
            .unwrap();
        let mut req = request();
        req.expected_current = Some(old.epoch.parse().unwrap());
        let plan = store
            .e2ee_reserve_local_candidate("123", &req, &gate)
            .unwrap();
        assert_eq!(plan.local.epoch, "2");
        stage(&mut store, &plan, &gate).unwrap();
        assert_eq!(current(&store.conn, "123").unwrap(), Some(1));
        assert_eq!(
            store
                .e2ee_snapshot("123", &[RecordKey::LocalIdentity { local: old.clone() }])
                .unwrap()
                .records[0]
                .value,
            b"old private identity bytes"
        );
        store
            .e2ee_validate_local_identity("123", &old, 7, &identity)
            .unwrap();
        assert_eq!(
            store.page("123", "d:456", None, 10).unwrap()[0].text,
            "Synthetic retained history"
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM e2ee_local_epochs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(store
            .e2ee_snapshot(
                "123",
                &[RecordKey::LocalIdentity {
                    local: plan.local.clone()
                }]
            )
            .is_err());
    }
    #[test]
    fn creation_policy_rejects_old_peer_delete_and_oversized_records_before_mutation() {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = GenerationGate::new(1).unwrap();
        let plan = store
            .e2ee_reserve_local_candidate("123", &request(), &gate)
            .unwrap();
        let mut bad = fixture(&plan);
        bad[0] = Mutation::Delete(RecordKey::LocalIdentity {
            local: plan.local.clone(),
        });
        assert!(creation_records(&plan, 7, &bad).is_err());
        bad[0] = Mutation::Put(Record {
            key: RecordKey::LocalIdentity {
                local: LocalNamespace {
                    epoch: "99".into(),
                    platform: Platform::Pc,
                },
            },
            value: vec![],
        });
        assert!(creation_records(&plan, 7, &bad).is_err());
        let mut large = fixture(&plan);
        let Mutation::Put(record) = &mut large[0] else {
            unreachable!()
        };
        record.value = vec![0; MAX_SNAPSHOT_BYTES];
        assert!(store
            .stage_pending_bundle(
                CreationData {
                    plan: &plan,
                    registration: 7,
                    mutations: &large,
                    public: b"public",
                    bundle_hash: &[7; 32]
                },
                &gate,
                || Ok(())
            )
            .is_err());
        assert!(store
            .stage_pending_bundle(
                CreationData {
                    plan: &plan,
                    registration: 7,
                    mutations: &fixture(&plan),
                    public: &vec![0; MAX_PUBLIC_BUNDLE_BYTES + 1],
                    bundle_hash: &[7; 32]
                },
                &gate,
                || Ok(())
            )
            .is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), 0);
        assert!(
            store
                .e2ee_candidate_state("123", plan.operation_id)
                .unwrap()
                == PendingState::Reserved
        );
    }
    #[test]
    fn restart_retains_exact_keys_public_payload_and_unknown_upload_is_not_reclaimed() {
        let path = std::env::temp_dir().join(format!(
            "zalo-pending-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let gate = GenerationGate::new(1).unwrap();
        let op;
        {
            let mut store = MessageStore::open(&path).unwrap();
            let plan = store
                .e2ee_reserve_local_candidate("123", &request(), &gate)
                .unwrap();
            op = plan.operation_id;
            stage(&mut store, &plan, &gate).unwrap();
        }
        let request_id;
        {
            let mut store = MessageStore::open(&path).unwrap();
            let recovered = store.e2ee_pending_candidate("123").unwrap().unwrap();
            assert_eq!(recovered.operation_id(), op);
            assert!(recovered.state() == PendingState::Ready);
            assert_eq!(recovered.stable_intent(), "initial-pc");
            assert!(store.e2ee_pending_candidate("999").unwrap().is_none());
            assert!(store
                .e2ee_claim_bundle_upload(
                    "999",
                    op,
                    &PublicAttemptContext {
                        generation: 1,
                        socket_epoch: 1
                    },
                    &gate
                )
                .is_err());
            let token = store
                .e2ee_claim_bundle_upload(
                    "123",
                    op,
                    &PublicAttemptContext {
                        generation: 1,
                        socket_epoch: 1,
                    },
                    &gate,
                )
                .unwrap();
            request_id = token.request_id().to_owned();
            assert_eq!(token.payload(), b"synthetic public bundle");
            assert!(token.can_submit(&gate));
            assert!(store
                .e2ee_claim_bundle_upload(
                    "123",
                    op,
                    &PublicAttemptContext {
                        generation: 1,
                        socket_epoch: 1
                    },
                    &gate
                )
                .is_err());
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            assert_eq!(store.e2ee_recover_bundle_uploads("999").unwrap(), 0);
            assert_eq!(store.e2ee_recover_bundle_uploads("123").unwrap(), 1);
            assert!(store.e2ee_candidate_state("123", op).unwrap() == PendingState::Unknown);
            gate.replace(2).unwrap();
            assert!(store
                .e2ee_claim_bundle_upload(
                    "123",
                    op,
                    &PublicAttemptContext {
                        generation: 2,
                        socket_epoch: 2
                    },
                    &gate
                )
                .is_err());
            assert_eq!(
                store
                    .conn
                    .query_row(
                        "SELECT request_id FROM e2ee_public_outbox WHERE account='123'",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                request_id
            );
            assert_eq!(
                store
                    .conn
                    .query_row("SELECT count(*) FROM e2ee_pending_records", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                4
            );
            assert!(current(&store.conn, "123").unwrap().is_none());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn tampered_private_or_public_bytes_cannot_claim_a_registration_upload() {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = GenerationGate::new(1).unwrap();
        let plan = store
            .e2ee_reserve_local_candidate("123", &request(), &gate)
            .unwrap();
        stage(&mut store, &plan, &gate).unwrap();
        store
            .conn
            .execute(
                "UPDATE e2ee_pending_records SET value=x'00' WHERE account='123'",
                [],
            )
            .unwrap();
        assert!(store
            .e2ee_claim_bundle_upload(
                "123",
                plan.operation_id,
                &PublicAttemptContext {
                    generation: 1,
                    socket_epoch: 1
                },
                &gate
            )
            .is_err());
        assert!(
            store
                .e2ee_candidate_state("123", plan.operation_id)
                .unwrap()
                == PendingState::Ready
        );
        store
            .conn
            .execute(
                "UPDATE e2ee_public_outbox SET payload=x'00' WHERE account='123'",
                [],
            )
            .unwrap();
        assert!(store
            .e2ee_claim_bundle_upload(
                "123",
                plan.operation_id,
                &PublicAttemptContext {
                    generation: 1,
                    socket_epoch: 1
                },
                &gate
            )
            .is_err());
    }
}
