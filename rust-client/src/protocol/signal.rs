//! Pure Signal transport contracts derived from the repository's own PC bundle.
//! No networking, key generation, signature verification, persistence or ACK side effects.
//! Public-key shape checks are NOT identity/signature verification. See PROTOCOL_SIGNAL.md.
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
    registration_generation: u64,
}
impl Address {
    pub fn new(
        scope: &Scope,
        peer: &str,
        device: DevicePlatform,
        registration_generation: u64,
    ) -> Result<Self> {
        uid(peer)?;
        if registration_generation == 0 {
            return Err(SignalError::Scope);
        }
        Ok(Self {
            scope: scope.clone(),
            peer: peer.into(),
            device,
            registration_generation,
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
    pub fn registration_generation(&self) -> u64 {
        self.registration_generation
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
    if timestamp == 0 || timestamp > MAX_SAFE_INTEGER {
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
        submitted_at: u64,
    },
}
pub fn own_key_reply(request: &SignalRequest, active: &Scope, reply: &Value) -> Result<OwnKey> {
    if request.command != Command::OwnKey {
        return Err(SignalError::Correlation);
    }
    match request.reply(active, 564, reply) {
        Ok(data) => Ok(OwnKey::Present {
            registration_id: registration(&data["keyId"])?,
            submitted_at: number(&data["submitTs"])?,
        }),
        Err(SignalError::Service(-1)) => Ok(OwnKey::Missing),
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
        cipher(&message, false)?;
        if let Some(p) = &properties {
            cipher(p, false)?
        }
        Ok(Self {
            address,
            remote_iid,
            message,
            properties,
        })
    }
}
fn cipher(value: &str, group: bool) -> Result<()> {
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
pub fn init_ack(
    scope: &Scope,
    sequence: u64,
    ciphertext: &EngineCiphertext,
    timestamp: u64,
) -> Result<SignalRequest> {
    scope.require(ciphertext.address.scope())?;
    if timestamp == 0 || timestamp > MAX_SAFE_INTEGER {
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
    timestamp: u64,
) -> Result<SignalRequest> {
    scope.require(ciphertext.address.scope())?;
    uid(group)?;
    if timestamp == 0 || timestamp > MAX_SAFE_INTEGER {
        return Err(SignalError::Invalid);
    }
    SignalRequest::new(
        scope,
        sequence,
        Command::InitGroup,
        json!({"type":kind.wire(),"uidTo":ciphertext.address.peer(),"data":[{"deviceId":ciphertext.address.device().wire(),"data":ciphertext.message,"ts":timestamp}],"tId":group}),
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
        registration(&json!(iid))?
    }
    let mut devices = BTreeSet::new();
    let mut records = Vec::new();
    let mut has_peer = false;
    for ciphertext in ciphertexts {
        scope.require(ciphertext.address.scope())?;
        let address = &ciphertext.address;
        if address.peer() != thread && address.peer() != scope.account() {
            return Err(SignalError::Scope);
        }
        has_peer |= address.peer() == thread;
        if !devices.insert((address.peer(), address.device())) {
            return Err(SignalError::Duplicate);
        }
        let mut ext = json!({"version":0,"compat_version":0,"iid":ciphertext.remote_iid});
        if let Some(iid) = sender_iid {
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
    local_registration_generation: u64,
    message: String,
    properties: Option<String>,
}
impl EngineGroupCiphertext {
    #[cfg(test)]
    fn from_verified_engine(
        scope: &Scope,
        group: &str,
        local_registration_generation: u64,
        message: String,
        properties: Option<String>,
    ) -> Result<Self> {
        uid(group)?;
        if local_registration_generation == 0 {
            return Err(SignalError::Scope);
        }
        cipher(&message, true)?;
        if let Some(properties) = &properties {
            cipher(properties, true)?
        }
        Ok(Self {
            scope: scope.clone(),
            group: group.into(),
            local_registration_generation,
            message,
            properties,
        })
    }
}
pub fn group_sms(
    scope: &Scope,
    local_registration_generation: u64,
    id: u64,
    visibility: u64,
    ciphertext: &EngineGroupCiphertext,
) -> Result<SmsParameters> {
    scope.require(&ciphertext.scope)?;
    client_id(id)?;
    if local_registration_generation != ciphertext.local_registration_generation
        || visibility > MAX_SAFE_INTEGER
    {
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
