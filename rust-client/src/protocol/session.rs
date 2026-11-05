//! Durable account ownership and opaque, verified runtime session capability.
use super::{
    bootstrap::{ServerConfig, SessionCipher},
    client::ClientType,
};
use crate::profile::{atomic_write_secret, Profile};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{collections::BTreeMap, fs::OpenOptions, io::Read, sync::Arc};
use url::Url;

const MAX_RECORD: u64 = 3 * 1024 * 1024;
pub(super) const AUTH_STATE_FILE: &str = "auth-state.json";

// Secret structs intentionally have no Debug implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredSession {
    #[serde(default = "ClientType::legacy30")]
    pub client_type: ClientType,
    pub account_id: String,
    pub display_name: String,
    pub imei: String,
    pub cookie: String,
    pub refresh_token: Option<String>,
    pub storage_key: String,
    pub session_key: String,
    pub proof: SessionProof,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAssurance {
    PhoneApproved,
    TrustedDevice,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "assurance", content = "trust", deny_unknown_fields)]
pub(super) enum SessionProof {
    PhoneApproved,
    TrustedDevice(StoredTrust),
}
impl SessionProof {
    pub fn assurance(&self) -> AuthAssurance {
        match self {
            Self::PhoneApproved => AuthAssurance::PhoneApproved,
            Self::TrustedDevice(_) => AuthAssurance::TrustedDevice,
        }
    }
    #[cfg(test)]
    pub fn trusted_ref(&self) -> &StoredTrust {
        match self {
            Self::TrustedDevice(trust) => trust,
            _ => panic!("expected synthetic trusted fixture"),
        }
    }
    #[cfg(test)]
    pub fn trusted_mut(&mut self) -> &mut StoredTrust {
        match self {
            Self::TrustedDevice(trust) => trust,
            _ => panic!("expected synthetic trusted fixture"),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredTrust {
    pub master_id: String,
    pub primary_key: Vec<u8>,
    pub companion_public: Vec<u8>,
    pub companion_private: Vec<u8>,
    pub linking_secret: Vec<u8>,
    pub signed_list: Vec<u8>,
    pub list_timestamp: u64,
    pub linking_timestamp: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthRecord {
    version: u8,
    pub owner_uid: String,
    pub replay: BTreeMap<String, u64>,
    pub active: Option<StoredSession>,
}
impl AuthRecord {
    pub fn empty(owner: &str) -> Self {
        Self {
            version: 2,
            owner_uid: owner.into(),
            replay: BTreeMap::new(),
            active: None,
        }
    }
    pub fn load(profile: &Profile) -> Result<Option<Self>> {
        let path = profile.paths.data_dir.join(AUTH_STATE_FILE);
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => bail!("private session record cannot be opened"),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_RECORD {
            bail!("invalid session record size");
        }
        #[cfg(unix)]
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            bail!("session record is not private");
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD {
            bail!("session record exceeds limit");
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid session record"))?;
        if value.get("version").and_then(serde_json::Value::as_u64) == Some(2)
            && value
                .get("active")
                .is_some_and(|s| !s.is_null() && s.get("client_type").is_none())
        {
            bail!("session record requires an explicit client type");
        }
        let mut record: Self =
            serde_json::from_value(value).map_err(|_| anyhow::anyhow!("invalid session record"))?;
        if !matches!(record.version, 1 | 2)
            || !valid_account(&record.owner_uid)
            || record.replay.len() > 32
            || record
                .active
                .as_ref()
                .is_some_and(|s| s.account_id != record.owner_uid)
        {
            bail!("inconsistent session ownership");
        }
        profile.check_account(&record.owner_uid)?;
        record.version = 2; // In-memory schema upgrade; loading never rewrites credentials.
        Ok(Some(record))
    }
    pub fn write(&self, profile: &Profile) -> Result<()> {
        profile.check_account(&self.owner_uid)?;
        let bytes = self.encode()?;
        atomic_write_secret(&profile.paths.data_dir.join(AUTH_STATE_FILE), &bytes)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.replay.len() > 32 {
            bail!("trust identity history limit reached");
        }
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() as u64 > MAX_RECORD {
            bail!("session record exceeds limit");
        }
        Ok(bytes)
    }
}
pub(super) fn valid_account(account: &str) -> bool {
    !account.is_empty() && account.len() <= 64 && account.bytes().all(|b| b.is_ascii_digit())
}

/// Only the authentication coordinator can construct this runtime capability.
/// No Debug, serialization or public constructor can manufacture verified login.
pub struct VerifiedSession {
    client_type: ClientType,
    account_id: String,
    assurance: AuthAssurance,
    display_name: String,
    imei: String,
    config: ServerConfig,
    cipher: SessionCipher,
    cookie: String,
    _profile: Arc<Profile>,
}
impl VerifiedSession {
    pub(super) fn from_committed(
        saved: &StoredSession,
        config: ServerConfig,
        profile: Arc<Profile>,
    ) -> Result<Self> {
        if !saved.client_type.supports_native_session() {
            bail!("Fresh phone approval is required for this client type");
        }
        Ok(Self {
            client_type: saved.client_type,
            account_id: saved.account_id.clone(),
            assurance: saved.proof.assurance(),
            display_name: saved.display_name.clone(),
            imei: saved.imei.clone(),
            config,
            cipher: SessionCipher::from_base64(&saved.session_key)?,
            cookie: saved.cookie.clone(),
            _profile: profile,
        })
    }
    pub fn assurance(&self) -> AuthAssurance {
        self.assurance
    }
    pub fn client_type(&self) -> ClientType {
        self.client_type
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn imei(&self) -> &str {
        &self.imei
    }
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
    pub fn encrypt_payload(&self, bytes: &[u8]) -> Result<String> {
        self.cipher.encrypt(bytes)
    }
    pub fn decrypt_payload(&self, text: &str) -> Result<Vec<u8>> {
        self.cipher.decrypt(text).map_err(anyhow::Error::new)
    }
    /// Directory metadata may contain 20,000 profiles. This separate, fixed
    /// budget never enlarges authentication or messaging payload limits.
    #[cfg(feature = "messaging")]
    pub(crate) fn decrypt_directory_payload(&self, text: &str) -> Result<Vec<u8>> {
        self.cipher
            .decrypt_bounded(text, super::directory::MAX_DIRECTORY_BODY)
            .map_err(anyhow::Error::new)
    }
    pub fn authorization_cookie(&self, url: &Url) -> Option<String> {
        if !matches!(url.scheme(), "https" | "wss")
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let origin_allowed = self
            .config
            .services
            .values()
            .flatten()
            .chain(self.config.chat_poll.iter())
            .chain(self.config.control_poll.iter())
            .chain(self.config.websocket.iter())
            .any(|base| {
                base.host_str() == url.host_str()
                    && base.port_or_known_default() == url.port_or_known_default()
            });
        origin_allowed.then(|| format!("zpw_sek={}", self.cookie))
    }
}
pub(super) fn encode_key(cipher: &SessionCipher) -> String {
    STANDARD.encode(&cipher.key)
}

/// Transport-only test capability. This symbol does not exist in application builds.
#[cfg(all(test, feature = "messaging"))]
pub(crate) fn synthetic_session(profile: Arc<Profile>, config: ServerConfig) -> VerifiedSession {
    VerifiedSession {
        client_type: ClientType::PcLinux24,
        account_id: "11".into(),
        assurance: AuthAssurance::PhoneApproved,
        display_name: "Synthetic account".into(),
        imei: "native-fixture".into(),
        config,
        cipher: SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap(),
        cookie: "synthetic".into(),
        _profile: profile,
    }
}
