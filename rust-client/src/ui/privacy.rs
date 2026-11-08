//! Fail-closed presentation of Zalo's existing hidden conversations.
use super::{dialog, live::LiveState, visuals, NativeApp};
use crate::app::live::{
    HiddenCandidate, HiddenPin, HiddenUnlockOutcome, LiveCommand, LiveEvent, LivePrivacyStatus,
    LiveWorker,
};
use eframe::egui::{self, vec2, RichText};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const SEARCH_DELAY: Duration = Duration::from_millis(180);

fn hidden_title(candidate: &HiddenCandidate) -> &str {
    if candidate.title.trim().is_empty() || candidate.title.trim() == candidate.thread_id {
        "Hội thoại ẩn"
    } else {
        &candidate.title
    }
}

pub(super) struct PinPrompt {
    pub thread: String,
    title: String,
    account: String,
    auth_generation: u64,
    worker_epoch: u64,
    revision: u64,
    pub pin: String,
    pub pending: Option<u64>,
    error: Option<&'static str>,
    focus: bool,
}
impl PinPrompt {
    fn clear_pin(&mut self) {
        let mut bytes = std::mem::take(&mut self.pin).into_bytes();
        bytes.fill(0);
    }
}
impl Drop for PinPrompt {
    fn drop(&mut self) {
        self.clear_pin();
    }
}

