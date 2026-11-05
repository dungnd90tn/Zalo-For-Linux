//! Constructors accept sealed coordinator results or durable verified dispatch
//! receipts. Caller strings and booleans never grant cryptographic provenance.
use super::*;
use crate::{
    app::live::signal_crypto::VerifiedPersonalResult,
    store::e2ee::{DispatchReceipt, GenerationGate},
};
use zalo_signal_engine::{Output, PersonalMessageType};

pub(crate) fn personal(verified: &VerifiedPersonalResult) -> Result<EngineCiphertext> {
    let result = verified.result();
    let address = verified.address();
    if result.account().as_str() != address.scope().account()
        || result.generation() != address.scope().generation()
    {
        return Err(SignalError::Scope);
    }
    let Output::PersonalEncrypted {
        message,
        remote_registration_id,
    } = result.output()
    else {
        return Err(SignalError::Invalid);
    };
    registration(&json!(remote_registration_id))?;
    let mut raw = Vec::with_capacity(message.body.len() + 1);
    raw.push(match message.message_type {
        PersonalMessageType::PreKey => 3,
        PersonalMessageType::Whisper => 1,
    });
    raw.extend_from_slice(&message.body);
    let message = STANDARD.encode(raw);
    validate_cipher(&message, false)?;
    Ok(EngineCiphertext {
        address: address.clone(),
        remote_iid: *remote_registration_id,
        message,
        properties: None,
    })
}

/// Restores authority after restart from the store's versioned verified marker,
/// immutable exact payload hash, current account/generation and active epochs.
/// This prepares an authenticated HTTP payload; it performs no network I/O.
pub struct AuthorizedSms {
    receipt: DispatchReceipt,
    parameters: SmsParameters,
}
impl AuthorizedSms {
    pub fn parameters(&self) -> &SmsParameters {
        &self.parameters
    }
    pub fn exact_json(&self) -> &[u8] {
        self.receipt.ciphertext()
    }
    pub fn can_attempt(&self, gate: &GenerationGate) -> bool {
        self.receipt.can_dispatch(gate)
    }
}
pub(crate) fn sms_from_dispatch(
    receipt: DispatchReceipt,
    gate: &GenerationGate,
) -> Result<AuthorizedSms> {
    if !receipt.can_dispatch(gate) || receipt.verified_codec() != Some(1) {
        return Err(SignalError::Authentication);
    }
    if receipt.ciphertext().len() > MAX_FRAME_BYTES || receipt.metadata().len() > 16 * 1024 {
        return Err(SignalError::Resource);
    }
    let metadata: Value =
        serde_json::from_slice(receipt.metadata()).map_err(|_| SignalError::Invalid)?;
    if metadata.as_object().map(|value| value.len()) != Some(4)
        || metadata["codec"] != 1
        || metadata["kind"] != "personal"
        || metadata["path"] != "/api/e2ee/pc/message/sms"
        || metadata["metric_command"] != 16000
    {
        return Err(SignalError::Unsupported);
    }
    let parameters: Value =
        serde_json::from_slice(receipt.ciphertext()).map_err(|_| SignalError::Invalid)?;
    if parameters.as_object().map(|value| value.len()) != Some(4)
        || parameters["destType"] != 3
        || number(&parameters["clientId"])? != receipt.client_id()
    {
        return Err(SignalError::Scope);
    }
    let thread = receipt
        .thread()
        .strip_prefix("d:")
        .ok_or(SignalError::Scope)?;
    if parameters["threadId"].as_str() != Some(thread) {
        return Err(SignalError::Scope);
    }
    let records: Value = serde_json::from_str(
        parameters["listParams"]
            .as_str()
            .ok_or(SignalError::Invalid)?,
    )
    .map_err(|_| SignalError::Invalid)?;
    let records = bounded_array(&records, 6)?;
    if records.is_empty() {
        return Err(SignalError::Invalid);
    }
    let expected: BTreeSet<_> = receipt
        .peers()
        .iter()
        .map(|peer| {
            (
                peer.uid.as_str(),
                match peer.platform {
                    crate::store::e2ee::Platform::Mobile => 0,
                    crate::store::e2ee::Platform::Web => 1,
                    crate::store::e2ee::Platform::Pc => 2,
                },
            )
        })
        .collect();
    let mut observed = BTreeSet::new();
    for record in records {
        let peer = record["toId"].as_str().ok_or(SignalError::Invalid)?;
        let platform = DevicePlatform::parse(&record["platform"])?;
        if !observed.insert((peer, platform.wire())) {
            return Err(SignalError::Duplicate);
        }
        validate_cipher(
            record["message"].as_str().ok_or(SignalError::Invalid)?,
            false,
        )?;
        if let Some(properties) = record.get("textProperties") {
            validate_cipher(properties.as_str().ok_or(SignalError::Invalid)?, false)?;
        }
    }
    if observed != expected {
        return Err(SignalError::Scope);
    }
    bounded_json(&parameters)?;
    Ok(AuthorizedSms {
        receipt,
        parameters: SmsParameters {
            path: "/api/e2ee/pc/message/sms",
            metric_command: 16000,
            parameters,
        },
    })
}
