//! Independently derived legacy text and socket wire formats from the local PC bundles.
//! Signal/E2EE commands are never downgraded to these legacy endpoints.
use super::client::ClientType;
use crate::domain::{
    canonical_plain_text, InfoContentError, InfoMessageContent, MediaContentError, MediaKind,
    MediaMessageContent, Message, StickerDescriptor, MAX_INFO_BATCH_BYTES, MAX_STICKER_ID,
    MAX_TEXT_BYTES,
};
use aes::cipher::consts::U16;
use aes_gcm::{
    aead::{Aead, Payload},
    AesGcm, KeyInit,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::bufread::{GzDecoder, ZlibDecoder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use url::Url;

use crate::domain::info;

pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_INFLATED: usize = 4 * 1024 * 1024;
pub const MAX_BATCH: usize = 200;
pub const API_VERSION: &str = "681";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadKind {
    Direct,
    Group,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: ThreadKind,
    pub remote_id: String,
}
impl Target {
    pub fn new(kind: ThreadKind, remote_id: &str) -> Result<Self, WireError> {
        numeric_id(remote_id)?;
        Ok(Self {
            kind,
            remote_id: remote_id.into(),
        })
    }
    pub fn thread_id(&self) -> String {
        format!(
            "{}:{}",
            if self.kind == ThreadKind::Direct {
                "d"
            } else {
                "g"
            },
            self.remote_id
        )
    }
    pub fn from_thread(thread: &str) -> Result<Self, WireError> {
        let (kind, id) = thread.split_once(':').ok_or(WireError::Invalid)?;
        Self::new(
            match kind {
                "d" => ThreadKind::Direct,
                "g" => ThreadKind::Group,
                _ => return Err(WireError::Invalid),
            },
            id,
        )
    }
}
pub fn numeric_id(id: &str) -> Result<(), WireError> {
    if id.is_empty()
        || id.len() > 24
        || !id.bytes().all(|b| b.is_ascii_digit())
        || id.starts_with('0')
    {
        return Err(WireError::Invalid);
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    Invalid,
    Oversize,
    Crypto,
    Unsupported,
    Service(i64),
    Authentication,
}
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Invalid messaging response",
            Self::Oversize => "Messaging response exceeds limit",
            Self::Crypto => "Messaging integrity check failed",
            Self::Unsupported => "This message encryption or type is not supported",
            Self::Service(_) => "The server rejected this messaging request",
            Self::Authentication => "The server rejected this session",
        })
    }
}
impl std::error::Error for WireError {}

