//! Validated values shared by storage, background workers, and presentation.
use anyhow::{bail, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

pub(crate) mod info;
pub mod media;
pub mod overview;
pub use media::{
    CardButton, CardRow, FileDescriptor, MediaContentError, MediaDescriptor, MediaKind,
    MediaMessageContent, MessageMediaBinding, PhotoDescriptor, PollDescriptor,
    WebContentDescriptor, MAX_MEDIA_DESCRIPTOR_BYTES,
};
pub use overview::{ConversationOverview, SharedItem, SharedKind, OVERVIEW_ITEM_LIMIT};

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_PAGE_SIZE: usize = 200;
pub const MAX_PREVIEW_CHARS: usize = 256;
pub const MAX_STICKER_ID: u64 = 9_007_199_254_740_991;
pub const LEGACY_STICKER_PLACEHOLDER: &str = "[Tin nhắn chat.sticker chưa được hỗ trợ]";
pub const STICKER_PREVIEW: &str = "[Nhãn dán]";
pub const MAX_INFO_CONTENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_INFO_BATCH_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_INFO_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoContentError {
    Invalid,
    Oversize,
}
impl std::fmt::Display for InfoContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Invalid info message content",
            Self::Oversize => "Info message content exceeds limit",
        })
    }
}
impl std::error::Error for InfoContentError {}

/// Complete source-backed webchat info content, kept off the rendering thread.
/// Unknown fields and the original nested `params` string are retained. The
/// presentation text lives separately in Message and cannot replace this data.
#[derive(Clone, PartialEq, Eq)]
pub struct InfoMessageContent(String);

/// Full scope accompanies metadata across preview pagination/chunking so that
/// it cannot be accidentally attached to a different message or account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInfoBinding {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub content: InfoMessageContent,
}

impl std::fmt::Debug for InfoMessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InfoMessageContent")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl InfoMessageContent {
    pub fn new(content: &serde_json::Value) -> std::result::Result<Self, InfoContentError> {
        if !content.is_object()
            || content.get("action").and_then(serde_json::Value::as_str)
                != Some("msginfo.actionlist")
        {
            return Err(InfoContentError::Invalid);
        }
        canonical_content_json(content).map(Self)
    }

    pub fn as_json(&self) -> &str {
        &self.0
    }
}

