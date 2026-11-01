//! Original bootstrap wire contracts recovered from the repository's local JS bundles.
//! These primitives neither authenticate an account nor commit credentials to a profile.
use super::client::ClientType;
use super::qr::{
    encrypt_parameters, sign_parameters, RequestSpec, ResponseCipher, MAX_RESPONSE_BYTES,
};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

const ZERO_IV: [u8; 16] = [0; 16];
const AUTH_ORIGIN: &str = "https://wpa.zaloapp.com";
const MAX_ENDPOINTS_PER_SERVICE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    Http(u16),
    Malformed,
    UnsafeEndpoint,
    UnsupportedSchema,
    AuthenticationRejected(i64),
    ServiceRejected(i64),
    EncryptionRejected(i64),
}
impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(status) => write!(
                f,
                "Bootstrap HTTP failure ({status}); credentials were preserved"
            ),
            Self::AuthenticationRejected(code) => {
                write!(f, "Server rejected authentication ({code})")
            }
            Self::ServiceRejected(code) => write!(
                f,
                "Bootstrap service rejected request ({code}); credentials were preserved"
            ),
            Self::EncryptionRejected(code) => write!(
                f,
                "Encrypted bootstrap rejected ({code}); plaintext downgrade is disabled"
            ),
            Self::Malformed => {
                f.write_str("Malformed bootstrap response; credentials were preserved")
            }
            Self::UnsafeEndpoint => f.write_str("Bootstrap returned an untrusted endpoint"),
            Self::UnsupportedSchema => f.write_str("Unsupported bootstrap configuration schema"),
        }
    }
}
impl std::error::Error for BootstrapError {}
impl BootstrapError {
    fn from_code(code: i64) -> Self {
        match code {
            600 | 102 => Self::AuthenticationRejected(code),
            18060 | 19000 => Self::EncryptionRejected(code),
            _ => Self::ServiceRejected(code),
        }
    }
    pub fn invalidates_authentication(&self) -> bool {
        matches!(self, Self::AuthenticationRejected(_))
    }
}

/// Sanitized post-confirmation diagnostics: only fixed stages/categories and
/// explicit numeric service status are retained, never response data or URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStage {
    Http,
    OuterEnvelope,
    ResponseDecryption,
    InnerEnvelope,
    Identity,
    StorageKey,
    SessionKey,
    ServiceConfiguration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapFailure {
    pub stage: BootstrapStage,
    pub category: BootstrapError,
}
impl std::fmt::Display for BootstrapFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stage = match self.stage {
            BootstrapStage::Http => "http",
            BootstrapStage::OuterEnvelope => "outer-envelope",
            BootstrapStage::ResponseDecryption => "decrypt",
            BootstrapStage::InnerEnvelope => "inner-envelope",
            BootstrapStage::Identity => "identity",
            BootstrapStage::StorageKey => "storage-key",
            BootstrapStage::SessionKey => "session-key",
            BootstrapStage::ServiceConfiguration => "service-config",
        };
        let (category, code) = match self.category {
            BootstrapError::Http(code) => ("http-status", Some(i64::from(code))),
            BootstrapError::Malformed => ("malformed", None),
            BootstrapError::UnsafeEndpoint => ("unsafe-endpoint", None),
            BootstrapError::UnsupportedSchema => ("unsupported-schema", None),
            BootstrapError::AuthenticationRejected(code) => ("authentication-rejected", Some(code)),
            BootstrapError::ServiceRejected(code) => ("service-rejected", Some(code)),
            BootstrapError::EncryptionRejected(code) => ("encryption-rejected", Some(code)),
        };
        write!(f, "bootstrap/{stage}/{category}")?;
        if let Some(code) = code {
            write!(f, " code={code}")?;
        }
        Ok(())
    }
}
impl std::error::Error for BootstrapFailure {}
fn at_stage(stage: BootstrapStage) -> impl FnOnce(BootstrapError) -> BootstrapFailure {
    move |category| BootstrapFailure { stage, category }
}

pub struct ServerInfoInput<'a> {
    pub client_type: ClientType,
    pub imei: &'a str,
    pub computer_name: &'a str,
    pub support_maintenance: Option<i64>,
    pub bucket_id: Option<i64>,
}
/// Plaintext parameters are the observed getServerInfo contract, not an encryption fallback.
pub fn server_info_request(input: &ServerInfoInput<'_>) -> Result<RequestSpec> {
    validate_device(input.imei, input.computer_name)?;
    let mut params = BTreeMap::from([
        ("imei".into(), input.imei.to_owned()),
        ("computer_name".into(), input.computer_name.to_owned()),
        ("type".into(), input.client_type.api_type().into()),
        ("client_version".into(), "681".into()),
        (
            "sp_mtn".into(),
            input.support_maintenance.unwrap_or(-1).to_string(),
        ),
        ("bkt".into(), input.bucket_id.unwrap_or(-1).to_string()),
    ]);
    params.insert("signkey".into(), sign_parameters("getserverinfo", &params));
    request("/api/login/getServerInfo", &params)
}