pub fn text_parameters(
    target: &Target,
    text: &str,
    client_id: u64,
    imei: &str,
) -> Result<Value, WireError> {
    numeric_id(&target.remote_id)?;
    if text.len() > MAX_TEXT_BYTES
        || imei.is_empty()
        || imei.len() > 256
        || client_id == 0
        || client_id > 9_007_199_254_740_991
    {
        return Err(WireError::Invalid);
    }
    let text = canonical_plain_text(text);
    if text.is_empty() {
        return Err(WireError::Invalid);
    }
    let mut v = json!({"message":text,"clientId":client_id,"imei":imei});
    match target.kind {
        ThreadKind::Direct => v["toid"] = json!(target.remote_id),
        ThreadKind::Group => {
            v["grid"] = json!(target.remote_id);
            v["visibility"] = json!(0);
        }
    }
    Ok(v)
}
pub fn send_url(root: &Url, kind: ThreadKind, client_type: ClientType) -> Result<Url, WireError> {
    validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(match kind {
        ThreadKind::Direct => "/api/message/sms",
        ThreadKind::Group => "/api/group/sendmsg",
    });
    url.query_pairs_mut()
        .append_pair("zpw_ver", API_VERSION)
        .append_pair("zpw_type", client_type.api_type());
    Ok(url)
}
pub fn socket_url(root: &Url, now_ms: i64, client_type: ClientType) -> Result<Url, WireError> {
    validate_origin(root, "wss")?;
    let mut url = root.clone();
    url.query_pairs_mut()
        .append_pair("zpw_ver", API_VERSION)
        .append_pair("zpw_type", client_type.api_type())
        .append_pair("t", &now_ms.to_string());
    Ok(url)
}
pub fn validate_origin(url: &Url, scheme: &str) -> Result<(), WireError> {
    let host = url.host_str().ok_or(WireError::Invalid)?;
    if url.scheme() != scheme
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        // The bundle preserves the advertised WebSocket path when adding its
        // common query parameters. HTTP message services remain origin roots.
        || (scheme != "wss" && url.path() != "/")
        || !(host == "zalo.me"
            || host.ends_with(".zalo.me")
            || host == "zaloapp.com"
            || host.ends_with(".zaloapp.com"))
    {
        return Err(WireError::Invalid);
    }
    Ok(())
}
pub fn encode_frame(cmd: u16, sub_cmd: u8, data: &Value) -> Result<Vec<u8>, WireError> {
    let body = serde_json::to_vec(data).map_err(|_| WireError::Invalid)?;
    if body.len() > MAX_FRAME - 4 {
        return Err(WireError::Oversize);
    }
    let mut result = Vec::with_capacity(body.len() + 4);
    result.extend_from_slice(&[1, cmd as u8, (cmd >> 8) as u8, sub_cmd]);
    result.extend_from_slice(&body);
    Ok(result)
}
#[derive(Debug)]
pub struct DecodedFrame {
    pub cmd: u16,
    pub sub_cmd: u8,
    pub payload: Value,
}
/// Fixed, secret-free parser locations; no peer-controlled values are retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameReason {
    MessageType,
    FrameLength,
    OuterJson,
    OuterEnvelope,
    AuthKey,
    EncryptionMode,
    OuterData,
    Base64,
    CipherIntegrity,
    Compression,
    InnerJson,
    InnerEnvelope,
    PlaintextAuth,
    QueueCommands,
    BatchEnvelope,
    BatchLimit,
    MessageTypeField,
    MessageSender,
    MessageRecipient,
    MessageScope,
    MessageTimestamp,
    MessageId,
    MessageContent,
    MessageValidation,
    MessageClientId,
    QueueStatus,
    QueueKey,
    QueueEvicted,
    QueueCursor,
    QueueMore,
    QueueContinuation,
    QueueProgress,
    QueuePageLimit,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FrameValueType {
    #[default]
    Absent,
    Null,
    Integer,
    Number,
    String,
    Boolean,
    Object,
    Array,
}
impl FrameValueType {
    pub(crate) fn of(value: Option<&Value>) -> Self {
        match value {
            None => Self::Absent,
            Some(Value::Null) => Self::Null,
            Some(Value::Number(n)) if n.is_i64() || n.is_u64() => Self::Integer,
            Some(Value::Number(_)) => Self::Number,
            Some(Value::String(_)) => Self::String,
            Some(Value::Bool(_)) => Self::Boolean,
            Some(Value::Object(_)) => Self::Object,
            Some(Value::Array(_)) => Self::Array,
        }
    }
}
/// Recognized source message families, never an arbitrary peer-provided label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameMessageKind {
    Webchat,
    WebchatInfo,
    Photo,
    WebContent,
    Recommended,
    Poll,
    File,
    Sticker,
    Other,
}
/// Structural data only. Never retains arbitrary JSON, URLs, keys or messages.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameContext {
    pub command: Option<u16>,
    pub subcommand: Option<u8>,
    pub encryption: Option<u8>,
    pub bytes: usize,
    pub status_type: FrameValueType,
    pub data_type: FrameValueType,
    pub field_type: Option<FrameValueType>,
    pub field_bytes: Option<usize>,
    pub message_kind: Option<FrameMessageKind>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFailure {
    pub error: WireError,
    pub reason: FrameReason,
    pub context: FrameContext,
}
impl FrameFailure {
    pub fn new(error: WireError, reason: FrameReason) -> Self {
        Self {
            error,
            reason,
            context: FrameContext::default(),
        }
    }
}
impl std::fmt::Display for FrameFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.error, self.reason)
    }
}
impl std::error::Error for FrameFailure {}
#[derive(Default)]
pub struct FrameDecoder {
    key: Vec<u8>,
    authenticated: bool,
}
impl FrameDecoder {
    pub fn authenticated(&self) -> bool {
        self.authenticated
    }
    /// Decode a receive batch belonging to this authenticated socket. Callers
    /// must also bind their worker/account to the session that opened it.
    /// `idTo` is not a universal recipient-account field in the PC protocol:
    /// incoming direct queues select the conversation by nonzero `uidFrom`.
    pub(crate) fn incoming_messages(
        &self,
        account: &str,
        payload: &Value,
    ) -> Result<Vec<Incoming>, FrameFailure> {
        if !self.authenticated {
            return Err(FrameFailure::new(
                WireError::Authentication,
                FrameReason::MessageScope,
            ));
        }
        incoming_with_scope(account, payload, IncomingScope::AuthenticatedQueue)
    }
    pub fn decode(&mut self, bytes: &[u8]) -> Result<DecodedFrame, WireError> {
        self.decode_detailed(bytes).map_err(|failure| failure.error)
    }
    pub fn decode_detailed(&mut self, bytes: &[u8]) -> Result<DecodedFrame, FrameFailure> {
        let mut reason = FrameReason::FrameLength;
        let mut context = FrameContext {
            bytes: bytes.len().min(MAX_FRAME + 1),
            ..Default::default()
        };
        if bytes.len() >= 4 {
            context.command = Some(u16::from_le_bytes([bytes[1], bytes[2]]));
            context.subcommand = Some(bytes[3]);
        }
        self.decode_with_reason(bytes, &mut reason, &mut context)
            .map_err(|error| FrameFailure {
                error,
                reason,
                context,
            })
    }
    fn decode_with_reason(
        &mut self,
        bytes: &[u8],
        reason: &mut FrameReason,
        context: &mut FrameContext,
    ) -> Result<DecodedFrame, WireError> {
        if bytes.len() > MAX_FRAME {
            return Err(WireError::Oversize);
        }
        if bytes.len() < 6 {
            return Err(WireError::Invalid);
        }
        let cmd = u16::from_le_bytes([bytes[1], bytes[2]]);
        let sub_cmd = bytes[3];
        // The original accepts the first header byte as opaque metadata. Only
        // command/subcommand select processing; it does not gate server version 1.
        *reason = FrameReason::OuterJson;
        let outer: Value = serde_json::from_slice(&bytes[4..]).map_err(|_| WireError::Invalid)?;
        context.status_type = FrameValueType::of(outer.get("error_code"));
        context.data_type = FrameValueType::of(outer.get("data"));
        context.encryption = outer
            .get("encrypt")
            .and_then(Value::as_u64)
            .and_then(|v| u8::try_from(v).ok())
            .filter(|v| *v <= 3);
        *reason = FrameReason::OuterEnvelope;
        if !outer.is_object() {
            return Err(WireError::Invalid);
        }
        // Source _onRawData routes PONG and lightweight PING_ACTIVE before the
        // ordinary outer-success/encryption gates. Missing/null status is legal
        // ONLY for these control commands and cannot authenticate a session.
        if cmd == 2 || cmd == 4 {
            if outer
                .get("error_code")
                .is_some_and(|value| !value.is_null())
            {
                service_code(&outer)?;
            }
            return Ok(DecodedFrame {
                cmd,
                sub_cmd,
                payload: outer,
            });
        }
        service_code(&outer)?;
        if cmd == 1 {
            self.authenticated = false;
            *reason = FrameReason::AuthKey;
            let key = STANDARD
                .decode(
                    outer
                        .get("key")
                        .and_then(Value::as_str)
                        .ok_or(WireError::Invalid)?,
                )
                .map_err(|_| WireError::Crypto)?;
            if ![16, 24, 32].contains(&key.len()) {
                return Err(WireError::Crypto);
            }
            self.key = key;
        } else if !self.authenticated {
            return Err(WireError::Authentication);
        }
        *reason = FrameReason::EncryptionMode;
        let encryption = outer
            .get("encrypt")
            .and_then(Value::as_u64)
            .ok_or(WireError::Invalid)?;
        *reason = FrameReason::OuterData;
        let raw = outer.get("data").ok_or(WireError::Invalid)?;
        if cmd == 1 && encryption == 0 {
            *reason = FrameReason::PlaintextAuth;
            return Err(WireError::Invalid);
        }
        *reason = FrameReason::InnerJson;
        let payload = match encryption {
            0 => match raw {
                Value::String(s) => parse_inner(s.as_bytes())?,
                Value::Object(_) => raw.clone(),
                _ => return Err(WireError::Invalid),
            },
            1..=3 => {
                *reason = FrameReason::OuterData;
                let s = raw.as_str().ok_or(WireError::Invalid)?;
                let decoded = if encryption == 1 {
                    *reason = FrameReason::Base64;
                    STANDARD.decode(s).map_err(|_| WireError::Crypto)?
                } else {
                    *reason = FrameReason::CipherIntegrity;
                    self.decrypt_gcm(s)?
                };
                if encryption == 3 {
                    *reason = FrameReason::InnerJson;
                    parse_inner(&decoded)?
                } else {
                    *reason = FrameReason::Compression;
                    let result = inflate_payload(&decoded)?;
                    *reason = FrameReason::InnerJson;
                    parse_inner(&result)?
                }
            }
            _ => {
                *reason = FrameReason::EncryptionMode;
                return Err(WireError::Unsupported);
            }
        };
        *reason = FrameReason::InnerEnvelope;
        service_code(&payload)?;
        if cmd == 1 {
            self.authenticated = true
        }
        Ok(DecodedFrame {
            cmd,
            sub_cmd,
            payload,
        })
    }
    fn decrypt_gcm(&self, s: &str) -> Result<Vec<u8>, WireError> {
        let decoded = percent_encoding::percent_decode_str(s)
            .decode_utf8()
            .map_err(|_| WireError::Invalid)?;
        let bytes = STANDARD
            .decode(decoded.as_bytes())
            .map_err(|_| WireError::Crypto)?;
        if bytes.len() < 48 {
            return Err(WireError::Crypto);
        }
        let nonce = aes_gcm::Nonce::<U16>::from_slice(&bytes[..16]);
        let input = Payload {
            msg: &bytes[32..],
            aad: &bytes[16..32],
        };
        let result = match self.key.len() {
            16 => AesGcm::<aes::Aes128, U16>::new_from_slice(&self.key)
                .map_err(|_| WireError::Crypto)?
                .decrypt(nonce, input),
            24 => AesGcm::<aes::Aes192, U16>::new_from_slice(&self.key)
                .map_err(|_| WireError::Crypto)?
                .decrypt(nonce, input),
            32 => AesGcm::<aes::Aes256, U16>::new_from_slice(&self.key)
                .map_err(|_| WireError::Crypto)?
                .decrypt(nonce, input),
            _ => return Err(WireError::Crypto),
        };
        result.map_err(|_| WireError::Crypto)
    }
}
/// Pako's default inflate accepts gzip and zlib. Keep strict checksum validation,
/// one complete member, no trailing bytes, and the same inflated-size ceiling.
fn inflate_payload(bytes: &[u8]) -> Result<Vec<u8>, WireError> {
    let mut input = bytes;
    let mut result = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(&mut input)
            .take((MAX_INFLATED + 1) as u64)
            .read_to_end(&mut result)
            .map_err(|_| WireError::Invalid)?;
    } else {
        ZlibDecoder::new(&mut input)
            .take((MAX_INFLATED + 1) as u64)
            .read_to_end(&mut result)
            .map_err(|_| WireError::Invalid)?;
    }
    if result.len() > MAX_INFLATED {
        return Err(WireError::Oversize);
    }
    if !input.is_empty() {
        return Err(WireError::Invalid);
    }
    Ok(result)
}
fn parse_inner(bytes: &[u8]) -> Result<Value, WireError> {
    if bytes.len() > MAX_INFLATED {
        return Err(WireError::Oversize);
    }
    serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)
}
pub fn service_code(value: &Value) -> Result<(), WireError> {
    let code = value
        .get("error_code")
        .and_then(Value::as_i64)
        .ok_or(WireError::Invalid)?;
    match code {
        0 => Ok(()),
        600 | 102 => Err(WireError::Authentication),
        other => Err(WireError::Service(other)),
    }
}
pub fn id_string(v: &Value) -> Result<String, WireError> {
    let id = if let Some(s) = v.as_str() {
        s.to_owned()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        return Err(WireError::Invalid);
    };
    numeric_id(&id)?;
    Ok(id)
}
#[derive(Debug)]
pub struct Incoming {
    pub message: Message,
    pub client_id: Option<String>,
    pub info: Option<InfoMessageContent>,
    pub media: Option<MediaMessageContent>,
    pub unsupported: bool,
    /// Kept in history but never creates or reorders a conversation-list entry (see
    /// `domain::system::list_silent`). Only for messages from someone else.
    pub list_silent: bool,
}
// Stickers carry a catalog address, never an image URL. Unknown/malformed media
// stays an explicit placeholder without poisoning other messages in a batch.
fn sticker_descriptor(message: &Value) -> Option<StickerDescriptor> {
    if message.get("msgType")?.as_str()? != "chat.sticker" || message.get("e2eeExt").is_some() {
        return None;
    }
    let content = message.get("content")?.as_object()?;
    fn number(value: &Value) -> Option<u64> {
        if let Some(number) = value.as_u64() {
            return (number <= MAX_STICKER_ID).then_some(number);
        }
        let string = value.as_str()?;
        if string.is_empty()
            || string.len() > 16
            || !string.bytes().all(|b| b.is_ascii_digit())
            || (string.len() > 1 && string.starts_with('0'))
        {
            return None;
        }
        let number: u64 = string.parse().ok()?;
        (number <= MAX_STICKER_ID).then_some(number)
    }
    let descriptor = StickerDescriptor {
        sticker_id: number(content.get("id")?)?,
        category_id: number(content.get("catId")?)?,
        sticker_type: number(content.get("type")?)?.try_into().ok()?,
    };
    descriptor.validate().ok()?;
    Some(descriptor)
}
/// Actual legacy webchat text and bounded chat.sticker catalog metadata only.
pub fn incoming_messages(account: &str, payload: &Value) -> Result<Vec<Incoming>, WireError> {
    incoming_messages_detailed(account, payload).map_err(|failure| failure.error)
}
/// HTTP previews are authenticated with the same verified account capability as
/// their encrypted request/response, and use the original source-peer routing.
/// No caller-supplied account string can opt into this routing contract.
pub(super) fn incoming_verified_preview(
    session: &super::session::VerifiedSession,
    payload: &Value,
) -> Result<Vec<Incoming>, FrameFailure> {
    incoming_with_scope(
        session.account_id(),
        payload,
        IncomingScope::AuthenticatedQueue,
    )
}
/// Reports only the fixed field that failed, never its value or identifiers.
pub fn incoming_messages_detailed(
    account: &str,
    payload: &Value,
) -> Result<Vec<Incoming>, FrameFailure> {
    incoming_with_scope(account, payload, IncomingScope::RecipientChecked)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum IncomingScope {
    RecipientChecked,
    AuthenticatedQueue,
}
fn incoming_with_scope(
    account: &str,
    payload: &Value,
    scope: IncomingScope,
) -> Result<Vec<Incoming>, FrameFailure> {
    let mut reason = FrameReason::BatchEnvelope;
    let mut context = FrameContext {
        status_type: FrameValueType::of(payload.get("error_code")),
        data_type: FrameValueType::of(payload.get("data")),
        ..Default::default()
    };
    incoming_with_reason(account, payload, scope, &mut reason, &mut context).map_err(|error| {
        FrameFailure {
            error,
            reason,
            context,
        }
    })
}
fn incoming_with_reason(
    account: &str,
    payload: &Value,
    scope: IncomingScope,
    reason: &mut FrameReason,
    context: &mut FrameContext,
) -> Result<Vec<Incoming>, WireError> {
    numeric_id(account)?;
    service_code(payload)?;
    context.field_type = Some(FrameValueType::of(payload.get("data")));
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or(WireError::Invalid)?;
    let mut result = Vec::new();
    let mut info_bytes = 0usize;
    for (field, kind) in [
        ("msgs", ThreadKind::Direct),
        ("groupMsgs", ThreadKind::Group),
        ("pageMsgs", ThreadKind::Direct),
    ] {
        let Some(array) = data.get(field) else {
            continue;
        };
        // Original queue handlers gate optional arrays with `field && field.length`.
        // Null is an empty optional collection, not a malformed message to drop.
        if array.is_null() {
            continue;
        }
        *reason = FrameReason::BatchEnvelope;
        context.message_kind = None;
        context.field_bytes = None;
        context.field_type = Some(FrameValueType::of(Some(array)));
        let array = array.as_array().ok_or(WireError::Invalid)?;
        if array.len() + result.len() > MAX_BATCH {
            *reason = FrameReason::BatchLimit;
            return Err(WireError::Oversize);
        }
        for v in array {
            context.message_kind = None;
            context.field_bytes = None;
            *reason = FrameReason::MessageTypeField;
            context.field_type = Some(FrameValueType::of(v.get("msgType")));
            let message_type = v
                .get("msgType")
                .and_then(Value::as_str)
                .ok_or(WireError::Invalid)?;
            if message_type.is_empty()
                || message_type.len() > 64
                || !message_type
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
            {
                return Err(WireError::Invalid);
            }
            context.message_kind = Some(match message_type {
                "webchat"
                    if v.pointer("/content/action").and_then(Value::as_str)
                        == Some("msginfo.actionlist") =>
                {
                    FrameMessageKind::WebchatInfo
                }
                "webchat" => FrameMessageKind::Webchat,
                "chat.sticker" => FrameMessageKind::Sticker,
                "chat.photo" => FrameMessageKind::Photo,
                "chat.webcontent" => FrameMessageKind::WebContent,
                "chat.recommended" => FrameMessageKind::Recommended,
                "group.poll" => FrameMessageKind::Poll,
                "share.file" => FrameMessageKind::File,
                _ => FrameMessageKind::Other,
            });
            let sticker = sticker_descriptor(v);

            *reason = FrameReason::MessageSender;
            context.field_type = Some(FrameValueType::of(v.get("uidFrom")));
            let raw_sender = v.get("uidFrom").ok_or(WireError::Invalid)?;
            let sender = if raw_sender.as_str() == Some("0") || raw_sender.as_u64() == Some(0) {
                account.to_owned()
            } else {
                id_string(raw_sender)?
            };
            *reason = FrameReason::MessageRecipient;
            context.field_type = Some(FrameValueType::of(v.get("idTo")));
            let raw_to = v.get("idTo").ok_or(WireError::Invalid)?;
            let to = if raw_to.as_str() == Some("0") || raw_to.as_u64() == Some(0) {
                account.to_owned()
            } else {
                id_string(raw_to)?
            };
            *reason = FrameReason::MessageScope;
            // Untrusted batches must explicitly name their recipient. The
            // authenticated PC queue instead uses uidFrom as the direct peer,
            // and idTo may carry a different conversation/event destination.
            // Runtime checks the socket's session account before this path.
            if scope == IncomingScope::RecipientChecked
                && kind == ThreadKind::Direct
                && sender != account
                && to != account
            {
                return Err(WireError::Invalid);
            }
            let peer = if kind == ThreadKind::Group || sender == account {
                to
            } else {
                sender.clone()
            };
            let thread = Target::new(kind, &peer)?.thread_id();
            *reason = FrameReason::MessageTimestamp;
            context.field_type = Some(FrameValueType::of(v.get("ts")));
            let ts = v.get("ts").ok_or(WireError::Invalid)?;
            let ts = if let Some(s) = ts.as_str() {
                s.parse().map_err(|_| WireError::Invalid)?
            } else {
                ts.as_i64().ok_or(WireError::Invalid)?
            };
            if ts < 0 {
                return Err(WireError::Invalid);
            }
            *reason = FrameReason::MessageId;
            context.field_type = Some(FrameValueType::of(v.get("msgId")));
            let id = id_string(v.get("msgId").ok_or(WireError::Invalid)?)?;
            *reason = FrameReason::MessageContent;
            context.field_type = Some(FrameValueType::of(v.get("content")));
            context.field_bytes = v
                .get("content")
                .and_then(Value::as_str)
                .map(|text| text.len().min(MAX_TEXT_BYTES + 1));
            let info = if context.message_kind == Some(FrameMessageKind::WebchatInfo)
                && v.get("e2eeExt").is_none()
            {
                let content = InfoMessageContent::new(v.get("content").ok_or(WireError::Invalid)?)
                    .map_err(|error| match error {
                        InfoContentError::Invalid => WireError::Invalid,
                        InfoContentError::Oversize => WireError::Oversize,
                    })?;
                info_bytes = info_bytes
                    .checked_add(content.as_json().len())
                    .ok_or(WireError::Oversize)?;
                if info_bytes > MAX_INFO_BATCH_BYTES {
                    *reason = FrameReason::BatchLimit;
                    return Err(WireError::Oversize);
                }
                Some(content)
            } else {
                None
            };
            let media = if let Some(kind) = MediaKind::from_wire(message_type) {
                // Encrypted messages cannot be projected into the legacy media path.
                if v.get("e2eeExt").is_some()
                    || ["isE2EE", "isE2ee", "e2ee"].iter().any(|key| {
                        v.get(key).is_some_and(|value| {
                            !matches!(value, Value::Null | Value::Bool(false))
                                && value.as_u64() != Some(0)
                        })
                    })
                {
                    return Err(WireError::Unsupported);
                }
                let content =
                    MediaMessageContent::new(kind, v.get("content").ok_or(WireError::Invalid)?)
                        .map_err(|error| match error {
                            MediaContentError::Invalid => WireError::Invalid,
                            MediaContentError::Oversize => WireError::Oversize,
                        })?;
                info_bytes = info_bytes
                    .checked_add(content.as_json().len())
                    .ok_or(WireError::Oversize)?;
                if info_bytes > MAX_INFO_BATCH_BYTES {
                    *reason = FrameReason::BatchLimit;
                    return Err(WireError::Oversize);
                }
                Some(content)
            } else {
                None
            };
            let presented = (message_type != "webchat" && sticker.is_none() && media.is_none())
                .then(|| {
                    v.get("content").and_then(|content| {
                        crate::domain::system::presentation(message_type, content)
                    })
                })
                .flatten()
                .filter(|text| !text.is_empty());
            let unsupported = message_type != "webchat"
                && sticker.is_none()
                && media.is_none()
                && presented.is_none();
            // Official Account broadcasts only exist in 1-1 threads; birthday cards are skipped everywhere.
            let list_silent = (kind == ThreadKind::Direct || message_type == "chat.ecard")
                && sender != account
                && v.get("content").is_some_and(|content| {
                    crate::domain::system::list_silent(message_type, content)
                });
            let message = Message {
                account_id: account.into(),
                thread_id: thread,
                id,
                sender,
                text: if info.is_some() {
                    info::presentation(account, v.get("content").ok_or(WireError::Invalid)?)
                } else if let Some(text) = presented {
                    text
                } else if message_type != "webchat" {
                    format!("[Tin nhắn {message_type} chưa được hỗ trợ]")
                } else {
                    v.get("content")
                        .and_then(Value::as_str)
                        .ok_or(WireError::Invalid)?
                        .into()
                },
                sticker,
                media: media.as_ref().map(|content| content.descriptor().clone()),
                sent_at_ms: ts,
            };
            *reason = FrameReason::MessageValidation;
            message.validate().map_err(|_| WireError::Invalid)?;
            context.field_bytes = None;
            *reason = FrameReason::MessageClientId;
            context.field_type = Some(FrameValueType::of(v.get("cliMsgId")));
            // cliMsgId is an optional correlation hint. The PC transform stringifies
            // it independently from server msgId. Zero/null carry no usable
            // client-generated correlation. Never treat these sentinels
            // as an outbox identity; durable server binding remains authoritative.
            let client_id = match v.get("cliMsgId") {
                None | Some(Value::Null) => None,
                Some(Value::Number(n)) if n.as_u64() == Some(0) => None,
                Some(Value::String(s)) if s == "0" => None,
                Some(value) => Some(id_string(value)?),
            };
            // An information event is never an acknowledgement of a reserved
            // plain-text send, even when its server-supplied cliMsgId matches.
            // Only a plain text message can acknowledge a plain-text send.
            let client_id = client_id
                .filter(|_| info.is_none() && media.is_none() && message_type == "webchat");
            result.push(Incoming {
                message,
                client_id,
                info,
                media,
                unsupported,
                list_silent,
            });
        }
    }
    Ok(result)
}
#[derive(Debug, PartialEq, Eq)]
pub struct SendAck {
    pub server_id: String,
    pub timestamp_ms: Option<i64>,
}
/// Call only after decrypting both the outer HTTP envelope and session-encrypted inner envelope.
pub fn send_ack(payload: &Value) -> Result<SendAck, WireError> {
    service_code(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    let server_id = id_string(data.get("msgId").ok_or(WireError::Invalid)?)?;
    let timestamp_ms = data
        .get("ts")
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.parse().map_err(|_| WireError::Invalid)
            } else {
                v.as_i64().ok_or(WireError::Invalid)
            }
        })
        .transpose()?;
    if timestamp_ms.is_some_and(|t| t < 0) {
        return Err(WireError::Invalid);
    }
    Ok(SendAck {
        server_id,
        timestamp_ms,
    })
}

