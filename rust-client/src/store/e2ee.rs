//! Durable E2EE state; called only by the existing MessageStore database owner.
//!
//! This module does not establish trust or perform cryptography/network I/O.
//! Verified identity decisions must come from the protocol/engine pipeline.
use super::{conversation_row, insert_validated, MessageStore, CONVERSATION_SQL};
use crate::domain::{
    validate_id, Conversation, Message, MAX_PAGE_SIZE, MAX_PREVIEW_CHARS, MAX_TEXT_BYTES,
};
use anyhow::{bail, ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(feature = "signal-store")]
pub mod engine_adapter;
#[cfg(feature = "signal-crypto")]
pub mod provisioning;

pub const MAX_RECORDS: usize = 512;
pub const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CIPHERTEXT_BYTES: usize = 128 * 1024;
pub const MAX_DISPATCH_PAYLOAD: usize = 1024 * 1024;
pub const MAX_DISPATCH_PEERS: usize = 64;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_PENDING_SENDS: i64 = 100;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Platform {
    Mobile,
    Web,
    Pc,
}
impl Platform {
    fn wire(self) -> u8 {
        match self {
            Self::Mobile => 0,
            Self::Web => 1,
            Self::Pc => 2,
        }
    }
}

/// Durable allocation, separate from transport authentication generations.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalNamespace {
    #[serde(deserialize_with = "deserialize_epoch")]
    pub epoch: String,
    pub platform: Platform,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerNamespace {
    #[serde(deserialize_with = "deserialize_account")]
    pub uid: String,
    pub platform: Platform,
    #[serde(deserialize_with = "deserialize_epoch")]
    pub epoch: String,
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
        #[serde(deserialize_with = "deserialize_account")]
        group: String,
        sender: PeerNamespace,
    },
}
impl RecordKey {
    fn local(&self) -> &LocalNamespace {
        match self {
            Self::LocalIdentity { local }
            | Self::PreKey { local, .. }
            | Self::SignedPreKey { local, .. }
            | Self::Session { local, .. }
            | Self::PeerIdentity { local, .. }
            | Self::SessionTimestamp { local, .. }
            | Self::GroupSender { local, .. } => local,
        }
    }
    fn peer(&self) -> Option<&PeerNamespace> {
        match self {
            Self::Session { peer, .. }
            | Self::PeerIdentity { peer, .. }
            | Self::SessionTimestamp { peer, .. } => Some(peer),
            Self::GroupSender { sender, .. } => Some(sender),
            _ => None,
        }
    }
    fn encoded(&self) -> Result<String> {
        epoch(&self.local().epoch)?;
        if let Some(peer) = self.peer() {
            account(&peer.uid)?;
            epoch(&peer.epoch)?;
        }
        match self {
            Self::PreKey { id, .. } | Self::SignedPreKey { id, .. } => {
                ensure!(*id > 0, "Invalid crypto record address")
            }
            Self::GroupSender { group, .. } => account(group)?,
            _ => {}
        }
        Ok(serde_json::to_string(self)?)
    }
}

/// Secret bytes intentionally have no Debug or Serialize implementation.
pub struct Record {
    pub key: RecordKey,
    pub value: Vec<u8>,
}
pub enum Mutation {
    Put(Record),
    Delete(RecordKey),
}
pub struct Snapshot {
    pub account: String,
    pub revision: u64,
    pub records: Vec<Record>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerActivation {
    pub local: LocalNamespace,
    pub peer: PeerNamespace,
    pub expected_previous: Option<u64>,
}
pub struct Delta {
    pub account: String,
    pub expected_revision: u64,
    pub generation: u64,
    pub operation_id: u64,
    pub input_digest: [u8; 32],
    /// Exact keys supplied in the committed engine input snapshot; no arbitrary writes.
    pub allowed_keys: Vec<RecordKey>,
    pub activate_peers: Vec<PeerActivation>,
    pub mutations: Vec<Mutation>,
}
/// The caller has already verified the signed device/bundle decision. IID is not identity.
pub struct VerifiedBinding {
    pub platform: Platform,
    pub registration_id: u16,
    pub identity_key: [u8; 33],
    pub session_timestamp: u64,
}
impl VerifiedBinding {
    fn fingerprint(&self) -> Result<Vec<u8>> {
        ensure!(
            (0..=16383).contains(&self.registration_id)
                && self.identity_key[0] == 5
                && self.identity_key[1..].iter().any(|b| *b != 0)
                && self.session_timestamp > 0
                && self.session_timestamp <= MAX_SAFE_INTEGER,
            "Invalid verified identity binding"
        );
        Ok(Sha256::digest(self.identity_key).to_vec())
    }
}

/// UI cancellation is one nonblocking atomic store. The final CAS immediately
/// before COMMIT defines the race: cancellation before it rolls back; after it
/// may commit, but a stale-generation receipt cannot authorize ACK/UI publication.
pub struct GenerationGate {
    current: AtomicU64,
}
const COMMIT_MARK: u64 = 1 << 63;
const CANCEL_MARK: u64 = 1 << 62;
const GENERATION_MASK: u64 = CANCEL_MARK - 1;
impl GenerationGate {
    pub fn new(generation: u64) -> Result<Self> {
        ensure!(
            generation > 0 && generation < CANCEL_MARK,
            "Invalid generation"
        );
        Ok(Self {
            current: AtomicU64::new(generation),
        })
    }
    pub fn replace(&self, generation: u64) -> Result<()> {
        ensure!(
            generation > 0 && generation < CANCEL_MARK,
            "Invalid generation"
        );
        let mut previous = self.current.load(Ordering::Acquire);
        loop {
            ensure!(
                generation > (previous & GENERATION_MASK),
                "Generation must increase"
            );
            match self.current.compare_exchange_weak(
                previous,
                generation,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => previous = actual,
            }
        }
    }
    pub fn cancel(&self) {
        self.current.fetch_or(CANCEL_MARK, Ordering::AcqRel);
    }
    pub fn is_current(&self, generation: u64) -> bool {
        self.current.load(Ordering::Acquire) == generation
    }
    fn check(&self, generation: u64) -> Result<()> {
        ensure!(
            self.is_current(generation),
            "Cancelled or stale crypto generation"
        );
        Ok(())
    }
    fn linearize(&self, generation: u64) -> Result<CommitLease<'_>> {
        self.current
            .compare_exchange(
                generation,
                generation | COMMIT_MARK,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| anyhow::anyhow!("Cancelled or stale crypto generation"))?;
        Ok(CommitLease {
            gate: self,
            generation,
        })
    }
}
struct CommitLease<'a> {
    gate: &'a GenerationGate,
    generation: u64,
}
impl Drop for CommitLease<'_> {
    fn drop(&mut self) {
        let _ = self.gate.current.compare_exchange(
            self.generation | COMMIT_MARK,
            self.generation,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Personal,
    Group,
}
/// Authenticated delivery identity; queue is deliberately not part of it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputContext {
    pub kind: TargetKind,
    pub thread: String,
    pub server_id: String,
    pub sender: PeerNamespace,
    pub registration_id: u16,
}
impl InputContext {
    fn encoded(&self) -> Result<String> {
        validate_id(&self.thread)?;
        validate_id(&self.server_id)?;
        ensure!(
            self.registration_id <= 16383,
            "Invalid input registration ID"
        );
        account(&self.sender.uid)?;
        epoch(&self.sender.epoch)?;
        ensure!(
            match self.kind {
                TargetKind::Personal => self.thread.starts_with("d:"),
                TargetKind::Group => self.thread.starts_with("g:"),
            },
            "Input target kind mismatch"
        );
        Ok(serde_json::to_string(self)?)
    }
}
pub struct Receive<'a> {
    pub delta: &'a Delta,
    pub messages: &'a [Message],
    pub queue: &'a str,
    pub cursor: u64,
    pub context: InputContext,
    pub ciphertext_digest: [u8; 32],
}
pub struct EncryptedInput<'a> {
    pub queue: &'a str,
    pub context: &'a InputContext,
    pub ciphertext_digest: &'a [u8; 32],
}
/// Durable logical work identity, separate from a transient engine job sequence.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationIdentity<'a> {
    Receive { context: &'a InputContext },
    Send { client_id: u64 },
}
pub struct ReplayDelivery<'a> {
    pub operation_id: u64,
    pub input: EncryptedInput<'a>,
    pub cursor: u64,
    pub generation: u64,
}
/// Wire-visible identity used before resolving today's peer epoch. A committed
/// duplicate must recover its original verified epoch, not a replacement device.
pub struct InputLocator<'a> {
    pub kind: TargetKind,
    pub thread: &'a str,
    pub server_id: &'a str,
    pub sender_uid: &'a str,
    pub sender_platform: Platform,
    pub registration_id: u16,
}
/// Constructed only after SQL COMMIT. Fields are private; it cannot be forged or
/// deserialized by protocol callers. Only this receipt may authorize publication/ACK.
pub struct CommitReceipt {
    account: String,
    operation_id: u64,
    revision: u64,
    replay: bool,
    generation: u64,
    cursor: Option<(String, u64)>,
    published: Vec<(Message, Conversation)>,
}
impl CommitReceipt {
    pub fn can_publish(&self, gate: &GenerationGate) -> bool {
        gate.is_current(self.generation)
    }
    pub fn account(&self) -> &str {
        &self.account
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
    pub fn acknowledged_cursor(&self) -> Option<(&str, u64)> {
        self.cursor.as_ref().map(|(q, c)| (q.as_str(), *c))
    }
    pub fn published(&self) -> &[(Message, Conversation)] {
        &self.published
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SendState {
    Reserved,
    Ready,
    Sending,
    Unknown,
    Acknowledged,
    Committed,
}
impl SendState {
    fn text(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Ready => "ready",
            Self::Sending => "sending",
            Self::Unknown => "unknown",
            Self::Acknowledged => "acknowledged",
            Self::Committed => "committed",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "reserved" => Self::Reserved,
            "ready" => Self::Ready,
            "sending" => Self::Sending,
            "unknown" => Self::Unknown,
            "acknowledged" => Self::Acknowledged,
            "committed" => Self::Committed,
            _ => bail!("Invalid encrypted outbox state"),
        })
    }
}
/// Secret plaintext/ciphertext intentionally omit Debug and Serialize.
pub struct Outgoing {
    pub client_id: u64,
    pub thread: String,
    pub local: LocalNamespace,
    pub state: SendState,
    pub ciphertext: Option<Vec<u8>>,
    pub metadata: Option<Vec<u8>>,
    pub server_id: Option<String>,
}
/// Non-cloneable dispatch authority created only by READY→SENDING COMMIT.
/// Protocol builders must accept this token, never publicly constructible Outgoing.
pub struct DispatchReceipt {
    account: String,
    generation: u64,
    operation_id: u64,
    revision: u64,
    client_id: u64,
    thread: String,
    local: LocalNamespace,
    peers: Vec<PeerNamespace>,
    ciphertext: Vec<u8>,
    metadata: Vec<u8>,
    payload_hash: [u8; 32],
    verified_codec: Option<u32>,
}
impl DispatchReceipt {
    pub fn can_dispatch(&self, gate: &GenerationGate) -> bool {
        gate.is_current(self.generation)
    }
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn operation_id(&self) -> u64 {
        self.operation_id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn client_id(&self) -> u64 {
        self.client_id
    }
    pub fn thread(&self) -> &str {
        &self.thread
    }
    pub fn local(&self) -> &LocalNamespace {
        &self.local
    }
    pub fn peers(&self) -> &[PeerNamespace] {
        &self.peers
    }
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }
    pub fn verified_codec(&self) -> Option<u32> {
        self.verified_codec
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
}
fn payload_hash(
    owner: &str,
    client: u64,
    thread: &str,
    local: &LocalNamespace,
    cipher: &[u8],
    metadata: &[u8],
    peers: &str,
) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    digest_part(&mut hash, owner.as_bytes());
    hash.update(client.to_be_bytes());
    digest_part(&mut hash, thread.as_bytes());
    digest_part(&mut hash, &serde_json::to_vec(local)?);
    digest_part(&mut hash, cipher);
    digest_part(&mut hash, metadata);
    digest_part(&mut hash, peers.as_bytes());
    Ok(hash.finalize().into())
}
pub struct PreparedSend<'a> {
    pub delta: &'a Delta,
    pub client_id: u64,
    pub ciphertext: &'a [u8],
    pub metadata: &'a [u8],
}

fn account(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 32
            && value.bytes().all(|b| b.is_ascii_digit())
            && !value.starts_with('0')
            && value.bytes().any(|b| b != b'0'),
        "Invalid crypto account"
    );
    Ok(())
}
fn deserialize_epoch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    epoch(&value).map_err(|_| serde::de::Error::custom("Invalid registration epoch"))?;
    Ok(value)
}
fn deserialize_account<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    account(&value).map_err(|_| serde::de::Error::custom("Invalid crypto account"))?;
    Ok(value)
}
fn epoch(value: &str) -> Result<i64> {
    let n = value.parse::<i64>()?;
    ensure!(
        n > 0 && n.to_string() == value,
        "Invalid registration epoch"
    );
    Ok(n)
}
fn integer(value: u64) -> Result<i64> {
    ensure!(
        value > 0 && value <= MAX_SAFE_INTEGER,
        "Invalid durable operation identifier"
    );
    Ok(value as i64)
}
fn digest_part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

