//! Confirmation and scoped settlement for recalling an ordinary direct or group message.
use super::{visuals, NativeApp};
use crate::{
    app::live::{LiveCommand, LiveConnection, LiveEvent, LivePrivacyStatus, RecallOutcome},
    domain::{reply::valid_source_id, MediaDescriptor, Message},
};
use eframe::egui::{self, vec2, RichText};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Photo,
    File,
}

impl Kind {
    fn of(message: &Message) -> Option<Self> {
        match &message.media {
            // Live media has a projected source ID only after matching a
            // committed PHOTO or FILE receipt in the worker.
            Some(MediaDescriptor::Photo(photo))
                if message.reply_blocked && photo.original_url.is_some() =>
            {
                Some(Self::Photo)
            }
            Some(MediaDescriptor::File(file))
                if message.reply_blocked
                    && file.url.is_some()
                    && !file.encrypted
                    && !file.is_folder =>
            {
                Some(Self::File)
            }
            None if !message.reply_blocked
                && !message.text.trim().is_empty()
                && !message.text.starts_with("[Tin nhắn ") =>
            {
                Some(Self::Text)
            }
            _ => None,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Text => "tin nhắn",
            Self::Photo => "ảnh này",
            Self::File => "tệp này",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Subject {
    account: String,
    auth: Option<u64>,
    thread: String,
    page: u64,
    message: String,
    source_client_id: String,
    kind: Kind,
}

impl Subject {
    fn matches(&self, account: &str, thread: &str, message: &str) -> bool {
        self.account == account && self.thread == thread && self.message == message
    }
}

struct Confirmation {
    subject: Subject,
    result: Option<RecallOutcome>,
    busy_queue: bool,
}

#[derive(Default)]
pub(super) struct Recall {
    confirmation: Option<Confirmation>,
    flight: Option<(u64, Subject)>,
    last: Option<(Subject, RecallOutcome)>,
    serial: u64,
}

fn label(outcome: RecallOutcome) -> &'static str {
    match outcome {
        RecallOutcome::Accepted => "Tin nhắn đã được thu hồi.",
        RecallOutcome::Rejected => "Zalo từ chối thu hồi tin nhắn.",
        RecallOutcome::Unavailable => "Tin nhắn không còn đủ điều kiện thu hồi.",
        RecallOutcome::Unknown => {
            "Chưa xác nhận được kết quả thu hồi. Hãy kiểm tra hội thoại để xác nhận."
        }
    }
}

impl Recall {
    pub(super) fn is_open(&self) -> bool {
        self.confirmation.is_some()
    }

    pub(super) fn clear_confirmation(&mut self) {
        self.confirmation = None;
    }

    fn settle(&mut self, subject: Subject, outcome: RecallOutcome) {
        self.flight = None;
        if let Some(confirmation) = self
            .confirmation
            .as_mut()
            .filter(|confirmation| confirmation.subject == subject)
        {
            confirmation.result = Some(outcome);
        }
        self.last = Some((subject, outcome));
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::RecallFinished {
                account_id,
                request_id,
                thread,
                message_id,
                outcome,
                ..
            } => {
                if let Some((id, subject)) = &self.flight {
                    if id == request_id && subject.matches(account_id, thread, message_id) {
                        self.settle(subject.clone(), *outcome);
                    }
                }
            }
            LiveEvent::Recalled {
                account_id,
                thread,
                message_id,
                actor_id,
                ..
            } if actor_id == account_id => {
                // Cross-device recall can prove an unknown result later. Only the
                // exact local subject is eligible for the corresponding notice.
                let subject = self
                    .flight
                    .as_ref()
                    .map(|(_, subject)| subject)
                    .or_else(|| self.last.as_ref().map(|(subject, _)| subject))
                    .or_else(|| self.confirmation.as_ref().map(|current| &current.subject));
                if let Some(subject) =
                    subject.filter(|subject| subject.matches(account_id, thread, message_id))
                {
                    self.settle(subject.clone(), RecallOutcome::Accepted);
                }
            }
            _ => {}
        }
    }

    pub(super) fn worker_closed(&mut self) {
        if let Some((_, subject)) = self.flight.take() {
            self.settle(subject, RecallOutcome::Unknown);
        }
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        if self.flight.is_some() {
            return Some("Đang chờ Zalo xác nhận thu hồi…");
        }
        self.last.as_ref().map(|(_, outcome)| label(*outcome))
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, auth: Option<u64>, page: u64, scene: &str) {
        let subject = Subject {
            account: "11".into(),
            auth,
            thread: if scene.starts_with("group-") {
                "g:70"
            } else {
                "d:22"
            }
            .into(),
            page,
            message: "901".into(),
            source_client_id: "900".into(),
            kind: if scene.contains("photo") {
                Kind::Photo
            } else if scene.contains("file") {
                Kind::File
            } else {
                Kind::Text
            },
        };
        self.confirmation = Some(Confirmation {
            subject,
            result: scene.ends_with("unknown").then_some(RecallOutcome::Unknown),
            busy_queue: false,
        });
    }
}

impl NativeApp {
    fn recall_thread(&self, thread: &str, account: &str) -> bool {
        if !self.privacy_list_visible(thread) {
            return false;
        }
        if let Some(peer) = thread.strip_prefix("d:") {
            return peer != account && valid_source_id(peer);
        }
        thread.strip_prefix("g:").is_some_and(valid_source_id)
            && self
                .model
                .conversations
                .iter()
                .any(|group| group.id == thread && !group.group_left)
            && self.live.as_ref().is_some_and(|state| {
                state.privacy.status == LivePrivacyStatus::Ready && state.group_recall_available()
            })
    }

