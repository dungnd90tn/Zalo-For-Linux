//! Scoped recipient selection and results for forwarding one ordinary text message.
use super::{visuals, NativeApp};
use crate::{
    app::live::{
        ForwardBatchResult, ForwardOutcome, ForwardTarget, LiveCommand, LiveConnection, LiveEvent,
        LivePrivacyStatus, LiveWorker, MAX_FORWARD_BATCH,
    },
    domain::{reply::valid_source_id, Conversation, Message},
};
use eframe::egui::{self, vec2, RichText};
use std::borrow::Cow;

const MAX_PAGE: usize = 25;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    account: String,
    auth: Option<u64>,
    thread: String,
    page: u64,
    message: String,
}

struct Request {
    source: Source,
    targets: Vec<String>,
    id: u64,
}

struct Recipient {
    target: ForwardTarget,
    result: Option<ForwardOutcome>,
}

struct Picker {
    source: Source,
    search: String,
    query: String,
    entries: Vec<ForwardTarget>,
    selected: Vec<Recipient>,
    show_selected: bool,
    lookup_id: Option<u64>,
    submitted: bool,
    error: Option<&'static str>,
}

fn retryable(outcome: ForwardOutcome) -> bool {
    matches!(
        outcome,
        ForwardOutcome::Rejected | ForwardOutcome::Unavailable
    )
}

impl Picker {
    fn new(source: Source) -> Self {
        Self {
            source,
            search: String::new(),
            query: String::new(),
            entries: Vec::new(),
            selected: Vec::new(),
            show_selected: false,
            lookup_id: None,
            submitted: false,
            error: None,
        }
    }

    fn selected(&self, thread: &str) -> bool {
        self.selected
            .iter()
            .any(|recipient| recipient.target.thread_id == thread)
    }

    fn toggle(&mut self, target: ForwardTarget) {
        if let Some(index) = self
            .selected
            .iter()
            .position(|recipient| recipient.target.thread_id == target.thread_id)
        {
            self.selected.remove(index);
        } else if self.selected.len() < MAX_FORWARD_BATCH {
            self.selected.push(Recipient {
                target,
                result: None,
            });
        }
    }

    fn send_targets(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter(|recipient| !self.submitted || recipient.result.is_some_and(retryable))
            .map(|recipient| recipient.target.thread_id.clone())
            .collect()
    }

    fn retain_recipients(
        &mut self,
        conversations: &[Conversation],
        visible: impl Fn(&str) -> bool,
    ) {
        let account = &self.source.account;
        let eligible = |thread: &str| {
            visible(thread)
                && (thread
                    .strip_prefix("d:")
                    .is_some_and(|peer| peer != account && valid_source_id(peer))
                    || (thread.strip_prefix("g:").is_some_and(valid_source_id)
                        && !conversations
                            .iter()
                            .any(|item| item.id == thread && item.group_left)))
        };
        self.entries.retain(|entry| eligible(&entry.thread_id));
        self.selected
            .retain(|recipient| eligible(&recipient.target.thread_id));
    }

    fn query_changed(&mut self) {
        self.entries.clear();
        self.error = None;
        self.lookup_id = None;
    }

    fn lookup(&mut self, worker: &LiveWorker, id: u64) {
        self.entries.clear();
        self.query = self.search.trim().to_owned();
        let command = LiveCommand::ForwardTargets {
            generation: id,
            source_thread: self.source.thread.clone(),
            source_message_id: self.source.message.clone(),
            query: self.query.clone(),
        };
        self.lookup_id = None;
        match worker.submit(command) {
            Ok(()) => {
                self.lookup_id = Some(id);
                self.error = None;
            }
            Err(_) => self.error = Some("Chưa tìm được người nhận. Hãy thử lại."),
        }
    }
}