pub struct LoginInfoInput<'a> {
    pub client_type: ClientType,
    pub imei: &'a str,
    pub computer_name: &'a str,
    pub language: &'a str,
    pub timestamp_ms: u64,
    pub nonce_hex: &'a str,
}
/// Identity submission extras are intentionally unsupported until their trust proofs are verified.
pub fn login_info_request(input: &LoginInfoInput<'_>) -> Result<(RequestSpec, ResponseCipher)> {
    validate_device(input.imei, input.computer_name)?;
    if !matches!(input.language, "vi" | "en") || input.timestamp_ms == 0 {
        bail!("Invalid login locale or timestamp");
    }
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        imei: &'a str,
        computer_name: &'a str,
        language: &'a str,
        ts: u64,
    }
    let payload = serde_json::to_vec(&Payload {
        imei: input.imei,
        computer_name: input.computer_name,
        language: input.language,
        ts: input.timestamp_ms,
    })?;
    let (mut params, key) = encrypt_parameters(
        input.client_type,
        input.imei,
        input.timestamp_ms,
        input.nonce_hex,
        &payload,
    )?;
    params.insert("type".into(), input.client_type.api_type().into());
    params.insert("client_version".into(), "681".into());
    params.insert("signkey".into(), sign_parameters("getlogininfo", &params));
    Ok((request("/api/login/getLoginInfo", &params)?, key))
}
fn validate_device(imei: &str, name: &str) -> Result<()> {
    if imei.is_empty()
        || imei.len() > 128
        || name.is_empty()
        || name.len() > 128
        || imei.chars().chain(name.chars()).any(char::is_control)
    {
        bail!("Invalid device identifier or computer name");
    }
    Ok(())
}
fn request(path: &str, params: &BTreeMap<String, String>) -> Result<RequestSpec> {
    let mut url = Url::parse(&format!("{AUTH_ORIGIN}{path}"))?;
    url.query_pairs_mut().extend_pairs(params);
    Ok(RequestSpec {
        url,
        timeout_ms: 10_000,
        max_response_bytes: MAX_RESPONSE_BYTES,
        include_cookies: true,
        follow_redirects: false,
    })
}

#[derive(Deserialize)]
struct Envelope {
    error_code: i64,
    #[serde(default)]
    data: Value,
}
fn envelope(body: &[u8]) -> std::result::Result<Value, BootstrapError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(BootstrapError::Malformed);
    }
    let reply: Envelope = serde_json::from_slice(body).map_err(|_| BootstrapError::Malformed)?;
    if reply.error_code != 0 {
        return Err(BootstrapError::from_code(reply.error_code));
    }
    Ok(reply.data)
}
pub fn decode_server_info(
    status: u16,
    body: &[u8],
) -> std::result::Result<PreloginConfig, BootstrapError> {
    if status != 200 {
        return Err(BootstrapError::Http(status));
    }
    PreloginConfig::parse(&envelope(body)?)
}
/// ParamCipher decrypts the full inner envelope, not just its `data` field.
pub fn decode_login_reply(
    status: u16,
    body: &[u8],
    cipher: &ResponseCipher,
) -> std::result::Result<LoginInfo, BootstrapError> {
    decode_login_reply_detailed(status, body, cipher).map_err(|failure| failure.category)
}
pub fn decode_login_reply_detailed(
    status: u16,
    body: &[u8],
    cipher: &ResponseCipher,
) -> std::result::Result<LoginInfo, BootstrapFailure> {
    if status != 200 {
        return Err(BootstrapFailure {
            stage: BootstrapStage::Http,
            category: BootstrapError::Http(status),
        });
    }
    let outer = envelope(body).map_err(at_stage(BootstrapStage::OuterEnvelope))?;
    let encrypted = outer.as_str().ok_or(BootstrapFailure {
        stage: BootstrapStage::OuterEnvelope,
        category: BootstrapError::Malformed,
    })?;
    let plaintext = cipher.decrypt(encrypted).map_err(|_| BootstrapFailure {
        stage: BootstrapStage::ResponseDecryption,
        category: BootstrapError::Malformed,
    })?;
    let data = envelope(&plaintext).map_err(at_stage(BootstrapStage::InnerEnvelope))?;
    LoginInfo::parse_detailed(&data)
}

