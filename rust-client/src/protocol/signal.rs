//! Pure Signal transport contracts derived from the repository's own PC bundle.
//! No networking, key generation, signature verification, persistence or ACK side effects.
//! Public-key shape checks are NOT identity/signature verification. See PROTOCOL_SIGNAL.md.
#[cfg(feature = "signal-crypto")]
pub(crate) mod engine_adapter;
#[cfg(feature = "signal-crypto")]
pub(crate) mod provisioning;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_CIPHER_BYTES: usize = 128 * 1024;
pub const MAX_PAGE_ITEMS: usize = 200;
pub const MAX_METADATA_ITEMS: usize = 20_000;
pub const MAX_PAGES: usize = 200;
pub const MAX_PREKEYS: usize = 100;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalError {
    Invalid,
    Resource,
    Scope,
    Correlation,
    Duplicate,
    Incomplete,
    OutOfOrder,
    Unsupported,
    Authentication,
    Service(i64),
}
impl std::fmt::Display for SignalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never retain values, peer IDs, request bodies or cryptographic material.
        f.write_str(match self {
            Self::Invalid => "Malformed Signal transport data",
            Self::Resource => "Signal transport limit exceeded",
            Self::Scope => "Signal account or generation mismatch",
            Self::Correlation => "Unmatched Signal response",
            Self::Duplicate => "Conflicting or repeated Signal data",
            Self::Incomplete => "Incomplete Signal metadata",
            Self::OutOfOrder => "Signal initialization ordering mismatch",
            Self::Unsupported => "Unsupported Signal data",
            Self::Authentication => "Signal session authentication rejected",
            Self::Service(_) => "Signal service rejected request",
        })
    }
}
impl std::error::Error for SignalError {}
type Result<T> = std::result::Result<T, SignalError>;

