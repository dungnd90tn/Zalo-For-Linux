//! Validated values shared by storage, background workers, and presentation.
use anyhow::{bail, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_PAGE_SIZE: usize = 200;
pub const MAX_PREVIEW_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub account_id: String,
    pub thread_id: String,
    pub id: String,
    pub sender: String,
    pub text: String,
    pub sent_at_ms: i64,
}

impl Message {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.account_id)?;
        validate_id(&self.thread_id)?;
        validate_id(&self.id)?;
        validate_id(&self.sender)?;
        if self.sender.len() > 256 || self.text.len() > MAX_TEXT_BYTES || self.sent_at_ms < 0 {
            bail!("Invalid message size or timestamp");
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversized_text_and_invalid_ids() {
        let mut m = Message {
            account_id: "a".into(),
            thread_id: "t".into(),
            id: "m".into(),
            sender: "s".into(),
            text: "hello".into(),
            sent_at_ms: 1,
        };
        assert!(m.validate().is_ok());
        m.text = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(m.validate().is_err());
        assert!(validate_id("\n").is_err());
        assert!(validate_id("").is_err());
    }
}