    fn recall_subject(&self, message: &Message) -> Option<Subject> {
        let account = self.account_id()?;
        let thread = self.model.selected.as_ref()?;
        let source_client_id = message
            .source_client_id
            .as_deref()
            .filter(|id| valid_source_id(id))?;
        let kind = Kind::of(message)?;
        if self.demo
            || self.model.pending
            || !self.recall_thread(thread, account)
            || message.account_id != account
            || message.sender != account
            || message.thread_id != *thread
            || !valid_source_id(&message.id)
            || message.is_recalled()
            || !message.mentions.is_empty()
            || message.quote.is_some()
            || message.sticker.is_some()
            || message.sent_at_ms <= 0
        {
            return None;
        }
        Some(Subject {
            account: account.into(),
            auth: self.auth_generation(),
            thread: thread.clone(),
            page: self.model.generation,
            message: message.id.clone(),
            source_client_id: source_client_id.into(),
            kind,
        })
    }

    pub(super) fn can_recall(&self, message: &Message) -> bool {
        self.live.as_ref().is_some_and(|state| {
            state.connection == LiveConnection::Online
                && state.worker.is_some()
                && state.recall.flight.is_none()
                && self.recall_subject(message).is_some()
        })
    }

    pub(super) fn recall_unavailable(&self) -> &'static str {
        if self.selected_group_left() {
            "Bạn đã rời nhóm. Chỉ có thể xem lịch sử trên máy."
        } else if self.live.as_ref().is_none_or(|state| {
            state.connection != LiveConnection::Online || state.worker.is_none()
        }) {
            "Kết nối lại để thu hồi tin nhắn."
        } else if self
            .model
            .selected
            .as_deref()
            .is_some_and(|thread| thread.starts_with("g:"))
            && !self
                .live
                .as_ref()
                .is_some_and(|state| state.group_recall_available())
        {
            "Phiên này chưa đủ điều kiện thu hồi tin nhắn nhóm."
        } else {
            "Chỉ thu hồi tin văn bản, ảnh hoặc tệp bạn đã gửi, đủ dữ liệu gốc và còn trong thời hạn."
        }
    }