/// Stable input binds account, canonical server-message identity and encrypted
/// bytes. Queue is validated but excluded: online/offline are delivery routes,
/// never different cryptographic operations. Canonical server identity must
/// include its thread/sender scope when server message IDs are not global.
pub fn receive_input_digest(
    owner: &str,
    queue: &str,
    context: &InputContext,
    ciphertext_digest: &[u8; 32],
) -> Result<[u8; 32]> {
    account(owner)?;
    validate_id(queue)?;
    let identity = context.encoded()?;
    let mut hash = Sha256::new();
    digest_part(&mut hash, owner.as_bytes());
    digest_part(&mut hash, identity.as_bytes());
    hash.update(ciphertext_digest);
    Ok(hash.finalize().into())
}
/// Included unconditionally in the existing schema transaction; defaults can read
/// databases written by optional E2EE builds. Only additive tables, no history reset.
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS e2ee_accounts(account TEXT PRIMARY KEY,revision INTEGER NOT NULL DEFAULT 0,next_local INTEGER NOT NULL DEFAULT 0,next_peer INTEGER NOT NULL DEFAULT 0,current_local INTEGER);
    CREATE TABLE IF NOT EXISTS e2ee_local_epochs(account TEXT NOT NULL,epoch INTEGER NOT NULL,platform INTEGER NOT NULL,registration_id INTEGER NOT NULL,fingerprint BLOB NOT NULL,session_ts INTEGER NOT NULL,next_prekey INTEGER NOT NULL,PRIMARY KEY(account,epoch));
    CREATE TABLE IF NOT EXISTS e2ee_peer_epochs(account TEXT NOT NULL,local_epoch INTEGER NOT NULL,peer_epoch INTEGER NOT NULL,uid TEXT NOT NULL,platform INTEGER NOT NULL,registration_id INTEGER NOT NULL,fingerprint BLOB NOT NULL,session_ts INTEGER NOT NULL,active INTEGER NOT NULL,PRIMARY KEY(account,local_epoch,peer_epoch));
    CREATE UNIQUE INDEX IF NOT EXISTS e2ee_active_peer ON e2ee_peer_epochs(account,local_epoch,uid,platform) WHERE active=1;
    CREATE TABLE IF NOT EXISTS e2ee_records(account TEXT NOT NULL,key TEXT NOT NULL,value BLOB NOT NULL,PRIMARY KEY(account,key));
    CREATE TABLE IF NOT EXISTS e2ee_inputs(account TEXT NOT NULL,queue TEXT NOT NULL,server_id TEXT NOT NULL,cipher_digest BLOB NOT NULL,operation_id INTEGER NOT NULL,PRIMARY KEY(account,queue,server_id),UNIQUE(account,cipher_digest));
    CREATE INDEX IF NOT EXISTS e2ee_input_identity ON e2ee_inputs(account,server_id);
    CREATE TABLE IF NOT EXISTS e2ee_deliveries(account TEXT NOT NULL,queue TEXT NOT NULL,server_id TEXT NOT NULL,operation_id INTEGER NOT NULL,cursor INTEGER NOT NULL,PRIMARY KEY(account,queue,server_id));
    CREATE TABLE IF NOT EXISTS e2ee_operation_counters(account TEXT PRIMARY KEY,last_id INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS e2ee_operation_reservations(account TEXT NOT NULL,stable_key TEXT NOT NULL,operation_id INTEGER NOT NULL,input_digest BLOB NOT NULL,PRIMARY KEY(account,stable_key),UNIQUE(account,operation_id));
    CREATE TABLE IF NOT EXISTS e2ee_operations(account TEXT NOT NULL,operation_id INTEGER NOT NULL,digest BLOB NOT NULL,input_digest BLOB NOT NULL,revision INTEGER NOT NULL,queue TEXT,cursor INTEGER,PRIMARY KEY(account,operation_id));
    CREATE TABLE IF NOT EXISTS e2ee_outbox(account TEXT NOT NULL,client_id INTEGER NOT NULL,thread TEXT NOT NULL,local_epoch INTEGER NOT NULL,platform INTEGER NOT NULL,body TEXT NOT NULL,created_ms INTEGER NOT NULL,state TEXT NOT NULL,ciphertext BLOB,metadata BLOB,server_id TEXT,operation_id INTEGER,crypto_revision INTEGER,peer_bindings TEXT,payload_hash BLOB,PRIMARY KEY(account,client_id));
    CREATE TABLE IF NOT EXISTS e2ee_verified_outbox(account TEXT NOT NULL,client_id INTEGER NOT NULL,codec INTEGER NOT NULL,operation_id INTEGER NOT NULL,crypto_revision INTEGER NOT NULL,payload_hash BLOB NOT NULL,PRIMARY KEY(account,client_id));
    CREATE TABLE IF NOT EXISTS e2ee_local_candidates(account TEXT NOT NULL,epoch INTEGER NOT NULL,platform INTEGER NOT NULL,intent TEXT NOT NULL,operation_id INTEGER NOT NULL,expected_current INTEGER,prekey_count INTEGER NOT NULL,state TEXT NOT NULL,base_revision INTEGER NOT NULL,plan_digest BLOB NOT NULL,stage_digest BLOB,registration_id INTEGER,PRIMARY KEY(account,epoch),UNIQUE(account,intent),UNIQUE(account,operation_id));
    CREATE TABLE IF NOT EXISTS e2ee_pending_records(account TEXT NOT NULL,epoch INTEGER NOT NULL,key TEXT NOT NULL,value BLOB NOT NULL,PRIMARY KEY(account,epoch,key));
    CREATE TABLE IF NOT EXISTS e2ee_public_outbox(account TEXT NOT NULL,operation_id INTEGER NOT NULL,epoch INTEGER NOT NULL,codec INTEGER NOT NULL,command INTEGER NOT NULL,payload BLOB NOT NULL,payload_hash BLOB NOT NULL,bundle_hash BLOB NOT NULL,state TEXT NOT NULL,attempt_counter INTEGER NOT NULL DEFAULT 0,attempt_generation INTEGER,socket_epoch INTEGER,request_id TEXT,attempt_hash BLOB,PRIMARY KEY(account,operation_id));
    CREATE UNIQUE INDEX IF NOT EXISTS e2ee_public_request_id ON e2ee_public_outbox(account,request_id) WHERE request_id IS NOT NULL;
    CREATE UNIQUE INDEX IF NOT EXISTS e2ee_server_message ON e2ee_outbox(account,thread,server_id) WHERE server_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS e2ee_outbox_state ON e2ee_outbox(account,state,client_id);")?;
    Ok(())
}