pub(super) struct Privacy {
    pub status: LivePrivacyStatus,
    pub revision: u64,
    hidden: HashSet<String>,
    backend_unlocked: Option<String>,
    active_unlocked: Option<String>,
    active_conversation: Option<crate::domain::Conversation>,
    worker_epoch: u64,
    serial: u64,
    query: String,
    query_generation: u64,
    query_due: Option<Instant>,
    pub candidates: Vec<HiddenCandidate>,
    pub prompt: Option<PinPrompt>,
    focused: bool,
}
impl Default for Privacy {
    fn default() -> Self {
        Self {
            status: LivePrivacyStatus::Checking,
            revision: 0,
            hidden: HashSet::new(),
            backend_unlocked: None,
            active_unlocked: None,
            active_conversation: None,
            worker_epoch: 0,
            serial: 0,
            query: String::new(),
            query_generation: 0,
            query_due: None,
            candidates: Vec::new(),
            prompt: None,
            focused: true,
        }
    }
}
impl Privacy {
    #[cfg(test)]
    pub fn fixture_ready() -> Self {
        Self {
            status: LivePrivacyStatus::Ready,
            revision: 1,
            ..Self::default()
        }
    }
    /// A policy is in force: Zalo's answer, or the last confirmed hidden set while a check is retried. Only the
    /// moment before the worker's first announcement keeps content back.
    pub fn ready(&self) -> bool {
        self.status != LivePrivacyStatus::Checking
    }
    pub fn hidden(&self, thread: &str) -> bool {
        self.hidden.contains(thread)
    }
    pub fn visible(&self, thread: &str) -> bool {
        self.ready()
            && (!self.hidden(thread)
                || (self.active_unlocked.as_deref() == Some(thread)
                    && self.backend_unlocked.as_deref() == Some(thread)))
    }
    pub fn list_visible(&self, thread: &str) -> bool {
        self.ready() && !self.hidden(thread)
    }
    pub fn conversation(&self, thread: &str) -> Option<&crate::domain::Conversation> {
        self.active_conversation
            .as_ref()
            .filter(|conversation| conversation.id == thread && self.visible(thread))
    }
    pub fn observe_conversation(&mut self, event: &LiveEvent) {
        let conversation = match event {
            LiveEvent::Opened { conversation, .. }
            | LiveEvent::Received { conversation, .. }
            | LiveEvent::Sent { conversation, .. } => conversation,
            _ => return,
        };
        if self.active_unlocked.as_deref() == Some(conversation.id.as_str())
            && self.visible(&conversation.id)
        {
            self.active_conversation = Some(conversation.clone());
        }
    }
    pub fn reset_worker(&mut self) {
        let epoch = self.worker_epoch.wrapping_add(1);
        let serial = self.serial.wrapping_add(1);
        // The known hidden threads stay hidden after the worker is gone; only unlocks, prompts and queries end.
        let hidden = std::mem::take(&mut self.hidden);
        *self = Self {
            worker_epoch: epoch,
            serial,
            hidden,
            ..Self::default()
        };
    }
    fn next_request(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
    pub fn sync_query(&mut self, query: &str, ctx: &egui::Context) {
        let query = query.trim();
        if self.query == query {
            return;
        }
        self.query = query.into();
        self.candidates.clear();
        self.query_generation = self.next_request();
        self.query_due = (self.ready()
            && !query.is_empty()
            && query.chars().count() <= 128
            && !query.chars().any(char::is_control))
        .then_some(Instant::now() + SEARCH_DELAY);
        if self.query_due.is_some() {
            ctx.request_repaint_after(SEARCH_DELAY);
        }
    }
    pub fn poll_query(&mut self, worker: Option<&LiveWorker>, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.raw.focused);
        if focused && !self.focused {
            if let Some(worker) = worker {
                let _ = worker.submit(LiveCommand::RefreshPrivacy);
            }
        }
        self.focused = focused;
        let Some(due) = self.query_due else {
            return;
        };
        if !self.ready() {
            return;
        }
        if due > Instant::now() {
            ctx.request_repaint_after(due - Instant::now());
            return;
        }
        let Some(worker) = worker else {
            return;
        };
        if worker
            .submit(LiveCommand::SearchHidden {
                generation: self.query_generation,
                query: self.query.clone(),
            })
            .is_ok()
        {
            self.query_due = None;
        } else {
            self.query_due = Some(Instant::now() + SEARCH_DELAY);
            ctx.request_repaint_after(SEARCH_DELAY);
        }
    }
    /// FIFO marker is handled before every content consumer, including caches.
    pub fn snapshot(&mut self, event: &LiveEvent) -> bool {
        let LiveEvent::PrivacyChanged {
            revision,
            status,
            hidden_threads,
            unlocked_thread,
            ..
        } = event
        else {
            return false;
        };
        if *revision <= self.revision {
            return false;
        }
        let next_hidden: HashSet<_> = hidden_threads.iter().cloned().collect();
        let granting_pending = *status == LivePrivacyStatus::Ready
            && next_hidden == self.hidden
            && self.prompt.as_ref().is_some_and(|prompt| {
                prompt.pending.is_some()
                    && unlocked_thread.as_deref() == Some(prompt.thread.as_str())
            });
        self.status = *status;
        self.revision = *revision;
        self.hidden = next_hidden;
        self.backend_unlocked = unlocked_thread.clone();
        if self.active_unlocked != self.backend_unlocked || !self.ready() {
            self.active_unlocked = None;
            self.active_conversation = None;
        }
        self.candidates.clear();
        self.query_generation = self.next_request();
        self.query_due =
            (self.ready() && !self.query.is_empty()).then_some(Instant::now() + SEARCH_DELAY);
        if !granting_pending {
            self.prompt = None;
        } else if let Some(prompt) = &mut self.prompt {
            prompt.revision = *revision;
            prompt.clear_pin();
        }
        true
    }
    pub fn matches(&mut self, event: &LiveEvent) {
        if let LiveEvent::HiddenMatches {
            generation,
            revision,
            query,
            candidates,
            ..
        } = event
        {
            if self.ready()
                && *revision == self.revision
                && *generation == self.query_generation
                && query == &self.query
                && !query.is_empty()
                && candidates.len() <= 50
            {
                self.candidates = candidates
                    .iter()
                    .filter(|candidate| self.hidden(&candidate.thread_id))
                    .cloned()
                    .collect();
            }
        }
    }
    pub fn begin_prompt(
        &mut self,
        account: String,
        auth_generation: u64,
        candidate: HiddenCandidate,
    ) {
        if !self.ready() || !self.hidden(&candidate.thread_id) {
            return;
        }
        let title = hidden_title(&candidate).to_owned();
        self.prompt = Some(PinPrompt {
            thread: candidate.thread_id,
            title,
            account,
            auth_generation,
            worker_epoch: self.worker_epoch,
            revision: self.revision,
            pin: String::new(),
            pending: None,
            error: None,
            focus: true,
        });
    }
    pub fn cancel(&mut self, worker: Option<&LiveWorker>) -> bool {
        let invalidate = self
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.pending.is_some())
            || self.active_unlocked.is_some()
            || self.backend_unlocked.is_some();
        self.prompt = None;
        self.active_unlocked = None;
        self.active_conversation = None;
        self.backend_unlocked = None;
        self.next_request();
        !invalidate || worker.is_some_and(|worker| worker.submit(LiveCommand::RelockHidden).is_ok())
    }
    pub fn unlock_result(
        &mut self,
        event: &LiveEvent,
        account: &str,
        auth_generation: u64,
    ) -> Option<(String, u64)> {
        let LiveEvent::HiddenUnlockResult {
            generation,
            revision,
            thread,
            outcome,
            ..
        } = event
        else {
            return None;
        };
        let prompt = self.prompt.as_mut()?;
        if prompt.account != account
            || prompt.auth_generation != auth_generation
            || prompt.worker_epoch != self.worker_epoch
            || prompt.pending != Some(*generation)
            || prompt.revision != *revision
            || *revision != self.revision
            || prompt.thread != *thread
        {
            return None;
        }
        prompt.pending = None;
        prompt.clear_pin();
        match outcome {
            HiddenUnlockOutcome::Unlocked
                if self.status == LivePrivacyStatus::Ready
                    && self.backend_unlocked.as_deref() == Some(thread) =>
            {
                self.active_unlocked = Some(thread.clone());
                self.prompt = None;
                Some((thread.clone(), *generation))
            }
            HiddenUnlockOutcome::IncorrectPin => {
                prompt.error = Some("Mã PIN không đúng. Vui lòng thử lại.");
                prompt.focus = true;
                None
            }
            _ => {
                prompt.error = Some("Chưa mở được hội thoại. Vui lòng thử lại.");
                prompt.focus = true;
                None
            }
        }
    }
    pub fn project(&self, mut event: LiveEvent) -> Option<LiveEvent> {
        match &mut event {
            LiveEvent::Ready { conversations, .. }
            | LiveEvent::DirectoryChanged { conversations, .. } => {
                if !self.ready() {
                    return None;
                }
                conversations.retain(|thread| self.list_visible(&thread.id));
            }
            LiveEvent::DirectoryPage { entries, .. } => {
                if !self.ready() {
                    return None;
                }
                entries.retain(|entry| self.list_visible(&entry.thread_id()));
            }
            LiveEvent::Avatars { avatars, .. } => {
                if !self.ready() {
                    return None;
                }
                avatars.retain(|avatar| self.visible(&avatar.thread_id));
            }
            LiveEvent::Opened { conversation, .. } => {
                if !self.visible(&conversation.id) {
                    return None;
                }
            }
            LiveEvent::Page { thread, .. }
            | LiveEvent::Overview { thread, .. }
            | LiveEvent::OverviewFailed { thread, .. }
            | LiveEvent::SearchResults { thread, .. }
            | LiveEvent::SearchFailed { thread, .. }
            | LiveEvent::SenderNames { thread, .. }
            | LiveEvent::MessagesRefreshed { thread, .. }
            | LiveEvent::MessagesRefreshFailed { thread, .. }
            | LiveEvent::RequestFailed { thread, .. }
            | LiveEvent::SendFailed { thread, .. }
            | LiveEvent::Sent { thread, .. } => {
                if !self.visible(thread) {
                    return None;
                }
            }
            LiveEvent::Received { message, .. } => {
                if !self.visible(&message.thread_id) {
                    return None;
                }
            }
            LiveEvent::DirectoryStatus { .. } | LiveEvent::DirectoryFailed { .. } => {
                if !self.ready() {
                    return None;
                }
            }
            LiveEvent::PrivacyChanged { .. }
            | LiveEvent::HiddenMatches { .. }
            | LiveEvent::HiddenUnlockResult { .. }
            | LiveEvent::SendSettled { .. } => return None,
            _ => {}
        }
        Some(event)
    }
}