/// Pre-login settings cannot authorize message/service endpoints or session cookies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrMode {
    PhoneApproved,
    TrustedDevice,
}
#[derive(Debug)]
pub struct PreloginConfig {
    pub qr_mode: Option<QrMode>,
    pub trusted_qr: bool,
    pub server_time_seconds: Option<i64>,
    pub parameter_encryption: Option<bool>,
}
impl PreloginConfig {
    fn parse(value: &Value) -> std::result::Result<Self, BootstrapError> {
        let object = value.as_object().ok_or(BootstrapError::Malformed)?;
        // Only bounded scalar settings enter the auth parser. Unused service maps
        // may contain routes/schemes that are not supported by the live messenger.
        let mut selected = serde_json::Map::new();
        if let Some(time) = object.get("server_time") {
            selected.insert("server_time".into(), time.clone());
        }
        for spelling in ["setttings", "settings"] {
            let mut auth = serde_json::json!({});
            for path in [
                "encryption/enable",
                "trusted_device/self_g",
                "trusted_device/qr_format_trust_device",
            ] {
                if let Some(flag) = value.pointer(&format!("/{spelling}/authen/{path}")) {
                    if !matches!(flag, Value::Bool(_) | Value::Number(_) | Value::Null) {
                        return Err(BootstrapError::Malformed);
                    }
                    let (section, key) = path.split_once('/').unwrap();
                    if auth.get(section).is_none() {
                        auth[section] = serde_json::json!({});
                    }
                    auth[section][key] = flag.clone();
                }
            }
            selected.insert(spelling.into(), serde_json::json!({"authen":auth}));
        }
        let parsed = ServerConfig::parse(&Value::Object(selected))?;
        for name in ["self_g", "qr_format_trust_device"] {
            let before = value.pointer(&format!("/setttings/authen/trusted_device/{name}"));
            let after = value.pointer(&format!("/settings/authen/trusted_device/{name}"));
            if before.is_some() && after.is_some() && before != after {
                return Err(BootstrapError::Malformed);
            }
        }
        let flags = value
            .pointer("/setttings/authen/trusted_device")
            .or_else(|| value.pointer("/settings/authen/trusted_device"));
        let qr_mode = match flags.map(|flags| {
            (
                flags.get("self_g").and_then(Value::as_i64),
                flags.get("qr_format_trust_device").and_then(Value::as_i64),
            )
        }) {
            Some((Some(0), Some(0))) => Some(QrMode::PhoneApproved),
            Some((Some(1), Some(1))) => Some(QrMode::TrustedDevice),
            _ => None,
        };
        Ok(Self {
            qr_mode,
            trusted_qr: parsed.trusted_qr,
            server_time_seconds: parsed.server_time_seconds,
            parameter_encryption: parsed.parameter_encryption,
        })
    }
}
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub trusted_qr: bool,
    pub server_time_seconds: Option<i64>,
    pub parameter_encryption: Option<bool>,
    pub services: BTreeMap<String, Vec<Url>>,
    pub chat_poll: Vec<Url>,
    pub control_poll: Vec<Url>,
    pub websocket: Vec<Url>,
}
impl ServerConfig {
    pub fn parse(value: &Value) -> std::result::Result<Self, BootstrapError> {
        let object = value.as_object().ok_or(BootstrapError::Malformed)?;
        let server_time_seconds = object
            .get("server_time")
            .map(|v| {
                v.as_i64()
                    .filter(|n| *n > 0)
                    .ok_or(BootstrapError::Malformed)
            })
            .transpose()?;
        // `setttings` is the actual pre-login bundle spelling. Post-login uses `settings`.
        let before = value.pointer("/setttings/authen/encryption/enable");
        let after = value.pointer("/settings/authen/encryption/enable");
        if before.is_some() && after.is_some() && before != after {
            return Err(BootstrapError::Malformed);
        }
        let parameter_encryption = before
            .or(after)
            .map(|v| match v {
                Value::Bool(flag) => Ok(*flag),
                Value::Number(number) if number.as_u64() == Some(0) => Ok(false),
                Value::Number(number) if number.as_u64() == Some(1) => Ok(true),
                _ => Err(BootstrapError::Malformed),
            })
            .transpose()?;
        let mut services = BTreeMap::new();
        let modern = object
            .get("zpw_service_map_v3")
            .map(|map| map.as_object().ok_or(BootstrapError::UnsupportedSchema))
            .transpose()?;
        let legacy = object.get("zpw_service_map_new").and_then(Value::as_object);
        // The bundle filters supported domain types before interpreting values.
        // Required messaging capabilities remain strict. Unused services must
        // neither reject authentication nor authorize credential destinations.
        for name in ["chat", "group"] {
            let urls = modern.and_then(|map| map.get(name));
            let mut endpoints = endpoint_list(urls, false)?;
            if endpoints.is_empty() {
                if let Some(fallback) = legacy.and_then(|map| map.get(name)) {
                    endpoints = endpoint_list(Some(&Value::Array(vec![fallback.clone()])), false)?;
                }
            }
            if !endpoints.is_empty() {
                services.insert(name.into(), endpoints);
            }
        }
        // Directory capabilities are optional. The bundle's merge ignores
        // unsupported modern values and falls back to the legacy string. Each
        // candidate is validated independently; discarded routes grant no cookies.
        for name in ["profile", "conversation"] {
            let mut endpoints =
                endpoint_list(modern.and_then(|map| map.get(name)), false).unwrap_or_default();
            if endpoints.is_empty() {
                if let Some(fallback) = legacy.and_then(|map| map.get(name)) {
                    endpoints = endpoint_list(Some(&Value::Array(vec![fallback.clone()])), false)
                        .unwrap_or_default();
                }
            }
            if !endpoints.is_empty() {
                services.insert(name.into(), endpoints);
            }
        }
        let trusted = value
            .pointer("/setttings/authen/trusted_device")
            .or_else(|| value.pointer("/settings/authen/trusted_device"));
        let trusted_qr = trusted.is_some_and(|v| {
            v.get("self_g").and_then(Value::as_i64) == Some(1)
                && v.get("qr_format_trust_device").and_then(Value::as_i64) == Some(1)
        });
        Ok(Self {
            trusted_qr,
            server_time_seconds,
            parameter_encryption,
            services,
            // The native runtime uses WebSockets, not legacy chat/control LP.
            // Unconsumed LP routes cannot reject login or authorize cookies.
            chat_poll: Vec::new(),
            control_poll: Vec::new(),
            websocket: endpoint_list(object.get("zpw_ws"), true)?,
        })
    }
}
fn endpoint_list(
    value: Option<&Value>,
    websocket: bool,
) -> std::result::Result<Vec<Url>, BootstrapError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .filter(|v| v.len() <= MAX_ENDPOINTS_PER_SERVICE)
        .ok_or(BootstrapError::UnsupportedSchema)?;
    list.iter()
        .map(|v| {
            let value = v
                .as_str()
                .filter(|s| s.len() <= 2048)
                .ok_or(BootstrapError::Malformed)?;
            let url = Url::parse(value).map_err(|_| BootstrapError::UnsafeEndpoint)?;
            let host = url.host_str().ok_or(BootstrapError::UnsafeEndpoint)?;
            if url.scheme() != if websocket { "wss" } else { "https" }
                || !trusted_host(host)
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port_or_known_default() != Some(443)
                || (!websocket && url.path() != "/")
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(BootstrapError::UnsafeEndpoint);
            }
            Ok(url)
        })
        .collect()
}
fn trusted_host(host: &str) -> bool {
    host == "zalo.me"
        || host.ends_with(".zalo.me")
        || host == "zaloapp.com"
        || host.ends_with(".zaloapp.com")
}

