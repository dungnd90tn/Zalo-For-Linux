//! Read-only poll details are requested explicitly and kept on the current page.
use super::{group_members::Scope, visuals, NativeApp};
use crate::{
    app::live::{LiveCommand, LiveConnection, LiveEvent, LiveWorker},
    domain::{poll::PollDetails, MediaDescriptor, Message},
};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub(super) struct Polls {
    available: bool,
    serial: u64,
    view: Option<View>,
}

struct View {
    scope: Scope,
    message_id: String,
    poll_id: u64,
    request_id: u64,
    state: ViewState,
}

enum ViewState {
    Loading,
    Failed(&'static str),
    Loaded(PollDetails),
}

impl View {
    fn matches(&self, scope: &Scope, message: &Message) -> bool {
        self.scope == *scope
            && message.id == self.message_id
            && poll_id(scope, message) == Some(self.poll_id)
    }
}

fn poll_id(scope: &Scope, message: &Message) -> Option<u64> {
    if message.is_recalled()
        || message.account_id != scope.account
        || message.thread_id != scope.thread
    {
        return None;
    }
    match message.media.as_ref() {
        Some(MediaDescriptor::Poll(poll)) => poll.poll_id,
        _ => None,
    }
}

const LOAD_FAILED: &str = "Chưa tải được bình chọn. Bạn có thể thử lại.";

impl Polls {
    fn sync(&mut self, scope: Option<&Scope>, messages: &[Message]) {
        if self.view.as_ref().is_some_and(|view| {
            scope.is_none_or(|scope| !messages.iter().any(|message| view.matches(scope, message)))
        }) {
            self.view = None;
        }
    }

    pub(super) fn clear_view(&mut self) {
        self.view = None;
    }

    pub(super) fn observe(
        &mut self,
        event: LiveEvent,
        scope: Option<&Scope>,
        messages: &[Message],
    ) {
        match event {
            LiveEvent::PollDetailCapability { available, .. } => {
                self.available = available;
                if !available {
                    self.clear_view();
                }
            }
            LiveEvent::PollDetail {
                account_id,
                thread,
                message_id,
                poll_id,
                request_id,
                generation,
                privacy_revision,
                detail,
                notice,
            } => {
                self.sync(scope, messages);
                let Some(view) = self.view.as_mut().filter(|view| {
                    scope == Some(&view.scope)
                        && matches!(view.state, ViewState::Loading)
                        && view.scope.account == account_id
                        && view.scope.thread == thread
                        && view.scope.generation == generation
                        && view.scope.privacy_revision == privacy_revision
                        && view.message_id == message_id
                        && view.request_id == request_id
                }) else {
                    return;
                };
                view.state = match detail {
                    Some(detail) if poll_id == Some(view.poll_id) => ViewState::Loaded(detail),
                    _ => ViewState::Failed(notice.unwrap_or(LOAD_FAILED)),
                };
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope, message: &Message, detail: PollDetails) {
        self.available = true;
        self.view = Some(View {
            poll_id: poll_id(&scope, message).unwrap(),
            scope,
            message_id: message.id.clone(),
            request_id: 1,
            state: ViewState::Loaded(detail),
        });
    }
}

pub(super) struct Access<'a> {
    pub scope: &'a Scope,
    pub polls: &'a mut Polls,
    pub worker: Option<&'a LiveWorker>,
}

impl Access<'_> {
    pub(super) fn detail(&self, message: &Message) -> Option<&PollDetails> {
        let view = self
            .polls
            .view
            .as_ref()
            .filter(|view| view.matches(self.scope, message))?;
        match &view.state {
            ViewState::Loaded(detail) => Some(detail),
            _ => None,
        }
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui, message: &Message) -> bool {
        let Some(poll_id) = poll_id(self.scope, message).filter(|_| self.polls.available) else {
            return false;
        };
        let current = self
            .polls
            .view
            .as_ref()
            .filter(|view| view.matches(self.scope, message));
        if let Some(view) = current {
            match view.state {
                ViewState::Loading => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        metadata(ui, "Đang tải bình chọn…");
                    });
                    return true;
                }
                ViewState::Failed(notice) => metadata(ui, notice),
                ViewState::Loaded(_) => {}
            }
        }
        let label = match current.map(|view| &view.state) {
            Some(ViewState::Loaded(_)) => "Cập nhật bình chọn",
            Some(ViewState::Failed(_)) => "Thử lại",
            _ => "Xem bình chọn",
        };
        let busy = self
            .polls
            .view
            .as_ref()
            .is_some_and(|view| matches!(view.state, ViewState::Loading));
        let worker = self.worker.filter(|_| !busy);
        let reason = if busy {
            "Đang tải một bình chọn khác."
        } else if worker.is_none() {
            "Tạm thời chưa thể gửi yêu cầu xem bình chọn."
        } else {
            label
        };
        let response = visuals::chip(ui, label, false, worker.is_some(), reason);
        if let Some(worker) = worker.filter(|_| response.clicked()) {
            self.polls.serial = self.polls.serial.wrapping_add(1);
            let request_id = self.polls.serial;
            let command = LiveCommand::LoadPollDetail {
                generation: self.scope.generation,
                thread: self.scope.thread.clone(),
                privacy_revision: self.scope.privacy_revision,
                message_id: message.id.clone(),
                request_id,
            };
            let state = if worker.submit(command).is_ok() {
                ViewState::Loading
            } else {
                ViewState::Failed("Chưa gửi được yêu cầu xem bình chọn. Hãy thử lại.")
            };
            self.polls.view = Some(View {
                scope: self.scope.clone(),
                message_id: message.id.clone(),
                poll_id,
                state,
                request_id,
            });
            ui.ctx().request_repaint();
        }
        true
    }
}

