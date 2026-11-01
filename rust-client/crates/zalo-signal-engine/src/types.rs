use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

pub const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_CIPHERTEXT_BYTES: usize = 128 * 1024;
pub const MAX_RECORDS: usize = 512;
pub const QUEUE_CAPACITY: usize = 16;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AccountId(String);
impl AccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, EngineError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 32
            || value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(EngineError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid account identifier"))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct GroupId(String);
impl GroupId {
    pub fn new(value: impl Into<String>) -> Result<Self, EngineError> {
        let id = AccountId::new(value)?;
        Ok(Self(id.0))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for GroupId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid group identifier"))
    }
}

/// Store-allocated stable registration epoch; never a transport/auth generation.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct RegistrationEpoch(String);
impl RegistrationEpoch {
    pub fn new(value: u64) -> Result<Self, EngineError> {
        if value == 0 {
            Err(EngineError::InvalidInput)
        } else {
            Ok(Self(value.to_string()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for RegistrationEpoch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let epoch = raw
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0 && value.to_string() == raw)
            .ok_or_else(|| serde::de::Error::custom("invalid registration epoch"))?;
        Self::new(epoch).map_err(|_| serde::de::Error::custom("invalid registration epoch"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignalPlatform {
    Mobile,
    Web,
    Pc,
}
impl SignalPlatform {
    pub fn wire_id(self) -> u8 {
        match self {
            Self::Mobile => 0,
            Self::Web => 1,
            Self::Pc => 2,
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalNamespace {
    pub epoch: RegistrationEpoch,
    pub platform: SignalPlatform,
}

/// `epoch` is allocated for a verified identity/session transition. A 14-bit
/// registration ID alone is insufficient to distinguish device generations.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerNamespace {
    pub uid: AccountId,
    pub platform: SignalPlatform,
    pub epoch: RegistrationEpoch,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordKey {
    LocalIdentity {
        local: LocalNamespace,
    },
    PreKey {
        local: LocalNamespace,
        id: u32,
    },
    SignedPreKey {
        local: LocalNamespace,
        id: u32,
    },
    Session {
        local: LocalNamespace,
        peer: PeerNamespace,
    },
    PeerIdentity {
        local: LocalNamespace,
        peer: PeerNamespace,
    },
    SessionTimestamp {
        local: LocalNamespace,
        peer: PeerNamespace,
    },
    GroupSender {
        local: LocalNamespace,
        group: GroupId,
        sender: PeerNamespace,
    },
}
impl RecordKey {
    pub fn validate(&self) -> Result<(), EngineError> {
        let local = match self {
            Self::LocalIdentity { local }
            | Self::PreKey { local, .. }
            | Self::SignedPreKey { local, .. }
            | Self::Session { local, .. }
            | Self::PeerIdentity { local, .. }
            | Self::SessionTimestamp { local, .. }
            | Self::GroupSender { local, .. } => local,
        };
        let epoch = local
            .epoch
            .0
            .parse::<u64>()
            .map_err(|_| EngineError::InvalidRecord)?;
        if epoch == 0 || epoch.to_string() != local.epoch.0 {
            return Err(EngineError::InvalidRecord);
        }
        match self {
            Self::Session { peer, .. }
            | Self::PeerIdentity { peer, .. }
            | Self::SessionTimestamp { peer, .. }
            | Self::GroupSender { sender: peer, .. } => {
                AccountId::new(peer.uid.0.clone()).map_err(|_| EngineError::InvalidRecord)?;
                let epoch = peer
                    .epoch
                    .0
                    .parse::<u64>()
                    .map_err(|_| EngineError::InvalidRecord)?;
                if epoch == 0 || epoch.to_string() != peer.epoch.0 {
                    return Err(EngineError::InvalidRecord);
                }
            }
            _ => {}
        }
        if let Self::GroupSender { group, .. } = self {
            GroupId::new(group.0.clone()).map_err(|_| EngineError::InvalidRecord)?;
        }
        Ok(())
    }
}

/// Opaque engine record. Contains secrets and intentionally lacks Debug.
pub struct Record {
    key: RecordKey,
    value: Vec<u8>,
}
impl Record {
    pub fn from_parts(key: RecordKey, value: Vec<u8>) -> Result<Self, EngineError> {
        key.validate()?;
        if value.is_empty() || value.len() > MAX_SNAPSHOT_BYTES {
            return Err(EngineError::LimitExceeded);
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&value).map_err(|_| EngineError::InvalidRecord)?;
        check_json(&parsed, 0)?;
        Ok(Self { key, value })
    }
    pub fn key(&self) -> &RecordKey {
        &self.key
    }
    pub fn value(&self) -> &[u8] {
        &self.value
    }
    pub fn into_parts(self) -> (RecordKey, Vec<u8>) {
        (self.key, self.value)
    }
    pub(crate) fn duplicate(&self) -> Self {
        Self {
            key: self.key.clone(),
            value: self.value.clone(),
        }
    }
}

fn check_json(value: &serde_json::Value, depth: usize) -> Result<(), EngineError> {
    if depth > 64 {
        return Err(EngineError::InvalidRecord);
    }
    match value {
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "__proto__" | "constructor" | "prototype") {
                    return Err(EngineError::InvalidRecord);
                }
                check_json(value, depth + 1)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                check_json(value, depth + 1)?
            }
        }
        _ => {}
    }
    Ok(())
}

/// Immutable caller-owned committed subset. No state is retained across jobs.
pub struct Snapshot {
    pub(crate) account: AccountId,
    pub(crate) revision: u64,
    pub(crate) records: Vec<Record>,
}
impl Snapshot {
    pub fn new(
        account: AccountId,
        revision: u64,
        records: Vec<Record>,
    ) -> Result<Self, EngineError> {
        if records.len() > MAX_RECORDS {
            return Err(EngineError::LimitExceeded);
        }
        let mut keys = BTreeSet::new();
        let mut size = 0usize;
        AccountId::new(account.0.clone()).map_err(|_| EngineError::InvalidRecord)?;
        for record in &records {
            record.key.validate()?;
            if !keys.insert(record.key.clone()) {
                return Err(EngineError::InvalidRecord);
            }
            size = size
                .checked_add(
                    record.value.len()
                        + serde_json::to_vec(&record.key)
                            .map_err(|_| EngineError::InvalidRecord)?
                            .len()
                        + 32,
                )
                .ok_or(EngineError::LimitExceeded)?;
        }
        if size > MAX_SNAPSHOT_BYTES {
            return Err(EngineError::LimitExceeded);
        }
        Ok(Self {
            account,
            revision,
            records,
        })
    }
    pub fn account(&self) -> &AccountId {
        &self.account
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    pub fn duplicate(&self) -> Self {
        Self {
            account: self.account.clone(),
            revision: self.revision,
            records: self.records.iter().map(Record::duplicate).collect(),
        }
    }
}

/// Explicit upstream bundle/device policy decision. AuthenticatedFirstUse means
/// authenticated first observation, not independent proof of a peer's identity.
/// The engine never derives approval from a server IID alone.
pub struct TrustDecision {
    pub peer: PeerNamespace,
    pub registration_id: u16,
    pub identity_key: [u8; 33],
    pub session_timestamp: u64,
    pub basis: TrustBasis,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum TrustBasis {
    AuthenticatedFirstUse,
    PersistedBinding,
    AuthenticatedDeviceTransition,
}

pub struct PeerBundle {
    pub registration_id: u16,
    pub identity_key: [u8; 33],
    pub signed_prekey_id: u32,
    pub signed_prekey: [u8; 33],
    pub signature: [u8; 64],
    pub prekey: Option<(u32, [u8; 33])>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PersonalMessageType {
    PreKey,
    Whisper,
}
pub struct PersonalCiphertext {
    pub message_type: PersonalMessageType,
    pub body: Vec<u8>,
}

pub enum Operation {
    CreateBundle {
        local: LocalNamespace,
        prekey_count: u8,
    },
    /// IDs must be reserved durably by the caller and never reused, even after
    /// the corresponding consumed private record has been removed.
    GeneratePreKeys {
        local: LocalNamespace,
        reserved_ids: Vec<u32>,
    },
    ProcessBundle {
        local: LocalNamespace,
        trust: TrustDecision,
        bundle: PeerBundle,
    },
    EncryptPersonal {
        local: LocalNamespace,
        trust: TrustDecision,
        plaintext: Vec<u8>,
    },
    DecryptPersonal {
        local: LocalNamespace,
        trust: TrustDecision,
        message: PersonalCiphertext,
        message_timestamp: u64,
    },
    CreateGroupDistribution {
        local: LocalNamespace,
        group: GroupId,
        sender_epoch: RegistrationEpoch,
    },
    /// Decrypt the personal envelope and install its raw distribution atomically.
    ProcessGroupDistribution {
        local: LocalNamespace,
        trust: TrustDecision,
        group: GroupId,
        message: PersonalCiphertext,
        message_timestamp: u64,
    },
    EncryptGroup {
        local: LocalNamespace,
        group: GroupId,
        sender_epoch: RegistrationEpoch,
        plaintext: Vec<u8>,
    },
    DecryptGroup {
        local: LocalNamespace,
        group: GroupId,
        trust: TrustDecision,
        ciphertext: Vec<u8>,
    },
}

pub struct Request {
    pub account: AccountId,
    pub generation: u64,
    pub operation_id: u64,
    pub expected_revision: u64,
    pub operation: Operation,
    pub snapshot: Snapshot,
}

pub enum Mutation {
    Put(Record),
    Delete(RecordKey),
}
pub struct MutationDelta {
    pub(crate) mutations: Vec<Mutation>,
}
impl MutationDelta {
    pub fn mutations(&self) -> &[Mutation] {
        &self.mutations
    }
    pub fn into_mutations(self) -> Vec<Mutation> {
        self.mutations
    }
}

pub struct PublicBundle {
    pub registration_id: u16,
    pub identity_key: [u8; 33],
    pub signed_prekey_id: u32,
    pub signed_prekey: [u8; 33],
    pub signature: [u8; 64],
    pub prekeys: Vec<(u32, [u8; 33])>,
}
pub enum Output {
    Bundle(PublicBundle),
    PreKeys {
        registration_id: u16,
        prekeys: Vec<(u32, [u8; 33])>,
    },
    SessionReady,
    PersonalEncrypted {
        message: PersonalCiphertext,
        remote_registration_id: u16,
    },
    Plaintext(Vec<u8>),
    GroupDistribution(Vec<u8>),
    GroupReady,
    GroupEncrypted(Vec<u8>),
}

pub struct OperationResult {
    pub(crate) account: AccountId,
    pub(crate) generation: u64,
    pub(crate) operation_id: u64,
    pub(crate) expected_revision: u64,
    pub(crate) output: Output,
    pub(crate) delta: MutationDelta,
}
impl OperationResult {
    pub fn account(&self) -> &AccountId {
        &self.account
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }
    pub fn output(&self) -> &Output {
        &self.output
    }
    pub fn delta(&self) -> &MutationDelta {
        &self.delta
    }
    /// Consume the sealed engine result at the trusted transaction boundary.
    pub fn into_parts(self) -> (AccountId, u64, u64, u64, Output, MutationDelta) {
        (
            self.account,
            self.generation,
            self.operation_id,
            self.expected_revision,
            self.output,
            self.delta,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineError {
    InvalidInput,
    InvalidRecord,
    LimitExceeded,
    WrongAccount,
    StaleGeneration,
    StaleRevision,
    StaleOperation,
    QueueFull,
    Stopped,
    Cancelled,
    Deadline,
    RuntimeFailure,
    TrustRejected,
    IdentityChanged,
    MissingRecord,
    AlreadyProvisioned,
    InvalidCiphertext,
    Replay,
    StorageBoundary,
}

pub struct EngineOptions {
    pub operation_timeout: Duration,
}
impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            operation_timeout: Duration::from_secs(5),
        }
    }
}