/// Secret-bearing bootstrap result. `dkey` is legacy UIN/storage material, not a verified user ID.
/// No Debug/Serialize implementation; persisting or activating this result is a separate trust gate.
pub struct LoginInfo {
    pub account_id: String,
    pub config: ServerConfig,
    storage_key: String,
    pub session_cipher: SessionCipher,
}
impl LoginInfo {
    pub fn parse(data: &Value) -> std::result::Result<Self, BootstrapError> {
        Self::parse_detailed(data).map_err(|failure| failure.category)
    }
    fn parse_detailed(data: &Value) -> std::result::Result<Self, BootstrapFailure> {
        let account_id =
            secret_field(data, "uid", 64).map_err(at_stage(BootstrapStage::Identity))?;
        if !account_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BootstrapFailure {
                stage: BootstrapStage::Identity,
                category: BootstrapError::Malformed,
            });
        }
        let storage_key =
            secret_field(data, "dkey", 1024).map_err(at_stage(BootstrapStage::StorageKey))?;
        let key =
            secret_field(data, "zpw_enk", 1024).map_err(at_stage(BootstrapStage::SessionKey))?;
        let session_cipher =
            SessionCipher::from_base64(&key).map_err(at_stage(BootstrapStage::SessionKey))?;
        let config =
            ServerConfig::parse(data).map_err(at_stage(BootstrapStage::ServiceConfiguration))?;
        Ok(Self {
            account_id,
            storage_key,
            session_cipher,
            config,
        })
    }
    pub fn has_storage_key(&self) -> bool {
        !self.storage_key.is_empty()
    }
}

pub struct SessionCipher {
    pub(super) key: Vec<u8>,
}
impl SessionCipher {
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        if plaintext.len() > MAX_RESPONSE_BYTES {
            bail!("session payload exceeds limit");
        }
        let encrypted = match self.key.len() {
            16 => cbc::Encryptor::<aes::Aes128>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| anyhow::anyhow!("invalid session cipher"))?
                .encrypt_padded_vec_mut::<Pkcs7>(plaintext),
            24 => cbc::Encryptor::<aes::Aes192>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| anyhow::anyhow!("invalid session cipher"))?
                .encrypt_padded_vec_mut::<Pkcs7>(plaintext),
            _ => cbc::Encryptor::<aes::Aes256>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| anyhow::anyhow!("invalid session cipher"))?
                .encrypt_padded_vec_mut::<Pkcs7>(plaintext),
        };
        Ok(STANDARD.encode(encrypted))
    }
    pub fn from_base64(key: &str) -> std::result::Result<Self, BootstrapError> {
        if key.len() > 1024 {
            return Err(BootstrapError::Malformed);
        }
        let key = STANDARD
            .decode(key)
            .map_err(|_| BootstrapError::Malformed)?;
        if !matches!(key.len(), 16 | 24 | 32) {
            return Err(BootstrapError::Malformed);
        }
        Ok(Self { key })
    }
    pub fn decrypt(&self, encoded: &str) -> std::result::Result<Vec<u8>, BootstrapError> {
        self.decrypt_bounded(encoded, MAX_RESPONSE_BYTES)
    }
    pub(super) fn decrypt_bounded(
        &self,
        encoded: &str,
        limit: usize,
    ) -> std::result::Result<Vec<u8>, BootstrapError> {
        if limit > 16 * 1024 * 1024 || encoded.len() > limit {
            return Err(BootstrapError::Malformed);
        }
        let encoded = super::qr::percent_decode(encoded).map_err(|_| BootstrapError::Malformed)?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| BootstrapError::Malformed)?;
        let result = match self.key.len() {
            16 => cbc::Decryptor::<aes::Aes128>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| BootstrapError::Malformed)?
                .decrypt_padded_vec_mut::<Pkcs7>(&bytes),
            24 => cbc::Decryptor::<aes::Aes192>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| BootstrapError::Malformed)?
                .decrypt_padded_vec_mut::<Pkcs7>(&bytes),
            _ => cbc::Decryptor::<aes::Aes256>::new_from_slices(&self.key, &ZERO_IV)
                .map_err(|_| BootstrapError::Malformed)?
                .decrypt_padded_vec_mut::<Pkcs7>(&bytes),
        };
        result.map_err(|_| BootstrapError::Malformed)
    }
}
fn secret_field(
    data: &Value,
    name: &str,
    limit: usize,
) -> std::result::Result<String, BootstrapError> {
    data.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(BootstrapError::Malformed)
}