    pub(super) fn open_recall(&mut self, id: &str) {
        let subject = self
            .model
            .messages
            .iter()
            .find(|message| message.id == id)
            .filter(|message| self.can_recall(message))
            .and_then(|message| self.recall_subject(message));
        if let (Some(state), Some(subject)) = (&mut self.live, subject) {
            let result = state
                .recall
                .last
                .as_ref()
                .filter(|(old, outcome)| {
                    *outcome == RecallOutcome::Unknown
                        && old.matches(&subject.account, &subject.thread, &subject.message)
                })
                .map(|(_, outcome)| *outcome);
            state.recall.confirmation = Some(Confirmation {
                subject,
                result,
                busy_queue: false,
            });
        }
    }

    pub(super) fn sync_recall(&mut self) {
        let Some(confirmation) = self
            .live
            .as_ref()
            .and_then(|state| state.recall.confirmation.as_ref())
        else {
            return;
        };
        let subject = &confirmation.subject;
        let same_scope = self.account_id() == Some(subject.account.as_str())
            && self.auth_generation() == subject.auth
            && self.model.selected.as_deref() == Some(subject.thread.as_str())
            && self.model.generation == subject.page
            && self.recall_thread(&subject.thread, &subject.account);
        let eligible = confirmation.result.is_some()
            || self
                .model
                .messages
                .iter()
                .find(|message| message.id == subject.message)
                .and_then(|message| self.recall_subject(message))
                .is_some_and(|current| current == *subject);
        if !(same_scope && eligible) {
            if let Some(state) = &mut self.live {
                state.recall.clear_confirmation();
            }
            self.modal_focus = None;
        }
    }

