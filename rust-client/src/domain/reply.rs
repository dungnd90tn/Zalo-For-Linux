//! Scoped references for ordinary text replies; opaque source fields remain bounded.
use super::{Message, MAX_TEXT_BYTES};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_QUOTE_BYTES: usize = 128 * 1024;

pub fn valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 24
        && !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyReference {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub sender: String,
    pub text: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageQuote {
    pub owner_id: String,
    pub message_id: String,
    pub client_id: Option<String>,
    pub message_type: u64,
    pub sent_at_ms: i64,
    pub text: String,
    pub ttl: Option<u64>,
}
impl std::fmt::Debug for MessageQuote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageQuote")
            .field("text_bytes", &self.text.len())
            .finish()
    }
}
fn id(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|v| v.to_string()))?;
    valid_source_id(&text).then_some(text)
}
impl MessageQuote {
    pub fn unsupported() -> Self {
        Self {
            owner_id: String::new(),
            message_id: String::new(),
            client_id: None,
            message_type: 0,
            sent_at_ms: 0,
            text: "[Nội dung trích dẫn chưa được hỗ trợ]".into(),
            ttl: None,
        }
    }
    pub fn from_wire(account: &str, value: &Value) -> Result<Self> {
        if !value.is_object() {
            return Ok(Self::unsupported());
        }
        let raw = serde_json::to_string(value)?;
        ensure!(raw.len() <= MAX_QUOTE_BYTES, "Quote metadata exceeds limit");
        let owner = value.get("ownerId").or_else(|| value.get("fromUid"));
        let owner_id = if owner.is_some_and(|v| v.as_str() == Some("0") || v.as_u64() == Some(0)) {
            account.to_owned()
        } else {
            id(value.get("ownerId"))
                .or_else(|| id(value.get("fromUid")))
                .unwrap_or_default()
        };
        let message_id = id(value.get("globalMsgId"))
            .or_else(|| id(value.get("msgId")))
            .unwrap_or_default();
        let client_id = id(value.get("cliMsgId"));
        let message_type = value.get("cliMsgType").and_then(Value::as_u64).unwrap_or(0);
        let sent_at_ms = value
            .get("ts")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(0);
        let text = if message_type == 1 {
            value
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("[Nội dung trích dẫn chưa được hỗ trợ]")
                .to_owned()
        } else {
            "[Nội dung trích dẫn chưa được hỗ trợ]".into()
        };
        let ttl = match value.get("ttl") {
            None | Some(Value::Null) => Some(0),
            Some(value) => value.as_u64(),
        };
        let quote = Self {
            owner_id,
            message_id,
            client_id,
            message_type,
            sent_at_ms,
            text,
            ttl,
        };
        quote.validate()?;
        Ok(quote)
    }
    pub fn from_message(message: &Message) -> Result<Self> {
        ensure!(
            message.reply_reference().is_some(),
            "This message has no verified reply reference"
        );
        Self::from_wire(
            &message.account_id,
            &json!({
                "ownerId":message.sender,"globalMsgId":message.id,"cliMsgId":message.source_client_id,
                "cliMsgType":1,"ts":message.sent_at_ms,"msg":message.text,"ttl":0
            }),
        )
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (self.owner_id.is_empty() || valid_source_id(&self.owner_id))
                && (self.message_id.is_empty() || valid_source_id(&self.message_id))
                && self.client_id.as_deref().is_none_or(valid_source_id)
                && self.sent_at_ms >= 0
                && self.text.len() <= MAX_TEXT_BYTES,
            "Invalid quote metadata"
        );
        Ok(())
    }
    pub fn same_reference(&self, other: &Self) -> bool {
        self.owner_id == other.owner_id
            && self.message_id == other.message_id
            && self.client_id == other.client_id
            && self.message_type == other.message_type
            && self.sent_at_ms == other.sent_at_ms
            && self.text == other.text
            && self.ttl == other.ttl
    }
}

impl Message {
    pub fn reply_reference(&self) -> Option<ReplyReference> {
        let (kind, remote) = self.thread_id.split_once(':')?;
        if !matches!(kind, "d" | "g")
            || !valid_source_id(remote)
            || (kind == "d" && self.sender != self.account_id && self.sender != remote)
        {
            return None;
        }
        if self.recalled
            || self.reply_blocked
            || self.sticker.is_some()
            || self.media.is_some()
            || !self
                .source_client_id
                .as_deref()
                .is_some_and(valid_source_id)
            || !valid_source_id(&self.id)
            || !valid_source_id(&self.sender)
            || !valid_source_id(&self.account_id)
            || self.text.starts_with("[Tin nhắn ")
        {
            return None;
        }
        Some(ReplyReference {
            account_id: self.account_id.clone(),
            thread_id: self.thread_id.clone(),
            message_id: self.id.clone(),
            sender: self.sender.clone(),
            text: self.text.clone(),
        })
    }
}

/// Complete original quote object, restricted to protocol/storage workers.
#[derive(Clone, PartialEq, Eq)]
pub struct QuoteContent(String);
impl std::fmt::Debug for QuoteContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuoteContent")
            .field("bytes", &self.0.len())
            .finish()
    }
}
impl QuoteContent {
    pub fn new(account: &str, value: &Value) -> Result<Self> {
        let _ = account;
        let raw = serde_json::to_string(value)?;
        ensure!(raw.len() <= MAX_QUOTE_BYTES, "Quote metadata exceeds limit");
        Ok(Self(raw))
    }
    pub fn as_json(&self) -> &str {
        &self.0
    }
    pub fn projection(&self, account: &str) -> Result<MessageQuote> {
        Ok(
            MessageQuote::from_wire(account, &serde_json::from_str(&self.0)?)
                .unwrap_or_else(|_| MessageQuote::unsupported()),
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageQuoteBinding {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub content: QuoteContent,
}

/// No send was reserved: the requested original is unavailable or unsupported.
#[derive(Debug)]
pub struct ReplyUnavailable;
impl std::fmt::Display for ReplyUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply reference is unavailable")
    }
}
impl std::error::Error for ReplyUnavailable {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quote_projection_normalizes_own_sender_but_retains_complete_worker_content() {
        let value = json!({"ownerId":"0","globalMsgId":"901","cliMsgId":"1001","cliMsgType":1,"ts":"1700000000000","msg":"Nội dung gốc","extra":{"unknown":"preserved"}});
        let raw = QuoteContent::new("11", &value).unwrap();
        assert_eq!(serde_json::from_str::<Value>(raw.as_json()).unwrap(), value);
        let projected = raw.projection("11").unwrap();
        assert_eq!(projected.owner_id, "11");
        assert_eq!(projected.text, "Nội dung gốc");
        assert_eq!(projected.ttl, Some(0));
        let snapshot = serde_json::to_value(&projected).unwrap();
        assert!(snapshot.get("extra").is_none());
        assert!(snapshot.get("raw").is_none());
    }
    #[test]
    fn malformed_and_unknown_quote_content_has_an_honest_projection_without_losing_raw() {
        for value in [
            json!("opaque malformed quote"),
            json!({"msg":{"opaque":true},"cliMsgType":999}),
            json!({"msg":"not a proven text kind"}),
        ] {
            let content = QuoteContent::new("11", &value).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(content.as_json()).unwrap(),
                value
            );
            assert_eq!(
                content.projection("11").unwrap().text,
                "[Nội dung trích dẫn chưa được hỗ trợ]"
            );
        }
        assert!(QuoteContent::new("11", &json!("x".repeat(MAX_QUOTE_BYTES))).is_err());
    }
}