pub(super) fn metadata(ui: &mut egui::Ui, text: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(visuals::TEXT_SMALL)
                .color(colors.text_secondary),
        )
        .wrap(),
    );
}

pub(super) fn options(ui: &mut egui::Ui, detail: &PollDetails) {
    let colors = crate::theme::palette(ui.ctx());
    for option in &detail.options {
        egui::Frame::none()
            .fill(colors.canvas)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.add(egui::Label::new(&option.text).wrap());
                if let Some(votes) = option.votes {
                    metadata(ui, &format!("{votes} phiếu"));
                }
            });
    }
    if detail.options.iter().any(|option| option.votes.is_none()) {
        metadata(ui, "Số phiếu chưa hiển thị.");
    }
}

impl NativeApp {
    pub(super) fn poll_scope(&self, state: &super::live::LiveState) -> Option<Scope> {
        if state.connection != LiveConnection::Online || self.model.pending {
            return None;
        }
        self.group_pin_scope(state)
    }

    pub(super) fn sync_polls(&mut self) {
        let scope = self.live.as_ref().and_then(|state| self.poll_scope(state));
        if let Some(state) = &mut self.live {
            state.polls.sync(scope.as_ref(), &self.model.messages);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{poll::PollOption, PollDescriptor};

    fn scope() -> Scope {
        Scope {
            account: "11".into(),
            auth: Some(4),
            thread: "g:22".into(),
            generation: 8,
            privacy_revision: 3,
        }
    }

    fn message() -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            id: "901".into(),
            sender: "62".into(),
            text: "[Bình chọn]".into(),
            sent_at_ms: 1_700_000_000_000,
            source_client_id: None,
            quote: None,
            mentions: Vec::new(),
            reply_blocked: true,
            recalled: false,
            sticker: None,
            media: Some(MediaDescriptor::Poll(PollDescriptor {
                title: "Đã tạo bình chọn".into(),
                question: "Họp khi nào?".into(),
                anonymous: None,
                poll_id: Some(81),
            })),
        }
    }

    fn details() -> PollDetails {
        PollDetails {
            question: "Thời gian họp nhóm phù hợp?".into(),
            options: vec![
                PollOption {
                    text: "Sáng thứ Hai".into(),
                    votes: Some(0),
                },
                PollOption {
                    text: "Chiều thứ Tư".into(),
                    votes: None,
                },
            ],
            closed: true,
            anonymous: true,
        }
    }

    fn reply(scope: &Scope, message_id: &str, poll_id: u64) -> LiveEvent {
        LiveEvent::PollDetail {
            account_id: scope.account.clone(),
            thread: scope.thread.clone(),
            message_id: message_id.into(),
            poll_id: Some(poll_id),
            request_id: 1,
            generation: scope.generation,
            privacy_revision: scope.privacy_revision,
            detail: Some(details()),
            notice: None,
        }
    }

    #[test]
    fn only_the_current_poll_can_receive_details() {
        let scope = scope();
        let message = message();
        let messages = std::slice::from_ref(&message);
        let mut polls = Polls::default();
        polls.fixture(scope.clone(), &message, details());
        polls.view.as_mut().unwrap().state = ViewState::Loading;
        let older = Scope {
            generation: scope.generation - 1,
            ..scope.clone()
        };
        for event in [reply(&older, "901", 81), reply(&scope, "902", 81)] {
            polls.observe(event, Some(&scope), messages);
            assert!(matches!(
                polls.view.as_ref().unwrap().state,
                ViewState::Loading
            ));
        }
        polls.observe(reply(&scope, "901", 82), Some(&scope), messages);
        assert!(matches!(
            polls.view.as_ref().unwrap().state,
            ViewState::Failed(LOAD_FAILED)
        ));
        polls.view.as_mut().unwrap().state = ViewState::Loading;
        polls.observe(reply(&scope, "901", 81), Some(&scope), messages);
        assert!(matches!(
            polls.view.as_ref().unwrap().state,
            ViewState::Loaded(_)
        ));
        let next = Scope {
            privacy_revision: scope.privacy_revision + 1,
            ..scope.clone()
        };
        polls.observe(reply(&scope, "901", 81), Some(&next), messages);
        assert!(polls.view.is_none());
    }

    #[test]
    fn loaded_poll_preserves_zero_and_hides_missing_counts() {
        let ctx = egui::Context::default();
        visuals::install(&ctx);
        let scope = scope();
        let message = message();
        let mut polls = Polls::default();
        polls.fixture(scope.clone(), &message, details());
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::super::media::show(
                    ui,
                    &message,
                    scope.generation,
                    None,
                    285.0,
                    Some(Access {
                        scope: &scope,
                        polls: &mut polls,
                        worker: None,
                    }),
                );
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        for label in [
            "Thời gian họp nhóm phù hợp?",
            "Bình chọn ẩn danh · Đã đóng",
            "Sáng thứ Hai",
            "Chiều thứ Tư",
            "Số phiếu chưa hiển thị.",
        ] {
            assert!(labels.contains(&label), "Missing label: {label}");
        }
        assert_eq!(
            labels.iter().filter(|label| **label == "0 phiếu").count(),
            1
        );
        assert!(output.platform_output.open_url.is_none());
    }
}
