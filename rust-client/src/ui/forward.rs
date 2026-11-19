//! Separate picker and request state for forwarding an ordinary direct text message.
use super::{visuals, NativeApp};
use crate::{
    app::live::{
        ForwardOutcome, ForwardTarget, LiveCommand, LiveConnection, LiveEvent, LiveWorker,
    },
    domain::Message,
};
use eframe::egui::{self, vec2, RichText};

const MAX_PAGE: usize = 25;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    account: String,
    auth: Option<u64>,
    thread: String,
    page: u64,
    message: String,
}

#[derive(Clone, PartialEq, Eq)]
struct Request {
    source: Source,
    target: String,
    id: u64,
}

struct Picker {
    source: Source,
    search: String,
    query: String,
    entries: Vec<ForwardTarget>,
    target: Option<String>,
    lookup_id: Option<u64>,
    result: Option<ForwardOutcome>,
    error: Option<&'static str>,
}

impl Picker {
    fn new(source: Source) -> Self {
        Self {
            source,
            search: String::new(),
            query: String::new(),
            entries: Vec::new(),
            target: None,
            lookup_id: None,
            result: None,
            error: None,
        }
    }

    fn locked(&self, flight: Option<&Request>) -> bool {
        flight.is_some() || self.result == Some(ForwardOutcome::Unknown)
    }

