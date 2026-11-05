//! Optional, original Rust host for the existing repository trust WASM binary.
//! This POC exposes no authenticated session and never contacts the network.
//! Unimplemented wasm-bindgen imports trap explicitly instead of faking success.

use anyhow::{bail, Context, Result};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use wasmi::{
    Caller, Config, Engine, ExternType, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, Val,
};

const MAX_WASM_BYTES: u64 = 1024 * 1024;
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ABI_BUFFER: usize = 1024 * 1024;
const MAX_HEAP_HANDLES: usize = 1024;
const MAX_HOST_ALLOCATION_BYTES: usize = 2 * 1024 * 1024;
const MAX_OBJECT_PROPERTIES: usize = 64;
const DEFAULT_FUEL: u64 = 30_000_000;
const EXPECTED_WASM_SHA256: &str =
    "3d53d066c7f4aaa1af3b01ec88c19e05ff7228584f965754ba65e78c1ddb606f";

#[derive(Clone)]
enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Error(String),
    Global,
    Crypto,
    Memory,
    Bytes(Rc<RefCell<Vec<u8>>>),
    View {
        start: usize,
        len: usize,
    },
    OwnedView {
        bytes: Rc<RefCell<Vec<u8>>>,
        start: usize,
        len: usize,
    },
    Object(Rc<RefCell<BTreeMap<String, JsValue>>>),
}

struct HostState {
    limits: StoreLimits,
    heap: Vec<Option<JsValue>>,
    free: Vec<usize>,
    calls: BTreeSet<String>,
    allocated_bytes: usize,
}

impl HostState {
    fn new() -> Self {
        let mut heap = vec![Some(JsValue::Undefined); 128];
        heap.extend([
            Some(JsValue::Undefined),
            Some(JsValue::Null),
            Some(JsValue::Bool(true)),
            Some(JsValue::Bool(false)),
        ]);
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MAX_MEMORY_BYTES)
                .memories(1)
                .instances(1)
                .tables(1)
                .table_elements(1024)
                .trap_on_grow_failure(true)
                .build(),
            heap,
            free: Vec::new(),
            calls: BTreeSet::new(),
            allocated_bytes: 0,
        }
    }
    fn get(&self, index: i32) -> std::result::Result<JsValue, wasmi::Error> {
        self.heap
            .get(index as usize)
            .and_then(Clone::clone)
            .ok_or_else(|| error("invalid or released wasm-bindgen handle"))
    }
    fn put(&mut self, value: JsValue) -> std::result::Result<i32, wasmi::Error> {
        let bytes = match &value {
            JsValue::String(value) | JsValue::Error(value) => value.len(),
            JsValue::Bytes(value) => value.borrow().len(),
            _ => 32,
        };
        self.charge(bytes)?;
        if let Some(index) = self.free.pop() {
            self.heap[index] = Some(value);
            return Ok(index as i32);
        }
        if self.heap.len() >= MAX_HEAP_HANDLES {
            return Err(error("wasm-bindgen heap limit exceeded"));
        }
        let index = self.heap.len();
        self.heap.push(Some(value));
        Ok(index as i32)
    }
    // A monotonic budget also bounds transient clones across a long-lived POC
    // host. Recreate the host after exhaustion; do not hide retained Rc storage.
    fn charge(&mut self, bytes: usize) -> std::result::Result<(), wasmi::Error> {
        self.allocated_bytes = self
            .allocated_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_HOST_ALLOCATION_BYTES)
            .ok_or_else(|| error("trust host allocation budget exceeded"))?;
        Ok(())
    }
    fn take(&mut self, index: i32) -> std::result::Result<JsValue, wasmi::Error> {
        let value = self.get(index)?;
        if index >= 132 {
            self.heap[index as usize] = None;
            self.free.push(index as usize);
        }
        Ok(value)
    }
    fn property(
        &mut self,
        object: &Rc<RefCell<BTreeMap<String, JsValue>>>,
        key: String,
        value: JsValue,
    ) -> std::result::Result<(), wasmi::Error> {
        let value_bytes = match &value {
            JsValue::String(text) => text.len(),
            JsValue::Undefined | JsValue::Null | JsValue::Bool(_) | JsValue::Number(_) => 16,
            _ => return Err(error("nested host object properties are unsupported")),
        };
        if key.len() > 128
            || (!object.borrow().contains_key(&key)
                && object.borrow().len() >= MAX_OBJECT_PROPERTIES)
        {
            return Err(error("host object property limit exceeded"));
        }
        self.charge(key.len() + value_bytes)?;
        object.borrow_mut().insert(key, value);
        Ok(())
    }
}

/// Key bytes deliberately have no Debug/Serialize implementation. The caller
/// must persist them only after verified trust and immutable profile binding.
pub struct IdentityKeys {
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}
pub struct LinkingSecret(pub(super) [u8; 32]);
pub struct EncodedLinkingQr(String);
impl EncodedLinkingQr {
    pub fn text(&self) -> &str {
        &self.0
    }
}
#[derive(Clone)]
pub struct CompanionProof {
    pub companion_id: u32,
    pub link_id: u64,
    pub linking_timestamp_ms: u64,
    pub linking_metadata: Vec<u8>,
    pub account_signature: Vec<u8>,
    pub device_signature: Vec<u8>,
    pub primary_public_key: Vec<u8>,
    pub master_id: String,
}

pub struct TrustHost {
    store: Store<HostState>,
    instance: Instance,
    poisoned: bool,
}

