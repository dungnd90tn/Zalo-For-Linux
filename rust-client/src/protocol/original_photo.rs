//! Subscription proof for the source's ordinary original-photo upload path.
use super::{
    bootstrap::AUTH_ORIGIN,
    client::ClientType,
    messaging::{self as wire, WireError},
};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use url::Url;

pub(crate) const UPLOAD_SERVICE: &str = "zcloud_up_file";
pub(crate) const PLAN_SERVICE: &str = "zcloud";
pub(super) const VIEWER_PATH: &str = "/api/getCloudViewerKey";
const PLAN_PATH: &str = "/cloudmedia/info/pc/v1/usage-mycloud";
pub(crate) const MAX_VIEWER_RESPONSE: usize = 64 * 1024;

pub(crate) fn viewer_url(client: ClientType) -> Result<Url, WireError> {
    let root = Url::parse(AUTH_ORIGIN).map_err(|_| WireError::Invalid)?;
    service_url(&root, VIEWER_PATH, client)
}

pub(crate) fn plan_url(root: &Url, client: ClientType) -> Result<Url, WireError> {
    service_url(root, PLAN_PATH, client)
}

fn service_url(root: &Url, path: &str, client: ClientType) -> Result<Url, WireError> {
    wire::validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", client.api_type());
    Ok(url)
}

pub(crate) struct Viewer {
    pub header: reqwest::header::HeaderValue,
    key: Vec<u8>,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}

impl Viewer {
    pub(crate) fn parse(payload: &Value) -> Result<Self, WireError> {
        wire::service_code(payload)?;
        let data = payload.get("data").ok_or(WireError::Invalid)?;
        let parsed;
        let data = if let Some(text) = data.as_str() {
            parsed = serde_json::from_str::<Value>(text).map_err(|_| WireError::Invalid)?;
            &parsed
        } else {
            data
        };
        let mut header = data
            .get("viewer_key")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty() && text.len() <= 4096)
            .and_then(|text| reqwest::header::HeaderValue::from_str(text).ok())
            .ok_or(WireError::Invalid)?;
        header.set_sensitive(true);
        let key = data
            .get("enk")
            .and_then(Value::as_str)
            .filter(|text| text.len() <= 128)
            .and_then(|text| STANDARD.decode(text).ok())
            .filter(|bytes| matches!(bytes.len(), 16 | 24 | 32))
            .ok_or(WireError::Invalid)?;
        Ok(Self { header, key })
    }

    pub(crate) fn paid_plan(&self, payload: &Value) -> Result<bool, WireError> {
        // Source accepts -904 responses for display, but it cannot prove paid entitlement.
        wire::service_code(payload)?;
        let cipher = payload
            .get("data")
            .and_then(Value::as_str)
            .filter(|text| text.len() <= MAX_VIEWER_RESPONSE)
            .ok_or(WireError::Invalid)?;
        let mut bytes = STANDARD.decode(cipher).map_err(|_| WireError::Crypto)?;
        if bytes.len() <= 16 || bytes.len() % 16 != 0 {
            return Err(WireError::Crypto);
        }
        let (iv, cipher) = bytes.split_at_mut(16);
        let plain = match self.key.len() {
            16 => cbc::Decryptor::<aes::Aes128>::new_from_slices(&self.key, iv)
                .map_err(|_| WireError::Crypto)?
                .decrypt_padded_mut::<Pkcs7>(cipher),
            24 => cbc::Decryptor::<aes::Aes192>::new_from_slices(&self.key, iv)
                .map_err(|_| WireError::Crypto)?
                .decrypt_padded_mut::<Pkcs7>(cipher),
            32 => cbc::Decryptor::<aes::Aes256>::new_from_slices(&self.key, iv)
                .map_err(|_| WireError::Crypto)?
                .decrypt_padded_mut::<Pkcs7>(cipher),
            _ => return Err(WireError::Crypto),
        }
        .map_err(|_| WireError::Crypto)?;
        let row: Value = serde_json::from_slice(plain).map_err(|_| WireError::Invalid)?;
        let parsed;
        let row = if let Some(text) = row.as_str() {
            parsed = serde_json::from_str::<Value>(text).map_err(|_| WireError::Invalid)?;
            &parsed
        } else {
            &row
        };
        Ok(row.get("plan").and_then(Value::as_i64) == Some(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;
    use serde_json::json;

    #[test]
    fn original_requires_exact_paid_plan_inside_authorized_cloud_cipher() {
        let key = [7u8; 32];
        let viewer = Viewer::parse(&json!({
            "error_code": 0,
            "data": {"viewer_key":"fixture-viewer","enk":STANDARD.encode(key)}
        }))
        .unwrap();
        assert!(viewer.header.is_sensitive());
        let payload = |value: Value| {
            let iv = [9u8; 16];
            let encrypted = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
                .unwrap()
                .encrypt_padded_vec_mut::<Pkcs7>(value.to_string().as_bytes());
            json!({"error_code":0,"data":STANDARD.encode([iv.as_slice(), encrypted.as_slice()].concat())})
        };
        assert_eq!(viewer.paid_plan(&payload(json!({"plan":0}))), Ok(true));
        for plan in [
            json!(-1),
            json!(69),
            json!(100),
            json!("0"),
            json!(1),
            Value::Null,
        ] {
            assert_eq!(viewer.paid_plan(&payload(json!({"plan":plan}))), Ok(false));
        }
        assert_eq!(
            viewer.paid_plan(&json!({"error_code":0,"data":{"plan":0}})),
            Err(WireError::Invalid)
        );
    }
}