/// The reply to an end-to-end encrypted send, read as the original's `parseE2eeResp` does
/// (`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`, line 547494):
/// the server's message id is `minGlobalMsgId` (a plain send answers `msgId`), plus `ts`. The per-device results
/// the reply may carry are not read, exactly as in the original.
pub fn e2ee_send_ack(payload: &Value) -> Result<SendAck, WireError> {
    service_code(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    let id = data
        .get("minGlobalMsgId")
        .or_else(|| data.get("msgId"))
        .ok_or(WireError::Invalid)?;
    let mut shaped = serde_json::Map::new();
    shaped.insert("msgId".into(), id.clone());
    if let Some(ts) = data.get("ts") {
        shaped.insert("ts".into(), ts.clone());
    }
    send_ack(&serde_json::json!({"error_code": 0, "data": shaped}))
}

/// Bind a synchronous encrypted-send reply to its durable attempt when the service
/// echoes a client ID. Older replies without that field remain HTTP-request bound.
pub fn e2ee_send_ack_for(payload: &Value, expected_client_id: u64) -> Result<SendAck, WireError> {
    let ack = e2ee_send_ack(payload)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    if let (Some(global), Some(message)) = (data.get("minGlobalMsgId"), data.get("msgId")) {
        if id_string(global)? != id_string(message)? {
            return Err(WireError::Invalid);
        }
    }
    if let Some(client) = payload.get("data").and_then(|data| data.get("clientId")) {
        if id_string(client)? != expected_client_id.to_string() {
            return Err(WireError::Invalid);
        }
    }
    Ok(ack)
}

/// Closed diagnostic labels, never peer IDs or arbitrary payload field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityReason {
    EncryptedTarget,
    Schema,
    Correlation,
    Timeout,
    StateChanged,
    Pagination,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityField {
    Envelope,
    Request,
    Status,
    More,
    Threads,
    Waiting,
    Uids,
    Pending,
    Cursor,
    Group,
    Encryption,
    Visibility,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EligibilityFailure {
    pub error: WireError,
    pub reason: EligibilityReason,
    pub field: Option<EligibilityField>,
    pub value_type: FrameValueType,
    pub command: Option<u16>,
}
impl EligibilityFailure {
    pub fn new(
        reason: EligibilityReason,
        field: Option<EligibilityField>,
        value: Option<&Value>,
    ) -> Self {
        Self {
            error: WireError::Unsupported,
            reason,
            field,
            value_type: FrameValueType::of(value),
            command: None,
        }
    }
}
impl std::fmt::Display for EligibilityFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Conversation encryption eligibility was not established")
    }
}
impl std::error::Error for EligibilityFailure {}
fn eligibility_schema(field: EligibilityField, value: Option<&Value>) -> EligibilityFailure {
    EligibilityFailure::new(EligibilityReason::Schema, Some(field), value)
}
fn eligibility_status(payload: &Value) -> Result<(), EligibilityFailure> {
    service_code(payload).map_err(|error| EligibilityFailure {
        error,
        ..eligibility_schema(EligibilityField::Status, payload.get("error_code"))
    })
}
/// The original rejects a list only when `err` is truthy, and the signal service leaves
/// zero-valued fields out, so a missing, null, false or zero `err` is success. Anything else,
/// including a non-empty string, is a refusal. (The frame decoder has already required a zero
/// `error_code` on the envelope, so only this inner status can be absent.)
fn signal_list_status(data: &serde_json::Map<String, Value>) -> Result<(), EligibilityFailure> {
    match data.get("err") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(()),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => Ok(()),
        value => Err(eligibility_schema(EligibilityField::Status, value)),
    }
}
/// Compatibility wrapper; runtime consumes detailed, content-free rejection reasons.
pub fn legacy_direct_allowed(
    payload: &Value,
    request_id: &str,
    target: &str,
) -> Result<(bool, Option<String>), WireError> {
    legacy_direct_allowed_detailed(payload, request_id, target).map_err(|failure| failure.error)
}
/// Source GET_LIST terminal logging dereferences all four arrays. Only falsey
/// pagination flags are optional; missing arrays never establish eligibility.
pub fn legacy_direct_allowed_detailed(
    payload: &Value,
    request_id: &str,
    target: &str,
) -> Result<(bool, Option<String>), EligibilityFailure> {
    let page = e2ee_list_page(payload, request_id)?;
    let encrypted = page
        .enabled
        .iter()
        .chain(&page.others)
        .any(|uid| uid == target);
    Ok((!encrypted, page.next))
}
/// One validated page of the account's E2EE list (command 563).
pub struct E2eeListPage {
    /// `threads` and `uids`: peers with end-to-end encryption enabled.
    pub enabled: Vec<String>,
    /// `waitUids` and `pendingUids`: peers in a transitional state. Sending is refused for them too.
    others: Vec<String>,
    pub next: Option<String>,
}
/// Validates the whole page exactly as the eligibility check always has, and keeps the peer ids.
pub fn e2ee_list_page(
    payload: &Value,
    request_id: &str,
) -> Result<E2eeListPage, EligibilityFailure> {
    eligibility_status(payload)?;
    let response = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| eligibility_schema(EligibilityField::Envelope, payload.get("data")))?;
    if response.get("reqId").and_then(Value::as_str) != Some(request_id) {
        return Err(EligibilityFailure::new(
            EligibilityReason::Correlation,
            Some(EligibilityField::Request),
            response.get("reqId"),
        ));
    }
    let data = response
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| eligibility_schema(EligibilityField::Envelope, response.get("data")))?;
    signal_list_status(data)?;
    let has_more = match data.get("hasMore") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::Number(number)) if number.as_u64() == Some(0) => false,
        Some(Value::Number(number)) if number.as_u64() == Some(1) => true,
        value => return Err(eligibility_schema(EligibilityField::More, value)),
    };
    let (mut enabled, mut others) = (Vec::new(), Vec::new());
    for (name, field) in [
        ("threads", EligibilityField::Threads),
        ("waitUids", EligibilityField::Waiting),
        ("uids", EligibilityField::Uids),
        ("pendingUids", EligibilityField::Pending),
    ] {
        let value = data.get(name);
        let entries = value
            .and_then(Value::as_array)
            .filter(|entries| entries.len() <= 100_000)
            .ok_or_else(|| eligibility_schema(field, value))?;
        for entry in entries {
            let uid = if matches!(name, "threads" | "pendingUids") {
                entry.get("uid")
            } else {
                Some(entry)
            };
            let uid = id_string(uid.ok_or_else(|| eligibility_schema(field, Some(entry)))?)
                .map_err(|_| eligibility_schema(field, Some(entry)))?;
            match name {
                "threads" | "uids" => enabled.push(uid),
                _ => others.push(uid),
            }
        }
    }
    let next = if has_more {
        Some(
            id_string(
                data.get("lastId")
                    .ok_or_else(|| eligibility_schema(EligibilityField::Cursor, None))?,
            )
            .map_err(|_| eligibility_schema(EligibilityField::Cursor, data.get("lastId")))?,
        )
    } else {
        None
    };
    Ok(E2eeListPage {
        enabled,
        others,
        next,
    })
}
pub fn legacy_group_allowed(payload: &Value, target: &str) -> Result<u64, WireError> {
    legacy_group_allowed_detailed(payload, target).map_err(|failure| failure.error)
}
pub fn legacy_group_allowed_detailed(
    payload: &Value,
    target: &str,
) -> Result<u64, EligibilityFailure> {
    eligibility_status(payload)?;
    let data = payload
        .get("data")
        .ok_or_else(|| eligibility_schema(EligibilityField::Envelope, None))?;
    let group = id_string(
        data.get("groupId")
            .ok_or_else(|| eligibility_schema(EligibilityField::Group, None))?,
    )
    .map_err(|_| eligibility_schema(EligibilityField::Group, data.get("groupId")))?;
    if group != target {
        return Err(EligibilityFailure::new(
            EligibilityReason::Correlation,
            Some(EligibilityField::Group),
            data.get("groupId"),
        ));
    }
    let visibility = match data.get("visibility") {
        None => 0,
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= 255)
            .ok_or_else(|| eligibility_schema(EligibilityField::Visibility, Some(value)))?,
    };
    match data.get("e2ee") {
        Some(Value::Bool(false)) => Ok(visibility),
        Some(Value::Number(number)) if number.as_u64() == Some(0) => Ok(visibility),
        Some(Value::Bool(true)) => Err(EligibilityFailure::new(
            EligibilityReason::EncryptedTarget,
            Some(EligibilityField::Encryption),
            data.get("e2ee"),
        )),
        Some(Value::Number(number)) if number.as_u64() == Some(1) => Err(EligibilityFailure::new(
            EligibilityReason::EncryptedTarget,
            Some(EligibilityField::Encryption),
            data.get("e2ee"),
        )),
        value => Err(eligibility_schema(EligibilityField::Encryption, value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authenticated() -> FrameDecoder {
        FrameDecoder {
            key: (0..32).collect(),
            authenticated: true,
        }
    }
    #[test]
    fn an_encrypted_send_reply_is_read_from_min_global_msg_id_and_ts() {
        let ack = e2ee_send_ack(&json!({"error_code":0,"data":{
            "clientId":"1760000000001","minGlobalMsgId":"9001","ts":1700000000123u64,
            "results":[{"deviceId":0,"err":0}]}}))
        .unwrap();
        assert_eq!(ack.server_id, "9001");
        assert_eq!(ack.timestamp_ms, Some(1_700_000_000_123));
        // A numeric id, a string timestamp and the plain `msgId` spelling all work.
        let ack = e2ee_send_ack(&json!({"error_code":0,"data":{"msgId":77,"ts":"5"}})).unwrap();
        assert_eq!((ack.server_id.as_str(), ack.timestamp_ms), ("77", Some(5)));
        // No id at all, or a service error, is refused.
        assert!(e2ee_send_ack(&json!({"error_code":0,"data":{"ts":1}})).is_err());
        assert!(e2ee_send_ack(&json!({"error_code":5,"data":{"minGlobalMsgId":"1"}})).is_err());
    }
    #[test]
    fn encrypted_ack_binds_present_client_id_and_rejects_conflicting_server_ids() {
        for client in [json!(123), json!("123")] {
            assert!(e2ee_send_ack_for(
                &json!({"error_code":0,"data":{"clientId":client,"minGlobalMsgId":"9001","ts":5}}),
                123
            )
            .is_ok());
        }
        for client in [
            json!(124),
            json!("124"),
            json!(null),
            json!(false),
            json!({}),
            json!("0123"),
        ] {
            assert!(e2ee_send_ack_for(
                &json!({"error_code":0,"data":{"clientId":client,"minGlobalMsgId":"9001"}}),
                123
            )
            .is_err());
        }
        // A synchronous older HTTP response may omit the echo; no global event is accepted here.
        assert!(e2ee_send_ack_for(&json!({"error_code":0,"data":{"msgId":9001}}), 123).is_ok());
        assert!(e2ee_send_ack_for(
            &json!({"error_code":0,"data":{"msgId":9001,"minGlobalMsgId":"9001"}}),
            123
        )
        .is_ok());
        assert!(e2ee_send_ack_for(
            &json!({"error_code":0,"data":{"msgId":9002,"minGlobalMsgId":"9001"}}),
            123
        )
        .is_err());
        assert!(e2ee_send_ack_for(
            &json!({"error_code":0,"data":{"clientId":"123","minGlobalMsgId":"9001","ts":-1}}),
            123
        )
        .is_err());
    }
    fn frame(cmd: u16, value: Value) -> Vec<u8> {
        encode_frame(cmd, 0, &value).unwrap()
    }
    fn incoming_fixture() -> Value {
        json!({"error_code":0,"data":{"msgs":[{
            "uidFrom":"22","idTo":"11","msgId":"811","cliMsgId":"1760000000001",
            "ts":"1760000000001","msgType":"webchat","content":"synthetic text"
        }]}})
    }
    #[test]
    fn source_media_retains_full_metadata_without_plaintext_ack_or_scope_downgrade() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/media.json"))
                .unwrap();
        let cards: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/rich_cards.json"
        ))
        .unwrap();
        let files: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/files.json"))
                .unwrap();
        for case in fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .chain(cards["cases"].as_array().unwrap())
            .chain(files["cases"].as_array().unwrap())
        {
            let mut payload = incoming_fixture();
            let message = &mut payload["data"]["msgs"][0];
            message["msgType"] = case["kind"].clone();
            message["content"] = case["content"].clone();
            let mut parsed = incoming_messages("11", &payload).unwrap();
            let incoming = parsed.pop().unwrap();
            assert!(
                !incoming.unsupported && incoming.client_id.is_none() && incoming.info.is_none()
            );
            assert_eq!(
                serde_json::from_str::<Value>(incoming.media.as_ref().unwrap().as_json()).unwrap(),
                case["content"]
            );
            assert_eq!(
                incoming.message.media.as_ref(),
                Some(incoming.media.as_ref().unwrap().descriptor())
            );
            assert_eq!(
                incoming.message.text,
                incoming.media.unwrap().kind().legacy_placeholder()
            );
            payload["data"]["msgs"][0]["idTo"] = json!("33");
            assert_eq!(
                incoming_messages_detailed("11", &payload)
                    .unwrap_err()
                    .reason,
                FrameReason::MessageScope
            );
            for marker in ["e2eeExt", "isE2EE", "isE2ee", "e2ee"] {
                let mut encrypted = payload.clone();
                encrypted["data"]["msgs"][0]["idTo"] = json!("11");
                encrypted["data"]["msgs"][0][marker] = json!(true);
                assert_eq!(
                    incoming_messages("11", &encrypted).unwrap_err(),
                    WireError::Unsupported
                );
            }
            payload["data"]["msgs"][0]["idTo"] = json!("11");
            let valid = payload["data"]["msgs"][0].clone();
            payload["data"]["msgs"][0]["content"] = Value::Null;
            payload["data"]["msgs"]
                .as_array_mut()
                .unwrap()
                .insert(0, valid);
            assert_eq!(
                incoming_messages_detailed("11", &payload)
                    .unwrap_err()
                    .reason,
                FrameReason::MessageContent
            );
        }
    }

    #[test]
    fn authenticated_queue_routes_source_peers_without_trusting_recipient_account_hint() {
        for field in ["msgs", "pageMsgs"] {
            for message_type in ["webchat", "chat.recommended"] {
                let mut payload = incoming_fixture();
                let mut message = payload["data"]["msgs"][0].clone();
                message["idTo"] = json!("33");
                message["msgType"] = json!(message_type);
                if message_type == "chat.recommended" {
                    message["content"] =
                        json!({"action":"recommened.link","title":"Synthetic link"});
                }
                payload["data"] = json!({field:[message]});
                assert_eq!(
                    incoming_messages_detailed("11", &payload)
                        .unwrap_err()
                        .reason,
                    FrameReason::MessageScope
                );
                assert_eq!(
                    FrameDecoder::default()
                        .incoming_messages("11", &payload)
                        .unwrap_err()
                        .error,
                    WireError::Authentication
                );
                let received = authenticated().incoming_messages("11", &payload).unwrap();
                assert_eq!(received.len(), 1);
                assert_eq!(received[0].message.account_id, "11");
                assert_eq!(received[0].message.thread_id, "d:22");
                assert_eq!(received[0].message.sender, "22");
                assert_eq!(received[0].message.id, "811");
                assert!(!received[0].unsupported);
            }
        }
        for (sender, recipient, expected_sender, expected_thread) in [
            ("22", "0", "22", "d:22"),
            ("0", "33", "11", "d:33"),
            ("11", "33", "11", "d:33"),
            ("0", "0", "11", "d:11"),
        ] {
            let mut payload = incoming_fixture();
            payload["data"]["msgs"][0]["uidFrom"] = json!(sender);
            payload["data"]["msgs"][0]["idTo"] = json!(recipient);
            let received = authenticated().incoming_messages("11", &payload).unwrap();
            assert_eq!(received[0].message.sender, expected_sender);
            assert_eq!(received[0].message.thread_id, expected_thread);
        }
        let mut payload = incoming_fixture();
        let message = payload["data"]["msgs"][0].clone();
        payload["data"] = json!({"groupMsgs":[message]});
        payload["data"]["groupMsgs"][0]["idTo"] = json!("33");
        assert_eq!(
            authenticated().incoming_messages("11", &payload).unwrap()[0]
                .message
                .thread_id,
            "g:33"
        );
        for field in ["uidFrom", "idTo", "msgId"] {
            let mut payload = incoming_fixture();
            payload["data"]["msgs"][0][field] = json!("untrusted_identity");
            assert!(authenticated().incoming_messages("11", &payload).is_err());
        }
        // A payload parsed without an authenticated socket cannot reassign data
        // from another account merely by supplying a different parser account.
        assert_eq!(
            incoming_messages_detailed("33", &incoming_fixture())
                .unwrap_err()
                .reason,
            FrameReason::MessageScope
        );
    }
    #[test]
    fn recognized_group_info_retains_full_content_and_cannot_acknowledge_plaintext() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/info.json")).unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let mut message = incoming_fixture()["data"]["msgs"][0].clone();
            message["idTo"] = json!("33");
            message["content"] = case["content"].clone();
            let payload = json!({"error_code":0,"data":{"groupMsgs":[message]}});
            let result = authenticated().incoming_messages("11", &payload).unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].message.thread_id, "g:33");
            assert_eq!(result[0].message.text, case["text"].as_str().unwrap());
            assert_eq!(result[0].message.sender, "22");
            assert!(!result[0].unsupported);
            assert_eq!(result[0].client_id, None);
            assert_eq!(
                serde_json::from_str::<Value>(result[0].info.as_ref().unwrap().as_json()).unwrap(),
                case["content"]
            );
        }
        let mut message = incoming_fixture()["data"]["msgs"][0].clone();
        message["content"] = json!({"action":"msginfo.actionlist","title":"Thông báo"});
        message["e2eeExt"] = json!({"cipher":"not-plaintext"});
        let payload = json!({"error_code":0,"data":{"msgs":[message]}});
        assert_eq!(
            authenticated()
                .incoming_messages("11", &payload)
                .unwrap_err()
                .reason,
            FrameReason::MessageContent
        );
    }
    #[test]
    fn information_content_and_batch_payload_limits_fail_before_any_partial_batch() {
        let mut message = incoming_fixture()["data"]["msgs"][0].clone();
        message["content"] = json!({"action":"msginfo.actionlist","title":"Thông báo",
            "unknown":"x".repeat(crate::domain::MAX_INFO_CONTENT_BYTES)});
        let payload = json!({"error_code":0,"data":{"msgs":[message.clone()]}});
        let failure = authenticated()
            .incoming_messages("11", &payload)
            .unwrap_err();
        assert_eq!(failure.error, WireError::Oversize);
        assert_eq!(failure.reason, FrameReason::MessageContent);
        message["content"]["unknown"] = json!("x".repeat(MAX_INFO_BATCH_BYTES / 2));
        let payload = json!({"error_code":0,"data":{"msgs":[message.clone(),message]}});
        let failure = authenticated()
            .incoming_messages("11", &payload)
            .unwrap_err();
        assert_eq!(failure.error, WireError::Oversize);
        assert_eq!(failure.reason, FrameReason::BatchLimit);
    }
    #[test]
    fn optional_client_correlations_and_null_arrays_preserve_messages_and_identity() {
        for sender in ["11", "22"] {
            for client_id in [
                None,
                Some(Value::Null),
                Some(json!(0)),
                Some(json!("0")),
                Some(json!("1760000000001")),
            ] {
                let mut payload = incoming_fixture();
                let message = &mut payload["data"]["msgs"][0];
                message["uidFrom"] = json!(sender);
                message["idTo"] = json!(if sender == "11" { "22" } else { "11" });
                if let Some(value) = &client_id {
                    message["cliMsgId"] = value.clone();
                } else {
                    message.as_object_mut().unwrap().remove("cliMsgId");
                }
                payload["data"]["groupMsgs"] = Value::Null;
                payload["data"]["pageMsgs"] = Value::Null;
                let messages = incoming_messages_detailed("11", &payload).unwrap();
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].message.sender, sender);
                assert_eq!(messages[0].message.id, "811");
                assert_eq!(messages[0].message.thread_id, "d:22");
                assert_eq!(messages[0].message.text, "synthetic text");
                assert_eq!(
                    messages[0].client_id.as_deref(),
                    if client_id == Some(json!("1760000000001")) {
                        Some("1760000000001")
                    } else {
                        None
                    }
                );
            }
        }
    }
    #[test]
    fn detailed_incoming_rejections_identify_the_field_without_retaining_peer_data() {
        let cases = [
            (
                "msgType",
                json!({"private_type":"secret"}),
                FrameReason::MessageTypeField,
            ),
            (
                "uidFrom",
                json!("private_sender"),
                FrameReason::MessageSender,
            ),
            (
                "idTo",
                json!("private_recipient"),
                FrameReason::MessageRecipient,
            ),
            ("idTo", json!("33"), FrameReason::MessageScope),
            (
                "ts",
                json!("private_timestamp"),
                FrameReason::MessageTimestamp,
            ),
            ("msgId", json!(0), FrameReason::MessageId),
            (
                "content",
                json!({"action":"unknown.action","private_body":"secret"}),
                FrameReason::MessageContent,
            ),
            (
                "content",
                json!("x".repeat(MAX_TEXT_BYTES + 1)),
                FrameReason::MessageValidation,
            ),
            (
                "cliMsgId",
                json!({"private_correlation":"secret"}),
                FrameReason::MessageClientId,
            ),
        ];
        for (field, value, reason) in cases {
            let mut payload = incoming_fixture();
            payload["data"]["msgs"][0][field] = value;
            let failure = incoming_messages_detailed("11", &payload).unwrap_err();
            assert_eq!(failure.reason, reason);
            assert_eq!(failure.error, WireError::Invalid);
            if reason == FrameReason::MessageContent {
                assert_eq!(failure.context.field_type, Some(FrameValueType::Object));
                assert_eq!(
                    failure.context.message_kind,
                    Some(FrameMessageKind::Webchat)
                );
                assert_eq!(failure.context.field_bytes, None);
            } else if reason == FrameReason::MessageValidation {
                assert_eq!(failure.context.field_type, Some(FrameValueType::String));
                assert_eq!(
                    failure.context.message_kind,
                    Some(FrameMessageKind::Webchat)
                );
                assert_eq!(failure.context.field_bytes, Some(MAX_TEXT_BYTES + 1));
            }
            assert!(!format!("{failure:?}").contains("private_"));
            assert!(!format!("{failure}").contains("secret"));
        }
        for field in ["uidFrom", "idTo", "msgId"] {
            for value in [Value::Null, json!({}), json!("private_identity")] {
                let mut payload = incoming_fixture();
                payload["data"]["msgs"][0][field] = value;
                assert!(incoming_messages_detailed("11", &payload).is_err());
            }
        }
        let mut payload = incoming_fixture();
        payload["data"]["groupMsgs"] = json!({"private_batch":"secret"});
        assert_eq!(
            incoming_messages_detailed("11", &payload)
                .unwrap_err()
                .reason,
            FrameReason::BatchEnvelope
        );
    }
    #[test]
    fn socket_aes_gcm_matches_independent_node_16_byte_iv_aad_and_zlib_fixture() {
        let fixtures=[(2,"oKGio6SlpqeoqaqrrK2ur8DBwsPExcbHyMnKy8zNzs9SP4jqUuDnvOwUUH+iXdgisJUSMzcYGPhPuDx8NHsJ0b4bSDW/HvPMxcP/96oQhcJI6PaomAxkJlQ0c+XNxbomHb4x0MgMY7hWX82d/yAxKHquoJnb0RQN7pFBDZpr+V5yVnKVu1ajGSkbvNzLKDwdg7w4OVZ71wLw30lzGhltpMZxTC4MzS4F4nNvrKgBsSSr6JbP+w4eed2V7C4bKhyCUFPIo7GtZdhM"),(3,"oKGio6SlpqeoqaqrrK2ur8DBwsPExcbHyMnKy8zNzs9RgUbOaqK/KaDxe9TPLr28oCEiDR8Wa9fffxnRahcc3Hezkwrxlc3SgysKgCpwldw4U8M2uyeXA9cIXEiDCNOOQPrUOCr1j1izvDxwCcnB35xMgYh9b6vGhmgddN4/MDnwOBmVFkBHgYnhjkzojMZ+MgIpX24Zi8QmfdulvymYtsSyaIwO4S3v2PDBNPrchKwcZ0yvmUyPLSj7mbjNkSGM0b3fZXMlZOOTUcxkqtSHEdNulGbXymzNOacnXwZ9CtvCs0nCDQYvFeS2")];
        for (encryption, data) in fixtures {
            let mut decoder = authenticated();
            let decoded = decoder
                .decode(&frame(
                    501,
                    json!({"error_code":0,"encrypt":encryption,"data":data}),
                ))
                .unwrap();
            let messages = incoming_messages("11", &decoded.payload).unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].message.text, "Xin chào Linux");
            assert_eq!(messages[0].message.id, "9007199254740993");
            assert_eq!(messages[0].message.thread_id, "d:22");
            let mut corrupted = STANDARD.decode(data).unwrap();
            *corrupted.last_mut().unwrap() ^= 1;
            assert_eq!(authenticated().decode(&frame(501,json!({"error_code":0,"encrypt":encryption,"data":STANDARD.encode(corrupted)}))).unwrap_err(),WireError::Crypto);
        }
    }
    #[test]
    fn original_post_auth_oracle_routes_only_control_status_exceptions_and_decodes_api_layers() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/post_auth_frames.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let bytes = STANDARD.decode(case["frame"].as_str().unwrap()).unwrap();
            let result = authenticated().decode_detailed(&bytes);
            if case["route"] == "rejected" {
                assert!(result.is_err(), "invalid message cannot cross status gates");
                continue;
            }
            let decoded = result.unwrap();
            if case["route"] == "pong" {
                assert_eq!(decoded.cmd, 2);
                assert_eq!(decoded.payload["eventId"], 1700000000000_i64)
            } else {
                assert_eq!(decoded.payload, case["payload"])
            }
        }
        let mut decoder = FrameDecoder::default();
        assert!(decoder
            .decode_detailed(&frame(2, json!({"eventId":123})))
            .is_ok());
        assert!(decoder
            .decode_detailed(&frame(4, json!({"data":{"reqId":"cmd_4"}})))
            .is_ok());
        assert!(!decoder.authenticated());
        let error = authenticated()
            .decode_detailed(&frame(
                501,
                json!({"encrypt":0,"data":"private-synthetic-message"}),
            ))
            .unwrap_err();
        assert_eq!(error.reason, FrameReason::OuterEnvelope);
        assert_eq!(error.context.command, Some(501));
        assert_eq!(error.context.status_type, FrameValueType::Absent);
        assert_eq!(error.context.data_type, FrameValueType::String);
        assert!(!format!("{error:?}").contains("private-synthetic-message"));
    }
    #[test]
    fn original_auth_oracle_accepts_server_header_metadata_gzip_and_encrypted_gzip() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/auth_frames.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let bytes = STANDARD.decode(case["frame"].as_str().unwrap()).unwrap();
            let mut decoder = FrameDecoder::default();
            let frame = decoder.decode_detailed(&bytes).unwrap();
            assert_eq!(frame.cmd, 1);
            assert_eq!(frame.payload, case["expected"]);
            assert!(decoder.authenticated());
        }
    }
    #[test]
    fn parser_reasons_are_fixed_and_gzip_crc_trailing_bytes_and_bombs_fail_closed() {
        use std::io::Write;
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/auth_frames.json"
        ))
        .unwrap();
        let original = STANDARD
            .decode(fixture["cases"][1]["frame"].as_str().unwrap())
            .unwrap();
        let outer: Value = serde_json::from_slice(&original[4..]).unwrap();
        let compressed = STANDARD.decode(outer["data"].as_str().unwrap()).unwrap();
        let mut corrupted = compressed.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        let mut trailing = compressed;
        trailing.extend(b"private-synthetic-suffix");
        let mut bomb = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        bomb.write_all(&vec![b' '; MAX_INFLATED + 1]).unwrap();
        for (bytes, expected) in [
            (corrupted, WireError::Invalid),
            (trailing, WireError::Invalid),
            (bomb.finish().unwrap(), WireError::Oversize),
        ] {
            let mut outer = outer.clone();
            outer["data"] = json!(STANDARD.encode(bytes));
            let mut decoder = FrameDecoder::default();
            let error = decoder.decode_detailed(&frame(1, outer)).unwrap_err();
            assert_eq!(error.reason, FrameReason::Compression);
            assert_eq!(error.error, expected);
            assert!(!decoder.authenticated());
            assert!(!format!("{error:?}").contains("private-synthetic-suffix"));
        }
        let mut decoder = FrameDecoder::default();
        let failure=decoder.decode_detailed(&frame(1,json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":0,"data":{"error_code":0}}))).unwrap_err();
        assert_eq!(failure.reason, FrameReason::PlaintextAuth);
        assert!(!decoder.authenticated());
        assert_eq!(
            decoder
                .decode_detailed(&[1, 1, 0, 0, b'{', b'?'])
                .unwrap_err()
                .reason,
            FrameReason::OuterJson
        );
    }
    #[test]
    fn auth_is_only_established_after_valid_key_and_successful_inner_envelope() {
        let mut decoder = FrameDecoder::default();
        use std::io::Write;
        let mut encoded =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoded
            .write_all(br#"{"error_code":0,"data":{"qCmds":[]}}"#)
            .unwrap();
        let auth = json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":1,"data":STANDARD.encode(encoded.finish().unwrap())});
        assert!(decoder.decode(&frame(1, auth)).is_ok());
        assert!(decoder.authenticated());
        assert!(decoder
            .decode(&frame(
                1,
                json!({"error_code":0,"key":"invalid","encrypt":0,"data":{}})
            ))
            .is_err());
        assert!(!decoder.authenticated());
        assert_eq!(
            FrameDecoder::default()
                .decode(&frame(
                    501,
                    json!({"error_code":0,"encrypt":0,"data":{"error_code":0}})
                ))
                .unwrap_err(),
            WireError::Authentication
        );
    }
    #[test]
    fn rejects_frame_and_inflation_bombs_unknown_encryption_and_auth_failures() {
        assert_eq!(
            authenticated().decode(&vec![0; MAX_FRAME + 1]).unwrap_err(),
            WireError::Oversize
        );
        assert_eq!(
            authenticated()
                .decode(&frame(501, json!({"error_code":0,"encrypt":99,"data":"x"})))
                .unwrap_err(),
            WireError::Unsupported
        );
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&vec![b' '; MAX_INFLATED + 1]).unwrap();
        let compressed = z.finish().unwrap();
        assert_eq!(
            authenticated()
                .decode(&frame(
                    501,
                    json!({"error_code":0,"encrypt":1,"data":STANDARD.encode(compressed)})
                ))
                .unwrap_err(),
            WireError::Oversize
        );
        assert_eq!(
            authenticated()
                .decode(&frame(501, json!({"error_code":102})))
                .unwrap_err(),
            WireError::Authentication
        );
        assert_eq!(
            authenticated()
                .decode(&frame(501, json!({"error_code":-102})))
                .unwrap_err(),
            WireError::Service(-102)
        );
    }
    #[test]
    fn readable_system_types_and_list_silent_flags() {
        let item = |id: &str, from: &str, kind: &str, content: Value| json!({"uidFrom":from,"idTo":if from == "0" {"22"} else {"11"},"msgId":id,"cliMsgId":"5","ts":"7","msgType":kind,"content":content});
        let payload = json!({"error_code":0,"data":{
            "msgs":[
                item("1","22","chat.ecard",json!({"title":"Hôm nay là sinh nhật của An","description":"","type":"1"})),
                item("2","22","chat.gif",json!({"href":"https://example.test/a.gif"})),
                item("3","22","chat.webcontent",json!({"title":"","params":"{\"item\":{\"zinstantdata_id\":\"templateId=1\"}}"})),
                item("4","0","chat.ecard",json!({"title":"Thiệp","type":"1"})),
                item("5","22","chat.ecard",json!({"title":"Hai bạn đã là bạn bè","type":"2"})),
            ],
            "groupMsgs":[item("6","22","chat.ecard",json!({"title":"Sinh nhật","type":"1"}))]
        }});
        let result = incoming_messages("11", &payload).unwrap();
        let summary: Vec<_> = result
            .iter()
            .map(|i| {
                (
                    i.message.text.as_str(),
                    i.list_silent,
                    i.unsupported,
                    i.client_id.is_some(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Hôm nay là sinh nhật của An", true, false, false),
                ("[GIF]", false, false, false),
                (
                    "[Tin nhắn chat.webcontent chưa được hỗ trợ]",
                    true,
                    false,
                    false
                ),
                // Own messages are never silent; a group birthday card is.
                ("Thiệp", false, false, false),
                ("Hai bạn đã là bạn bè", false, false, false),
                ("Sinh nhật", true, false, false),
            ]
        );
    }
    #[test]
    fn routes_peer_self_echo_and_groups_without_numeric_id_collision_and_hides_unsupported_content()
    {
        let mut base = json!({"uidFrom":"22","idTo":"11","msgId":"1","cliMsgId":"2","ts":"7","msgType":"webchat","content":"plain"});
        let payload =
            json!({"error_code":0,"data":{"msgs":[base.clone()],"groupMsgs":[base.clone()]}});
        let messages = incoming_messages("11", &payload).unwrap();
        assert_eq!(messages[0].message.thread_id, "d:22");
        assert_eq!(messages[1].message.thread_id, "g:11");
        base["uidFrom"] = json!("0");
        base["idTo"] = json!("22");
        base["msgId"] = json!("3");
        let mut secure = base.clone();
        secure["msgType"] = json!("chat.e2ee");
        secure["content"] = json!("must-not-render-ciphertext");
        secure["msgId"] = json!("4");
        let mixed = incoming_messages("11", &json!({"error_code":0,"data":{"msgs":[base,secure]}}))
            .unwrap();
        assert_eq!(mixed[0].message.sender, "11");
        assert_eq!(mixed[0].message.thread_id, "d:22");
        assert_eq!(mixed[0].message.text, "plain");
        assert!(mixed[1].unsupported);
        assert!(!mixed[1].message.text.contains("must-not-render"));
    }
    #[test]
    fn source_sticker_metadata_survives_mixed_batches_without_rendering_urls_or_ciphertext() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/stickers/wire.json")).unwrap();
        let base = json!({"uidFrom":"22","idTo":"11","msgId":"101","ts":7,"msgType":fixture["type"],"content":fixture["content"]});
        let mut bad = base.clone();
        bad["msgId"] = json!("102");
        bad["content"]["id"] = json!("secret-ciphertext");
        let mut malicious = base.clone();
        malicious["msgId"] = json!("103");
        malicious["content"]["id"] = json!("https://private.invalid/token");
        let mut secure = base.clone();
        secure["msgId"] = json!("104");
        secure["e2eeExt"] = json!({"iid":3});
        let mut text = base.clone();
        text["msgId"] = json!("105");
        text["msgType"] = json!("webchat");
        text["content"] = json!("still received");
        let result=incoming_messages("11",&json!({"error_code":0,"data":{"msgs":[base.clone(),bad,malicious,secure,text],"groupMsgs":[base]}})).unwrap();
        assert_eq!(result.len(), 6);
        assert_eq!(
            result[0].message.sticker.as_ref().unwrap().sticker_id,
            20695
        );
        assert_eq!(
            result[0].message.text,
            crate::domain::LEGACY_STICKER_PLACEHOLDER
        );
        assert!(!result[0].unsupported);
        for item in &result[1..4] {
            assert!(item.unsupported);
            assert!(item.message.sticker.is_none());
            assert!(!item.message.text.contains("secret"));
            assert!(!item.message.text.contains("https"));
        }
        assert_eq!(result[4].message.text, "still received");
        assert_eq!(result[5].message.thread_id, "g:11");
        assert!(result[5].message.sticker.is_some());
        for id in [
            json!("01"),
            json!(-1),
            json!(9_007_199_254_740_992u64),
            json!(1.5),
            json!({}),
        ] {
            let mut bad = result_fixture(&fixture);
            bad["content"]["id"] = id;
            assert!(sticker_descriptor(&bad).is_none());
        }
        let mut strings = result_fixture(&fixture);
        strings["content"] = json!({"id":"20695","catId":"10047","type":"3"});
        assert!(sticker_descriptor(&strings).is_some());
        fn result_fixture(fixture: &Value) -> Value {
            json!({"msgType":fixture["type"],"content":fixture["content"]})
        }
    }
    #[test]
    fn direct_eligibility_matches_original_terminal_flags_without_accepting_incomplete_snapshots() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/eligibility.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap().iter().take(8) {
            let envelope = json!({"error_code":0,"data":{"reqId":"r1","data":case["pages"][0]}});
            assert_eq!(
                legacy_direct_allowed_detailed(&envelope, "r1", "22").is_ok(),
                case["accepted"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
        let complete = json!({"error_code":0,"data":{"reqId":"r1","data":{"err":0,"threads":[],"waitUids":[],"uids":[],"pendingUids":[]}}});
        for (name, field) in [
            ("threads", EligibilityField::Threads),
            ("waitUids", EligibilityField::Waiting),
            ("uids", EligibilityField::Uids),
            ("pendingUids", EligibilityField::Pending),
        ] {
            for value in [None, Some(Value::Null), Some(json!({})), Some(json!(false))] {
                let mut bad = complete.clone();
                if let Some(value) = value {
                    bad["data"]["data"][name] = value;
                } else {
                    bad["data"]["data"].as_object_mut().unwrap().remove(name);
                }
                let failure = legacy_direct_allowed_detailed(&bad, "r1", "22").unwrap_err();
                assert_eq!(failure.reason, EligibilityReason::Schema);
                assert_eq!(failure.field, Some(field));
            }
        }
        for flag in [
            json!("false"),
            json!("0"),
            json!({}),
            json!([]),
            json!(2),
            json!(-1),
        ] {
            let mut bad = complete.clone();
            bad["data"]["data"]["hasMore"] = flag;
            assert_eq!(
                legacy_direct_allowed_detailed(&bad, "r1", "22")
                    .unwrap_err()
                    .field,
                Some(EligibilityField::More)
            );
        }
        for flag in [json!(true), json!(1)] {
            let mut next = complete.clone();
            next["data"]["data"]["hasMore"] = flag;
            next["data"]["data"]["lastId"] = json!(90);
            assert_eq!(
                legacy_direct_allowed_detailed(&next, "r1", "22").unwrap(),
                (true, Some("90".into()))
            );
            next["data"]["data"]["lastId"] = json!("40");
            assert_eq!(
                legacy_direct_allowed_detailed(&next, "r1", "22").unwrap(),
                (true, Some("40".into())),
                "cursor values are opaque; source forwards them without assuming ascending IDs"
            );
        }
        let unknown = json!({"error_code":0,"data":{"reqId":"r1","data":{}}});
        assert!(legacy_direct_allowed_detailed(&unknown, "r1", "22").is_err());
        assert_eq!(
            legacy_direct_allowed_detailed(&complete, "other", "22")
                .unwrap_err()
                .reason,
            EligibilityReason::Correlation
        );
    }
    #[test]
    fn a_missing_zero_err_is_success_but_any_truthy_err_still_refuses_the_send() {
        let complete = |err: Option<Value>| {
            let mut data = json!({"threads":[],"waitUids":[],"uids":[],"pendingUids":[]});
            if let Some(err) = err {
                data["err"] = err;
            }
            json!({"error_code":0,"data":{"reqId":"r1","data":data}})
        };
        // The signal service omits `err` when it is zero; the original treats that as success.
        for ok in [
            None,
            Some(json!(null)),
            Some(json!(0)),
            Some(json!(false)),
            Some(json!(0.0)),
        ] {
            assert_eq!(
                legacy_direct_allowed_detailed(&complete(ok.clone()), "r1", "22")
                    .unwrap_or_else(|f| panic!("{ok:?} must be accepted: {f:?}")),
                (true, None)
            );
        }
        for bad in [
            json!(1),
            json!(-1),
            json!(true),
            json!("0"),
            json!("err"),
            json!({}),
            json!([]),
        ] {
            let failure = legacy_direct_allowed_detailed(&complete(Some(bad.clone())), "r1", "22")
                .expect_err(&format!("{bad} must be refused"));
            assert_eq!(failure.reason, EligibilityReason::Schema, "{bad}");
            assert_eq!(failure.field, Some(EligibilityField::Status), "{bad}");
        }
        // Dropping `err` never relaxes the rest: arrays, correlation and encrypted peers.
        let mut missing_array = complete(None);
        missing_array["data"]["data"]
            .as_object_mut()
            .unwrap()
            .remove("uids");
        assert_eq!(
            legacy_direct_allowed_detailed(&missing_array, "r1", "22")
                .unwrap_err()
                .field,
            Some(EligibilityField::Uids)
        );
        assert_eq!(
            legacy_direct_allowed_detailed(&complete(None), "other", "22")
                .unwrap_err()
                .reason,
            EligibilityReason::Correlation
        );
        let mut encrypted = complete(None);
        encrypted["data"]["data"]["uids"] = json!(["22"]);
        assert_eq!(
            legacy_direct_allowed_detailed(&encrypted, "r1", "22").unwrap(),
            (false, None),
            "a peer in the encrypted list is still refused when err is absent"
        );
        // A nonzero envelope code is still a refusal.
        let mut refused = complete(None);
        refused["error_code"] = json!(7);
        assert_eq!(
            legacy_direct_allowed_detailed(&refused, "r1", "22")
                .unwrap_err()
                .error,
            WireError::Service(7)
        );
    }
    #[test]
    fn group_eligibility_distinguishes_confirmed_encryption_from_unknown_metadata() {
        for flag in [json!(true), json!(1)] {
            assert_eq!(
                legacy_group_allowed_detailed(
                    &json!({"error_code":0,"data":{"groupId":"22","e2ee":flag}}),
                    "22"
                )
                .unwrap_err()
                .reason,
                EligibilityReason::EncryptedTarget
            );
        }
        for metadata in [
            json!({"groupId":"22"}),
            json!({"groupId":"22","e2ee":null}),
            json!({"groupId":"22","e2ee":"false"}),
        ] {
            let failure =
                legacy_group_allowed_detailed(&json!({"error_code":0,"data":metadata}), "22")
                    .unwrap_err();
            assert_eq!(failure.reason, EligibilityReason::Schema);
            assert_eq!(failure.field, Some(EligibilityField::Encryption));
        }
        assert_eq!(
            legacy_group_allowed_detailed(
                &json!({"error_code":0,"data":{"groupId":"33","e2ee":false}}),
                "22"
            )
            .unwrap_err()
            .reason,
            EligibilityReason::Correlation
        );
    }
    #[test]
    fn capability_requires_correlated_complete_explicit_schema_and_rejects_encrypted_members() {
        let complete = json!({"error_code":0,"data":{"reqId":"r1","data":{"err":0,"hasMore":false,"uids":[],"waitUids":[],"threads":[],"pendingUids":[]}}});
        assert_eq!(
            legacy_direct_allowed(&complete, "r1", "22").unwrap(),
            (true, None)
        );
        assert!(legacy_direct_allowed(&complete, "other", "22").is_err());
        let mut partial = complete.clone();
        partial["data"]["data"]
            .as_object_mut()
            .unwrap()
            .remove("threads");
        assert!(legacy_direct_allowed(&partial, "r1", "22").is_err());
        partial = complete.clone();
        partial["data"]["data"]
            .as_object_mut()
            .unwrap()
            .remove("hasMore");
        assert_eq!(
            legacy_direct_allowed(&partial, "r1", "22").unwrap(),
            (true, None)
        );
        partial = complete.clone();
        partial["data"]["data"]["hasMore"] = json!(true);
        assert!(legacy_direct_allowed(&partial, "r1", "22").is_err());
        for name in ["uids", "waitUids", "threads", "pendingUids"] {
            let mut encrypted = complete.clone();
            encrypted["data"]["data"][name] =
                json!([if name == "threads" || name == "pendingUids" {
                    json!({"uid":"22"})
                } else {
                    json!("22")
                }]);
            assert_eq!(
                legacy_direct_allowed(&encrypted, "r1", "22").unwrap(),
                (false, None)
            );
        }
        assert!(legacy_group_allowed(
            &json!({"error_code":0,"data":{"groupId":"22","e2ee":false}}),
            "22"
        )
        .is_ok());
        for flag in [json!(true), json!(1), json!(null), json!("false")] {
            assert!(legacy_group_allowed(
                &json!({"error_code":0,"data":{"groupId":"22","e2ee":flag}}),
                "22"
            )
            .is_err());
        }
    }
    #[test]
    fn personal_and_group_requests_preserve_text_and_explicit_target_kinds() {
        for kind in [ThreadKind::Direct, ThreadKind::Group] {
            let target = Target::new(kind, "22").unwrap();
            assert_eq!(Target::from_thread(&target.thread_id()).unwrap(), target);
            let params =
                text_parameters(&target, "xin & + = chào", 1760000000000, "device").unwrap();
            assert_eq!(params["message"], "xin & + = chào");
            let params = text_parameters(
                &target,
                "\u{feff} \tchào  bạn\n tốt \r\n",
                1760000000000,
                "device",
            )
            .unwrap();
            assert_eq!(params["message"], "chào  bạn\n tốt");
            assert_eq!(params["clientId"], 1760000000000_u64);
            assert_eq!(
                params[if kind == ThreadKind::Direct {
                    "toid"
                } else {
                    "grid"
                }],
                "22"
            );
        }
        assert!(Target::new(ThreadKind::Direct, "0").is_err());
        assert!(Target::new(ThreadKind::Group, "g22").is_err());
        assert!(Target::new(ThreadKind::Direct, "22\n").is_err());
        assert!(send_url(
            &Url::parse("https://chat.zalo.me").unwrap(),
            ThreadKind::Direct,
            ClientType::Legacy30
        )
        .unwrap()
        .as_str()
        .contains("/api/message/sms?zpw_ver=681&zpw_type=30"));
        for bad in [
            "https://zalo.me.example.org",
            "https://user@chat.zalo.me",
            "http://chat.zalo.me",
            "https://chat.zalo.me:444",
            "https://chat.zalo.me/?token=secret",
            "https://chat.zalo.me/socket",
        ] {
            assert!(send_url(
                &Url::parse(bad).unwrap(),
                ThreadKind::Direct,
                ClientType::Legacy30
            )
            .is_err());
        }
        assert_eq!(
            send_ack(
                &json!({"error_code":0,"data":{"msgId":"9007199254740993","ts":"1760000000001"}})
            )
            .unwrap()
            .server_id,
            "9007199254740993"
        );
        assert!(send_ack(&json!({"error_code":0,"data":{}})).is_err());
    }

    #[test]
    fn socket_preserves_advertised_path_without_relaxing_endpoint_policy() {
        let advertised = Url::parse("wss://ws1.chat.zalo.me/socket/v1%2Fmessages").unwrap();
        let url = socket_url(&advertised, 1760000000000, ClientType::Legacy30).unwrap();
        assert_eq!(url.path(), advertised.path());
        assert_eq!(
            url.as_str(),
            "wss://ws1.chat.zalo.me/socket/v1%2Fmessages?zpw_ver=681&zpw_type=30&t=1760000000000"
        );
        assert!(socket_url(
            &Url::parse("wss://chat.zalo.me/").unwrap(),
            1,
            ClientType::Legacy30
        )
        .is_ok());
        for bad in [
            "ws://chat.zalo.me/socket",
            "https://chat.zalo.me/socket",
            "wss://chat.zalo.me.example.org/socket",
            "wss://127.0.0.1/socket",
            "wss://user@chat.zalo.me/socket",
            "wss://user:password@chat.zalo.me/socket",
            "wss://chat.zalo.me:444/socket",
            "wss://chat.zalo.me/socket?token=secret",
            "wss://chat.zalo.me/socket?",
            "wss://chat.zalo.me/socket#fragment",
        ] {
            assert!(socket_url(&Url::parse(bad).unwrap(), 1, ClientType::Legacy30).is_err());
        }
        for kind in [ThreadKind::Direct, ThreadKind::Group] {
            assert!(send_url(
                &Url::parse("https://chat.zalo.me/socket").unwrap(),
                kind,
                ClientType::Legacy30
            )
            .is_err());
        }
    }
}