    fn lookup(&mut self, worker: &LiveWorker, id: u64) {
        self.target = None;
        self.entries.clear();
        self.query = self.search.trim().to_owned();
        let command = LiveCommand::ForwardTargets {
            generation: id,
            source_thread: self.source.thread.clone(),
            source_message_id: self.source.message.clone(),
            query: self.query.clone(),
        };
        match worker.submit(command) {
            Ok(()) => {
                self.lookup_id = Some(id);
                self.error = None;
            }
            Err(_) => {
                self.lookup_id = None;
                self.error = Some("Chưa tìm được người nhận. Hãy thử lại.");
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Forwarding {
    picker: Option<Picker>,
    flight: Option<Request>,
    notice: Option<ForwardOutcome>,
    serial: u64,
}

impl Forwarding {
    pub(super) fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    fn id(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }

    pub(super) fn clear_picker(&mut self) {
        self.picker = None;
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::ForwardTargets {
                account_id,
                generation,
                source_thread,
                source_message_id,
                query,
                entries,
                failed,
                ..
            } => {
                if let Some(picker) = self.match_lookup(
                    account_id,
                    *generation,
                    source_thread,
                    source_message_id,
                    query,
                ) {
                    picker.lookup_id = None;
                    if *failed || entries.len() > MAX_PAGE {
                        picker.error = Some("Chưa tìm được người nhận. Hãy thử lại.");
                        return;
                    }
                    picker.entries.clone_from(entries);
                }
            }
            LiveEvent::ForwardFinished {
                account_id,
                request_id,
                source_thread,
                source_message_id,
                target_thread,
                outcome,
                ..
            } => {
                let Some(flight) = &self.flight else { return };
                if flight.id != *request_id
                    || flight.source.account != *account_id
                    || flight.source.thread != *source_thread
                    || flight.source.message != *source_message_id
                    || flight.target != *target_thread
                {
                    return;
                }
                if let Some(picker) = self
                    .picker
                    .as_mut()
                    .filter(|picker| picker.source == flight.source)
                {
                    picker.result = Some(*outcome);
                }
                self.notice = Some(*outcome);
                self.flight = None;
            }
            _ => {}
        }
    }

    fn match_lookup(
        &mut self,
        account: &str,
        generation: u64,
        thread: &str,
        message: &str,
        query: &str,
    ) -> Option<&mut Picker> {
        self.picker.as_mut().filter(|picker| {
            picker.lookup_id == Some(generation)
                && picker.source.account == account
                && picker.source.thread == thread
                && picker.source.message == message
                && picker.query == query
        })
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        if self.flight.is_some() {
            return Some("Đang chờ xác nhận chuyển tiếp…");
        }
        Some(match self.notice? {
            ForwardOutcome::Accepted => "Zalo đã nhận tin nhắn chuyển tiếp.",
            ForwardOutcome::Rejected => "Zalo từ chối chuyển tiếp tin nhắn.",
            ForwardOutcome::Unavailable => "Chưa chuyển tiếp được tin nhắn.",
            ForwardOutcome::Unknown => "Chưa rõ tin nhắn đã chuyển tiếp hay chưa.",
        })
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, account: &str, auth: Option<u64>, page: u64, scene: &str) {
        let mut picker = Picker::new(Source {
            account: account.into(),
            auth,
            thread: "d:22".into(),
            page,
            message: "901".into(),
        });
        picker.entries = vec![
            ForwardTarget {
                thread_id: "d:33".into(),
                title: "Đặng Hà".into(),
            },
            ForwardTarget {
                thread_id: "d:44".into(),
                title: "Lê Minh".into(),
            },
        ];
        picker.target = Some("d:33".into());
        if scene == "unknown" {
            picker.result = Some(ForwardOutcome::Unknown);
        }
        self.picker = Some(picker);
    }

    pub(super) fn worker_closed(&mut self) {
        if self.flight.take().is_some() {
            self.notice = Some(ForwardOutcome::Unknown);
            if let Some(picker) = &mut self.picker {
                picker.result = Some(ForwardOutcome::Unknown);
            }
        }
        if let Some(picker) = &mut self.picker {
            if picker.lookup_id.take().is_some() {
                picker.error = Some("Chưa tìm được người nhận. Hãy thử lại sau khi kết nối.");
            }
        }
    }
}

impl NativeApp {
    fn forward_source(&self, message: &Message) -> Option<Source> {
        let account = self.account_id()?;
        let thread = self.model.selected.as_ref()?;
        let peer = thread.strip_prefix("d:")?;
        if self.demo
            || self.model.pending
            || peer == account
            || (message.sender != account && message.sender != peer)
            || message.thread_id != *thread
            || message.account_id != account
            || !self.privacy_list_visible(thread)
            || message.is_recalled()
            || message.quote.is_some()
            || message.media.is_some()
            || message.sticker.is_some()
            || message.reply_blocked
            || message.source_client_id.is_none()
            || message.text.trim().is_empty()
            || message.text.starts_with("[Tin nhắn ")
        {
            return None;
        }
        Some(Source {
            account: account.into(),
            auth: self.auth_generation(),
            thread: thread.clone(),
            page: self.model.generation,
            message: message.id.clone(),
        })
    }

    pub(super) fn can_forward(&self, message: &Message) -> bool {
        self.live.as_ref().is_some_and(|state| {
            state.connection == LiveConnection::Online
                && state.worker.is_some()
                && state.forwarding.flight.is_none()
                && self.forward_source(message).is_some()
        })
    }

    pub(super) fn open_forward(&mut self, id: &str) {
        let source = self
            .model
            .messages
            .iter()
            .find(|message| message.id == id)
            .filter(|message| self.can_forward(message))
            .and_then(|message| self.forward_source(message));
        if let (Some(state), Some(source)) = (&mut self.live, source) {
            if let Some(worker) = &state.worker {
                let mut picker = Picker::new(source);
                picker.lookup(worker, state.forwarding.id());
                state.forwarding.picker = Some(picker);
            }
        }
    }

    pub(super) fn sync_forward(&mut self) {
        let Some(picker) = self
            .live
            .as_ref()
            .and_then(|state| state.forwarding.picker.as_ref())
        else {
            return;
        };
        let valid = self
            .model
            .messages
            .iter()
            .find(|message| message.id == picker.source.message)
            .and_then(|message| self.forward_source(message))
            .is_some_and(|scope| scope == picker.source);
        if !valid {
            if let Some(state) = &mut self.live {
                state.forwarding.clear_picker();
            }
            self.modal_focus = None;
        }
    }

    pub(super) fn show_forward_picker(&mut self, ctx: &egui::Context) {
        self.sync_forward();
        self.latch_modal_input(ctx);
        let Some(state) = &mut self.live else { return };
        let forwarding = &mut state.forwarding;
        let Some(mut picker) = forwarding.picker.take() else {
            return;
        };
        let Some(message) = self
            .model
            .messages
            .iter()
            .find(|message| message.id == picker.source.message)
        else {
            return;
        };
        let preview = message.preview_text();
        let busy = forwarding.flight.is_some();
        let locked = picker.locked(forwarding.flight.as_ref());
        let accepted = picker.result == Some(ForwardOutcome::Accepted);
        let online = state.connection == LiveConnection::Online && state.worker.is_some();
        // Invalidate the old page's permission as soon as the query changes.
        let mut lookup = false;
        let mut send = false;
        let mut done = false;
        let dialog = super::dialog::show(
            ctx,
            "forward_message",
            "Chuyển tiếp tin nhắn",
            450.0,
            true,
            |ui| {
                let colors = crate::theme::palette(ctx);
                super::dialog::body(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(preview).color(colors.text_secondary))
                            .truncate(),
                    )
                    .on_hover_text(preview);
                    if accepted {
                        ui.label("Zalo đã nhận tin nhắn chuyển tiếp.");
                    } else if matches!(picker.result, Some(ForwardOutcome::Unknown)) {
                        ui.colored_label(colors.error, "Chưa xác định được tin nhắn đã gửi hay chưa. Hãy kiểm tra với người nhận.");
                    } else if busy {
                        ui.label("Đang chờ Zalo xác nhận…");
                    } else {
                        let search = ui.add_sized(
                            vec2(ui.available_width(), 44.0),
                            egui::TextEdit::singleline(&mut picker.search)
                                .id_source("forward_recipient_search")
                                .hint_text("Tìm người nhận")
                                .char_limit(128),
                        );
                        if search.changed() {
                            picker.target = None;
                            picker.entries.clear();
                            picker.result = None;
                            picker.error = None;
                            picker.lookup_id = None;
                        }
                        let changed = picker.search.trim() != picker.query;
                        if ui
                            .add_sized(vec2(88.0, 44.0), egui::Button::new("Tìm"))
                            .clicked()
                        {
                            lookup = true;
                        }
                        if let Some(error) = picker.error {
                            ui.colored_label(colors.error, error);
                        }
                        if picker.result == Some(ForwardOutcome::Unavailable) {
                            ui.colored_label(
                                colors.error,
                                "Hội thoại chưa đủ điều kiện chuyển tiếp.",
                            );
                        }
                        if picker.result == Some(ForwardOutcome::Rejected) {
                            ui.colored_label(colors.error, "Zalo từ chối chuyển tiếp tin nhắn.");
                        }
                        if picker.lookup_id.is_some() {
                            ui.label("Đang tìm người nhận…");
                        } else if changed {
                            ui.label("Chọn Tìm để cập nhật kết quả.");
                        } else if picker.entries.is_empty() && picker.error.is_none() {
                            ui.label("Không tìm thấy liên hệ phù hợp.");
                        }
                        if !changed {
                            let mut offered = false;
                            for entry in &picker.entries {
                                let thread = &entry.thread_id;
                                if thread.strip_prefix("d:").is_none_or(|peer| {
                                    peer.is_empty() || peer == picker.source.account
                                }) || !state.privacy.list_visible(thread)
                                {
                                    continue;
                                }
                                offered |= picker.target.as_ref() == Some(thread);
                                let button = egui::Button::new(&entry.title)
                                    .selected(picker.target.as_ref() == Some(thread))
                                    .truncate();
                                if ui
                                    .add_sized(vec2(ui.available_width(), 44.0), button)
                                    .on_hover_text(&entry.title)
                                    .clicked()
                                {
                                    picker.target = Some(thread.clone());
                                    picker.result = None;
                                    offered = true;
                                }
                            }
                            if !offered {
                                picker.target = None;
                            }
                        }
                        ui.label(
                            RichText::new("Ứng dụng kiểm tra quyền gửi khi bạn chọn Chuyển tiếp.")
                                .size(visuals::TEXT_SMALL)
                                .color(colors.text_secondary),
                        );
                    }
                });
                super::dialog::footer(ui, |ui| {
                    if !accepted && !locked {
                        send = visuals::primary_button(
                            ui,
                            online
                                && picker.target.is_some()
                                && picker.lookup_id.is_none()
                                && !lookup,
                            "Chuyển tiếp",
                            vec2(132.0, 44.0),
                        )
                        .clicked();
                    }
                    done = ui
                        .add_sized(
                            vec2(72.0, 44.0),
                            egui::Button::new(if busy || accepted || locked {
                                "Đóng"
                            } else {
                                "Hủy"
                            }),
                        )
                        .clicked();
                });
            },
        );
        if dialog.closed || done {
            return;
        }
        if let Some(worker) = &state.worker {
            if lookup {
                let id = forwarding.id();
                picker.lookup(worker, id);
            } else if let Some(target) = picker
                .target
                .clone()
                .filter(|target| send && online && state.privacy.list_visible(target))
            {
                let request = Request {
                    id: forwarding.id(),
                    source: picker.source.clone(),
                    target,
                };
                let command = LiveCommand::Forward {
                    request_id: request.id,
                    source_thread: request.source.thread.clone(),
                    source_message_id: request.source.message.clone(),
                    target_thread: request.target.clone(),
                };
                match worker.submit(command) {
                    Ok(()) => {
                        forwarding.flight = Some(request);
                        forwarding.notice = None;
                        picker.result = None;
                        picker.error = None;
                    }
                    Err(_) => picker.error = Some("Chưa gửi yêu cầu chuyển tiếp. Hãy thử lại."),
                }
            }
        }
        forwarding.picker = Some(picker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker() -> Picker {
        Picker::new(Source {
            account: "11".into(),
            auth: Some(3),
            thread: "d:22".into(),
            page: 4,
            message: "901".into(),
        })
    }

    #[test]
    fn changed_or_closed_query_rejects_old_recipient_results() {
        let mut current = picker();
        current.query = "Hà".into();
        current.lookup_id = Some(9);
        let mut view = Forwarding {
            picker: Some(current),
            ..Default::default()
        };
        let results = |account: &str, generation, query: &str| LiveEvent::ForwardTargets {
            account_id: account.into(),
            generation,
            source_thread: "d:22".into(),
            source_message_id: "901".into(),
            query: query.into(),
            entries: vec![ForwardTarget {
                thread_id: "d:33".into(),
                title: "Đặng Hà".into(),
            }],
            failed: false,
        };
        view.observe(&results("12", 9, "Hà"));
        view.observe(&results("11", 8, "Hà"));
        view.observe(&results("11", 9, "Mai"));
        assert!(view.picker.as_ref().unwrap().entries.is_empty());
        view.observe(&results("11", 9, "Hà"));
        assert_eq!(view.picker.as_ref().unwrap().entries[0].thread_id, "d:33");
        view.clear_picker();
        view.observe(&results("11", 9, "Hà"));
        assert!(!view.is_open());
    }

    #[test]
    fn only_matching_ack_settles_forward_and_unknown_locks_submission() {
        let picker = picker();
        let mut view = Forwarding {
            flight: Some(Request {
                source: picker.source.clone(),
                target: "d:33".into(),
                id: 1,
            }),
            picker: Some(picker),
            ..Default::default()
        };
        let finish = |target: &str, outcome| LiveEvent::ForwardFinished {
            account_id: "11".into(),
            request_id: 1,
            source_thread: "d:22".into(),
            source_message_id: "901".into(),
            target_thread: target.into(),
            outcome,
        };
        view.observe(&finish("d:44", ForwardOutcome::Accepted));
        assert!(view.flight.is_some());
        view.observe(&finish("d:33", ForwardOutcome::Unknown));
        assert!(view.flight.is_none());
        assert!(view.picker.as_ref().unwrap().locked(None));
        view.observe(&finish("d:33", ForwardOutcome::Accepted));
        assert!(view.picker.as_ref().unwrap().locked(None));
    }
}
