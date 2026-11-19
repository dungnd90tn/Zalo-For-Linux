//! Payload-free, account-scoped read and delivery projections.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptReference {
    pub message_id: String,
    pub client_id: Option<String>,
    pub sender_id: String,
    pub recipient_id: String,
    pub message_type: String,
    pub st: i64,
    pub at: i64,
    pub command: i64,
    pub sent_at_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptBinding {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub reference: ReceiptReference,
    pub counts_unread: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveContext {
    LiveFresh,
    OfflineSync,
    History,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadReadState {
    pub thread_id: String,
    pub unread: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationPreferences {
    pub enabled: bool,
    pub preview: bool,
}
impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            preview: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeliveryState {
    Accepted,
    Delivered,
    Seen,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDelivery {
    pub message_id: String,
    pub state: DeliveryState,
    pub seen_count: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptUpdate {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub client_id: Option<String>,
    pub reader_id: Option<String>,
    pub state: DeliveryState,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerReadWatermark {
    pub thread_id: String,
    pub last_message_id: String,
}
