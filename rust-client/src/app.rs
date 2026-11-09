//! Bounded UI/storage boundary. SQLite and filesystem work never run on the render thread.
#[cfg(feature = "messaging")]
pub mod directory;
#[cfg(feature = "messaging")]
pub mod download;
#[cfg(feature = "messaging")]
pub mod live;
#[cfg(feature = "messaging")]
pub mod media;
pub mod search;
#[cfg(feature = "live-login")]
pub mod stickers;

use crate::domain::{
    local_message_id, validate_id, Conversation, ConversationOverview, Cursor, Message,
    MAX_PAGE_SIZE, MAX_TEXT_BYTES, OVERVIEW_ITEM_LIMIT,
};
use crate::store::MessageStore;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc;

pub const COMMAND_CAPACITY: usize = 16;
pub const EVENT_CAPACITY: usize = 4;
pub const EVENTS_PER_FRAME: usize = 8;
pub const DEMO_ACCOUNT: &str = "0";
pub const PAGE_SIZE: usize = 50;

#[derive(Debug)]
pub enum Command {
    Load {
        generation: u64,
        thread: String,
        before: Option<Cursor>,
    },
    SendDemo {
        generation: u64,
        thread: String,
        text: String,
        request_id: u64,
    },
    /// Store a file message locally in the offline demo. The file is not read or uploaded:
    /// only its name, extension and size become a stored message.
    SendDemoFile {
        generation: u64,
        thread: String,
        name: String,
        extension: String,
        size_bytes: u64,
        request_id: u64,
    },
    /// Counts and the newest pictures, files and links of one conversation's saved history.
    Overview {
        generation: u64,
        thread: String,
    },
    /// Saved messages of one conversation containing `query`.
    Search {
        request: u64,
        thread: String,
        query: String,
        before: Option<Cursor>,
    },
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Ready(Vec<Conversation>),
    Page {
        generation: u64,
        thread: String,
        messages: Vec<Message>,
        has_older: bool,
    },
    DemoStored {
        generation: u64,
        thread: String,
        id: String,
        request_id: u64,
        conversation: Conversation,
    },
    Overview {
        generation: u64,
        thread: String,
        overview: Box<ConversationOverview>,
    },
    /// The summary could not be read; the conversation itself is unaffected.
    OverviewFailed {
        generation: u64,
        thread: String,
    },
    SearchResults {
        request: u64,
        thread: String,
        search: Box<crate::domain::MessageSearch>,
    },
    SearchFailed {
        request: u64,
        thread: String,
    },
    Fault(String),
    RequestFailed {
        generation: u64,
        thread: String,
        error: String,
        request_id: Option<u64>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Busy,
    Stopped,
    Invalid,
}
impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "Background queue is full. Your draft is preserved; try again.",
            Self::Stopped => "Background worker stopped.",
            Self::Invalid => "Message or conversation is invalid or exceeds the size limit.",
        })
    }
}

