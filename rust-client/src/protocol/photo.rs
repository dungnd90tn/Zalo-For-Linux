//! Ordinary photo upload and final submission; resource completion is not an ACK.
use super::{
    bootstrap::PhotoUploadRoute,
    client::ClientType,
    messaging::{self, Target, ThreadKind, WireError},
    upload,
};
pub use crate::domain::{
    media::{has_album_message_fields, MAX_WIRE_ALBUM_PHOTOS},
    PhotoAlbumMember as AlbumMember,
};
use crate::domain::{MediaKind, MediaMessageContent, MAX_TEXT_BYTES};
use serde_json::{json, Value};
use url::Url;

impl AlbumMember {
    /// PC bundle N136681–136724 and N401554–401571. Every photo is submitted
    /// separately with the same layout ID and total, and its own zero-based index.
    pub fn apply_to_message_parameters(
        self,
        parameters: &mut Value,
        attach_extra_metadata: bool,
    ) -> Result<(), WireError> {
        if has_album_message_fields(parameters)
            || !self.is_valid()
            || self.count() > MAX_WIRE_ALBUM_PHOTOS
        {
            return Err(WireError::Invalid);
        }
        let fields = parameters.as_object_mut().ok_or(WireError::Invalid)?;
        let layout_id = self.layout_id().to_string();
        if attach_extra_metadata {
            fields.insert(
                "extMsgProp".into(),
                json!(json!({"groupMediaMsg":{"groupLayoutId":layout_id}}).to_string()),
            );
        }
        fields.insert("groupLayoutId".into(), json!(layout_id));
        fields.insert("idInGroup".into(), json!(self.index()));
        fields.insert("isGroupLayout".into(), json!(1));
        fields.insert("totalItemInGroup".into(), json!(self.count()));
        Ok(())
    }

