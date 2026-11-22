//! One-message deletion confirmation tied to the open conversation and account.
use super::{message_actions::Scope, visuals, NativeApp};
use crate::{
    app::live::{
        DeleteForMeMode, DeleteForMeOutcome, LiveCommand, LiveConnection, LiveEvent,
        LivePrivacyStatus,
    },
    domain::{reply::valid_source_id, Message},
};
use eframe::egui::{self, vec2, RichText};

pub(super) fn menu_label(mode: DeleteForMeMode) -> &'static str {
    match mode {
        DeleteForMeMode::Account => "Xóa chỉ ở phía tôi",
        DeleteForMeMode::Device => "Xóa trên thiết bị này",
    }
}

fn result_label(outcome: DeleteForMeOutcome) -> &'static str {
    match outcome {
        DeleteForMeOutcome::Accepted => "Đã xóa tin nhắn.",
        DeleteForMeOutcome::Rejected => "Yêu cầu xóa tin nhắn bị từ chối.",
        DeleteForMeOutcome::Unavailable => "Tin nhắn không còn đủ điều kiện xóa.",
        DeleteForMeOutcome::Unknown => {
            "Chưa rõ kết quả xóa. Tin nhắn vẫn hiển thị để bạn kiểm tra."
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Subject {
    scope: Scope,
    message: String,
    sender: String,
    mode: DeleteForMeMode,
}

impl Subject {
    fn matches(&self, account: &str, thread: &str, message: &str) -> bool {
        self.scope.account == account && self.scope.thread == thread && self.message == message
    }
}

struct Confirmation {
    subject: Subject,
    result: Option<DeleteForMeOutcome>,
    queue_busy: bool,
}

#[derive(Default)]
pub(super) struct DeleteForMe {
    account_sync: Option<bool>,
    confirmation: Option<Confirmation>,
    flight: Option<(u64, Subject)>,
    last: Option<(Subject, DeleteForMeOutcome)>,
    serial: u64,
}

impl DeleteForMe {
    fn mode(&self) -> Option<DeleteForMeMode> {
        self.account_sync.map(|account_sync| {
            if account_sync {
                DeleteForMeMode::Account
            } else {
                DeleteForMeMode::Device
            }
        })
    }

    pub(super) fn is_open(&self) -> bool {
        self.confirmation.is_some()
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        if self.flight.is_some() {
            Some("Đang xử lý yêu cầu xóa…")
        } else {
            self.last
                .as_ref()
                .map(|(_, outcome)| result_label(*outcome))
        }
    }

    fn settle(&mut self, subject: Subject, outcome: DeleteForMeOutcome) {
        if self
            .flight
            .as_ref()
            .is_some_and(|(_, pending)| *pending == subject)
        {
            self.flight = None;
        }
        if let Some(open) = &mut self.confirmation {
            if open.subject == subject {
                open.result = Some(outcome);
            }
        }
        self.last = Some((subject, outcome));
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::DeleteForMeCapability { account_sync, .. } => {
                self.account_sync = Some(*account_sync);
            }
            LiveEvent::DeleteForMeFinished {
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
            LiveEvent::DeletedForMe {
                account_id,
                thread,
                message_id,
                sender_id,
                ..
            } => {
                let matches = |subject: &Subject| {
                    subject.matches(account_id, thread, message_id) && subject.sender == *sender_id
                };
                let subject = self
                    .flight
                    .as_ref()
                    .map(|(_, subject)| subject)
                    .filter(|s| matches(s))
                    .or_else(|| {
                        self.confirmation
                            .as_ref()
                            .map(|open| &open.subject)
                            .filter(|s| matches(s))
                    })
                    .or_else(|| {
                        self.last
                            .as_ref()
                            .map(|(subject, _)| subject)
                            .filter(|s| matches(s))
                    });
                if let Some(subject) = subject {
                    self.settle(subject.clone(), DeleteForMeOutcome::Accepted);
                }
            }
            _ => {}
        }
    }

    pub(super) fn worker_closed(&mut self) {
        self.account_sync = None;
        self.interrupted();
    }

    pub(super) fn interrupted(&mut self) {
        if let Some((_, subject)) = self.flight.take() {
            self.settle(subject, DeleteForMeOutcome::Unknown);
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope, scene: &str) {
        let mode = if scene == "device" {
            DeleteForMeMode::Device
        } else {
            DeleteForMeMode::Account
        };
        self.account_sync = Some(mode == DeleteForMeMode::Account);
        if scene == "menu" {
            return;
        }
        self.confirmation = Some(Confirmation {
            subject: Subject {
                scope,
                message: "901".into(),
                sender: "22".into(),
                mode,
            },
            result: (scene == "unknown").then_some(DeleteForMeOutcome::Unknown),
            queue_busy: false,
        });
    }
}

impl NativeApp {
    fn delete_for_me_subject(&self, message: &Message, mode: DeleteForMeMode) -> Option<Subject> {
        let scope = self.message_menu_scope()?;
        self.live
            .as_ref()
            .filter(|state| state.privacy.status == LivePrivacyStatus::Ready)?;
        let thread_ok = scope.thread.strip_prefix("d:").is_some_and(|peer| {
            valid_source_id(peer)
                && peer != scope.account
                && (message.sender == scope.account || message.sender == peer)
        }) || scope.thread.strip_prefix("g:").is_some_and(valid_source_id)
            && self
                .model
                .conversations
                .iter()
                .any(|group| group.id == scope.thread && !group.group_left);
        if self.demo
            || self.model.pending
            || !thread_ok
            || !self.privacy_list_visible(&scope.thread)
            || message.account_id != scope.account
            || message.thread_id != scope.thread
            || !valid_source_id(&message.id)
            || !valid_source_id(&message.sender)
            || mode == DeleteForMeMode::Account
                && !message
                    .source_client_id
                    .as_deref()
                    .is_some_and(valid_source_id)
            || message.is_recalled()
            || message.reply_blocked
            || !message.mentions.is_empty()
            || message.quote.is_some()
            || message.media.is_some()
            || message.sticker.is_some()
            || message.sent_at_ms <= 0
            || message.text.trim().is_empty()
            || message.text.starts_with("[Tin nhắn ")
        {
            return None;
        }
        Some(Subject {
            scope,
            message: message.id.clone(),
            sender: message.sender.clone(),
            mode,
        })
    }

    pub(super) fn delete_for_me_menu(&self, message: &Message) -> Option<(DeleteForMeMode, bool)> {
        let state = self.live.as_ref()?;
        let mode = state.delete_for_me.mode()?;
        self.delete_for_me_subject(message, mode)?;
        let enabled = state.worker.is_some()
            && state.delete_for_me.flight.is_none()
            && (mode == DeleteForMeMode::Device || state.connection == LiveConnection::Online);
        Some((mode, enabled))
    }

    pub(super) fn open_delete_for_me(&mut self, id: &str, mode: DeleteForMeMode) {
        let Some(subject) = self
            .model
            .messages
            .iter()
            .find(|message| {
                message.id == id && self.delete_for_me_menu(message) == Some((mode, true))
            })
            .and_then(|message| self.delete_for_me_subject(message, mode))
        else {
            return;
        };
        let state = &mut self.live.as_mut().expect("eligible deletion").delete_for_me;
        let result = state
            .last
            .as_ref()
            .filter(|(last, result)| {
                *result == DeleteForMeOutcome::Unknown
                    && last.matches(
                        &subject.scope.account,
                        &subject.scope.thread,
                        &subject.message,
                    )
                    && last.sender == subject.sender
            })
            .map(|(_, result)| *result);
        state.confirmation = Some(Confirmation {
            subject,
            result,
            queue_busy: false,
        });
    }

    pub(super) fn sync_delete_for_me(&mut self) {
        let Some(state) = self.live.as_ref() else {
            return;
        };
        let Some(open) = &state.delete_for_me.confirmation else {
            return;
        };
        let subject = &open.subject;
        let same_scope = self.message_menu_scope().as_ref() == Some(&subject.scope)
            && state.privacy.status == LivePrivacyStatus::Ready
            && self.privacy_list_visible(&subject.scope.thread);
        let settled_or_pending = open.result.is_some() || state.delete_for_me.flight.is_some();
        let can_continue = settled_or_pending
            || state.delete_for_me.mode() == Some(subject.mode)
                && self.model.messages.iter().any(|message| {
                    message.id == subject.message
                        && self.delete_for_me_subject(message, subject.mode).as_ref()
                            == Some(subject)
                });
        if !same_scope || !can_continue {
            self.live.as_mut().unwrap().delete_for_me.confirmation = None;
            self.modal_focus = None;
        }
    }

    pub(super) fn show_delete_for_me(&mut self, ctx: &egui::Context) {
        self.sync_delete_for_me();
        self.latch_modal_input(ctx);
        let Some(state) = &mut self.live else { return };
        let deletion = &mut state.delete_for_me;
        let Some(mut open) = deletion.confirmation.take() else {
            return;
        };
        let subject = &open.subject;
        let preview = self
            .model
            .messages
            .iter()
            .find(|message| {
                message.id == subject.message
                    && message.thread_id == subject.scope.thread
                    && message.account_id == subject.scope.account
                    && message.sender == subject.sender
            })
            .map(Message::preview_text);
        let ready = open.result.is_none() && deletion.flight.is_none();
        let connected = state.worker.is_some()
            && (subject.mode == DeleteForMeMode::Device
                || state.connection == LiveConnection::Online);
        let mut submit = false;
        let mut close = false;
        let dialog = super::dialog::show(
            ctx,
            "delete_for_me_message",
            menu_label(subject.mode),
            400.0,
            false,
            |ui| {
                super::dialog::body(ui, |ui| {
                    let colors = crate::theme::palette(ctx);
                    if let Some(outcome) = open.result {
                        let color = if outcome == DeleteForMeOutcome::Accepted {
                            colors.text_primary
                        } else {
                            colors.error
                        };
                        ui.add(
                            egui::Label::new(RichText::new(result_label(outcome)).color(color))
                                .wrap(),
                        );
                    } else if !ready {
                        ui.label("Đang xử lý yêu cầu xóa…");
                    } else {
                        ui.add(egui::Label::new(match subject.mode {
                        DeleteForMeMode::Account => "Tin nhắn sẽ bị xóa khỏi lịch sử trên các thiết bị của bạn. Những người khác vẫn xem được tin nhắn.",
                        DeleteForMeMode::Device => "Tin nhắn sẽ bị xóa khỏi lịch sử trên thiết bị này. Các thiết bị khác và người trong cuộc trò chuyện vẫn xem được tin nhắn.",
                    }).wrap());
                        if let Some(preview) = preview {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(preview).color(colors.text_secondary),
                                )
                                .truncate(),
                            );
                        }
                        if !connected {
                            ui.label("Kết nối lại để tiếp tục.");
                        } else if open.queue_busy {
                            ui.colored_label(colors.error, "Chưa gửi yêu cầu xóa. Hãy thử lại.");
                        }
                    }
                });
                super::dialog::footer(ui, |ui| {
                    if ready {
                        submit = visuals::primary_button(ui, connected, "Xóa", vec2(96.0, 44.0))
                            .clicked();
                    }
                    close = ui
                        .add_sized(
                            vec2(72.0, 44.0),
                            egui::Button::new(if ready { "Hủy" } else { "Đóng" }),
                        )
                        .clicked();
                });
            },
        );
        if dialog.closed || close {
            return;
        }
        if let Some(worker) = state
            .worker
            .as_ref()
            .filter(|_| ready && connected && submit)
        {
            let request_id = deletion.serial.wrapping_add(1);
            match worker.submit(LiveCommand::DeleteForMe {
                request_id,
                thread: subject.scope.thread.clone(),
                message_id: subject.message.clone(),
                mode: subject.mode,
            }) {
                Ok(()) => {
                    deletion.serial = request_id;
                    deletion.flight = Some((request_id, subject.clone()));
                    open.queue_busy = false;
                }
                Err(_) => open.queue_busy = true,
            }
        }
        deletion.confirmation = Some(open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "d:22";
    const MESSAGE: &str = "901";

    #[test]
    fn delete_dialog_binds_scope_and_waits_for_matching_settlement() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let (worker, _commands, _events) = crate::app::live::LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.worker = Some(worker);
        state.connection = LiveConnection::Online;
        state
            .delete_for_me
            .observe(&LiveEvent::DeleteForMeCapability {
                account_id: "11".into(),
                account_sync: true,
            });
        app.model.selected = Some(THREAD.into());
        app.model.messages.push(Message {
            account_id: "11".into(),
            thread_id: THREAD.into(),
            id: MESSAGE.into(),
            sender: "22".into(),
            text: "Tin nhắn mẫu".into(),
            sent_at_ms: 1,
            source_client_id: Some("900".into()),
            reply_blocked: false,
            expires_at_ms: None,
            recalled: false,
            mentions: vec![],
            quote: None,
            media: None,
            sticker: None,
        });
        app.drafts.select(THREAD).unwrap();
        app.drafts.active.text = "Nháp cần giữ".into();
        assert_eq!(
            app.delete_for_me_menu(&app.model.messages[0]),
            Some((DeleteForMeMode::Account, true))
        );
        app.open_delete_for_me(MESSAGE, DeleteForMeMode::Device);
        assert!(!app.live.as_ref().unwrap().delete_for_me.is_open());
        app.open_delete_for_me(MESSAGE, DeleteForMeMode::Account);
        app.model.messages[0].sender = "11".into();
        app.sync_delete_for_me();
        assert!(!app.live.as_ref().unwrap().delete_for_me.is_open());
        app.model.messages[0].sender = "22".into();
        app.open_delete_for_me(MESSAGE, DeleteForMeMode::Account);
        let state = app.live.as_mut().unwrap();
        state.delete_for_me.account_sync = Some(false);
        app.sync_delete_for_me();
        assert!(!app.live.as_ref().unwrap().delete_for_me.is_open());
        app.model.messages[0].source_client_id = None;
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Offline;
        assert_eq!(
            app.delete_for_me_menu(&app.model.messages[0]),
            Some((DeleteForMeMode::Device, true))
        );
        app.open_delete_for_me(MESSAGE, DeleteForMeMode::Device);
        let deletion = &mut app.live.as_mut().unwrap().delete_for_me;
        deletion.flight = Some((5, deletion.confirmation.as_ref().unwrap().subject.clone()));
        let finish = |thread: &str, request_id| LiveEvent::DeleteForMeFinished {
            account_id: "11".into(),
            request_id,
            thread: thread.into(),
            message_id: MESSAGE.into(),
            outcome: DeleteForMeOutcome::Unknown,
        };
        deletion.observe(&finish("d:33", 5));
        deletion.observe(&finish(THREAD, 4));
        assert!(deletion.flight.is_some());
        deletion.observe(&finish(THREAD, 5));
        assert_eq!(
            deletion.confirmation.as_ref().unwrap().result,
            Some(DeleteForMeOutcome::Unknown)
        );
        app.open_delete_for_me(MESSAGE, DeleteForMeMode::Device);
        assert_eq!(
            app.live
                .as_ref()
                .unwrap()
                .delete_for_me
                .confirmation
                .as_ref()
                .unwrap()
                .result,
            Some(DeleteForMeOutcome::Unknown)
        );
        assert_eq!(app.model.messages.len(), 1);
        assert_eq!(app.drafts.active.text, "Nháp cần giữ");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
