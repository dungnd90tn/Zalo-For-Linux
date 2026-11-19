//! Confirmation and scoped settlement for recalling a previously sent direct message.
use super::{visuals, NativeApp};
use crate::{
    app::live::{LiveCommand, LiveConnection, LiveEvent, RecallOutcome},
    domain::{reply::valid_source_id, Message},
};
use eframe::egui::{self, vec2, RichText};

#[derive(Clone, PartialEq, Eq)]
struct Subject {
    account: String,
    auth: Option<u64>,
    thread: String,
    page: u64,
    message: String,
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
            thread: "d:22".into(),
            page,
            message: "901".into(),
        };
        self.confirmation = Some(Confirmation {
            subject,
            result: (scene == "unknown").then_some(RecallOutcome::Unknown),
            busy_queue: false,
        });
    }
}

impl NativeApp {
    fn recall_subject(&self, message: &Message) -> Option<Subject> {
        let account = self.account_id()?;
        let thread = self.model.selected.as_ref()?;
        let peer = thread.strip_prefix("d:")?;
        if self.demo
            || self.model.pending
            || peer == account
            || !self.privacy_list_visible(thread)
            || message.account_id != account
            || message.sender != account
            || message.thread_id != *thread
            || !valid_source_id(peer)
            || !valid_source_id(&message.id)
            || !message
                .source_client_id
                .as_deref()
                .is_some_and(valid_source_id)
            || message.is_recalled()
            || message.reply_blocked
            || message.quote.is_some()
            || message.media.is_some()
            || message.sticker.is_some()
            || message.text.trim().is_empty()
            || message.text.starts_with("[Tin nhắn ")
        {
            return None;
        }
        Some(Subject {
            account: account.into(),
            auth: self.auth_generation(),
            thread: thread.clone(),
            page: self.model.generation,
            message: message.id.clone(),
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
            && self.privacy_list_visible(&subject.thread);
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
            .find(|message| message.id == confirmation.subject.message)
            .map(Message::preview_text);
        let pending = recall.flight.is_some();
        let ready = confirmation.result.is_none();
        let can_submit = ready
            && !pending
            && state.connection == LiveConnection::Online
            && state.worker.is_some();
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
                        ui.add(
                            egui::Label::new(
                                "Thu hồi tin nhắn đối với mọi người trong cuộc trò chuyện?",
                            )
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
                        if confirmation.busy_queue {
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

    #[test]
    fn unrelated_replies_cannot_settle_recall_and_durable_update_can_resolve_unknown() {
        let mut state = Recall::default();
        state.fixture(Some(3), 4, "confirm");
        let subject = state.confirmation.as_ref().unwrap().subject.clone();
        state.flight = Some((10, subject));
        let finish = |account: &str, request_id, outcome| LiveEvent::RecallFinished {
            account_id: account.into(),
            request_id,
            thread: "d:22".into(),
            message_id: "901".into(),
            outcome,
        };
        state.observe(&finish("12", 10, RecallOutcome::Accepted));
        state.observe(&finish("11", 9, RecallOutcome::Accepted));
        assert!(state.flight.is_some());
        state.observe(&finish("11", 10, RecallOutcome::Unknown));
        assert_eq!(
            state.confirmation.as_ref().unwrap().result,
            Some(RecallOutcome::Unknown)
        );
        state.clear_confirmation();
        state.observe(&LiveEvent::Recalled {
            account_id: "11".into(),
            thread: "d:22".into(),
            message_id: "901".into(),
            actor_id: "11".into(),
            message: None,
            conversation: None,
        });
        assert_eq!(state.status_label(), Some("Tin nhắn đã được thu hồi."));
        assert!(!state.is_open());
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

        drafts.remove_recalled_reply("11", "d:22", "901", "11");
        drafts.select("d:22").unwrap();
        assert!(drafts.active.reply().is_some());
        drafts.select("d:33").unwrap();
        drafts.remove_recalled_reply("11", "d:22", "901", "22");
        assert!(drafts.active.reply().is_some());
        drafts.select("d:22").unwrap();
        assert_eq!(drafts.active.text, "Nháp cần giữ");
        assert!(drafts.active.reply().is_none());
    }
}
