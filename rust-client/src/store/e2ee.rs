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

pub struct Receive<'a> {
    pub delta: &'a Delta,
    pub messages: &'a [Message],
    pub queue: &'a str,
    pub cursor: u64,
    pub server_message_id: String,
    pub ciphertext_digest: [u8; 32],
}
pub struct EncryptedInput<'a> {
    pub queue: &'a str,
    pub server_id: &'a str,
    pub ciphertext_digest: &'a [u8; 32],
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

/// Stable input identity includes account, queue, server-message identity and
/// encrypted bytes, not ciphertext alone. Use it before invoking the engine.
pub fn receive_input_digest(
    owner: &str,
    queue: &str,
    server_id: &str,
    ciphertext_digest: &[u8; 32],
) -> Result<[u8; 32]> {
    account(owner)?;
    validate_id(queue)?;
    validate_id(server_id)?;
    let mut hash = Sha256::new();
    digest_part(&mut hash, owner.as_bytes());
    digest_part(&mut hash, queue.as_bytes());
    digest_part(&mut hash, server_id.as_bytes());
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
    CREATE TABLE IF NOT EXISTS e2ee_operations(account TEXT NOT NULL,operation_id INTEGER NOT NULL,digest BLOB NOT NULL,input_digest BLOB NOT NULL,revision INTEGER NOT NULL,queue TEXT,cursor INTEGER,PRIMARY KEY(account,operation_id));
    CREATE TABLE IF NOT EXISTS e2ee_outbox(account TEXT NOT NULL,client_id INTEGER NOT NULL,thread TEXT NOT NULL,local_epoch INTEGER NOT NULL,platform INTEGER NOT NULL,body TEXT NOT NULL,created_ms INTEGER NOT NULL,state TEXT NOT NULL,ciphertext BLOB,metadata BLOB,server_id TEXT,operation_id INTEGER,crypto_revision INTEGER,peer_bindings TEXT,payload_hash BLOB,PRIMARY KEY(account,client_id));
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
        ensure!(conn.query_row("SELECT EXISTS(SELECT 1 FROM e2ee_peer_epochs WHERE account=?1 AND local_epoch=?2 AND peer_epoch=?3 AND uid=?4 AND platform=?5 AND (?6=0 OR active=1))",params![owner,epoch(&local.epoch)?,epoch(&peer.epoch)?,peer.uid,peer.platform.wire(),i64::from(active)],|r|r.get::<_,bool>(0))?,"Unknown or retired peer crypto namespace");
    }
    Ok(encoded)
}
fn delta_digest(conn: &Connection, delta: &Delta) -> Result<Vec<u8>> {
    account(&delta.account)?;
    integer(delta.operation_id)?;
    integer(delta.generation)?;
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

impl MessageStore {
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
    pub fn e2ee_committed_receive(
        &self,
        owner: &str,
        operation_id: u64,
        input: EncryptedInput<'_>,
        generation: u64,
        gate: &GenerationGate,
    ) -> Result<Option<CommitReceipt>> {
        let EncryptedInput {
            queue,
            server_id,
            ciphertext_digest,
        } = input;
        let input = receive_input_digest(owner, queue, server_id, ciphertext_digest)?;
        let previous:Option<(String,String,i64)>=self.conn.query_row("SELECT queue,server_id,operation_id FROM e2ee_inputs WHERE account=?1 AND cipher_digest=?2",params![owner,ciphertext_digest.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((old_queue, old_server, old_op)) = previous {
            ensure!(
                old_queue == queue && old_server == server_id && old_op == integer(operation_id)?,
                "Ciphertext replay has a different server identity"
            );
        }
        self.e2ee_committed_operation(owner, operation_id, &input, generation, gate)
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
                    &receive.server_message_id,
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
                message.account_id == delta.account,
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
        tx.execute("INSERT INTO e2ee_inputs(account,queue,server_id,cipher_digest,operation_id) VALUES(?1,?2,?3,?4,?5)",params![delta.account,receive.queue,receive.server_message_id,receive.ciphertext_digest.as_slice(),integer(delta.operation_id)?])?;
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
        let cursor = receive.cursor.to_string();
        tx.execute("INSERT INTO receive_cursors(account_id,queue,last_id) VALUES(?1,?2,?3) ON CONFLICT(account_id,queue) DO UPDATE SET last_id=CASE WHEN length(excluded.last_id)>length(last_id) OR (length(excluded.last_id)=length(last_id) AND excluded.last_id>last_id) THEN excluded.last_id ELSE last_id END",params![delta.account,receive.queue,cursor])?;
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
        let delta = send.delta;
        gate.check(delta.generation)?;
        integer(send.client_id)?;
        ensure!(
            !send.ciphertext.is_empty()
                && send.ciphertext.len() <= MAX_DISPATCH_PAYLOAD
                && send.metadata.len() <= 16 * 1024,
            "Encrypted send exceeds budget"
        );
        let mut hash = Sha256::new();
        digest_part(&mut hash, &delta_digest(&self.conn, delta)?);
        digest_part(&mut hash, b"send");
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
        let peers: Vec<_> = delta
            .allowed_keys
            .iter()
            .filter_map(RecordKey::peer)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let peers_json = serde_json::to_string(&peers)?;
        ensure!(
            peers.len() <= MAX_DISPATCH_PEERS
                && send.ciphertext.len() + send.metadata.len() + peers_json.len()
                    <= MAX_DISPATCH_PAYLOAD,
            "Aggregate encrypted dispatch exceeds budget"
        );
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
        remember(&tx, delta, &digest, next, None)?;
        let lease = gate.linearize(delta.generation)?;
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
                &format!("op-{id}"),
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
            sent_at_ms: 1000,
        }
    }
    fn receive<'a>(delta: &'a Delta, messages: &'a [Message]) -> Receive<'a> {
        Receive {
            delta,
            messages,
            queue: "signal-personal",
            cursor: 100,
            server_message_id: format!("op-{}", delta.operation_id),
            ciphertext_digest: Sha256::digest(delta.operation_id.to_be_bytes()).into(),
        }
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
                    server_message_id: format!("op-{}", d.operation_id),
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
            store.conn.execute_batch("DROP TABLE e2ee_outbox;DROP TABLE e2ee_inputs;DROP TABLE e2ee_operations;DROP TABLE e2ee_records;DROP TABLE e2ee_peer_epochs;DROP TABLE e2ee_local_epochs;DROP TABLE e2ee_accounts;").unwrap();
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
                4
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