/// A staging plan, never an installed cookie jar or accepted session.
pub struct PendingCookies {
    pub(super) account_id: String,
    pub(super) storage_key: String,
    pub(super) session_cookie: String,
    pub(super) refresh_token: Option<String>,
}
impl PendingCookies {
    pub fn from_confirmation(data: &Value) -> std::result::Result<Self, BootstrapError> {
        let account_id = secret_field(data, "uid", 64)?;
        if !account_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(BootstrapError::Malformed);
        }
        let storage_key = secret_field(data, "dkey", 1024)?;
        let session_cookie = secret_field(data, "zpw_sek", 8192)?;
        if !session_cookie
            .bytes()
            .all(|b| matches!(b,0x21|0x23..=0x2b|0x2d..=0x3a|0x3c..=0x5b|0x5d..=0x7e))
        {
            return Err(BootstrapError::Malformed);
        }
        let refresh_token = data
            .get("zpw_kt")
            .map(|_| secret_field(data, "zpw_kt", 8192))
            .transpose()?;
        Ok(Self {
            account_id,
            storage_key,
            session_cookie,
            refresh_token,
        })
    }
    pub fn claimed_account(&self) -> &str {
        &self.account_id
    }
    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token.is_some()
    }
    pub fn has_storage_key(&self) -> bool {
        !self.storage_key.is_empty()
    }
    /// Exactly the three observed Electron scopes, all secure and HttpOnly.
    pub fn cookie_domains(&self) -> [&'static str; 3] {
        ["zaloapp.com", "zalo.me", "chat.zalo.me"]
    }
    pub fn request_cookie(&self, url: &Url) -> Option<String> {
        if url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some_and(trusted_host)
        {
            Some(format!("zpw_sek={}", self.session_cookie))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn login_input() -> LoginInfoInput<'static> {
        LoginInfoInput {
            client_type: ClientType::Legacy30,
            imei: "fixture-imei",
            computer_name: "fixture-pc",
            language: "vi",
            timestamp_ms: 1_700_000_000_123,
            nonce_hex: "abcdef123456",
        }
    }
    #[test]
    fn qr_mode_never_downgrades_missing_mixed_or_conflicting_flags() {
        for flags in [
            serde_json::json!({}),
            serde_json::json!({"self_g":1,"qr_format_trust_device":0}),
            serde_json::json!({"self_g":0,"qr_format_trust_device":1}),
            serde_json::json!({"self_g":0,"qr_format_trust_device":2}),
            serde_json::json!({"self_g":"0","qr_format_trust_device":0}),
        ] {
            let value = serde_json::json!({"setttings":{"authen":{"trusted_device":flags}}});
            assert!(PreloginConfig::parse(&value)
                .map(|v| v.qr_mode.is_none())
                .unwrap_or(true));
        }
        let conflict = serde_json::json!({"setttings":{"authen":{"trusted_device":{"self_g":0,"qr_format_trust_device":0}}},"settings":{"authen":{"trusted_device":{"self_g":1,"qr_format_trust_device":1}}}});
        assert!(PreloginConfig::parse(&conflict).is_err());
        assert_eq!(PreloginConfig::parse(&serde_json::json!({"setttings":{"authen":{"trusted_device":{"self_g":0,"qr_format_trust_device":0}}}})).unwrap().qr_mode,Some(QrMode::PhoneApproved));
    }
    #[test]
    fn prelogin_ignores_unused_routes_without_granting_them_to_a_session() {
        let value = serde_json::json!({"server_time":1700000000,"setttings":{"authen":{"encryption":{"enable":1},"trusted_device":{"self_g":1,"qr_format_trust_device":1}}},"zpw_service_map_v3":{"unused":["http://unused.invalid/path"]},"zpw_ws":["wss://ws1.zalo.me/socket"]});
        let parsed = decode_server_info(
            200,
            &serde_json::to_vec(&serde_json::json!({"error_code":0,"data":value})).unwrap(),
        )
        .unwrap();
        assert!(parsed.trusted_qr);
        assert_eq!(parsed.parameter_encryption, Some(true));
        assert!(ServerConfig::parse(&value).unwrap().services.is_empty());
    }
    #[test]
    fn public_prelogin_integer_encryption_flag_is_strictly_supported() {
        for (flag, expected) in [
            (serde_json::json!(1), true),
            (serde_json::json!(0), false),
            (serde_json::json!(true), true),
        ] {
            let value = serde_json::json!({"setttings":{"authen":{"encryption":{"enable":flag},"trusted_device":{"self_g":1,"qr_format_trust_device":1}}}});
            let parsed = ServerConfig::parse(&value).unwrap();
            assert_eq!(parsed.parameter_encryption, Some(expected));
            assert!(parsed.trusted_qr);
        }
        for flag in [
            serde_json::json!(2),
            serde_json::json!(-1),
            serde_json::json!("1"),
            serde_json::json!(null),
        ] {
            assert!(ServerConfig::parse(
                &serde_json::json!({"setttings":{"authen":{"encryption":{"enable":flag}}}})
            )
            .is_err());
        }
    }
    #[test]
    fn server_request_matches_independent_node_signature_and_local_field_contract() {
        let request = server_info_request(&ServerInfoInput {
            client_type: ClientType::Legacy30,
            imei: "fixture-imei",
            computer_name: "fixture-pc",
            support_maintenance: None,
            bucket_id: None,
        })
        .unwrap();
        let params: BTreeMap<_, _> = request
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(request.url.path(), "/api/login/getServerInfo");
        assert_eq!(params["signkey"], "d12b1a8f9693b2f6361ca580b6810188");
        assert_eq!(params["sp_mtn"], "-1");
        assert_eq!(params["bkt"], "-1");
        assert_eq!(params["client_version"], "681");
        assert_eq!(params["type"], "30");
        assert_eq!(request.timeout_ms, 10_000);
        assert!(request.include_cookies);
        assert!(!request.follow_redirects);
    }
    #[test]
    fn encrypted_login_request_contains_only_verified_minimal_fields() {
        let (request, cipher) = login_info_request(&login_input()).unwrap();
        let params: BTreeMap<_, _> = request
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(!params.contains_key("imei"));
        assert_eq!(params["enc_ver"], "v2");
        let plaintext = cipher.decrypt(&params["params"]).unwrap();
        assert_eq!(
            std::str::from_utf8(&plaintext).unwrap(),
            r#"{"imei":"fixture-imei","computer_name":"fixture-pc","language":"vi","ts":1700000000123}"#
        );
    }
    #[test]
    fn login_reply_decrypts_complete_nested_envelope_from_independent_node_fixture() {
        let (_, cipher) = login_info_request(&login_input()).unwrap();
        // Independently generated with Node crypto AES256CBC, fixed QR parameter key,
        // and synthetic session fields. This is not a capture of a successful account.
        let encrypted="nWqS/+3ptzV3WbEutt6y3ajhUU1tfIXV5EH8HSxtxEG3R3Koa5MIWKMpMVNliv0vVcMK8tq3rN0/SgrjmasIs5UTWu91O8NAITE5zUzlbCUyic01fpxPmBFpYsJVofTKUar4BrMpy505XxJoQSxJNK9LrJnm2F3BqLQ18KkconglrsKJHhdVJQEwNswdBBNlbdTRZEuL/VAG+neEgxfL00wE8xCYBvlWWOVJpBZz6sYeZLYvxX7as8EGOtBnLAsRl5IzRAy6y5q+G8jg2c8QS0nqH7hGPXEv1IrA95eTBxDAzIK7sHGfpvcVA7fGea4EcblIjHhML/pCBK/zFtev1phCxtaTDkKXpXcTFrHD9z1hIo7jLVKUnflOayoLtk10Xe7NXb9M97fi5R65xFMmgF7bZ5/fNu8v1b2gFM5iezOyu37k7wm8FgCaL/e/RR+s";
        let outer = serde_json::to_vec(&json!({"error_code":0,"data":encrypted})).unwrap();
        let parsed = decode_login_reply(200, &outer, &cipher).unwrap();
        assert!(parsed.has_storage_key());
        assert_eq!(parsed.account_id, "11");
        assert_eq!(
            parsed.config.services["profile"][0].host_str(),
            Some("profile-wpa.zaloapp.com")
        ); // Directory profile capability is now consumed.
        assert_eq!(parsed.config.websocket[0].scheme(), "wss");
    }
    #[test]
    fn websocket_routes_are_preserved_while_unused_long_poll_cannot_grant_credentials() {
        let value = serde_json::json!({"zpw_service_map_v3":{"chat":["https://chat-wpa.zaloapp.com/"]},"zpw_ws":["wss://ws1.zalo.me/legacy/socket"],"zpw_chts_new":{"unsupported":"http://unconsumed.invalid/poll"},"zpw_ctrs_new":["https://unconsumed.invalid/control"]});
        let parsed = ServerConfig::parse(&value).unwrap();
        assert_eq!(parsed.websocket[0].path(), "/legacy/socket");
        assert!(parsed.chat_poll.is_empty());
        assert!(parsed.control_poll.is_empty());
        for url in [
            "ws://ws1.zalo.me/legacy/socket",
            "wss://ws1.zalo.me/legacy/socket?token=unsafe",
            "wss://ws1.zalo.me:444/socket",
            "wss://ws1.zalo.me.attacker.invalid/socket",
            "wss://user@ws1.zalo.me/socket",
            "wss://127.0.0.1/socket",
        ] {
            assert!(ServerConfig::parse(&serde_json::json!({"zpw_ws":[url]})).is_err());
        }
    }
    #[test]
    fn service_capabilities_filter_unused_schemas_and_validate_consumed_origins() {
        let mut map = serde_json::Map::new();
        for index in 0..100 {
            map.insert(
                format!("unused-cdn-{index}"),
                serde_json::json!({"future_schema":"http://unused.invalid/path"}),
            );
        }
        map.insert(
            "chat".into(),
            serde_json::json!(["https://chat-wpa.zaloapp.com/"]),
        );
        map.insert(
            "analytics".into(),
            serde_json::json!(["https://metrics.invalid/collect"]),
        );
        let mut value = serde_json::json!({"zpw_service_map_v3":map,"zpw_service_map_new":{"group":"https://group-wpa.zaloapp.com/"}});
        let parsed = ServerConfig::parse(&value).unwrap();
        assert_eq!(parsed.services.len(), 2);
        assert_eq!(
            parsed.services["group"][0].host_str(),
            Some("group-wpa.zaloapp.com")
        );
        for bad in [
            "http://chat-wpa.zaloapp.com/",
            "https://chat-wpa.zaloapp.com.attacker.invalid/",
            "https://127.0.0.1/",
            "https://chat-wpa.zaloapp.com/path",
        ] {
            value["zpw_service_map_v3"]["chat"] = serde_json::json!([bad]);
            assert!(ServerConfig::parse(&value).is_err());
        }
    }
    #[test]
    fn optional_directory_routes_fail_locally_without_rejecting_verified_bootstrap_identity() {
        let base = json!({"uid":"11","dkey":"synthetic-storage","zpw_enk":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=","zpw_service_map_v3":{"chat":["https://chat-wpa.zaloapp.com"],"group":["https://group-wpa.zaloapp.com"]},"zpw_ws":["wss://ws1.zalo.me/socket"]});
        let expected = LoginInfo::parse(&base)
            .unwrap()
            .session_cipher
            .encrypt(b"same session key")
            .unwrap();
        for bad in [
            json!(null),
            json!({"future":"schema"}),
            json!("https://discarded.zaloapp.com"),
            json!(["https://attacker.invalid"]),
            json!(["https://discarded.zaloapp.com", "https://attacker.invalid"]),
            json!(["https://profile.zalo.me/path"]),
        ] {
            let mut value = base.clone();
            value["zpw_service_map_v3"]["profile"] = bad.clone();
            value["zpw_service_map_v3"]["conversation"] = bad;
            let parsed = LoginInfo::parse(&value).unwrap();
            assert_eq!(parsed.account_id, "11");
            assert_eq!(
                parsed.session_cipher.encrypt(b"same session key").unwrap(),
                expected
            );
            assert_eq!(parsed.config.services.len(), 2);
            assert_eq!(parsed.config.websocket[0].path(), "/socket");
            value["zpw_service_map_new"] = json!({"profile":"https://safe-profile.zaloapp.com","conversation":"https://safe-conversation.zalo.me"});
            let parsed = LoginInfo::parse(&value).unwrap();
            assert_eq!(
                parsed.config.services["profile"][0].host_str(),
                Some("safe-profile.zaloapp.com")
            );
            assert_eq!(
                parsed.config.services["conversation"][0].host_str(),
                Some("safe-conversation.zalo.me")
            );
            value["zpw_service_map_new"] = json!({"profile":"http://unsafe.zalo.me","conversation":"https://attacker.invalid"});
            assert_eq!(LoginInfo::parse(&value).unwrap().config.services.len(), 2);
            value["zpw_service_map_v3"]["chat"] = json!(["https://attacker.invalid"]);
            assert!(
                LoginInfo::parse(&value).is_err(),
                "required messaging origins stay strict"
            );
        }
    }
    #[cfg(feature = "messaging")]
    #[test]
    fn discarded_optional_routes_never_receive_session_cookies() {
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!(
            "native-optional-route-{}",
            crate::domain::local_message_id().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&root, "fixture").unwrap());
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://chat.zalo.me"],"profile":["https://discarded.zaloapp.com","https://attacker.invalid"],"conversation":{"future":"https://discarded-conversation.zalo.me"}},"zpw_service_map_new":{"profile":"https://safe-profile.zaloapp.com","conversation":"https://attacker.invalid"}})).unwrap();
        let session = super::super::session::synthetic_session(profile.clone(), config);
        for denied in [
            "https://discarded.zaloapp.com/api/social/friend/getfriends",
            "https://discarded-conversation.zalo.me/api/preloadconvers/get-last-msgs",
            "https://attacker.invalid/",
        ] {
            assert!(session
                .authorization_cookie(&Url::parse(denied).unwrap())
                .is_none());
        }
        assert!(session
            .authorization_cookie(
                &Url::parse("https://safe-profile.zaloapp.com/api/social/friend/getfriends")
                    .unwrap()
            )
            .is_some());
        drop(session);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn detailed_bootstrap_failures_do_not_mislabel_parsing_as_server_rejection() {
        let (_, cipher) = login_info_request(&login_input()).unwrap();
        for (status, body, stage, category) in [
            (
                503,
                b"not returned".as_slice(),
                BootstrapStage::Http,
                BootstrapError::Http(503),
            ),
            (
                200,
                b"not json".as_slice(),
                BootstrapStage::OuterEnvelope,
                BootstrapError::Malformed,
            ),
            (
                200,
                br#"{"error_code":0,"data":"not-a-ciphertext-or-secret"}"#.as_slice(),
                BootstrapStage::ResponseDecryption,
                BootstrapError::Malformed,
            ),
            (
                200,
                br#"{"error_code":600,"error_message":"not-retained-secret"}"#.as_slice(),
                BootstrapStage::OuterEnvelope,
                BootstrapError::AuthenticationRejected(600),
            ),
        ] {
            let failure = decode_login_reply_detailed(status, body, &cipher)
                .err()
                .unwrap();
            assert_eq!(failure.stage, stage);
            assert_eq!(failure.category, category);
            assert!(!format!("{failure:?} {failure}").contains("secret"));
        }
    }
    #[test]
    fn session_cipher_matches_node_known_answers_for_all_supported_aes_key_lengths() {
        let fixtures=[
            ("AAECAwQFBgcICQoLDA0ODw==","xM8stAIW28htiNwUylMQIKBerDIhBpnlD1fkhZTGrwvehihwxr0tifekORn9mjZo0brD+1CKcokl6mdS2QyhkA=="),
            ("AAECAwQFBgcICQoLDA0ODxAREhMUFRYX","R/R6rr0Bv8HD3XoEFt0+u7/VzMGavgHigb0FMiNtbJeXIahLa/Uil52ftGsjtQDM2tneKTseOZf95nPbjokCBQ=="),
            ("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=","JMyqeImeSOds/xfo2x65M+l8ZmPYjkminVDQ/rpkLaLwc3IvDzkqCG/9i2JgaRaIKZ06FXoIahfb+iSSuySy0g=="),
        ];
        for (key, ciphertext) in fixtures {
            let cipher = SessionCipher::from_base64(key).unwrap();
            assert_eq!(
                std::str::from_utf8(&cipher.decrypt(ciphertext).unwrap()).unwrap(),
                r#"{"error_code":0,"data":{"fixture":"Tiếng Việt"}}"#
            );
            assert_eq!(
                cipher.decrypt(&ciphertext.replace('+', "%2B")).unwrap(),
                cipher.decrypt(ciphertext).unwrap()
            );
            assert!(cipher.decrypt("invalid").is_err());
        }
        assert!(SessionCipher::from_base64("c2hvcnQ=").is_err());
    }
    #[test]
    fn only_locally_verified_auth_codes_expire_authentication_and_never_http_failures() {
        for code in [600, 102, -102, 112, -69, 19000, 18060, 999] {
            let body = serde_json::to_vec(&json!({"error_code":code})).unwrap();
            let error = decode_server_info(200, &body).unwrap_err();
            assert_eq!(
                error.invalidates_authentication(),
                matches!(code, 600 | 102)
            );
            assert!(!decode_server_info(503, &body)
                .unwrap_err()
                .invalidates_authentication());
        }
        assert!(!decode_server_info(200, b"not json")
            .unwrap_err()
            .invalidates_authentication());
        let (_, cipher) = login_info_request(&login_input()).unwrap();
        assert!(matches!(
            decode_login_reply(200, br#"{"error_code":19000}"#, &cipher),
            Err(BootstrapError::EncryptionRejected(19000))
        ));
        assert!(matches!(
            decode_login_reply(200, br#"{"error_code":0,"data":{}}"#, &cipher),
            Err(BootstrapError::Malformed)
        ));
    }
    #[test]
    fn configuration_preserves_prelogin_spelling_and_rejects_untrusted_hosts_and_schemas() {
        let parsed=decode_server_info(200,br#"{"error_code":0,"data":{"server_time":1700000000,"setttings":{"authen":{"encryption":{"enable":true}}}}}"#).unwrap();
        assert_eq!(parsed.parameter_encryption, Some(true));
        assert_eq!(parsed.server_time_seconds, Some(1_700_000_000));
        for host in [
            "http://profile.zalo.me",
            "https://zalo.me.evil.example",
            "https://evil.example",
            "https://user:password@profile.zalo.me",
            "https://profile.zalo.me:444",
            "https://profile.zalo.me/?secret=x",
        ] {
            assert!(
                ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":[host]}})).is_err(),
                "{host}"
            );
        }
        assert!(ServerConfig::parse(
            &json!({"zpw_service_map_v3":{"chat":"https://chat.zalo.me"}})
        )
        .is_err());
        assert!(ServerConfig::parse(&json!({"zpw_ws":["https://ws1-msg.chat.zalo.me"]})).is_err());
        assert!(ServerConfig::parse(&json!({"setttings":{"authen":{"encryption":{"enable":true}}},"settings":{"authen":{"encryption":{"enable":false}}}})).is_err());
    }
    #[test]
    fn confirmation_cookie_plan_does_not_confuse_identity_refresh_token_and_auth_cookie() {
        let pending=PendingCookies::from_confirmation(&json!({"uid":"123456","dkey":"opaque-storage-key","zpw_sek":"synthetic-cookie","zpw_kt":"synthetic-refresh"})).unwrap();
        assert_eq!(pending.claimed_account(), "123456");
        assert!(pending.has_refresh_token());
        assert!(pending.has_storage_key());
        assert_eq!(
            pending.cookie_domains(),
            ["zaloapp.com", "zalo.me", "chat.zalo.me"]
        );
        assert_eq!(
            pending.request_cookie(
                &Url::parse("https://wpa.zaloapp.com/api/login/getLoginInfo").unwrap()
            ),
            Some("zpw_sek=synthetic-cookie".into())
        );
        for origin in [
            "http://wpa.zaloapp.com",
            "https://zalo.me.evil.example",
            "https://example.com",
            "https://wpa.zaloapp.com:444",
        ] {
            assert!(pending
                .request_cookie(&Url::parse(origin).unwrap())
                .is_none());
        }
        assert!(PendingCookies::from_confirmation(
            &json!({"uid":"123","dkey":"d","zpw_sek":"bad; injected=x"})
        )
        .is_err());
        assert!(PendingCookies::from_confirmation(
            &json!({"uid":"not-account","dkey":"d","zpw_sek":"cookie"})
        )
        .is_err());
    }
}