impl TrustHost {
    /// Load lazily from an explicit existing path. No home lookup, binary copy,
    /// JIT, JavaScript engine, WASI, filesystem imports or network imports.
    pub fn open(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_WASM_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_WASM_BYTES {
            bail!("trust WASM exceeds 1 MiB module limit");
        }
        if format!("{:x}", Sha256::digest(&bytes)) != EXPECTED_WASM_SHA256 {
            bail!("trust WASM hash differs from the pinned protocol ABI");
        }
        Self::from_bytes(&bytes)
    }
    pub fn bundled() -> Result<Self> {
        let bytes=include_bytes!("../../../pc-dist/libs/trusted_device_protocol_bg.c6da592e2114c13d62a77d3e37aa722c.wasm");
        if format!("{:x}", Sha256::digest(bytes)) != EXPECTED_WASM_SHA256 {
            bail!("bundled trust ABI hash mismatch");
        }
        Self::from_bytes(bytes)
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, bytes).context("validate trust WASM")?;
        let mut store = Store::new(&engine, HostState::new());
        store.limiter(|state| &mut state.limits);
        store.set_fuel(DEFAULT_FUEL)?;
        let mut linker = Linker::<HostState>::new(&engine);
        for import in module.imports() {
            if import.module() != "wbg" {
                bail!("unapproved WASM import namespace");
            }
            let ExternType::Func(ty) = import.ty() else {
                bail!("only function imports are allowed");
            };
            let name = import.name().to_owned();
            linker.func_new(
                "wbg",
                import.name(),
                ty.clone(),
                move |caller, params, results| dispatch(caller, &name, params, results),
            )?;
        }
        let instance = linker.instantiate_and_start(&mut store, &module)?;
        Ok(Self {
            store,
            instance,
            poisoned: false,
        })
    }
    pub fn memory_bytes(&self) -> usize {
        self.memory()
            .map(|memory| memory.data(&self.store).len())
            .unwrap_or(0)
    }
    pub fn exercised_imports(&self) -> Vec<String> {
        self.store.data().calls.iter().cloned().collect()
    }

    pub fn generate_identity(&mut self) -> Result<IdentityKeys> {
        self.prepare()?;
        let pointer = self.object_result("zprotoEd25519GenerateKeyPair")?;
        let result = (|| {
            let public_key = self.byte_getter("zprotoed25519keypair_publicKey", pointer)?;
            let private_key = self.byte_getter("zprotoed25519keypair_privateKey", pointer)?;
            if public_key.len() != 33 || private_key.len() != 32 {
                bail!(
                    "unexpected trust identity key representation: public={},private={}",
                    public_key.len(),
                    private_key.len()
                );
            }
            Ok(IdentityKeys {
                public_key,
                private_key,
            })
        })();
        self.finish_object(result, "__wbg_zprotoed25519keypair_free", pointer)
    }

    pub fn generate_link_secret(&mut self) -> Result<LinkingSecret> {
        self.prepare()?;
        let pointer = self.object_result("zprotoTrustGenerateLinkingSecretKey")?;
        let result = self
            .byte_getter("zprototrustlinkingsecretkey_key", pointer)
            .and_then(|bytes| {
                let key: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("unexpected linking-secret key size"))?;
                Ok(LinkingSecret(key))
            });
        self.finish_object(result, "__wbg_zprototrustlinkingsecretkey_free", pointer)
    }

    /// Generate trusted-device QR content using the existing protocol binary.
    /// Platform Pc=2 is evidenced in local qg1Z; no relink identity is reused.
    pub fn generate_linking_qr(
        &mut self,
        identity: &IdentityKeys,
        secret: &LinkingSecret,
        token: &str,
        timestamp_ms: u64,
    ) -> Result<EncodedLinkingQr> {
        validate_identity(identity)?;
        if token.is_empty()
            || token.len() > 512
            || timestamp_ms == 0
            || timestamp_ms > i64::MAX as u64
        {
            bail!("invalid linking QR token or timestamp");
        }
        self.prepare()?;
        let public = self.allocate(&identity.public_key)?;
        let key = self.allocate(&secret.0)?;
        let token_pointer = self.allocate(token.as_bytes())?;
        let frame = self.stack(-16)?;
        let result = self
            .call(
                "zprotoTrustGenerateLinkingQr",
                &[
                    Val::I32(frame),
                    Val::I32(public),
                    Val::I32(identity.public_key.len() as i32),
                    Val::I32(key),
                    Val::I32(32),
                    Val::I32(token_pointer),
                    Val::I32(token.len() as i32),
                    Val::I64(timestamp_ms as i64),
                    Val::I32(2),
                    Val::I32(0),
                    Val::I32(0),
                ],
                &mut [],
            )
            .and_then(|_| {
                let fields = self.frame(frame)?;
                if fields[3] != 0 {
                    return Err(self.take_exception(fields[2]));
                }
                if fields[1] <= 0 || fields[1] as usize > 8192 {
                    bail!("linking QR output exceeds limit");
                }
                let mut bytes = vec![0; fields[1] as usize];
                self.memory()?
                    .read(&self.store, fields[0] as usize, &mut bytes)?;
                self.call(
                    "__wbindgen_export_1",
                    &[Val::I32(fields[0]), Val::I32(fields[1]), Val::I32(1)],
                    &mut [],
                )?;
                Ok(EncodedLinkingQr(
                    String::from_utf8(bytes).context("linking QR is not UTF-8")?,
                ))
            });
        self.finish_frame(result)
    }

    /// A successful proof still cannot establish a trusted session: the final
    /// signed device list and bootstrap must also pass independent validation.
    /// Verify the decoded primary-linking protobuf and its HMAC, then generate
    /// a companion proof. Transport decryption happens before this boundary.
    /// Success alone supplies no account UID binding, freshness or session.
    pub fn create_companion_proof(
        &mut self,
        identity: &IdentityKeys,
        secret: &LinkingSecret,
        primary_linking_data: &[u8],
        linking_hmac: &[u8],
    ) -> Result<CompanionProof> {
        validate_identity(identity)?;
        if primary_linking_data.is_empty()
            || primary_linking_data.len() > MAX_ABI_BUFFER
            || linking_hmac.len() != 32
        {
            bail!("invalid companion-proof input size");
        }
        self.prepare()?;
        let public = self.allocate(&identity.public_key)?;
        let private = self.allocate(&identity.private_key)?;
        let key = self.allocate(&secret.0)?;
        let data = self.allocate(primary_linking_data)?;
        let mac = self.allocate(linking_hmac)?;
        let frame = self.stack(-16)?;
        let result = self
            .call(
                "zprotoTrustCreateCompanionLinkingProof",
                &[
                    Val::I32(frame),
                    Val::I32(public),
                    Val::I32(identity.public_key.len() as i32),
                    Val::I32(private),
                    Val::I32(32),
                    Val::I32(key),
                    Val::I32(32),
                    Val::I32(data),
                    Val::I32(primary_linking_data.len() as i32),
                    Val::I32(mac),
                    Val::I32(32),
                    Val::I32(2),
                ],
                &mut [],
            )
            .and_then(|_| {
                let fields = self.frame(frame)?;
                if fields[2] != 0 {
                    return Err(self.take_exception(fields[1]));
                }
                let pointer = fields[0];
                let result = (|| {
                    Ok(CompanionProof {
                        companion_id: self.integer_getter(
                            "zprototrustcompanionlinkingdata_companionId",
                            pointer,
                        )? as u32,
                        link_id: self
                            .integer_getter("zprototrustcompanionlinkingdata_linkId", pointer)?,
                        linking_timestamp_ms: self.integer_getter(
                            "zprototrustcompanionlinkingdata_linkingTimestamp",
                            pointer,
                        )?,
                        linking_metadata: self.byte_getter(
                            "zprototrustcompanionlinkingdata_linkingMetadata",
                            pointer,
                        )?,
                        account_signature: self.byte_getter(
                            "zprototrustcompanionlinkingdata_accountSignature",
                            pointer,
                        )?,
                        device_signature: self.byte_getter(
                            "zprototrustcompanionlinkingdata_deviceSignature",
                            pointer,
                        )?,
                        primary_public_key: self.byte_getter(
                            "zprototrustcompanionlinkingdata_primaryPublicIdentityKey",
                            pointer,
                        )?,
                        master_id: String::from_utf8(
                            self.byte_getter("zprototrustcompanionlinkingdata_masterId", pointer)?,
                        )
                        .context("invalid proof master ID")?,
                    })
                })();
                self.finish_object(
                    result,
                    "__wbg_zprototrustcompanionlinkingdata_free",
                    pointer,
                )
            });
        self.finish_frame(result)
    }

    /// Exercise real verification against untrusted serialized data. Success is
    /// never promoted to a usable trust result by this POC. Positive signature
    /// fixtures do not establish account UID binding or replay protection.
    pub fn inspect_untrusted_device_list(
        &mut self,
        encoded: &[u8],
        primary_key: &[u8],
        timestamp_ms: i64,
    ) -> Result<()> {
        self.verify_device_list_bytes(encoded, primary_key, timestamp_ms, None)?;
        bail!("device-list inspection does not establish companion or account trust");
    }

    pub(super) fn verify_device_list_bytes(
        &mut self,
        encoded: &[u8],
        primary_key: &[u8],
        timestamp_ms: i64,
        companion_key: Option<&[u8]>,
    ) -> Result<()> {
        if encoded.len() > MAX_ABI_BUFFER
            || primary_key.len() > 64
            || companion_key.is_some_and(|key| key.len() > 64)
        {
            bail!("trust verification input exceeds limit");
        }
        self.prepare()?;
        let data = self.allocate(encoded)?;
        let key = self.allocate(primary_key)?;
        let companion = match companion_key {
            Some(key) => self.allocate(key)?,
            None => 0,
        };
        let frame = self.stack(-16)?;
        let call = self.call(
            "zprotoTrustVerifyDeviceListFromBytes",
            &[
                Val::I32(frame),
                Val::I32(data),
                Val::I32(encoded.len() as i32),
                Val::I32(key),
                Val::I32(primary_key.len() as i32),
                Val::I64(timestamp_ms),
                Val::I32(companion),
                Val::I32(companion_key.map_or(0, |key| key.len()) as i32),
            ],
            &mut [],
        );
        let outcome = call.and_then(|_| {
            let fields = self.frame(frame)?;
            if fields[2] != 0 {
                return Err(self.take_exception(fields[1]));
            }
            let pointer = fields[0];
            // This export returns ZprotoTrustDeviceList, NOT the similarly
            // named ZprotoTrustDeviceListBytes. The JS ABI wraps it as K.
            let read = (|| {
                let mut trusted = [Val::I32(0)];
                self.call(
                    "zprototrustdevicelist_isTrusted",
                    &[Val::I32(pointer)],
                    &mut trusted,
                )?;
                let mut verification = [Val::I32(0)];
                self.call(
                    "zprototrustdevicelist_verificationError",
                    &[Val::I32(pointer)],
                    &mut verification,
                )?;
                if trusted[0].i32() != Some(1) {
                    bail!(
                        "trust WASM rejected input: device-list verification failed (code {})",
                        verification[0].i32().unwrap_or(-1)
                    );
                }
                Ok(())
            })();
            self.finish_object(read, "__wbg_zprototrustdevicelist_free", pointer)
        });
        self.finish_frame(outcome)
    }

    fn memory(&self) -> Result<Memory> {
        self.instance
            .get_memory(&self.store, "memory")
            .context("missing WASM memory")
    }
    fn call(&mut self, name: &str, args: &[Val], results: &mut [Val]) -> Result<()> {
        if self.poisoned {
            bail!("trust host is poisoned; create a fresh instance");
        }
        let result = self
            .instance
            .get_func(&self.store, name)
            .with_context(|| format!("missing WASM export {name}"))?
            .call(&mut self.store, args, results)
            .with_context(|| format!("WASM call {name} failed"));
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn prepare(&mut self) -> Result<()> {
        if self.poisoned {
            bail!("trust host is poisoned; create a fresh instance");
        }
        self.store.set_fuel(DEFAULT_FUEL)?;
        Ok(())
    }
    fn finish_frame<T>(&mut self, result: Result<T>) -> Result<T> {
        if self.poisoned {
            return result;
        }
        let cleanup = self.stack(16);
        match result {
            Err(primary) => Err(primary),
            Ok(value) => {
                cleanup?;
                Ok(value)
            }
        }
    }
    fn finish_object<T>(&mut self, result: Result<T>, export: &str, pointer: i32) -> Result<T> {
        if self.poisoned {
            return result;
        }
        let cleanup = self.call(export, &[Val::I32(pointer), Val::I32(0)], &mut []);
        match result {
            Err(primary) => Err(primary),
            Ok(value) => {
                cleanup?;
                Ok(value)
            }
        }
    }
    fn stack(&mut self, delta: i32) -> Result<i32> {
        let mut output = [Val::I32(0)];
        self.call(
            "__wbindgen_add_to_stack_pointer",
            &[Val::I32(delta)],
            &mut output,
        )?;
        output[0].i32().context("invalid stack pointer")
    }
    fn frame(&self, pointer: i32) -> Result<[i32; 4]> {
        let mut bytes = [0_u8; 16];
        self.memory()?
            .read(&self.store, pointer as usize, &mut bytes)?;
        Ok(std::array::from_fn(|index| {
            i32::from_le_bytes(
                bytes[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("four bytes"),
            )
        }))
    }
    fn object_result(&mut self, name: &str) -> Result<i32> {
        let frame = self.stack(-16)?;
        let result = self.call(name, &[Val::I32(frame)], &mut []).and_then(|_| {
            let fields = self.frame(frame)?;
            if fields[2] != 0 {
                Err(self.take_exception(fields[1]))
            } else {
                Ok(fields[0])
            }
        });
        self.finish_frame(result)
    }
    fn integer_getter(&mut self, name: &str, pointer: i32) -> Result<u64> {
        let function = self
            .instance
            .get_func(&self.store, name)
            .context("missing integer ABI")?;
        let output = function.ty(&self.store).results()[0];
        let mut result = [if output == wasmi::ValType::I64 {
            Val::I64(0)
        } else {
            Val::I32(0)
        }];
        self.call(name, &[Val::I32(pointer)], &mut result)?;
        match result[0] {
            Val::I64(value) => Ok(value as u64),
            Val::I32(value) => Ok(value as u32 as u64),
            _ => bail!("unexpected integer ABI"),
        }
    }
    fn byte_getter(&mut self, name: &str, pointer: i32) -> Result<Vec<u8>> {
        let frame = self.stack(-16)?;
        let result = self
            .call(name, &[Val::I32(frame), Val::I32(pointer)], &mut [])
            .and_then(|_| {
                let fields = self.frame(frame)?;
                if fields[1] < 0 || fields[1] as usize > MAX_ABI_BUFFER {
                    bail!("WASM output buffer exceeds limit");
                }
                let mut bytes = vec![0; fields[1] as usize];
                self.memory()?
                    .read(&self.store, fields[0] as usize, &mut bytes)?;
                self.call(
                    "__wbindgen_export_1",
                    &[Val::I32(fields[0]), Val::I32(fields[1]), Val::I32(1)],
                    &mut [],
                )?;
                Ok(bytes)
            });
        self.finish_frame(result)
    }
    fn allocate(&mut self, bytes: &[u8]) -> Result<i32> {
        let mut output = [Val::I32(0)];
        self.call(
            "__wbindgen_export_2",
            &[Val::I32(bytes.len() as i32), Val::I32(1)],
            &mut output,
        )?;
        let pointer = output[0].i32().context("invalid allocation")?;
        self.memory()?
            .write(&mut self.store, pointer as usize, bytes)?;
        Ok(pointer)
    }
    fn take_exception(&mut self, handle: i32) -> anyhow::Error {
        match self.store.data_mut().take(handle) {
            Ok(JsValue::String(message) | JsValue::Error(message)) => {
                anyhow::anyhow!("trust WASM rejected input: {message}")
            }
            Ok(JsValue::Object(object)) => {
                let message = object
                    .borrow()
                    .get("message")
                    .and_then(|value| {
                        if let JsValue::String(text) = value {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "structured protocol rejection".into());
                anyhow::anyhow!("trust WASM rejected input: {message}")
            }
            _ => anyhow::anyhow!("trust WASM rejected input with an unsupported exception"),
        }
    }
}

fn error(message: impl Into<String>) -> wasmi::Error {
    wasmi::Error::new(message.into())
}
fn validate_identity(identity: &IdentityKeys) -> Result<()> {
    if identity.public_key.len() != 33
        || identity.public_key[0] != 0x05
        || identity.private_key.len() != 32
    {
        bail!("invalid tagged Curve25519 identity representation");
    }
    Ok(())
}
fn bounded_length(length: usize) -> std::result::Result<(), wasmi::Error> {
    if length > MAX_ABI_BUFFER {
        return Err(error("host buffer exceeds 1 MiB limit"));
    }
    Ok(())
}
fn memory(caller: &Caller<'_, HostState>) -> std::result::Result<Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| error("missing caller memory"))
}
fn read_text(
    caller: &Caller<'_, HostState>,
    pointer: i32,
    length: i32,
) -> std::result::Result<String, wasmi::Error> {
    if length < 0 || length as usize > MAX_ABI_BUFFER {
        return Err(error("host text buffer limit exceeded"));
    }
    let mut bytes = vec![0; length as usize];
    memory(caller)?
        .read(caller, pointer as usize, &mut bytes)
        .map_err(|_| error("invalid WASM memory read"))?;
    String::from_utf8(bytes).map_err(|_| error("invalid WASM UTF-8"))
}

fn dispatch(
    mut caller: Caller<'_, HostState>,
    name: &str,
    params: &[Val],
    results: &mut [Val],
) -> std::result::Result<(), wasmi::Error> {
    caller.data_mut().calls.insert(name.to_owned());
    let arg = |index: usize| {
        params
            .get(index)
            .and_then(Val::i32)
            .ok_or_else(|| error("invalid host i32 argument"))
    };
    let handle =
        match name {
            "__wbindgen_object_drop_ref" => {
                caller.data_mut().take(arg(0)?)?;
                return Ok(());
            }
            "__wbindgen_object_clone_ref" => {
                let value = caller.data().get(arg(0)?)?;
                Some(caller.data_mut().put(value)?)
            }
            "__wbindgen_is_undefined" => {
                Some(matches!(caller.data().get(arg(0)?)?, JsValue::Undefined) as i32)
            }
            "__wbindgen_is_object" => Some(matches!(
                caller.data().get(arg(0)?)?,
                JsValue::Global
                    | JsValue::Crypto
                    | JsValue::Object(_)
                    | JsValue::Memory
                    | JsValue::Bytes(_)
                    | JsValue::View { .. }
                    | JsValue::OwnedView { .. }
                    | JsValue::Error(_)
            ) as i32),
            "__wbindgen_is_string" => {
                Some(matches!(caller.data().get(arg(0)?)?, JsValue::String(_)) as i32)
            }
            "__wbindgen_boolean_get" => Some(match caller.data().get(arg(0)?)? {
                JsValue::Bool(value) => value as i32,
                _ => 2,
            }),
            "__wbindgen_as_number" => {
                let JsValue::Number(value) = caller.data().get(arg(0)?)? else {
                    return Err(error("unsupported host numeric coercion"));
                };
                results[0] = Val::F64(value.into());
                return Ok(());
            }
            "__wbindgen_is_function" => Some(0), // this host has no callable JS objects
            "__wbindgen_memory" => Some(caller.data_mut().put(JsValue::Memory)?),
            "__wbindgen_string_new" => {
                let text = read_text(&caller, arg(0)?, arg(1)?)?;
                Some(caller.data_mut().put(JsValue::String(text))?)
            }
            "__wbindgen_error_new" => {
                let text = read_text(&caller, arg(0)?, arg(1)?)?;
                Some(caller.data_mut().put(JsValue::Error(text))?)
            }
            "__wbindgen_throw" => return Err(error(read_text(&caller, arg(0)?, arg(1)?)?)),
            "__wbindgen_number_new" => {
                let number = params
                    .first()
                    .and_then(Val::f64)
                    .ok_or_else(|| error("invalid host f64 argument"))?;
                Some(caller.data_mut().put(JsValue::Number(number.into()))?)
            }
            "__wbg_new_405e22f390576ce2" => Some(
                caller
                    .data_mut()
                    .put(JsValue::Object(Rc::new(RefCell::new(BTreeMap::new()))))?,
            ),
            "__wbg_set_3f1d0b984ed272ed" => {
                let JsValue::Object(object) = caller.data().get(arg(0)?)? else {
                    return Err(error("host property target is not an object"));
                };
                let JsValue::String(key) = caller.data().get(arg(1)?)? else {
                    return Err(error("host property key is not a string"));
                };
                let value = caller.data().get(arg(2)?)?;
                caller.data_mut().property(&object, key, value)?;
                return Ok(());
            }
            "__wbg_static_accessor_GLOBAL_THIS_56578be7e9f832b0"
            | "__wbg_static_accessor_SELF_37c5d418e4bf5819" => {
                Some(caller.data_mut().put(JsValue::Global)?)
            }
            "__wbg_static_accessor_GLOBAL_88a902d13a557d07"
            | "__wbg_static_accessor_WINDOW_5de37043a91a9c40" => Some(0),
            "__wbg_crypto_574e78ad8b13b65f" => {
                if !matches!(caller.data().get(arg(0)?)?, JsValue::Global) {
                    return Err(error("crypto accessed on a non-global object"));
                }
                Some(caller.data_mut().put(JsValue::Crypto)?)
            }
            "__wbg_buffer_609cc3eee51ed158" => {
                let value = match caller.data().get(arg(0)?)? {
                    JsValue::OwnedView { bytes, .. } => JsValue::Bytes(bytes),
                    JsValue::View { .. } => JsValue::Memory,
                    value @ JsValue::Memory | value @ JsValue::Bytes(_) => value,
                    _ => return Err(error("unsupported host buffer access")),
                };
                Some(caller.data_mut().put(value)?)
            }
            "__wbg_newwithlength_a381634e90c276d4" => {
                let length = arg(0)?;
                if length < 0 || length as usize > MAX_ABI_BUFFER {
                    return Err(error("typed array allocation limit exceeded"));
                }
                Some(caller.data_mut().put(JsValue::Bytes(Rc::new(RefCell::new(
                    vec![0; length as usize],
                ))))?)
            }
            "__wbg_new_a12002a7f91c75be" => {
                if matches!(caller.data().get(arg(0)?)?, JsValue::Memory) {
                    let len = memory(&caller)?.data(&caller).len();
                    let handle = caller.data_mut().put(JsValue::View { start: 0, len })?;
                    *results
                        .first_mut()
                        .ok_or_else(|| error("host result signature mismatch"))? = Val::I32(handle);
                    return Ok(());
                }
                let bytes = match caller.data().get(arg(0)?)? {
                    JsValue::Bytes(bytes) => {
                        bounded_length(bytes.borrow().len())?;
                        bytes.borrow().clone()
                    }
                    JsValue::OwnedView { bytes, start, len } => {
                        bounded_length(len)?;
                        bytes.borrow()[start..start + len].to_vec()
                    }
                    JsValue::View { start, len } => {
                        bounded_length(len)?;
                        let mut bytes = vec![0; len];
                        memory(&caller)?
                            .read(&caller, start, &mut bytes)
                            .map_err(|_| error("invalid typed-array copy"))?;
                        bytes
                    }
                    _ => return Err(error("unsupported typed-array constructor input")),
                };
                Some(
                    caller
                        .data_mut()
                        .put(JsValue::Bytes(Rc::new(RefCell::new(bytes))))?,
                )
            }
            "__wbg_newwithbyteoffsetandlength_d97e637ebe145a9a" => {
                if !matches!(caller.data().get(arg(0)?)?, JsValue::Memory) {
                    return Err(error("unsupported typed-array buffer"));
                }
                let start = arg(1)? as usize;
                let len = arg(2)? as usize;
                if len > MAX_ABI_BUFFER
                    || start
                        .checked_add(len)
                        .filter(|end| {
                            *end <= memory(&caller).map(|m| m.data(&caller).len()).unwrap_or(0)
                        })
                        .is_none()
                {
                    return Err(error("invalid typed-array view"));
                }
                Some(caller.data_mut().put(JsValue::View { start, len })?)
            }
            "__wbg_getRandomValues_b8f5dbd5f3995a9e" => {
                if !matches!(caller.data().get(arg(0)?)?, JsValue::Crypto) {
                    return Err(error("randomness requested on a non-crypto object"));
                }
                match caller.data().get(arg(1)?)? {
                    JsValue::Bytes(bytes) => rand::rngs::OsRng
                        .try_fill_bytes(&mut bytes.borrow_mut())
                        .map_err(|_| error("OS randomness unavailable"))?,
                    JsValue::OwnedView { bytes, start, len } => rand::rngs::OsRng
                        .try_fill_bytes(&mut bytes.borrow_mut()[start..start + len])
                        .map_err(|_| error("OS randomness unavailable"))?,
                    JsValue::View { start, len } => {
                        bounded_length(len)?;
                        let mut bytes = vec![0; len];
                        rand::rngs::OsRng
                            .try_fill_bytes(&mut bytes)
                            .map_err(|_| error("OS randomness unavailable"))?;
                        memory(&caller)?
                            .write(&mut caller, start, &bytes)
                            .map_err(|_| error("invalid random output buffer"))?;
                    }
                    _ => return Err(error("randomness output is not a typed array")),
                }
                return Ok(());
            }
            "__wbg_subarray_aa9065fa9dc5df96" => {
                let begin = arg(1)? as u32 as usize;
                let end = arg(2)? as u32 as usize;
                let value = match caller.data().get(arg(0)?)? {
                    JsValue::Bytes(bytes) => {
                        let length = bytes.borrow().len();
                        let start = begin.min(length);
                        JsValue::OwnedView {
                            bytes,
                            start,
                            len: end.min(length).saturating_sub(start),
                        }
                    }
                    JsValue::OwnedView { bytes, start, len } => {
                        let begin = begin.min(len);
                        JsValue::OwnedView {
                            bytes,
                            start: start + begin,
                            len: end.min(len).saturating_sub(begin),
                        }
                    }
                    JsValue::View { start, len } => {
                        let begin = begin.min(len);
                        JsValue::View {
                            start: start + begin,
                            len: end.min(len).saturating_sub(begin),
                        }
                    }
                    _ => return Err(error("subarray target is not a typed array")),
                };
                Some(caller.data_mut().put(value)?)
            }
            "__wbg_set_65595bdd868b3009" => {
                let input = caller.data().get(arg(1)?)?;
                let bytes = match input {
                    JsValue::Bytes(bytes) => {
                        bounded_length(bytes.borrow().len())?;
                        bytes.borrow().clone()
                    }
                    JsValue::OwnedView { bytes, start, len } => {
                        bounded_length(len)?;
                        bytes.borrow()[start..start + len].to_vec()
                    }
                    JsValue::View { start, len } => {
                        bounded_length(len)?;
                        let mut bytes = vec![0; len];
                        memory(&caller)?
                            .read(&caller, start, &mut bytes)
                            .map_err(|_| error("invalid typed-array read"))?;
                        bytes
                    }
                    _ => return Err(error("typed-array source is unsupported")),
                };
                let offset = arg(2)? as u32 as usize;
                match caller.data().get(arg(0)?)? {
                    JsValue::Bytes(target) => {
                        let mut target = target.borrow_mut();
                        let end = offset
                            .checked_add(bytes.len())
                            .filter(|end| *end <= target.len())
                            .ok_or_else(|| error("typed-array set out of bounds"))?;
                        target[offset..end].copy_from_slice(&bytes);
                    }
                    JsValue::View { start, len } => {
                        if offset
                            .checked_add(bytes.len())
                            .filter(|end| *end <= len)
                            .is_none()
                        {
                            return Err(error("typed-array set out of bounds"));
                        }
                        memory(&caller)?
                            .write(&mut caller, start + offset, &bytes)
                            .map_err(|_| error("invalid typed-array write"))?;
                    }
                    JsValue::OwnedView {
                        bytes: target,
                        start,
                        len,
                    } => {
                        if offset
                            .checked_add(bytes.len())
                            .filter(|end| *end <= len)
                            .is_none()
                        {
                            return Err(error("typed-array set out of bounds"));
                        }
                        target.borrow_mut()[start + offset..start + offset + bytes.len()]
                            .copy_from_slice(&bytes);
                    }
                    _ => return Err(error("typed-array set target is unsupported")),
                }
                return Ok(());
            }
            "__wbg_length_a446193dc22c12f8" => Some(match caller.data().get(arg(0)?)? {
                JsValue::Bytes(bytes) => bytes.borrow().len() as i32,
                JsValue::View { len, .. } | JsValue::OwnedView { len, .. } => len as i32,
                _ => return Err(error("unsupported typed-array length")),
            }),
            _ => return Err(error(format!("unsupported wasm-bindgen import: {name}"))),
        };
    if let Some(value) = handle {
        let result = results
            .first_mut()
            .ok_or_else(|| error("host result signature mismatch"))?;
        *result = Val::I32(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binary() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../pc-dist/libs/trusted_device_protocol_bg.c6da592e2114c13d62a77d3e37aa722c.wasm",
        )
    }
    #[test]
    fn existing_binary_generates_curve_identity_verified_by_openssl() {
        let mut host = TrustHost::open(&binary()).unwrap();
        let first = host.generate_identity().unwrap();
        let second = host.generate_identity().unwrap();
        assert_eq!(first.public_key.len(), 33);
        assert_eq!(first.private_key.len(), 32);
        assert_ne!(first.public_key, second.public_key);
        assert_ne!(first.private_key, second.private_key);
        assert!(host.memory_bytes() <= MAX_MEMORY_BYTES);
        // Independent OpenSSL X25519 derivation. Despite the export's name,
        // this binary returns a Signal-style 0x05-tagged Curve25519 public key.
        use std::io::Write;
        use std::process::{Command, Stdio};
        assert_eq!(first.public_key[0], 0x05);
        let mut pkcs8 = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x04, 0x22,
            0x04, 0x20,
        ];
        pkcs8.extend_from_slice(&first.private_key);
        let mut child = Command::new("openssl")
            .args(["pkey", "-inform", "DER", "-pubout", "-outform", "DER"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("OpenSSL is required to independently verify the trust-WASM key fixture");
        child.stdin.take().unwrap().write_all(&pkcs8).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "OpenSSL rejected ephemeral X25519 private key"
        );
        let spki_prefix = [
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
        ];
        assert!(output.stdout.starts_with(&spki_prefix));
        assert_eq!(&first.public_key[1..], &output.stdout[spki_prefix.len()..]);
    }
    #[test]
    fn malformed_device_list_never_becomes_trusted() {
        let mut host = TrustHost::open(&binary()).unwrap();
        let error = host
            .inspect_untrusted_device_list(b"not a protocol message", &[0; 32], 1700000000000)
            .unwrap_err();
        assert!(
            error.to_string().starts_with("trust WASM rejected input:"),
            "{error:#}"
        );
    }

    #[test]
    fn real_link_secret_and_qr_generation_do_not_authenticate_or_contact_network() {
        let mut host = TrustHost::open(&binary()).unwrap();
        let identity = host.generate_identity().unwrap();
        let secret = host.generate_link_secret().unwrap();
        assert!(secret.0.iter().any(|byte| *byte != 0));
        let qr = host
            .generate_linking_qr(&identity, &secret, "fixture-token", 1700000000000)
            .unwrap();
        assert!(!qr.text().is_empty());
        assert!(qr.text().len() < 8192);
        let error = host
            .create_companion_proof(
                &identity,
                &secret,
                b"malformed primary linking protobuf",
                &[0; 32],
            )
            .err()
            .unwrap();
        assert!(
            error.to_string().starts_with("trust WASM rejected input:"),
            "{error:#}"
        );
    }

    fn module(sections: &[(u8, &[u8])]) -> Vec<u8> {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        for (id, payload) in sections {
            assert!(payload.len() < 128);
            bytes.extend([*id, payload.len() as u8]);
            bytes.extend(*payload);
        }
        bytes
    }

    #[test]
    fn actual_infinite_guest_loop_exhausts_fuel() {
        // (func (export "spin") (loop (br 0)))
        let bytes = module(&[
            (1, &[1, 0x60, 0, 0]),
            (3, &[1, 0]),
            (7, &[1, 4, b's', b'p', b'i', b'n', 0, 0]),
            (10, &[1, 7, 0, 3, 0x40, 0x0c, 0, 0x0b, 0x0b]),
        ]);
        let mut host = TrustHost::from_bytes(&bytes).unwrap();
        host.store.set_fuel(100).unwrap();
        let function = host.instance.get_func(&host.store, "spin").unwrap();
        let error = function.call(&mut host.store, &[], &mut []).unwrap_err();
        assert_eq!(error.as_trap_code(), Some(wasmi::TrapCode::OutOfFuel));
    }

    #[test]
    fn trapped_instance_stays_poisoned_after_fuel_reset_and_preserves_primary_error() {
        let bytes = module(&[
            (1, &[1, 0x60, 0, 0]),
            (3, &[1, 0]),
            (7, &[1, 4, b's', b'p', b'i', b'n', 0, 0]),
            (10, &[1, 7, 0, 3, 0x40, 0x0c, 0, 0x0b, 0x0b]),
        ]);
        let mut host = TrustHost::from_bytes(&bytes).unwrap();
        host.store.set_fuel(100).unwrap();
        let error = host.call("spin", &[], &mut []).unwrap_err();
        assert!(host.poisoned);
        assert!(format!("{error:#}").contains("all fuel consumed"));
        let preserved = host.finish_frame::<()>(Err(error)).unwrap_err();
        assert!(format!("{preserved:#}").contains("all fuel consumed"));
        host.store.set_fuel(DEFAULT_FUEL).unwrap();
        let error = host.generate_identity().err().unwrap();
        assert!(error.to_string().contains("poisoned"));
        assert!(host
            .call("spin", &[], &mut [])
            .unwrap_err()
            .to_string()
            .contains("poisoned"));
    }

    #[test]
    fn actual_guest_memory_growth_cannot_exceed_limit() {
        // One initial page; exported grow requests another 300 pages.
        let bytes = module(&[
            (1, &[1, 0x60, 0, 1, 0x7f]),
            (3, &[1, 0]),
            (5, &[1, 0, 1]),
            (
                7,
                &[
                    2, 6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0, 4, b'g', b'r', b'o', b'w', 0, 0,
                ],
            ),
            (10, &[1, 7, 0, 0x41, 0xac, 2, 0x40, 0, 0x0b]),
        ]);
        let mut host = TrustHost::from_bytes(&bytes).unwrap();
        let function = host.instance.get_func(&host.store, "grow").unwrap();
        let error = function
            .call(&mut host.store, &[], &mut [Val::I32(0)])
            .unwrap_err();
        assert_eq!(
            error.as_trap_code(),
            Some(wasmi::TrapCode::GrowthOperationLimited)
        );
        assert_eq!(host.memory_bytes(), 65536);
    }

    #[test]
    fn unknown_host_import_traps_instead_of_succeeding() {
        // Import wbg.unsupported; exported run invokes that actual import.
        let bytes = module(&[
            (1, &[1, 0x60, 0, 0]),
            (
                2,
                &[
                    1, 3, b'w', b'b', b'g', 11, b'u', b'n', b's', b'u', b'p', b'p', b'o', b'r',
                    b't', b'e', b'd', 0, 0,
                ],
            ),
            (3, &[1, 0]),
            (7, &[1, 3, b'r', b'u', b'n', 0, 1]),
            (10, &[1, 4, 0, 0x10, 0, 0x0b]),
        ]);
        let mut host = TrustHost::from_bytes(&bytes).unwrap();
        let error = host.call("run", &[], &mut []).unwrap_err();
        assert!(format!("{error:#}").contains("unsupported wasm-bindgen import: unsupported"));
    }

    #[test]
    fn aggregate_heap_and_property_budgets_prevent_retention_and_cycles() {
        let mut state = HostState::new();
        state
            .put(JsValue::String("a".repeat(MAX_ABI_BUFFER)))
            .unwrap();
        state
            .put(JsValue::String("b".repeat(MAX_ABI_BUFFER)))
            .unwrap();
        assert!(state.put(JsValue::String("c".into())).is_err());
        let mut state = HostState::new();
        let object = Rc::new(RefCell::new(BTreeMap::new()));
        for index in 0..MAX_OBJECT_PROPERTIES {
            state
                .property(&object, format!("key-{index}"), JsValue::Number(1.0))
                .unwrap();
        }
        assert!(state
            .property(&object, "one-more".into(), JsValue::Null)
            .is_err());
        assert!(state
            .property(&object, "cycle".into(), JsValue::Object(object.clone()))
            .is_err());
        assert!(state
            .property(&object, "x".repeat(129), JsValue::Null)
            .is_err());
        assert_eq!(object.borrow().len(), MAX_OBJECT_PROPERTIES);
    }

    #[test]
    fn public_loader_rejects_unpinned_binary_before_import_dispatch() {
        let path = std::env::temp_dir().join(format!("zalo-trust-unpinned-{}", std::process::id()));
        std::fs::write(&path, b"not the pinned module").unwrap();
        let result = TrustHost::open(&path);
        std::fs::remove_file(path).unwrap();
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("pinned protocol ABI"));
    }
}

#[cfg(test)]
mod signed_list_fixtures {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/trust/signed_primary_list.json"
        ))
        .unwrap()
    }
    fn bytes(fixture: &serde_json::Value, field: &str) -> Vec<u8> {
        let text = fixture[field].as_str().unwrap();
        assert_eq!(text.len() % 2, 0);
        (0..text.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
            .collect()
    }
    fn host() -> TrustHost {
        TrustHost::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../pc-dist/libs/trusted_device_protocol_bg.c6da592e2114c13d62a77d3e37aa722c.wasm",
        ))
        .unwrap()
    }
    fn rejected(result: Result<()>, code: i32) {
        assert_eq!(
            result.unwrap_err().to_string(),
            format!("trust WASM rejected input: device-list verification failed (code {code})")
        );
    }

    #[test]
    fn independent_signed_list_reaches_correct_getters_destructor_and_clean_reuse() {
        let fixture = fixture();
        let primary = bytes(&fixture, "primary_public_hex");
        let companion = bytes(&fixture, "companion_public_hex");
        let data = bytes(&fixture, "device_list_hex");
        let empty = bytes(&fixture, "empty_device_list_hex");
        let timestamp = fixture["timestamp_ms"].as_i64().unwrap();
        let mut host = host();
        host.verify_device_list_bytes(&empty, &primary, timestamp, None)
            .unwrap();
        host.verify_device_list_bytes(&data, &primary, timestamp, Some(&companion))
            .unwrap();
        // Domain rejection must release the correct returned object and frame;
        // another real positive operation on this SAME instance remains valid.
        let bad = bytes(&fixture, "invalid_device_signature_resigned_list_hex");
        // The outer L3 signature was independently re-signed after mutating D2.
        // This proves failure comes from selected-companion proof verification.
        host.verify_device_list_bytes(&bad, &primary, timestamp, None)
            .unwrap();
        rejected(
            host.verify_device_list_bytes(&bad, &primary, timestamp, Some(&companion)),
            4,
        );
        host.verify_device_list_bytes(&data, &primary, timestamp, Some(&companion))
            .unwrap();
        assert!(!host.poisoned);
    }

    #[test]
    fn signed_list_rejects_modified_signature_and_wrong_primary_or_companion() {
        let fixture = fixture();
        let primary = bytes(&fixture, "primary_public_hex");
        let companion = bytes(&fixture, "companion_public_hex");
        let mut data = bytes(&fixture, "device_list_hex");
        let timestamp = fixture["timestamp_ms"].as_i64().unwrap();
        let mut host = host();
        rejected(
            host.verify_device_list_bytes(&data, &companion, timestamp, Some(&companion)),
            3,
        );
        rejected(
            host.verify_device_list_bytes(&data, &primary, timestamp, Some(&primary)),
            6,
        );
        let mut modified_master = data.clone();
        modified_master[2] ^= 1;
        rejected(
            host.verify_device_list_bytes(&modified_master, &primary, timestamp, Some(&companion)),
            1,
        );
        *data.last_mut().unwrap() ^= 1;
        rejected(
            host.verify_device_list_bytes(&data, &primary, timestamp, Some(&companion)),
            1,
        );
    }

    #[test]
    fn signed_list_enforces_list_expiry_and_stale_companion_expiry() {
        let fixture = fixture();
        let primary = bytes(&fixture, "primary_public_hex");
        let companion = bytes(&fixture, "companion_public_hex");
        let data = bytes(&fixture, "device_list_hex");
        let timestamp = fixture["timestamp_ms"].as_i64().unwrap();
        let mut host = host();
        rejected(
            host.verify_device_list_bytes(
                &data,
                &primary,
                timestamp + 1_814_400_001,
                Some(&companion),
            ),
            2,
        );
        let expired = bytes(&fixture, "expired_stale_companion_resigned_list_hex");
        rejected(
            host.verify_device_list_bytes(&expired, &primary, timestamp, Some(&companion)),
            5,
        );
    }

    #[test]
    fn valid_signature_still_cannot_establish_account_binding_or_replay_protection() {
        let fixture = fixture();
        let primary = bytes(&fixture, "primary_public_hex");
        let companion = bytes(&fixture, "companion_public_hex");
        let data = bytes(&fixture, "device_list_hex");
        let timestamp = fixture["timestamp_ms"].as_i64().unwrap();
        let mut host = host();
        // The guest cryptographic verifier is stateless: an identical signed
        // list remains valid inside its TTL. Future session code MUST persist
        // a trusted timestamp watermark and bind authenticated account context.
        host.verify_device_list_bytes(&data, &primary, timestamp, Some(&companion))
            .unwrap();
        host.verify_device_list_bytes(&data, &primary, timestamp + 1, Some(&companion))
            .unwrap();
        assert_eq!(
            host.inspect_untrusted_device_list(&data, &primary, timestamp)
                .unwrap_err()
                .to_string(),
            "device-list inspection does not establish companion or account trust"
        );
    }

    #[test]
    fn independent_fixture_generator_matches_checked_in_vectors() {
        let output = std::process::Command::new("python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/trust/generate_signed_list.py"),
            )
            .output()
            .expect("Python 3 and OpenSSL are required for independent fixture verification");
        assert!(
            output.status.success(),
            "independent fixture arithmetic or OpenSSL cross-check failed"
        );
        let generated: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(generated, fixture());
    }
}