#[derive(Default)]
pub(super) struct Forwarding {
    picker: Option<Picker>,
    parked: Option<Picker>,
    flight: Option<Request>,
    notice: Option<&'static str>,
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
        self.parked = None;
    }

    fn submit(&mut self, picker: &mut Picker, worker: &LiveWorker, targets: Vec<String>) {
        if targets.is_empty() || self.flight.is_some() {
            return;
        }
        let request = Request {
            id: self.id(),
            source: picker.source.clone(),
            targets,
        };
        let command = LiveCommand::ForwardBatch {
            request_id: request.id,
            source_thread: request.source.thread.clone(),
            source_message_id: request.source.message.clone(),
            target_threads: request.targets.clone(),
        };
        if worker.submit(command).is_ok() {
            for recipient in &mut picker.selected {
                if request.targets.contains(&recipient.target.thread_id) {
                    recipient.result = None;
                }
            }
            picker.submitted = true;
            picker.lookup_id = None;
            picker.error = None;
            self.flight = Some(request);
            self.notice = None;
        } else {
            picker.error = Some("Chưa gửi yêu cầu chuyển tiếp. Hãy thử lại.");
        }
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
            LiveEvent::ForwardBatchFinished {
                account_id,
                request_id,
                source_thread,
                source_message_id,
                results,
            } => {
                let Some(flight) = &self.flight else { return };
                if flight.id != *request_id
                    || flight.source.account != *account_id
                    || flight.source.thread != *source_thread
                    || flight.source.message != *source_message_id
                {
                    return;
                }
                let matching = results.len() == flight.targets.len()
                    && results
                        .iter()
                        .zip(&flight.targets)
                        .all(|(result, target)| result.target_thread == *target);
                self.settle(matching.then_some(results.as_slice()));
            }
            _ => {}
        }
    }

    fn settle(&mut self, results: Option<&[ForwardBatchResult]>) {
        let Some(flight) = self.flight.take() else {
            return;
        };
        if let Some(picker) = self
            .picker
            .as_mut()
            .or(self.parked.as_mut())
            .filter(|picker| picker.source == flight.source)
        {
            for recipient in &mut picker.selected {
                if let Some(index) = flight
                    .targets
                    .iter()
                    .position(|target| *target == recipient.target.thread_id)
                {
                    recipient.result = Some(
                        results.map_or(ForwardOutcome::Unknown, |results| results[index].outcome),
                    );
                }
            }
        }
        self.notice = Some(match results {
            None => "Chưa rõ kết quả chuyển tiếp. Hãy kiểm tra với người nhận.",
            Some(results)
                if results
                    .iter()
                    .all(|result| result.outcome == ForwardOutcome::Accepted) =>
            {
                "Zalo đã xác nhận lượt chuyển tiếp."
            }
            Some(results)
                if results
                    .iter()
                    .any(|result| result.outcome == ForwardOutcome::Unknown) =>
            {
                "Có lượt chuyển tiếp chưa được xác nhận."
            }
            Some(results)
                if results
                    .iter()
                    .any(|result| result.outcome == ForwardOutcome::Accepted) =>
            {
                "Đã chuyển tiếp cho một phần người nhận."
            }
            Some(_) => "Chưa chuyển tiếp được tin nhắn.",
        });
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
            !picker.submitted
                && picker.lookup_id == Some(generation)
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
        self.notice
    }

    #[cfg(test)]
    pub(super) fn fixture(
        &mut self,
        account: &str,
        auth: Option<u64>,
        page: u64,
        thread: &str,
        scene: &str,
    ) {
        let mut picker = Picker::new(Source {
            account: account.into(),
            auth,
            thread: thread.into(),
            page,
            message: "901".into(),
        });
        let friends = [
            ("d:33", "Đặng Hà"),
            ("d:44", "Lê Minh"),
            ("d:55", "Nguyễn Thanh Thảo"),
            ("d:66", "Trần Vũ Phương"),
        ];
        let mixed = [
            friends[0],
            ("g:33", "Kế hoạch ra mắt tháng mười hai"),
            ("g:50", "Nhóm kiểm thử cùng khách hàng"),
            friends[1],
        ];
        picker.entries = if matches!(scene, "group-targets" | "group-results" | "group-source") {
            mixed
        } else {
            friends
        }
        .into_iter()
        .map(|(thread, name)| ForwardTarget {
            thread_id: thread.into(),
            title: name.into(),
        })
        .collect();
        let count = match scene {
            "mixed" | "group-results" => 4,
            "batch" | "group-targets" | "group-source" => 2,
            _ => 1,
        };
        for index in 0..count {
            let entry = picker.entries[index].clone();
            picker.toggle(entry);
        }
        if matches!(scene, "mixed" | "group-results" | "unknown") {
            picker.submitted = true;
            let outcomes = if scene != "unknown" {
                [
                    ForwardOutcome::Accepted,
                    ForwardOutcome::Unknown,
                    ForwardOutcome::Rejected,
                    ForwardOutcome::Unavailable,
                ]
            } else {
                [ForwardOutcome::Unknown; 4]
            };
            for (recipient, outcome) in picker.selected.iter_mut().zip(outcomes) {
                recipient.result = Some(outcome);
            }
        }
        self.picker = Some(picker);
    }

    pub(super) fn worker_closed(&mut self) {
        self.settle(None);
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
        let (kind, remote) = thread.split_once(':')?;
        let source_visible = match kind {
            "d" => remote != account,
            "g" => {
                self.live
                    .as_ref()
                    .is_some_and(|state| state.privacy.status == LivePrivacyStatus::Ready)
                    && self
                        .model
                        .conversations
                        .iter()
                        .any(|group| group.id == *thread && !group.group_left)
            }
            _ => false,
        };
        if self.demo
            || self.model.pending
            || !source_visible
            || message.thread_id != *thread
            || message.account_id != account
            || !self.privacy_list_visible(thread)
            || message.quote.is_some()
            || !(1..=9_007_199_254_740_991).contains(&message.sent_at_ms)
            || message.text.trim().is_empty()
            || message.reply_reference().is_none()
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
        let Some(source) = self.forward_source(message) else {
            return false;
        };
        self.live.as_ref().is_some_and(|state| {
            state
                .forwarding
                .parked
                .as_ref()
                .is_some_and(|picker| picker.source == source)
                || (state.connection == LiveConnection::Online
                    && state.worker.is_some()
                    && state.forwarding.flight.is_none())
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
            let forwarding = &mut state.forwarding;
            if let Some(parked) = forwarding
                .parked
                .take()
                .filter(|picker| picker.source == source)
            {
                forwarding.picker = Some(parked);
            } else if let Some(worker) = &state.worker {
                let mut picker = Picker::new(source);
                picker.lookup(worker, forwarding.id());
                forwarding.picker = Some(picker);
            }
        }
    }

    pub(super) fn sync_forward(&mut self) {
        let Some(state) = &self.live else { return };
        let forwarding = &state.forwarding;
        let open = forwarding.is_open();
        let Some(picker) = forwarding.picker.as_ref().or(forwarding.parked.as_ref()) else {
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
            if open {
                self.modal_focus = None;
            }
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
        let online = state.connection == LiveConnection::Online && state.worker.is_some();
        let visible = |thread: &str| state.privacy.list_visible(thread);
        picker.retain_recipients(&self.model.conversations, visible);
        let ime_active = self.ime_composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(_))))
            });
        if ime_active && !picker.submitted && !picker.show_selected {
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
        let mut lookup = false;
        let mut send = false;
        let mut done = false;
        let mut restart = false;
        let mut targets = Vec::new();
        let dialog = super::dialog::show(
            ctx,
            "forward_message",
            "Chuyển tiếp tin nhắn",
            464.0,
            true,
            |ui| {
                let colors = crate::theme::palette(ctx);
                super::dialog::body(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(preview).color(colors.text_secondary))
                            .truncate(),
                    )
                    .on_hover_text(preview);
                    if picker.submitted {
                        if busy {
                            ui.label(
                                "Đang chờ Zalo xác nhận. Yêu cầu vẫn chạy khi bạn đóng hộp thoại.",
                            );
                        }
                        show_outcomes(ui, &picker, colors);
                    } else {
                        lookup = show_recipients(ui, &mut picker, online && !ime_active);
                    }
                    if let Some(error) = picker.error {
                        ui.colored_label(colors.error, error);
                    } else if !online && !busy {
                        ui.label(
                            RichText::new("Kết nối lại để chuyển tiếp.")
                                .color(colors.text_secondary),
                        );
                    }
                });
                super::dialog::footer(ui, |ui| {
                    targets = picker.send_targets();
                    if !busy && (!picker.submitted || !targets.is_empty()) {
                        let label = if picker.submitted {
                            format!("Thử lại ({})", targets.len())
                        } else if targets.is_empty() {
                            "Chuyển tiếp".into()
                        } else {
                            format!("Chuyển tiếp ({})", targets.len())
                        };
                        send = visuals::primary_button(
                            ui,
                            online
                                && !ime_active
                                && !lookup
                                && picker.lookup_id.is_none()
                                && !targets.is_empty(),
                            &label,
                            vec2(140.0, 44.0),
                        )
                        .clicked();
                    }
                    done = ui
                        .add_sized(
                            vec2(72.0, 44.0),
                            egui::Button::new(if picker.submitted { "Đóng" } else { "Hủy" }),
                        )
                        .clicked();
                    if picker.submitted && !busy {
                        restart = ui
                            .add_enabled(
                                online,
                                egui::Button::new("Lượt mới").min_size(vec2(84.0, 44.0)),
                            )
                            .clicked();
                    }
                });
            },
        );
        if dialog.closed || done {
            if picker.submitted {
                forwarding.parked = Some(picker);
            }
            return;
        }
        if let Some(worker) = &state.worker {
            if restart && online {
                picker = Picker::new(picker.source);
                lookup = true;
            }
            if lookup && online {
                picker.lookup(worker, forwarding.id());
            } else if send
                && online
                && !targets.is_empty()
                && targets.iter().all(|target| visible(target))
            {
                forwarding.submit(&mut picker, worker, targets);
            }
        }
        forwarding.picker = Some(picker);
    }
}

