//! Current ordinary single-file async upload. Resource completion is not message acceptance.
use super::{
    client::ClientType,
    messaging::{self, Target, ThreadKind, WireError},
};
use crate::domain::{MediaKind, MediaMessageContent, MAX_DIRECT_FILE_BYTES};
use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;
use url::Url;
pub fn source_extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| extension.encode_utf16().count() < 7)
        .unwrap_or("")
        .to_owned()
}
pub fn normalized_file_name(name: &str) -> String {
    name.nfc().collect()
}

pub const MAX_CONTROL_CACHE: usize = 64;
pub const RESOURCE_TTL_SECS: u64 = 120;
pub fn resource_url(value: &str) -> Result<Url, WireError> {
    let url = Url::parse(value).map_err(|_| WireError::Invalid)?;
    if value.len() > 4096
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(WireError::Invalid);
    }
    Ok(url)
}
pub fn file_id(value: &Value) -> Result<String, WireError> {
    let id = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) if n.is_u64() => n.to_string(),
        _ => return Err(WireError::Invalid),
    };
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(WireError::Invalid);
    }
    Ok(id)
}
pub fn endpoint(
    root: &Url,
    kind: ThreadKind,
    client: ClientType,
    upload: bool,
) -> Result<Url, WireError> {
    messaging::validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(match (kind, upload) {
        (ThreadKind::Direct, true) => "/api/message/asyncfile/upload",
        (ThreadKind::Group, true) => "/api/group/asyncfile/upload",
        (ThreadKind::Direct, false) => "/api/message/asyncfile/msg",
        (ThreadKind::Group, false) => "/api/group/asyncfile/msg",
    });
    url.query_pairs_mut()
        .append_pair("zpw_ver", messaging::API_VERSION)
        .append_pair("zpw_type", client.api_type());
    if upload {
        url.query_pairs_mut().append_pair(
            "type",
            if kind == ThreadKind::Direct {
                "2"
            } else {
                "11"
            },
        );
    }
    Ok(url)
}
#[allow(clippy::too_many_arguments)]
pub fn chunk_parameters(
    target: &Target,
    name: &str,
    size: u64,
    id: u64,
    imei: &str,
    chunk: u64,
    total: u64,
) -> Result<Value, WireError> {
    validate_file(name, size)?;
    if id == 0 || chunk == 0 || chunk > total || total > 2000 || imei.is_empty() {
        return Err(WireError::Invalid);
    }
    let mut value = json!({"totalChunk":total,"fileName":name,"clientId":id,"totalSize":size,"imei":imei,"chunkId":chunk});
    value[if target.kind == ThreadKind::Direct {
        "toid"
    } else {
        "grid"
    }] = json!(target.remote_id);
    Ok(value)
}
fn validate_file(name: &str, size: u64) -> Result<(), WireError> {
    if size == 0
        || size > MAX_DIRECT_FILE_BYTES
        || name.is_empty()
        || name.len() > 255
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(WireError::Invalid);
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn message_parameters(
    target: &Target,
    name: &str,
    extension: &str,
    size: u64,
    checksum: &str,
    id: u64,
    imei: &str,
    server_file_id: &str,
    url: &str,
) -> Result<Value, WireError> {
    validate_file(name, size)?;
    file_id(&json!(server_file_id))?;
    resource_url(url)?;
    if id == 0
        || extension.len() > 128
        || checksum.len() != 32
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(WireError::Invalid);
    }
    let name = normalized_file_name(name);
    let mut value = json!({"fileId":server_file_id,"checksum":checksum,"checksumSha":"","extension":extension,"totalSize":size,"fileName":name,"clientId":id,"fType":1,"fileCount":0,"fdata":"{}","fileUrl":url,"ttl":0});
    value[if target.kind == ThreadKind::Direct {
        "toid"
    } else {
        "grid"
    }] = json!(target.remote_id);
    if target.kind == ThreadKind::Direct {
        if imei.is_empty() {
            return Err(WireError::Invalid);
        }
        value["imei"] = json!(imei);
    }
    Ok(value)
}
pub fn media_content(
    name: &str,
    extension: &str,
    size: u64,
    checksum: &str,
    url: &str,
) -> Result<MediaMessageContent, WireError> {
    validate_file(name, size)?;
    resource_url(url)?;
    let name = normalized_file_name(name);
    MediaMessageContent::new(MediaKind::File,&json!({"title":name,"description":"","href":url,"params":{"fileSize":size,"checksum":checksum,"checksumSha":"","fileExt":extension,"fdata":"{}","fType":1}})).map_err(|_|WireError::Invalid)
}
/// Online control only. Unknown/malformed controls never become chat messages or authorize URLs.
pub fn file_done(payload: &Value) -> Vec<(String, String)> {
    if messaging::service_code(payload).is_err() {
        return vec![];
    }
    let Some(controls) = payload
        .pointer("/data/controls")
        .and_then(Value::as_array)
        .filter(|v| v.len() <= 200)
    else {
        return vec![];
    };
    controls
        .iter()
        .filter_map(|control| {
            let content = control.get("content")?;
            if content.get("act_type")?.as_str()? != "file_done" {
                return None;
            }
            let id = file_id(content.get("fileId")?).ok()?;
            let raw = content.get("data")?;
            let decoded;
            let data = if let Some(text) = raw.as_str() {
                if text.len() > 16 * 1024 {
                    return None;
                }
                decoded = serde_json::from_str::<Value>(text).ok()?;
                &decoded
            } else {
                raw
            };
            let url = data.get("url")?.as_str()?;
            resource_url(url).ok()?;
            Some((id, url.to_owned()))
        })
        .take(MAX_CONTROL_CACHE)
        .collect()
}
#[derive(Debug, PartialEq, Eq)]
pub enum ChunkResult {
    Continue,
    Finished {
        file_id: String,
        url: Option<String>,
    },
}
pub fn chunk_result(payload: &Value) -> Result<ChunkResult, WireError> {
    let code = payload
        .get("error_code")
        .and_then(Value::as_i64)
        .ok_or(WireError::Invalid)?;
    if code != 0 && code != 1 {
        return Err(WireError::Service(code));
    }
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    if code == 0
        && !data
            .get("finished")
            .is_some_and(|v| v.as_bool() == Some(true) || v.as_u64() == Some(1))
    {
        return Ok(ChunkResult::Continue);
    }
    let id = file_id(data.get("fileId").ok_or(WireError::Invalid)?)?;
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .map(|value| resource_url(value).map(|_| value.to_owned()))
        .transpose()?;
    // A duplicate without its URL cannot safely recreate the resource using this client ID.
    if code == 1 && url.is_none() {
        return Err(WireError::Invalid);
    }
    Ok(ChunkResult::Finished { file_id: id, url })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_file_parameters_keep_current_source_defaults_and_distinct_group_route() {
        let direct = Target::new(ThreadKind::Direct, "22").unwrap();
        let group = Target::new(ThreadKind::Group, "33").unwrap();
        let digest = "0123456789abcdef0123456789abcdef";
        let p = message_parameters(
            &direct,
            "báo cáo.txt",
            "txt",
            42,
            digest,
            123,
            "imei",
            "987",
            "https://files.example.test/f",
        )
        .unwrap();
        assert_eq!(
            p,
            json!({"fileId":"987","checksum":digest,"checksumSha":"","extension":"txt","totalSize":42,"fileName":"báo cáo.txt","clientId":123,"fType":1,"fileCount":0,"fdata":"{}","fileUrl":"https://files.example.test/f","ttl":0,"toid":"22","imei":"imei"})
        );
        let g = message_parameters(
            &group,
            "a.txt",
            "txt",
            42,
            digest,
            123,
            "imei",
            "987",
            "https://files.example.test/f",
        )
        .unwrap();
        assert_eq!(g["grid"], "33");
        assert!(g.get("imei").is_none());
        assert!(g.get("visibility").is_none());
        let c = chunk_parameters(&direct, "a.txt", 42, 123, "imei", 1, 1).unwrap();
        assert_eq!(c["chunkId"], 1);
        assert_eq!(c["totalChunk"], 1);
        assert!(c.get("checksum").is_none());
    }
    #[test]
    fn completion_is_separate_from_message_ack_and_bad_control_does_not_poison_chat() {
        assert_eq!(
            chunk_result(&json!({"error_code":0,"data":{}})).unwrap(),
            ChunkResult::Continue
        );
        assert_eq!(
            chunk_result(&json!({"error_code":0,"data":{"finished":1,"fileId":987}})).unwrap(),
            ChunkResult::Finished {
                file_id: "987".into(),
                url: None
            }
        );
        assert!(chunk_result(&json!({"error_code":1,"data":{"fileId":987}})).is_err());
        assert!(chunk_result(&json!({"error_code":2,"data":{}})).is_err());
        let payload = json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"other","fileId":"bad"}},{"content":{"act_type":"file_done","fileId":987,"data":"{\"url\":\"https://files.example.test/f\"}"}},{"content":{"act_type":"file_done","fileId":988,"data":{"url":"https://x:y@files.example.test/f"}}}]}});
        assert_eq!(
            file_done(&payload),
            vec![("987".into(), "https://files.example.test/f".into())]
        );
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use crate::protocol::bootstrap::ServerConfig;
    #[test]
    fn file_capability_requires_trusted_file_service_and_supported_handler() {
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"file":["https://file.zalo.me/"]},"settings":{"sharefile":{"chunk_size_file":"64000"}}})).unwrap();
        assert_eq!(config.file_chunk_size, 64000);
        assert!(config.services.contains_key("file"));
        for flag in ["enable_multi_site_api", "enable_my_cloud"] {
            let mut value = json!({"zpw_service_map_v3":{"file":["https://file.zalo.me/"]},"settings":{"multi_site_upload":{}}});
            value["settings"]["multi_site_upload"][flag] = json!(true);
            assert!(!ServerConfig::parse(&value)
                .unwrap()
                .services
                .contains_key("file"));
        }
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"file":["https://untrusted.example.test/"]},"settings":{"sharefile":{"chunk_size_file":999999999}}})).unwrap();
        assert!(!config.services.contains_key("file"));
        assert_eq!(config.file_chunk_size, 256000);
    }
    #[test]
    fn final_file_name_is_nfc_but_chunk_name_keeps_source_spelling() {
        let target = Target::new(ThreadKind::Direct, "22").unwrap();
        let name = "ba\u{301}o.txt";
        assert_eq!(
            chunk_parameters(&target, name, 3, 1, "imei", 1, 1).unwrap()["fileName"],
            name
        );
        assert_eq!(
            message_parameters(
                &target,
                name,
                "txt",
                3,
                "900150983cd24fb0d6963f7d28e17f72",
                1,
                "imei",
                "1",
                "https://files.example.test/f"
            )
            .unwrap()["fileName"],
            "báo.txt"
        );
    }
}