#[cfg(test)]
mod linking_proof_fixtures {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/trust/primary_linking_proof.json"
        ))
        .unwrap()
    }
    fn bytes(fixture: &serde_json::Value, field: &str) -> Vec<u8> {
        let text = fixture[field].as_str().unwrap();
        (0..text.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
            .collect()
    }
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    fn identity(fixture: &serde_json::Value) -> IdentityKeys {
        IdentityKeys {
            public_key: bytes(fixture, "companion_public_hex"),
            private_key: bytes(fixture, "companion_fixture_private_hex"),
        }
    }
    fn secret(fixture: &serde_json::Value) -> LinkingSecret {
        LinkingSecret(
            bytes(fixture, "fixture_linking_secret_hex")
                .try_into()
                .unwrap(),
        )
    }
    fn host() -> TrustHost {
        TrustHost::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../pc-dist/libs/trusted_device_protocol_bg.c6da592e2114c13d62a77d3e37aa722c.wasm",
        ))
        .unwrap()
    }
    fn checker_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/trust/generate_linking_proof.py")
    }
    fn independent_signature_check(public: &[u8], message: &[u8], signature: &[u8]) -> bool {
        let request = serde_json::json!({
            "public_hex": hex(public), "message_hex": hex(message), "signature_hex": hex(signature),
        });
        let mut child = Command::new("python3")
            .arg(checker_path())
            .arg("--verify-signature")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Python 3 is required for independent synthetic signature checking");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&request).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "independent synthetic signature checker failed"
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["verified"]
            .as_bool()
            .unwrap()
    }

    #[test]
    fn positive_primary_hmac_and_account_signature_produce_independently_valid_device_proof() {
        let fixture = fixture();
        let identity = identity(&fixture);
        let mut host = host();
        let proof = host
            .create_companion_proof(
                &identity,
                &secret(&fixture),
                &bytes(&fixture, "primary_linking_data_hex"),
                &bytes(&fixture, "linking_hmac_hex"),
            )
            .unwrap();
        assert_eq!(proof.master_id, fixture["master_id"].as_str().unwrap());
        assert_eq!(
            proof.primary_public_key,
            bytes(&fixture, "primary_public_hex")
        );
        assert_eq!(
            proof.linking_metadata,
            bytes(&fixture, "expected_pc_metadata_hex")
        );
        assert_eq!(
            proof.account_signature,
            bytes(&fixture, "account_signature_hex")
        );
        let list_fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/trust/signed_primary_list.json"
        ))
        .unwrap();
        assert_eq!(
            proof.account_signature,
            bytes(&list_fixture, "account_signature_hex")
        );
        assert!(independent_signature_check(
            &proof.primary_public_key,
            &bytes(&fixture, "account_signature_message_hex"),
            &proof.account_signature
        ));
        let mut device_message = b"D2".to_vec();
        device_message.extend_from_slice(&proof.linking_metadata);
        device_message.extend_from_slice(&identity.public_key);
        device_message.extend_from_slice(&proof.primary_public_key);
        assert!(independent_signature_check(
            &identity.public_key,
            &device_message,
            &proof.device_signature
        ));
        let mut tampered = proof.device_signature.clone();
        tampered[0] ^= 1;
        assert!(!independent_signature_check(
            &identity.public_key,
            &device_message,
            &tampered
        ));
        assert!(!host.poisoned);
    }

    #[test]
    fn changed_primary_data_mac_or_linking_secret_fail_hmac_before_proof() {
        let fixture = fixture();
        let identity = identity(&fixture);
        let data = bytes(&fixture, "primary_linking_data_hex");
        let mac = bytes(&fixture, "linking_hmac_hex");
        let mut host = host();
        let mut changed_data = data.clone();
        // Last byte belongs to fixed64 timestamp, preserving protobuf shape.
        *changed_data.last_mut().unwrap() ^= 1;
        let mut changed_mac = mac.clone();
        changed_mac[0] ^= 1;
        let mut changed_secret = secret(&fixture);
        changed_secret.0[0] ^= 1;
        for (data, mac, key) in [
            (&changed_data, &mac, secret(&fixture)),
            (&data, &changed_mac, secret(&fixture)),
            (&data, &mac, changed_secret),
        ] {
            let error = host
                .create_companion_proof(&identity, &key, data, mac)
                .err()
                .unwrap();
            assert_eq!(
                error.to_string(),
                "trust WASM rejected input: invalid linking MAC"
            );
        }
        // Domain rejection leaves the frame/object state reusable.
        host.create_companion_proof(&identity, &secret(&fixture), &data, &mac)
            .unwrap();
    }

    #[test]
    fn valid_hmac_does_not_bypass_bad_account_signature_or_wrong_companion() {
        let fixture = fixture();
        let identity = identity(&fixture);
        let mut host = host();
        let error = host
            .create_companion_proof(
                &identity,
                &secret(&fixture),
                &bytes(&fixture, "invalid_account_data_hex"),
                &bytes(&fixture, "invalid_account_valid_hmac_hex"),
            )
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "trust WASM rejected input: invalid account signature"
        );
        let wrong_identity = IdentityKeys {
            public_key: bytes(&fixture, "primary_public_hex"),
            private_key: identity.private_key,
        };
        let error = host
            .create_companion_proof(
                &wrong_identity,
                &secret(&fixture),
                &bytes(&fixture, "primary_linking_data_hex"),
                &bytes(&fixture, "linking_hmac_hex"),
            )
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "trust WASM rejected input: invalid account signature"
        );
    }

    #[test]
    fn independent_linking_generator_matches_checked_in_hmac_and_account_vectors() {
        let output = Command::new("python3").arg(checker_path()).output().expect(
            "Python 3 and OpenSSL are required for independent linking fixture verification",
        );
        assert!(
            output.status.success(),
            "independent HMAC/signature/OpenSSL cross-check failed"
        );
        let generated: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(generated, fixture());
    }
}