fn canonical_content_json(
    content: &serde_json::Value,
) -> std::result::Result<String, InfoContentError> {
    fn check_depth(
        value: &serde_json::Value,
        depth: usize,
    ) -> std::result::Result<(), InfoContentError> {
        // serde_json starts with 128 and refuses the container that would
        // decrement it to zero. Scalars do not consume recursion depth.
        if depth >= MAX_INFO_DEPTH - 1 && (value.is_array() || value.is_object()) {
            return Err(InfoContentError::Oversize);
        }
        match value {
            serde_json::Value::Array(values) => {
                for value in values {
                    check_depth(value, depth + 1)?;
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values() {
                    check_depth(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    check_depth(content, 0)?;
    struct BoundedWriter(Vec<u8>);
    impl std::io::Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_INFO_CONTENT_BYTES.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("Info message exceeds limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = BoundedWriter(Vec::new());
    serde_json::to_writer(&mut output, content).map_err(|_| InfoContentError::Oversize)?;
    String::from_utf8(output.0).map_err(|_| InfoContentError::Invalid)
}

/// Plain-text composer sends use ECMAScript String.trim in the original PC
/// client (_sendPlainText). This deliberately differs from Rust's Unicode trim:
/// JavaScript trims BOM but does not trim NEXT LINE (U+0085).
pub fn canonical_plain_text(text: &str) -> &str {
    text.trim_matches(|character| {
        matches!(character,
            '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
            '\u{205f}' | '\u{3000}' | '\u{feff}')
    })
}

/// Source-backed chat.sticker catalog address. Media URLs come from a separate
/// validated catalog request, never from arbitrary incoming message payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickerDescriptor {
    pub sticker_id: u64,
    pub category_id: u64,
    pub sticker_type: u32,
}
impl StickerDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.sticker_id > MAX_STICKER_ID
            || self.category_id > MAX_STICKER_ID
            || self.sticker_type > 255
        {
            bail!("Invalid sticker descriptor")
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub account_id: String,
    pub thread_id: String,
    pub id: String,
    pub sender: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker: Option<StickerDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaDescriptor>,
    pub sent_at_ms: i64,
}

impl Message {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.account_id)?;
        validate_id(&self.thread_id)?;
        validate_id(&self.id)?;
        validate_id(&self.sender)?;
        if let Some(media) = &self.media {
            media.validate()?;
            if self.sticker.is_some() || self.text != media.kind().legacy_placeholder() {
                bail!("Media metadata requires its source fallback text")
            }
        }
        if let Some(sticker) = &self.sticker {
            sticker.validate()?;
            if self.text != LEGACY_STICKER_PLACEHOLDER {
                bail!("Sticker metadata requires its source fallback text")
            }
        }
        if self.sender.len() > 256 || self.text.len() > MAX_TEXT_BYTES || self.sent_at_ms < 0 {
            bail!("Invalid message size or timestamp");
        }
        Ok(())
    }

    pub fn preview_text(&self) -> &str {
        if let Some(media) = &self.media {
            media.preview_text()
        } else if self.sticker.is_some() {
            STICKER_PREVIEW
        } else {
            &self.text
        }
    }

    pub fn cursor(&self) -> Cursor {
        Cursor {
            sent_at_ms: self.sent_at_ms,
            id: self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub sent_at_ms: i64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub last_message_at_ms: i64,
    /// Actual latest message, limited to Unicode characters for list snapshots.
    pub last_text: String,
    pub last_sender: String,
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        bail!("IDs must contain 1–256 bytes and no control characters");
    }
    Ok(())
}

pub fn local_message_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| anyhow::anyhow!("Cannot create local message ID: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Literal Vietnamese accent/case-insensitive search. Persisted on metadata
/// writes; querying never loads the directory onto the rendering thread.
pub fn fold_search(text: &str) -> String {
    const GROUPS: [(&str, char); 7] = [
        ("àáảãạăằắẳẵặâầấẩẫậ", 'a'),
        ("èéẻẽẹêềếểễệ", 'e'),
        ("ìíỉĩị", 'i'),
        ("òóỏõọôồốổỗộơờớởỡợ", 'o'),
        ("ùúủũụưừứửữự", 'u'),
        ("ỳýỷỹỵ", 'y'),
        ("đ", 'd'),
    ];
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ('\u{0300}'..='\u{036f}').contains(&ch) {
                return None;
            }
            Some(
                GROUPS
                    .iter()
                    .find_map(|(letters, base)| letters.contains(ch).then_some(*base))
                    .unwrap_or(ch),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_text_boundary_normalization_matches_javascript_without_changing_inner_content() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/messaging/plain_text.json"))
                .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            assert_eq!(
                canonical_plain_text(case["input"].as_str().unwrap()),
                case["expected"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
        assert_eq!(canonical_plain_text("\u{feff} \tchào\r\n \u{00a0}"), "chào");
        for character in ['\u{0085}', '\u{180e}', '\u{200b}', '\u{2060}'] {
            let text = format!("{character}chào{character}");
            assert_eq!(canonical_plain_text(&text), text);
        }
        assert_eq!(canonical_plain_text(" a  b\n c \t"), "a  b\n c");
        assert_eq!(canonical_plain_text("  cha\u{0300}o  "), "cha\u{0300}o");
    }
    #[test]
    fn rejects_oversized_text_and_invalid_ids() {
        let mut m = Message {
            account_id: "a".into(),
            thread_id: "t".into(),
            id: "m".into(),
            sender: "s".into(),
            text: "hello".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        };
        assert!(m.validate().is_ok());
        m.text = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(m.validate().is_err());
        assert!(validate_id("\n").is_err());
        assert!(validate_id("").is_err());
    }
    #[test]
    fn legacy_message_json_remains_compatible_and_sticker_schema_is_closed() {
        let old = r#"{"account_id":"11","thread_id":"d:22","id":"1","sender":"22","text":"hello","sent_at_ms":1}"#;
        let message: Message = serde_json::from_str(old).unwrap();
        assert!(message.sticker.is_none());
        assert!(!serde_json::to_string(&message).unwrap().contains("sticker"));
        assert_eq!(message.preview_text(), "hello");
        let mut hidden_text = message.clone();
        hidden_text.sticker = Some(StickerDescriptor {
            sticker_id: 1,
            category_id: 2,
            sticker_type: 3,
        });
        assert!(hidden_text.validate().is_err());
        assert!(serde_json::from_str::<StickerDescriptor>(
            r#"{"sticker_id":1,"category_id":2,"sticker_type":3,"url":"https://invalid.example"}"#
        )
        .is_err());
        assert!(StickerDescriptor {
            sticker_id: MAX_STICKER_ID,
            category_id: 0,
            sticker_type: 255
        }
        .validate()
        .is_ok());
    }
}
