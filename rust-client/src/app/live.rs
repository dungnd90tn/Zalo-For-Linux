//! Live account worker: bounded commands/events, no rendering-thread I/O.
use super::SubmitError;
pub use crate::protocol::messaging::ThreadKind;
use crate::{
    domain::{Conversation, Cursor, Message, MAX_TEXT_BYTES},
    profile::Profile,
    protocol::session::VerifiedSession,
};
use anyhow::Result;
use std::{sync::Arc, thread::JoinHandle};
use tokio::sync::{mpsc, watch};
mod runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveConnection {
    Connecting,
    Online,
    Reconnecting,
    Offline,
    Expired,
}
#[derive(Debug)]
pub enum LiveCommand {
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
    Send {
        generation: u64,
        thread: String,
        text: String,
        request_id: u64,
    },
    Shutdown,
}
#[derive(Debug)]
pub enum LiveEvent {
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
            Self::Ready { account_id, .. }
            | Self::Opened { account_id, .. }
            | Self::Page { account_id, .. }
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
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn submit(&self, command: LiveCommand) -> std::result::Result<(), SubmitError> {
        use crate::protocol::messaging::Target;
        let valid = match &command {
            LiveCommand::Open {
                kind, remote_id, ..
            } => Target::new(*kind, remote_id).is_ok(),
            LiveCommand::Load { thread, before, .. } => {
                Target::from_thread(thread).is_ok()
                    && before
                        .as_ref()
                        .is_none_or(|c| crate::domain::validate_id(&c.id).is_ok())
            }
            LiveCommand::Send { thread, text, .. } => {
                Target::from_thread(thread).is_ok()
                    && !text.trim().is_empty()
                    && text.len() <= MAX_TEXT_BYTES
            }
            LiveCommand::Shutdown => true,
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
    pub fn send_live(
        &mut self,
        generation: u64,
        thread: String,
        enqueue: impl FnOnce(LiveCommand) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        if self.in_flight() {
            return Err(SubmitError::Busy);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        enqueue(LiveCommand::Send {
            generation,
            thread,
            text: self.text.clone(),
            request_id,
        })?;
        self.pending = Some((request_id, self.revision));
        Ok(())
    }
    pub fn apply_live(&mut self, event: &LiveEvent, account_id: &str) {
        if event.account_id() != account_id {
            return;
        }
        match event {
            LiveEvent::Sent { request_id, .. } => {
                if let Some((pending, revision)) = self.pending {
                    if pending == *request_id {
                        if self.revision == revision {
                            self.text.clear();
                            self.edited();
                        }
                        self.pending = None;
                    }
                }
            }
            LiveEvent::SendFailed { request_id, .. } => {
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
            LiveEvent::Sent { thread, .. } | LiveEvent::SendFailed { thread, .. } => {
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
            LiveEvent::Ready { conversations, .. } => {
                self.conversations = conversations;
                true
            }
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
        self.upsert_live_conversation(conversation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DraftBook, Model};
    fn message() -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "900".into(),
            sender: "11".into(),
            text: "hello".into(),
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
                account_id: "33".into(),
                ..message()
            },
            conversation: conversation(),
        };
        assert!(!m.apply_live(bad, "11", true));
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