    pub fn apply_to_media_content(
        self,
        content: &MediaMessageContent,
    ) -> Result<MediaMessageContent, WireError> {
        if content.kind() != MediaKind::Photo || self.count() > MAX_WIRE_ALBUM_PHOTOS {
            return Err(WireError::Invalid);
        }
        let mut raw: Value =
            serde_json::from_str(content.as_json()).map_err(|_| WireError::Invalid)?;
        let params = raw
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or(WireError::Invalid)?;
        for (name, value) in [
            ("group_layout_id", json!(self.layout_id().to_string())),
            ("id_in_group", json!(self.index())),
            ("is_group_layout", json!(1)),
            ("total_item_in_group", json!(self.count())),
        ] {
            if params.insert(name.into(), value).is_some() {
                return Err(WireError::Invalid);
            }
        }
        let result =
            MediaMessageContent::new(MediaKind::Photo, &raw).map_err(|_| WireError::Invalid)?;
        if result.album_member() != Some(self) {
            return Err(WireError::Invalid);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoUrls {
    pub org: String,
    pub thumb: String,
    pub hd: String,
    pub thumb_size: Option<u64>,
    pub file_size: Option<u64>,
    pub hd_size: Option<u64>,
    explicit_original: bool,
}
impl PhotoUrls {
    pub fn from_value(value: &Value) -> Result<Self, WireError> {
        let get = |key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        };
        let org = get("org")
            .or_else(|| get("hd"))
            .or_else(|| get("thumb"))
            .ok_or(WireError::Invalid)?;
        let thumb = get("thumb")
            .or_else(|| get("hd"))
            .or_else(|| get("org"))
            .ok_or(WireError::Invalid)?;
        let hd = get("hd")
            .or_else(|| get("org"))
            .or_else(|| get("thumb"))
            .ok_or(WireError::Invalid)?;
        for url in [org, thumb, hd] {
            upload::resource_url(url)?;
        }
        let size = |name| {
            value
                .get(name)
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n <= 104_857_600)
                        .ok_or(WireError::Invalid)
                })
                .transpose()
        };
        Ok(Self {
            org: org.into(),
            thumb: thumb.into(),
            hd: hd.into(),
            thumb_size: size("thumbSize")?,
            file_size: size("fileSize")?,
            hd_size: size("hdSize")?,
            explicit_original: get("org").is_some(),
        })
    }

    pub(crate) fn matches_original(&self, size: u64) -> bool {
        self.explicit_original && self.file_size == Some(size)
    }
}
pub fn endpoint(
    root: &Url,
    kind: ThreadKind,
    client: ClientType,
    route: Option<PhotoUploadRoute>,
) -> Result<Url, WireError> {
    messaging::validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(match (kind, route) {
        (ThreadKind::Direct, Some(PhotoUploadRoute::File)) => "/api/message/photo_original/upload",
        (ThreadKind::Group, Some(PhotoUploadRoute::File)) => "/api/group/photo_original/upload",
        (ThreadKind::Direct, Some(PhotoUploadRoute::Photo)) => "/api/message/upload/photo",
        (ThreadKind::Group, Some(PhotoUploadRoute::Photo)) => "/api/group/upload/photo",
        (ThreadKind::Direct, Some(PhotoUploadRoute::Original)) => {
            "/api/message/zcloud-upload/photo"
        }
        (ThreadKind::Group, Some(PhotoUploadRoute::Original)) => "/api/group/zcloud-upload/photo",
        (ThreadKind::Direct, None) => "/api/message/photo_original/send",
        (ThreadKind::Group, None) => "/api/group/photo_original/send",
    });
    url.query_pairs_mut()
        .append_pair("zpw_ver", messaging::API_VERSION)
        .append_pair("zpw_type", client.api_type());
    if route.is_some() {
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
pub fn photo_success(payload: &Value) -> Vec<(String, PhotoUrls)> {
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
        .filter_map(|entry| {
            let content = entry.get("content")?;
            if content.get("act_type")?.as_str()? != "photo_success"
                || content.get("act")?.as_str()? != "photo"
            {
                return None;
            }
            let id = upload::file_id(content.get("fileId")?).ok()?;
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
            Some((id, PhotoUrls::from_value(data).ok()?))
        })
        .take(upload::MAX_CONTROL_CACHE)
        .collect()
}
pub enum PhotoChunkResult {
    Continue,
    Finished {
        photo_id: String,
        urls: Option<PhotoUrls>,
    },
}
pub fn chunk_result(payload: &Value) -> Result<PhotoChunkResult, WireError> {
    let code = payload
        .get("error_code")
        .and_then(Value::as_i64)
        .ok_or(WireError::Invalid)?;
    if code != 0 && code != 1 {
        return Err(WireError::Service(code));
    }
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    let truthy = |key| {
        data.get(key)
            .is_some_and(|v| v.as_bool() == Some(true) || v.as_u64() == Some(1))
    };
    if code == 0 && !truthy("finished") && !truthy("isFinish") {
        return Ok(PhotoChunkResult::Continue);
    }
    let id = upload::file_id(data.get("photoId").ok_or(WireError::Invalid)?)?;
    let urls = if code == 1 {
        let normal = data
            .get("normalUrl")
            .and_then(Value::as_str)
            .ok_or(WireError::Invalid)?;
        let thumb = data
            .get("thumbUrl")
            .and_then(Value::as_str)
            .ok_or(WireError::Invalid)?;
        let mut urls = json!({"org":normal,"thumb":thumb,"hd":data.get("hdUrl").and_then(Value::as_str).unwrap_or(normal)});
        for field in ["thumbSize", "fileSize", "hdSize"] {
            if let Some(value) = data.get(field).filter(|value| !value.is_null()) {
                urls[field] = value.clone();
            }
        }
        Some(PhotoUrls::from_value(&urls)?)
    } else {
        None
    };
    Ok(PhotoChunkResult::Finished { photo_id: id, urls })
}
#[allow(clippy::too_many_arguments)]
pub fn message_parameters(
    target: &Target,
    photo_id: &str,
    id: u64,
    caption: &str,
    width: u32,
    height: u32,
    imei: &str,
    urls: &PhotoUrls,
) -> Result<Value, WireError> {
    upload::file_id(&json!(photo_id))?;
    if id == 0
        || caption.len() > MAX_TEXT_BYTES
        || width == 0
        || height == 0
        || width > 2560
        || height > 2560
        || imei.is_empty()
    {
        return Err(WireError::Invalid);
    }
    for url in [&urls.org, &urls.thumb, &urls.hd] {
        upload::resource_url(url)?;
    }
    let mut value = json!({"photoId":photo_id,"clientId":id,"desc":caption,"width":width,"height":height,"rawUrl":urls.org,"thumbUrl":urls.thumb,"hdUrl":urls.hd,"language":"VN","imei":imei,"ttl":0});
    match target.kind {
        ThreadKind::Direct => {
            value["toid"] = json!(target.remote_id);
            value["normalUrl"] = json!(urls.org);
        }
        ThreadKind::Group => {
            value["grid"] = json!(target.remote_id);
            value["oriUrl"] = json!(urls.org);
        }
    }
    for (key, size) in [
        ("thumbSize", urls.thumb_size),
        ("fileSize", urls.file_size),
        ("hdSize", urls.hd_size),
    ] {
        if let Some(size) = size {
            value[key] = json!(size);
        }
    }
    Ok(value)
}
pub fn media_content(
    caption: &str,
    width: u32,
    height: u32,
    urls: &PhotoUrls,
) -> Result<MediaMessageContent, WireError> {
    content_for_resolution(
        caption,
        width,
        height,
        urls,
        crate::domain::PhotoResolution::Hd,
    )
}

pub(crate) fn content_for_resolution(
    caption: &str,
    width: u32,
    height: u32,
    urls: &PhotoUrls,
    resolution: crate::domain::PhotoResolution,
) -> Result<MediaMessageContent, WireError> {
    let mut content = json!({"title":"","description":caption,"href":urls.org,"thumb":urls.thumb,
        "params":{"width":width,"height":height,"hd":urls.hd,"rawUrl":urls.org}});
    if resolution == crate::domain::PhotoResolution::Original {
        content["params"]["is_original"] = json!(1);
    }
    MediaMessageContent::new(MediaKind::Photo, &content).map_err(|_| WireError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn online_photo_resource_has_its_own_action_and_urls_not_a_file_done_alias() {
        let payload = json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"file_done","fileId":"7","data":{"url":"https://files.example.test/wrong"}}},{"content":{"act_type":"photo_success","act":"photo","fileId":"7","data":"{\"hd\":\"https://files.example.test/hd\",\"fileSize\":42}"}}]}});
        let photo = photo_success(&payload);
        assert_eq!(photo.len(), 1);
        assert_eq!(photo[0].0, "7");
        assert_eq!(photo[0].1.org, "https://files.example.test/hd");
        assert_eq!(photo[0].1.thumb, photo[0].1.org);
        assert_eq!(photo[0].1.file_size, Some(42));
        assert!(matches!(
            chunk_result(&json!({"error_code":0,"data":{"isFinish":1,"photoId":7}})).unwrap(),
            PhotoChunkResult::Finished { urls: None, .. }
        ));
        assert!(chunk_result(
            &json!({"error_code":1,"data":{"photoId":7,"normalUrl":"https://files.example.test/n"}})
        )
        .is_err());
    }
    #[test]
    fn final_photo_is_not_share_file_and_both_personal_group_have_imei_and_language() {
        let urls=PhotoUrls::from_value(&json!({"org":"https://files.example.test/n","thumb":"https://files.example.test/t","hd":"https://files.example.test/h"})).unwrap();
        for kind in [ThreadKind::Direct, ThreadKind::Group] {
            let target = Target::new(kind, "22").unwrap();
            let p =
                message_parameters(&target, "7", 123, "Chú thích", 48, 24, "imei", &urls).unwrap();
            assert_eq!(p["imei"], "imei");
            assert_eq!(p["language"], "VN");
            assert_eq!(p["ttl"], 0);
            assert_eq!(p["desc"], "Chú thích");
            assert!(p.get("fileId").is_none());
            assert!(p.get("checksum").is_none());
            assert!(p.get("visibility").is_none());
            assert_eq!(
                p[if kind == ThreadKind::Direct {
                    "normalUrl"
                } else {
                    "oriUrl"
                }],
                urls.org
            );
        }
    }
    #[test]
    fn capability_requires_the_selected_validated_service_and_honors_smaller_limits() {
        use crate::protocol::bootstrap::ServerConfig;
        let mut value = json!({"zpw_service_map_v3":{"file":["https://wpa.zaloapp.com"]},"settings":{"file_photo_limit":{"enable_new_endpoint_upload_photo":1,"resolutionLimit":{"hd":{"width":1280,"height":720,"size":1048576}}}}});
        assert!(ServerConfig::parse(&value)
            .unwrap()
            .photo_upload_route
            .is_none());
        value["zpw_service_map_v3"]["up_file"] = json!(["https://upload.zaloapp.com"]);
        let parsed = ServerConfig::parse(&value).unwrap();
        assert_eq!(parsed.photo_upload_route, Some(PhotoUploadRoute::Photo));
        assert_eq!(
            (
                parsed.photo_limits.width,
                parsed.photo_limits.height,
                parsed.photo_limits.size
            ),
            (1280, 720, 1048576)
        );
        value["settings"]["file_photo_limit"]["resolutionLimit"]["hd"]["width"] = json!(0);
        assert!(ServerConfig::parse(&value)
            .unwrap()
            .photo_upload_route
            .is_none());
    }
    #[test]
    fn malformed_optional_photo_containers_disable_only_photo_capability() {
        use crate::protocol::bootstrap::ServerConfig;
        for malformed in [
            json!(7),
            json!("opaque"),
            json!([]),
            json!({"resolutionLimit":7}),
            json!({"resolutionLimit":{"hd":"opaque"}}),
        ] {
            let value = json!({"zpw_service_map_v3":{"file":["https://wpa.zaloapp.com"],"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"]},"settings":{"file_photo_limit":malformed}});
            let config = ServerConfig::parse(&value).unwrap();
            assert!(config.photo_upload_route.is_none());
            for service in ["file", "chat", "group"] {
                assert!(config.services.contains_key(service));
            }
        }
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"file":["https://wpa.zaloapp.com"]},"settings":{"file_photo_limit":{"resolutionLimit":{"hd":{}}}}})).unwrap();
        assert_eq!(config.photo_upload_route, Some(PhotoUploadRoute::File));
    }
    #[test]
    fn source_nested_features_and_null_defaults_are_honored_without_disabling_text() {
        use crate::protocol::bootstrap::ServerConfig;
        let services = json!({"file":["https://wpa.zaloapp.com"],"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"up_file":["https://upload.zaloapp.com"]});
        for spelling in ["settings", "setttings"] {
            let mut value = json!({"zpw_service_map_v3":services});
            value[spelling] = json!({"features":{"file_photo_limit":{"enable_new_endpoint_upload_photo":1,"resolutionLimit":{"hd":{"width":1280}}},"sharefile":{"chunk_size_file":64000}}});
            let config = ServerConfig::parse(&value).unwrap();
            assert_eq!(config.photo_upload_route, Some(PhotoUploadRoute::Photo));
            assert_eq!(config.photo_limits.width, 1280);
            assert_eq!(config.file_chunk_size, 64000);
            value[spelling]["features"]["multi_site_upload"] =
                json!({"enable_multi_site_api":true});
            let config = ServerConfig::parse(&value).unwrap();
            assert!(!config.services.contains_key("file"));
            assert!(config.photo_upload_route.is_none());
            assert!(config.services.contains_key("chat"));
            value[spelling]["features"] =
                json!({"file_photo_limit":null,"multi_site_upload":null,"sharefile":null});
            let config = ServerConfig::parse(&value).unwrap();
            assert!(config.services.contains_key("file"));
            assert_eq!(config.photo_upload_route, Some(PhotoUploadRoute::File));
            assert_eq!(config.file_chunk_size, 256000);
            value[spelling]["features"]["file_photo_limit"] =
                json!({"enable_new_endpoint_upload_photo":null,"resolutionLimit":{"hd":null}});
            assert_eq!(
                ServerConfig::parse(&value).unwrap().photo_upload_route,
                Some(PhotoUploadRoute::File)
            );
        }
    }
}
