//! Public registration evidence and bounded reconciliation. No transport or DB writes.
//! A numeric registration ID alone never proves which identity is installed.
#![allow(dead_code)]
use super::*;
use sha2::{Digest, Sha256};

pub const MAX_PUBLIC_PAYLOAD: usize = 64 * 1024;

/// Exact identity/SPK binding of a durably staged, engine-generated bundle.
/// Public shape parsing does not itself create submission/activation authority.
pub struct CandidateIdentity {
    registration_id: u16,
    identity: PublicKey,
    signed_prekey_id: u32,
    signed_prekey: PublicKey,
    signature: Signature,
    payload_hash: [u8; 32],
}
impl CandidateIdentity {
    pub fn from_public_payload(payload: &[u8]) -> Result<Self> {
        if payload.is_empty() || payload.len() > MAX_PUBLIC_PAYLOAD {
            return Err(SignalError::Resource);
        }
        let value: Value = serde_json::from_slice(payload).map_err(|_| SignalError::Invalid)?;
        if value.as_object().map(|x| x.len()) != Some(2)
            || !matches!(value["delSS"].as_u64(), Some(0 | 1))
        {
            return Err(SignalError::Invalid);
        }
        let bundle = parse_bundle(&value["key"])?;
        if bundle.device != DevicePlatform::Pc {
            return Err(SignalError::Scope);
        }
        Ok(Self {
            registration_id: bundle.registration_id,
            identity: bundle.identity,
            signed_prekey_id: bundle.signed_prekey_id,
            signed_prekey: bundle.signed_prekey,
            signature: bundle.signature,
            payload_hash: Sha256::digest(payload).into(),
        })
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn registration_id(&self) -> u16 {
        self.registration_id
    }
    pub fn matches(&self, bundle: &PublicBundle) -> bool {
        bundle.device == DevicePlatform::Pc
            && bundle.registration_id == self.registration_id
            && bundle.identity.bytes() == self.identity.bytes()
            && bundle.signed_prekey_id == self.signed_prekey_id
            && bundle.signed_prekey.bytes() == self.signed_prekey.bytes()
            && bundle.signature.bytes() == self.signature.bytes()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadState {
    AwaitingAck,
    AcceptedNeedsMetadata,
    Unknown,
    IdentityConfirmed,
    NeedsRecovery,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrekeyOutcome {
    Acknowledged,
    Unknown,
}
/// State only; it cannot activate SQL namespaces or authorize a second upload.
pub struct Reconciliation {
    candidate: CandidateIdentity,
    state: UploadState,
    acknowledged: bool,
    before: Option<(u16, u64)>,
    full_identity: bool,
}
impl Reconciliation {
    pub(crate) fn new(candidate: CandidateIdentity) -> Self {
        Self {
            candidate,
            state: UploadState::AwaitingAck,
            acknowledged: false,
            before: None,
            full_identity: false,
        }
    }
    pub fn state(&self) -> UploadState {
        self.state
    }
    pub fn prekeys(&self) -> PrekeyOutcome {
        if self.acknowledged {
            PrekeyOutcome::Acknowledged
        } else {
            PrekeyOutcome::Unknown
        }
    }
    pub(crate) fn unknown(&mut self) {
        if matches!(
            self.state,
            UploadState::AwaitingAck | UploadState::AcceptedNeedsMetadata | UploadState::Unknown
        ) {
            self.state = UploadState::Unknown;
            self.before = None;
            self.full_identity = false;
        }
    }
    fn fail_recovery(&mut self) {
        self.state = UploadState::NeedsRecovery;
        self.before = None;
        self.full_identity = false;
    }
    pub(crate) fn cancel(&mut self) {
        self.state = UploadState::Cancelled;
        self.before = None;
        self.full_identity = false;
    }
    /// A validated success ACK may omit ts in the original bundle. Never invent one.
    pub(crate) fn acknowledged(&mut self, data: &Value) -> Result<Option<u64>> {
        if self.state != UploadState::AwaitingAck {
            return Err(SignalError::OutOfOrder);
        }
        if !data.is_object() {
            return Err(SignalError::Invalid);
        }
        if let Some(err) = data.get("err") {
            service_code(err)?;
        }
        let timestamp = match data.get("ts") {
            None | Some(Value::Null) => None,
            Some(value) => Some(number(value)?).filter(|ts| *ts > 0),
        };
        self.acknowledged = true;
        self.state = UploadState::AcceptedNeedsMetadata;
        Ok(timestamp)
    }
    pub(crate) fn begin_metadata(&mut self, own: OwnKey) -> Result<()> {
        if !matches!(
            self.state,
            UploadState::Unknown | UploadState::AcceptedNeedsMetadata
        ) {
            return Err(SignalError::OutOfOrder);
        }
        let OwnKey::Present {
            registration_id,
            submitted_at,
        } = own
        else {
            self.fail_recovery();
            return Err(SignalError::Incomplete);
        };
        if registration_id != self.candidate.registration_id || submitted_at == 0 {
            self.fail_recovery();
            return Err(SignalError::Scope);
        }
        self.before = Some((registration_id, submitted_at));
        self.full_identity = false;
        Ok(())
    }
    pub(crate) fn identity(&mut self, own: &FetchedBundles, account: &str) -> Result<()> {
        if self.before.is_none()
            || !matches!(
                self.state,
                UploadState::Unknown | UploadState::AcceptedNeedsMetadata
            )
        {
            return Err(SignalError::OutOfOrder);
        }
        let pc: Vec<_> = own
            .bundles
            .iter()
            .filter(|bundle| bundle.device == DevicePlatform::Pc)
            .collect();
        if own.peer != account || pc.len() != 1 || !self.candidate.matches(pc[0]) {
            self.fail_recovery();
            return Err(SignalError::Scope);
        }
        self.full_identity = true;
        Ok(())
    }
    pub(crate) fn needs_identity(&self) -> bool {
        self.before.is_some()
            && !self.full_identity
            && matches!(
                self.state,
                UploadState::Unknown | UploadState::AcceptedNeedsMetadata
            )
    }
    pub(crate) fn needs_final_metadata(&self) -> bool {
        self.full_identity
            && matches!(
                self.state,
                UploadState::Unknown | UploadState::AcceptedNeedsMetadata
            )
    }
    pub(crate) fn finish_metadata(&mut self, own: OwnKey) -> Result<u64> {
        if !matches!(
            self.state,
            UploadState::Unknown | UploadState::AcceptedNeedsMetadata
        ) || !self.full_identity
        {
            return Err(SignalError::OutOfOrder);
        }
        let OwnKey::Present {
            registration_id,
            submitted_at,
        } = own
        else {
            self.fail_recovery();
            return Err(SignalError::Incomplete);
        };
        if self.before != Some((registration_id, submitted_at)) {
            self.fail_recovery();
            return Err(SignalError::Scope);
        }
        self.state = UploadState::IdentityConfirmed;
        Ok(submitted_at)
    }
}

fn request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 112
        || !value.starts_with("signal_req_")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(SignalError::Invalid);
    }
    Ok(())
}
pub(crate) fn upload_request(scope: &Scope, id: &str, payload: &[u8]) -> Result<SignalRequest> {
    request_id(id)?;
    CandidateIdentity::from_public_payload(payload)?;
    let data: Value = serde_json::from_slice(payload).map_err(|_| SignalError::Invalid)?;
    let mut request = SignalRequest::new(scope, 0, Command::SubmitBundle, data)?;
    request.request_id = id.into();
    Ok(request)
}
pub(crate) fn own_query(scope: &Scope, id: &str) -> Result<SignalRequest> {
    request_id(id)?;
    let mut request = own_key(scope, 0, DevicePlatform::Pc)?;
    request.request_id = id.into();
    Ok(request)
}
pub(crate) fn identity_query(scope: &Scope, id: &str) -> Result<SignalRequest> {
    request_id(id)?;
    let mut request = get_bundle(
        scope,
        0,
        scope.account(),
        DeviceSelection::All,
        BundleOptions {
            reinit: false,
            notify: false,
            auto_upgrade: false,
        },
    )?;
    request.request_id = id.into();
    Ok(request)
}