/// Signal platform IDs, deliberately distinct from authentication client type 24.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DevicePlatform {
    Mobile,
    Web,
    Pc,
}
impl DevicePlatform {
    pub const fn wire(self) -> u8 {
        match self {
            Self::Mobile => 0,
            Self::Web => 1,
            Self::Pc => 2,
        }
    }
    pub fn parse(value: &Value) -> Result<Self> {
        match value.as_u64() {
            Some(0) => Ok(Self::Mobile),
            Some(1) => Ok(Self::Web),
            Some(2) => Ok(Self::Pc),
            _ => Err(SignalError::Unsupported),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Scope {
    account: String,
    generation: u64,
}
impl Scope {
    pub fn new(account: &str, generation: u64) -> Result<Self> {
        uid(account)?;
        if generation == 0 {
            return Err(SignalError::Scope);
        }
        Ok(Self {
            account: account.into(),
            generation,
        })
    }
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    fn require(&self, active: &Self) -> Result<()> {
        if self == active {
            Ok(())
        } else {
            Err(SignalError::Scope)
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Address {
    scope: Scope,
    peer: String,
    device: DevicePlatform,
    local_epoch: String,
    peer_epoch: String,
}
impl Address {
    pub fn new(
        scope: &Scope,
        peer: &str,
        device: DevicePlatform,
        local_epoch: &str,
        peer_epoch: &str,
    ) -> Result<Self> {
        uid(peer)?;
        epoch(local_epoch)?;
        epoch(peer_epoch)?;
        Ok(Self {
            scope: scope.clone(),
            peer: peer.into(),
            device,
            local_epoch: local_epoch.into(),
            peer_epoch: peer_epoch.into(),
        })
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn peer(&self) -> &str {
        &self.peer
    }
    pub fn device(&self) -> DevicePlatform {
        self.device
    }
    pub fn local_epoch(&self) -> &str {
        &self.local_epoch
    }
    pub fn peer_epoch(&self) -> &str {
        &self.peer_epoch
    }
}
fn epoch(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|b| b.is_ascii_digit())
        || value.parse::<i64>().ok().is_none_or(|n| n <= 0)
    {
        Err(SignalError::Scope)
    } else {
        Ok(())
    }
}
fn uid(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 24
        || value.starts_with('0')
        || !value.bytes().all(|x| x.is_ascii_digit())
    {
        Err(SignalError::Invalid)
    } else {
        Ok(())
    }
}
fn number(value: &Value) -> Result<u64> {
    let n = if let Some(n) = value.as_u64() {
        n
    } else if let Some(s) = value.as_str() {
        if s.is_empty() || s.len() > 16 || !s.bytes().all(|x| x.is_ascii_digit()) {
            return Err(SignalError::Invalid);
        }
        s.parse().map_err(|_| SignalError::Invalid)?
    } else {
        return Err(SignalError::Invalid);
    };
    if n > MAX_SAFE_INTEGER {
        Err(SignalError::Invalid)
    } else {
        Ok(n)
    }
}
fn registration(value: &Value) -> Result<u16> {
    let n = number(value)?;
    if n > 16_383 {
        Err(SignalError::Invalid)
    } else {
        Ok(n as u16)
    }
}
fn bounded_json(value: &Value) -> Result<()> {
    if serde_json::to_vec(value)
        .map_err(|_| SignalError::Invalid)?
        .len()
        > MAX_FRAME_BYTES - 4
    {
        Err(SignalError::Resource)
    } else {
        Ok(())
    }
}
fn bounded_array(value: &Value, maximum: usize) -> Result<&Vec<Value>> {
    let a = value.as_array().ok_or(SignalError::Invalid)?;
    if a.len() > maximum {
        Err(SignalError::Resource)
    } else {
        Ok(a)
    }
}
fn base64_bytes(value: &str, maximum: usize) -> Result<Vec<u8>> {
    if value.is_empty() || value.len() > maximum.div_ceil(3) * 4 {
        return Err(SignalError::Resource);
    }
    let bytes = STANDARD.decode(value).map_err(|_| SignalError::Invalid)?;
    if bytes.len() > maximum {
        return Err(SignalError::Resource);
    }
    if STANDARD.encode(&bytes) != value {
        return Err(SignalError::Invalid);
    }
    Ok(bytes)
}
#[derive(Clone)]
pub struct PublicKey([u8; 33]);
impl PublicKey {
    pub fn parse(value: &str) -> Result<Self> {
        let bytes = base64_bytes(value, 33)?;
        let key: [u8; 33] = bytes.try_into().map_err(|_| SignalError::Invalid)?;
        if key[0] != 5 || key[1..].iter().all(|x| *x == 0) {
            return Err(SignalError::Invalid);
        }
        Ok(Self(key))
    }
    pub fn bytes(&self) -> &[u8; 33] {
        &self.0
    }
    fn wire(&self) -> String {
        STANDARD.encode(self.0)
    }
}
#[derive(Clone)]
pub struct Signature([u8; 64]);
impl Signature {
    pub fn parse(value: &str) -> Result<Self> {
        Ok(Self(
            base64_bytes(value, 64)?
                .try_into()
                .map_err(|_| SignalError::Invalid)?,
        ))
    }
    pub fn bytes(&self) -> &[u8; 64] {
        &self.0
    }
    fn wire(&self) -> String {
        STANDARD.encode(self.0)
    }
}
#[derive(Clone)]
pub struct OneTimePreKey {
    pub id: u32,
    pub key: PublicKey,
}
#[derive(Clone)]
pub struct PublicBundle {
    pub device: DevicePlatform,
    pub registration_id: u16,
    pub identity: PublicKey,
    pub signed_prekey_id: u32,
    pub signed_prekey: PublicKey,
    pub signature: Signature,
    pub one_time_prekeys: Vec<OneTimePreKey>,
}
impl PublicBundle {
    fn validate(&self) -> Result<()> {
        registration(&json!(self.registration_id))?;
        if self.signed_prekey_id == 0 || self.one_time_prekeys.len() > MAX_PREKEYS {
            return Err(SignalError::Resource);
        }
        let mut seen = BTreeSet::new();
        for key in &self.one_time_prekeys {
            if key.id == 0 || !seen.insert(key.id) || key.id == self.signed_prekey_id {
                return Err(SignalError::Duplicate);
            }
        }
        Ok(())
    }
    fn wire(&self) -> Result<Value> {
        self.validate()?;
        Ok(
            json!({"deviceId":self.device.wire(),"preKey":{"iKey":{"id":self.registration_id,"key":self.identity.wire()},"signature":self.signature.wire(),"spKey":{"id":self.signed_prekey_id,"key":self.signed_prekey.wire()}},"opKeys":self.one_time_prekeys.iter().map(|p|json!({"id":p.id,"key":p.key.wire()})).collect::<Vec<_>>()}),
        )
    }
}
fn key_id(value: &Value) -> Result<u32> {
    let n = number(value)?;
    if n == 0 || n > u32::MAX as u64 {
        Err(SignalError::Invalid)
    } else {
        Ok(n as u32)
    }
}
fn parse_key(value: &Value) -> Result<PublicKey> {
    PublicKey::parse(value.as_str().ok_or(SignalError::Invalid)?)
}
fn parse_bundle(value: &Value) -> Result<PublicBundle> {
    let pre = value.get("preKey").ok_or(SignalError::Invalid)?;
    let one_time_prekeys = bounded_array(&value["opKeys"], MAX_PREKEYS)?
        .iter()
        .map(|key| {
            Ok(OneTimePreKey {
                id: key_id(&key["id"])?,
                key: parse_key(&key["key"])?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let bundle = PublicBundle {
        device: DevicePlatform::parse(&value["deviceId"])?,
        registration_id: registration(&pre["iKey"]["id"])?,
        identity: parse_key(&pre["iKey"]["key"])?,
        signed_prekey_id: key_id(&pre["spKey"]["id"])?,
        signed_prekey: parse_key(&pre["spKey"]["key"])?,
        signature: Signature::parse(pre["signature"].as_str().ok_or(SignalError::Invalid)?)?,
        one_time_prekeys,
    };
    bundle.validate()?;
    Ok(bundle)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Command {
    SubmitBundle = 540,
    GetBundle = 541,
    SubmitPrekeys = 542,
    Init = 543,
    InitAck = 544,
    DeleteKey = 547,
    DeleteNotice = 548,
    RequestPrekeys = 549,
    Compatible = 560,
    InitGroup = 561,
    CompatibleGroup = 562,
    Metadata = 563,
    OwnKey = 564,
}
#[derive(Clone, Copy)]
pub enum DeviceSelection {
    All,
    One(DevicePlatform),
}
impl DeviceSelection {
    fn wire(self) -> i8 {
        match self {
            Self::All => -1,
            Self::One(d) => d.wire() as i8,
        }
    }
}
#[derive(Clone, Copy)]
pub enum GroupInitType {
    Create,
    Reinit,
    Response,
}
impl GroupInitType {
    fn wire(self) -> u8 {
        match self {
            Self::Create => 0,
            Self::Reinit => 1,
            Self::Response => 2,
        }
    }
}
#[derive(Clone, Copy)]
pub struct BundleOptions {
    pub reinit: bool,
    pub notify: bool,
    pub auto_upgrade: bool,
}

/// Contains account-sensitive public keys/ciphertext; deliberately no Debug/Serialize.
/// `wire_data` is the only explicit transport serialization boundary.
pub struct SignalRequest {
    scope: Scope,
    command: Command,
    request_id: String,
    data: Value,
}
impl SignalRequest {
    fn new(scope: &Scope, sequence: u64, command: Command, data: Value) -> Result<Self> {
        if sequence > MAX_SAFE_INTEGER {
            return Err(SignalError::Resource);
        }
        bounded_json(&data)?;
        Ok(Self {
            scope: scope.clone(),
            command,
            request_id: format!("signal_req_{sequence}"),
            data,
        })
    }
    pub fn command(&self) -> Command {
        self.command
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn wire_data(&self, active: &Scope) -> Result<Value> {
        self.scope.require(active)?;
        Ok(json!({"reqId":self.request_id,"data":self.data}))
    }
    pub fn frame(&self, active: &Scope) -> Result<Vec<u8>> {
        let data = self.wire_data(active)?;
        bounded_json(&data)?;
        let mut out = vec![
            1,
            self.command as u16 as u8,
            ((self.command as u16) >> 8) as u8,
            0,
        ];
        out.extend(serde_json::to_vec(&data).map_err(|_| SignalError::Invalid)?);
        Ok(out)
    }
    /// Accept a decoded, authenticated WSS frame, never an unbound raw HTTP body.
    pub fn reply(&self, active: &Scope, command: u16, reply: &Value) -> Result<Value> {
        self.scope.require(active)?;
        bounded_json(reply)?;
        if command != self.command as u16
            || reply.pointer("/data/reqId").and_then(Value::as_str) != Some(self.request_id())
        {
            return Err(SignalError::Correlation);
        }
        service_code(reply.get("error_code").ok_or(SignalError::Invalid)?)?;
        let data = reply.pointer("/data/data").ok_or(SignalError::Invalid)?;
        if let Some(error) = data.get("err") {
            service_code(error)?;
        }
        Ok(data.clone())
    }
}
fn service_code(value: &Value) -> Result<()> {
    match value.as_i64() {
        Some(0) => Ok(()),
        Some(102 | 600) => Err(SignalError::Authentication),
        Some(code) => Err(SignalError::Service(code)),
        None => Err(SignalError::Invalid),
    }
}
pub fn submit_bundle(
    scope: &Scope,
    sequence: u64,
    bundle: &PublicBundle,
    delete_sessions: bool,
) -> Result<SignalRequest> {
    SignalRequest::new(
        scope,
        sequence,
        Command::SubmitBundle,
        json!({"delSS":u8::from(delete_sessions),"key":bundle.wire()?}),
    )
}
pub fn get_bundle(
    scope: &Scope,
    sequence: u64,
    peer: &str,
    device: DeviceSelection,
    options: BundleOptions,
) -> Result<SignalRequest> {
    uid(peer)?;
    SignalRequest::new(
        scope,
        sequence,
        Command::GetBundle,
        json!({"uids":[{"uid":peer,"deviceId":device.wire()}],"isReInit":u8::from(options.reinit),"msgInfoNoti":u8::from(options.notify),"isAutoUpgrade":u8::from(options.auto_upgrade)}),
    )
}
pub fn replenish(
    scope: &Scope,
    sequence: u64,
    device: DevicePlatform,
    registration_id: u16,
    prekeys: &[OneTimePreKey],
) -> Result<SignalRequest> {
    registration(&json!(registration_id))?;
    if prekeys.is_empty() || prekeys.len() > MAX_PREKEYS {
        return Err(SignalError::Resource);
    }
    let mut seen = BTreeSet::new();
    for key in prekeys {
        if key.id == 0 || !seen.insert(key.id) {
            return Err(SignalError::Duplicate);
        }
    }
    SignalRequest::new(
        scope,
        sequence,
        Command::SubmitPrekeys,
        json!({"deviceId":device.wire(),"id":registration_id,"opKeys":prekeys.iter().map(|key|json!({"id":key.id,"key":key.key.wire()})).collect::<Vec<_>>()}),
    )
}
pub fn own_key(scope: &Scope, sequence: u64, device: DevicePlatform) -> Result<SignalRequest> {
    SignalRequest::new(
        scope,
        sequence,
        Command::OwnKey,
        json!({"platformType":device.wire()}),
    )
}
pub fn compatible(scope: &Scope, sequence: u64, peer: &str) -> Result<SignalRequest> {
    uid(peer)?;
    SignalRequest::new(scope, sequence, Command::Compatible, json!({"uids":[peer]}))
}
pub fn compatible_group(
    scope: &Scope,
    sequence: u64,
    group: &str,
    members: &[String],
    enable: bool,
) -> Result<SignalRequest> {
    uid(group)?;
    unique_uids(members)?;
    SignalRequest::new(
        scope,
        sequence,
        Command::CompatibleGroup,
        json!({"uids":members,"groupId":group,"enableEE":u8::from(enable)}),
    )
}
pub fn delete_keys(
    scope: &Scope,
    sequence: u64,
    key_id: u16,
    targets: &[String],
    groups: bool,
    timestamp: u64,
) -> Result<SignalRequest> {
    registration(&json!(key_id))?;
    unique_uids(targets)?;
    if timestamp > MAX_SAFE_INTEGER {
        return Err(SignalError::Invalid);
    }
    let mut data = json!({"keyId":key_id,"ts":timestamp});
    data[if groups { "groupIds" } else { "uids" }] = json!(targets);
    SignalRequest::new(scope, sequence, Command::DeleteKey, data)
}
fn unique_uids(items: &[String]) -> Result<()> {
    if items.is_empty() || items.len() > MAX_PAGE_ITEMS {
        return Err(SignalError::Resource);
    }
    let mut seen = BTreeSet::new();
    for item in items {
        uid(item)?;
        if !seen.insert(item) {
            return Err(SignalError::Duplicate);
        }
    }
    Ok(())
}

/// A syntactically checked peer bundle still requires engine signature verification,
/// identity approval and one transaction before creating any usable session.
pub struct FetchedBundles {
    pub peer: String,
    pub bundles: Vec<PublicBundle>,
}
pub fn bundles_reply(
    request: &SignalRequest,
    active: &Scope,
    reply: &Value,
    peer: &str,
    selection: DeviceSelection,
) -> Result<FetchedBundles> {
    if request.command != Command::GetBundle {
        return Err(SignalError::Correlation);
    }
    if request.data.pointer("/uids/0/uid").and_then(Value::as_str) != Some(peer)
        || request
            .data
            .pointer("/uids/0/deviceId")
            .and_then(Value::as_i64)
            != Some(selection.wire() as i64)
    {
        return Err(SignalError::Correlation);
    }
    let data = request.reply(active, 541, reply)?;
    let keys = bounded_array(&data["keys"], 1)?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in keys {
        if item["uid"].as_str() != Some(peer) {
            return Err(SignalError::Scope);
        }
        for key in bounded_array(&item["keys"], 3)? {
            let bundle = parse_bundle(key)?;
            if let DeviceSelection::One(device) = selection {
                if bundle.device != device {
                    return Err(SignalError::Scope);
                }
            }
            if !seen.insert(bundle.device) {
                return Err(SignalError::Duplicate);
            }
            out.push(bundle)
        }
    }
    Ok(FetchedBundles {
        peer: peer.into(),
        bundles: out,
    })
}
pub enum OwnKey {
    Missing,
    Present {
        registration_id: u16,
        /// The real service omits `submitTs` (the original reads it as `undefined`); never invent one.
        submitted_at: Option<u64>,
    },
}
pub fn own_key_reply(request: &SignalRequest, active: &Scope, reply: &Value) -> Result<OwnKey> {
    if request.command != Command::OwnKey {
        return Err(SignalError::Correlation);
    }
    // -1 means "not uploaded" only inside the correlated command's inner data.
    // A rejected outer envelope must never authorize generating/replacing keys.
    let data = request.reply(active, 564, reply);
    match data {
        Ok(data) => Ok(OwnKey::Present {
            registration_id: registration(&data["keyId"])?,
            submitted_at: match data.get("submitTs") {
                None | Some(Value::Null) => None,
                Some(value) => Some(number(value)?),
            },
        }),
        Err(SignalError::Service(-1))
            if reply["error_code"].as_i64() == Some(0)
                && reply.pointer("/data/data/err").and_then(Value::as_i64) == Some(-1) =>
        {
            Ok(OwnKey::Missing)
        }
        Err(error) => Err(error),
    }
}

/// An opaque server cursor retains its JSON type; it is never invented from a UID.
#[derive(Clone, PartialEq, Eq)]
pub struct Cursor {
    value: Value,
    canonical: String,
}
impl Cursor {
    pub fn parse(value: &Value) -> Result<Self> {
        let text = match value {
            Value::String(s)
                if !s.is_empty() && s.len() <= 24 && s.bytes().all(|b| b.is_ascii_digit()) =>
            {
                s.clone()
            }
            Value::Number(_) => number(value)?.to_string(),
            _ => return Err(SignalError::Invalid),
        };
        let canonical = text.trim_start_matches('0');
        let canonical = if canonical.is_empty() { "0" } else { canonical }.to_owned();
        Ok(Self {
            value: value.clone(),
            canonical,
        })
    }
    fn advances(&self, previous: &Self) -> bool {
        self.canonical.len() > previous.canonical.len()
            || (self.canonical.len() == previous.canonical.len()
                && self.canonical > previous.canonical)
    }
}
pub fn metadata(scope: &Scope, sequence: u64, after: Option<&Cursor>) -> Result<SignalRequest> {
    let mut data = json!({"reqIid":1,"first":after.is_none()});
    if let Some(after) = after {
        data["lastId"] = after.value.clone()
    }
    SignalRequest::new(scope, sequence, Command::Metadata, data)
}
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceMetadata {
    pub device: DevicePlatform,
    pub iid: u16,
}
#[derive(Clone, PartialEq, Eq)]
pub struct ThreadMetadata {
    pub peer: String,
    pub devices: Vec<DeviceMetadata>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct PendingInit {
    pub peer: String,
    pub device: DevicePlatform,
    pub timestamp: u64,
}
pub struct MetadataSnapshot {
    pub scope: Scope,
    pub threads: Vec<ThreadMetadata>,
    pub enabled_uids: Vec<String>,
    pub waiting_uids: Vec<String>,
    pub pending: Vec<PendingInit>,
}
/// Partial pages confer no legacy-send permission and are not usable snapshots.
pub struct MetadataPages {
    scope: Scope,
    pages: usize,
    cursor: Option<Cursor>,
    complete: bool,
    threads: BTreeMap<String, ThreadMetadata>,
    uids: BTreeSet<String>,
    waiting: BTreeSet<String>,
    pending: BTreeMap<(String, DevicePlatform), PendingInit>,
}
impl MetadataPages {
    pub fn new(scope: &Scope) -> Self {
        Self {
            scope: scope.clone(),
            pages: 0,
            cursor: None,
            complete: false,
            threads: BTreeMap::new(),
            uids: BTreeSet::new(),
            waiting: BTreeSet::new(),
            pending: BTreeMap::new(),
        }
    }
    pub fn request(&self, sequence: u64) -> Result<SignalRequest> {
        if self.complete {
            return Err(SignalError::OutOfOrder);
        }
        metadata(&self.scope, sequence, self.cursor.as_ref())
    }
    pub fn accept(
        &mut self,
        request: &SignalRequest,
        active: &Scope,
        reply: &Value,
    ) -> Result<bool> {
        self.scope.require(active)?;
        if self.complete || self.pages >= MAX_PAGES {
            return Err(SignalError::Resource);
        }
        if request.command != Command::Metadata
            || request.data["first"].as_bool() != Some(self.cursor.is_none())
            || request.data.get("lastId") != self.cursor.as_ref().map(|c| &c.value)
        {
            return Err(SignalError::Correlation);
        }
        let data = request.reply(active, 563, reply)?;
        let more = data["hasMore"].as_bool().ok_or(SignalError::Incomplete)?;
        let threads = bounded_array(&data["threads"], MAX_PAGE_ITEMS)?;
        let uids = bounded_array(&data["uids"], MAX_PAGE_ITEMS)?;
        let waiting = bounded_array(&data["waitUids"], MAX_PAGE_ITEMS)?;
        let pending = bounded_array(&data["pendingUids"], MAX_PAGE_ITEMS)?;
        let next = if more {
            let next = Cursor::parse(data.get("lastId").ok_or(SignalError::Incomplete)?)?;
            if self.cursor.as_ref().is_some_and(|old| !next.advances(old)) {
                return Err(SignalError::OutOfOrder);
            }
            Some(next)
        } else {
            None
        };
        // Parse into temporary bounded state; a malformed final item cannot mutate progress.
        let mut parsed_threads = BTreeMap::new();
        let mut parsed_pending = BTreeMap::new();
        for thread in threads {
            let peer = thread["uid"].as_str().ok_or(SignalError::Invalid)?;
            uid(peer)?;
            let mut devices = Vec::new();
            let mut seen = BTreeSet::new();
            for device in bounded_array(&thread["devices"], 3)? {
                let platform = DevicePlatform::parse(&device["di"])?;
                if !seen.insert(platform) {
                    return Err(SignalError::Duplicate);
                }
                devices.push(DeviceMetadata {
                    device: platform,
                    iid: registration(&device["iid"])?,
                })
            }
            let item = ThreadMetadata {
                peer: peer.into(),
                devices,
            };
            if parsed_threads
                .insert(peer.to_owned(), item.clone())
                .is_some()
                || self.threads.get(peer).is_some_and(|old| *old != item)
            {
                return Err(SignalError::Duplicate);
            }
        }
        let parse_uids = |values: &[Value]| -> Result<BTreeSet<String>> {
            let mut parsed = BTreeSet::new();
            for value in values {
                let value = value.as_str().ok_or(SignalError::Invalid)?;
                uid(value)?;
                if !parsed.insert(value.to_owned()) {
                    return Err(SignalError::Duplicate);
                }
            }
            Ok(parsed)
        };
        let parsed_uids = parse_uids(uids)?;
        let parsed_waiting = parse_uids(waiting)?;
        for item in pending {
            let peer = item["uid"].as_str().ok_or(SignalError::Invalid)?;
            uid(peer)?;
            let device = DevicePlatform::parse(&item["deviceId"])?;
            let value = PendingInit {
                peer: peer.into(),
                device,
                timestamp: number(&item["ts"])?,
            };
            let key = (peer.to_owned(), device);
            if parsed_pending.insert(key.clone(), value.clone()).is_some()
                || self.pending.get(&key).is_some_and(|old| *old != value)
            {
                return Err(SignalError::Duplicate);
            }
        }
        if self.threads.len()
            + parsed_threads.len()
            + self.uids.len()
            + parsed_uids.len()
            + self.waiting.len()
            + parsed_waiting.len()
            + self.pending.len()
            + parsed_pending.len()
            > MAX_METADATA_ITEMS
        {
            return Err(SignalError::Resource);
        }
        self.threads.extend(parsed_threads);
        self.uids.extend(parsed_uids);
        self.waiting.extend(parsed_waiting);
        self.pending.extend(parsed_pending);
        self.cursor = next;
        self.complete = !more;
        self.pages += 1;
        Ok(self.complete)
    }
    pub fn finish(self, active: &Scope) -> Result<MetadataSnapshot> {
        self.scope.require(active)?;
        if !self.complete {
            return Err(SignalError::Incomplete);
        }
        Ok(MetadataSnapshot {
            scope: self.scope,
            threads: self.threads.into_values().collect(),
            enabled_uids: self.uids.into_iter().collect(),
            waiting_uids: self.waiting.into_iter().collect(),
            pending: self.pending.into_values().collect(),
        })
    }
}

/// Engine output only. Structural checks here cannot replace MAC/signature/identity
/// verification or the adapter's atomic ratchet/outbox transaction. No Debug/Serialize.
pub struct EngineCiphertext {
    address: Address,
    remote_iid: u16,
    message: String,
    properties: Option<String>,
}
impl EngineCiphertext {
    /// Called only by the future verified engine adapter, after checking its scoped
    /// operation/revision and committing the exact ciphertext with its ratchet delta.
    #[cfg(test)]
    fn from_verified_engine(
        address: Address,
        remote_iid: u16,
        message: String,
        properties: Option<String>,
    ) -> Result<Self> {
        registration(&json!(remote_iid))?;
        validate_cipher(&message, false)?;
        if let Some(p) = &properties {
            validate_cipher(p, false)?
        }
        Ok(Self {
            address,
            remote_iid,
            message,
            properties,
        })
    }
}
fn validate_cipher(value: &str, group: bool) -> Result<()> {
    let bytes = base64_bytes(value, MAX_CIPHER_BYTES)?;
    let kind = bytes.first().copied().ok_or(SignalError::Invalid)?;
    if (if group {
        kind != 4
    } else {
        !matches!(kind, 1 | 3)
    }) || bytes.get(1) != Some(&0x33)
        || bytes.len() < if group { 67 } else { 11 }
    {
        return Err(SignalError::Invalid);
    }
    Ok(())
}
pub fn init(
    scope: &Scope,
    sequence: u64,
    peer: &str,
    ciphertexts: &[EngineCiphertext],
    enabled: bool,
) -> Result<SignalRequest> {
    uid(peer)?;
    if ciphertexts.is_empty() || ciphertexts.len() > 3 {
        return Err(SignalError::Resource);
    }
    let mut devices = BTreeSet::new();
    let mut records = Vec::new();
    for ciphertext in ciphertexts {
        scope.require(ciphertext.address.scope())?;
        if ciphertext.address.peer() != peer {
            return Err(SignalError::Scope);
        }
        if !devices.insert(ciphertext.address.device()) {
            return Err(SignalError::Duplicate);
        }
        let raw = base64_bytes(&ciphertext.message, MAX_CIPHER_BYTES)?;
        if raw[0] != 3 {
            return Err(SignalError::OutOfOrder);
        }
        records
            .push(json!({"deviceId":ciphertext.address.device().wire(),"data":ciphertext.message}))
    }
    SignalRequest::new(
        scope,
        sequence,
        Command::Init,
        json!({"type":u8::from(enabled),"uidTo":peer,"data":records}),
    )
}
#[cfg(test)]
fn init_ack(
    scope: &Scope,
    sequence: u64,
    ciphertext: &EngineCiphertext,
    timestamp: u64,
) -> Result<SignalRequest> {
    scope.require(ciphertext.address.scope())?;
    if timestamp > MAX_SAFE_INTEGER {
        return Err(SignalError::Invalid);
    }
    SignalRequest::new(
        scope,
        sequence,
        Command::InitAck,
        json!({"type":1,"uidTo":ciphertext.address.peer(),"data":[{"deviceId":ciphertext.address.device().wire(),"ts":timestamp,"data":ciphertext.message}]}),
    )
}
pub fn init_group(
    scope: &Scope,
    sequence: u64,
    group: &str,
    ciphertext: &EngineCiphertext,
    kind: GroupInitType,
    timestamp: Option<u64>,
) -> Result<SignalRequest> {
    scope.require(ciphertext.address.scope())?;
    uid(group)?;
    if timestamp.is_some_and(|t| t > MAX_SAFE_INTEGER) {
        return Err(SignalError::Invalid);
    }
    let mut item = json!({"deviceId":ciphertext.address.device().wire(),"data":ciphertext.message});
    if let Some(timestamp) = timestamp {
        item["ts"] = json!(timestamp);
    }
    SignalRequest::new(
        scope,
        sequence,
        Command::InitGroup,
        json!({"type":kind.wire(),"uidTo":ciphertext.address.peer(),"data":[item],"tId":group}),
    )
}
/// Contains only ciphertext already accepted by the engine adapter; no raw text API.
pub struct SmsParameters {
    pub path: &'static str,
    pub metric_command: u16,
    parameters: Value,
}
impl SmsParameters {
    pub fn parameters(&self) -> &Value {
        &self.parameters
    }
    /// Persist these exact bytes with stable client ID; session AES/form encoding
    /// is a separate authenticated HTTP adapter step, never a legacy fallback.
    pub fn exact_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.parameters).map_err(|_| SignalError::Invalid)
    }
}
fn client_id(id: u64) -> Result<()> {
    if id == 0 || id > MAX_SAFE_INTEGER {
        Err(SignalError::Invalid)
    } else {
        Ok(())
    }
}
pub fn personal_sms(
    scope: &Scope,
    thread: &str,
    id: u64,
    sender_iid: Option<u16>,
    ciphertexts: &[EngineCiphertext],
) -> Result<SmsParameters> {
    uid(thread)?;
    client_id(id)?;
    if ciphertexts.is_empty() || ciphertexts.len() > 6 {
        return Err(SignalError::Resource);
    }
    if let Some(iid) = sender_iid {
        registration(&json!(iid))?;
    }
    let mut devices = BTreeSet::new();
    let mut records = Vec::new();
    let mut has_peer = false;
    let local_epoch = &ciphertexts[0].address.local_epoch;
    for ciphertext in ciphertexts {
        scope.require(ciphertext.address.scope())?;
        let address = &ciphertext.address;
        if &address.local_epoch != local_epoch {
            return Err(SignalError::Scope);
        }
        if address.peer() != thread && address.peer() != scope.account() {
            return Err(SignalError::Scope);
        }
        has_peer |= address.peer() == thread;
        if !devices.insert((address.peer(), address.device())) {
            return Err(SignalError::Duplicate);
        }
        let mut ext = json!({"version":0,"compat_version":0});
        if ciphertext.remote_iid != 0 {
            ext["iid"] = json!(ciphertext.remote_iid);
        }
        if let Some(iid) = sender_iid.filter(|iid| *iid != 0) {
            ext["sender_iid"] = json!(iid)
        }
        let mut record = json!({"message":ciphertext.message,"toId":address.peer(),"platform":address.device().wire(),"e2eeExt":serde_json::to_string(&ext).map_err(|_|SignalError::Invalid)?});
        if let Some(properties) = &ciphertext.properties {
            record["textProperties"] = json!(properties)
        }
        records.push(record);
    }
    if !has_peer {
        return Err(SignalError::Scope);
    }
    let parameters = json!({"listParams":serde_json::to_string(&records).map_err(|_|SignalError::Invalid)?,"destType":3,"threadId":thread,"clientId":id});
    bounded_json(&parameters)?;
    Ok(SmsParameters {
        path: "/api/e2ee/pc/message/sms",
        metric_command: 16000,
        parameters,
    })
}
pub struct EngineGroupCiphertext {
    scope: Scope,
    group: String,
    local_epoch: String,
    message: String,
    properties: Option<String>,
}
impl EngineGroupCiphertext {
    #[cfg(test)]
    fn from_verified_engine(
        scope: &Scope,
        group: &str,
        local_epoch: &str,
        message: String,
        properties: Option<String>,
    ) -> Result<Self> {
        uid(group)?;
        epoch(local_epoch)?;
        validate_cipher(&message, true)?;
        if let Some(properties) = &properties {
            validate_cipher(properties, true)?
        }
        Ok(Self {
            scope: scope.clone(),
            group: group.into(),
            local_epoch: local_epoch.into(),
            message,
            properties,
        })
    }
}
pub fn group_sms(
    scope: &Scope,
    local_epoch: &str,
    id: u64,
    visibility: u64,
    ciphertext: &EngineGroupCiphertext,
) -> Result<SmsParameters> {
    scope.require(&ciphertext.scope)?;
    client_id(id)?;
    if local_epoch != ciphertext.local_epoch || visibility > MAX_SAFE_INTEGER {
        return Err(SignalError::Scope);
    }
    // Group ciphertexts have sender-key state, not a fabricated per-device IID.
    let mut parameters = json!({"message":ciphertext.message,"grid":ciphertext.group,"clientId":id,"visibility":visibility,"e2eeExt":"{\"version\":0,\"compat_version\":0}"});
    if let Some(properties) = &ciphertext.properties {
        parameters["textProperties"] = json!(properties)
    }
    bounded_json(&parameters)?;
    Ok(SmsParameters {
        path: "/api/e2ee/pc/group/sms",
        metric_command: 16100,
        parameters,
    })
}

/// Socket queue identity. These are receive routes, never permission to ACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueKind {
    SessionsPc,
    SessionsWeb,
    DeliveryDirect,
    DeliveryGroup,
    DirectMessages,
    GroupMessages,
    OfflineGroupMessages,
}
impl QueueKind {
    pub fn from_command(command: u16) -> Result<Self> {
        match command {
            516 => Ok(Self::SessionsPc),
            517 => Ok(Self::SessionsWeb),
            518 => Ok(Self::DeliveryDirect),
            519 => Ok(Self::DeliveryGroup),
            551 => Ok(Self::DirectMessages),
            552 => Ok(Self::GroupMessages),
            514 => Ok(Self::OfflineGroupMessages),
            _ => Err(SignalError::Unsupported),
        }
    }
}
/// Authenticated transport is not cryptographic verification. This descriptor
/// remains encrypted/untrusted until the engine and database transaction succeed.
#[derive(Clone)]
pub struct SessionEvent {
    pub server_id: String,
    pub source_command: u16,
    pub command: Command,
    pub sender: String,
    pub device: DevicePlatform,
    pub timestamp: u64,
    payload: Value,
}
impl SessionEvent {
    pub fn payload(&self) -> &Value {
        &self.payload
    }
}
pub struct QueueEnvelope {
    pub scope: Scope,
    pub kind: QueueKind,
    pub more: bool,
    pub last_action_id: Option<Cursor>,
    pub sessions: Vec<SessionEvent>,
    // Retain complete records for the typed message adapter; never plaintext/UI.
    records: Vec<Value>,
}
impl QueueEnvelope {
    pub fn records(&self) -> &[Value] {
        &self.records
    }
}
fn session_command(command: u64) -> Result<Command> {
    match command {
        543 | 545 => Ok(Command::Init),
        544 => Ok(Command::InitAck),
        548 => Ok(Command::DeleteNotice),
        549 => Ok(Command::RequestPrekeys),
        561 => Ok(Command::InitGroup),
        // Disable/downgrade/stale-failure events need explicit future semantics.
        // No caller may discard these and advance a durable cursor.
        _ => Err(SignalError::Unsupported),
    }
}
/// Input must be the decoded inner-success envelope of the authenticated socket.
/// No cursor is committed and no receipt/ACK is constructed by this parser.
pub fn queue_envelope(
    scope: &Scope,
    active: &Scope,
    command: u16,
    reply: &Value,
) -> Result<QueueEnvelope> {
    scope.require(active)?;
    bounded_json(reply)?;
    let kind = QueueKind::from_command(command)?;
    service_code(reply.get("error_code").ok_or(SignalError::Invalid)?)?;
    let data = reply
        .get("data")
        .filter(|x| x.is_object())
        .ok_or(SignalError::Invalid)?;
    let more = match data.get("more") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(SignalError::Invalid),
    };
    let last_action_id = data.get("lastActionId").map(Cursor::parse).transpose()?;
    let mut sessions = Vec::new();
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    match kind {
        QueueKind::SessionsPc | QueueKind::SessionsWeb => {
            for item in bounded_array(&data["eesession"], MAX_PAGE_ITEMS)? {
                let event_command = session_command(number(&item["cmd"])?)?;
                let server_id = item["msgId"].as_str().ok_or(SignalError::Invalid)?;
                uid(server_id)?;
                if !ids.insert(server_id.to_owned()) {
                    return Err(SignalError::Duplicate);
                }
                let payload = item
                    .get("sessionData")
                    .filter(|v| v.is_object())
                    .ok_or(SignalError::Invalid)?;
                let sender = payload["uidFrom"].as_str().ok_or(SignalError::Invalid)?;
                uid(sender)?;
                if let Some(recipient) = payload.get("uidTo") {
                    if recipient.as_str() != Some(scope.account()) {
                        return Err(SignalError::Scope);
                    }
                } else if matches!(
                    event_command,
                    Command::Init | Command::InitAck | Command::InitGroup
                ) {
                    return Err(SignalError::Scope);
                }
                let device = DevicePlatform::parse(&payload["deviceId"])?;
                let timestamp = payload
                    .get("reqServerTs")
                    .or_else(|| payload.get("ts"))
                    .map(number)
                    .transpose()?
                    .unwrap_or(0);
                match event_command {
                    Command::Init => {
                        let value = payload["data"].as_str().ok_or(SignalError::Invalid)?;
                        validate_cipher(value, false)?;
                        if base64_bytes(value, MAX_CIPHER_BYTES)?[0] != 3 {
                            return Err(SignalError::OutOfOrder);
                        }
                    }
                    Command::InitAck => match payload["type"].as_i64() {
                        Some(1) => validate_cipher(
                            payload["data"].as_str().ok_or(SignalError::Invalid)?,
                            false,
                        )?,
                        Some(0) => {
                            registration(&payload["iid"])?;
                        }
                        _ => return Err(SignalError::Unsupported),
                    },
                    Command::InitGroup => {
                        let group = payload["threadId"].as_str().ok_or(SignalError::Invalid)?;
                        uid(group)?;
                        validate_cipher(
                            payload["data"].as_str().ok_or(SignalError::Invalid)?,
                            false,
                        )?;
                    }
                    Command::RequestPrekeys => {
                        if !matches!(payload["type"].as_u64(), Some(1 | 2)) {
                            return Err(SignalError::Unsupported);
                        }
                    }
                    Command::DeleteNotice => {
                        if !matches!(payload["deleteType"].as_u64(), Some(0 | 1 | 3)) {
                            return Err(SignalError::Unsupported);
                        }
                    }
                    _ => unreachable!(),
                }
                sessions.push(SessionEvent {
                    server_id: server_id.into(),
                    source_command: number(&item["cmd"])? as u16,
                    command: event_command,
                    sender: sender.into(),
                    device,
                    timestamp,
                    payload: payload.clone(),
                });
            }
        }
        QueueKind::DeliveryDirect | QueueKind::DeliveryGroup => {
            for item in bounded_array(&data["delivereds"], MAX_PAGE_ITEMS)? {
                if !item.is_object() {
                    return Err(SignalError::Invalid);
                }
                // Receipt subtypes are deliberately opaque at this stage. No
                // server acceptance/delivery or retry is inferred from them.
                records.push(item.clone());
            }
        }
        _ => {
            let fields: &[&str] = if matches!(kind, QueueKind::DirectMessages) {
                &["msgs", "pageMsgs"]
            } else {
                &["groupMsgs"]
            };
            for field in fields {
                let Some(items) = data.get(*field) else {
                    continue;
                };
                for item in bounded_array(items, MAX_PAGE_ITEMS)? {
                    let server_id = item["msgId"].as_str().ok_or(SignalError::Invalid)?;
                    uid(server_id)?;
                    if !ids.insert(server_id.to_owned()) {
                        return Err(SignalError::Duplicate);
                    }
                    let sender = item["uidFrom"].as_str().ok_or(SignalError::Invalid)?;
                    // The server uses zero for this account's other device echo.
                    if sender != "0" {
                        uid(sender)?;
                    }
                    let destination = item["idTo"].as_str().ok_or(SignalError::Invalid)?;
                    if matches!(kind, QueueKind::DirectMessages) {
                        if sender != "0"
                            && sender != scope.account()
                            && destination != scope.account()
                        {
                            return Err(SignalError::Scope);
                        }
                        if destination != "0" {
                            uid(destination)?;
                        }
                    } else {
                        uid(destination)?;
                    }
                    DevicePlatform::parse(&item["paramsExt"]["platformType"])?;
                    number(&item["ts"])?;
                    records.push(item.clone());
                }
            }
            if records.len() > MAX_PAGE_ITEMS {
                return Err(SignalError::Resource);
            }
        }
    }
    Ok(QueueEnvelope {
        scope: scope.clone(),
        kind,
        more,
        last_action_id,
        sessions,
        records,
    })
}

/// Source-backed ordinary-text codec. It carries no signature/identity proof.
/// The original sender encrypts raw UTF-8 and skips an empty style string.
/// Thread/client ID belong to the transport envelope, not encrypted plaintext.
pub struct BusinessText {
    scope: Scope,
    thread: String,
    client_id: u64,
    text: String,
}
impl BusinessText {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn thread(&self) -> &str {
        &self.thread
    }
    pub fn client_id(&self) -> u64 {
        self.client_id
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }
}
pub fn encode_personal_business(
    scope: &Scope,
    thread: &str,
    id: u64,
    text: &str,
) -> Result<BusinessText> {
    uid(thread)?;
    client_id(id)?;
    if text.is_empty() || text.len() > crate::domain::MAX_TEXT_BYTES {
        return Err(SignalError::Resource);
    }
    Ok(BusinessText {
        scope: scope.clone(),
        thread: thread.into(),
        client_id: id,
        text: text.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/signal/wire.json")).unwrap()
    }
    fn scope() -> Scope {
        Scope::new("11", 7).unwrap()
    }
    fn ciphertext(kind: &str, peer: &str, platform: DevicePlatform) -> EngineCiphertext {
        let f = fixture();
        EngineCiphertext::from_verified_engine(
            Address::new(&scope(), peer, platform, "1", "2").unwrap(),
            2345,
            f[kind].as_str().unwrap().into(),
            None,
        )
        .unwrap()
    }
    fn response(request: &SignalRequest, data: Value) -> Value {
        json!({"error_code":0,"data":{"reqId":request.request_id(),"data":data}})
    }
    fn normalize_sms(mut value: Value) -> Value {
        if let Some(records) = value.get_mut("listParams") {
            let mut parsed: Value = serde_json::from_str(records.as_str().unwrap()).unwrap();
            for record in parsed.as_array_mut().unwrap() {
                record["e2eeExt"] =
                    serde_json::from_str(record["e2eeExt"].as_str().unwrap()).unwrap();
            }
            *records = parsed;
        } else if let Some(ext) = value.get_mut("e2eeExt") {
            *ext = serde_json::from_str(ext.as_str().unwrap()).unwrap();
        }
        value
    }
    #[test]
    fn thirteen_requests_match_independently_executed_original_bundle() {
        let f = fixture();
        let s = scope();
        let bundle = parse_bundle(&f["requests"][0]["data"]["data"]["key"]).unwrap();
        let initial = ciphertext("prekey", "22", DevicePlatform::Mobile);
        let ack = ciphertext("personal", "22", DevicePlatform::Mobile);
        let requests = vec![
            submit_bundle(&s, 0, &bundle, true).unwrap(),
            get_bundle(
                &s,
                1,
                "22",
                DeviceSelection::All,
                BundleOptions {
                    reinit: true,
                    notify: false,
                    auto_upgrade: true,
                },
            )
            .unwrap(),
            replenish(&s, 2, DevicePlatform::Pc, 1234, &bundle.one_time_prekeys).unwrap(),
            init(&s, 3, "22", &[initial], true).unwrap(),
            init_ack(&s, 4, &ack, 1_700_000_000_000).unwrap(),
            init_group(
                &s,
                5,
                "33",
                &ack,
                GroupInitType::Response,
                Some(1_700_000_000_000),
            )
            .unwrap(),
            own_key(&s, 6, DevicePlatform::Pc).unwrap(),
            compatible(&s, 7, "22").unwrap(),
            compatible_group(&s, 8, "33", &["22".into(), "44".into()], true).unwrap(),
            metadata(&s, 9, None).unwrap(),
            metadata(&s, 10, Some(&Cursor::parse(&json!("100")).unwrap())).unwrap(),
            delete_keys(&s, 11, 1234, &["22".into()], false, 1_700_000_000_000).unwrap(),
            delete_keys(&s, 12, 1234, &["33".into()], true, 1_700_000_000_000).unwrap(),
        ];
        for (request, expected) in requests.iter().zip(f["requests"].as_array().unwrap()) {
            assert_eq!(
                request.command() as u16,
                expected["cmd"].as_u64().unwrap() as u16
            );
            assert_eq!(request.wire_data(&s).unwrap(), expected["data"]);
            let frame = request.frame(&s).unwrap();
            assert_eq!(
                u16::from_le_bytes([frame[1], frame[2]]),
                request.command() as u16
            );
            assert_eq!(frame[3], 0);
            assert_eq!(
                serde_json::from_slice::<Value>(&frame[4..]).unwrap(),
                expected["data"]
            );
        }
    }
    #[test]
    fn personal_and_group_sms_match_original_outer_envelopes_without_raw_cipher_constructor() {
        let f = fixture();
        let s = scope();
        let c = ciphertext("personal", "22", DevicePlatform::Mobile);
        let personal = personal_sms(&s, "22", 1_700_000_000_001, Some(1234), &[c]).unwrap();
        let g = EngineGroupCiphertext::from_verified_engine(
            &s,
            "33",
            "1",
            f["group"].as_str().unwrap().into(),
            None,
        )
        .unwrap();
        let group = group_sms(&s, "1", 1_700_000_000_001, 0, &g).unwrap();
        for (actual, expected) in [personal, group].iter().zip(f["sms"].as_array().unwrap()) {
            assert_eq!(actual.path, expected["route"]["path"]);
            assert_eq!(
                actual.metric_command as u64,
                expected["metric"].as_u64().unwrap()
            );
            assert_eq!(
                normalize_sms(actual.parameters().clone()),
                normalize_sms(expected["parameters"].clone())
            );
        }
        assert!(group_sms(&s, "2", 3, 0, &g).is_err());
    }
    #[test]
    fn opaque_ciphertext_scope_epoch_device_order_and_safe_integer_boundaries() {
        let s = scope();
        let c = ciphertext("personal", "22", DevicePlatform::Mobile);
        assert!(matches!(
            init(&s, 1, "22", &[c], true),
            Err(SignalError::OutOfOrder)
        ));
        let initial = ciphertext("prekey", "22", DevicePlatform::Mobile);
        assert!(matches!(
            init(&Scope::new("44", 7).unwrap(), 1, "22", &[initial], true),
            Err(SignalError::Scope)
        ));
        assert!(Address::new(&s, "22", DevicePlatform::Pc, "0", "2").is_err());
        assert!(Address::new(&s, "22", DevicePlatform::Pc, "1", "02").is_err());
        assert_eq!(DevicePlatform::Pc.wire(), 2);
        assert!(DevicePlatform::parse(&json!(24)).is_err());
        let c = ciphertext("personal", "22", DevicePlatform::Mobile);
        assert!(personal_sms(&s, "22", MAX_SAFE_INTEGER + 1, None, &[c]).is_err());
        let mut zero = ciphertext("personal", "22", DevicePlatform::Mobile);
        zero.remote_iid = 0;
        let sms = personal_sms(&s, "22", MAX_SAFE_INTEGER, Some(0), &[zero]).unwrap();
        assert_eq!(
            normalize_sms(sms.parameters().clone())["listParams"][0]["e2eeExt"],
            json!({"version":0,"compat_version":0})
        );
        let a = ciphertext("personal", "22", DevicePlatform::Mobile);
        let mut b = ciphertext("personal", "11", DevicePlatform::Web);
        b.address.local_epoch = "3".into();
        assert!(matches!(
            personal_sms(&s, "22", 1, None, &[a, b]),
            Err(SignalError::Scope)
        ));
        let raw = STANDARD.encode([1u8, 0x33, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(validate_cipher(&raw, true).is_err());
        assert!(validate_cipher(&STANDARD.encode(vec![3; MAX_CIPHER_BYTES + 1]), false).is_err());
    }
    #[test]
    fn response_correlation_and_inner_only_missing_key_cannot_trigger_unbound_registration() {
        let s = scope();
        let request = own_key(&s, 1, DevicePlatform::Pc).unwrap();
        let ok = response(&request, json!({"keyId":0,"submitTs":1}));
        assert!(matches!(
            own_key_reply(&request, &s, &ok),
            Ok(OwnKey::Present {
                registration_id: 0,
                ..
            })
        ));
        // The real service answers `{err:0,keyId}` without `submitTs` (observed on a live account).
        assert!(matches!(
            own_key_reply(
                &request,
                &s,
                &response(&request, json!({"err":0,"keyId":7}))
            ),
            Ok(OwnKey::Present {
                registration_id: 7,
                submitted_at: None
            })
        ));
        assert!(own_key_reply(
            &request,
            &s,
            &response(&request, json!({"keyId":7,"submitTs":"x"}))
        )
        .is_err());
        assert!(matches!(
            own_key_reply(&request, &s, &response(&request, json!({"err":-1}))),
            Ok(OwnKey::Missing)
        ));
        let mut outer = response(&request, json!({"err":-1}));
        outer["error_code"] = json!(-1);
        assert!(matches!(
            own_key_reply(&request, &s, &outer),
            Err(SignalError::Service(-1))
        ));
        let mut wrong = ok.clone();
        wrong["data"]["reqId"] = json!("signal_req_2");
        assert!(matches!(
            own_key_reply(&request, &s, &wrong),
            Err(SignalError::Correlation)
        ));
        assert!(matches!(
            own_key_reply(&request, &Scope::new("11", 8).unwrap(), &ok),
            Err(SignalError::Scope)
        ));
        assert!(matches!(
            request.reply(&s, 540, &ok),
            Err(SignalError::Correlation)
        ));
        assert!(matches!(
            request.reply(&s, 564, &response(&request, json!({"err":-9}))),
            Err(SignalError::Service(-9))
        ));
    }
    #[test]
    fn bundle_reply_requires_matching_uid_device_and_shape_but_does_not_claim_crypto_verification()
    {
        let s = scope();
        let f = fixture();
        let bundle = f["requests"][0]["data"]["data"]["key"].clone();
        let req = get_bundle(
            &s,
            1,
            "22",
            DeviceSelection::One(DevicePlatform::Pc),
            BundleOptions {
                reinit: false,
                notify: false,
                auto_upgrade: false,
            },
        )
        .unwrap();
        let data = json!({"keys":[{"uid":"22","keys":[bundle]}]});
        let parsed = bundles_reply(
            &req,
            &s,
            &response(&req, data.clone()),
            "22",
            DeviceSelection::One(DevicePlatform::Pc),
        )
        .unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        let mut wrong = data.clone();
        wrong["keys"][0]["uid"] = json!("44");
        assert!(matches!(
            bundles_reply(
                &req,
                &s,
                &response(&req, wrong),
                "22",
                DeviceSelection::One(DevicePlatform::Pc)
            ),
            Err(SignalError::Scope)
        ));
        let mut bad = data.clone();
        bad["keys"][0]["keys"][0]["preKey"]["iKey"]["key"] = json!(STANDARD.encode([0; 33]));
        assert!(bundles_reply(
            &req,
            &s,
            &response(&req, bad),
            "22",
            DeviceSelection::One(DevicePlatform::Pc)
        )
        .is_err());
        let mut duplicate = data;
        duplicate["keys"][0]["keys"]
            .as_array_mut()
            .unwrap()
            .push(bundle);
        assert!(matches!(
            bundles_reply(
                &req,
                &s,
                &response(&req, duplicate),
                "22",
                DeviceSelection::One(DevicePlatform::Pc)
            ),
            Err(SignalError::Duplicate)
        ));
    }
    fn page(more: bool, last: Value) -> Value {
        json!({"err":0,"hasMore":more,"lastId":last,"threads":[],"uids":[],"waitUids":[],"pendingUids":[]})
    }
    #[test]
    fn metadata_requires_complete_correlated_monotonic_bounded_pages_without_partial_permission() {
        let s = scope();
        let mut pages = MetadataPages::new(&s);
        let first = pages.request(1).unwrap();
        let mut data = page(true, json!("100"));
        data["threads"] = json!([{"uid":"22","devices":[{"di":0,"iid":0}]}]);
        assert!(!pages.accept(&first, &s, &response(&first, data)).unwrap());
        let next = pages.request(2).unwrap();
        assert_eq!(next.wire_data(&s).unwrap()["data"]["lastId"], json!("100"));
        let repeated = response(&next, page(true, json!(100)));
        assert!(matches!(
            pages.accept(&next, &s, &repeated),
            Err(SignalError::OutOfOrder)
        ));
        let mut incomplete = page(false, json!(null));
        incomplete.as_object_mut().unwrap().remove("pendingUids");
        assert!(pages
            .accept(&next, &s, &response(&next, incomplete))
            .is_err());
        assert!(pages
            .accept(&next, &s, &response(&next, page(false, json!(null))))
            .unwrap());
        assert!(pages.request(3).is_err());
        let snap = pages.finish(&s).unwrap();
        assert_eq!(snap.threads[0].devices[0].iid, 0);
        assert!(MetadataPages::new(&s).finish(&s).is_err());
        let mut pages = MetadataPages::new(&s);
        pages.pages = MAX_PAGES;
        assert!(matches!(
            pages.accept(&first, &s, &response(&first, page(false, json!(null)))),
            Err(SignalError::Resource)
        ));
        let oversized = json!({"uids":vec!["22";MAX_PAGE_ITEMS+1],"hasMore":false,"threads":[],"waitUids":[],"pendingUids":[]});
        assert!(MetadataPages::new(&s)
            .accept(&first, &s, &response(&first, oversized))
            .is_err());
    }
    #[test]
    fn business_plaintext_is_utf8_not_a_json_envelope_and_limits_precede_crypto() {
        let s = scope();
        let text = "Xin chào 🦀 \"\\\n";
        let encoded = encode_personal_business(&s, "22", 2, text).unwrap();
        assert_eq!(encoded.as_bytes(), text.as_bytes());
        assert_eq!(encoded.thread(), "22");
        assert_eq!(encoded.client_id(), 2);
        assert!(
            encode_personal_business(&s, "22", 2, &"x".repeat(crate::domain::MAX_TEXT_BYTES))
                .is_ok()
        );
        assert!(encode_personal_business(
            &s,
            "22",
            2,
            &"x".repeat(crate::domain::MAX_TEXT_BYTES + 1)
        )
        .is_err());
        assert!(encode_personal_business(&s, "22", 0, text).is_err());
    }
}

/// Offline session queues must finish before dependent encrypted messages are
/// processed. Partial pages cannot expose installable sessions or advance a DB
/// cursor. Replays with identical IDs are retained once; conflicts fail closed.
pub struct SessionPages {
    scope: Scope,
    command: u16,
    pages: usize,
    cursor: Option<Cursor>,
    complete: bool,
    events: Vec<SessionEvent>,
    ids: BTreeMap<String, (u16, Value)>,
}
impl SessionPages {
    pub fn new(scope: &Scope, command: u16) -> Result<Self> {
        if !matches!(command, 516 | 517) {
            return Err(SignalError::Unsupported);
        }
        Ok(Self {
            scope: scope.clone(),
            command,
            pages: 0,
            cursor: None,
            complete: false,
            events: Vec::new(),
            ids: BTreeMap::new(),
        })
    }
    pub fn accept(&mut self, active: &Scope, reply: &Value) -> Result<bool> {
        if self.complete || self.pages >= MAX_PAGES {
            return Err(SignalError::OutOfOrder);
        }
        let parsed = queue_envelope(&self.scope, active, self.command, reply)?;
        if parsed.more && parsed.last_action_id.is_none() {
            return Err(SignalError::Incomplete);
        }
        if parsed.more
            && self
                .cursor
                .as_ref()
                .zip(parsed.last_action_id.as_ref())
                .is_some_and(|(old, next)| !next.advances(old))
        {
            return Err(SignalError::OutOfOrder);
        }
        if self.events.len() + parsed.sessions.len() > MAX_METADATA_ITEMS {
            return Err(SignalError::Resource);
        }
        for event in &parsed.sessions {
            if self
                .ids
                .get(&event.server_id)
                .is_some_and(|(command, payload)| {
                    *command != event.source_command || *payload != event.payload
                })
            {
                return Err(SignalError::Duplicate);
            }
        }
        for event in parsed.sessions {
            if self
                .ids
                .insert(
                    event.server_id.clone(),
                    (event.source_command, event.payload.clone()),
                )
                .is_none()
            {
                self.events.push(event);
            }
        }
        self.pages += 1;
        self.complete = !parsed.more;
        self.cursor = parsed.last_action_id;
        Ok(self.complete)
    }
    pub fn finish(self, active: &Scope) -> Result<Vec<SessionEvent>> {
        self.scope.require(active)?;
        if !self.complete {
            return Err(SignalError::Incomplete);
        }
        Ok(self.events)
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/signal/receive.json")).unwrap()
    }
    fn scope() -> Scope {
        Scope::new("11", 7).unwrap()
    }
    #[test]
    fn original_session_dispatch_holds_partial_pages_then_preserves_order_without_ack() {
        let f = fixture();
        let s = scope();
        let mut pages = SessionPages::new(&s, 516).unwrap();
        assert!(!pages.accept(&s, &f["first"]).unwrap());
        assert!(pages.accept(&s, &f["last"]).unwrap());
        assert!(pages.accept(&s, &f["last"]).is_err());
        let events = pages.finish(&s).unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(
            events.iter().map(|e| e.source_command).collect::<Vec<_>>(),
            vec![543, 544, 549, 548, 549]
        );
        for (event, oracle) in events.iter().zip(f["dispatch"].as_array().unwrap()) {
            assert_eq!(event.server_id, oracle["event"]["msgId"]);
            assert_eq!(event.payload(), &oracle["event"]["sessionData"]);
        }
        assert!(SessionPages::new(&s, 516).unwrap().finish(&s).is_err());
        assert!(SessionPages::new(&s, 551).is_err());
    }
    #[test]
    fn queue_routes_sessions_cipher_messages_and_delivery_descriptors_without_plaintext_or_success_inference(
    ) {
        let f = fixture();
        let s = scope();
        for (command, key, count) in [
            (516, "group", 1),
            (517, "first", 1),
            (551, "messages", 1),
            (552, "groupMessages", 1),
            (514, "groupMessages", 1),
            (518, "deliveries", 1),
            (519, "deliveries", 1),
        ] {
            let envelope = queue_envelope(&s, &s, command, &f[key]).unwrap();
            assert_eq!(envelope.sessions.len() + envelope.records().len(), count);
        }
        assert!(QueueKind::from_command(545).is_err());
        let mut wrong = f["first"].clone();
        wrong["data"]["eesession"][0]["sessionData"]["uidTo"] = json!("44");
        assert!(matches!(
            queue_envelope(&s, &s, 516, &wrong),
            Err(SignalError::Scope)
        ));
        assert!(matches!(
            queue_envelope(&s, &Scope::new("44", 7).unwrap(), 516, &f["first"]),
            Err(SignalError::Scope)
        ));
        let mut unknown = f["first"].clone();
        unknown["data"]["eesession"][0]["cmd"] = json!(580);
        assert!(matches!(
            queue_envelope(&s, &s, 516, &unknown),
            Err(SignalError::Unsupported)
        ));
        let mut malformed = f["first"].clone();
        malformed["data"]["eesession"][0]["sessionData"]["data"] = json!("not ciphertext");
        assert!(queue_envelope(&s, &s, 516, &malformed).is_err());
        let mut mixed = f["messages"].clone();
        mixed["data"]["msgs"][0]["idTo"] = json!("44");
        assert!(matches!(
            queue_envelope(&s, &s, 551, &mixed),
            Err(SignalError::Scope)
        ));
    }
    #[test]
    fn duplicate_cursor_conflict_oversize_and_out_of_order_init_never_mutate_pending_pages() {
        let f = fixture();
        let s = scope();
        let mut pages = SessionPages::new(&s, 516).unwrap();
        pages.accept(&s, &f["first"]).unwrap();
        assert!(matches!(
            pages.accept(&s, &f["first"]),
            Err(SignalError::OutOfOrder)
        ));
        let mut invalid = f["last"].clone();
        invalid["data"]["eesession"][0]["msgId"] = json!("100");
        assert!(matches!(
            pages.accept(&s, &invalid),
            Err(SignalError::Duplicate)
        ));
        assert!(pages.accept(&s, &f["last"]).unwrap());
        assert_eq!(pages.finish(&s).unwrap().len(), 5);
        let mut invalid = f["first"].clone();
        invalid["data"]["eesession"][0]["sessionData"]["data"] =
            fixture()["last"]["data"]["eesession"][0]["sessionData"]["data"].clone();
        assert!(matches!(
            queue_envelope(&s, &s, 516, &invalid),
            Err(SignalError::OutOfOrder)
        ));
        let mut huge = f["first"].clone();
        huge["data"]["eesession"] = json!(vec![
            f["first"]["data"]["eesession"][0].clone();
            MAX_PAGE_ITEMS + 1
        ]);
        assert!(matches!(
            queue_envelope(&s, &s, 516, &huge),
            Err(SignalError::Resource)
        ));
        let mut missing = f["first"].clone();
        missing["data"]
            .as_object_mut()
            .unwrap()
            .remove("lastActionId");
        assert!(matches!(
            SessionPages::new(&s, 516).unwrap().accept(&s, &missing),
            Err(SignalError::Incomplete)
        ));
    }
}
