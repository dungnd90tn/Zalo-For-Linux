//! Pending registration creation only. No server I/O, active-key replacement,
//! timestamp invention or activation authority is exposed here.
use super::{wire_platform, AccountEngine, Cancellation, BATCH_TIMEOUT};
use crate::{
    protocol::signal,
    store::e2ee::{self, provisioning::ProvisionPlan},
};
use anyhow::{anyhow, ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use libsignal_core::curve::{PrivateKey, PublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::Instant;
use zalo_signal_engine as engine;

pub struct EngineCreationCandidate {
    plan: ProvisionPlan,
    delta: e2ee::Delta,
    bundle: engine::PublicBundle,
    public_payload: Vec<u8>,
    bundle_fingerprint: [u8; 32],
    cancellation: Cancellation,
    deadline: Instant,
}
impl EngineCreationCandidate {
    pub(crate) fn plan(&self) -> &ProvisionPlan {
        &self.plan
    }
    pub(crate) fn delta(&self) -> &e2ee::Delta {
        &self.delta
    }
    pub(crate) fn bundle(&self) -> &engine::PublicBundle {
        &self.bundle
    }
    pub(crate) fn public_payload(&self) -> &[u8] {
        &self.public_payload
    }
    pub(crate) fn bundle_fingerprint(&self) -> &[u8; 32] {
        &self.bundle_fingerprint
    }
    pub(crate) fn check_current(&self, gate: &e2ee::GenerationGate) -> Result<()> {
        self.check_preparation()?;
        ensure!(
            gate.is_current(self.plan.generation()),
            "Provisioning generation is stale"
        );
        Ok(())
    }
    pub(crate) fn check_preparation(&self) -> Result<()> {
        self.cancellation.check()?;
        ensure!(
            Instant::now() < self.deadline,
            "Provisioning preparation expired"
        );
        Ok(())
    }
}

/// Owns only a lazy bounded engine. The actor reserves the opaque plan before
/// awaiting this worker and later stages its sealed result in one transaction.
pub struct PendingProvisioner {
    worker: AccountEngine,
}
impl PendingProvisioner {
    pub fn new(account: engine::AccountId, generation: u64) -> Result<Self> {
        Ok(AccountEngine::new(account, generation)?.provisioner())
    }
    pub(super) fn from_worker(worker: AccountEngine) -> Self {
        Self { worker }
    }
    pub async fn prepare(
        &mut self,
        plan: ProvisionPlan,
        gate: &e2ee::GenerationGate,
        cancel: &Cancellation,
    ) -> Result<EngineCreationCandidate> {
        let deadline = Instant::now() + BATCH_TIMEOUT;
        cancel.check()?;
        ensure!(
            plan.account() == self.worker.inner.account.as_str()
                && plan.generation() == self.worker.inner.generation
                && gate.is_current(self.worker.inner.generation),
            "Provisioning plan scope mismatch"
        );
        ensure!(
            (1..=100).contains(&plan.prekey_count()),
            "Invalid provisioning prekey count"
        );
        let local: engine::LocalNamespace =
            serde_json::from_value(serde_json::to_value(plan.local())?)?;
        let snapshot = e2ee::engine_adapter::into_engine_snapshot(plan.snapshot())?;
        ensure!(
            snapshot.records().is_empty()
                && snapshot.account() == &self.worker.inner.account
                && snapshot.revision() == plan.expected_revision(),
            "Creation requires an empty reserved local snapshot"
        );
        let completed = self
            .worker
            .execute(
                engine::Operation::CreateBundle {
                    local: local.clone(),
                    prekey_count: plan.prekey_count(),
                },
                snapshot,
                gate,
                cancel,
                deadline,
            )
            .await?;
        seal_result(
            plan,
            completed.result,
            completed.job,
            cancel.clone(),
            deadline,
        )
    }
}

fn seal_result(
    plan: ProvisionPlan,
    result: engine::OperationResult,
    job: u64,
    cancellation: Cancellation,
    deadline: Instant,
) -> Result<EngineCreationCandidate> {
    ensure!(
        result.account().as_str() == plan.account()
            && result.generation() == plan.generation()
            && result.operation_id() == job
            && result.expected_revision() == plan.expected_revision(),
        "Creation result context mismatch"
    );
    let (account, generation, _, revision, output, mutations) = result.into_parts();
    let engine::Output::Bundle(bundle) = output else {
        return Err(anyhow!("Creation returned an unexpected output"));
    };
    let mut converted = Vec::new();
    for mutation in mutations.into_mutations() {
        let engine::Mutation::Put(record) = mutation else {
            return Err(anyhow!("Creation cannot delete existing records"));
        };
        let (key, value) = record.into_parts();
        converted.push(e2ee::Mutation::Put(e2ee::Record {
            key: serde_json::from_slice(&serde_json::to_vec(&key)?)?,
            value,
        }));
    }
    validate_material(&plan, &bundle, &converted)?;
    let allowed_keys = converted
        .iter()
        .map(|mutation| match mutation {
            e2ee::Mutation::Put(record) => record.key.clone(),
            e2ee::Mutation::Delete(_) => unreachable!(),
        })
        .collect();
    let public_payload = public_payload(&plan, &bundle)?;
    let mut hash = Sha256::new();
    hash.update(b"zalo-pending-public-bundle-v1\0");
    hash.update((public_payload.len() as u64).to_be_bytes());
    hash.update(&public_payload);
    let bundle_fingerprint = hash.finalize().into();
    let delta = e2ee::Delta {
        account: account.as_str().into(),
        expected_revision: revision,
        generation,
        operation_id: plan.operation_id(),
        input_digest: *plan.plan_digest(),
        allowed_keys,
        activate_peers: vec![],
        mutations: converted,
    };
    Ok(EngineCreationCandidate {
        plan,
        delta,
        bundle,
        public_payload,
        bundle_fingerprint,
        cancellation,
        deadline,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyPair {
    #[serde(rename = "publicKey")]
    public: Vec<u8>,
    #[serde(rename = "privateKey")]
    private: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityPair {
    #[serde(rename = "publicKey")]
    public: Vec<u8>,
    #[serde(rename = "privateKey")]
    private: Vec<u8>,
    #[serde(rename = "registrationId")]
    registration: u16,
}
fn verify_pair(public: &[u8], private: &[u8], expected: &[u8; 33]) -> Result<()> {
    ensure!(
        public == expected && public.len() == 33 && public[0] == 5 && private.len() == 32,
        "Creation public/private key framing mismatch"
    );
    let key =
        PrivateKey::deserialize(private).map_err(|_| anyhow!("Invalid creation private key"))?;
    let derived = key
        .public_key()
        .map_err(|_| anyhow!("Cannot derive creation public key"))?
        .serialize();
    ensure!(
        &*derived == public,
        "Creation private key does not match its public key"
    );
    Ok(())
}
fn validate_material(
    plan: &ProvisionPlan,
    bundle: &engine::PublicBundle,
    mutations: &[e2ee::Mutation],
) -> Result<()> {
    ensure!(
        bundle.registration_id <= 16383
            && bundle.prekeys.len() == usize::from(plan.prekey_count())
            && mutations.len() == bundle.prekeys.len() + 2
            && mutations.len() <= engine::MAX_RECORDS,
        "Creation bundle count or registration mismatch"
    );
    let reg = u32::from(bundle.registration_id);
    ensure!(
        bundle.signed_prekey_id == reg + 1,
        "Creation signed prekey policy mismatch"
    );
    let mut expected = BTreeMap::new();
    expected.insert(
        serde_json::to_string(&e2ee::RecordKey::LocalIdentity {
            local: plan.local().clone(),
        })?,
        bundle.identity_key,
    );
    expected.insert(
        serde_json::to_string(&e2ee::RecordKey::SignedPreKey {
            local: plan.local().clone(),
            id: reg + 1,
        })?,
        bundle.signed_prekey,
    );
    for (index, (id, public)) in bundle.prekeys.iter().enumerate() {
        ensure!(
            *id == reg + 3 + index as u32,
            "Creation one-time prekey policy mismatch"
        );
        expected.insert(
            serde_json::to_string(&e2ee::RecordKey::PreKey {
                local: plan.local().clone(),
                id: *id,
            })?,
            *public,
        );
    }
    let identity = PublicKey::deserialize(&bundle.identity_key)
        .map_err(|_| anyhow!("Invalid creation identity public key"))?;
    ensure!(
        identity.verify_signature(&bundle.signed_prekey, &bundle.signature),
        "Creation signed prekey signature rejected"
    );
    let mut seen = BTreeSet::new();
    let mut bytes = 0usize;
    for mutation in mutations {
        let e2ee::Mutation::Put(record) = mutation else {
            return Err(anyhow!("Creation cannot delete existing records"));
        };
        let key = serde_json::to_string(&record.key)?;
        ensure!(seen.insert(key.clone()), "Duplicate creation record");
        let expected = expected
            .remove(&key)
            .ok_or_else(|| anyhow!("Creation record is outside its reserved namespace"))?;
        bytes = bytes
            .checked_add(key.len() + record.value.len() + 32)
            .ok_or_else(|| anyhow!("Creation record budget exceeded"))?;
        ensure!(
            bytes <= engine::MAX_SNAPSHOT_BYTES,
            "Creation record budget exceeded"
        );
        if matches!(record.key, e2ee::RecordKey::LocalIdentity { .. }) {
            let pair: IdentityPair = serde_json::from_slice(&record.value)
                .map_err(|_| anyhow!("Malformed creation private identity record"))?;
            ensure!(
                pair.registration == bundle.registration_id,
                "Creation private identity registration mismatch"
            );
            verify_pair(&pair.public, &pair.private, &expected)?;
        } else {
            let pair: KeyPair = serde_json::from_slice(&record.value)
                .map_err(|_| anyhow!("Malformed creation private key record"))?;
            verify_pair(&pair.public, &pair.private, &expected)?;
        }
    }
    ensure!(expected.is_empty(), "Creation private keyset is incomplete");
    Ok(())
}
fn public_payload(plan: &ProvisionPlan, bundle: &engine::PublicBundle) -> Result<Vec<u8>> {
    let parse = |key: &[u8; 33]| signal::PublicKey::parse(&STANDARD.encode(key));
    let public = signal::PublicBundle {
        device: wire_platform(
            serde_json::from_value::<engine::LocalNamespace>(serde_json::to_value(plan.local())?)?
                .platform,
        ),
        registration_id: bundle.registration_id,
        identity: parse(&bundle.identity_key)?,
        signed_prekey_id: bundle.signed_prekey_id,
        signed_prekey: parse(&bundle.signed_prekey)?,
        signature: signal::Signature::parse(&STANDARD.encode(bundle.signature))?,
        one_time_prekeys: bundle
            .prekeys
            .iter()
            .map(|(id, key)| {
                Ok(signal::OneTimePreKey {
                    id: *id,
                    key: parse(key)?,
                })
            })
            .collect::<Result<_>>()?,
    };
    let scope = signal::Scope::new(plan.account(), plan.generation())?;
    // `delSS: 1`, as the original client sends (observed on the wire): the service drops this device's old
    // sessions and tells peers to initialize new ones toward the new key.
    let wire = signal::submit_bundle(&scope, 0, &public, true)?.wire_data(&scope)?;
    let payload = serde_json::to_vec(&wire["data"])?;
    ensure!(
        !payload.is_empty() && payload.len() <= e2ee::provisioning::MAX_PUBLIC_BUNDLE_BYTES,
        "Creation public payload budget exceeded"
    );
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MessageStore;
    async fn genuine_candidate() -> EngineCreationCandidate {
        let mut store = MessageStore::in_memory().unwrap();
        let gate = e2ee::GenerationGate::new(1).unwrap();
        let plan = store
            .e2ee_reserve_local_candidate(
                "101",
                &e2ee::provisioning::CandidateRequest {
                    platform: e2ee::Platform::Pc,
                    expected_current: None,
                    stable_intent: "synthetic-material-check".into(),
                    prekey_count: 3,
                    generation: 1,
                },
                &gate,
            )
            .unwrap();
        PendingProvisioner::new(engine::AccountId::new("101").unwrap(), 1)
            .unwrap()
            .prepare(plan, &gate, &Cancellation::default())
            .await
            .unwrap()
    }
    fn copy_mutations(candidate: &EngineCreationCandidate) -> Vec<e2ee::Mutation> {
        candidate
            .delta
            .mutations
            .iter()
            .map(|mutation| match mutation {
                e2ee::Mutation::Put(record) => e2ee::Mutation::Put(e2ee::Record {
                    key: record.key.clone(),
                    value: record.value.clone(),
                }),
                _ => panic!("unexpected deletion"),
            })
            .collect()
    }
    fn copy_bundle(value: &engine::PublicBundle) -> engine::PublicBundle {
        engine::PublicBundle {
            registration_id: value.registration_id,
            identity_key: value.identity_key,
            signed_prekey_id: value.signed_prekey_id,
            signed_prekey: value.signed_prekey,
            signature: value.signature,
            prekeys: value.prekeys.clone(),
        }
    }
    #[tokio::test]
    async fn mature_primitive_checks_reject_private_public_signature_and_keyset_tampering() {
        let candidate = genuine_candidate().await;
        let mut mutations = copy_mutations(&candidate);
        let record = mutations
            .iter_mut()
            .find_map(|mutation| match mutation {
                e2ee::Mutation::Put(record)
                    if matches!(record.key, e2ee::RecordKey::LocalIdentity { .. }) =>
                {
                    Some(record)
                }
                _ => None,
            })
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&record.value).unwrap();
        value["privateKey"][10] = (value["privateKey"][10].as_u64().unwrap() ^ 1).into();
        record.value = serde_json::to_vec(&value).unwrap();
        assert!(validate_material(&candidate.plan, &candidate.bundle, &mutations).is_err());
        let mut bundle = copy_bundle(&candidate.bundle);
        bundle.signature[0] ^= 1;
        assert!(validate_material(&candidate.plan, &bundle, &candidate.delta.mutations).is_err());
        let mut bundle = copy_bundle(&candidate.bundle);
        bundle.prekeys[0].1[5] ^= 1;
        assert!(validate_material(&candidate.plan, &bundle, &candidate.delta.mutations).is_err());
        let mut bundle = copy_bundle(&candidate.bundle);
        bundle.prekeys[0].0 += 100;
        assert!(validate_material(&candidate.plan, &bundle, &candidate.delta.mutations).is_err());
        let mut mutations = copy_mutations(&candidate);
        mutations.pop();
        assert!(validate_material(&candidate.plan, &candidate.bundle, &mutations).is_err());
        let mut mutations = copy_mutations(&candidate);
        let e2ee::Mutation::Put(record) = &mut mutations[0] else {
            unreachable!()
        };
        match &mut record.key {
            e2ee::RecordKey::LocalIdentity { local }
            | e2ee::RecordKey::PreKey { local, .. }
            | e2ee::RecordKey::SignedPreKey { local, .. } => local.epoch = "2".into(),
            _ => unreachable!(),
        };
        assert!(validate_material(&candidate.plan, &candidate.bundle, &mutations).is_err());
        let other = genuine_candidate().await;
        assert!(other.bundle.identity_key != candidate.bundle.identity_key);
    }
    #[tokio::test]
    async fn actual_sealed_creation_result_rejects_unrelated_job_and_namespace() {
        for wrong_namespace in [false, true] {
            let mut store = MessageStore::in_memory().unwrap();
            let gate = e2ee::GenerationGate::new(1).unwrap();
            let plan = store
                .e2ee_reserve_local_candidate(
                    "101",
                    &e2ee::provisioning::CandidateRequest {
                        platform: e2ee::Platform::Pc,
                        expected_current: None,
                        stable_intent: "synthetic-scope-check".into(),
                        prekey_count: 3,
                        generation: 1,
                    },
                    &gate,
                )
                .unwrap();
            let local = engine::LocalNamespace {
                platform: engine::SignalPlatform::Pc,
                epoch: engine::RegistrationEpoch::new(if wrong_namespace { 2 } else { 1 }).unwrap(),
            };
            let worker = AccountEngine::new(engine::AccountId::new("101").unwrap(), 1).unwrap();
            let completed = worker
                .execute(
                    engine::Operation::CreateBundle {
                        local,
                        prekey_count: 3,
                    },
                    e2ee::engine_adapter::into_engine_snapshot(plan.snapshot()).unwrap(),
                    &gate,
                    &Cancellation::default(),
                    Instant::now() + BATCH_TIMEOUT,
                )
                .await
                .unwrap();
            assert!(seal_result(
                plan,
                completed.result,
                if wrong_namespace {
                    completed.job
                } else {
                    completed.job + 1
                },
                Cancellation::default(),
                Instant::now() + BATCH_TIMEOUT
            )
            .is_err());
        }
    }
}