fn ensure_owner(conn: &Connection, owner: &str) -> Result<()> {
    account(owner)?;
    conn.execute(
        "INSERT INTO e2ee_accounts(account) VALUES(?1) ON CONFLICT DO NOTHING",
        [owner],
    )?;
    Ok(())
}
fn revision(conn: &Connection, owner: &str) -> Result<u64> {
    Ok(conn
        .query_row(
            "SELECT revision FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0) as u64)
}
fn validate_address(
    conn: &Connection,
    owner: &str,
    key: &RecordKey,
    active: bool,
) -> Result<String> {
    let encoded = key.encoded()?;
    let local = key.local();
    ensure!(conn.query_row("SELECT EXISTS(SELECT 1 FROM e2ee_local_epochs WHERE account=?1 AND epoch=?2 AND platform=?3)",params![owner,epoch(&local.epoch)?,local.platform.wire()],|r|r.get::<_,bool>(0))?,"Unknown local crypto namespace");
    if active {
        ensure!(
            conn.query_row(
                "SELECT current_local=?2 FROM e2ee_accounts WHERE account=?1",
                params![owner, epoch(&local.epoch)?],
                |r| r.get::<_, bool>(0)
            )?,
            "Retired local crypto namespace"
        );
    }
    if let Some(peer) = key.peer() {
        if matches!(key, RecordKey::GroupSender { .. }) && peer.uid == owner {
            ensure!(
                peer.platform == local.platform && peer.epoch == local.epoch,
                "Own group sender must match the local registration epoch"
            );
            return Ok(encoded);
        }
        ensure!(conn.query_row("SELECT EXISTS(SELECT 1 FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND peer_epoch=?3 AND uid=?4 AND platform=?5 AND (?6=0 OR active=1))",params![owner,epoch(&local.epoch)?,epoch(&peer.epoch)?,peer.uid,peer.platform.wire(),i64::from(active)],|r|r.get::<_,bool>(0))?,"Unknown or retired peer crypto namespace");
    }
    Ok(encoded)
}
fn delta_digest(conn: &Connection, delta: &Delta) -> Result<Vec<u8>> {
    account(&delta.account)?;
    integer(delta.operation_id)?;
    integer(delta.generation)?;
    let reserved:Option<Vec<u8>>=conn.query_row("SELECT input_digest FROM e2ee_operation_reservations WHERE account=?1 AND operation_id=?2",params![delta.account,integer(delta.operation_id)?],|r|r.get(0)).optional()?;
    ensure!(
        reserved.is_none_or(|digest| digest == delta.input_digest),
        "Crypto delta differs from its reserved input"
    );
    ensure!(
        delta.expected_revision < i64::MAX as u64
            && delta.allowed_keys.len() <= MAX_RECORDS
            && delta.mutations.len() <= MAX_RECORDS
            && delta.activate_peers.len() <= MAX_DISPATCH_PEERS,
        "Crypto delta exceeds budget"
    );
    let mut allowed = BTreeSet::new();
    let mut total = 0usize;
    for key in &delta.allowed_keys {
        ensure!(
            allowed.insert(validate_address(conn, &delta.account, key, false)?),
            "Duplicate snapshot key"
        );
    }
    let mut touched = BTreeSet::new();
    let mut hash = Sha256::new();
    digest_part(&mut hash, delta.account.as_bytes());
    hash.update(delta.expected_revision.to_be_bytes());
    hash.update(delta.input_digest);
    for key in &allowed {
        digest_part(&mut hash, key.as_bytes());
    }
    let mut identities = BTreeSet::new();
    for decision in &delta.activate_peers {
        validate_address(
            conn,
            &delta.account,
            &RecordKey::PeerIdentity {
                local: decision.local.clone(),
                peer: decision.peer.clone(),
            },
            false,
        )?;
        ensure!(
            identities.insert((
                decision.local.epoch.clone(),
                decision.peer.uid.clone(),
                decision.peer.platform
            )),
            "Duplicate peer activation"
        );
        digest_part(&mut hash, &serde_json::to_vec(decision)?);
    }
    // Generation is intentionally not hashed: reconnect may replay a committed operation.
    for mutation in &delta.mutations {
        let (key, value, tag) = match mutation {
            Mutation::Put(record) => (&record.key, Some(&record.value), 1u8),
            Mutation::Delete(key) => (key, None, 0u8),
        };
        let key = validate_address(conn, &delta.account, key, false)?;
        ensure!(
            allowed.contains(&key) && touched.insert(key.clone()),
            "Delta writes outside its snapshot keyset"
        );
        total = total
            .checked_add(key.len() + 32 + value.map_or(0, |v| v.len()))
            .ok_or_else(|| anyhow::anyhow!("Crypto delta exceeds budget"))?;
        ensure!(
            total <= MAX_SNAPSHOT_BYTES && value.is_none_or(|v| !v.is_empty()),
            "Crypto delta exceeds budget"
        );
        hash.update([tag]);
        digest_part(&mut hash, key.as_bytes());
        if let Some(value) = value {
            digest_part(&mut hash, value);
        }
    }
    Ok(hash.finalize().to_vec())
}
fn apply_delta(tx: &Transaction<'_>, delta: &Delta) -> Result<u64> {
    ensure!(
        revision(tx, &delta.account)? == delta.expected_revision,
        "Stale crypto revision"
    );
    for decision in &delta.activate_peers {
        let current:Option<i64>=tx.query_row("SELECT peer_epoch FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND active=1",params![delta.account,epoch(&decision.local.epoch)?,decision.peer.uid,decision.peer.platform.wire()],|r|r.get(0)).optional()?;
        ensure!(
            current.map(|n| n as u64) == decision.expected_previous,
            "Peer identity decision conflict"
        );
        tx.execute("UPDATE e2ee_peer_epochs SET active=0 WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND active=1",params![delta.account,epoch(&decision.local.epoch)?,decision.peer.uid,decision.peer.platform.wire()])?;
        let changed=tx.execute("UPDATE e2ee_peer_epochs SET active=1 WHERE account=?1 AND local_epoch=?2 AND peer_epoch=?3 AND uid=?4 AND platform=?5 AND active=-1",params![delta.account,epoch(&decision.local.epoch)?,epoch(&decision.peer.epoch)?,decision.peer.uid,decision.peer.platform.wire()])?;
        ensure!(
            changed == 1,
            "Peer epoch was not reserved by a verified transition"
        );
    }
    for mutation in &delta.mutations {
        let key = match mutation {
            Mutation::Put(record) => &record.key,
            Mutation::Delete(key) => key,
        };
        validate_address(tx, &delta.account, key, true)?;
        match mutation {
            Mutation::Put(record) => {
                tx.execute("INSERT INTO e2ee_records(account,key,value) VALUES(?1,?2,?3) ON CONFLICT(account,key) DO UPDATE SET value=excluded.value",params![delta.account,record.key.encoded()?,record.value])?;
            }
            Mutation::Delete(key) => {
                tx.execute(
                    "DELETE FROM e2ee_records WHERE account=?1 AND key=?2",
                    params![delta.account, key.encoded()?],
                )?;
            }
        }
    }
    let next = delta
        .expected_revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Crypto revision exhausted"))?;
    let changed = tx.execute(
        "UPDATE e2ee_accounts SET revision=?3 WHERE account=?1 AND revision=?2",
        params![delta.account, delta.expected_revision as i64, next as i64],
    )?;
    ensure!(changed == 1, "Stale crypto revision");
    Ok(next)
}
type ReplayReceipt = (u64, Option<(String, u64)>);
type StoredOperation = (Vec<u8>, i64, Option<String>, Option<i64>);
fn replay(
    tx: &Transaction<'_>,
    owner: &str,
    id: u64,
    digest: &[u8],
) -> Result<Option<ReplayReceipt>> {
    let found:Option<StoredOperation>=tx.query_row("SELECT digest,revision,queue,cursor FROM e2ee_operations WHERE account=?1 AND operation_id=?2",params![owner,integer(id)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((previous, rev, queue, cursor)) = found {
        ensure!(previous == digest, "Conflicting crypto operation replay");
        Ok(Some((
            rev as u64,
            queue.zip(cursor).map(|(q, c)| (q, c as u64)),
        )))
    } else {
        Ok(None)
    }
}
fn remember(
    tx: &Transaction<'_>,
    delta: &Delta,
    digest: &[u8],
    rev: u64,
    cursor: Option<(&str, u64)>,
) -> Result<()> {
    tx.execute("INSERT INTO e2ee_operations(account,operation_id,digest,revision,queue,cursor,input_digest) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![delta.account,integer(delta.operation_id)?,digest,rev as i64,cursor.map(|(q,_)|q),cursor.map(|(_,c)|c as i64),delta.input_digest.as_slice()])?;
    Ok(())
}

fn canonical_input(
    conn: &Connection,
    owner: &str,
    input: &EncryptedInput<'_>,
) -> Result<Option<(u64, u64)>> {
    let digest = receive_input_digest(owner, input.queue, input.context, input.ciphertext_digest)?;
    let identity = input.context.encoded()?;
    let by_cipher: Option<String> = conn
        .query_row(
            "SELECT server_id FROM e2ee_inputs WHERE account=?1 AND cipher_digest=?2",
            params![owner, input.ciphertext_digest.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = by_cipher {
        ensure!(
            previous == identity,
            "Ciphertext replay has a different server identity"
        );
    }
    let row:Option<(Vec<u8>,i64,i64,Vec<u8>)>=conn.query_row("SELECT i.cipher_digest,i.operation_id,o.revision,o.input_digest FROM e2ee_inputs i JOIN e2ee_operations o ON o.account=i.account AND o.operation_id=i.operation_id WHERE i.account=?1 AND i.server_id=?2",params![owner,identity],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    row.map(|(cipher, id, rev, previous)| {
        ensure!(
            cipher == input.ciphertext_digest && previous == digest,
            "Conflicting committed encrypted input"
        );
        Ok((id as u64, rev as u64))
    })
    .transpose()
}
fn reserve_operation_id(
    conn: &Connection,
    owner: &str,
    stable_key: &str,
    input_digest: &[u8; 32],
) -> Result<u64> {
    let existing:Option<(i64,Vec<u8>)>=conn.query_row("SELECT operation_id,input_digest FROM e2ee_operation_reservations WHERE account=?1 AND stable_key=?2",params![owner,stable_key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((id, digest)) = existing {
        ensure!(digest == input_digest, "Conflicting reserved crypto input");
        return Ok(id as u64);
    }
    let last:i64=conn.query_row("SELECT MAX(n) FROM (SELECT COALESCE(MAX(operation_id),0) n FROM e2ee_operations WHERE account=?1 UNION ALL SELECT COALESCE(MAX(operation_id),0) FROM e2ee_operation_reservations WHERE account=?1 UNION ALL SELECT COALESCE(MAX(last_id),0) FROM e2ee_operation_counters WHERE account=?1)",[owner],|r|r.get(0))?;
    let next = integer(
        (last as u64)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Crypto operation IDs exhausted"))?,
    )?;
    conn.execute("INSERT INTO e2ee_operation_counters(account,last_id) VALUES(?1,?2) ON CONFLICT(account) DO UPDATE SET last_id=excluded.last_id",params![owner,next])?;
    conn.execute("INSERT INTO e2ee_operation_reservations(account,stable_key,operation_id,input_digest) VALUES(?1,?2,?3,?4)",params![owner,stable_key,next,input_digest.as_slice()])?;
    Ok(next as u64)
}
fn dispatch_peers(delta: &Delta) -> Result<Vec<PeerNamespace>> {
    let mut peers = BTreeSet::new();
    for key in &delta.allowed_keys {
        if let Some(peer) = key.peer() {
            if matches!(key,RecordKey::GroupSender{local,sender,..} if sender.uid==delta.account&&sender.platform==local.platform&&sender.epoch==local.epoch)
            {
                continue;
            }
            peers.insert(peer.clone());
            ensure!(
                peers.len() <= MAX_DISPATCH_PEERS,
                "Encrypted dispatch fanout exceeds budget"
            );
        }
    }
    Ok(peers.into_iter().collect())
}
fn advance_cursor(conn: &Connection, owner: &str, queue: &str, cursor: u64) -> Result<()> {
    let cursor = cursor.to_string();
    conn.execute("INSERT INTO receive_cursors(account_id,queue,last_id) VALUES(?1,?2,?3) ON CONFLICT(account_id,queue) DO UPDATE SET last_id=CASE WHEN length(excluded.last_id)>length(last_id) OR (length(excluded.last_id)=length(last_id) AND excluded.last_id>last_id) THEN excluded.last_id ELSE last_id END",params![owner,queue,cursor])?;
    Ok(())
}
/// A peer device's currently trusted identity binding.
pub struct ActivePeer {
    pub namespace: PeerNamespace,
    pub registration_id: u16,
    /// SHA-256 of the identity public key.
    pub fingerprint: Vec<u8>,
    pub session_timestamp: u64,
}
impl MessageStore {
    /// Check the immutable verified DB binding before trusting opaque engine
    /// identity bytes. This performs no transition or implicit first-use trust.
    pub fn e2ee_validate_local_identity(
        &self,
        owner: &str,
        local: &LocalNamespace,
        registration_id: u16,
        identity_key: &[u8; 33],
    ) -> Result<()> {
        account(owner)?;
        validate_address(
            &self.conn,
            owner,
            &RecordKey::LocalIdentity {
                local: local.clone(),
            },
            true,
        )?;
        ensure!(
            registration_id <= 16383
                && identity_key[0] == 5
                && identity_key[1..].iter().any(|byte| *byte != 0),
            "Invalid local identity binding"
        );
        let(id,fingerprint):(u16,Vec<u8>)=self.conn.query_row("SELECT registration_id,fingerprint FROM e2ee_local_epochs WHERE account=?1 AND epoch=?2 AND platform=?3",params![owner,epoch(&local.epoch)?,local.platform.wire()],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            id == registration_id && fingerprint == Sha256::digest(identity_key).as_slice(),
            "Local identity differs from its verified namespace"
        );
        Ok(())
    }
    pub fn e2ee_validate_peer_identity(
        &self,
        owner: &str,
        local: &LocalNamespace,
        peer: &PeerNamespace,
        registration_id: u16,
        identity_key: &[u8; 33],
        session_timestamp: u64,
    ) -> Result<()> {
        account(owner)?;
        validate_address(
            &self.conn,
            owner,
            &RecordKey::PeerIdentity {
                local: local.clone(),
                peer: peer.clone(),
            },
            true,
        )?;
        ensure!(
            registration_id <= 16383
                && identity_key[0] == 5
                && identity_key[1..].iter().any(|byte| *byte != 0),
            "Invalid peer identity binding"
        );
        integer(session_timestamp)?;
        let(id,fingerprint,ts):(u16,Vec<u8>,i64)=self.conn.query_row("SELECT registration_id,fingerprint,session_ts FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND peer_epoch=?3 AND uid=?4 AND platform=?5 AND active=1",params![owner,epoch(&local.epoch)?,epoch(&peer.epoch)?,peer.uid,peer.platform.wire()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        ensure!(
            id == registration_id
                && fingerprint == Sha256::digest(identity_key).as_slice()
                && ts == session_timestamp as i64,
            "Peer identity differs from its verified namespace"
        );
        Ok(())
    }
    /// Allocate only after an explicit verified local registration transition.
    /// Previous epochs/records remain available; reconnect alone never calls this.
    pub fn e2ee_register_local(
        &mut self,
        owner: &str,
        expected_previous: Option<u64>,
        binding: &VerifiedBinding,
    ) -> Result<LocalNamespace> {
        account(owner)?;
        let fingerprint = binding.fingerprint()?;
        let tx = self.conn.transaction()?;
        ensure_owner(&tx, owner)?;
        let current: Option<i64> = tx.query_row(
            "SELECT current_local FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get(0),
        )?;
        ensure!(
            current.map(|n| n as u64) == expected_previous,
            "Local identity transition conflict"
        );
        if let Some(current) = current {
            let same:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM e2ee_local_epochs WHERE account=?1 AND epoch=?2 AND platform=?3 AND registration_id=?4 AND fingerprint=?5 AND session_ts=?6)",params![owner,current,binding.platform.wire(),binding.registration_id,fingerprint,binding.session_timestamp as i64],|r|r.get(0))?;
            if same {
                return Ok(LocalNamespace {
                    epoch: current.to_string(),
                    platform: binding.platform,
                });
            }
        }
        let previous: i64 = tx.query_row(
            "SELECT next_local FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get(0),
        )?;
        let next = previous
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Local epoch exhausted"))?;
        tx.execute(
            "INSERT INTO e2ee_local_epochs VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                owner,
                next,
                binding.platform.wire(),
                binding.registration_id,
                fingerprint,
                binding.session_timestamp as i64,
                u64::from(binding.registration_id) + 2
            ],
        )?;
        tx.execute("UPDATE e2ee_accounts SET next_local=?2,current_local=?2,revision=revision+1 WHERE account=?1",params![owner,next])?;
        tx.commit()?;
        Ok(LocalNamespace {
            epoch: next.to_string(),
            platform: binding.platform,
        })
    }
    /// The active binding for one peer device in a local epoch. Read-only.
    pub fn e2ee_active_peer(
        &self,
        owner: &str,
        local: &LocalNamespace,
        uid: &str,
        platform: Platform,
    ) -> Result<Option<ActivePeer>> {
        account(owner)?;
        account(uid)?;
        let row: Option<(i64, u16, Vec<u8>, i64)> = self.conn.query_row(
            "SELECT peer_epoch,registration_id,fingerprint,session_ts FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND active=1",
            params![owner, epoch(&local.epoch)?, uid, platform.wire()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional()?;
        Ok(row.map(
            |(peer_epoch, registration_id, fingerprint, ts)| ActivePeer {
                namespace: PeerNamespace {
                    uid: uid.into(),
                    platform,
                    epoch: peer_epoch.to_string(),
                },
                registration_id,
                fingerprint,
                session_timestamp: ts as u64,
            },
        ))
    }
    /// Removes a draft that was reserved but never committed (nothing was encrypted or sent).
    pub fn e2ee_discard_reserved(&mut self, owner: &str, client_id: u64) -> Result<bool> {
        account(owner)?;
        integer(client_id)?;
        Ok(self.conn.execute(
            "DELETE FROM e2ee_outbox WHERE account=?1 AND client_id=?2 AND state='reserved'",
            params![owner, client_id as i64],
        )? == 1)
    }
    pub fn e2ee_register_peer(
        &mut self,
        owner: &str,
        local: &LocalNamespace,
        uid: &str,
        expected_previous: Option<u64>,
        binding: &VerifiedBinding,
    ) -> Result<PeerNamespace> {
        account(owner)?;
        account(uid)?;
        let fingerprint = binding.fingerprint()?;
        let tx = self.conn.transaction()?;
        validate_address(
            &tx,
            owner,
            &RecordKey::LocalIdentity {
                local: local.clone(),
            },
            true,
        )?;
        let current:Option<(i64,u16,Vec<u8>,i64)>=tx.query_row("SELECT peer_epoch,registration_id,fingerprint,session_ts FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND active=1",params![owner,epoch(&local.epoch)?,uid,binding.platform.wire()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        ensure!(
            current.as_ref().map(|r| r.0 as u64) == expected_previous,
            "Peer identity transition conflict"
        );
        if let Some((previous, reg, fp, ts)) = &current {
            if *reg == binding.registration_id
                && *fp == fingerprint
                && *ts == binding.session_timestamp as i64
            {
                return Ok(PeerNamespace {
                    uid: uid.into(),
                    platform: binding.platform,
                    epoch: previous.to_string(),
                });
            }
        }
        let previous: i64 = tx.query_row(
            "SELECT next_peer FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get(0),
        )?;
        let next = previous
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Peer epoch exhausted"))?;
        tx.execute("UPDATE e2ee_peer_epochs SET active=0 WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND active=1",params![owner,epoch(&local.epoch)?,uid,binding.platform.wire()])?;
        tx.execute(
            "INSERT INTO e2ee_peer_epochs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1)",
            params![
                owner,
                epoch(&local.epoch)?,
                next,
                uid,
                binding.platform.wire(),
                binding.registration_id,
                fingerprint,
                binding.session_timestamp as i64
            ],
        )?;
        tx.execute(
            "UPDATE e2ee_accounts SET next_peer=?2,revision=revision+1 WHERE account=?1",
            params![owner, next],
        )?;
        tx.commit()?;
        Ok(PeerNamespace {
            uid: uid.into(),
            platform: binding.platform,
            epoch: next.to_string(),
        })
    }
    /// Reserve a verified candidate without trusting/retiring either identity yet.
    /// Delta.activate_peers installs the decision in the message/ratchet transaction.
    pub fn e2ee_reserve_peer(
        &mut self,
        owner: &str,
        local: &LocalNamespace,
        uid: &str,
        binding: &VerifiedBinding,
    ) -> Result<PeerNamespace> {
        account(owner)?;
        account(uid)?;
        let fingerprint = binding.fingerprint()?;
        let tx = self.conn.transaction()?;
        validate_address(
            &tx,
            owner,
            &RecordKey::LocalIdentity {
                local: local.clone(),
            },
            true,
        )?;
        let existing:Option<i64>=tx.query_row("SELECT peer_epoch FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND uid=?3 AND platform=?4 AND registration_id=?5 AND fingerprint=?6 AND session_ts=?7 AND active=-1 LIMIT 1",params![owner,epoch(&local.epoch)?,uid,binding.platform.wire(),binding.registration_id,fingerprint,binding.session_timestamp as i64],|r|r.get(0)).optional()?;
        if let Some(existing) = existing {
            return Ok(PeerNamespace {
                uid: uid.into(),
                platform: binding.platform,
                epoch: existing.to_string(),
            });
        }
        let previous: i64 = tx.query_row(
            "SELECT next_peer FROM e2ee_accounts WHERE account=?1",
            [owner],
            |r| r.get(0),
        )?;
        let next = previous
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Peer epoch exhausted"))?;
        tx.execute(
            "INSERT INTO e2ee_peer_epochs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,-1)",
            params![
                owner,
                epoch(&local.epoch)?,
                next,
                uid,
                binding.platform.wire(),
                binding.registration_id,
                fingerprint,
                binding.session_timestamp as i64
            ],
        )?;
        tx.execute(
            "UPDATE e2ee_accounts SET next_peer=?2,revision=revision+1 WHERE account=?1",
            params![owner, next],
        )?;
        tx.commit()?;
        Ok(PeerNamespace {
            uid: uid.into(),
            platform: binding.platform,
            epoch: next.to_string(),
        })
    }
    /// Read only the engine's explicit required keyset; never load all history.
    pub fn e2ee_snapshot(&self, owner: &str, keys: &[RecordKey]) -> Result<Snapshot> {
        account(owner)?;
        ensure!(
            keys.len() <= MAX_RECORDS,
            "Crypto snapshot exceeds record budget"
        );
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        let mut bytes = 0usize;
        for key in keys {
            let encoded = validate_address(&self.conn, owner, key, false)?;
            ensure!(seen.insert(encoded.clone()), "Duplicate snapshot key");
            // Check SQLite's stored length before allocating the opaque record.
            let length: Option<i64> = self
                .conn
                .query_row(
                    "SELECT length(value) FROM e2ee_records WHERE account=?1 AND key=?2",
                    params![owner, encoded],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(length) = length {
                bytes = bytes
                    .checked_add(usize::try_from(length)? + encoded.len() + 32)
                    .ok_or_else(|| anyhow::anyhow!("Crypto snapshot exceeds byte budget"))?;
                ensure!(
                    bytes <= MAX_SNAPSHOT_BYTES,
                    "Crypto snapshot exceeds byte budget"
                );
                let value = self.conn.query_row(
                    "SELECT value FROM e2ee_records WHERE account=?1 AND key=?2",
                    params![owner, encoded],
                    |r| r.get(0),
                )?;
                records.push(Record {
                    key: key.clone(),
                    value,
                });
            }
        }
        Ok(Snapshot {
            account: owner.into(),
            revision: revision(&self.conn, owner)?,
            records,
        })
    }
    /// Consult before decrypting a duplicate input: never consume a prekey twice.
    /// A changed ciphertext/input under an existing ID is a hard conflict.
    pub fn e2ee_committed_operation(
        &self,
        owner: &str,
        operation_id: u64,
        input_digest: &[u8; 32],
        generation: u64,
        gate: &GenerationGate,
    ) -> Result<Option<CommitReceipt>> {
        account(owner)?;
        integer(operation_id)?;
        gate.check(generation)?;
        let row:Option<StoredOperation>=self.conn.query_row("SELECT input_digest,revision,queue,cursor FROM e2ee_operations WHERE account=?1 AND operation_id=?2",params![owner,operation_id as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        row.map(|(digest, revision, queue, cursor)| {
            ensure!(digest == input_digest, "Conflicting crypto input replay");
            Ok(CommitReceipt {
                account: owner.into(),
                operation_id,
                revision: revision as u64,
                replay: true,
                generation,
                cursor: queue.zip(cursor).map(|(q, c)| (q, c as u64)),
                published: Vec::new(),
            })
        })
        .transpose()
    }
    /// Has this exact ciphertext already been committed? (A redelivered push is skipped, never decrypted twice.)
    pub fn e2ee_ciphertext_seen(&self, owner: &str, ciphertext_digest: &[u8; 32]) -> Result<bool> {
        account(owner)?;
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM e2ee_inputs WHERE account=?1 AND cipher_digest=?2)",
            params![owner, ciphertext_digest.as_slice()],
            |r| r.get(0),
        )?)
    }
    pub fn e2ee_lookup_committed_input(
        &self,
        owner: &str,
        locator: InputLocator<'_>,
        ciphertext_digest: &[u8; 32],
    ) -> Result<Option<InputContext>> {
        account(owner)?;
        account(locator.sender_uid)?;
        validate_id(locator.thread)?;
        validate_id(locator.server_id)?;
        ensure!(
            locator.registration_id <= 16383,
            "Invalid input registration ID"
        );
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT server_id FROM e2ee_inputs WHERE account=?1 AND cipher_digest=?2",
                params![owner, ciphertext_digest.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|encoded| {
            let context: InputContext = serde_json::from_str(&encoded)?;
            context.encoded()?;
            ensure!(
                context.kind == locator.kind
                    && context.thread == locator.thread
                    && context.server_id == locator.server_id
                    && context.sender.uid == locator.sender_uid
                    && context.sender.platform == locator.sender_platform
                    && context.registration_id == locator.registration_id,
                "Ciphertext replay has a different authenticated input context"
            );
            Ok(context)
        })
        .transpose()
    }
    /// Read-only lookup never authorizes a new queue/cursor. Use replay_receive
    /// to durably acknowledge a duplicate arriving by a different delivery route.
    pub fn e2ee_committed_receive(
        &self,
        owner: &str,
        operation_id: u64,
        input: EncryptedInput<'_>,
        generation: u64,
        gate: &GenerationGate,
    ) -> Result<Option<CommitReceipt>> {
        gate.check(generation)?;
        let Some((id, rev)) = canonical_input(&self.conn, owner, &input)? else {
            return Ok(None);
        };
        ensure!(id == operation_id, "Conflicting durable operation identity");
        let cursor:Option<i64>=self.conn.query_row("SELECT cursor FROM e2ee_deliveries WHERE account=?1 AND queue=?2 AND server_id=?3 AND operation_id=?4",params![owner,input.queue,input.context.encoded()?,integer(id)?],|r|r.get(0)).optional()?;
        Ok(cursor.map(|cursor| CommitReceipt {
            account: owner.into(),
            operation_id: id,
            revision: rev,
            replay: true,
            generation,
            cursor: Some((input.queue.into(), cursor as u64)),
            published: vec![],
        }))
    }
    /// Reuse committed plaintext/ratchets, committing only the new queue cursor.
    /// No engine overlay is needed and nothing is republished.
    pub fn e2ee_replay_receive(
        &mut self,
        owner: &str,
        delivery: ReplayDelivery<'_>,
        gate: &GenerationGate,
    ) -> Result<Option<CommitReceipt>> {
        gate.check(delivery.generation)?;
        integer(delivery.cursor)?;
        let tx = self.conn.transaction()?;
        let Some((id, rev)) = canonical_input(&tx, owner, &delivery.input)? else {
            tx.rollback()?;
            return Ok(None);
        };
        ensure!(
            id == delivery.operation_id,
            "Conflicting durable operation identity"
        );
        tx.execute("INSERT INTO e2ee_deliveries(account,queue,server_id,operation_id,cursor) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account,queue,server_id) DO UPDATE SET cursor=MAX(cursor,excluded.cursor) WHERE operation_id=excluded.operation_id",params![owner,delivery.input.queue,delivery.input.context.encoded()?,integer(id)?,integer(delivery.cursor)?])?;
        advance_cursor(&tx, owner, delivery.input.queue, delivery.cursor)?;
        let lease = gate.linearize(delivery.generation)?;
        tx.commit()?;
        drop(lease);
        Ok(Some(CommitReceipt {
            account: owner.into(),
            operation_id: id,
            revision: rev,
            replay: true,
            generation: delivery.generation,
            cursor: Some((delivery.input.queue.into(), delivery.cursor)),
            published: vec![],
        }))
    }
    /// Stable logical operation IDs survive retries/restart; engine jobs have
    /// their own monotonic, ephemeral sequence and must not replace this ID.
    pub fn e2ee_reserve_operation(
        &mut self,
        owner: &str,
        identity: OperationIdentity<'_>,
        input_digest: &[u8; 32],
    ) -> Result<u64> {
        account(owner)?;
        match &identity {
            OperationIdentity::Receive { context } => {
                context.encoded()?;
            }
            OperationIdentity::Send { client_id } => {
                integer(*client_id)?;
                let _ = self.e2ee_reserved_plaintext(owner, *client_id)?;
            }
        }
        let key = serde_json::to_string(&identity)?;
        let tx = self.conn.transaction()?;
        ensure_owner(&tx, owner)?;
        let id = reserve_operation_id(&tx, owner, &key, input_digest)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn e2ee_reserved_plaintext(&self, owner: &str, client_id: u64) -> Result<String> {
        account(owner)?;
        integer(client_id)?;
        Ok(self.conn.query_row("SELECT body FROM e2ee_outbox WHERE account=?1 AND client_id=?2 AND state='reserved' AND length(CAST(body AS BLOB))<=?3",params![owner,client_id as i64,MAX_TEXT_BYTES as i64],|r|r.get(0))?)
    }
    /// Monotonic allocation survives prekey consumption/deletion and crashes.
    pub fn e2ee_reserve_prekey_ids(
        &mut self,
        owner: &str,
        local: &LocalNamespace,
        count: u32,
    ) -> Result<Vec<u32>> {
        account(owner)?;
        ensure!(
            (1..=100).contains(&count),
            "Invalid prekey allocation count"
        );
        let tx = self.conn.transaction()?;
        validate_address(
            &tx,
            owner,
            &RecordKey::LocalIdentity {
                local: local.clone(),
            },
            true,
        )?;
        let previous: u32 = tx.query_row(
            "SELECT next_prekey FROM e2ee_local_epochs WHERE account=?1 AND epoch=?2",
            params![owner, epoch(&local.epoch)?],
            |r| r.get(0),
        )?;
        let last = previous
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("Prekey ID exhausted"))?;
        tx.execute(
            "UPDATE e2ee_local_epochs SET next_prekey=?3 WHERE account=?1 AND epoch=?2",
            params![owner, epoch(&local.epoch)?, last],
        )?;
        tx.execute(
            "UPDATE e2ee_accounts SET revision=revision+1 WHERE account=?1",
            [owner],
        )?;
        tx.commit()?;
        Ok((previous + 1..=last).collect())
    }
    pub fn e2ee_commit_receive(
        &mut self,
        receive: Receive<'_>,
        gate: &GenerationGate,
    ) -> Result<CommitReceipt> {
        self.commit_receive_inner(receive, gate, || {}, || {})
    }
    fn commit_receive_inner(
        &mut self,
        receive: Receive<'_>,
        gate: &GenerationGate,
        before: impl FnOnce(),
        after: impl FnOnce(),
    ) -> Result<CommitReceipt> {
        let delta = receive.delta;
        gate.check(delta.generation)?;
        validate_id(receive.queue)?;
        integer(receive.cursor)?;
        ensure!(
            delta.input_digest
                == receive_input_digest(
                    &delta.account,
                    receive.queue,
                    &receive.context,
                    &receive.ciphertext_digest
                )?,
            "Receive operation input identity mismatch"
        );
        ensure!(
            receive.messages.len() <= MAX_PAGE_SIZE,
            "Receive batch exceeds budget"
        );
        let mut digest = Sha256::new();
        digest_part(&mut digest, &delta_digest(&self.conn, delta)?);
        digest_part(&mut digest, b"receive");
        digest_part(&mut digest, receive.queue.as_bytes());
        digest.update(receive.cursor.to_be_bytes());
        let mut message_bytes = 0usize;
        for message in receive.messages {
            message.validate()?;
            ensure!(
                message.account_id == delta.account && message.thread_id == receive.context.thread,
                "Crypto receive account mismatch"
            );
            message_bytes += message.text.len();
            ensure!(
                message_bytes <= MAX_SNAPSHOT_BYTES,
                "Receive plaintext exceeds budget"
            );
            digest_part(&mut digest, &serde_json::to_vec(message)?);
        }
        let digest = digest.finalize().to_vec();
        let tx = self.conn.transaction()?;
        if let Some((revision, cursor)) = replay(&tx, &delta.account, delta.operation_id, &digest)?
        {
            tx.rollback()?;
            return Ok(CommitReceipt {
                account: delta.account.clone(),
                operation_id: delta.operation_id,
                revision,
                replay: true,
                generation: delta.generation,
                cursor,
                published: vec![],
            });
        }
        let identity = receive.context.encoded()?;
        let existing_identity: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM e2ee_inputs WHERE account=?1 AND server_id=?2)",
            params![delta.account, identity],
            |r| r.get(0),
        )?;
        ensure!(
            !existing_identity,
            "Server message identity is already committed; use replay receipt"
        );
        tx.execute("INSERT INTO e2ee_inputs(account,queue,server_id,cipher_digest,operation_id) VALUES(?1,?2,?3,?4,?5)",params![delta.account,receive.queue,identity,receive.ciphertext_digest.as_slice(),integer(delta.operation_id)?])?;
        let next = apply_delta(&tx, delta)?;
        let mut published = Vec::new();
        for message in receive.messages {
            if insert_validated(&tx, std::slice::from_ref(message))? > 0 {
                let conversation = tx.query_row(
                    CONVERSATION_SQL,
                    params![delta.account, message.thread_id, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )?;
                published.push((message.clone(), conversation));
            }
        }
        advance_cursor(&tx, &delta.account, receive.queue, receive.cursor)?;
        tx.execute("INSERT INTO e2ee_deliveries(account,queue,server_id,operation_id,cursor) VALUES(?1,?2,?3,?4,?5)",params![delta.account,receive.queue,identity,integer(delta.operation_id)?,integer(receive.cursor)?])?;
        remember(
            &tx,
            delta,
            &digest,
            next,
            Some((receive.queue, receive.cursor)),
        )?;
        before();
        let lease = gate.linearize(delta.generation)?;
        after();
        tx.commit()?;
        drop(lease);
        Ok(CommitReceipt {
            account: delta.account.clone(),
            operation_id: delta.operation_id,
            revision: next,
            replay: false,
            generation: delta.generation,
            cursor: Some((receive.queue.into(), receive.cursor)),
            published,
        })
    }
    /// Stable client ID is reserved durably before invoking the engine. This is
    /// known-unsent preparation, not permission to dispatch plaintext/ciphertext.
    pub fn e2ee_reserve_send(
        &mut self,
        owner: &str,
        local: &LocalNamespace,
        thread: &str,
        text: &str,
        now_ms: i64,
    ) -> Result<u64> {
        account(owner)?;
        validate_id(thread)?;
        ensure!(
            !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES && now_ms > 0,
            "Invalid encrypted draft"
        );
        let tx = self.conn.transaction()?;
        validate_address(
            &tx,
            owner,
            &RecordKey::LocalIdentity {
                local: local.clone(),
            },
            true,
        )?;
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM e2ee_outbox WHERE account=?1 AND state<>'committed'",
            [owner],
            |r| r.get(0),
        )?;
        ensure!(
            pending < MAX_PENDING_SENDS,
            "Encrypted outbox exceeds pending budget"
        );
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM e2ee_outbox WHERE account=?1 AND thread=?2 AND body=?3 AND state<>'committed')",params![owner,thread,text],|r|r.get(0))?;
        ensure!(!duplicate, "This encrypted draft has an unresolved send");
        let previous: Option<i64> = tx
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [owner],
                |r| r.get(0),
            )
            .optional()?;
        let next = now_ms.max(
            previous
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Client ID exhausted"))?,
        );
        integer(next as u64)?;
        tx.execute("INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",params![owner,next])?;
        tx.execute("INSERT INTO e2ee_outbox(account,client_id,thread,local_epoch,platform,body,created_ms,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'reserved')",params![owner,next,thread,epoch(&local.epoch)?,local.platform.wire(),text,now_ms])?;
        tx.commit()?;
        Ok(next as u64)
    }
    pub fn e2ee_commit_send(
        &mut self,
        send: PreparedSend<'_>,
        gate: &GenerationGate,
    ) -> Result<CommitReceipt> {
        self.commit_send_inner(send, gate, None, || Ok(()))
    }
    #[cfg(feature = "signal-crypto")]
    pub fn e2ee_commit_verified_send(
        &mut self,
        send: &crate::app::live::signal_crypto::VerifiedPreparedSend,
        gate: &GenerationGate,
    ) -> Result<CommitReceipt> {
        send.check_current(gate)?;
        let peers = dispatch_peers(send.delta())?;
        ensure!(
            peers == send.peers(),
            "Verified dispatch peer scope mismatch"
        );
        ensure!(
            send.delta()
                .allowed_keys
                .iter()
                .all(|key| key.local() == send.local()),
            "Verified dispatch local scope mismatch"
        );
        self.commit_send_inner(
            PreparedSend {
                delta: send.delta(),
                client_id: send.client_id(),
                ciphertext: send.exact_sms(),
                metadata: send.metadata(),
            },
            gate,
            Some(1),
            || send.check_preparation(),
        )
    }
    fn commit_send_inner(
        &mut self,
        send: PreparedSend<'_>,
        gate: &GenerationGate,
        verified_codec: Option<u32>,
        check_preparation: impl Fn() -> Result<()>,
    ) -> Result<CommitReceipt> {
        let delta = send.delta;
        gate.check(delta.generation)?;
        integer(send.client_id)?;
        ensure!(
            !send.ciphertext.is_empty()
                && send.ciphertext.len() <= MAX_DISPATCH_PAYLOAD
                && send.metadata.len() <= 16 * 1024,
            "Encrypted send exceeds budget"
        );
        let peers = dispatch_peers(delta)?;
        let peers_json = serde_json::to_string(&peers)?;
        let total = send
            .ciphertext
            .len()
            .checked_add(send.metadata.len())
            .and_then(|n| n.checked_add(peers_json.len()));
        ensure!(
            total.is_some_and(|n| n <= MAX_DISPATCH_PAYLOAD),
            "Aggregate encrypted dispatch exceeds budget"
        );
        let mut hash = Sha256::new();
        digest_part(&mut hash, &delta_digest(&self.conn, delta)?);
        digest_part(&mut hash, b"send");
        hash.update(verified_codec.unwrap_or(0).to_be_bytes());
        hash.update(send.client_id.to_be_bytes());
        digest_part(&mut hash, send.ciphertext);
        digest_part(&mut hash, send.metadata);
        let digest = hash.finalize().to_vec();
        let tx = self.conn.transaction()?;
        if let Some((revision, cursor)) = replay(&tx, &delta.account, delta.operation_id, &digest)?
        {
            tx.rollback()?;
            return Ok(CommitReceipt {
                account: delta.account.clone(),
                operation_id: delta.operation_id,
                revision,
                replay: true,
                generation: delta.generation,
                cursor,
                published: vec![],
            });
        }
        let (local_epoch,platform):(i64,u8)=tx.query_row("SELECT local_epoch,platform FROM e2ee_outbox WHERE account=?1 AND client_id=?2 AND state='reserved'",params![delta.account,send.client_id as i64],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            delta
                .allowed_keys
                .iter()
                .all(|key| key.local().epoch == local_epoch.to_string()
                    && key.local().platform.wire() == platform),
            "Send namespace mismatch"
        );
        let next = apply_delta(&tx, delta)?;
        let thread: String = tx.query_row(
            "SELECT thread FROM e2ee_outbox WHERE account=?1 AND client_id=?2",
            params![delta.account, send.client_id as i64],
            |r| r.get(0),
        )?;
        let local = delta
            .allowed_keys
            .first()
            .ok_or_else(|| anyhow::anyhow!("Encrypted send requires a planned namespace"))?
            .local();
        let hash = payload_hash(
            &delta.account,
            send.client_id,
            &thread,
            local,
            send.ciphertext,
            send.metadata,
            &peers_json,
        )?;
        tx.execute("UPDATE e2ee_outbox SET state='ready',ciphertext=?3,metadata=?4,operation_id=?5,crypto_revision=?6,peer_bindings=?7,payload_hash=?8 WHERE account=?1 AND client_id=?2 AND state='reserved'",params![delta.account,send.client_id as i64,send.ciphertext,send.metadata,integer(delta.operation_id)?,next as i64,peers_json,hash.as_slice()])?;
        if let Some(codec) = verified_codec {
            tx.execute("INSERT INTO e2ee_verified_outbox(account,client_id,codec,operation_id,crypto_revision,payload_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![delta.account,send.client_id as i64,codec,integer(delta.operation_id)?,next as i64,hash.as_slice()])?;
        }
        remember(&tx, delta, &digest, next, None)?;
        let lease = gate.linearize(delta.generation)?;
        // Preparation-only checkpoint: failure rolls back; cancellation after
        // this check may durably prepare Ready. It never revokes an existing
        // outbox entry. Auth/logout cancellation still uses GenerationGate.
        check_preparation()?;
        tx.commit()?;
        drop(lease);
        Ok(CommitReceipt {
            account: delta.account.clone(),
            operation_id: delta.operation_id,
            revision: next,
            replay: false,
            generation: delta.generation,
            cursor: None,
            published: vec![],
        })
    }
    /// Only Ready is dispatchable. Sending/Unknown/acknowledged are never selected
    /// by this API, so retry cannot silently reencrypt or resend unknown delivery.
    pub fn e2ee_take_ready(&mut self, owner: &str, client_id: u64) -> Result<Outgoing> {
        account(owner)?;
        integer(client_id)?;
        let tx = self.conn.transaction()?;
        let changed=tx.execute("UPDATE e2ee_outbox SET state='sending' WHERE account=?1 AND client_id=?2 AND state='ready' AND local_epoch=(SELECT current_local FROM e2ee_accounts WHERE account=?1)",params![owner,client_id as i64])?;
        ensure!(changed == 1, "Encrypted send is not safely dispatchable");
        let row = outgoing(&tx, owner, client_id)?;
        tx.commit()?;
        Ok(row)
    }
    /// Reload exact committed bytes and atomically claim their single dispatch.
    /// Cancellation is nonblocking and the returned token still requires a fresh
    /// generation check immediately before actual network use.
    pub fn e2ee_begin_dispatch(
        &mut self,
        owner: &str,
        client_id: u64,
        generation: u64,
        gate: &GenerationGate,
    ) -> Result<DispatchReceipt> {
        account(owner)?;
        integer(client_id)?;
        gate.check(generation)?;
        let tx = self.conn.transaction()?;
        let changed=tx.execute("UPDATE e2ee_outbox SET state='sending' WHERE account=?1 AND client_id=?2 AND state='ready' AND local_epoch=(SELECT current_local FROM e2ee_accounts WHERE account=?1)",params![owner,client_id as i64])?;
        ensure!(changed == 1, "Encrypted send is not safely dispatchable");
        let row = outgoing(&tx, owner, client_id)?;
        let (operation_id,crypto_revision,peers_json,expected):(i64,i64,String,Vec<u8>)=tx.query_row("SELECT operation_id,crypto_revision,peer_bindings,payload_hash FROM e2ee_outbox WHERE account=?1 AND client_id=?2 AND length(peer_bindings)<=262144 AND length(payload_hash)=32",params![owner,client_id as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let peers: Vec<PeerNamespace> = serde_json::from_str(&peers_json)?;
        ensure!(
            peers.len() <= MAX_DISPATCH_PEERS,
            "Encrypted peer list exceeds budget"
        );
        for peer in &peers {
            validate_address(
                &tx,
                owner,
                &RecordKey::PeerIdentity {
                    local: row.local.clone(),
                    peer: peer.clone(),
                },
                true,
            )?;
        }
        let ciphertext = row
            .ciphertext
            .ok_or_else(|| anyhow::anyhow!("Missing committed ciphertext"))?;
        let metadata = row
            .metadata
            .ok_or_else(|| anyhow::anyhow!("Missing committed metadata"))?;
        let actual = payload_hash(
            owner,
            client_id,
            &row.thread,
            &row.local,
            &ciphertext,
            &metadata,
            &peers_json,
        )?;
        ensure!(
            actual.as_slice() == expected,
            "Committed ciphertext binding mismatch"
        );
        let marker:Option<(u32,i64,i64,Vec<u8>)>=tx.query_row("SELECT codec,operation_id,crypto_revision,payload_hash FROM e2ee_verified_outbox WHERE account=?1 AND client_id=?2",params![owner,client_id as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let verified_codec = marker
            .map(|(codec, op, rev, hash)| {
                ensure!(
                    codec == 1 && op == operation_id && rev == crypto_revision && hash == actual,
                    "Verified dispatch marker mismatch"
                );
                Ok(codec)
            })
            .transpose()?;
        let lease = gate.linearize(generation)?;
        tx.commit()?;
        drop(lease);
        Ok(DispatchReceipt {
            account: owner.into(),
            generation,
            operation_id: operation_id as u64,
            revision: crypto_revision as u64,
            client_id,
            thread: row.thread,
            local: row.local,
            peers,
            ciphertext,
            metadata,
            payload_hash: actual,
            verified_codec,
        })
    }
    pub fn e2ee_outgoing(&self, owner: &str, client_id: u64) -> Result<Outgoing> {
        account(owner)?;
        integer(client_id)?;
        outgoing(&self.conn, owner, client_id)
    }
    pub fn e2ee_recover_sends(&mut self, owner: &str) -> Result<usize> {
        account(owner)?;
        Ok(self.conn.execute(
            "UPDATE e2ee_outbox SET state='unknown' WHERE account=?1 AND state='sending'",
            [owner],
        )?)
    }
    /// A definite pre-write network failure may explicitly restore the same ready
    /// bytes. Ambiguous delivery stays Unknown and requires reconciliation.
    pub fn e2ee_send_failed(
        &mut self,
        owner: &str,
        client_id: u64,
        known_unsent: bool,
    ) -> Result<()> {
        account(owner)?;
        integer(client_id)?;
        let state = if known_unsent {
            SendState::Ready
        } else {
            SendState::Unknown
        };
        ensure!(self.conn.execute("UPDATE e2ee_outbox SET state=?3 WHERE account=?1 AND client_id=?2 AND state='sending'",params![owner,client_id as i64,state.text()])?==1,"Encrypted send transition conflict");
        Ok(())
    }
    pub fn e2ee_record_ack(&mut self, owner: &str, client_id: u64, server_id: &str) -> Result<()> {
        account(owner)?;
        integer(client_id)?;
        validate_id(server_id)?;
        ensure!(self.conn.execute("UPDATE e2ee_outbox SET server_id=?3,state=CASE WHEN state='committed' THEN state ELSE 'acknowledged' END WHERE account=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','committed') AND (server_id IS NULL OR server_id=?3)",params![owner,client_id as i64,server_id])?==1,"Conflicting encrypted acknowledgement");
        Ok(())
    }
    /// Closes a session-initialization send (command 543). It carries no chat message, so no echo will
    /// ever arrive to commit it; without this the draft guard would block the next init for the same
    /// thread. Only an acknowledged row whose server id marks it as an init can be retired this way.
    pub fn e2ee_retire_init(&mut self, owner: &str, client_id: u64) -> Result<()> {
        account(owner)?;
        integer(client_id)?;
        ensure!(
            self.conn.execute(
                "UPDATE e2ee_outbox SET state='committed' WHERE account=?1 AND client_id=?2 AND state='acknowledged' AND server_id LIKE 'init-%'",
                params![owner, client_id as i64],
            )? == 1,
            "Only an acknowledged init send can be retired"
        );
        Ok(())
    }
    /// ACK and echo both call this reconciliation path. A unique server ID and
    /// immutable message payload make either arrival order converge to one row.
    pub fn e2ee_commit_echo(
        &mut self,
        owner: &str,
        client_id: u64,
        server_id: &str,
        timestamp: Option<i64>,
    ) -> Result<(Message, Conversation)> {
        account(owner)?;
        integer(client_id)?;
        validate_id(server_id)?;
        if let Some(ts) = timestamp {
            ensure!(ts >= 0, "Invalid message timestamp");
        }
        let tx = self.conn.transaction()?;
        let(thread,body,created,previous):(String,String,i64,Option<String>)=tx.query_row("SELECT thread,body,created_ms,server_id FROM e2ee_outbox WHERE account=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','committed')",params![owner,client_id as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        ensure!(
            previous.as_deref().is_none_or(|id| id == server_id),
            "Conflicting encrypted echo"
        );
        let existing:Option<i64>=tx.query_row("SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",params![owner,thread,server_id],|r|r.get(0)).optional()?;
        let message = Message {
            account_id: owner.into(),
            thread_id: thread.clone(),
            id: server_id.into(),
            sender: owner.into(),
            text: body,
            sticker: None,
            media: None,
            sent_at_ms: existing.or(timestamp).unwrap_or(created),
        };
        message.validate()?;
        insert_validated(&tx, std::slice::from_ref(&message))?;
        tx.execute("UPDATE e2ee_outbox SET state='committed',server_id=?3 WHERE account=?1 AND client_id=?2",params![owner,client_id as i64,server_id])?;
        let conversation = tx.query_row(
            CONVERSATION_SQL,
            params![owner, thread, MAX_PREVIEW_CHARS as i64],
            conversation_row,
        )?;
        tx.commit()?;
        Ok((message, conversation))
    }
}
fn outgoing(conn: &Connection, owner: &str, client_id: u64) -> Result<Outgoing> {
    let (cipher_len,metadata_len):(Option<i64>,Option<i64>)=conn.query_row(
        "SELECT length(ciphertext),length(metadata) FROM e2ee_outbox WHERE account=?1 AND client_id=?2",
        params![owner,client_id as i64],|r|Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        cipher_len.is_none_or(|n| (0..=MAX_DISPATCH_PAYLOAD as i64).contains(&n))
            && metadata_len.is_none_or(|n| (0..=16 * 1024).contains(&n)),
        "Stored outbox exceeds budget"
    );
    conn.query_row("SELECT thread,local_epoch,platform,state,ciphertext,metadata,server_id FROM e2ee_outbox WHERE account=?1 AND client_id=?2",params![owner,client_id as i64],|r|{
        let platform:u8=r.get(2)?;let state:String=r.get(3)?;
        Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,platform,state,r.get::<_,Option<Vec<u8>>>(4)?,r.get::<_,Option<Vec<u8>>>(5)?,r.get::<_,Option<String>>(6)?))
    }).map_err(Into::into).and_then(|(thread,local_epoch,platform,state,ciphertext,metadata,server_id)|{
        let platform=match platform{0=>Platform::Mobile,1=>Platform::Web,2=>Platform::Pc,_=>bail!("Invalid stored Signal platform")};
        ensure!(ciphertext.as_ref().is_none_or(|v|v.len()<=MAX_DISPATCH_PAYLOAD)&&metadata.as_ref().is_none_or(|v|v.len()<=16*1024),"Stored outbox exceeds budget");
        Ok(Outgoing{client_id,thread,local:LocalNamespace{epoch:local_epoch.to_string(),platform},state:SendState::parse(&state)?,ciphertext,metadata,server_id})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(platform: Platform, identity: u8) -> VerifiedBinding {
        let mut key = [identity; 33];
        key[0] = 5;
        VerifiedBinding {
            platform,
            registration_id: 0,
            identity_key: key,
            session_timestamp: 100,
        }
    }
    fn setup() -> (MessageStore, LocalNamespace, PeerNamespace) {
        let mut store = MessageStore::in_memory().unwrap();
        initialize(&store.conn).unwrap();
        let local = store
            .e2ee_register_local("123", None, &binding(Platform::Pc, 1))
            .unwrap();
        let peer = store
            .e2ee_register_peer("123", &local, "456", None, &binding(Platform::Mobile, 2))
            .unwrap();
        (store, local, peer)
    }
    fn key(local: &LocalNamespace, id: u32) -> RecordKey {
        RecordKey::PreKey {
            local: local.clone(),
            id,
        }
    }
    fn input_context(id: u64) -> InputContext {
        InputContext {
            kind: TargetKind::Personal,
            thread: "d:456".into(),
            server_id: format!("op-{id}"),
            sender: PeerNamespace {
                uid: "456".into(),
                platform: Platform::Mobile,
                epoch: "1".into(),
            },
            registration_id: 0,
        }
    }
    fn delta(
        store: &MessageStore,
        _local: &LocalNamespace,
        id: u64,
        mutations: Vec<Mutation>,
    ) -> Delta {
        let allowed_keys = mutations
            .iter()
            .map(|m| match m {
                Mutation::Put(r) => r.key.clone(),
                Mutation::Delete(k) => k.clone(),
            })
            .collect();
        Delta {
            account: "123".into(),
            expected_revision: revision(&store.conn, "123").unwrap(),
            generation: 1,
            operation_id: id,
            input_digest: receive_input_digest(
                "123",
                "signal-personal",
                &input_context(id),
                &Sha256::digest(id.to_be_bytes()).into(),
            )
            .unwrap(),
            allowed_keys,
            activate_peers: Vec::new(),
            mutations,
        }
    }
    fn put(k: RecordKey, value: &[u8]) -> Mutation {
        Mutation::Put(Record {
            key: k,
            value: value.into(),
        })
    }
    fn message(id: &str) -> Message {
        Message {
            account_id: "123".into(),
            thread_id: "d:456".into(),
            id: id.into(),
            sender: "456".into(),
            text: "secret fixture".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1000,
        }
    }
    fn receive<'a>(delta: &'a Delta, messages: &'a [Message]) -> Receive<'a> {
        Receive {
            delta,
            messages,
            queue: "signal-personal",
            cursor: 100,
            context: input_context(delta.operation_id),
            ciphertext_digest: Sha256::digest(delta.operation_id.to_be_bytes()).into(),
        }
    }
    #[test]
    fn planned_identity_bytes_must_match_active_verified_sqlite_bindings() {
        let (mut store, local, peer) = setup();
        let local_binding = binding(Platform::Pc, 1);
        let peer_binding = binding(Platform::Mobile, 2);
        store
            .e2ee_validate_local_identity("123", &local, 0, &local_binding.identity_key)
            .unwrap();
        store
            .e2ee_validate_peer_identity("123", &local, &peer, 0, &peer_binding.identity_key, 100)
            .unwrap();
        assert!(store
            .e2ee_validate_local_identity("123", &local, 1, &local_binding.identity_key)
            .is_err());
        assert!(store
            .e2ee_validate_local_identity("123", &local, 0, &peer_binding.identity_key)
            .is_err());
        assert!(store
            .e2ee_validate_local_identity("999", &local, 0, &local_binding.identity_key)
            .is_err());
        assert!(store
            .e2ee_validate_peer_identity("123", &local, &peer, 1, &peer_binding.identity_key, 100)
            .is_err());
        assert!(store
            .e2ee_validate_peer_identity("123", &local, &peer, 0, &local_binding.identity_key, 100)
            .is_err());
        assert!(store
            .e2ee_validate_peer_identity("123", &local, &peer, 0, &peer_binding.identity_key, 101)
            .is_err());
        let next = store
            .e2ee_register_peer(
                "123",
                &local,
                "456",
                Some(peer.epoch.parse().unwrap()),
                &binding(Platform::Mobile, 9),
            )
            .unwrap();
        assert!(store
            .e2ee_validate_peer_identity("123", &local, &peer, 0, &peer_binding.identity_key, 100)
            .is_err());
        store
            .e2ee_validate_peer_identity(
                "123",
                &local,
                &next,
                0,
                &binding(Platform::Mobile, 9).identity_key,
                100,
            )
            .unwrap();
        store
            .e2ee_register_local(
                "123",
                Some(local.epoch.parse().unwrap()),
                &binding(Platform::Pc, 8),
            )
            .unwrap();
        assert!(store
            .e2ee_validate_local_identity("123", &local, 0, &local_binding.identity_key)
            .is_err());
        assert!(store
            .e2ee_validate_peer_identity(
                "123",
                &local,
                &next,
                0,
                &binding(Platform::Mobile, 9).identity_key,
                100
            )
            .is_err());
    }
    #[test]
    fn verified_marker_is_atomic_restart_recoverable_and_never_minted_by_raw_metadata() {
        let path = std::env::temp_dir().join(format!(
            "zalo-e2ee-marker-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let gate = GenerationGate::new(1).unwrap();
        let local;
        let trusted;
        {
            let mut store = MessageStore::open(&path).unwrap();
            local = store
                .e2ee_register_local("123", None, &binding(Platform::Pc, 1))
                .unwrap();
            let client = store
                .e2ee_reserve_send("123", &local, "d:456", "raw", 1000)
                .unwrap();
            let d = delta(&store, &local, 1, vec![put(key(&local, 1), b"raw")]);
            let metadata=br#"{"codec":1,"kind":"personal","path":"/api/e2ee/pc/message/sms","metric_command":16000}"#;
            store
                .e2ee_commit_send(
                    PreparedSend {
                        delta: &d,
                        client_id: client,
                        ciphertext: b"raw ciphertext",
                        metadata,
                    },
                    &gate,
                )
                .unwrap();
            assert_eq!(
                store
                    .e2ee_begin_dispatch("123", client, 1, &gate)
                    .unwrap()
                    .verified_codec(),
                None
            );
            trusted = store
                .e2ee_reserve_send("123", &local, "d:456", "trusted", 1001)
                .unwrap();
            let d = delta(
                &store,
                &local,
                2,
                vec![put(key(&local, 2), b"verified overlay")],
            );
            let before = revision(&store.conn, "123").unwrap();
            store.conn.execute_batch("CREATE TRIGGER marker_fault BEFORE INSERT ON e2ee_verified_outbox BEGIN SELECT RAISE(ABORT,'fixture marker write failure'); END;").unwrap();
            // Exercise the private transaction branch; production can reach this
            // only with a sealed coordinator candidate, never a public boolean.
            assert!(store
                .commit_send_inner(
                    PreparedSend {
                        delta: &d,
                        client_id: trusted,
                        ciphertext: b"exact verified bytes",
                        metadata
                    },
                    &gate,
                    Some(1),
                    || Ok(()),
                )
                .is_err());
            assert_eq!(revision(&store.conn, "123").unwrap(), before);
            assert!(store.e2ee_outgoing("123", trusted).unwrap().state == SendState::Reserved);
            assert!(store
                .e2ee_snapshot("123", &[key(&local, 2)])
                .unwrap()
                .records
                .is_empty());
            store
                .conn
                .execute_batch("DROP TRIGGER marker_fault")
                .unwrap();
            assert!(store
                .commit_send_inner(
                    PreparedSend {
                        delta: &d,
                        client_id: trusted,
                        ciphertext: b"exact verified bytes",
                        metadata
                    },
                    &gate,
                    Some(1),
                    || bail!("fixture preparation expired")
                )
                .is_err());
            assert!(gate.is_current(1));
            assert_eq!(revision(&store.conn, "123").unwrap(), before);
            assert_eq!(store.conn.query_row("SELECT count(*) FROM e2ee_verified_outbox WHERE account='123' AND client_id=?1",[trusted as i64],|r|r.get::<_,i64>(0)).unwrap(),0);
            assert!(store.e2ee_outgoing("123", trusted).unwrap().state == SendState::Reserved);
            store
                .commit_send_inner(
                    PreparedSend {
                        delta: &d,
                        client_id: trusted,
                        ciphertext: b"exact verified bytes",
                        metadata,
                    },
                    &gate,
                    Some(1),
                    || Ok(()),
                )
                .unwrap();
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            store.conn.execute("UPDATE e2ee_verified_outbox SET operation_id=99 WHERE account='123' AND client_id=?1",[trusted as i64]).unwrap();
            assert!(store.e2ee_begin_dispatch("123", trusted, 1, &gate).is_err());
            assert!(store.e2ee_outgoing("123", trusted).unwrap().state == SendState::Ready);
            store.conn.execute("UPDATE e2ee_verified_outbox SET operation_id=2 WHERE account='123' AND client_id=?1",[trusted as i64]).unwrap();
            let receipt = store.e2ee_begin_dispatch("123", trusted, 1, &gate).unwrap();
            assert_eq!(receipt.verified_codec(), Some(1));
            assert_eq!(receipt.ciphertext(), b"exact verified bytes");
            assert!(receipt.local() == &local);
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn own_group_sender_uses_local_identity_and_remote_namespaces_stay_strict() {
        let (mut store, local, peer) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let own = RecordKey::GroupSender {
            local: local.clone(),
            group: "789".into(),
            sender: PeerNamespace {
                uid: "123".into(),
                platform: local.platform,
                epoch: local.epoch.clone(),
            },
        };
        let d = delta(
            &store,
            &local,
            1,
            vec![put(own.clone(), b"own sender chain")],
        );
        store.e2ee_commit_receive(receive(&d, &[]), &gate).unwrap();
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM e2ee_peer_epochs WHERE uid='123'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .e2ee_snapshot("123", std::slice::from_ref(&own))
                .unwrap()
                .records[0]
                .value,
            b"own sender chain"
        );
        let remote = RecordKey::GroupSender {
            local: local.clone(),
            group: "789".into(),
            sender: peer,
        };
        let d = delta(&store, &local, 2, vec![put(remote, b"remote sender chain")]);
        store.e2ee_commit_receive(receive(&d, &[]), &gate).unwrap();
        let wrong = RecordKey::GroupSender {
            local: local.clone(),
            group: "789".into(),
            sender: PeerNamespace {
                uid: "123".into(),
                platform: Platform::Web,
                epoch: local.epoch.clone(),
            },
        };
        let d = delta(&store, &local, 3, vec![put(wrong, b"bad")]);
        assert!(store.e2ee_commit_receive(receive(&d, &[]), &gate).is_err());
        let unbound = RecordKey::GroupSender {
            local: local.clone(),
            group: "789".into(),
            sender: PeerNamespace {
                uid: "999".into(),
                platform: Platform::Pc,
                epoch: "1".into(),
            },
        };
        let d = delta(&store, &local, 4, vec![put(unbound, b"bad")]);
        assert!(store.e2ee_commit_receive(receive(&d, &[]), &gate).is_err());
    }
    #[test]
    fn cross_queue_replay_reuses_commit_after_rotation_and_rejects_changed_context() {
        let (mut store, local, peer) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let k = key(&local, 1);
        let d = delta(&store, &local, 1, vec![put(k.clone(), b"prekey")]);
        store.e2ee_commit_receive(receive(&d, &[]), &gate).unwrap();
        let mut context = input_context(2);
        context.sender = peer.clone();
        let cipher: [u8; 32] = Sha256::digest(b"fixture encrypted prekey input").into();
        let digest = receive_input_digest("123", "online", &context, &cipher).unwrap();
        let operation = store
            .e2ee_reserve_operation(
                "123",
                OperationIdentity::Receive { context: &context },
                &digest,
            )
            .unwrap();
        let mut d = delta(&store, &local, operation, vec![Mutation::Delete(k.clone())]);
        d.input_digest = digest;
        let committed = store
            .e2ee_commit_receive(
                Receive {
                    delta: &d,
                    messages: &[message("same")],
                    queue: "online",
                    cursor: 10,
                    context: context.clone(),
                    ciphertext_digest: cipher,
                },
                &gate,
            )
            .unwrap();
        assert_eq!(committed.published().len(), 1);
        let rev = revision(&store.conn, "123").unwrap();
        store
            .e2ee_register_peer(
                "123",
                &local,
                "456",
                Some(peer.epoch.parse().unwrap()),
                &binding(Platform::Mobile, 9),
            )
            .unwrap();
        let locator = InputLocator {
            kind: TargetKind::Personal,
            thread: "d:456",
            server_id: &context.server_id,
            sender_uid: "456",
            sender_platform: Platform::Mobile,
            registration_id: 0,
        };
        let restored = store
            .e2ee_lookup_committed_input("123", locator, &cipher)
            .unwrap()
            .unwrap();
        assert!(restored.sender == peer);
        let input = || EncryptedInput {
            queue: "offline",
            context: &context,
            ciphertext_digest: &cipher,
        };
        assert!(store
            .e2ee_committed_receive("123", operation, input(), 1, &gate)
            .unwrap()
            .is_none());
        let replay = store
            .e2ee_replay_receive(
                "123",
                ReplayDelivery {
                    operation_id: operation,
                    input: input(),
                    cursor: 20,
                    generation: 1,
                },
                &gate,
            )
            .unwrap()
            .unwrap();
        assert!(replay.is_replay() && replay.published().is_empty());
        assert_eq!(replay.revision(), rev);
        assert_eq!(replay.acknowledged_cursor(), Some(("offline", 20)));
        assert!(store.e2ee_snapshot("123", &[k]).unwrap().records.is_empty());
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        let after = revision(&store.conn, "123").unwrap();
        let mut different = context.clone();
        different.thread = "d:999".into();
        assert!(store
            .e2ee_replay_receive(
                "123",
                ReplayDelivery {
                    operation_id: operation,
                    input: EncryptedInput {
                        queue: "offline",
                        context: &different,
                        ciphertext_digest: &cipher
                    },
                    cursor: 30,
                    generation: 1
                },
                &gate
            )
            .is_err());
        let changed = [7; 32];
        assert!(store
            .e2ee_replay_receive(
                "123",
                ReplayDelivery {
                    operation_id: operation,
                    input: EncryptedInput {
                        queue: "offline",
                        context: &context,
                        ciphertext_digest: &changed
                    },
                    cursor: 30,
                    generation: 1
                },
                &gate
            )
            .is_err());
        store.conn.execute_batch("CREATE TRIGGER reject_replay BEFORE INSERT ON receive_cursors WHEN NEW.queue='retry' BEGIN SELECT RAISE(ABORT,'fixture cursor failure'); END;").unwrap();
        assert!(store
            .e2ee_replay_receive(
                "123",
                ReplayDelivery {
                    operation_id: operation,
                    input: EncryptedInput {
                        queue: "retry",
                        context: &context,
                        ciphertext_digest: &cipher
                    },
                    cursor: 30,
                    generation: 1
                },
                &gate
            )
            .is_err());
        assert_eq!(store.receive_cursor("123", "offline").unwrap(), "20");
        assert_eq!(store.receive_cursor("123", "retry").unwrap(), "1");
        assert_eq!(revision(&store.conn, "123").unwrap(), after);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM e2ee_deliveries WHERE queue='retry'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        gate.cancel();
        assert!(store
            .e2ee_replay_receive(
                "123",
                ReplayDelivery {
                    operation_id: operation,
                    input: input(),
                    cursor: 40,
                    generation: 1
                },
                &gate
            )
            .is_err());
    }
    #[test]
    fn durable_operation_reservations_survive_failure_and_restart_without_reusing_job_ids() {
        let path = std::env::temp_dir().join(format!(
            "zalo-e2ee-ops-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let context = input_context(9);
        let digest = [9; 32];
        let first;
        {
            let mut store = MessageStore::open(&path).unwrap();
            first = store
                .e2ee_reserve_operation(
                    "123",
                    OperationIdentity::Receive { context: &context },
                    &digest,
                )
                .unwrap();
            assert_eq!(first, 1);
            assert_eq!(
                store
                    .e2ee_reserve_operation(
                        "123",
                        OperationIdentity::Receive { context: &context },
                        &digest
                    )
                    .unwrap(),
                first
            );
        }
        {
            let mut store = MessageStore::open(&path).unwrap();
            assert_eq!(
                store
                    .e2ee_reserve_operation(
                        "123",
                        OperationIdentity::Receive { context: &context },
                        &digest
                    )
                    .unwrap(),
                first
            );
            assert!(store
                .e2ee_reserve_operation(
                    "123",
                    OperationIdentity::Receive { context: &context },
                    &[8; 32]
                )
                .is_err());
            let next = input_context(10);
            store.conn.execute_batch("CREATE TRIGGER reservation_fault BEFORE INSERT ON e2ee_operation_reservations BEGIN SELECT RAISE(ABORT,'fixture reservation failure'); END;").unwrap();
            assert!(store
                .e2ee_reserve_operation(
                    "123",
                    OperationIdentity::Receive { context: &next },
                    &digest
                )
                .is_err());
            assert_eq!(
                store
                    .conn
                    .query_row(
                        "SELECT last_id FROM e2ee_operation_counters WHERE account='123'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
            store
                .conn
                .execute_batch("DROP TRIGGER reservation_fault")
                .unwrap();
            assert_eq!(
                store
                    .e2ee_reserve_operation(
                        "123",
                        OperationIdentity::Receive { context: &next },
                        &digest
                    )
                    .unwrap(),
                2
            );
            assert_eq!(
                store
                    .e2ee_reserve_operation(
                        "999",
                        OperationIdentity::Receive { context: &context },
                        &digest
                    )
                    .unwrap(),
                1
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn six_full_size_device_payloads_preserve_all_bytes_and_aggregate_budget() {
        use base64::Engine;
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", &"x".repeat(MAX_TEXT_BYTES), 1000)
            .unwrap();
        let cipher = vec![42; MAX_TEXT_BYTES + 64];
        assert!(cipher.len() <= MAX_CIPHERTEXT_BYTES);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&cipher);
        let sms=serde_json::to_vec(&serde_json::json!({"devices":(0..6).map(|n|serde_json::json!({"device":n,"cipher":encoded})).collect::<Vec<_>>()})).unwrap();
        assert!(sms.len() < MAX_DISPATCH_PAYLOAD);
        let d = delta(&store, &local, 100, vec![put(key(&local, 1), b"ratchet")]);
        let metadata = b"independent six-device fixture";
        store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: &sms,
                    metadata,
                },
                &gate,
            )
            .unwrap();
        let row = store.e2ee_outgoing("123", client).unwrap();
        assert_eq!(row.ciphertext.unwrap(), sms);
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", "second", 1001)
            .unwrap();
        let d = delta(
            &store,
            &local,
            101,
            vec![put(key(&local, 2), b"must not commit")],
        );
        let before = revision(&store.conn, "123").unwrap();
        let boundary = vec![0; MAX_DISPATCH_PAYLOAD - metadata.len() - 2];
        store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: &boundary,
                    metadata,
                },
                &gate,
            )
            .unwrap();
        assert_eq!(
            store
                .e2ee_outgoing("123", client)
                .unwrap()
                .ciphertext
                .unwrap()
                .len(),
            boundary.len()
        );
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", "third", 1002)
            .unwrap();
        let d = delta(
            &store,
            &local,
            102,
            vec![put(key(&local, 3), b"must not commit")],
        );
        let too_large = vec![0; MAX_DISPATCH_PAYLOAD - metadata.len() - 1];
        assert!(store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: &too_large,
                    metadata
                },
                &gate
            )
            .is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), before + 1);
        assert!(store
            .e2ee_snapshot("123", &[key(&local, 3)])
            .unwrap()
            .records
            .is_empty());
        let mut d = delta(&store, &local, 103, vec![]);
        for n in 1..=65 {
            d.allowed_keys.push(RecordKey::Session {
                local: local.clone(),
                peer: PeerNamespace {
                    uid: format!("9{n}"),
                    epoch: n.to_string(),
                    platform: Platform::Pc,
                },
            })
        }
        assert!(store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"bounded",
                    metadata
                },
                &gate
            )
            .is_err());
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Reserved);
    }
    #[test]
    fn receive_is_atomic_replay_safe_and_account_scoped() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let k = key(&local, 1);
        let d = delta(&store, &local, 1, vec![put(k.clone(), b"prekey")]);
        let receipt = store
            .e2ee_commit_receive(receive(&d, &[message("m1")]), &gate)
            .unwrap();
        assert!(receipt.can_publish(&gate));
        assert_eq!(receipt.published().len(), 1);
        assert_eq!(
            receipt.acknowledged_cursor(),
            Some(("signal-personal", 100))
        );
        let replay = store
            .e2ee_commit_receive(receive(&d, &[message("m1")]), &gate)
            .unwrap();
        assert!(replay.is_replay() && replay.published().is_empty());
        let before_engine = store
            .e2ee_committed_operation("123", d.operation_id, &d.input_digest, 1, &gate)
            .unwrap()
            .unwrap();
        assert!(before_engine.is_replay() && before_engine.can_publish(&gate));
        assert!(store
            .e2ee_committed_operation("123", d.operation_id, &[0; 32], 1, &gate)
            .is_err());

        assert_eq!(
            store
                .e2ee_snapshot("123", std::slice::from_ref(&k))
                .unwrap()
                .records[0]
                .value,
            b"prekey"
        );
        assert!(store.e2ee_snapshot("999", &[k]).is_err());
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        assert!(store.page("999", "d:456", None, 10).unwrap().is_empty());
        let mut tampered = message("m1");
        tampered.text = "tampered".into();
        assert!(store
            .e2ee_commit_receive(receive(&d, &[tampered]), &gate)
            .is_err());
    }
    #[test]
    fn consumed_prekey_message_cursor_and_revision_roll_back_on_sqlite_failure() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let k = key(&local, 1);
        let d = delta(&store, &local, 1, vec![put(k.clone(), b"unconsumed")]);
        store.e2ee_commit_receive(receive(&d, &[]), &gate).unwrap();
        let before = revision(&store.conn, "123").unwrap();
        let d = delta(&store, &local, 2, vec![Mutation::Delete(k.clone())]);
        store.conn.execute_batch("CREATE TRIGGER reject_signal BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'simulated disk write failure'); END;").unwrap();
        assert!(store
            .e2ee_commit_receive(
                Receive {
                    delta: &d,
                    messages: &[message("fail")],
                    queue: "signal-personal",
                    cursor: 200,
                    context: input_context(d.operation_id),
                    ciphertext_digest: Sha256::digest(d.operation_id.to_be_bytes()).into(),
                },
                &gate
            )
            .is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), before);
        assert_eq!(
            store.receive_cursor("123", "signal-personal").unwrap(),
            "100"
        );
        assert!(store.page("123", "d:456", None, 10).unwrap().is_empty());
        assert_eq!(
            store.e2ee_snapshot("123", &[k]).unwrap().records[0].value,
            b"unconsumed"
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM e2ee_operations WHERE operation_id=2",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn sqlite_full_rolls_back_consumed_prekey_and_all_receive_effects() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let k = key(&local, 1);
        let initial = delta(&store, &local, 1, vec![put(k.clone(), b"unconsumed")]);
        store
            .e2ee_commit_receive(receive(&initial, &[]), &gate)
            .unwrap();
        let page_count: i64 = store
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        store
            .conn
            .pragma_update(None, "max_page_count", page_count)
            .unwrap();
        let revision_before = revision(&store.conn, "123").unwrap();
        let d = delta(
            &store,
            &local,
            2,
            vec![
                Mutation::Delete(k.clone()),
                put(key(&local, 2), &vec![7; 256 * 1024]),
            ],
        );
        let error = match store.e2ee_commit_receive(receive(&d, &[message("disk-full")]), &gate) {
            Ok(_) => panic!("disk full did not abort"),
            Err(error) => error,
        };
        assert!(
            matches!(error.downcast_ref::<rusqlite::Error>(),Some(rusqlite::Error::SqliteFailure(code,_)) if code.code==rusqlite::ErrorCode::DiskFull)
        );
        assert_eq!(revision(&store.conn, "123").unwrap(), revision_before);
        assert_eq!(
            store.e2ee_snapshot("123", &[k]).unwrap().records[0].value,
            b"unconsumed"
        );
        assert!(store.page("123", "d:456", None, 10).unwrap().is_empty());
        assert_eq!(
            store.receive_cursor("123", "signal-personal").unwrap(),
            "100"
        );
    }
    #[test]
    fn deferred_commit_failure_returns_no_receipt_and_cleans_marker_without_undoing_cancel() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let revision_before = revision(&store.conn, "123").unwrap();
        store.conn.execute_batch("CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY);
            CREATE TABLE fixture_child(id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TRIGGER fail_commit AFTER INSERT ON messages BEGIN INSERT INTO fixture_child VALUES(999); END;").unwrap();
        let d = delta(&store, &local, 1, vec![put(key(&local, 1), b"ratchet")]);
        assert!(store
            .commit_receive_inner(
                receive(&d, &[message("commit-fail")]),
                &gate,
                || {},
                || {
                    gate.cancel();
                    gate.replace(2).unwrap();
                }
            )
            .is_err());
        assert!(gate.is_current(2));
        assert_eq!(revision(&store.conn, "123").unwrap(), revision_before);
        assert!(store.page("123", "d:456", None, 10).unwrap().is_empty());
        assert!(store
            .e2ee_snapshot("123", &[key(&local, 1)])
            .unwrap()
            .records
            .is_empty());
        assert_eq!(store.receive_cursor("123", "signal-personal").unwrap(), "1");
    }
    #[test]
    fn stale_revision_and_outside_keyset_never_mutate_state() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let a = delta(&store, &local, 1, vec![put(key(&local, 1), b"a")]);
        let b = delta(&store, &local, 2, vec![put(key(&local, 2), b"b")]);
        store.e2ee_commit_receive(receive(&a, &[]), &gate).unwrap();
        assert!(store
            .e2ee_commit_receive(receive(&b, &[message("stale")]), &gate)
            .is_err());
        let mut c = delta(&store, &local, 3, vec![put(key(&local, 2), b"b")]);
        c.allowed_keys = vec![key(&local, 1)];
        assert!(store.e2ee_commit_receive(receive(&c, &[]), &gate).is_err());
        assert!(store
            .e2ee_snapshot("123", &[key(&local, 2)])
            .unwrap()
            .records
            .is_empty());
    }
    #[test]
    fn cancelled_before_commit_rolls_back_and_after_cas_never_resurrects_generation() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let d = delta(&store, &local, 1, vec![put(key(&local, 1), b"x")]);
        let before = revision(&store.conn, "123").unwrap();
        assert!(store
            .commit_receive_inner(
                receive(&d, &[message("cancel")]),
                &gate,
                || gate.cancel(),
                || {}
            )
            .is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), before);
        assert!(store.page("123", "d:456", None, 10).unwrap().is_empty());
        assert!(gate.replace(1).is_err());
        gate.replace(2).unwrap();
        let mut d = delta(&store, &local, 2, vec![put(key(&local, 1), b"x")]);
        d.generation = 2;
        let receipt = store
            .commit_receive_inner(
                receive(&d, &[message("committed")]),
                &gate,
                || {},
                || {
                    gate.cancel();
                    gate.replace(3).unwrap();
                },
            )
            .unwrap();
        assert!(!receipt.can_publish(&gate));
        assert!(gate.is_current(3));
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        assert!(GenerationGate::new(CANCEL_MARK).is_err());
    }
    #[test]
    fn iid_collisions_and_verified_rotation_preserve_old_namespace_records() {
        let (mut store, local, peer) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let k = RecordKey::Session {
            local: local.clone(),
            peer: peer.clone(),
        };
        let d = delta(&store, &local, 1, vec![put(k.clone(), b"old session")]);
        store
            .e2ee_commit_receive(receive(&d, &[message("old")]), &gate)
            .unwrap();
        let web = store
            .e2ee_register_peer("123", &local, "456", None, &binding(Platform::Web, 2))
            .unwrap();
        assert_ne!(web.epoch, peer.epoch);
        assert!(store
            .e2ee_register_peer("123", &local, "456", None, &binding(Platform::Mobile, 3))
            .is_err());
        let next = store
            .e2ee_register_peer(
                "123",
                &local,
                "456",
                Some(epoch(&peer.epoch).unwrap() as u64),
                &binding(Platform::Mobile, 3),
            )
            .unwrap();
        assert_ne!(next.epoch, peer.epoch);
        assert_eq!(
            store
                .e2ee_snapshot("123", std::slice::from_ref(&k))
                .unwrap()
                .records[0]
                .value,
            b"old session"
        );
        let d = delta(&store, &local, 2, vec![put(k, b"overwrite")]);
        assert!(store.e2ee_commit_receive(receive(&d, &[]), &gate).is_err());
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        let new_local = store
            .e2ee_register_local(
                "123",
                Some(epoch(&local.epoch).unwrap() as u64),
                &binding(Platform::Pc, 4),
            )
            .unwrap();
        assert_ne!(new_local.epoch, local.epoch);
    }
    #[test]
    fn pending_verified_identity_only_activates_with_successful_ratchet_message_commit() {
        let (mut store, local, old) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let pending = store
            .e2ee_reserve_peer("123", &local, "456", &binding(Platform::Mobile, 9))
            .unwrap();
        let old_key = RecordKey::Session {
            local: local.clone(),
            peer: old.clone(),
        };
        let new_key = RecordKey::Session {
            local: local.clone(),
            peer: pending.clone(),
        };
        let mut d = delta(
            &store,
            &local,
            1,
            vec![put(new_key.clone(), b"new identity session")],
        );
        d.activate_peers.push(PeerActivation {
            local: local.clone(),
            peer: pending.clone(),
            expected_previous: Some(epoch(&old.epoch).unwrap() as u64),
        });
        store.conn.execute_batch("CREATE TRIGGER reject_identity BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(store
            .e2ee_commit_receive(receive(&d, &[message("new-identity")]), &gate)
            .is_err());
        assert!(validate_address(&store.conn, "123", &old_key, true).is_ok());
        assert!(validate_address(&store.conn, "123", &new_key, true).is_err());
        assert!(store
            .e2ee_snapshot("123", std::slice::from_ref(&new_key))
            .unwrap()
            .records
            .is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_identity")
            .unwrap();
        store
            .e2ee_commit_receive(receive(&d, &[message("new-identity")]), &gate)
            .unwrap();
        assert!(validate_address(&store.conn, "123", &old_key, true).is_err());
        assert!(validate_address(&store.conn, "123", &new_key, true).is_ok());
    }
    #[test]
    fn released_v4_schema_and_all_existing_bytes_survive_additive_provisioning_tables() {
        let path = std::env::temp_dir().join(format!(
            "zalo-released-v4-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let tables = [
            "conversations",
            "messages",
            "outgoing",
            "outgoing_counters",
            "receive_cursors",
            "directory_entries",
            "e2ee_accounts",
            "e2ee_local_epochs",
            "e2ee_peer_epochs",
            "e2ee_records",
            "e2ee_inputs",
            "e2ee_operations",
            "e2ee_outbox",
        ];
        fn dump(conn: &Connection, tables: &[&str]) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
            tables
                .iter()
                .map(|table| {
                    let mut stmt = conn
                        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                        .unwrap();
                    let columns = stmt.column_count();
                    let rows = stmt
                        .query_map([], |row| {
                            (0..columns)
                                .map(|column| row.get::<_, rusqlite::types::Value>(column))
                                .collect::<rusqlite::Result<Vec<_>>>()
                        })
                        .unwrap();
                    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
                })
                .collect()
        }
        let before;
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(include_str!(
                "../../tests/fixtures/e2ee/released_v4_90b25d5.sql"
            ))
            .unwrap();
            conn.execute_batch("INSERT INTO conversations VALUES('123','d:456','Synthetic released thread',1000);
            INSERT INTO messages VALUES('123','d:456','released-id','456','Synthetic preserved message',1000);
            INSERT INTO outgoing VALUES('123','d:456','9','Synthetic unresolved send',1000,'unknown',NULL);
            INSERT INTO outgoing_counters VALUES('123',9);
            INSERT INTO receive_cursors VALUES('123','released-queue','100');
            INSERT INTO directory_entries VALUES('123','d:456','456',0,'Synthetic contact','synthetic contact',NULL,1,0,NULL,1000);
            INSERT INTO e2ee_accounts VALUES('123',7,1,1,1);
            INSERT INTO e2ee_local_epochs VALUES('123',1,2,7,zeroblob(32),100,29);
            INSERT INTO e2ee_peer_epochs VALUES('123',1,1,'456',0,7,zeroblob(32),101,1);
            INSERT INTO e2ee_records VALUES('123','released-opaque-address',x'0102030405');
            INSERT INTO e2ee_inputs VALUES('123','released-queue','released-id',zeroblob(32),1);
            INSERT INTO e2ee_operations VALUES('123',1,x'01',x'02',7,'released-queue',100);
            INSERT INTO e2ee_outbox VALUES('123',9,'d:456',1,2,'Synthetic encrypted draft',1000,'unknown',x'0504030201',x'7b7d',NULL,1,7,'[]',zeroblob(32));").unwrap();
            before = dump(&conn, &tables);
        }
        {
            let store = MessageStore::open(&path).unwrap();
            assert_eq!(dump(&store.conn, &tables), before);
            assert_eq!(
                store
                    .conn
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                8
            );
            for table in [
                "e2ee_local_candidates",
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
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn v2_v3_migrations_preserve_messages_legacy_outbox_cursors_and_directory() {
        for version in [2, 3] {
            let root = std::env::temp_dir().join(format!(
                "zalo-e2ee-migrate-{}",
                crate::domain::local_message_id().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let path = root.join("disposable.sqlite3");
            let mut store = MessageStore::open(&path).unwrap();
            store.insert_batch(&[message("legacy")]).unwrap();
            let client = store
                .prepare_outgoing("123", "d:456", "legacy outgoing", 1000)
                .unwrap();
            store
                .receive_batch("123", &[], Some(("old-queue", "77")))
                .unwrap();
            store.conn.execute("INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,search_name,observed_at_ms) VALUES('123','d:456','456',0,'Legacy contact','legacy contact',1000)",[]).unwrap();
            store.conn.execute_batch("DROP TABLE e2ee_outbox;DROP TABLE e2ee_verified_outbox; DROP TABLE e2ee_deliveries;DROP TABLE e2ee_operation_reservations;DROP TABLE e2ee_operation_counters;DROP TABLE e2ee_inputs;DROP TABLE e2ee_operations;DROP TABLE e2ee_records;DROP TABLE e2ee_peer_epochs;DROP TABLE e2ee_local_epochs;DROP TABLE e2ee_accounts;").unwrap();
            store
                .conn
                .pragma_update(None, "user_version", version)
                .unwrap();
            drop(store);
            let store = MessageStore::open(&path).unwrap();
            assert_eq!(
                store
                    .conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                8
            );
            assert_eq!(
                store.page("123", "d:456", None, 10).unwrap()[0].id,
                "legacy"
            );
            assert_eq!(store.receive_cursor("123", "old-queue").unwrap(), "77");
            assert_eq!(
                store
                    .conn
                    .query_row(
                        "SELECT body FROM outgoing WHERE account_id='123' AND client_id=?1",
                        [client.to_string()],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                "legacy outgoing"
            );
            assert_eq!(store.conn.query_row("SELECT display_name FROM directory_entries WHERE account_id='123' AND thread_id='d:456'",[],|r|r.get::<_,String>(0)).unwrap(),"Legacy contact");
            assert_eq!(
                store
                    .conn
                    .query_row("SELECT count(*) FROM e2ee_accounts", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            drop(store);
            // Reopen with the ordinary/default constructor: no optional engine needed.
            MessageStore::open(&path).unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn snapshot_and_delta_resource_caps_are_checked_before_allocating_or_writing() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let keys: Vec<_> = (1..=513).map(|id| key(&local, id)).collect();
        assert!(store.e2ee_snapshot("123", &keys).is_err());
        let d = delta(
            &store,
            &local,
            1,
            vec![put(key(&local, 1), &vec![1; MAX_SNAPSHOT_BYTES])],
        );
        assert!(store.e2ee_commit_receive(receive(&d, &[]), &gate).is_err());
        assert!(store
            .e2ee_snapshot("123", &[key(&local, 1)])
            .unwrap()
            .records
            .is_empty());
        store
            .conn
            .execute(
                "INSERT INTO e2ee_records VALUES(?1,?2,zeroblob(?3))",
                params![
                    "123",
                    key(&local, 1).encoded().unwrap(),
                    MAX_SNAPSHOT_BYTES + 1
                ],
            )
            .unwrap();
        assert!(store.e2ee_snapshot("123", &[key(&local, 1)]).is_err());
        let bad = LocalNamespace {
            epoch: "01".into(),
            platform: Platform::Pc,
        };
        assert!(store.e2ee_snapshot("123", &[key(&bad, 1)]).is_err());
        assert!(serde_json::from_str::<RecordKey>(
            r#"{"kind":"local_identity","local":{"epoch":"1","platform":"Pc"},"extra":true}"#
        )
        .is_err());
    }
    #[test]
    fn malformed_key_dtos_are_rejected_during_deserialization() {
        assert!(serde_json::from_str::<RecordKey>(
            r#"{"kind":"local_identity","local":{"epoch":"01","platform":"Pc"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<RecordKey>(r#"{"kind":"session","local":{"epoch":"1","platform":"Pc"},"peer":{"uid":"0","platform":"Pc","epoch":"1"}}"#).is_err());
    }
    fn prepared(store: &mut MessageStore, local: &LocalNamespace, id: u64, text: &str) -> u64 {
        let client = store
            .e2ee_reserve_send("123", local, "d:456", text, 1000)
            .unwrap();
        let d = delta(
            store,
            local,
            id,
            vec![put(key(local, id as u32), b"advanced ratchet")],
        );
        let receipt = store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"exact ciphertext",
                    metadata: b"inner metadata",
                },
                &GenerationGate::new(1).unwrap(),
            )
            .unwrap();
        assert!(!receipt.is_replay());
        client
    }
    #[test]
    fn send_is_atomic_and_exact_ciphertext_survives_unknown_and_reconciliation() {
        let (mut store, local, _) = setup();
        let text = "x".repeat(MAX_TEXT_BYTES);
        let client = prepared(&mut store, &local, 1, &text);
        let bytes = store.e2ee_take_ready("123", client).unwrap();
        assert!(bytes.state == SendState::Sending);
        assert_eq!(
            bytes.ciphertext.as_deref(),
            Some(b"exact ciphertext".as_slice())
        );
        store.e2ee_send_failed("123", client, true).unwrap();
        let again = store.e2ee_take_ready("123", client).unwrap();
        assert_eq!(again.ciphertext, bytes.ciphertext);
        assert_eq!(store.e2ee_recover_sends("123").unwrap(), 1);
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Unknown);
        assert!(store.e2ee_take_ready("123", client).is_err());
        store.e2ee_record_ack("123", client, "server-1").unwrap();
        store
            .e2ee_commit_echo("123", client, "server-1", Some(2000))
            .unwrap();
        store
            .e2ee_commit_echo("123", client, "server-1", Some(2001))
            .unwrap();
        store.e2ee_record_ack("123", client, "server-1").unwrap();
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Committed);
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        assert!(store.e2ee_record_ack("123", client, "tampered").is_err());
        assert!(store.e2ee_outgoing("999", client).is_err());
        assert!(store
            .e2ee_reserve_send(
                "123",
                &local,
                "d:456",
                &"x".repeat(MAX_TEXT_BYTES + 1),
                3000
            )
            .is_err());
    }
    #[test]
    fn accepted_send_survives_echo_storage_failure_without_becoming_dispatchable() {
        let (mut store, local, _) = setup();
        let client = prepared(&mut store, &local, 1, "accepted draft");
        let dispatched = store.e2ee_take_ready("123", client).unwrap();
        store.e2ee_record_ack("123", client, "9001").unwrap();
        store.conn.execute_batch("CREATE TRIGGER fail_e2ee_echo BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT, 'synthetic storage failure'); END;").unwrap();
        assert!(store
            .e2ee_commit_echo("123", client, "9001", Some(2000))
            .is_err());
        let accepted = store.e2ee_outgoing("123", client).unwrap();
        assert!(accepted.state == SendState::Acknowledged);
        assert_eq!(accepted.server_id.as_deref(), Some("9001"));
        assert_eq!(accepted.ciphertext, dispatched.ciphertext);
        assert_eq!(store.e2ee_recover_sends("123").unwrap(), 0);
        assert!(store.e2ee_take_ready("123", client).is_err());
        assert!(store.e2ee_send_failed("123", client, true).is_err());
        store
            .conn
            .execute_batch("DROP TRIGGER fail_e2ee_echo;")
            .unwrap();
        store
            .e2ee_commit_echo("123", client, "9001", Some(2000))
            .unwrap();
        store
            .e2ee_commit_echo("123", client, "9001", Some(2001))
            .unwrap();
        assert_eq!(store.page("123", "d:456", None, 10).unwrap().len(), 1);
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Committed);
    }
    #[test]
    fn dispatch_authority_requires_committed_exact_bytes_current_peers_and_generation() {
        let (mut store, local, peer) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", "draft", 1000)
            .unwrap();
        let d = delta(
            &store,
            &local,
            1,
            vec![put(
                RecordKey::Session {
                    local: local.clone(),
                    peer: peer.clone(),
                },
                b"ratchet",
            )],
        );
        store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"exact",
                    metadata: b"metadata",
                },
                &gate,
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE e2ee_outbox SET ciphertext=?2 WHERE client_id=?1",
                params![client as i64, b"tampered".as_slice()],
            )
            .unwrap();
        assert!(store.e2ee_begin_dispatch("123", client, 1, &gate).is_err());
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Ready);
        store
            .conn
            .execute(
                "UPDATE e2ee_outbox SET ciphertext=?2 WHERE client_id=?1",
                params![client as i64, b"exact".as_slice()],
            )
            .unwrap();
        gate.cancel();
        assert!(store.e2ee_begin_dispatch("123", client, 1, &gate).is_err());
        gate.replace(2).unwrap();
        let token = store.e2ee_begin_dispatch("123", client, 2, &gate).unwrap();
        assert_eq!(token.account(), "123");
        assert_eq!(token.client_id(), client);
        assert_eq!(token.operation_id(), 1);
        assert_eq!(token.ciphertext(), b"exact");
        assert_eq!(token.metadata(), b"metadata");
        assert!(token.peers() == [peer]);
        assert!(token.can_dispatch(&gate));
        gate.cancel();
        assert!(!token.can_dispatch(&gate));
        assert!(store.e2ee_begin_dispatch("123", client, 2, &gate).is_err());
    }
    #[test]
    fn replayed_unknown_send_never_returns_to_ready_or_accepts_reencrypted_bytes() {
        let (mut store, local, _) = setup();
        let gate = GenerationGate::new(1).unwrap();
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", "draft", 1000)
            .unwrap();
        let d = delta(&store, &local, 1, vec![put(key(&local, 1), b"ratchet")]);
        store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"exact",
                    metadata: b"metadata",
                },
                &gate,
            )
            .unwrap();
        store.e2ee_begin_dispatch("123", client, 1, &gate).unwrap();
        store.e2ee_recover_sends("123").unwrap();
        let receipt = store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"exact",
                    metadata: b"metadata",
                },
                &gate,
            )
            .unwrap();
        assert!(receipt.is_replay());
        assert!(store.e2ee_outgoing("123", client).unwrap().state == SendState::Unknown);
        assert!(store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"reencrypted",
                    metadata: b"metadata"
                },
                &gate
            )
            .is_err());
        assert!(store.e2ee_take_ready("123", client).is_err());
        assert!(store
            .e2ee_reserve_send("123", &local, "d:456", "draft", 2000)
            .is_err());
    }
    #[test]
    fn send_trigger_failure_keeps_reserved_bytes_and_ratchet_unchanged() {
        let (mut store, local, _) = setup();
        let client = store
            .e2ee_reserve_send("123", &local, "d:456", "draft", 1000)
            .unwrap();
        let before = revision(&store.conn, "123").unwrap();
        store.conn.execute_batch("CREATE TRIGGER fail_cipher BEFORE UPDATE ON e2ee_outbox WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'fixture storage failure'); END;").unwrap();
        let d = delta(&store, &local, 1, vec![put(key(&local, 1), b"ratchet")]);
        assert!(store
            .e2ee_commit_send(
                PreparedSend {
                    delta: &d,
                    client_id: client,
                    ciphertext: b"ciphertext",
                    metadata: b"metadata"
                },
                &GenerationGate::new(1).unwrap()
            )
            .is_err());
        assert_eq!(revision(&store.conn, "123").unwrap(), before);
        let row = store.e2ee_outgoing("123", client).unwrap();
        assert!(row.state == SendState::Reserved && row.ciphertext.is_none());
        assert!(store
            .e2ee_snapshot("123", &[key(&local, 1)])
            .unwrap()
            .records
            .is_empty());
    }
    #[test]
    fn crash_reopen_preserves_exact_ciphertext_and_never_automatically_replays_unknown() {
        let root = std::env::temp_dir().join(format!(
            "zalo-e2ee-store-{}",
            crate::domain::local_message_id().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("disposable.sqlite3");
        let mut store = MessageStore::open(&path).unwrap();
        initialize(&store.conn).unwrap();
        let local = store
            .e2ee_register_local("123", None, &binding(Platform::Pc, 1))
            .unwrap();
        let client = prepared(&mut store, &local, 1, "draft");
        store.e2ee_take_ready("123", client).unwrap();
        drop(store);
        let mut store = MessageStore::open(&path).unwrap();
        store.e2ee_recover_sends("123").unwrap();
        let row = store.e2ee_outgoing("123", client).unwrap();
        assert!(row.state == SendState::Unknown);
        assert_eq!(
            row.ciphertext.as_deref(),
            Some(b"exact ciphertext".as_slice())
        );
        assert_eq!(row.metadata.as_deref(), Some(b"inner metadata".as_slice()));
        assert!(store.e2ee_take_ready("123", client).is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
