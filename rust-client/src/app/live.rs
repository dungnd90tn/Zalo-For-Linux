//! Live account worker: bounded commands/events, no rendering-thread I/O.
use super::SubmitError;
pub use crate::app::directory::{DirectoryFailure, DirectoryReport};
pub use crate::protocol::group_roster::{Page as GroupMemberPage, Role as GroupMemberRole};
pub use crate::protocol::messaging::ThreadKind;
pub use crate::store::directory::{
    AvatarIdentity, DirectoryCursor, DirectoryRecord, SenderName, MAX_AVATARS, MAX_SENDER_NAMES,
};
pub use crate::store::{GroupActionOutcome, GroupInviteTarget};
use crate::{
    domain::{Conversation, Cursor, Message, MAX_TEXT_BYTES},
    profile::Profile,
    protocol::session::VerifiedSession,
};
use anyhow::Result;
use std::{sync::Arc, thread::JoinHandle};
use tokio::sync::{mpsc, watch};
mod photo;
pub use crate::domain::PhotoResolution;
pub use photo::{PhotoPreparationFailure, PhotoThumbnail, PreparedPhoto};
pub(crate) const MAX_ORIGINAL_PHOTO_BYTES: u64 = crate::store::photos::MAX_PHOTO_BYTES;
pub const MAX_ALBUM_PHOTOS: usize = crate::store::photos::MAX_ALBUM_PHOTOS;
pub(crate) mod runtime;
#[cfg(feature = "signal-crypto")]
pub mod signal_crypto;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveConnection {
    Connecting,
    Online,
    Reconnecting,
    Offline,
    Expired,
}
/// Fixed diagnostic fields; transport URLs, headers and response bodies never enter UI events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStage {
    Configuration,
    Connect,
    Authentication,
    Send,
    Receive,
    Synchronization,
    Heartbeat,
    Storage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCategory {
    Timeout,
    Network,
    Tls,
    Http,
    Protocol,
    Crypto,
    Unsupported,
    Authentication,
    Server,
    Closed,
    Resource,
    Storage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveDiagnostic {
    pub stage: LiveStage,
    pub category: LiveCategory,
    pub http_status: Option<u16>,
    pub server_code: Option<i64>,
    pub parser_reason: Option<crate::protocol::messaging::FrameReason>,
}
impl LiveDiagnostic {
    pub fn safe_message(&self) -> &'static str {
        match self.category {
            LiveCategory::Timeout => "Quá thời gian chờ máy chủ nhắn tin.",
            LiveCategory::Network => "Không kết nối được máy chủ nhắn tin.",
            LiveCategory::Tls => "Không xác minh được kết nối TLS của máy chủ nhắn tin.",
            LiveCategory::Http => "Máy chủ từ chối nâng cấp kết nối WebSocket.",
            LiveCategory::Protocol => "Định dạng kết nối nhắn tin chưa được hỗ trợ.",
            LiveCategory::Crypto => "Không xác minh được dữ liệu mã hóa của kết nối nhắn tin.",
            LiveCategory::Unsupported => "Máy chủ yêu cầu tính năng nhắn tin chưa được hỗ trợ.",
            LiveCategory::Authentication => "Máy chủ từ chối phiên nhắn tin. Cần đăng nhập lại.",
            LiveCategory::Server => "Máy chủ trả về lỗi nhắn tin.",
            LiveCategory::Closed => "Máy chủ đã đóng kết nối nhắn tin.",
            LiveCategory::Resource => "Dữ liệu nhắn tin vượt giới hạn xử lý an toàn.",
            LiveCategory::Storage => "Không lưu được lịch sử nhắn tin trên thiết bị.",
        }
    }
}
impl std::fmt::Display for LiveDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{:?}/{:?}",
            self.safe_message(),
            self.stage,
            self.category
        )?;
        if let Some(status) = self.http_status {
            write!(f, "; HTTP {status}")?;
        }
        if let Some(code) = self.server_code {
            write!(f, "; code {code}")?;
        }
        if let Some(reason) = self.parser_reason {
            write!(f, "; parser {reason:?}")?;
        }
        f.write_str("]")
    }
}
/// Public presentation state; the remote PIN verifier never crosses this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePrivacyStatus {
    Checking,
    Ready,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenUnlockOutcome {
    Unlocked,
    IncorrectPin,
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenCandidate {
    pub thread_id: String,
    pub title: String,
}
/// Four existing Zalo PIN digits. Debug output is deliberately redacted.
pub struct HiddenPin([u8; 4]);
impl HiddenPin {
    pub fn new(value: &str) -> Option<Self> {
        if value.len() != 4 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut digits = [0; 4];
        digits.copy_from_slice(value.as_bytes());
        Some(Self(digits))
    }
    pub(crate) fn digest(&self) -> [u8; 16] {
        use md5::{Digest, Md5};
        Md5::digest(self.0).into()
    }
}
impl std::fmt::Debug for HiddenPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HiddenPin([REDACTED])")
    }
}
impl Drop for HiddenPin {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

use crate::domain::read_state::{MessageDelivery, NotificationPreferences, ThreadReadState};
pub use crate::protocol::sticker_catalog::StickerPack;
pub use crate::store::forward::ForwardTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardOutcome {
    Accepted,
    Rejected,
    Unavailable,
    Unknown,
}

pub const MAX_FORWARD_BATCH: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardBatchResult {
    pub target_thread: String,
    pub outcome: ForwardOutcome,
}

fn valid_forward_targets(targets: &[String], account: &str) -> bool {
    use crate::protocol::messaging::Target;
    if !(1..=MAX_FORWARD_BATCH).contains(&targets.len()) {
        return false;
    }
    let mut unique = std::collections::HashSet::new();
    targets.iter().all(|thread| {
        unique.insert(thread)
            && Target::from_thread(thread).is_ok_and(|target| {
                target.kind == ThreadKind::Direct && target.remote_id != account
            })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallOutcome {
    Accepted,
    Rejected,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteHistoryOutcome {
    Imported { count: usize, has_more: bool },
    Exhausted,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickerOutcome {
    Accepted,
    Rejected,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPinOutcome {
    Accepted,
    Rejected,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPinEntry {
    pub message_id: String,
    pub hit: Option<crate::domain::search::MessageHit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPinSnapshot {
    pub can_mutate: bool,
    pub entries: Vec<GroupPinEntry>,
    pub unresolved: Vec<String>,
}

pub use crate::protocol::reactions::{
    ReactionAction, ReactionKind, ReactionOutcome, ReactionSummary,
};

#[derive(Debug)]
pub enum LiveCommand {
    React {
        request_id: u64,
        thread: String,
        message_id: String,
        action: ReactionAction,
    },
    ReactionState {
        generation: u64,
        thread: String,
        message_ids: Vec<String>,
    },
    GroupMembers {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        after: Option<String>,
    },
    GroupPins {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
    },
    SetGroupPin {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        message_id: String,
        pinned: bool,
    },
    CancelGroupMembers {
        request_id: u64,
    },
    GroupInviteTargets {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        query: String,
    },
    GroupInvite {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        member: String,
    },
    GroupLeave {
        request_id: u64,
        thread: String,
        privacy_revision: u64,
    },
    Recall {
        request_id: u64,
        thread: String,
        message_id: String,
    },
    LoadRemoteHistory {
        request_id: u64,
        generation: u64,
        thread: String,
        anchor: Cursor,
        privacy_revision: u64,
    },
    Forward {
        request_id: u64,
        source_thread: String,
        source_message_id: String,
        target_thread: String,
    },
    ForwardBatch {
        request_id: u64,
        source_thread: String,
        source_message_id: String,
        target_threads: Vec<String>,
    },
    ForwardTargets {
        generation: u64,
        source_thread: String,
        source_message_id: String,
        query: String,
    },
    SendSticker {
        request_id: u64,
        thread: String,
        sticker: crate::domain::StickerDescriptor,
    },
    SearchStickers {
        request_id: u64,
        thread: String,
        query: String,
    },
    ListStickerPacks {
        request_id: u64,
        thread: String,
    },
    BrowseStickerPack {
        request_id: u64,
        thread: String,
        pack_list_id: u64,
        category_id: u64,
    },
    SendCatalogSticker {
        request_id: u64,
        search_id: u64,
        thread: String,
        sticker: crate::domain::StickerDescriptor,
    },
    ReadVisible {
        generation: u64,
        thread: String,
        message_ids: Vec<String>,
    },
    ReadState {
        generation: u64,
        threads: Vec<String>,
        thread: Option<String>,
        message_ids: Vec<String>,
    },
    NotificationForeground {
        thread: Option<String>,
    },
    SetNotificationPreferences {
        preferences: NotificationPreferences,
    },
    ResolveAvatars {
        generation: u64,
        thread_ids: Vec<String>,
    },
    RefreshPrivacy,
    SearchHidden {
        generation: u64,
        query: String,
    },
    UnlockHidden {
        generation: u64,
        revision: u64,
        thread: String,
        pin: HiddenPin,
    },
    RelockHidden,
    Reconnect,
    RefreshMessages {
        generation: u64,
        thread: String,
        message_ids: Vec<String>,
    },
    ResolveSenders {
        generation: u64,
        thread: String,
        sender_ids: Vec<String>,
    },
    Directory {
        generation: u64,
        is_group: bool,
        query: String,
        after: Option<DirectoryCursor>,
    },
    RefreshDirectory {
        generation: u64,
    },
    Open {
        generation: u64,
        kind: ThreadKind,
        remote_id: String,
    },
    Load {
        generation: u64,
        thread: String,
        before: Option<Cursor>,
    },
    /// Counts and the newest pictures, files and links of the saved history.
    Overview {
        generation: u64,
        thread: String,
    },
    /// Saved messages of one conversation containing `query`; never asks the server.
    Search {
        request: u64,
        thread: String,
        query: String,
        before: Option<Cursor>,
    },
    Send {
        generation: u64,
        thread: String,
        text: String,
        reply_to: Option<String>,
        request_id: u64,
    },
    SendMentions {
        generation: u64,
        thread: String,
        text: String,
        mentions: Vec<crate::domain::mention::Mention>,
        privacy_revision: u64,
        request_id: u64,
    },
    SendFile {
        generation: u64,
        thread: String,
        attachment: crate::domain::Attachment,
        request_id: u64,
    },
    CancelFile {
        request_id: u64,
    },
    CancelAlbum {
        thread: String,
        request_id: u64,
    },
    PreparePhoto {
        generation: u64,
        thread: String,
        attachment: crate::domain::Attachment,
        resolution: PhotoResolution,
        request_id: u64,
    },
    PrepareAlbum {
        generation: u64,
        thread: String,
        attachments: Vec<crate::domain::Attachment>,
        request_id: u64,
    },
    CancelPhotoPreparation {
        request_id: u64,
    },
    CancelAlbumPreparation {
        thread: String,
        request_id: u64,
    },
    SendPhoto {
        generation: u64,
        thread: String,
        photo: PreparedPhoto,
        caption: String,
        request_id: u64,
    },
    SendAlbum {
        generation: u64,
        thread: String,
        photos: Vec<PreparedPhoto>,
        request_id: u64,
    },
    Shutdown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    File,
    Photo,
    Album,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTransferPhase {
    Preparing,
    Uploading,
    WaitingForResource,
    Finalizing,
    Accepted,
    Cancelled,
    Failed,
    Unknown,
    AcceptedLocalFailure,
    Partial,
}
#[derive(Debug)]
pub enum LiveEvent {
    GroupPins {
        account_id: String,
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        snapshot: Option<GroupPinSnapshot>,
    },
    GroupPinFinished {
        account_id: String,
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        message_id: String,
        outcome: GroupPinOutcome,
        snapshot: Option<GroupPinSnapshot>,
    },
    ReactionCapability {
        account_id: String,
        available: bool,
    },
    ReactionFinished {
        account_id: String,
        request_id: u64,
        thread: String,
        message_id: String,
        outcome: ReactionOutcome,
    },
    ReactionState {
        account_id: String,
        generation: u64,
        thread: String,
        messages: Vec<(String, Vec<ReactionSummary>)>,
    },
    ReactionsChanged {
        account_id: String,
        thread: String,
    },
    GroupMembers {
        account_id: String,
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        page: Option<GroupMemberPage>,
    },
    GroupInviteTargets {
        account_id: String,
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        targets: Option<Vec<GroupInviteTarget>>,
    },
    GroupActionFinished {
        account_id: String,
        request_id: u64,
        thread: String,
        privacy_revision: u64,
        member: Option<String>,
        outcome: GroupActionOutcome,
    },
    RecallFinished {
        account_id: String,
        request_id: u64,
        thread: String,
        message_id: String,
        outcome: RecallOutcome,
    },
    RemoteHistoryCapability {
        account_id: String,
        available: bool,
    },
    RemoteHistoryFinished {
        account_id: String,
        request_id: u64,
        generation: u64,
        thread: String,
        privacy_revision: u64,
        outcome: RemoteHistoryOutcome,
    },
    Recalled {
        account_id: String,
        thread: String,
        message_id: String,
        actor_id: String,
        message: Option<Message>,
        conversation: Option<Conversation>,
    },
    ForwardFinished {
        account_id: String,
        request_id: u64,
        source_thread: String,
        source_message_id: String,
        target_thread: String,
        outcome: ForwardOutcome,
    },
    ForwardBatchFinished {
        account_id: String,
        request_id: u64,
        source_thread: String,
        source_message_id: String,
        results: Vec<ForwardBatchResult>,
    },
    ForwardTargets {
        account_id: String,
        generation: u64,
        source_thread: String,
        source_message_id: String,
        query: String,
        entries: Vec<ForwardTarget>,
        failed: bool,
    },
    StickerFinished {
        account_id: String,
        request_id: u64,
        thread: String,
        outcome: StickerOutcome,
    },
    StickerSearchCapability {
        account_id: String,
        available: bool,
    },
    StickersFound {
        account_id: String,
        request_id: u64,
        thread: String,
        stickers: Option<Vec<crate::domain::StickerDescriptor>>,
    },
    StickerPacksFound {
        account_id: String,
        request_id: u64,
        thread: String,
        packs: Option<Vec<StickerPack>>,
    },
    StickerPackFound {
        account_id: String,
        request_id: u64,
        thread: String,
        category_id: u64,
        stickers: Option<Vec<crate::domain::StickerDescriptor>>,
    },
    ReadState {
        account_id: String,
        generation: u64,
        unread: Vec<ThreadReadState>,
        thread: Option<String>,
        delivery: Vec<MessageDelivery>,
    },
    ReadStateChanged {
        account_id: String,
    },
    NotificationPreferences {
        account_id: String,
        preferences: Option<NotificationPreferences>,
    },
    NotificationOpen {
        account_id: String,
        thread: String,
    },
    PhotoCapability {
        account_id: String,
        available: bool,
        original_available: bool,
        album_max_count: usize,
    },
    PhotoPrepared {
        account_id: String,
        generation: u64,
        thread: String,
        request_id: u64,
        result: std::result::Result<PreparedPhoto, PhotoPreparationFailure>,
    },
    AlbumPrepared {
        account_id: String,
        generation: u64,
        thread: String,
        request_id: u64,
        result: std::result::Result<Vec<PreparedPhoto>, PhotoPreparationFailure>,
    },
    FileCapability {
        account_id: String,
        available: bool,
    },
    FileTransfer {
        kind: TransferKind,
        account_id: String,
        thread: String,
        request_id: u64,
        phase: FileTransferPhase,
        uploaded: u64,
        total: u64,
        message: Option<String>,
    },
    FileAccepted {
        kind: TransferKind,
        account_id: String,
        thread: String,
        request_id: u64,
        message: Message,
        conversation: Conversation,
    },
    Avatars {
        account_id: String,
        generation: u64,
        avatars: Vec<AvatarIdentity>,
    },
    PrivacyChanged {
        account_id: String,
        revision: u64,
        status: LivePrivacyStatus,
        hidden_threads: Vec<String>,
        unlocked_thread: Option<String>,
    },
    HiddenMatches {
        account_id: String,
        generation: u64,
        revision: u64,
        query: String,
        candidates: Vec<HiddenCandidate>,
    },
    HiddenUnlockResult {
        account_id: String,
        generation: u64,
        revision: u64,
        thread: String,
        outcome: HiddenUnlockOutcome,
    },
    /// Content-free durable acknowledgement for a draft whose view was locked meanwhile.
    SendSettled {
        account_id: String,
        thread: String,
        request_id: u64,
        accepted: bool,
    },
    MessagesRefreshed {
        account_id: String,
        generation: u64,
        thread: String,
        messages: Vec<Message>,
    },
    MessagesRefreshFailed {
        account_id: String,
        generation: u64,
        thread: String,
    },
    SenderNames {
        account_id: String,
        generation: u64,
        thread: String,
        names: Vec<SenderName>,
    },
    DirectoryPage {
        account_id: String,
        generation: u64,
        is_group: bool,
        query: String,
        entries: Vec<DirectoryRecord>,
        next: Option<DirectoryCursor>,
    },
    DirectoryStatus {
        account_id: String,
        generation: u64,
        syncing: bool,
        report: DirectoryReport,
    },
    DirectoryChanged {
        account_id: String,
        conversations: Vec<Conversation>,
    },
    DirectoryFailed {
        account_id: String,
        generation: u64,
        error: DirectoryFailure,
    },
    Diagnostic {
        account_id: String,
        diagnostic: LiveDiagnostic,
    },
    Ready {
        account_id: String,
        conversations: Vec<Conversation>,
    },
    Opened {
        account_id: String,
        generation: u64,
        conversation: Conversation,
    },
    Page {
        account_id: String,
        generation: u64,
        thread: String,
        messages: Vec<Message>,
        has_older: bool,
    },
    Overview {
        account_id: String,
        generation: u64,
        thread: String,
        overview: Box<crate::domain::ConversationOverview>,
    },
    OverviewFailed {
        account_id: String,
        generation: u64,
        thread: String,
    },
    SearchResults {
        account_id: String,
        request: u64,
        thread: String,
        search: Box<crate::domain::MessageSearch>,
    },
    SearchFailed {
        account_id: String,
        request: u64,
        thread: String,
    },
    Connection {
        account_id: String,
        state: LiveConnection,
    },
    Received {
        account_id: String,
        message: Message,
        conversation: Conversation,
    },
    Sent {
        account_id: String,
        generation: u64,
        thread: String,
        request_id: u64,
        message: Message,
        conversation: Conversation,
    },
    SendFailed {
        account_id: String,
        generation: u64,
        thread: String,
        request_id: u64,
        error: String,
        unknown_delivery: bool,
        server_accepted: bool,
    },
    RequestFailed {
        account_id: String,
        generation: u64,
        thread: String,
        error: String,
    },
    Fault {
        account_id: String,
        error: String,
    },
}
impl LiveEvent {
    pub fn account_id(&self) -> &str {
        match self {
            Self::ReactionCapability { account_id, .. }
            | Self::ReactionFinished { account_id, .. }
            | Self::ReactionState { account_id, .. }
            | Self::ReactionsChanged { account_id, .. }
            | Self::GroupMembers { account_id, .. }
            | Self::GroupInviteTargets { account_id, .. }
            | Self::GroupActionFinished { account_id, .. }
            | Self::GroupPins { account_id, .. }
            | Self::GroupPinFinished { account_id, .. }
            | Self::RecallFinished { account_id, .. }
            | Self::RemoteHistoryCapability { account_id, .. }
            | Self::RemoteHistoryFinished { account_id, .. }
            | Self::Recalled { account_id, .. }
            | Self::ForwardFinished { account_id, .. }
            | Self::ForwardBatchFinished { account_id, .. }
            | Self::ForwardTargets { account_id, .. }
            | Self::StickerFinished { account_id, .. }
            | Self::StickerSearchCapability { account_id, .. }
            | Self::StickersFound { account_id, .. }
            | Self::StickerPacksFound { account_id, .. }
            | Self::StickerPackFound { account_id, .. }
            | Self::ReadState { account_id, .. }
            | Self::ReadStateChanged { account_id, .. }
            | Self::NotificationPreferences { account_id, .. }
            | Self::NotificationOpen { account_id, .. }
            | Self::PhotoCapability { account_id, .. }
            | Self::PhotoPrepared { account_id, .. }
            | Self::AlbumPrepared { account_id, .. }
            | Self::FileCapability { account_id, .. }
            | Self::FileTransfer { account_id, .. }
            | Self::FileAccepted { account_id, .. }
            | Self::Avatars { account_id, .. }
            | Self::PrivacyChanged { account_id, .. }
            | Self::HiddenMatches { account_id, .. }
            | Self::HiddenUnlockResult { account_id, .. }
            | Self::SendSettled { account_id, .. }
            | Self::MessagesRefreshed { account_id, .. }
            | Self::MessagesRefreshFailed { account_id, .. }
            | Self::SenderNames { account_id, .. }
            | Self::DirectoryPage { account_id, .. }
            | Self::DirectoryStatus { account_id, .. }
            | Self::DirectoryChanged { account_id, .. }
            | Self::DirectoryFailed { account_id, .. }
            | Self::Diagnostic { account_id, .. }
            | Self::Ready { account_id, .. }
            | Self::Opened { account_id, .. }
            | Self::Page { account_id, .. }
            | Self::Overview { account_id, .. }
            | Self::OverviewFailed { account_id, .. }
            | Self::SearchResults { account_id, .. }
            | Self::SearchFailed { account_id, .. }
            | Self::Connection { account_id, .. }
            | Self::Received { account_id, .. }
            | Self::Sent { account_id, .. }
            | Self::SendFailed { account_id, .. }
            | Self::RequestFailed { account_id, .. }
            | Self::Fault { account_id, .. } => account_id,
        }
    }
}
pub struct LiveWorker {
    account: String,
    commands: mpsc::Sender<LiveCommand>,
    pub events: mpsc::Receiver<LiveEvent>,
    stop: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}
pub type MessagingHandle = LiveWorker;
impl LiveWorker {
    pub fn start(
        profile: Arc<Profile>,
        session: Arc<VerifiedSession>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let account = session.account_id().to_owned();
        // Binding and all SQLite/network operations happen in the worker, not the UI.
        let (commands, rx) = mpsc::channel(super::COMMAND_CAPACITY);
        let (tx, events) = mpsc::channel(super::EVENT_CAPACITY);
        let (stop, cancel) = watch::channel(false);
        let thread = std::thread::Builder::new()
            .name("native-messaging".into())
            .spawn(move || {
                let account_id = session.account_id().to_owned();
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match result {
                    Ok(rt) => rt.block_on(runtime::run(profile, session, rx, tx, repaint, cancel)),
                    Err(_) => {
                        let _ = tx.blocking_send(LiveEvent::Fault {
                            account_id,
                            error: "Cannot start messaging worker".into(),
                        });
                        repaint();
                    }
                }
            })?;
        Ok(Self {
            account,
            commands,
            events,
            stop,
            thread: Some(thread),
        })
    }
    /// Synthetic presentation harness: production bounds and submit validation,
    /// without a runtime thread, session, profile or network capability.
    #[cfg(test)]
    pub(crate) fn presentation_fixture(
        account: &str,
    ) -> (Self, mpsc::Receiver<LiveCommand>, mpsc::Sender<LiveEvent>) {
        let (commands, rx) = mpsc::channel(super::COMMAND_CAPACITY);
        let (tx, events) = mpsc::channel(super::EVENT_CAPACITY);
        let (stop, _) = watch::channel(false);
        (
            Self {
                account: account.into(),
                commands,
                events,
                stop,
                thread: None,
            },
            rx,
            tx,
        )
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn send_sticker(
        &self,
        request_id: u64,
        thread: String,
        sticker: crate::domain::StickerDescriptor,
    ) -> bool {
        self.submit(LiveCommand::SendSticker {
            request_id,
            thread,
            sticker,
        })
        .is_ok()
    }
    pub fn search_stickers(&self, request_id: u64, thread: String, query: String) -> bool {
        self.submit(LiveCommand::SearchStickers {
            request_id,
            thread,
            query,
        })
        .is_ok()
    }
    pub fn sticker_packs(&self, request_id: u64, thread: String) -> bool {
        self.submit(LiveCommand::ListStickerPacks { request_id, thread })
            .is_ok()
    }
    pub fn browse_sticker_pack(
        &self,
        request_id: u64,
        thread: String,
        pack_list_id: u64,
        category_id: u64,
    ) -> bool {
        self.submit(LiveCommand::BrowseStickerPack {
            request_id,
            thread,
            pack_list_id,
            category_id,
        })
        .is_ok()
    }
    pub fn send_catalog_sticker(
        &self,
        request_id: u64,
        search_id: u64,
        thread: String,
        sticker: crate::domain::StickerDescriptor,
    ) -> bool {
        self.submit(LiveCommand::SendCatalogSticker {
            request_id,
            search_id,
            thread,
            sticker,
        })
        .is_ok()
    }
    pub fn submit(&self, command: LiveCommand) -> std::result::Result<(), SubmitError> {
        use crate::protocol::messaging::Target;
        let valid_message_ids = |ids: &[String]| {
            ids.len() <= crate::store::MAX_REFRESH_MESSAGES
                && ids
                    .iter()
                    .all(|id| crate::protocol::messaging::numeric_id(id).is_ok())
        };
        let valid = match &command {
            LiveCommand::React {
                request_id,
                thread,
                message_id,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
                    && crate::protocol::messaging::numeric_id(message_id).is_ok()
            }
            LiveCommand::ReactionState {
                thread,
                message_ids,
                ..
            } => Target::from_thread(thread).is_ok() && valid_message_ids(message_ids),
            LiveCommand::GroupMembers {
                request_id,
                thread,
                after,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && after.as_ref().is_none_or(|value| value.len() == 67)
            }
            LiveCommand::CancelGroupMembers { request_id } => *request_id > 0,
            LiveCommand::GroupPins {
                request_id, thread, ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
            }
            LiveCommand::SetGroupPin {
                request_id,
                thread,
                message_id,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && crate::protocol::messaging::numeric_id(message_id).is_ok()
            }
            LiveCommand::GroupInviteTargets {
                request_id,
                thread,
                query,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && query.len() <= 512
                    && query.chars().count() <= 128
                    && !query.chars().any(char::is_control)
            }
            LiveCommand::GroupInvite {
                request_id,
                thread,
                member,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && crate::protocol::messaging::numeric_id(member).is_ok()
                    && member != &self.account
            }
            LiveCommand::GroupLeave {
                request_id, thread, ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
            }
            LiveCommand::Recall {
                request_id,
                thread,
                message_id,
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|t| t.kind == ThreadKind::Group || t.remote_id != self.account)
                    && crate::protocol::messaging::numeric_id(message_id).is_ok()
            }
            LiveCommand::LoadRemoteHistory {
                request_id,
                thread,
                anchor,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && anchor.sent_at_ms > 0
                    && crate::protocol::messaging::numeric_id(&anchor.id).is_ok()
            }
            LiveCommand::Forward {
                request_id,
                source_thread,
                source_message_id,
                target_thread,
            } => {
                *request_id > 0
                    && Target::from_thread(source_thread)
                        .is_ok_and(|t| t.kind == ThreadKind::Direct)
                    && Target::from_thread(target_thread)
                        .is_ok_and(|t| t.kind == ThreadKind::Direct)
                    && crate::protocol::messaging::numeric_id(source_message_id).is_ok()
            }
            LiveCommand::ForwardBatch {
                request_id,
                source_thread,
                source_message_id,
                target_threads,
            } => {
                *request_id > 0
                    && Target::from_thread(source_thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Direct && target.remote_id != self.account
                    })
                    && crate::protocol::messaging::numeric_id(source_message_id).is_ok()
                    && valid_forward_targets(target_threads, &self.account)
            }
            LiveCommand::ForwardTargets {
                generation,
                source_thread,
                source_message_id,
                query,
            } => {
                *generation > 0
                    && Target::from_thread(source_thread)
                        .is_ok_and(|t| t.kind == ThreadKind::Direct)
                    && crate::protocol::messaging::numeric_id(source_message_id).is_ok()
                    && query.len() <= 512
                    && query.chars().count() <= 128
                    && !query.chars().any(char::is_control)
            }
            LiveCommand::ReadVisible {
                thread,
                message_ids,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && !message_ids.is_empty()
                    && valid_message_ids(message_ids)
            }
            LiveCommand::ReadState {
                threads,
                thread,
                message_ids,
                ..
            } => {
                threads.len() <= crate::domain::MAX_PAGE_SIZE
                    && threads
                        .iter()
                        .all(|thread| Target::from_thread(thread).is_ok())
                    && thread
                        .as_ref()
                        .is_none_or(|thread| Target::from_thread(thread).is_ok())
                    && valid_message_ids(message_ids)
            }
            LiveCommand::NotificationForeground { thread } => thread
                .as_ref()
                .is_none_or(|thread| Target::from_thread(thread).is_ok()),
            LiveCommand::SetNotificationPreferences { .. } => true,
            LiveCommand::ResolveAvatars { thread_ids, .. } => {
                thread_ids.len() <= MAX_AVATARS
                    && thread_ids
                        .iter()
                        .all(|thread| Target::from_thread(thread).is_ok())
            }
            LiveCommand::RefreshPrivacy | LiveCommand::RelockHidden => true,
            LiveCommand::SearchHidden { query, .. } => {
                query.chars().count() <= 128 && !query.chars().any(char::is_control)
            }
            LiveCommand::UnlockHidden { thread, .. } => Target::from_thread(thread).is_ok(),
            LiveCommand::RefreshMessages {
                thread,
                message_ids,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && message_ids.len() <= crate::store::MAX_REFRESH_MESSAGES
                    && message_ids
                        .iter()
                        .all(|id| crate::domain::validate_id(id).is_ok())
            }
            LiveCommand::ResolveSenders {
                thread, sender_ids, ..
            } => {
                Target::from_thread(thread).is_ok()
                    && sender_ids.len() <= MAX_SENDER_NAMES
                    && sender_ids
                        .iter()
                        .all(|sender| crate::domain::validate_id(sender).is_ok())
            }
            LiveCommand::Directory {
                is_group,
                query,
                after,
                ..
            } => {
                query.chars().count() <= 128
                    && !query.chars().any(char::is_control)
                    && after.as_ref().is_none_or(|cursor| {
                        cursor.name.len() <= 512
                            && !cursor.name.chars().any(char::is_control)
                            && Target::from_thread(&cursor.thread_id)
                                .is_ok_and(|target| (target.kind == ThreadKind::Group) == *is_group)
                    })
            }
            LiveCommand::Open {
                kind, remote_id, ..
            } => Target::new(*kind, remote_id).is_ok(),
            LiveCommand::Load { thread, before, .. } => {
                Target::from_thread(thread).is_ok()
                    && before
                        .as_ref()
                        .is_none_or(|c| crate::domain::validate_id(&c.id).is_ok())
            }
            LiveCommand::Overview { thread, .. } => Target::from_thread(thread).is_ok(),
            LiveCommand::Search {
                thread,
                query,
                before,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && crate::domain::search::search_query(query) == Some(query.as_str())
                    && crate::domain::search::valid_search_cursor(before.as_ref())
            }
            LiveCommand::Send {
                thread,
                text,
                reply_to,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && reply_to
                        .as_deref()
                        .is_none_or(crate::domain::reply::valid_source_id)
                    && !text.trim().is_empty()
                    && text.len() <= MAX_TEXT_BYTES
            }
            LiveCommand::SendMentions {
                request_id,
                thread,
                text,
                mentions,
                ..
            } => {
                *request_id > 0
                    && Target::from_thread(thread)
                        .is_ok_and(|target| target.kind == ThreadKind::Group)
                    && crate::domain::mention::validate_outgoing(text, mentions).is_ok()
                    && mentions.iter().all(|mention| mention.uid != self.account)
            }
            LiveCommand::SendSticker {
                request_id,
                thread,
                sticker,
            } => {
                *request_id > 0
                    && sticker.is_builtin()
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
            }
            LiveCommand::SearchStickers {
                request_id,
                thread,
                query,
            } => {
                *request_id > 0
                    && crate::protocol::sticker_catalog::valid_query(query)
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
            }
            LiveCommand::ListStickerPacks { request_id, thread } => {
                *request_id > 0
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
            }
            LiveCommand::BrowseStickerPack {
                request_id,
                thread,
                pack_list_id,
                category_id,
            } => {
                *request_id > 0
                    && *pack_list_id > 0
                    && crate::protocol::sticker_catalog::valid_pack_id(*category_id)
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
            }
            LiveCommand::SendCatalogSticker {
                request_id,
                search_id,
                thread,
                sticker,
            } => {
                *request_id > 0
                    && *search_id > 0
                    && sticker.is_catalog()
                    && Target::from_thread(thread).is_ok_and(|target| {
                        target.kind == ThreadKind::Group || target.remote_id != self.account
                    })
            }
            LiveCommand::SendFile {
                thread,
                attachment,
                request_id,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && *request_id != 0
                    && attachment.size_bytes > 0
                    && attachment.size_bytes <= crate::domain::MAX_DIRECT_FILE_BYTES
                    && !attachment.name.is_empty()
                    && attachment.name.len() <= 255
            }
            LiveCommand::PreparePhoto {
                thread,
                attachment,
                request_id,
                resolution,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && *request_id != 0
                    && attachment.size_bytes > 0
                    && attachment.size_bytes
                        <= match resolution {
                            PhotoResolution::Hd => 20 * 1024 * 1024,
                            PhotoResolution::Original => MAX_ORIGINAL_PHOTO_BYTES,
                        }
            }
            LiveCommand::CancelPhotoPreparation { request_id } => *request_id != 0,
            LiveCommand::CancelAlbum { thread, request_id }
            | LiveCommand::CancelAlbumPreparation { thread, request_id } => {
                Target::from_thread(thread).is_ok() && *request_id != 0
            }
            LiveCommand::PrepareAlbum {
                thread,
                attachments,
                request_id,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && *request_id != 0
                    && (2..=MAX_ALBUM_PHOTOS).contains(&attachments.len())
                    && attachments
                        .iter()
                        .all(|attachment| (1..=20 * 1024 * 1024).contains(&attachment.size_bytes))
            }
            LiveCommand::SendPhoto {
                thread,
                photo,
                caption,
                request_id,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && *request_id != 0
                    && photo.size_bytes() > 0
                    && photo.size_bytes() <= 5 * 1024 * 1024
                    && caption.len() <= MAX_TEXT_BYTES
            }
            LiveCommand::SendAlbum {
                thread,
                photos,
                request_id,
                ..
            } => {
                Target::from_thread(thread).is_ok()
                    && *request_id != 0
                    && (2..=MAX_ALBUM_PHOTOS).contains(&photos.len())
                    && photos.iter().all(|photo| {
                        photo.resolution() == PhotoResolution::Hd
                            && (1..=crate::store::photos::MAX_PHOTO_BYTES)
                                .contains(&photo.size_bytes())
                    })
            }
            LiveCommand::CancelFile { request_id } => *request_id != 0,
            LiveCommand::Reconnect
            | LiveCommand::RefreshDirectory { .. }
            | LiveCommand::Shutdown => true,
        };
        if !valid {
            return Err(SubmitError::Invalid);
        }
        self.commands.try_send(command).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => SubmitError::Busy,
            mpsc::error::TrySendError::Closed(_) => SubmitError::Stopped,
        })
    }
}
impl Drop for LiveWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.events.close();
        let _ = self.commands.try_send(LiveCommand::Shutdown); // Teardown is observed by the worker; the render thread never joins network or disk I/O.
        self.thread.take();
    }
}

impl super::Composer {
    fn accept_live_attachment(&mut self, thread: &str, request_id: u64, kind: TransferKind) {
        if let Some(file) = self.pending_file.as_ref().filter(|file| {
            file.request_id == request_id && file.thread == thread && file.kind == kind
        }) {
            let same_file = kind == TransferKind::File
                && file.attachment.is_some()
                && file.revision == self.attachment_revision
                && self.attachment.as_ref() == file.attachment.as_ref();
            let same_photo = kind == TransferKind::Photo
                && file.photo.is_some()
                && file.revision == self.photo_revision
                && self.photo.as_ref() == file.photo.as_ref();
            let same_album = kind == TransferKind::Album && file.revision == self.photo_revision;
            let clear_caption = same_photo && self.revision == file.draft_revision;
            if same_file {
                self.clear_attachment();
            }
            if same_photo {
                self.clear_photo();
            }
            if same_album {
                self.clear_album();
            }
            if clear_caption {
                self.text.clear();
                self.clear_mentions();
                self.edited();
            }
            self.pending_file = None;
            if self.pending.is_some_and(|(id, _)| id == request_id) {
                self.pending = None;
            }
        }
    }

    pub fn prepare_live_photo(
        &mut self,
        generation: u64,
        thread: String,
        attachment: crate::domain::Attachment,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        self.prepare_live_photo_with_resolution(
            generation,
            thread,
            attachment,
            PhotoResolution::Hd,
            enqueue,
        )
    }

    pub fn photo_resolution(&self) -> PhotoResolution {
        self.photo_resolution
    }

    pub fn prepare_live_photo_with_resolution(
        &mut self,
        generation: u64,
        thread: String,
        attachment: crate::domain::Attachment,
        resolution: PhotoResolution,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.reply().is_some() || self.attachment().is_some() {
            return Err(SubmitError::Invalid);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::PreparePhoto {
            generation,
            thread: thread.clone(),
            attachment: attachment.clone(),
            resolution,
            request_id,
        })?;
        self.clear_album();
        self.photo_revision = self.photo_revision.wrapping_add(1);
        self.photo_source = Some(attachment);
        self.photo_resolution = resolution;
        self.photo = None;
        self.photo_problem = None;
        self.photo_preparation = Some(super::PendingPhotoPreparation {
            request_id,
            thread,
            generation,
            revision: self.photo_revision,
        });
        self.edited();
        Ok(())
    }

    pub fn prepare_live_album(
        &mut self,
        generation: u64,
        thread: String,
        attachments: Vec<crate::domain::Attachment>,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.reply().is_some()
            || self.attachment().is_some()
            || !(2..=MAX_ALBUM_PHOTOS).contains(&attachments.len())
        {
            return Err(SubmitError::Invalid);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::PrepareAlbum {
            generation,
            thread: thread.clone(),
            attachments: attachments.clone(),
            request_id,
        })?;
        self.clear_photo();
        self.photo_revision = self.photo_revision.wrapping_add(1);
        self.album = super::AlbumDraft {
            sources: attachments,
            preparation: Some(super::PendingPhotoPreparation {
                request_id,
                thread,
                generation,
                revision: self.photo_revision,
            }),
            ..Default::default()
        };
        self.edited();
        Ok(())
    }

    pub fn send_live_album(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.reply().is_some()
            || self.attachment().is_some()
            || self.album_preparing()
            || self.album_problem().is_some()
        {
            return Err(SubmitError::Invalid);
        }
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        let photos = self.album().ok_or(SubmitError::Invalid)?.to_vec();
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::SendAlbum {
            generation,
            thread: thread.clone(),
            photos,
            request_id,
        })?;
        self.pending_file = Some(super::PendingFile {
            request_id,
            thread,
            revision: self.photo_revision,
            kind: TransferKind::Album,
            attachment: None,
            photo: None,
            draft_revision: self.revision,
        });
        self.pending = Some((request_id, self.revision));
        Ok(())
    }

    pub fn send_live_photo(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.reply().is_some() || self.attachment().is_some() || self.photo_preparing() {
            return Err(SubmitError::Invalid);
        }
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        let photo = self.photo().cloned().ok_or(SubmitError::Invalid)?;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::SendPhoto {
            generation,
            thread: thread.clone(),
            photo: photo.clone(),
            caption: self.text.clone(),
            request_id,
        })?;
        self.pending_file = Some(super::PendingFile {
            request_id,
            thread,
            kind: TransferKind::Photo,
            revision: self.photo_revision,
            attachment: None,
            photo: Some(photo),
            draft_revision: self.revision,
        });
        self.pending = Some((request_id, self.revision));
        Ok(())
    }

    pub fn send_live_file(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.reply().is_some() || self.has_photo_intent() {
            return Err(SubmitError::Invalid);
        }
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        let attachment = self.attachment().cloned().ok_or(SubmitError::Invalid)?;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::SendFile {
            generation,
            thread: thread.clone(),
            attachment: attachment.clone(),
            request_id,
        })?;
        self.pending_file = Some(super::PendingFile {
            request_id,
            thread,
            revision: self.attachment_revision,
            kind: TransferKind::File,
            attachment: Some(attachment),
            photo: None,
            draft_revision: self.revision,
        });
        self.pending = Some((request_id, self.revision));
        Ok(())
    }

    pub fn send_live(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        self.send_live_in_scope(generation, thread, None, enqueue)
    }

    pub fn send_live_in_scope(
        &mut self,
        generation: u64,
        thread: String,
        privacy_revision: Option<u64>,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.attachment().is_some()
            || self.has_photo_intent()
            || self.reply().is_some_and(|reply| reply.thread_id != thread)
        {
            return Err(SubmitError::Invalid);
        }
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        if self.has_mentions() {
            if self.reply().is_some() || !thread.starts_with("g:") {
                return Err(SubmitError::Invalid);
            }
            let privacy_revision = privacy_revision.ok_or(SubmitError::Invalid)?;
            enqueue(LiveCommand::SendMentions {
                generation,
                thread,
                text: crate::domain::canonical_plain_text(&self.text).to_owned(),
                mentions: self.wire_mentions().map_err(|_| SubmitError::Invalid)?,
                privacy_revision,
                request_id,
            })?;
        } else {
            enqueue(LiveCommand::Send {
                generation,
                thread,
                text: self.text.clone(),
                reply_to: self.reply().map(|reply| reply.message_id.clone()),
                request_id,
            })?;
        }
        self.pending = Some((request_id, self.revision));
        Ok(())
    }
    pub fn apply_live(&mut self, event: &LiveEvent, account_id: &str) {
        if event.account_id() != account_id {
            return;
        }
        match event {
            LiveEvent::PhotoPrepared {
                generation,
                thread,
                request_id,
                result,
                ..
            } => {
                if self.photo_preparation.as_ref().is_some_and(|pending| {
                    pending.request_id == *request_id
                        && pending.thread == *thread
                        && pending.generation == *generation
                        && pending.revision == self.photo_revision
                }) {
                    self.photo_preparation = None;
                    if self.reply().is_some() || self.attachment().is_some() {
                        self.photo_problem = Some(
                            "Đã thay đổi bản nháp; bỏ ảnh trước khi gửi tệp hoặc trích dẫn.".into(),
                        );
                    } else {
                        match result {
                            Ok(photo) if photo.resolution() == self.photo_resolution => {
                                self.photo = Some(photo.clone());
                                self.photo_problem = None;
                            }
                            Err(error) => {
                                self.photo = None;
                                self.photo_problem = Some(error.to_string());
                            }
                            Ok(_) => {
                                self.photo = None;
                                self.photo_problem =
                                    Some("Chế độ ảnh đã thay đổi; hãy chọn lại ảnh.".into());
                            }
                        }
                    }
                }
            }
            LiveEvent::FileAccepted {
                request_id,
                thread,
                message,
                conversation,
                kind,
                ..
            } if *kind != TransferKind::Album
                && message.account_id == account_id
                && message.thread_id == *thread
                && conversation.id == *thread =>
            {
                self.accept_live_attachment(thread, *request_id, *kind);
            }
            LiveEvent::FileTransfer {
                request_id,
                thread,
                phase: FileTransferPhase::Accepted,
                kind,
                ..
            } => {
                // The worker can confirm a durable ACK without publishing the
                // file message after its privacy lease has been revoked.
                self.accept_live_attachment(thread, *request_id, *kind);
            }
            LiveEvent::AlbumPrepared {
                generation,
                thread,
                request_id,
                result,
                ..
            } if self.album.preparation.as_ref().is_some_and(|pending| {
                pending.request_id == *request_id
                    && pending.thread == *thread
                    && pending.generation == *generation
                    && pending.revision == self.photo_revision
            }) =>
            {
                self.album.preparation = None;
                if self.reply().is_some() || self.attachment().is_some() {
                    self.album.problem = Some(
                        "Bản nháp đã thay đổi. Hãy bỏ album để gửi tệp hoặc trích dẫn.".into(),
                    );
                } else {
                    match result {
                        Ok(photos)
                            if photos.len() == self.album.sources.len()
                                && photos
                                    .iter()
                                    .all(|photo| photo.resolution() == PhotoResolution::Hd) =>
                        {
                            self.album.photos.clone_from(photos);
                            self.album.problem = None;
                        }
                        Ok(_) => {
                            self.album.problem = Some("Album đã thay đổi. Hãy chọn lại ảnh.".into())
                        }
                        Err(error) => self.album.problem = Some(error.to_string()),
                    }
                }
            }
            LiveEvent::FileTransfer {
                request_id,
                thread,
                kind,
                phase:
                    phase @ (FileTransferPhase::Cancelled
                    | FileTransferPhase::Failed
                    | FileTransferPhase::Unknown
                    | FileTransferPhase::AcceptedLocalFailure
                    | FileTransferPhase::Partial),
                message,
                ..
            } => {
                if self.pending_file.as_ref().is_some_and(|file| {
                    file.request_id == *request_id && file.thread == *thread && file.kind == *kind
                }) {
                    if *kind == TransferKind::Album
                        && self
                            .pending_file
                            .as_ref()
                            .is_some_and(|file| file.revision == self.photo_revision)
                        && matches!(
                            phase,
                            FileTransferPhase::Unknown
                                | FileTransferPhase::AcceptedLocalFailure
                                | FileTransferPhase::Partial
                        )
                    {
                        self.album.problem = Some(message.clone().unwrap_or_else(|| {
                            "Hãy kiểm tra tin đã gửi trước khi chọn lại album.".into()
                        }));
                    }
                    self.pending_file = None;
                    if self.pending.is_some_and(|(id, _)| id == *request_id) {
                        self.pending = None;
                    }
                }
            }
            LiveEvent::Sent { request_id, .. }
            | LiveEvent::SendSettled {
                request_id,
                accepted: true,
                ..
            } => {
                if let Some((pending, revision)) = self.pending {
                    if pending == *request_id {
                        if self.revision == revision {
                            self.text.clear();
                            self.clear_mentions();
                            self.reply = None;
                            self.edited();
                        }
                        self.pending = None;
                    }
                }
            }
            LiveEvent::SendFailed { request_id, .. }
            | LiveEvent::SendSettled {
                request_id,
                accepted: false,
                ..
            } => {
                if self.pending.is_some_and(|(p, _)| p == *request_id) {
                    self.pending = None;
                }
            }
            LiveEvent::Fault { .. }
            | LiveEvent::Connection {
                state: LiveConnection::Expired,
                ..
            } => {
                self.pending = None;
                self.pending_file = None;
                if self.photo_source.is_some() {
                    self.photo = None;
                    self.photo_preparation = None;
                    self.photo_problem =
                        Some("Kết nối đã thay đổi; chuẩn bị lại ảnh trước khi gửi.".into());
                    self.photo_revision = self.photo_revision.wrapping_add(1);
                }
                if !self.album.sources.is_empty() {
                    self.album.photos.clear();
                    self.album.preparation = None;
                    self.album.problem = Some(
                        "Kết nối đã thay đổi. Hãy kiểm tra hội thoại trước khi chọn lại album."
                            .into(),
                    );
                    self.photo_revision = self.photo_revision.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
}
impl super::DraftBook {
    pub fn apply_live(&mut self, event: &LiveEvent, account_id: &str) {
        if event.account_id() != account_id {
            return;
        }
        match event {
            LiveEvent::Sent { thread, .. }
            | LiveEvent::FileAccepted { thread, .. }
            | LiveEvent::FileTransfer { thread, .. }
            | LiveEvent::PhotoPrepared { thread, .. }
            | LiveEvent::AlbumPrepared { thread, .. }
            | LiveEvent::SendFailed { thread, .. }
            | LiveEvent::SendSettled { thread, .. } => {
                if self.thread.as_deref() == Some(thread) {
                    self.active.apply_live(event, account_id)
                } else if let Some(composer) = self.parked.get_mut(thread) {
                    composer.apply_live(event, account_id)
                }
            }
            LiveEvent::Fault { .. }
            | LiveEvent::Connection {
                state: LiveConnection::Expired,
                ..
            } => {
                self.active.apply_live(event, account_id);
                for composer in self.parked.values_mut() {
                    composer.apply_live(event, account_id)
                }
            }
            _ => {}
        }
    }
}
impl super::Model {
    pub fn select_live_if_accepted(
        &mut self,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        let generation = self.generation.wrapping_add(1);
        enqueue(LiveCommand::Load {
            generation,
            thread: thread.clone(),
            before: None,
        })?;
        self.generation = generation;
        self.selected = Some(thread);
        self.messages.clear();
        self.has_older = false;
        self.pending = true;
        Ok(())
    }
    pub fn apply_live(&mut self, event: LiveEvent, account_id: &str, latest_page: bool) -> bool {
        if event.account_id() != account_id {
            return false;
        }
        match event {
            LiveEvent::Ready { conversations, .. }
            | LiveEvent::DirectoryChanged { conversations, .. } => {
                self.conversations = conversations;
                true
            }
            LiveEvent::Overview {
                generation,
                thread,
                overview,
                ..
            } => self.apply_overview(generation, &thread, *overview),
            LiveEvent::OverviewFailed {
                generation, thread, ..
            } => {
                self.overview_failed(generation, &thread);
                false
            }
            LiveEvent::SearchResults {
                request,
                thread,
                search,
                ..
            } => self.apply_search(request, &thread, *search),
            LiveEvent::SearchFailed {
                request, thread, ..
            } => self.apply_search_failed(request, &thread),
            LiveEvent::Page {
                generation,
                thread,
                messages,
                has_older,
                ..
            } => self.apply(super::Event::Page {
                generation,
                thread,
                messages,
                has_older,
            }),
            LiveEvent::Opened {
                generation,
                conversation,
                ..
            } => {
                if generation != self.generation {
                    return false;
                }
                let thread = conversation.id.clone();
                self.upsert_live_conversation(conversation);
                self.selected = Some(thread);
                self.messages.clear();
                self.pending = false;
                self.has_older = false;
                true
            }
            LiveEvent::Received {
                message,
                conversation,
                ..
            } => {
                if message.account_id != account_id || message.thread_id != conversation.id {
                    return false;
                }
                self.append_live_message(message, conversation, latest_page);
                true
            }
            LiveEvent::Recalled {
                thread,
                message_id,
                actor_id,
                message,
                conversation,
                ..
            } => {
                if message.as_ref().is_some_and(|m| {
                    m.account_id != account_id
                        || m.thread_id != thread
                        || m.id != message_id
                        || m.sender != actor_id
                        || !m.is_recalled()
                }) || conversation.as_ref().is_some_and(|c| c.id != thread)
                {
                    return false;
                }
                if let Some(conversation) = conversation {
                    if !self.conversations.iter().any(|saved| {
                        saved.id == thread
                            && saved.last_message_at_ms > conversation.last_message_at_ms
                    }) {
                        self.upsert_live_conversation(conversation);
                    }
                }
                if self.selected.as_deref() == Some(&thread) {
                    self.clear_search();
                    for visible in &mut self.messages {
                        if visible.account_id != account_id || visible.thread_id != thread {
                            continue;
                        }
                        if visible.id == message_id && visible.sender == actor_id {
                            if let Some(recalled) = &message {
                                *visible = recalled.clone();
                            }
                        }
                        if let Some(quote) = visible
                            .quote
                            .as_mut()
                            .filter(|q| q.message_id == message_id && q.owner_id == actor_id)
                        {
                            crate::domain::RECALLED_TEXT.clone_into(&mut quote.text);
                            quote.message_type = 0;
                        }
                    }
                }
                true
            }
            LiveEvent::Sent {
                generation,
                thread,
                message,
                conversation,
                ..
            } => {
                if message.account_id != account_id
                    || message.thread_id != thread
                    || conversation.id != thread
                {
                    return false;
                }
                self.append_live_message(message, conversation, latest_page);
                if generation == self.generation && self.selected.as_deref() == Some(&thread) {
                    self.status = "Zalo đã nhận tin nhắn. Chưa xác nhận người nhận đã xem.".into();
                }
                true
            }
            LiveEvent::FileAccepted {
                thread,
                message,
                conversation,
                kind,
                ..
            } => {
                if message.account_id != account_id
                    || message.thread_id != thread
                    || conversation.id != thread
                {
                    return false;
                }
                self.append_live_message(message, conversation, latest_page);
                if kind != TransferKind::Album && self.selected.as_deref() == Some(&thread) {
                    self.status = "Zalo đã nhận tệp. Chưa xác nhận người nhận đã xem.".into();
                }
                true
            }
            LiveEvent::FileTransfer {
                thread,
                phase,
                message,
                kind,
                ..
            } => {
                if self.selected.as_deref() == Some(&thread) {
                    if kind == TransferKind::Album {
                        if phase == FileTransferPhase::Accepted {
                            self.status = "Zalo đã nhận đủ ảnh trong album.".into();
                        } else if let Some(message) = message {
                            self.status = message;
                        } else {
                            return false;
                        }
                        return true;
                    }
                    self.status = match phase {
                        FileTransferPhase::Unknown => {
                            "Chưa xác định tệp đã gửi hay chưa. Không tự gửi lại.".into()
                        }
                        FileTransferPhase::AcceptedLocalFailure => {
                            "Zalo đã nhận tệp; chưa lưu được xác nhận trên thiết bị. Không gửi lại."
                                .into()
                        }
                        FileTransferPhase::Cancelled => {
                            "Đã hủy gửi tệp. Tệp và bản nháp được giữ lại.".into()
                        }
                        FileTransferPhase::Failed => message.unwrap_or_else(|| {
                            "Chưa gửi được tệp. Tệp và bản nháp được giữ lại.".into()
                        }),
                        _ => return false,
                    };
                    return true;
                }
                false
            }
            LiveEvent::Connection { state, .. } => {
                self.status = match state {
                    LiveConnection::Connecting => "Đang kết nối Zalo…",
                    LiveConnection::Online => "Đã kết nối Zalo",
                    LiveConnection::Reconnecting => "Đang kết nối lại…",
                    LiveConnection::Offline => "Mất kết nối. Bản nháp được giữ lại.",
                    LiveConnection::Expired => "Phiên đã hết hạn. Vui lòng đăng nhập lại.",
                }
                .into();
                true
            }
            LiveEvent::SendFailed {
                generation,
                thread,
                error,
                unknown_delivery,
                server_accepted,
                ..
            } => {
                if generation == self.generation && self.selected.as_deref() == Some(&thread) {
                    self.status = if server_accepted {
                        error
                    } else if unknown_delivery {
                        format!("Chưa xác định tin đã gửi hay chưa. Không tự gửi lại. {error}")
                    } else {
                        error
                    };
                }
                true
            }
            LiveEvent::RequestFailed {
                generation,
                thread,
                error,
                ..
            } => {
                if generation != self.generation || self.selected.as_deref() != Some(&thread) {
                    return false;
                }
                self.pending = false;
                self.status = error;
                true
            }
            LiveEvent::Diagnostic { diagnostic, .. } => {
                self.status = diagnostic.to_string();
                true
            }
            LiveEvent::GroupActionFinished {
                thread,
                outcome: GroupActionOutcome::Left,
                ..
            } => {
                if let Some(group) = self.conversations.iter_mut().find(|item| item.id == thread) {
                    group.group_left = true;
                }
                true
            }
            LiveEvent::ReactionCapability { .. }
            | LiveEvent::ReactionFinished { .. }
            | LiveEvent::ReactionState { .. }
            | LiveEvent::ReactionsChanged { .. }
            | LiveEvent::GroupMembers { .. }
            | LiveEvent::GroupInviteTargets { .. }
            | LiveEvent::GroupActionFinished { .. }
            | LiveEvent::GroupPins { .. }
            | LiveEvent::GroupPinFinished { .. }
            | LiveEvent::RecallFinished { .. }
            | LiveEvent::RemoteHistoryCapability { .. }
            | LiveEvent::RemoteHistoryFinished { .. }
            | LiveEvent::ForwardFinished { .. }
            | LiveEvent::ForwardBatchFinished { .. }
            | LiveEvent::ForwardTargets { .. }
            | LiveEvent::StickerFinished { .. }
            | LiveEvent::StickerSearchCapability { .. }
            | LiveEvent::StickersFound { .. }
            | LiveEvent::StickerPacksFound { .. }
            | LiveEvent::StickerPackFound { .. }
            | LiveEvent::ReadState { .. }
            | LiveEvent::ReadStateChanged { .. }
            | LiveEvent::NotificationPreferences { .. }
            | LiveEvent::NotificationOpen { .. }
            | LiveEvent::Avatars { .. }
            | LiveEvent::PhotoPrepared { .. }
            | LiveEvent::AlbumPrepared { .. }
            | LiveEvent::PhotoCapability { .. }
            | LiveEvent::FileCapability { .. }
            | LiveEvent::PrivacyChanged { .. }
            | LiveEvent::HiddenMatches { .. }
            | LiveEvent::HiddenUnlockResult { .. }
            | LiveEvent::SendSettled { .. }
            | LiveEvent::MessagesRefreshed { .. }
            | LiveEvent::MessagesRefreshFailed { .. }
            | LiveEvent::SenderNames { .. }
            | LiveEvent::DirectoryPage { .. }
            | LiveEvent::DirectoryStatus { .. }
            | LiveEvent::DirectoryFailed { .. } => false,
            LiveEvent::Fault { error, .. } => {
                self.status = error;
                self.pending = false;
                true
            }
        }
    }
    fn upsert_live_conversation(&mut self, conversation: Conversation) {
        if let Some(c) = self
            .conversations
            .iter_mut()
            .find(|c| c.id == conversation.id)
        {
            *c = conversation
        } else {
            self.conversations.push(conversation)
        }
        self.conversations.sort_by(|a, b| {
            b.last_message_at_ms
                .cmp(&a.last_message_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        self.conversations.truncate(crate::domain::MAX_PAGE_SIZE);
    }
    fn append_live_message(
        &mut self,
        message: Message,
        conversation: Conversation,
        latest_page: bool,
    ) {
        if latest_page && self.selected.as_deref() == Some(&message.thread_id) {
            if let Some(existing) = self.messages.iter_mut().find(|m| m.id == message.id) {
                *existing = message
            } else {
                self.messages.push(message)
            }
            self.messages.sort_by(|a, b| {
                a.sent_at_ms
                    .cmp(&b.sent_at_ms)
                    .then_with(|| a.id.cmp(&b.id))
            });
            if self.messages.len() > super::PAGE_SIZE {
                self.messages
                    .drain(..self.messages.len() - super::PAGE_SIZE);
                self.has_older = true;
            }
        }
        // A conversation with only list-silent messages (birthday cards, Official Account broadcasts) is not
        // added to the list; one already shown is still updated.
        if conversation.last_message_at_ms > 0
            || self.conversations.iter().any(|c| c.id == conversation.id)
        {
            self.upsert_live_conversation(conversation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DraftBook, Model};
    fn message() -> Message {
        Message {
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            source_client_id: None,
            quote: None,
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "900".into(),
            sender: "11".into(),
            text: "hello".into(),
            sticker: None,
            media: None,
            sent_at_ms: 20,
        }
    }
    fn conversation() -> Conversation {
        Conversation {
            id: "d:22".into(),
            title: "Friend".into(),
            last_message_at_ms: 20,
            last_text: "hello".into(),
            last_sender: "11".into(),
            group_left: false,
        }
    }
    fn sent(request_id: u64) -> LiveEvent {
        LiveEvent::Sent {
            account_id: "11".into(),
            generation: 1,
            thread: "d:22".into(),
            request_id,
            message: message(),
            conversation: conversation(),
        }
    }
    fn file_accepted(request_id: u64) -> LiveEvent {
        let mut file = message();
        file.text = "A.pdf".into();
        file.reply_blocked = true;
        file.media = Some(crate::domain::MediaDescriptor::File(
            crate::domain::FileDescriptor {
                title: "A.pdf".into(),
                extension: "pdf".into(),
                size_bytes: Some(32),
                url: None,
                encrypted: false,
                is_folder: false,
            },
        ));
        LiveEvent::FileAccepted {
            kind: crate::app::live::TransferKind::File,
            account_id: "11".into(),
            thread: "d:22".into(),
            request_id,
            message: file,
            conversation: conversation(),
        }
    }
    #[test]
    fn avatar_submit_is_bounded_and_only_accepts_typed_live_threads() {
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        for threads in [
            vec!["d:22".into(); MAX_AVATARS + 1],
            vec!["22".into()],
            vec!["g:0".into()],
        ] {
            assert_eq!(
                worker.submit(LiveCommand::ResolveAvatars {
                    generation: 5,
                    thread_ids: threads
                }),
                Err(SubmitError::Invalid)
            );
        }
        assert!(commands.try_recv().is_err());
        worker
            .submit(LiveCommand::ResolveAvatars {
                generation: 5,
                thread_ids: vec!["d:11".into(), "g:22".into()],
            })
            .unwrap();
        assert!(matches!(
            commands.try_recv().unwrap(),
            LiveCommand::ResolveAvatars { generation: 5, .. }
        ));
    }
    #[test]
    fn wrong_account_and_older_page_events_do_not_mutate_selected_pane() {
        let mut m = Model {
            selected: Some("d:22".into()),
            generation: 1,
            ..Default::default()
        };
        assert!(!m.apply_live(sent(1), "33", true));
        assert!(m.messages.is_empty());
        assert!(m.conversations.is_empty());
        assert!(m.apply_live(sent(1), "11", false));
        assert!(m.messages.is_empty());
        assert_eq!(m.conversations[0].last_text, "hello");
        assert!(m.apply_live(sent(1), "11", true));
        assert_eq!(m.messages.len(), 1);
        let bad = LiveEvent::Received {
            account_id: "11".into(),
            message: Message {
                mentions: Vec::new(),
                reply_blocked: false,
                recalled: false,
                source_client_id: None,
                quote: None,
                account_id: "33".into(),
                ..message()
            },
            conversation: conversation(),
        };
        assert!(!m.apply_live(bad, "11", true));
    }
    struct FileFixtures(std::path::PathBuf);
    impl FileFixtures {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "zalo-draft-files-{}",
                crate::domain::local_message_id().unwrap()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn attachment(&self, name: &str) -> crate::domain::Attachment {
            let path = self.0.join(name);
            std::fs::write(&path, [7u8; 32]).unwrap();
            crate::domain::Attachment::inspect(&path).unwrap()
        }
    }
    impl Drop for FileFixtures {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn photo_draft_fixture(files: &FileFixtures) -> (crate::domain::Attachment, PreparedPhoto) {
        let profile =
            crate::profile::Profile::at_root(&files.0.join("profile"), "photo-draft").unwrap();
        let path = files.0.join("synthetic.png");
        image::RgbImage::from_pixel(40, 24, image::Rgb([40, 100, 180]))
            .save(&path)
            .unwrap();
        let source = crate::domain::Attachment::inspect(&path).unwrap();
        let photo = runtime::photo::prepare_fixture(&profile, &source, "11", "d:22").unwrap();
        (source, photo)
    }
    fn ready_photo(
        composer: &mut crate::app::Composer,
        source: crate::domain::Attachment,
        photo: PreparedPhoto,
    ) {
        let mut request = 0;
        composer
            .prepare_live_photo(1, "d:22".into(), source, |command| {
                if let LiveCommand::PreparePhoto { request_id, .. } = command {
                    request = request_id;
                }
                Ok(())
            })
            .unwrap();
        composer.apply_live(
            &LiveEvent::PhotoPrepared {
                account_id: "11".into(),
                generation: 1,
                thread: "d:22".into(),
                request_id: request,
                result: Ok(photo),
            },
            "11",
        );
    }
    #[test]
    fn album_ack_applies_only_to_the_complete_batch_and_leaves_text_intact() {
        let files = FileFixtures::new();
        let (source, photo) = photo_draft_fixture(&files);
        for final_phase in [FileTransferPhase::Accepted, FileTransferPhase::Partial] {
            let mut composer = crate::app::Composer {
                text: "Bản nháp".into(),
                ..Default::default()
            };
            composer
                .prepare_live_album(
                    1,
                    "d:22".into(),
                    vec![source.clone(), source.clone()],
                    |_| Ok(()),
                )
                .unwrap();
            let prepared = |generation| LiveEvent::AlbumPrepared {
                account_id: "11".into(),
                thread: "d:22".into(),
                generation,
                request_id: 1,
                result: Ok(vec![photo.clone(), photo.clone()]),
            };
            composer.apply_live(&prepared(2), "11");
            assert!(composer.album_preparing());
            composer.apply_live(&prepared(1), "11");
            assert_eq!(composer.album().map(|album| album.len()), Some(2));
            composer
                .send_live_album(1, "d:22".into(), |_| Ok(()))
                .unwrap();
            // Receipt publication may happen for every member. The draft waits
            // for the actor's batch result, even if one receipt precedes failure.
            composer.apply_live(
                &LiveEvent::FileAccepted {
                    account_id: "11".into(),
                    thread: "d:22".into(),
                    kind: TransferKind::Album,
                    request_id: 2,
                    message: message(),
                    conversation: conversation(),
                },
                "11",
            );
            assert!(composer.in_flight());
            composer.apply_live(
                &LiveEvent::FileTransfer {
                    account_id: "11".into(),
                    thread: "d:22".into(),
                    kind: TransferKind::Album,
                    request_id: 2,
                    phase: final_phase,
                    uploaded: 50,
                    total: 50,
                    message: None,
                },
                "11",
            );
            assert!(!composer.in_flight());
            assert_eq!(composer.text, "Bản nháp");
            if final_phase == FileTransferPhase::Partial {
                assert_eq!(composer.album_sources().len(), 2);
                assert!(composer.album_problem().is_some());
                assert!(composer
                    .send_live_album(1, "d:22".into(), |_| panic!("Album retry is blocked"))
                    .is_err());
            } else {
                assert!(composer.album_sources().is_empty());
            }
        }
    }
    #[test]
    fn photo_ack_preserves_new_component_caption_and_quote_intent() {
        let files = FileFixtures::new();
        let (source, photo) = photo_draft_fixture(&files);
        for change in ["none", "caption", "photo", "quote", "wrong-kind"] {
            let mut composer = crate::app::Composer {
                text: "Caption X".into(),
                ..Default::default()
            };
            ready_photo(&mut composer, source.clone(), photo.clone());
            let mut request = 0;
            composer
                .send_live_photo(1, "d:22".into(), |command| {
                    if let LiveCommand::SendPhoto {
                        request_id,
                        caption,
                        ..
                    } = command
                    {
                        request = request_id;
                        assert_eq!(caption, "Caption X");
                    }
                    Ok(())
                })
                .unwrap();
            match change {
                "caption" => {
                    composer.text = "Caption Y".into();
                    composer.edited();
                }
                "photo" => ready_photo(&mut composer, source.clone(), photo.clone()),
                "quote" => composer.set_reply(crate::domain::ReplyReference {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    message_id: "9001".into(),
                    sender: "22".into(),
                    text: "New quote".into(),
                }),
                _ => {}
            }
            let event = LiveEvent::FileTransfer {
                account_id: "11".into(),
                thread: "d:22".into(),
                request_id: request,
                kind: if change == "wrong-kind" {
                    TransferKind::File
                } else {
                    TransferKind::Photo
                },
                phase: FileTransferPhase::Accepted,
                uploaded: 0,
                total: 0,
                message: None,
            };
            composer.apply_live(&event, "11");
            if change == "none" {
                assert!(composer.text.is_empty());
                assert!(!composer.has_photo_intent());
            } else {
                assert!(!composer.text.is_empty(), "{change}");
            }
            if change == "photo" || change == "wrong-kind" {
                assert!(composer.photo().is_some(), "{change}");
            }
            if change == "quote" {
                assert!(composer.reply().is_some());
            }
        }
    }
    #[test]
    fn pending_failed_and_removed_photo_never_silently_send_caption() {
        let files = FileFixtures::new();
        let (source, photo) = photo_draft_fixture(&files);
        let mut composer = crate::app::Composer {
            text: "Keep caption".into(),
            ..Default::default()
        };
        composer
            .prepare_live_photo(1, "d:22".into(), source, |_| Ok(()))
            .unwrap();
        assert_eq!(
            composer.send_live(1, "d:22".into(), |_| panic!("must not send text")),
            Err(SubmitError::Invalid)
        );
        composer.apply_live(
            &LiveEvent::PhotoPrepared {
                account_id: "11".into(),
                generation: 1,
                thread: "d:22".into(),
                request_id: 1,
                result: Err(PhotoPreparationFailure::InvalidFormat),
            },
            "11",
        );
        assert!(composer.has_photo_intent());
        assert_eq!(
            composer.send_live(1, "d:22".into(), |_| panic!("must not send text")),
            Err(SubmitError::Invalid)
        );
        composer.clear_photo();
        composer.apply_live(
            &LiveEvent::PhotoPrepared {
                account_id: "11".into(),
                generation: 1,
                thread: "d:22".into(),
                request_id: 1,
                result: Ok(photo),
            },
            "11",
        );
        assert!(!composer.has_photo_intent());
        assert!(composer.photo().is_none());
        assert_eq!(composer.text, "Keep caption");
        assert!(composer.send_live(1, "d:22".into(), |_| Ok(())).is_ok());
    }
    #[test]
    fn file_ack_clears_only_the_submitted_attachment_component() {
        let files = FileFixtures::new();
        let file = files.attachment("A.pdf");
        for change in ["none", "caption", "new-file", "reselected", "quote"] {
            let mut composer = super::super::Composer {
                text: "Chữ phải được giữ lại".into(),
                ..Default::default()
            };
            composer.set_attachment(file.clone());
            composer
                .send_live_file(1, "d:22".into(), |command| {
                    assert!(matches!(
                        command,
                        LiveCommand::SendFile { request_id: 1, .. }
                    ));
                    Ok(())
                })
                .unwrap();
            if change == "caption" {
                composer.text = "Chữ vừa được chỉnh sửa".into();
                composer.edited();
            }
            if change == "new-file" {
                composer.set_attachment(files.attachment("B.pdf"));
            }
            if change == "reselected" {
                composer.set_attachment(file.clone());
            }
            if change == "quote" {
                composer.set_reply(crate::domain::ReplyReference {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    message_id: "9001".into(),
                    sender: "22".into(),
                    text: "Trích dẫn mới".into(),
                });
            }
            let caption = composer.text.clone();
            let quote = composer.reply().cloned();
            let event = file_accepted(1);
            composer.apply_live(&event, "11");
            assert_eq!(composer.text, caption);
            assert_eq!(composer.reply(), quote.as_ref());
            assert!(!composer.in_flight());
            match change {
                "new-file" => assert_eq!(composer.attachment().unwrap().name, "B.pdf"),
                "reselected" => assert_eq!(composer.attachment(), Some(&file)),
                _ => assert!(composer.attachment().is_none()),
            }
        }
    }

    #[test]
    fn file_queue_failure_terminal_failure_and_quote_combination_preserve_the_draft() {
        let files = FileFixtures::new();
        let file = files.attachment("A.pdf");
        let mut composer = super::super::Composer {
            text: "Chữ chưa gửi".into(),
            ..Default::default()
        };
        composer.set_attachment(file.clone());
        assert_eq!(
            composer.send_live_file(1, "d:22".into(), |_| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert!(!composer.in_flight());
        assert_eq!(composer.attachment(), Some(&file));
        assert_eq!(
            composer.send_live(1, "d:22".into(), |_| panic!(
                "text send cannot discard an attachment"
            )),
            Err(SubmitError::Invalid)
        );
        composer.set_reply(crate::domain::ReplyReference {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: "9001".into(),
            sender: "22".into(),
            text: "Tin gốc".into(),
        });
        assert_eq!(
            composer.send_live_file(1, "d:22".into(), |_| panic!(
                "file and quote combination must be refused"
            )),
            Err(SubmitError::Invalid)
        );
        assert!(composer.reply().is_some());
        assert_eq!(composer.attachment(), Some(&file));
        composer.clear_reply();
        for phase in [
            FileTransferPhase::Cancelled,
            FileTransferPhase::Failed,
            FileTransferPhase::Unknown,
            FileTransferPhase::AcceptedLocalFailure,
        ] {
            let mut request = 0;
            composer
                .send_live_file(1, "d:22".into(), |command| {
                    if let LiveCommand::SendFile { request_id, .. } = command {
                        request = request_id;
                    }
                    Ok(())
                })
                .unwrap();
            composer.apply_live(
                &LiveEvent::FileTransfer {
                    kind: crate::app::live::TransferKind::File,
                    account_id: "11".into(),
                    thread: "d:22".into(),
                    request_id: request,
                    phase,
                    uploaded: 32,
                    total: 32,
                    message: None,
                },
                "11",
            );
            assert!(!composer.in_flight());
            assert_eq!(composer.attachment(), Some(&file));
            assert_eq!(composer.text, "Chữ chưa gửi");
        }
    }

    #[test]
    fn privacy_redacted_file_acceptance_settles_only_its_scoped_attachment() {
        let files = FileFixtures::new();
        let original = files.attachment("A.pdf");
        let replacement = files.attachment("B.pdf");
        for change in ["none", "caption", "new-file", "quote"] {
            let mut drafts = DraftBook::default();
            drafts.select("d:22").unwrap();
            drafts.active.text = "Chữ đang soạn".into();
            drafts.active.set_attachment(original.clone());
            drafts
                .active
                .send_live_file(1, "d:22".into(), |_| Ok(()))
                .unwrap();
            if change == "caption" {
                drafts.active.text = "Chữ mới được giữ lại".into();
                drafts.active.edited();
            }
            if change == "new-file" {
                drafts.active.set_attachment(replacement.clone());
            }
            if change == "quote" {
                drafts.active.set_reply(crate::domain::ReplyReference {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    message_id: "9901".into(),
                    sender: "22".into(),
                    text: "Trích dẫn mới".into(),
                });
            }
            let caption = drafts.active.text.clone();
            let quote = drafts.active.reply().cloned();
            let proof = |account: &str, thread: &str| LiveEvent::FileTransfer {
                kind: crate::app::live::TransferKind::File,
                account_id: account.into(),
                thread: thread.into(),
                request_id: 1,
                phase: FileTransferPhase::Accepted,
                uploaded: 0,
                total: 0,
                message: None,
            };
            drafts.active.apply_live(&proof("33", "d:22"), "11");
            drafts.active.apply_live(&proof("11", "g:90"), "11");
            assert!(
                drafts.active.in_flight(),
                "foreign proof must not settle the file"
            );
            // Privacy revocation parks the hidden draft without exposing its pane.
            drafts.park_active();
            drafts.select("g:90").unwrap();
            drafts.active.text = "Bản nháp hội thoại khác".into();
            drafts.active.set_attachment(replacement.clone());
            let accepted = proof("11", "d:22");
            drafts.apply_live(&accepted, "11");
            assert_eq!(drafts.active.text, "Bản nháp hội thoại khác");
            assert_eq!(drafts.active.attachment(), Some(&replacement));
            drafts.select("d:22").unwrap();
            assert_eq!(drafts.active.text, caption);
            assert_eq!(drafts.active.reply(), quote.as_ref());
            assert!(!drafts.active.in_flight());
            if change == "new-file" {
                assert_eq!(drafts.active.attachment(), Some(&replacement));
            } else {
                assert!(drafts.active.attachment().is_none());
            }
            let mut model = Model::default();
            assert!(!model.apply_live(accepted, "11", true));
            assert!(model.messages.is_empty() && model.conversations.is_empty());
        }
    }

    #[test]
    fn parked_file_progress_and_ack_do_not_mutate_another_threads_draft() {
        let files = FileFixtures::new();
        let file = files.attachment("A.pdf");
        let mut drafts = DraftBook::default();
        drafts.select("d:22").unwrap();
        drafts.active.text = "Chữ hội thoại A".into();
        drafts.active.set_attachment(file.clone());
        drafts
            .active
            .send_live_file(1, "d:22".into(), |_| Ok(()))
            .unwrap();
        drafts.select("g:22").unwrap();
        drafts.active.text = "Chữ hội thoại B".into();
        let other = files.attachment("B.pdf");
        drafts.active.set_attachment(other.clone());
        drafts.apply_live(
            &LiveEvent::FileTransfer {
                kind: crate::app::live::TransferKind::File,
                account_id: "11".into(),
                thread: "d:22".into(),
                request_id: 1,
                phase: FileTransferPhase::Uploading,
                uploaded: 16,
                total: 32,
                message: None,
            },
            "11",
        );
        let accepted = file_accepted(1);
        drafts.apply_live(&accepted, "33");
        drafts.select("d:22").unwrap();
        assert!(drafts.active.in_flight());
        drafts.select("g:22").unwrap();
        drafts.apply_live(&accepted, "11");
        assert_eq!(drafts.active.text, "Chữ hội thoại B");
        assert_eq!(drafts.active.attachment(), Some(&other));
        drafts.select("d:22").unwrap();
        assert_eq!(drafts.active.text, "Chữ hội thoại A");
        assert!(drafts.active.attachment().is_none());
        assert!(!drafts.active.in_flight());
    }

    #[test]
    fn reply_ack_clears_only_the_unchanged_submitted_text_and_quote() {
        let target = |id: &str| crate::domain::ReplyReference {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: id.into(),
            sender: "22".into(),
            text: format!("Tin gốc {id}"),
        };
        for changed in [false, true] {
            let mut composer = super::super::Composer {
                text: "Nội dung trả lời A".into(),
                ..Default::default()
            };
            composer.set_reply(target("9001"));
            composer
                .send_live(1, "d:22".into(), |command| {
                    let LiveCommand::Send { reply_to, text, .. } = command else {
                        panic!("reply must use the scoped send command");
                    };
                    assert_eq!(reply_to.as_deref(), Some("9001"));
                    assert_eq!(text, "Nội dung trả lời A");
                    Ok(())
                })
                .unwrap();
            if changed {
                composer.text = "Nội dung trả lời B".into();
                composer.edited();
                composer.set_reply(target("9002"));
            }
            composer.apply_live(&sent(1), "11");
            assert!(!composer.in_flight());
            if changed {
                assert_eq!(composer.text, "Nội dung trả lời B");
                assert_eq!(composer.reply(), Some(&target("9002")));
            } else {
                assert!(composer.text.is_empty());
                assert!(composer.reply().is_none());
            }
        }
    }

    #[test]
    fn cancel_reply_retains_text_and_invalidates_the_old_ack_revision() {
        let mut composer = super::super::Composer {
            text: "Giữ nội dung này khi hủy trích dẫn".into(),
            ..Default::default()
        };
        composer.set_reply(crate::domain::ReplyReference {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: "9001".into(),
            sender: "22".into(),
            text: "Tin gốc".into(),
        });
        composer.send_live(1, "d:22".into(), |_| Ok(())).unwrap();
        let submitted_revision = composer.revision;
        composer.clear_reply();
        assert_ne!(composer.revision, submitted_revision);
        let cancelled_revision = composer.revision;
        composer.clear_reply();
        assert_eq!(composer.revision, cancelled_revision);
        composer.apply_live(&sent(1), "11");
        assert_eq!(composer.text, "Giữ nội dung này khi hủy trích dẫn");
        assert!(composer.reply().is_none());
        assert!(!composer.in_flight());
    }

    #[test]
    fn quoted_drafts_follow_their_thread_and_ignore_foreign_account_ack() {
        let original = crate::domain::ReplyReference {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: "9001".into(),
            sender: "22".into(),
            text: "Tin gốc trực tiếp".into(),
        };
        let mut drafts = DraftBook::default();
        drafts.select("d:22").unwrap();
        drafts.active.text = "Trả lời hội thoại A".into();
        drafts.active.set_reply(original.clone());
        drafts
            .active
            .send_live(1, "d:22".into(), |_| Ok(()))
            .unwrap();
        drafts.select("g:22").unwrap();
        drafts.active.text = "Bản nháp nhóm B".into();
        drafts.apply_live(&sent(1), "33");
        drafts.select("d:22").unwrap();
        assert!(drafts.active.in_flight());
        assert_eq!(drafts.active.reply(), Some(&original));
        assert_eq!(drafts.active.text, "Trả lời hội thoại A");
        assert_eq!(
            drafts.active.send_live(2, "g:22".into(), |_| {
                panic!("a foreign-thread reply must never reach the queue");
            }),
            Err(SubmitError::Invalid)
        );
        drafts.select("g:22").unwrap();
        drafts.apply_live(&sent(1), "11");
        assert_eq!(drafts.active.text, "Bản nháp nhóm B");
        drafts.select("d:22").unwrap();
        assert!(drafts.active.text.is_empty());
        assert!(drafts.active.reply().is_none());
        assert!(!drafts.active.in_flight());
    }

    #[test]
    fn live_draft_ack_tracks_parked_recipient_and_revision_not_current_editor() {
        let mut drafts = DraftBook::default();
        drafts.select("d:22").unwrap();
        drafts.active.text = "hello".into();
        drafts.active.edited();
        drafts
            .active
            .send_live(1, "d:22".into(), |_| Ok(()))
            .unwrap();
        drafts.select("g:22").unwrap();
        drafts.active.text = "new group draft".into();
        drafts.active.edited();
        drafts.apply_live(&sent(1), "33");
        drafts.select("d:22").unwrap();
        assert_eq!(drafts.active.text, "hello");
        drafts.active.text = "edited".into();
        drafts.active.edited();
        drafts.select("g:22").unwrap();
        drafts.apply_live(&sent(1), "11");
        drafts.select("d:22").unwrap();
        assert_eq!(drafts.active.text, "edited");
        assert!(!drafts.active.in_flight());
    }
    #[test]
    fn busy_live_queue_preserves_selected_pane_generation_and_composer() {
        let mut model = Model {
            generation: 5,
            selected: Some("d:22".into()),
            messages: vec![message()],
            ..Default::default()
        };
        assert_eq!(
            model.select_live_if_accepted("g:22".into(), |_| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert_eq!(model.generation, 5);
        assert_eq!(model.selected.as_deref(), Some("d:22"));
        assert_eq!(model.messages.len(), 1);
        let mut composer = super::super::Composer {
            text: "draft".into(),
            ..Default::default()
        };
        assert_eq!(
            composer.send_live(5, "d:22".into(), |_| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert_eq!(composer.text, "draft");
        assert!(!composer.in_flight());
    }
    #[test]
    fn known_server_acceptance_pending_local_save_is_truthful_and_does_not_clear_draft() {
        let event = LiveEvent::SendFailed {
            account_id: "11".into(),
            generation: 1,
            thread: "d:22".into(),
            request_id: 1,
            error: "Zalo đã nhận; chưa lưu được xác nhận trên thiết bị. Không gửi lại.".into(),
            unknown_delivery: true,
            server_accepted: true,
        };
        let mut composer = super::super::Composer {
            text: "hello".into(),
            ..Default::default()
        };
        composer.send_live(1, "d:22".into(), |_| Ok(())).unwrap();
        composer.apply_live(&event, "11");
        assert_eq!(composer.text, "hello");
        assert!(!composer.in_flight());
        let mut model = Model {
            generation: 1,
            selected: Some("d:22".into()),
            ..Default::default()
        };
        model.apply_live(event, "11", true);
        assert!(model.status.starts_with("Zalo đã nhận"));
        assert!(!model.status.contains("Chưa xác định"));
    }
}