pub struct Worker {
    commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// The caller must hold a Profile guard and bind account 0 before opening demo storage.
    pub fn demo(database: PathBuf, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let (commands, mut rx) = mpsc::channel(COMMAND_CAPACITY);
        let (tx, events) = mpsc::channel(EVENT_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("native-storage".into())
            .spawn(move || {
                let emit = |event| {
                    repaint();
                    let sent = tx.blocking_send(event).is_ok();
                    repaint();
                    sent
                };
                let store = MessageStore::open(&database).and_then(|mut store| {
                    seed_demo(&mut store)?;
                    Ok(store)
                });
                let mut store = match store {
                    Ok(s) => s,
                    Err(e) => {
                        emit(Event::Fault(e.to_string()));
                        return;
                    }
                };
                match store.conversations(DEMO_ACCOUNT, MAX_PAGE_SIZE) {
                    Ok(t) => {
                        if !emit(Event::Ready(t)) {
                            return;
                        }
                    }
                    Err(e) => {
                        emit(Event::Fault(e.to_string()));
                        return;
                    }
                }
                while !worker_stop.load(Ordering::Acquire) {
                    let Some(command) = rx.blocking_recv() else {
                        break;
                    };
                    if worker_stop.load(Ordering::Acquire) || matches!(command, Command::Shutdown) {
                        break;
                    }
                    let (generation, thread, request_id, result) = match command {
                        Command::Load {
                            generation,
                            thread,
                            before,
                        } => {
                            let result = store
                                .page(DEMO_ACCOUNT, &thread, before.as_ref(), PAGE_SIZE + 1)
                                .map(|mut messages| {
                                    let has_older = messages.len() > PAGE_SIZE;
                                    if has_older {
                                        messages.remove(0);
                                    }
                                    Event::Page {
                                        generation,
                                        thread: thread.clone(),
                                        messages,
                                        has_older,
                                    }
                                });
                            (generation, thread, None, result)
                        }
                        Command::SendDemo {
                            generation,
                            thread,
                            text,
                            request_id,
                        } => {
                            let result = store_demo_message(&mut store, &thread, &text).map(
                                |(id, conversation)| Event::DemoStored {
                                    generation,
                                    thread: thread.clone(),
                                    id,
                                    request_id,
                                    conversation,
                                },
                            );
                            (generation, thread, Some(request_id), result)
                        }
                        Command::SendDemoFile {
                            generation,
                            thread,
                            name,
                            extension,
                            size_bytes,
                            request_id,
                        } => {
                            let result =
                                store_demo_file(&mut store, &thread, &name, &extension, size_bytes)
                                    .map(|(id, conversation)| Event::DemoStored {
                                        generation,
                                        thread: thread.clone(),
                                        id,
                                        request_id,
                                        conversation,
                                    });
                            (generation, thread, Some(request_id), result)
                        }
                        Command::Overview { generation, thread } => {
                            let event = match store.conversation_overview(
                                DEMO_ACCOUNT,
                                &thread,
                                OVERVIEW_ITEM_LIMIT,
                            ) {
                                Ok(overview) => Event::Overview {
                                    generation,
                                    thread,
                                    overview: Box::new(overview),
                                },
                                Err(_) => Event::OverviewFailed { generation, thread },
                            };
                            if !emit(event) {
                                break;
                            }
                            continue;
                        }
                        Command::Search {
                            request,
                            thread,
                            query,
                            before,
                        } => {
                            let event = match store.search_messages_before(
                                DEMO_ACCOUNT,
                                &thread,
                                &query,
                                crate::domain::SEARCH_RESULT_LIMIT,
                                before.as_ref(),
                            ) {
                                Ok(search) => Event::SearchResults {
                                    request,
                                    thread,
                                    search: Box::new(search),
                                },
                                Err(_) => Event::SearchFailed { request, thread },
                            };
                            if !emit(event) {
                                break;
                            }
                            continue;
                        }
                        Command::Shutdown => break,
                    };
                    let event = result.unwrap_or_else(|e| Event::RequestFailed {
                        generation,
                        thread,
                        error: e.to_string(),
                        request_id,
                    });
                    if !emit(event) {
                        break;
                    }
                }
            })?;
        Ok(Self {
            commands,
            events,
            stop,
            thread: Some(thread),
        })
    }
    pub fn submit(&self, command: Command) -> std::result::Result<(), SubmitError> {
        submit(&self.commands, command)
    }
}

fn submit(
    sender: &mpsc::Sender<Command>,
    command: Command,
) -> std::result::Result<(), SubmitError> {
    let valid = match &command {
        Command::Load { thread, before, .. } => {
            validate_id(thread).is_ok()
                && before.as_ref().is_none_or(|c| validate_id(&c.id).is_ok())
        }
        Command::SendDemo { thread, text, .. } => {
            validate_id(thread).is_ok() && !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES
        }
        Command::SendDemoFile {
            thread,
            name,
            extension,
            size_bytes,
            ..
        } => {
            validate_id(thread).is_ok()
                && !name.is_empty()
                && name.len() <= crate::domain::attachment::MAX_FILE_NAME_BYTES
                && extension.chars().count() <= 32
                && *size_bytes > 0
                && *size_bytes <= crate::domain::MAX_DIRECT_FILE_BYTES
        }
        Command::Overview { thread, .. } => validate_id(thread).is_ok(),
        Command::Search {
            thread,
            query,
            before,
            ..
        } => {
            validate_id(thread).is_ok()
                && crate::domain::search::search_query(query) == Some(query)
                && crate::domain::search::valid_search_cursor(before.as_ref())
        }
        Command::Shutdown => true,
    };
    if !valid {
        return Err(SubmitError::Invalid);
    }
    sender.try_send(command).map_err(|e| match e {
        mpsc::error::TrySendError::Full(_) => SubmitError::Busy,
        mpsc::error::TrySendError::Closed(_) => SubmitError::Stopped,
    })
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Closing the receiver unblocks a worker applying event backpressure.
        self.events.close();
        let _ = self.commands.try_send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn seed_demo(store: &mut MessageStore) -> Result<()> {
    let messages=[
        Message{account_id:DEMO_ACCOUNT.into(),thread_id:"welcome".into(),id:"demo-1".into(),sender:"Demo guide".into(),text:"Offline native client. These messages are synthetic and stay in this demo profile.".into(),sticker:None,media:None,sent_at_ms:1_700_000_000_000},
        Message{account_id:DEMO_ACCOUNT.into(),thread_id:"welcome".into(),id:"demo-2".into(),sender:"Demo guide".into(),text:"Try writing a message. The button stores it locally; it does not send to Zalo.".into(),sticker:None,media:None,sent_at_ms:1_700_000_000_001},
        Message{account_id:DEMO_ACCOUNT.into(),thread_id:"architecture".into(),id:"demo-3".into(),sender:"Demo guide".into(),text:"The UI uses bounded queues. SQLite runs on a dedicated worker, and conversation pages use indexed cursors.".into(),sticker:None,media:None,sent_at_ms:1_700_000_000_002},
    ];
    store.insert_batch(&messages)?;
    store.set_title(DEMO_ACCOUNT, "welcome", "Getting started")?;
    store.set_title(DEMO_ACCOUNT, "architecture", "Architecture")?;
    // Additive v2 fixtures: the original three immutable IDs/bodies remain replay-safe.
    // All names and conversations below are synthetic and never contact a server.
    let rooms = [
        ("demo-design", "Nhóm thiết kế", [
            ("Xưởng thiết kế", "Mình vừa hoàn thiện bản giao diện mới. Cả nhóm xem thử nhé!"),
            ("You (local demo)", "Màu xanh nhẹ và khoảng cách thoáng nhìn dễ chịu hơn nhiều."),
            ("Xưởng thiết kế", "Đúng rồi. Mình giữ danh sách trò chuyện gọn, còn nội dung có nhiều không gian hơn."),
            ("You (local demo)", "Ổn rồi nhé. Chiều nay mình cùng rà lại trải nghiệm trên cửa sổ nhỏ."),
        ], 1_700_001_200_000_i64),
        ("demo-coffee", "Cà phê cuối tuần", [
            ("Bạn minh họa", "Cuối tuần mình dành chút thời gian nghỉ ngơi nhé?"),
            ("You (local demo)", "Được chứ! Một ly cà phê và một cuốn sách là đủ."),
            ("Bạn minh họa", "Mình thích một góc yên tĩnh cạnh cửa sổ."),
            ("You (local demo)", "Hẹn cuối tuần nhé!"),
        ], 1_700_001_000_000_i64),
        ("demo-notes", "Ghi chú cá nhân", [
            ("You (local demo)", "Ý tưởng cho tuần này: giữ mọi thứ đơn giản và tập trung vào trải nghiệm."),
            ("You (local demo)", "Kiểm tra bản nháp khi chuyển hội thoại."),
            ("You (local demo)", "Thử tìm kiếm tiếng Việt có dấu và không dấu."),
            ("You (local demo)", "Những ghi chú này chỉ là dữ liệu minh họa trên thiết bị."),
        ], 1_700_000_800_000_i64),
        ("demo-lab", "Phòng thử nghiệm", [
            ("Trợ lý minh họa", "Chào mừng bạn đến với bản thử nghiệm native."),
            ("You (local demo)", "Mình có thể thử những gì ở đây?"),
            ("Trợ lý minh họa", "Tìm hội thoại, viết bản nháp và lưu tin nhắn cục bộ. Chưa có kết nối tới Zalo."),
            ("You (local demo)", "Rõ rồi, mình sẽ bắt đầu từ những trải nghiệm nhỏ."),
        ], 1_700_000_600_000_i64),
    ];
    let mut additional = Vec::with_capacity(16);
    for (thread, title, entries, start) in rooms {
        for (index, (sender, text)) in entries.into_iter().enumerate() {
            additional.push(Message {
                account_id: DEMO_ACCOUNT.into(),
                thread_id: thread.into(),
                id: format!("{thread}-v2-{index}"),
                sender: sender.into(),
                text: text.into(),
                sticker: None,
                media: None,
                sent_at_ms: start + index as i64 * 60_000,
            });
        }
        store.set_title(DEMO_ACCOUNT, thread, title)?;
    }
    store.insert_batch(&additional)?;
    Ok(())
}
/// Persist a file message in the offline demo. Only metadata is stored; there is no upload and no
/// download address, so the card says the file is not available to open.
fn store_demo_file(
    store: &mut MessageStore,
    thread: &str,
    name: &str,
    extension: &str,
    size_bytes: u64,
) -> Result<(String, Conversation)> {
    use crate::domain::{MediaKind, MediaMessageContent};
    let id = local_message_id()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let params = serde_json::json!({"fileExt": extension, "fileSize": size_bytes.to_string()});
    let content = MediaMessageContent::new(
        MediaKind::File,
        &serde_json::json!({"title": name, "params": params.to_string()}),
    )?;
    let message = Message {
        account_id: DEMO_ACCOUNT.into(),
        thread_id: thread.into(),
        id: id.clone(),
        sender: "You (local demo)".into(),
        text: format!("[Tin nhắn {} chưa được hỗ trợ]", MediaKind::File.as_str()),
        sticker: None,
        media: Some(content.descriptor().clone()),
        sent_at_ms: i64::try_from(now)?,
    };
    let stored = store.receive_batch_with_media(
        DEMO_ACCOUNT,
        &[(message, None, None, Some(content))],
        None,
    )?;
    let (_, conversation) = stored
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("The file message was not stored"))?;
    Ok((id, conversation))
}
fn store_demo_message(
    store: &mut MessageStore,
    thread: &str,
    text: &str,
) -> Result<(String, Conversation)> {
    if text.trim().is_empty() {
        bail!("An empty message cannot be stored");
    }
    let id = local_message_id()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let sent_at_ms = i64::try_from(now)?;
    let conversation = store.insert_with_conversation(&Message {
        account_id: DEMO_ACCOUNT.into(),
        thread_id: thread.into(),
        id: id.clone(),
        sender: "You (local demo)".into(),
        text: text.into(),
        sticker: None,
        media: None,
        sent_at_ms,
    })?;
    Ok((id, conversation))
}

#[derive(Default)]
pub struct Composer {
    pub text: String,
    /// A file chosen to be sent with the next send; it belongs to this draft like the text does.
    attachment: Option<crate::domain::Attachment>,
    next_request_id: u64,
    pending: Option<(u64, u64)>,
    revision: u64,
}
impl Composer {
    pub fn attachment(&self) -> Option<&crate::domain::Attachment> {
        self.attachment.as_ref()
    }
    pub fn set_attachment(&mut self, attachment: crate::domain::Attachment) {
        self.attachment = Some(attachment);
        self.edited();
    }
    pub fn clear_attachment(&mut self) {
        if self.attachment.take().is_some() {
            self.edited();
        }
    }
    pub fn in_flight(&self) -> bool {
        self.pending.is_some()
    }
    pub fn edited(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    /// A failed enqueue leaves both text and pending state untouched.
    pub fn send(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(Command) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        // A chosen file is sent on its own; the typed text stays in the editor for next time.
        if let Some(file) = &self.attachment {
            enqueue(Command::SendDemoFile {
                generation,
                thread,
                name: file.name.clone(),
                extension: file.extension.clone(),
                size_bytes: file.size_bytes,
                request_id,
            })?;
            self.pending = Some((request_id, self.revision));
            return Ok(());
        }
        enqueue(Command::SendDemo {
            generation,
            thread,
            text: self.text.clone(),
            request_id,
        })?;
        self.pending = Some((request_id, self.revision));
        Ok(())
    }
    pub fn apply(&mut self, event: &Event) {
        let Some((pending_id, submitted_revision)) = &self.pending else {
            return;
        };
        match event {
            Event::DemoStored { request_id, .. } if request_id == pending_id => {
                if self.revision == *submitted_revision {
                    // Whatever was submitted is gone from the draft; the other half is untouched.
                    if self.attachment.take().is_none() {
                        self.text.clear();
                    }
                    self.revision = self.revision.wrapping_add(1);
                }
                self.pending = None;
            }
            Event::RequestFailed {
                request_id: Some(request_id),
                ..
            } if request_id == pending_id => {
                self.pending = None;
            }
            Event::Fault(_) => {
                self.pending = None;
            }
            _ => {}
        }
    }
}

/// Drafts and durable acknowledgements remain associated with their original recipient.
#[derive(Default)]
pub struct DraftBook {
    pub active: Composer,
    thread: Option<String>,
    parked: std::collections::HashMap<String, Composer>,
}
impl DraftBook {
    /// Remove the visible composer while retaining its recipient and pending acknowledgement.
    pub fn park_active(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.parked.insert(thread, std::mem::take(&mut self.active));
        }
    }