fn show_recipients(ui: &mut egui::Ui, picker: &mut Picker, search_enabled: bool) -> bool {
    let mut lookup = false;
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                vec2(108.0, 44.0),
                egui::Button::new("Người nhận")
                    .selected(!picker.show_selected)
                    .rounding(12.0),
            )
            .clicked()
        {
            picker.show_selected = false;
        }
        let count = format!("Đã chọn {}/{}", picker.selected.len(), MAX_FORWARD_BATCH);
        if ui
            .add_sized(
                vec2(140.0, 44.0),
                egui::Button::new(count)
                    .selected(picker.show_selected)
                    .rounding(12.0),
            )
            .clicked()
        {
            picker.show_selected = true;
        }
    });
    if !picker.show_selected {
        ui.horizontal(|ui| {
            let search_width =
                (ui.available_width() - 64.0 - ui.spacing().item_spacing.x).max(80.0);
            if ui
                .add_sized(
                    vec2(search_width, 44.0),
                    egui::TextEdit::singleline(&mut picker.search)
                        .id_source((
                            "forward_recipient_search",
                            &picker.source.account,
                            &picker.source.thread,
                            &picker.source.message,
                        ))
                        .hint_text("Tìm bạn bè hoặc nhóm")
                        .vertical_align(egui::Align::Center)
                        .char_limit(128),
                )
                .changed()
            {
                picker.query_changed();
            }
            lookup = ui
                .add_enabled(
                    search_enabled && picker.lookup_id.is_none(),
                    egui::Button::new("Tìm").min_size(vec2(64.0, 44.0)),
                )
                .clicked();
        });
        if picker.lookup_id.is_some() {
            ui.label("Đang tìm người nhận…");
        } else if picker.search.trim() != picker.query {
            ui.label("Chọn Tìm để cập nhật kết quả.");
        } else if picker.entries.is_empty() && picker.error.is_none() {
            ui.label("Không tìm thấy bạn bè hoặc nhóm phù hợp.");
        }
    } else if picker.selected.is_empty() {
        ui.label("Chưa chọn người nhận.");
    }
    let entries: Vec<_> = if picker.show_selected {
        picker
            .selected
            .iter()
            .map(|recipient| &recipient.target)
            .collect()
    } else if picker.search.trim() == picker.query {
        picker.entries.iter().collect()
    } else {
        Vec::new()
    };
    let max_height = (ui.ctx().screen_rect().height() - 360.0).clamp(96.0, 320.0);
    let mut toggled = None;
    egui::ScrollArea::vertical()
        .id_salt((
            "forward_recipients",
            &picker.source.thread,
            &picker.source.message,
            picker.show_selected,
            &picker.query,
        ))
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for entry in entries {
                let checked = picker.selected(&entry.thread_id);
                let name = recipient_name(entry);
                let label = if checked {
                    format!("{name} · Đã chọn")
                } else {
                    name.to_string()
                };
                let button = egui::Button::new(label)
                    .selected(checked)
                    .rounding(12.0)
                    .truncate()
                    .min_size(vec2(ui.available_width(), 44.0));
                let response = ui
                    .push_id(&entry.thread_id, |ui| {
                        ui.add_enabled(checked || picker.selected.len() < MAX_FORWARD_BATCH, button)
                    })
                    .inner;
                if response.clicked() {
                    toggled = Some(entry.clone());
                }
                response
                    .on_hover_text(if checked {
                        format!("Bỏ chọn {name}")
                    } else {
                        name.into_owned()
                    })
                    .on_disabled_hover_text("Đã chọn tối đa 10 người nhận.");
            }
        });
    if let Some(target) = toggled {
        picker.toggle(target);
    }
    lookup
}