impl NativeApp {
    pub(super) fn privacy_ready(&self) -> bool {
        self.live
            .as_ref()
            .is_none_or(|state| state.account.is_none() || state.privacy.ready())
    }
    pub(super) fn privacy_visible(&self, thread: &str) -> bool {
        self.live
            .as_ref()
            .is_none_or(|state| state.account.is_none() || state.privacy.visible(thread))
    }
    pub(super) fn privacy_list_visible(&self, thread: &str) -> bool {
        self.live
            .as_ref()
            .is_none_or(|state| state.account.is_none() || state.privacy.list_visible(thread))
    }
    pub(super) fn visible_conversation(
        &self,
        thread: &str,
    ) -> Option<&crate::domain::Conversation> {
        if !self.privacy_visible(thread) {
            return None;
        }
        self.model
            .conversations
            .iter()
            .find(|conversation| conversation.id == thread)
            .or_else(|| {
                self.live
                    .as_ref()
                    .and_then(|state| state.privacy.conversation(thread))
            })
    }
    pub(super) fn quarantine_privacy(
        &mut self,
        state: &mut LiveState,
        ctx: &egui::Context,
        changed: bool,
    ) {
        let had_content = self.model.selected.is_some() || !self.model.messages.is_empty();
        if !state.privacy.ready() {
            self.drafts.park_active();
        }
        self.model
            .conversations
            .retain(|thread| state.privacy.list_visible(&thread.id));
        let blocked = self
            .model
            .selected
            .as_deref()
            .is_some_and(|thread| !state.privacy.visible(thread));
        if blocked || (!state.privacy.ready() && had_content) {
            self.drafts.park_active();
            self.model.selected = None;
            self.model.messages.clear();
            self.model.generation = self.model.generation.wrapping_add(1);
            self.model.pending = false;
            self.model.has_older = false;
            self.conversation_details = None;
            self.account_popup = None;
            self.modal_focus = None;
            self.account_trigger_focus = None;
            self.focus_composer = false;
            self.ime_composing = false;
            state.review_dialog = false;
            state.open_dialog = false;
            if let Some(id) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(id));
            }
        }
        if changed || blocked {
            state.sender_names = Default::default();
            state.hydration = Default::default();
            state.media = Default::default();
            state.avatars = Default::default();
            state.stickers = Default::default();
            state.directory.privacy_reset(ctx, state.privacy.ready());
        }
    }
    pub(super) fn relock_hidden(&mut self) {
        let Some(mut state) = self.live.take() else {
            return;
        };
        if self
            .model
            .selected
            .as_deref()
            .is_some_and(|thread| state.privacy.hidden(thread))
        {
            state.avatars = Default::default();
        }
        if !state.privacy.cancel(state.worker.as_ref()) {
            state.worker = None;
            state.privacy.reset_worker();
            state.connection = crate::app::live::LiveConnection::Offline;
            state.reason = Some("Chưa xác nhận được trạng thái riêng tư. Hãy kết nối lại.".into());
        }
        let ctx = state.context.clone();
        self.quarantine_privacy(&mut state, &ctx, false);
        self.live = Some(state);
    }
    pub(super) fn show_privacy_neutral(&self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let status = match self.live.as_ref().map(|state| state.privacy.status) {
            Some(LivePrivacyStatus::Failed) => {
                "Chưa xác minh được trạng thái riêng tư của hội thoại"
            }
            _ => "Đang xác minh trạng thái riêng tư của hội thoại",
        };
        ui.add_space(36.0);
        let width = ui.available_width();
        let font = egui::TextStyle::Body.resolve(ui.style());
        for (text, color) in [
            (status, colors.text_secondary),
            ("Nội dung được giữ kín đến khi kiểm tra xong. Bấm avatar để xem trạng thái hoặc thử lại.", colors.text_primary),
        ] {
            let galley = ui.painter().layout(text.into(), font.clone(), color, width);
            let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), egui::Sense::hover());
            let origin = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top());
            ui.painter().galley(origin, galley, color);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
        }
    }
    pub(super) fn privacy_status_controls(&mut self, ui: &mut egui::Ui) {
        let Some(state) = &mut self.live else {
            return;
        };
        if state.account.is_none() {
            return;
        }
        ui.separator();
        ui.add(
            egui::Label::new(match state.privacy.status {
                LivePrivacyStatus::Checking => "Đang kiểm tra hội thoại ẩn trên Zalo…",
                LivePrivacyStatus::Failed => {
                    "Chưa kiểm tra được hội thoại ẩn. Nội dung tạm được giữ kín."
                }
                LivePrivacyStatus::Ready => "Đã đồng bộ trạng thái hội thoại ẩn từ Zalo.",
            })
            .wrap(),
        );
        if state.privacy.status != LivePrivacyStatus::Checking
            && ui
                .add_enabled(
                    state.connection != crate::app::live::LiveConnection::Expired,
                    egui::Button::new("Kiểm tra lại quyền riêng tư").small(),
                )
                .on_disabled_hover_text("Phiên đã hết hạn. Đăng nhập lại từ menu bánh răng.")
                .clicked()
        {
            if let Some(worker) = &state.worker {
                if worker.submit(LiveCommand::RefreshPrivacy).is_err() {
                    ui.label("Chưa gửi được yêu cầu. Vui lòng thử lại.");
                }
            } else {
                state.ensure_worker(self._profile.clone());
            }
        }
    }
    pub(super) fn hidden_search_rows(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(state) = &self.live else {
            return false;
        };
        if self.search.trim().is_empty() || !state.privacy.ready() {
            return false;
        }
        let mut selected = None;
        let found = !state.privacy.candidates.is_empty();
        for candidate in &state.privacy.candidates {
            let title = hidden_title(candidate);
            let colors = crate::theme::palette(ui.ctx());
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 56.0), egui::Sense::hover());
            if ui.is_enabled() && ui.rect_contains_pointer(rect) {
                ui.painter().rect_filled(rect, 4.0, colors.hover);
            }
            let mut row =
                ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 10.0))));
            row.horizontal(|ui| {
                let (lock, _) = ui.allocate_exact_size(vec2(20.0, 24.0), egui::Sense::hover());
                visuals::icon(ui, lock, visuals::Icon::Shield, colors.text_secondary);
                ui.add(egui::Label::new(title).truncate())
                    .on_hover_text(title);
            });
            // Register the row's click target after its label/tooltip widgets.
            let response = ui.interact(
                rect,
                ui.id().with(("hidden_result", &candidate.thread_id)),
                egui::Sense::click(),
            );
            visuals::focus_outline(ui, &response, rect, 4.0);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!("{title} · hội thoại ẩn"),
                )
            });
            if response.clicked() {
                selected = Some(candidate.clone());
            }
        }
        if let Some(candidate) = selected {
            self.relock_hidden();
            if let Some(state) = &mut self.live {
                if let Some(account) = &state.account {
                    state.privacy.begin_prompt(
                        account.id.clone(),
                        state.login.generation(),
                        candidate,
                    );
                }
            }
        }
        found
    }
    pub(super) fn hidden_pin_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.live.take() else {
            return;
        };
        let Some(mut prompt) = state.privacy.prompt.take() else {
            self.live = Some(state);
            return;
        };
        let valid_scope = state
            .account
            .as_ref()
            .is_some_and(|account| account.id == prompt.account)
            && state.login.generation() == prompt.auth_generation
            && state.privacy.worker_epoch == prompt.worker_epoch;
        if !valid_scope {
            state.privacy.prompt = Some(prompt);
            if !state.privacy.cancel(state.worker.as_ref()) {
                state.worker = None;
                state.privacy.reset_worker();
            }
            self.live = Some(state);
            return;
        }
        let mut submit = false;
        let mut cancel = false;
        let ime_active = self.ime_composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
            });
        let response = dialog::show(
            ctx,
            "hidden_conversation_pin",
            "Mở hội thoại ẩn",
            360.0,
            false,
            |ui| {
                dialog::body(ui, |ui| {
                    ui.add(egui::Label::new(&prompt.title).wrap());
                    ui.add(egui::Label::new("Nhập mã PIN gồm 4 chữ số đã đặt trên Zalo.").wrap());
                    let response = ui.add_enabled(
                        prompt.pending.is_none(),
                        egui::TextEdit::singleline(&mut prompt.pin)
                            .id(egui::Id::new((
                                "hidden_conversation_pin_input",
                                &prompt.account,
                                &prompt.thread,
                                prompt.auth_generation,
                                prompt.worker_epoch,
                            )))
                            .password(true)
                            .char_limit(4)
                            .hint_text("Mã PIN Zalo")
                            .desired_width(ui.available_width()),
                    );
                    if prompt.focus {
                        response.request_focus();
                        prompt.focus = false;
                    }
                    if (response.has_focus() || response.lost_focus()) && !ime_active
                        && prompt.pending.is_none() && HiddenPin::new(&prompt.pin).is_some()
                        && state.privacy.ready() && ctx.input(|input| input.events.iter().any(|event| matches!(event,
                            egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if *modifiers == egui::Modifiers::NONE))) {
                        submit = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    }
                    if let Some(mut edit) = egui::TextEdit::load_state(ctx, response.id) {
                        edit.clear_undoer();
                        edit.store(ctx, response.id);
                    }
                    if let Some(error) = prompt.error {
                        ui.add(
                            egui::Label::new(
                                RichText::new(error).color(crate::theme::palette(ctx).error),
                            )
                            .wrap(),
                        );
                    }
                    if prompt.pending.is_some() {
                        ui.label("Đang xác minh mã PIN…");
                    }
                });
                dialog::footer(ui, |ui| {
                    submit |= visuals::primary_button(
                        ui,
                        prompt.pending.is_none()
                            && HiddenPin::new(&prompt.pin).is_some()
                            && state.privacy.ready(),
                        "Mở hội thoại",
                        vec2(120.0, 32.0),
                    )
                    .clicked();
                    cancel = ui.button("Hủy").clicked();
                });
            },
        );
        if submit {
            if let Some(pin) = HiddenPin::new(&prompt.pin) {
                let generation = state
                    .privacy
                    .next_request()
                    .max(self.model.generation.wrapping_add(1));
                state.privacy.serial = generation;
                prompt.clear_pin();
                if state.worker.as_ref().is_some_and(|worker| {
                    worker
                        .submit(LiveCommand::UnlockHidden {
                            generation,
                            revision: state.privacy.revision,
                            thread: prompt.thread.clone(),
                            pin,
                        })
                        .is_ok()
                }) {
                    prompt.pending = Some(generation);
                    prompt.revision = state.privacy.revision;
                    prompt.error = None;
                } else {
                    prompt.error = Some("Chưa gửi được yêu cầu. Vui lòng thử lại.");
                }
            }
        }
        state.privacy.prompt = Some(prompt);
        let closed = response.closed || cancel;
        if closed && !state.privacy.cancel(state.worker.as_ref()) {
            state.worker = None;
            state.privacy.reset_worker();
        }
        self.live = Some(state);
    }
}