    pub fn can_select(&self, thread: &str) -> bool {
        self.thread.as_deref() == Some(thread)
            || self.parked.contains_key(thread)
            || self.parked.len() + usize::from(self.thread.is_some()) < MAX_PAGE_SIZE
    }
    pub fn select(&mut self, thread: &str) -> std::result::Result<(), SubmitError> {
        if self.thread.as_deref() == Some(thread) {
            return Ok(());
        }
        if !self.can_select(thread) {
            return Err(SubmitError::Busy);
        }
        let next = self.parked.remove(thread).unwrap_or_default();
        let previous = std::mem::replace(&mut self.active, next);
        if let Some(old_thread) = self.thread.replace(thread.to_owned()) {
            self.parked.insert(old_thread, previous);
        }
        Ok(())
    }
    pub fn apply(&mut self, event: &Event) {
        let thread = match event {
            Event::DemoStored { thread, .. } | Event::RequestFailed { thread, .. } => thread,
            Event::Fault(_) => {
                self.active.apply(event);
                for composer in self.parked.values_mut() {
                    composer.apply(event);
                }
                return;
            }
            _ => return,
        };
        if self.thread.as_deref() == Some(thread) {
            self.active.apply(event);
        } else if let Some(composer) = self.parked.get_mut(thread) {
            composer.apply(event);
        }
    }
}

/// What the information panel shows for the selected conversation. The data belongs to one
/// thread and one selection generation; anything else is ignored or refetched.
#[derive(Default)]
pub(crate) struct OverviewState {
    thread: Option<String>,
    generation: u64,
    pending: bool,
    data: Option<ConversationOverview>,
    /// Loaded-message signature when the data was requested, to notice newer messages.
    signature: Option<(usize, Option<String>)>,
    requested: Option<(usize, Option<String>)>,
    /// Signature of the last failed request, so a failure is not retried every frame.
    failed: Option<(usize, Option<String>)>,
}

#[derive(Default)]
pub struct Model {
    pub conversations: Vec<Conversation>,
    pub selected: Option<String>,
    pub messages: Vec<Message>,
    pub generation: u64,
    pub has_older: bool,
    pub pending: bool,
    pub status: String,
    pub(crate) overview: OverviewState,
    pub(crate) search: search::SearchState,
}
impl Model {
    fn message_signature(&self) -> (usize, Option<String>) {
        (
            self.messages.len(),
            self.messages.last().map(|message| message.id.clone()),
        )
    }
    /// The saved-history summary for `thread`, if one has arrived for the current selection.
    pub fn overview(&self, thread: &str) -> Option<&ConversationOverview> {
        (self.overview.thread.as_deref() == Some(thread)
            && self.overview.generation == self.generation)
            .then_some(self.overview.data.as_ref())
            .flatten()
    }
    /// Whether a summary request for `thread` is waiting for its reply.
    pub fn overview_pending(&self, thread: &str) -> bool {
        self.overview.pending
            && self.overview.generation == self.generation
            && self.overview.thread.as_deref() == Some(thread)
    }
    /// True when a request is worth making: the data is for another thread or selection, or
    /// there is none (and the last attempt did not just fail), or it is older than the loaded
    /// messages. Never true while a request for this very selection is in flight.
    pub fn overview_stale(&self, thread: &str) -> bool {
        let state = &self.overview;
        if state.thread.as_deref() != Some(thread) || state.generation != self.generation {
            return true;
        }
        let signature = Some(self.message_signature());
        if state.pending || state.failed == signature {
            return false;
        }
        state.data.is_none() || state.signature != signature
    }
    /// Record a request after `enqueue` accepted it, passing the selection generation to scope
    /// the reply. Older data for the same thread stays visible while the new one loads.
    pub fn request_overview(
        &mut self,
        thread: &str,
        enqueue: impl FnOnce(u64) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        let generation = self.generation;
        enqueue(generation)?;
        if self.overview.thread.as_deref() != Some(thread) || self.overview.generation != generation
        {
            self.overview.data = None;
            self.overview.signature = None;
        }
        self.overview.thread = Some(thread.to_owned());
        self.overview.generation = generation;
        self.overview.pending = true;
        self.overview.failed = None;
        self.overview.requested = Some(self.message_signature());
        Ok(())
    }
    pub fn apply_overview(
        &mut self,
        generation: u64,
        thread: &str,
        overview: ConversationOverview,
    ) -> bool {
        if generation != self.generation
            || self.selected.as_deref() != Some(thread)
            || self.overview.thread.as_deref() != Some(thread)
            || self.overview.generation != generation
        {
            return false;
        }
        self.overview.data = Some(overview);
        self.overview.signature = self.overview.requested.clone();
        self.overview.failed = None;
        self.overview.pending = false;
        true
    }
    /// A failed request must not block later attempts, nor be retried every frame.
    pub fn overview_failed(&mut self, generation: u64, thread: &str) {
        if self.overview.generation == generation && self.overview.thread.as_deref() == Some(thread)
        {
            self.overview.pending = false;
            self.overview.failed = self.overview.requested.clone();
        }
    }
    pub fn select(&mut self, thread: String) -> Command {
        self.generation = self.generation.wrapping_add(1);
        self.selected = Some(thread.clone());
        self.messages.clear();
        self.has_older = false;
        self.pending = true;
        Command::Load {
            generation: self.generation,
            thread,
            before: None,
        }
    }
    pub fn select_if_accepted(
        &mut self,
        thread: String,
        enqueue: impl FnOnce(Command) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        enqueue(Command::Load {
            generation: self.generation.wrapping_add(1),
            thread: thread.clone(),
            before: None,
        })?;
        self.select(thread);
        Ok(())
    }
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Ready(threads) => {
                self.conversations = threads;
                true
            }
            Event::Page {
                generation,
                thread,
                messages,
                has_older,
            } => {
                if generation != self.generation
                    || self.selected.as_deref() != Some(thread.as_str())
                {
                    return false;
                }
                self.messages = messages;
                self.has_older = has_older;
                self.pending = false;
                true
            }
            Event::DemoStored {
                generation,
                thread,
                conversation,
                ..
            } => {
                if conversation.id != thread {
                    return false;
                }
                if let Some(existing) = self.conversations.iter_mut().find(|c| c.id == thread) {
                    *existing = conversation;
                } else {
                    self.conversations.push(conversation);
                }
                self.conversations.sort_by(|a, b| {
                    b.last_message_at_ms
                        .cmp(&a.last_message_at_ms)
                        .then_with(|| a.id.cmp(&b.id))
                });
                self.conversations.truncate(MAX_PAGE_SIZE);
                if generation == self.generation
                    && self.selected.as_deref() == Some(thread.as_str())
                {
                    self.status =
                        "Stored locally in the offline demo; nothing was sent to Zalo.".into();
                }
                true
            }
            Event::Overview {
                generation,
                thread,
                overview,
            } => self.apply_overview(generation, &thread, *overview),
            Event::OverviewFailed { generation, thread } => {
                self.overview_failed(generation, &thread);
                false
            }
            Event::SearchResults {
                request,
                thread,
                search,
            } => self.apply_search(request, &thread, *search),
            Event::SearchFailed { request, thread } => self.apply_search_failed(request, &thread),
            Event::Fault(error) => {
                self.pending = false;
                self.status = error;
                true
            }
            Event::RequestFailed {
                generation,
                thread,
                error,
                ..
            } => {
                if generation != self.generation
                    || self.selected.as_deref() != Some(thread.as_str())
                {
                    return false;
                }
                self.pending = false;
                self.status = error;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(thread: &str) -> Conversation {
        Conversation {
            id: thread.into(),
            title: thread.into(),
            last_message_at_ms: 1,
            last_text: "stored fixture".into(),
            last_sender: "You (local demo)".into(),
        }
    }
    #[test]
    fn switching_conversations_preserves_recipient_drafts_and_routes_late_ack() {
        let mut drafts = DraftBook::default();
        drafts.select("a").unwrap();
        drafts.active.text = "draft for A".into();
        let mut request_id = 0;
        drafts
            .active
            .send(1, "a".into(), |command| {
                if let Command::SendDemo { request_id: id, .. } = command {
                    request_id = id;
                }
                Ok(())
            })
            .unwrap();
        drafts.select("b").unwrap();
        assert!(drafts.active.text.is_empty());
        drafts.active.text = "draft for B".into();
        drafts.apply(&Event::DemoStored {
            generation: 1,
            thread: "a".into(),
            conversation: snapshot("a"),
            id: "stored-a".into(),
            request_id,
        });
        assert_eq!(drafts.active.text, "draft for B");
        drafts.select("a").unwrap();
        assert!(drafts.active.text.is_empty());
        assert!(!drafts.active.in_flight());
        drafts.select("b").unwrap();
        assert_eq!(drafts.active.text, "draft for B");
    }
    #[test]
    fn recipient_draft_bank_is_bounded() {
        let mut drafts = DraftBook::default();
        for i in 0..MAX_PAGE_SIZE {
            drafts.select(&format!("thread-{i}")).unwrap();
        }
        assert_eq!(drafts.select("overflow"), Err(SubmitError::Busy));
        assert!(drafts.select("thread-0").is_ok());
    }
    #[test]
    fn busy_selection_retains_current_thread_page_and_pending_request() {
        let mut model = Model::default();
        model.select("a".into());
        let generation = model.generation;
        assert_eq!(
            model.select_if_accepted("b".into(), |_| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert_eq!(model.selected.as_deref(), Some("a"));
        assert_eq!(model.generation, generation);
        assert!(model.pending);
    }
    #[test]
    fn drafts_survive_backpressure_storage_failure_and_edits_before_durable_ack() {
        let mut composer = Composer {
            text: "important draft".into(),
            ..Default::default()
        };
        assert_eq!(
            composer.send(1, "a".into(), |_| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert_eq!(composer.text, "important draft");
        assert!(!composer.in_flight());
        let mut id = 0;
        composer
            .send(1, "a".into(), |command| {
                if let Command::SendDemo { request_id, .. } = command {
                    id = request_id;
                }
                Ok(())
            })
            .unwrap();
        composer.apply(&Event::RequestFailed {
            generation: 1,
            thread: "a".into(),
            error: "disk full".into(),
            request_id: Some(id),
        });
        assert_eq!(composer.text, "important draft");
        assert!(!composer.in_flight());
        composer
            .send(1, "a".into(), |command| {
                if let Command::SendDemo { request_id, .. } = command {
                    id = request_id;
                }
                Ok(())
            })
            .unwrap();
        composer.text = "new edits".into();
        composer.edited();
        composer.apply(&Event::DemoStored {
            generation: 1,
            thread: "a".into(),
            conversation: snapshot("a"),
            id: "m".into(),
            request_id: id.wrapping_add(1),
        });
        assert!(composer.in_flight());
        composer.apply(&Event::DemoStored {
            generation: 1,
            thread: "a".into(),
            conversation: snapshot("a"),
            id: "m".into(),
            request_id: id,
        });
        assert_eq!(composer.text, "new edits");
        assert!(!composer.in_flight());
        composer
            .send(1, "a".into(), |command| {
                if let Command::SendDemo { request_id, .. } = command {
                    id = request_id;
                }
                Ok(())
            })
            .unwrap();
        composer.apply(&Event::DemoStored {
            generation: 1,
            thread: "a".into(),
            conversation: snapshot("a"),
            id: "m2".into(),
            request_id: id,
        });
        assert!(composer.text.is_empty());
    }
    #[test]
    fn editing_away_and_back_before_ack_does_not_clear_new_revision() {
        let mut composer = Composer {
            text: "same text".into(),
            ..Default::default()
        };
        let mut id = 0;
        composer
            .send(1, "a".into(), |command| {
                if let Command::SendDemo { request_id, .. } = command {
                    id = request_id;
                }
                Ok(())
            })
            .unwrap();
        composer.text = "new text".into();
        composer.edited();
        composer.text = "same text".into();
        composer.edited();
        composer.apply(&Event::DemoStored {
            generation: 1,
            thread: "a".into(),
            conversation: snapshot("a"),
            id: "m".into(),
            request_id: id,
        });
        assert_eq!(composer.text, "same text");
        assert!(!composer.in_flight());
    }
    #[test]
    fn stale_failure_does_not_clear_new_request_pending_state() {
        let mut model = Model::default();
        model.select("a".into());
        let old = model.generation;
        model.select("b".into());
        assert!(!model.apply(Event::RequestFailed {
            generation: old,
            thread: "a".into(),
            error: "old failure".into(),
            request_id: None
        }));
        assert!(model.pending);
        assert!(model.status.is_empty());
    }
    #[test]
    fn demo_worker_searches_saved_history_and_refuses_invalid_queries() {
        let root =
            std::env::temp_dir().join(format!("zalo-search-test-{}", local_message_id().unwrap()));
        let profile = crate::profile::Profile::at_root(&root, "demo").unwrap();
        profile.bind_account(DEMO_ACCOUNT).unwrap();
        let path = profile.paths.data_dir.join("messages.sqlite3");
        let mut worker = Worker::demo(path, Arc::new(|| {})).unwrap();
        let mut model = Model::default();
        assert!(model.apply(worker.events.blocking_recv().unwrap()));
        assert_eq!(
            worker.submit(Command::Search {
                request: 1,
                thread: "welcome".into(),
                query: " untrimmed ".into(),
                before: None,
            }),
            Err(SubmitError::Invalid)
        );
        model
            .request_search("welcome", "OFFLINE", |request, query| {
                worker.submit(Command::Search {
                    request,
                    thread: "welcome".into(),
                    query,
                    before: None,
                })
            })
            .unwrap();
        assert!(model.apply(worker.events.blocking_recv().unwrap()));
        let results = model.search_results("welcome").unwrap();
        assert_eq!(results.query, "OFFLINE");
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].message_id, "demo-1");
        assert_eq!(results.hits[0].jump, None);
        drop(worker);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn demo_worker_search_continues_and_restarts_without_losing_rows() {
        let root =
            std::env::temp_dir().join(format!("zalo-search-pages-{}", local_message_id().unwrap()));
        let profile = crate::profile::Profile::at_root(&root, "demo").unwrap();
        profile.bind_account(DEMO_ACCOUNT).unwrap();
        let path = profile.paths.data_dir.join("messages.sqlite3");
        let mut store = MessageStore::open(&path).unwrap();
        let messages: Vec<_> = (0..75)
            .map(|i| Message {
                account_id: DEMO_ACCOUNT.into(),
                thread_id: "search-pages".into(),
                id: format!("p{i:03}"),
                sender: "fixture".into(),
                text: "hẹn gặp".into(),
                sticker: None,
                media: None,
                sent_at_ms: 10,
            })
            .collect();
        store.insert_batch(&messages).unwrap();
        drop(store);
        let mut worker = Worker::demo(path, Arc::new(|| {})).unwrap();
        let mut model = Model::default();
        assert!(model.apply(worker.events.blocking_recv().unwrap()));
        let mut submit = |model: &mut Model, before| {
            model
                .request_search_before("search-pages", "hen", before, |request, query, before| {
                    worker.submit(Command::Search {
                        request,
                        thread: "search-pages".into(),
                        query,
                        before,
                    })
                })
                .unwrap();
            assert!(model.apply(worker.events.blocking_recv().unwrap()));
        };
        submit(&mut model, None);
        let first = model.search_results("search-pages").unwrap().clone();
        assert_eq!(first.hits.len(), 50);
        submit(&mut model, first.next.clone());
        let second = model.search_results("search-pages").unwrap();
        assert_eq!(second.hits.len(), 25);
        assert!(second.next.is_none());
        let actual: Vec<_> = first
            .hits
            .iter()
            .chain(&second.hits)
            .map(|h| h.message_id.as_str())
            .collect();
        let expected: Vec<_> = messages.iter().rev().map(|m| m.id.as_str()).collect();
        assert_eq!(actual, expected);
        submit(&mut model, None);
        assert_eq!(model.search_results("search-pages"), Some(&first));
        assert_eq!(
            worker.submit(Command::Search {
                request: 100,
                thread: "search-pages".into(),
                query: "hen".into(),
                before: Some(Cursor {
                    sent_at_ms: -1,
                    id: "x".into()
                }),
            }),
            Err(SubmitError::Invalid)
        );
        drop(worker);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn background_worker_round_trip_and_saturated_shutdown() {
        let root =
            std::env::temp_dir().join(format!("zalo-worker-test-{}", local_message_id().unwrap()));
        let profile = crate::profile::Profile::at_root(&root, "demo").unwrap();
        profile.bind_account(DEMO_ACCOUNT).unwrap();
        let path = profile.paths.data_dir.join("messages.sqlite3");
        let mut worker = Worker::demo(path.clone(), Arc::new(|| {})).unwrap();
        assert!(matches!(
            worker.events.blocking_recv(),
            Some(Event::Ready(_))
        ));
        worker
            .submit(Command::SendDemo {
                generation: 1,
                thread: "welcome".into(),
                text: "worker round-trip".into(),
                request_id: 1,
            })
            .unwrap();
        assert!(matches!(
            worker.events.blocking_recv(),
            Some(Event::DemoStored { generation: 1, .. })
        ));
        worker
            .submit(Command::Load {
                generation: 2,
                thread: "welcome".into(),
                before: None,
            })
            .unwrap();
        match worker.events.blocking_recv().unwrap() {
            Event::Page { messages, .. } => {
                assert!(messages.iter().any(|m| m.text == "worker round-trip"))
            }
            event => panic!("{event:?}"),
        }
        // A fixture trigger models a real SQLite write failure after initialization.
        let fixture = rusqlite::Connection::open(&path).unwrap();
        fixture.execute_batch("CREATE TRIGGER reject_demo BEFORE INSERT ON messages
            WHEN NEW.body = 'important failed draft' BEGIN SELECT RAISE(ABORT, 'fixture storage failure'); END;").unwrap();
        drop(fixture);
        let mut composer = Composer {
            text: "important failed draft".into(),
            ..Default::default()
        };
        composer
            .send(2, "welcome".into(), |command| worker.submit(command))
            .unwrap();
        let failure = worker.events.blocking_recv().unwrap();
        assert!(matches!(
            &failure,
            Event::RequestFailed {
                request_id: Some(_),
                ..
            }
        ));
        composer.apply(&failure);
        assert_eq!(composer.text, "important failed draft");
        assert!(!composer.in_flight());
        for generation in 3..COMMAND_CAPACITY as u64 {
            let _ = worker.submit(Command::Load {
                generation,
                thread: "welcome".into(),
                before: None,
            });
        }
        // Drop must close an event channel even when the producer is backpressured.
        drop(worker);
        assert_eq!(
            MessageStore::open(&path)
                .unwrap()
                .page(DEMO_ACCOUNT, "welcome", None, 10)
                .unwrap()
                .len(),
            3
        );
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_worker_late_ack_updates_sidebar_without_changing_selected_pane_or_draft() {
        let root =
            std::env::temp_dir().join(format!("zalo-late-ack-{}", local_message_id().unwrap()));
        let profile = crate::profile::Profile::at_root(&root, "demo").unwrap();
        profile.bind_account(DEMO_ACCOUNT).unwrap();
        let path = profile.paths.data_dir.join("messages.sqlite3");
        let mut worker = Worker::demo(path.clone(), Arc::new(|| {})).unwrap();
        let mut model = Model::default();
        assert!(model.apply(worker.events.blocking_recv().unwrap()));
        model.select("welcome".into());
        let mut drafts = DraftBook::default();
        drafts.select("welcome").unwrap();
        let text = "ế🦀".repeat(crate::domain::MAX_PREVIEW_CHARS);
        drafts.active.text = text.clone();
        drafts
            .active
            .send(model.generation, "welcome".into(), |cmd| worker.submit(cmd))
            .unwrap();
        model
            .select_if_accepted("architecture".into(), |cmd| worker.submit(cmd))
            .unwrap();
        drafts.select("architecture").unwrap();
        drafts.active.text = "New draft in B".into();
        let generation = model.generation;
        let ack = worker.events.blocking_recv().unwrap();
        assert!(
            matches!(&ack,Event::DemoStored{thread,conversation,..} if thread=="welcome" && conversation.last_text==text.chars().take(crate::domain::MAX_PREVIEW_CHARS).collect::<String>())
        );
        drafts.apply(&ack);
        assert!(model.apply(ack));
        assert_eq!(model.conversations[0].id, "welcome");
        assert_eq!(model.conversations[0].last_sender, "You (local demo)");
        assert_eq!(model.selected.as_deref(), Some("architecture"));
        assert_eq!(model.generation, generation);
        assert!(model.pending && model.messages.is_empty() && model.status.is_empty());
        assert_eq!(drafts.active.text, "New draft in B");
        assert!(model.apply(worker.events.blocking_recv().unwrap()));
        assert!(model.messages.iter().all(|m| m.thread_id == "architecture"));
        let sidebar = model.conversations.clone();
        let (tx, _rx) = mpsc::channel(1);
        submit(&tx, Command::Shutdown).unwrap();
        assert_eq!(
            drafts
                .active
                .send(generation, "architecture".into(), |cmd| submit(&tx, cmd)),
            Err(SubmitError::Busy)
        );
        assert_eq!(drafts.active.text, "New draft in B");
        assert!(!drafts.active.in_flight());
        assert_eq!(model.conversations, sidebar);
        drafts.select("welcome").unwrap();
        assert!(drafts.active.text.is_empty() && !drafts.active.in_flight());
        drop(worker);
        assert!(MessageStore::open(&path)
            .unwrap()
            .page(DEMO_ACCOUNT, "welcome", None, 10)
            .unwrap()
            .iter()
            .any(|m| m.text == text));
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn queue_full_is_reported_without_unbounded_growth() {
        let (tx, _rx) = mpsc::channel(1);
        assert!(submit(
            &tx,
            Command::Load {
                generation: 1,
                thread: "a".into(),
                before: None
            }
        )
        .is_ok());
        assert_eq!(
            submit(
                &tx,
                Command::Load {
                    generation: 2,
                    thread: "b".into(),
                    before: None
                }
            ),
            Err(SubmitError::Busy)
        );
        assert_eq!(tx.capacity(), 0);
    }
    #[test]
    fn selecting_another_thread_ignores_stale_reply() {
        let mut model = Model::default();
        model.select("a".into());
        let old = model.generation;
        model.select("b".into());
        assert!(!model.apply(Event::Page {
            generation: old,
            thread: "a".into(),
            messages: vec![],
            has_older: true
        }));
        assert_eq!(model.selected.as_deref(), Some("b"));
        assert!(model.pending);
        assert!(model.apply(Event::Page {
            generation: model.generation,
            thread: "b".into(),
            messages: vec![],
            has_older: false
        }));
        assert!(!model.pending);
    }
    #[test]
    fn replaying_demo_seed_does_not_duplicate_messages() {
        let mut store = MessageStore::in_memory().unwrap();
        seed_demo(&mut store).unwrap();
        seed_demo(&mut store).unwrap();
        assert_eq!(
            store.page(DEMO_ACCOUNT, "welcome", None, 10).unwrap().len(),
            2
        );
    }
    fn summary(total: u64) -> ConversationOverview {
        ConversationOverview {
            total_messages: total,
            ..Default::default()
        }
    }
    fn message(id: &str) -> Message {
        Message {
            account_id: DEMO_ACCOUNT.into(),
            thread_id: "welcome".into(),
            id: id.into(),
            sender: "x".into(),
            text: "t".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        }
    }
    #[test]
    fn overview_is_scoped_to_the_selection_and_refetched_only_when_stale() {
        let mut model = Model {
            selected: Some("welcome".into()),
            messages: vec![message("a")],
            ..Default::default()
        };
        assert!(model.overview_stale("welcome"), "nothing requested yet");
        let mut sent = None;
        model
            .request_overview("welcome", |generation| {
                sent = Some(generation);
                Ok(())
            })
            .unwrap();
        assert_eq!(sent, Some(model.generation));
        assert!(model.overview_pending("welcome"));
        assert!(!model.overview_stale("welcome"), "one request at a time");
        // A reply for another thread or an older selection is ignored.
        assert!(!model.apply_overview(model.generation, "other", summary(9)));
        assert!(!model.apply_overview(model.generation + 1, "welcome", summary(9)));
        assert!(model.overview("welcome").is_none());
        assert!(model.apply_overview(model.generation, "welcome", summary(2)));
        assert_eq!(model.overview("welcome").unwrap().total_messages, 2);
        assert!(!model.overview_pending("welcome"));
        assert!(
            !model.overview_stale("welcome"),
            "fresh until messages change"
        );
        // A newer message makes it stale; the old data stays visible while refetching.
        model.messages.push(message("b"));
        assert!(model.overview_stale("welcome"));
        model.request_overview("welcome", |_| Ok(())).unwrap();
        assert_eq!(model.overview("welcome").unwrap().total_messages, 2);
        // Changing the selection hides it at once.
        model.generation += 1;
        assert!(model.overview("welcome").is_none());
        assert!(model.overview_stale("welcome"));
    }
    #[test]
    fn rejected_or_failed_overview_requests_do_not_stick_or_spin() {
        let mut model = Model {
            selected: Some("welcome".into()),
            ..Default::default()
        };
        let busy = model.request_overview("welcome", |_| Err(SubmitError::Busy));
        assert_eq!(busy, Err(SubmitError::Busy));
        assert!(
            !model.overview_pending("welcome"),
            "a refused request is not pending"
        );
        assert!(model.overview_stale("welcome"), "and may be retried");
        model.request_overview("welcome", |_| Ok(())).unwrap();
        model.overview_failed(model.generation, "welcome");
        assert!(!model.overview_pending("welcome"));
        assert!(
            !model.overview_stale("welcome"),
            "a failure is not retried until the messages change"
        );
        model.messages.push(message("c"));
        assert!(model.overview_stale("welcome"));
    }
    #[test]
    fn a_demo_file_message_is_stored_as_a_file_card_without_an_address() {
        let mut store = MessageStore::in_memory().unwrap();
        let (id, conversation) =
            store_demo_file(&mut store, "welcome", "bao-cao.pdf", "pdf", 2048).unwrap();
        assert_eq!(conversation.id, "welcome");
        assert_eq!(conversation.last_text, "bao-cao.pdf");
        let page = store.page(DEMO_ACCOUNT, "welcome", None, 10).unwrap();
        let stored = page.iter().find(|message| message.id == id).unwrap();
        match stored.media.as_ref().unwrap() {
            crate::domain::MediaDescriptor::File(file) => {
                assert_eq!(file.title, "bao-cao.pdf");
                assert_eq!(file.extension, "pdf");
                assert_eq!(file.size_bytes, Some(2048));
                assert!(file.url.is_none() && !file.encrypted);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(stored.sender, "You (local demo)");
    }
}