fn recipient_name(target: &ForwardTarget) -> Cow<'_, str> {
    match (target.thread_id.starts_with("g:"), target.title.is_empty()) {
        (true, false) => Cow::Owned(format!("Nhóm · {}", target.title)),
        (true, true) => Cow::Borrowed("Nhóm"),
        (false, true) => Cow::Borrowed("Bạn bè"),
        (false, false) => Cow::Borrowed(&target.title),
    }
}

fn show_outcomes(ui: &mut egui::Ui, picker: &Picker, colors: crate::theme::Palette) {
    for recipient in &picker.selected {
        let (label, color) = match recipient.result {
            None => ("Đang chờ xác nhận", colors.text_secondary),
            Some(ForwardOutcome::Accepted) => ("Zalo đã nhận", colors.text_secondary),
            Some(ForwardOutcome::Rejected) => ("Zalo từ chối", colors.error),
            Some(ForwardOutcome::Unavailable) => ("Chưa gửi được", colors.error),
            Some(ForwardOutcome::Unknown) => {
                ("Chưa rõ kết quả · kiểm tra với người nhận", colors.error)
            }
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let name = recipient_name(&recipient.target);
            ui.add(egui::Label::new(RichText::new(name.as_ref()).strong()).truncate())
                .on_hover_text(name.into_owned());
            ui.label(RichText::new(label).color(color).size(visuals::TEXT_SMALL));
        });
        ui.separator();
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

    fn target(id: u64) -> ForwardTarget {
        ForwardTarget {
            thread_id: format!("d:{id}"),
            title: format!("Liên hệ {id}"),
        }
    }

    fn group(id: u64) -> ForwardTarget {
        ForwardTarget {
            thread_id: format!("g:{id}"),
            title: format!("Kế hoạch {id}"),
        }
    }

    #[test]
    fn group_source_picker_binds_original_message_and_closes_when_scope_changes() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        app.model.selected = Some("g:70".into());
        app.drafts.select("g:70").unwrap();
        app.drafts.active.text = "Bản nháp đang nhập".into();
        app.model.conversations.push(Conversation {
            id: "g:70".into(),
            title: "Nhóm kiểm thử".into(),
            last_message_at_ms: 1,
            last_text: "Tin nhắn thử".into(),
            last_sender: "44".into(),
            group_left: false,
        });
        let message = Message {
            account_id: "11".into(),
            thread_id: "g:70".into(),
            id: "901".into(),
            sender: "44".into(),
            text: "Tin nhắn thử".into(),
            source_client_id: Some("900".into()),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
            sent_at_ms: 1,
        };
        assert!(app.can_forward(&message));
        let mut unsupported = message.clone();
        unsupported.mentions.push(crate::domain::mention::Mention {
            uid: "33".into(),
            pos: 0,
            len: 4,
        });
        assert!(!app.can_forward(&unsupported));
        unsupported = message.clone();
        unsupported.thread_id = "d:70".into();
        assert!(!app.can_forward(&unsupported));
        app.model.messages.push(message.clone());

        app.open_forward("901");
        assert!(matches!(
            commands.try_recv(),
            Ok(LiveCommand::ForwardTargets { source_thread, source_message_id, .. })
                if source_thread == "g:70" && source_message_id == "901"
        ));
        assert!(app.live.as_ref().unwrap().forwarding.is_open());
        app.model.generation += 1;
        app.sync_forward();
        assert!(!app.live.as_ref().unwrap().forwarding.is_open());

        app.open_forward("901");
        let state = app.live.as_mut().unwrap();
        assert!(state.forwarding.is_open());
        assert!(state.privacy.snapshot(&LiveEvent::PrivacyChanged {
            account_id: "11".into(),
            revision: 2,
            status: LivePrivacyStatus::Ready,
            hidden_threads: vec!["g:70".into()],
            unlocked_thread: None,
        }));
        app.sync_forward();
        assert!(!app.live.as_ref().unwrap().forwarding.is_open());
        assert!(!app.can_forward(&message));

        app.live.as_mut().unwrap().privacy = super::super::privacy::Privacy::fixture_ready();
        app.model.conversations[0].group_left = true;
        assert!(!app.can_forward(&message));
        assert_eq!(app.drafts.active.text, "Bản nháp đang nhập");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selections_survive_search_and_remain_unique_and_bounded() {
        let mut picker = picker();
        picker.toggle(group(33));
        for id in 33..=(32 + MAX_FORWARD_BATCH as u64) {
            picker.toggle(target(id));
        }
        assert_eq!(picker.selected.len(), MAX_FORWARD_BATCH);
        assert!(!picker.selected(&target(32 + MAX_FORWARD_BATCH as u64).thread_id));
        picker.search = "Minh".into();
        picker.entries = vec![target(100)];
        picker.lookup_id = Some(9);
        picker.query_changed();
        assert!(picker.entries.is_empty());
        assert!(picker.lookup_id.is_none());
        assert_eq!(picker.send_targets().len(), MAX_FORWARD_BATCH);
        assert!(picker.selected("g:33"));
        assert!(picker.selected("d:33"));
        picker.toggle(target(33));
        assert!(!picker.selected("d:33"));
        assert!(picker.selected("g:33"));
        picker.toggle(target(100));
        assert_eq!(picker.send_targets().last().unwrap(), "d:100");
        assert_eq!(picker.send_targets().len(), MAX_FORWARD_BATCH);
    }

    #[test]
    fn hidden_and_departed_recipients_disappear_from_results_and_selection() {
        let mut picker = picker();
        picker.entries = vec![target(33), group(33), group(44), group(55)];
        for entry in picker.entries.clone() {
            picker.toggle(entry);
        }
        let left = Conversation {
            id: "g:44".into(),
            title: "Đã rời".into(),
            last_message_at_ms: 0,
            last_text: String::new(),
            last_sender: String::new(),
            group_left: true,
        };
        picker.retain_recipients(&[left], |thread| thread != "g:33");
        assert_eq!(picker.send_targets(), ["d:33", "g:55"]);
        assert_eq!(picker.entries, [target(33), group(55)]);
        assert_eq!(recipient_name(&picker.entries[1]), "Nhóm · Kế hoạch 55");
    }

    #[test]
    fn parked_batch_settles_by_scope_and_retries_only_definite_failures() {
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let mut picker = picker();
        let mut view = Forwarding::default();
        for entry in [target(33), group(33), target(35), group(36)] {
            picker.toggle(entry);
        }
        let targets = picker.send_targets();
        view.submit(&mut picker, &worker, targets.clone());
        assert!(matches!(
            commands.try_recv(),
            Ok(LiveCommand::ForwardBatch { request_id: 1, target_threads, .. }) if target_threads == targets
        ));
        view.parked = Some(picker);
        let event = |request_id| LiveEvent::ForwardBatchFinished {
            account_id: "11".into(),
            request_id,
            source_thread: "d:22".into(),
            source_message_id: "901".into(),
            results: targets
                .iter()
                .zip([
                    ForwardOutcome::Accepted,
                    ForwardOutcome::Unknown,
                    ForwardOutcome::Rejected,
                    ForwardOutcome::Unavailable,
                ])
                .map(|(target, outcome)| ForwardBatchResult {
                    target_thread: target.clone(),
                    outcome,
                })
                .collect(),
        };
        view.observe(&event(100));
        assert!(view.flight.is_some());
        view.observe(&event(1));
        assert!(view.flight.is_none());
        assert!(!view.is_open());
        let mut parked = view.parked.take().unwrap();
        let retry = parked.send_targets();
        assert_eq!(retry, ["d:35", "g:36"]);
        view.submit(&mut parked, &worker, retry.clone());
        assert!(matches!(
            commands.try_recv(),
            Ok(LiveCommand::ForwardBatch { request_id: 2, target_threads, .. }) if target_threads == retry
        ));
        view.picker = Some(parked);
        view.worker_closed();
        let picker = view.picker.as_ref().unwrap();
        assert_eq!(picker.selected[0].result, Some(ForwardOutcome::Accepted));
        assert!(picker.selected[1..]
            .iter()
            .all(|recipient| recipient.result == Some(ForwardOutcome::Unknown)));
        assert!(picker.send_targets().is_empty());
    }
}