    pub(super) fn show_recall_confirmation(&mut self, ctx: &egui::Context) {
        self.sync_recall();
        self.latch_modal_input(ctx);
        let Some(state) = &mut self.live else { return };
        let recall = &mut state.recall;
        let Some(mut confirmation) = recall.confirmation.take() else {
            return;
        };
        let preview = self
            .model
            .messages
            .iter()
            .find(|message| {
                let subject = &confirmation.subject;
                message.account_id == subject.account
                    && message.thread_id == subject.thread
                    && message.id == subject.message
                    && message.sender == subject.account
                    && message.source_client_id.as_deref()
                        == Some(subject.source_client_id.as_str())
                    && Kind::of(message) == Some(subject.kind)
            })
            .map(|message| match &message.media {
                Some(MediaDescriptor::Photo(photo)) if !photo.caption.trim().is_empty() => {
                    photo.caption.as_str()
                }
                _ => message.preview_text(),
            });
        let pending = recall.flight.is_some();
        let ready = confirmation.result.is_none();
        let online = state.connection == LiveConnection::Online && state.worker.is_some();
        let can_submit = ready && !pending && online;
        let mut send = false;
        let mut dismiss = false;

        let dialog = super::dialog::show(
            ctx,
            "recall_message",
            "Thu hồi tin nhắn",
            400.0,
            false,
            |ui| {
                super::dialog::body(ui, |ui| {
                    let colors = crate::theme::palette(ctx);
                    if pending {
                        ui.label("Đang chờ Zalo xác nhận thu hồi…");
                    } else if let Some(outcome) = confirmation.result {
                        ui.add(
                            egui::Label::new(RichText::new(label(outcome)).color(
                                if outcome == RecallOutcome::Accepted {
                                    colors.text_primary
                                } else {
                                    colors.error
                                },
                            ))
                            .wrap(),
                        );
                    } else {
                        let audience = if confirmation.subject.thread.starts_with("g:") {
                            "tất cả thành viên trong nhóm"
                        } else {
                            "mọi người trong cuộc trò chuyện"
                        };
                        ui.add(
                            egui::Label::new(format!(
                                "Thu hồi {} đối với {audience}?",
                                confirmation.subject.kind.noun()
                            ))
                            .wrap(),
                        );
                        if let Some(preview) = preview {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(preview).color(colors.text_secondary),
                                )
                                .truncate(),
                            );
                        }
                        if !online {
                            ui.label("Kết nối lại để thu hồi tin nhắn.");
                        } else if confirmation.busy_queue {
                            ui.colored_label(
                                colors.error,
                                "Chưa gửi yêu cầu thu hồi. Hãy thử lại.",
                            );
                        }
                    }
                });
                super::dialog::footer(ui, |ui| {
                    if ready && !pending {
                        send =
                            visuals::primary_button(ui, can_submit, "Thu hồi", vec2(106.0, 44.0))
                                .clicked();
                    }
                    dismiss = ui
                        .add_sized(
                            vec2(72.0, 44.0),
                            egui::Button::new(if ready && !pending { "Hủy" } else { "Đóng" }),
                        )
                        .clicked();
                });
            },
        );
        if dialog.closed || dismiss {
            return;
        }
        if let Some(worker) = state.worker.as_ref().filter(|_| send && can_submit) {
            let request_id = recall.serial.wrapping_add(1);
            let command = LiveCommand::Recall {
                request_id,
                thread: confirmation.subject.thread.clone(),
                message_id: confirmation.subject.message.clone(),
            };
            match worker.submit(command) {
                Ok(()) => {
                    recall.serial = request_id;
                    recall.flight = Some((request_id, confirmation.subject.clone()));
                    confirmation.busy_queue = false;
                }
                Err(_) => confirmation.busy_queue = true,
            }
        }
        recall.confirmation = Some(confirmation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileDescriptor, MediaKind, PhotoAlbumMember, PhotoDescriptor};

    #[test]
    fn committed_photo_and_file_recall_keeps_the_exact_selected_source() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let (worker, _commands, _events) = crate::app::live::LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Bản nháp cần giữ".into();
        let photo = MediaDescriptor::Photo(PhotoDescriptor {
            title: String::new(),
            caption: "Ảnh trong album".into(),
            thumbnail_url: None,
            original_url: Some("https://f12-zpcloud.zdn.vn/synthetic-recall.jpg".into()),
            width: Some(100),
            height: Some(100),
            album: PhotoAlbumMember::new(777, 1, 3),
        });
        let file = MediaDescriptor::File(FileDescriptor {
            title: "Bao cao.pdf".into(),
            extension: "pdf".into(),
            size_bytes: Some(16_384),
            url: Some("https://f12-zfile.zdn.vn/synthetic-recall.pdf".into()),
            is_folder: false,
            encrypted: false,
        });
        for media in [photo, file] {
            let kind = media.kind();
            let mut message = Message {
                account_id: "11".into(),
                thread_id: "d:22".into(),
                id: "901".into(),
                sender: "11".into(),
                text: kind.legacy_placeholder().into(),
                sent_at_ms: 1,
                source_client_id: None,
                reply_blocked: true,
                expires_at_ms: None,
                recalled: false,
                mentions: vec![],
                quote: None,
                media: Some(media),
                sticker: None,
            };
            assert!(!app.can_recall(&message));
            message.source_client_id = Some("900".into());
            let mut foreign = message.clone();
            foreign.sender = "22".into();
            assert!(!app.can_recall(&foreign));
            if let Some(MediaDescriptor::File(descriptor)) = message.media.as_ref() {
                let mut encrypted = descriptor.clone();
                encrypted.encrypted = true;
                let mut candidate = message.clone();
                candidate.media = Some(MediaDescriptor::File(encrypted));
                assert!(!app.can_recall(&candidate));
            }
            assert!(app.can_recall(&message));
            app.model.messages = vec![message];
            app.open_recall("901");
            let state = &app.live.as_ref().unwrap().recall;
            let subject = &state.confirmation.as_ref().unwrap().subject;
            assert!(matches!(
                (kind, subject.kind),
                (MediaKind::Photo, Kind::Photo) | (MediaKind::File, Kind::File)
            ));
            assert_eq!(subject.source_client_id, "900");
            app.model.messages[0].source_client_id = Some("911".into());
            app.sync_recall();
            assert!(!app.live.as_ref().unwrap().recall.is_open());
        }
        assert_eq!(app.drafts.active.text, "Bản nháp cần giữ");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unrelated_replies_cannot_settle_recall_and_durable_update_can_resolve_unknown() {
        let mut state = Recall::default();
        state.fixture(Some(3), 4, "group-confirm");
        let subject = state.confirmation.as_ref().unwrap().subject.clone();
        state.flight = Some((10, subject));
        let finish = |account: &str, request_id, outcome| LiveEvent::RecallFinished {
            account_id: account.into(),
            request_id,
            thread: "g:70".into(),
            message_id: "901".into(),
            outcome,
        };
        state.observe(&finish("12", 10, RecallOutcome::Accepted));
        state.observe(&finish("11", 9, RecallOutcome::Accepted));
        state.observe(&LiveEvent::RecallFinished {
            account_id: "11".into(),
            request_id: 10,
            thread: "d:22".into(),
            message_id: "901".into(),
            outcome: RecallOutcome::Accepted,
        });
        assert!(state.flight.is_some());
        state.observe(&finish("11", 10, RecallOutcome::Unknown));
        assert_eq!(
            state.confirmation.as_ref().unwrap().result,
            Some(RecallOutcome::Unknown)
        );
        state.clear_confirmation();
        state.observe(&LiveEvent::Recalled {
            account_id: "11".into(),
            thread: "g:70".into(),
            message_id: "901".into(),
            actor_id: "11".into(),
            message: None,
            conversation: None,
        });
        assert_eq!(state.status_label(), Some("Tin nhắn đã được thu hồi."));
        assert!(!state.is_open());
    }

    #[test]
    fn group_recall_requires_the_own_message_in_a_joined_visible_group() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let (worker, _commands, _events) = crate::app::live::LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        app.model.selected = Some("g:70".into());
        app.drafts.select("g:70").unwrap();
        app.drafts.active.text = "Nháp đang nhập".into();
        app.model.conversations.push(crate::domain::Conversation {
            id: "g:70".into(),
            title: "Nhóm kiểm thử".into(),
            last_message_at_ms: 1,
            last_text: "Tin nhắn thử".into(),
            last_sender: "11".into(),
            group_left: false,
        });
        app.model.messages.push(Message {
            mentions: Vec::new(),
            reply_blocked: false,
            expires_at_ms: None,
            recalled: false,
            source_client_id: Some("900".into()),
            quote: None,
            account_id: "11".into(),
            thread_id: "g:70".into(),
            id: "901".into(),
            sender: "11".into(),
            text: "Tin nhắn thử".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        });
        let mut other = app.model.messages[0].clone();
        other.sender = "22".into();
        assert!(!app.can_recall(&other));
        assert!(app.can_recall(&app.model.messages[0]));
        app.open_recall("901");
        assert!(app.live.as_ref().unwrap().recall.is_open());
        app.model.conversations[0].group_left = true;
        app.sync_recall();
        assert!(!app.can_recall(&app.model.messages[0]));
        assert!(!app.live.as_ref().unwrap().recall.is_open());
        assert_eq!(app.drafts.active.text, "Nháp đang nhập");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recall_clears_only_the_matching_parked_draft_quote() {
        let mut drafts = crate::app::DraftBook::default();
        let quote = |thread: &str, sender: &str| crate::domain::ReplyReference {
            account_id: "11".into(),
            thread_id: thread.into(),
            message_id: "901".into(),
            sender: sender.into(),
            text: "Tin gốc".into(),
        };
        drafts.select("d:22").unwrap();
        drafts.active.text = "Nháp cần giữ".into();
        drafts.active.set_reply(quote("d:22", "22"));
        drafts.select("d:33").unwrap();
        drafts.active.set_reply(quote("d:33", "33"));

        drafts.remove_message_reply("11", "d:22", "901", "11");
        drafts.select("d:22").unwrap();
        assert!(drafts.active.reply().is_some());
        drafts.select("d:33").unwrap();
        drafts.remove_message_reply("11", "d:22", "901", "22");
        assert!(drafts.active.reply().is_some());
        drafts.select("d:22").unwrap();
        assert_eq!(drafts.active.text, "Nháp cần giữ");
        assert!(drafts.active.reply().is_none());
    }
}
