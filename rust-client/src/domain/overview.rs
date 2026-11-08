//! Read-only summary of what a conversation's saved history contains: counts, and the most recent
//! pictures, files and links. It is derived from stored rows only and never from the network.

/// How many recent items of each kind a summary carries.
pub const OVERVIEW_ITEM_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedKind {
    Image,
    File,
    Link,
}

/// One picture, file or link that appeared in the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedItem {
    pub message_id: String,
    pub sender: String,
    pub sent_at_ms: i64,
    pub kind: SharedKind,
    /// Caption, file name or link title; may be empty.
    pub title: String,
    /// File extension without the dot; empty for pictures and links.
    pub extension: String,
    pub size_bytes: Option<u64>,
    /// Host of the link, empty for pictures and files.
    pub host: String,
    /// Validated http(s) address that opens only when the user asks.
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationOverview {
    pub total_messages: u64,
    pub first_at_ms: Option<i64>,
    pub last_at_ms: Option<i64>,
    pub sticker_count: u64,
    pub image_count: u64,
    pub file_count: u64,
    pub link_count: u64,
    /// Newest first, at most [`OVERVIEW_ITEM_LIMIT`] each.
    pub images: Vec<SharedItem>,
    pub files: Vec<SharedItem>,
    pub links: Vec<SharedItem>,
}
