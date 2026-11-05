//! Bounded rendering snapshots and complete immutable source media content.
use super::{canonical_content_json, InfoContentError};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

pub const MAX_MEDIA_DESCRIPTOR_BYTES: usize = 16 * 1024;
pub const MAX_MEDIA_URL_BYTES: usize = 4096;
pub const PHOTO_PREVIEW: &str = "[Hình ảnh]";
pub const WEBCONTENT_PREVIEW: &str = "[Tin nhắn Media]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Photo,
    WebContent,
}
impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Photo => "chat.photo",
            Self::WebContent => "chat.webcontent",
        }
    }
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "chat.photo" => Some(Self::Photo),
            "chat.webcontent" => Some(Self::WebContent),
            _ => None,
        }
    }
    pub fn legacy_placeholder(self) -> &'static str {
        match self {
            Self::Photo => "[Tin nhắn chat.photo chưa được hỗ trợ]",
            Self::WebContent => "[Tin nhắn chat.webcontent chưa được hỗ trợ]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoDescriptor {
    pub title: String,
    pub caption: String,
    pub thumbnail_url: Option<String>,
    pub original_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardRow {
    pub label: String,
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardButton {
    pub label: String,
    pub url: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebContentDescriptor {
    pub title: String,
    pub description: String,
    pub image_url: Option<String>,
    pub url: Option<String>,
    pub rows: Vec<CardRow>,
    pub buttons: Vec<CardButton>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MediaDescriptor {
    Photo(PhotoDescriptor),
    WebContent(WebContentDescriptor),
}
impl MediaDescriptor {
    pub fn kind(&self) -> MediaKind {
        match self {
            Self::Photo(_) => MediaKind::Photo,
            Self::WebContent(_) => MediaKind::WebContent,
        }
    }
    pub fn preview_text(&self) -> &str {
        match self {
            Self::Photo(_) => PHOTO_PREVIEW,
            Self::WebContent(card) => &card.title,
        }
    }
    pub fn validate(&self) -> Result<()> {
        let text = |value: &str, max: usize| {
            value.len() <= max
                && !value
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
        };
        let url = |value: &Option<String>| {
            value
                .as_ref()
                .is_none_or(|value| safe_url(value).as_ref() == Some(value))
        };
        match self {
            Self::Photo(photo) => {
                ensure!(
                    text(&photo.title, 1024)
                        && text(&photo.caption, 4096)
                        && url(&photo.thumbnail_url)
                        && url(&photo.original_url)
                        && photo.width.is_none_or(|n| n > 0 && n <= 65_535)
                        && photo.height.is_none_or(|n| n > 0 && n <= 65_535),
                    "Invalid photo descriptor"
                );
            }
            Self::WebContent(card) => {
                ensure!(
                    !card.title.is_empty()
                        && text(&card.title, 1024)
                        && text(&card.description, 4096)
                        && url(&card.image_url)
                        && url(&card.url)
                        && card.rows.len() <= 16
                        && card.buttons.len() <= 8
                        && card
                            .rows
                            .iter()
                            .all(|row| text(&row.label, 128) && text(&row.value, 512))
                        && card
                            .buttons
                            .iter()
                            .all(|button| text(&button.label, 128) && url(&button.url)),
                    "Invalid web content descriptor"
                );
            }
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_MEDIA_DESCRIPTOR_BYTES,
            "Media descriptor exceeds limit"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaContentError {
    Invalid,
    Oversize,
}
impl std::fmt::Display for MediaContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Invalid media content",
            Self::Oversize => "Media content exceeds limit",
        })
    }
}
impl std::error::Error for MediaContentError {}

#[derive(Clone, PartialEq, Eq)]
pub struct MediaMessageContent {
    kind: MediaKind,
    raw: String,
    descriptor: MediaDescriptor,
}
impl std::fmt::Debug for MediaMessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaMessageContent")
            .field("kind", &self.kind)
            .field("bytes", &self.raw.len())
            .finish()
    }
}
impl MediaMessageContent {
    pub fn new(kind: MediaKind, content: &Value) -> std::result::Result<Self, MediaContentError> {
        if !content.is_object() {
            return Err(MediaContentError::Invalid);
        }
        let raw = canonical_content_json(content).map_err(|error| match error {
            InfoContentError::Invalid => MediaContentError::Invalid,
            InfoContentError::Oversize => MediaContentError::Oversize,
        })?;
        let mut descriptor = match kind {
            MediaKind::Photo => MediaDescriptor::Photo(photo(content)),
            MediaKind::WebContent => MediaDescriptor::WebContent(web_content(content)),
        };
        fit_descriptor(&mut descriptor);
        descriptor
            .validate()
            .map_err(|_| MediaContentError::Invalid)?;
        Ok(Self {
            kind,
            raw,
            descriptor,
        })
    }
    pub fn kind(&self) -> MediaKind {
        self.kind
    }
    pub fn as_json(&self) -> &str {
        &self.raw
    }
    pub fn descriptor(&self) -> &MediaDescriptor {
        &self.descriptor
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMediaBinding {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub content: MediaMessageContent,
}

// Keep the rendering snapshot bounded even when legal raw fields contain many
// JSON-escaped characters. Full content remains immutable in the sidecar.
fn fit_descriptor(descriptor: &mut MediaDescriptor) {
    while serde_json::to_vec(descriptor).is_ok_and(|bytes| bytes.len() > MAX_MEDIA_DESCRIPTOR_BYTES)
    {
        let shortened = match descriptor {
            MediaDescriptor::Photo(photo) => {
                if !photo.caption.is_empty() {
                    photo
                        .caption
                        .truncate(photo.caption.floor_char_boundary(photo.caption.len() / 2));
                    true
                } else if !photo.title.is_empty() {
                    photo
                        .title
                        .truncate(photo.title.floor_char_boundary(photo.title.len() / 2));
                    true
                } else if photo.thumbnail_url.take().is_some() {
                    true
                } else {
                    photo.original_url.take().is_some()
                }
            }
            MediaDescriptor::WebContent(card) => {
                if !card.description.is_empty() {
                    card.description.truncate(
                        card.description
                            .floor_char_boundary(card.description.len() / 2),
                    );
                    true
                } else {
                    card.image_url.take().is_some()
                }
            }
        };
        if !shortened {
            break;
        }
    }
}

fn parsed(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Object(_) => value.cloned(),
        _ => None,
    }
}
fn bounded(value: Option<&Value>, maximum: usize) -> String {
    let Some(value) = value.and_then(Value::as_str) else {
        return String::new();
    };
    let mut result = String::new();
    for character in value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
    {
        if result.len() + character.len_utf8() > maximum {
            while result.len() + '…'.len_utf8() > maximum {
                result.pop();
            }
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}
pub fn safe_url(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > MAX_MEDIA_URL_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let parsed = Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some_and(|port| port != 80 && port != 443)
    {
        return None;
    }
    let normalized = parsed.to_string();
    (normalized.len() <= MAX_MEDIA_URL_BYTES).then_some(normalized)
}
fn url(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).and_then(safe_url)
}
fn has_encrypted_file_key(content: &Value, params: &Value) -> bool {
    // Original getFileIdData and photo-upload normalization test key truthiness
    // before attaching cryptography. Empty optional keys do not encrypt a file.
    let present = |value: Option<&Value>| match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Number(value)) => value.as_f64() != Some(0.0),
        _ => true, // Unknown nonempty marker must never trigger plaintext fetch.
    };
    ["secureKey", "e2eeKey"]
        .iter()
        .any(|key| present(content.get(key)) || present(params.get(key)))
}

fn photo(content: &Value) -> PhotoDescriptor {
    let params = parsed(content.get("params")).unwrap_or(Value::Null);
    let dimension = |name| {
        params
            .get(name)
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .filter(|value| *value > 0 && *value <= 65_535)
            .map(|value| value as u32)
    };
    // Source secureKey/e2eeKey files need a separate authenticated decryptor;
    // retaining their content does not authorize a legacy image download.
    let encrypted = has_encrypted_file_key(content, &params);
    PhotoDescriptor {
        title: bounded(content.get("title"), 1024),
        caption: bounded(content.get("description"), 4096),
        thumbnail_url: if encrypted {
            None
        } else {
            url(content.get("thumb"))
        },
        original_url: if encrypted {
            None
        } else {
            url(params.get("hd")).or_else(|| url(content.get("href")))
        },
        width: dimension("width"),
        height: dimension("height"),
    }
}
fn web_content(content: &Value) -> WebContentDescriptor {
    // The original ZInstant V1 reader parses params (JSON string or object).
    // Its customMsg is the source-proven readable notification projection;
    // item/pcItem.bundle_data belongs to remote templates, never executable here.
    let params = parsed(content.get("params")).unwrap_or(Value::Null);
    let custom = params.get("customMsg");
    let projected = custom.map(|custom| {
        super::info::presentation("", &serde_json::json!({"params":custom.to_string()}))
    });
    let title = if custom
        .and_then(|value| value.pointer("/msg/vi"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        bounded(
            projected
                .as_ref()
                .map(|text| Value::String(text.clone()))
                .as_ref(),
            1024,
        )
    } else {
        let title = bounded(content.get("title"), 1024);
        if title.is_empty() {
            WEBCONTENT_PREVIEW.into()
        } else {
            title
        }
    };
    WebContentDescriptor {
        title,
        description: bounded(content.get("description"), 4096),
        image_url: url(content.get("thumb")),
        url: None,
        rows: Vec::new(),
        buttons: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rendering_matches_original_photo_normalizer_and_zinstant_reader() {
        let oracle: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/media.json"))
                .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let content = MediaMessageContent::new(
                MediaKind::from_wire(case["kind"].as_str().unwrap()).unwrap(),
                &case["content"],
            )
            .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(content.as_json()).unwrap(),
                case["content"]
            );
            content.descriptor().validate().unwrap();
            let expected = &case["expected"];
            match content.descriptor() {
                MediaDescriptor::Photo(photo) => {
                    assert_eq!(photo.title, expected["title"]);
                    assert_eq!(photo.caption, expected["caption"]);
                    assert_eq!(
                        photo.thumbnail_url.as_deref(),
                        expected["thumbnail_url"].as_str()
                    );
                    assert_eq!(
                        photo.original_url.as_deref(),
                        expected["original_url"].as_str()
                    );
                    assert_eq!(photo.width, Some(640));
                    assert_eq!(photo.height, Some(480));
                }
                MediaDescriptor::WebContent(card) => {
                    assert_eq!(card.title, expected["title"]);
                    assert_eq!(card.description, expected["description"]);
                    assert_eq!(card.image_url.as_deref(), expected["image_url"].as_str());
                    assert!(card.url.is_none() && card.rows.is_empty() && card.buttons.is_empty());
                }
            }
        }
    }

    #[test]
    fn opaque_templates_and_actions_are_complete_readonly_content() {
        let content = json!({"params":{"item":{"bundle_data":"{\"unknownTemplate\":[1,2,3]}"},"customMsg":{"msg":{"vi":"%1$s đã gửi thông báo"},"highLightsV2":[{"type":0,"uid":"22","dpn":"Cửa hàng"}],"actions":[{"actionType":"action.open.outapp","actionData":"javascript:never_run"}]}},"href":"javascript:never_run"});
        let media = MediaMessageContent::new(MediaKind::WebContent, &content).unwrap();
        assert_eq!(
            media.descriptor().preview_text(),
            "Cửa hàng đã gửi thông báo"
        );
        assert_eq!(
            serde_json::from_str::<Value>(media.as_json()).unwrap(),
            content
        );
        let MediaDescriptor::WebContent(card) = media.descriptor() else {
            panic!()
        };
        assert!(card.url.is_none() && card.buttons.is_empty());
        let opaque = MediaMessageContent::new(
            MediaKind::WebContent,
            &json!({"params":{"item":{"bundle_data":"opaque"}}}),
        )
        .unwrap();
        assert_eq!(opaque.descriptor().preview_text(), WEBCONTENT_PREVIEW);
        assert!(!format!("{media:?}").contains("javascript"));
    }

    #[test]
    fn expanded_unicode_urls_are_withheld_without_rejecting_complete_media() {
        let address = format!("https://f12-zpcloud.zdn.vn/{}", "界".repeat(1000));
        assert!(address.len() <= MAX_MEDIA_URL_BYTES);
        assert!(Url::parse(&address).unwrap().as_str().len() > MAX_MEDIA_URL_BYTES);
        assert!(safe_url(&address).is_none());
        let raw = json!({"title":"Thông báo", "description":"Nội dung vẫn đọc được", "href":address, "thumb":address});
        for kind in [MediaKind::Photo, MediaKind::WebContent] {
            let content = MediaMessageContent::new(kind, &raw).unwrap();
            content.descriptor().validate().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(content.as_json()).unwrap(),
                raw
            );
            match content.descriptor() {
                MediaDescriptor::Photo(photo) => {
                    assert!(photo.original_url.is_none() && photo.thumbnail_url.is_none())
                }
                MediaDescriptor::WebContent(card) => {
                    assert!(card.image_url.is_none() && card.title == "Thông báo")
                }
            }
        }
    }

    #[test]
    fn source_empty_secure_keys_leave_ordinary_photos_usable() {
        for field in ["secureKey", "e2eeKey"] {
            for marker in [Value::Null, json!(""), json!(false), json!(0)] {
                let content = json!({"href":"https://f12-zpcloud.zdn.vn/photo.jpg", "thumb":"https://f12-zpcloud.zdn.vn/thumb.jpg",field:marker});
                let media = MediaMessageContent::new(MediaKind::Photo, &content).unwrap();
                let MediaDescriptor::Photo(photo) = media.descriptor() else {
                    panic!()
                };
                assert!(photo.original_url.is_some() && photo.thumbnail_url.is_some());
            }
            for marker in [
                json!("synthetic-key"),
                json!({"unknown":"key-format"}),
                json!(true),
                json!([]),
            ] {
                let content = json!({"href":"https://f12-zpcloud.zdn.vn/photo.jpg", "thumb":"https://f12-zpcloud.zdn.vn/thumb.jpg", "params":json!({field:marker}).to_string()});
                let media = MediaMessageContent::new(MediaKind::Photo, &content).unwrap();
                let MediaDescriptor::Photo(photo) = media.descriptor() else {
                    panic!()
                };
                assert!(photo.original_url.is_none() && photo.thumbnail_url.is_none());
                assert_eq!(
                    serde_json::from_str::<Value>(media.as_json()).unwrap(),
                    content
                );
            }
        }
    }

    #[test]
    fn raw_limits_url_validation_and_snapshot_work_are_bounded() {
        for value in [
            "javascript:alert(1)",
            "file:///tmp/private",
            "https://user:secret@host.test/image",
            "https://host.test:444/image",
            " https://host.test/image",
            "https://host.test/image\n",
        ] {
            assert!(safe_url(value).is_none());
        }
        let content = json!({"title":"á\"".repeat(2048),"description":"\\\"".repeat(16384),"thumb":format!("https://f12-zpcloud.zdn.vn/{}", "\\".repeat(3900)),"href":"https://f12-zpcloud.zdn.vn/image.jpg","unknown":"retain"});
        let media = MediaMessageContent::new(MediaKind::Photo, &content).unwrap();
        assert!(
            serde_json::to_vec(media.descriptor()).unwrap().len() <= MAX_MEDIA_DESCRIPTOR_BYTES
        );
        assert_eq!(
            serde_json::from_str::<Value>(media.as_json()).unwrap(),
            content
        );
        assert_eq!(
            MediaMessageContent::new(
                MediaKind::Photo,
                &json!({"unknown":"x".repeat(crate::domain::MAX_INFO_CONTENT_BYTES)})
            )
            .unwrap_err(),
            MediaContentError::Oversize
        );
        assert_eq!(
            MediaMessageContent::new(MediaKind::Photo, &Value::Null).unwrap_err(),
            MediaContentError::Invalid
        );
        let mut nested = json!({});
        for _ in 0..crate::domain::MAX_INFO_DEPTH {
            nested = json!({"next":nested});
        }
        assert_eq!(
            MediaMessageContent::new(MediaKind::WebContent, &nested).unwrap_err(),
            MediaContentError::Oversize
        );
    }
}
