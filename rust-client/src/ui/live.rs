//! UI-side coordination of independent authentication and messaging workers.
//! Only owned, validated events cross this boundary; session secrets are never formatted here.
use super::{
    login::{Action, LoginView, Phase},
    NativeApp,
};
use crate::{
    app::{DraftBook, Model, EVENTS_PER_FRAME},
    profile::Profile,
    protocol::auth::{LoginController, LoginEvent, LoginStage},
};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "messaging")]
use crate::app::live::{LiveCommand, LiveConnection, LiveEvent, LiveWorker, ThreadKind};

pub(super) struct Account {
    pub id: String,
    pub name: String,
}

pub(super) struct LiveState {
    controller: LoginController,
    pub login: LoginView,
    pub account: Option<Account>,
    #[cfg(feature = "messaging")]
    pub sender_names: super::senders::Names,
    #[cfg(feature = "messaging")]
    hydration: super::hydration::Hydration,
    #[cfg(feature = "messaging")]
    pub worker: Option<LiveWorker>,
    #[cfg(feature = "messaging")]
    session: Option<Arc<crate::protocol::session::VerifiedSession>>,
    #[cfg(feature = "messaging")]
    context: egui::Context,
    #[cfg(feature = "messaging")]
    reason: Option<String>,
    #[cfg(feature = "messaging")]
    pub connection: LiveConnection,
    #[cfg(feature = "messaging")]
    pub delivery: super::delivery::DeliveryBook,
    #[cfg(feature = "messaging")]
    pub directory: super::directory::DirectoryView,
    #[cfg(feature = "messaging")]
    pub stickers: super::stickers::StickerCache,
    #[cfg(feature = "messaging")]
    pub media: super::media::MediaCache,
    #[cfg(feature = "messaging")]
    pub review_dialog: bool,
    #[cfg(feature = "messaging")]
    pub open_dialog: bool,
    #[cfg(feature = "messaging")]
    target: String,
    #[cfg(feature = "messaging")]
    group: bool,
    pub logout_dialog: bool,
}

impl LiveState {
    #[cfg(all(test, feature = "messaging"))]
    pub fn new(
        profile: Arc<Profile>,
        trust: Option<PathBuf>,
        ctx: &egui::Context,
    ) -> anyhow::Result<Self> {
        let repaint = ctx.clone();
        let controller =
            LoginController::new(profile, trust, Arc::new(move || repaint.request_repaint()))?;
        Ok(Self::with_controller(controller, ctx))
    }

    fn with_controller(controller: LoginController, _ctx: &egui::Context) -> Self {
        Self {
            controller,
            login: LoginView::default(),
            account: None,
            #[cfg(feature = "messaging")]
            sender_names: Default::default(),
            #[cfg(feature = "messaging")]
            hydration: Default::default(),
            #[cfg(feature = "messaging")]
            worker: None,
            #[cfg(feature = "messaging")]
            session: None,
            #[cfg(feature = "messaging")]
            context: _ctx.clone(),
            #[cfg(feature = "messaging")]
            reason: None,
            #[cfg(feature = "messaging")]
            connection: LiveConnection::Offline,
            #[cfg(feature = "messaging")]
            delivery: Default::default(),
            #[cfg(feature = "messaging")]
            directory: Default::default(),
            #[cfg(feature = "messaging")]
            stickers: Default::default(),
            #[cfg(feature = "messaging")]
            media: Default::default(),
            #[cfg(feature = "messaging")]
            review_dialog: false,
            #[cfg(feature = "messaging")]
            open_dialog: false,
            #[cfg(feature = "messaging")]
            target: String::new(),
            #[cfg(feature = "messaging")]
            group: false,
            logout_dialog: false,
        }
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Begin | Action::Resume => {
                let result = if action == Action::Resume {
                    self.controller.startup()
                } else {
                    self.controller.begin()
                };
                match result {
                    Ok(generation) => self.login.start(generation, action == Action::Resume),
                    Err(_) => self.login.failed(
                        self.login.generation(),
                        "Chưa thể bắt đầu đăng nhập. Vui lòng thử lại.",
                    ),
                }
            }
            Action::Cancel => {
                let generation = self.controller.cancel();
                self.login.signed_out(generation);
            }
        }
    }

    #[cfg(feature = "messaging")]
    pub(super) fn ensure_worker(&mut self, profile: Arc<Profile>) -> bool {
        if self.worker.is_some() {
            return true;
        }
        let Some(session) = self.session.as_ref() else {
            return false;
        };
        let repaint = self.context.clone();
        match LiveWorker::start(
            profile,
            session.clone(),
            Arc::new(move || repaint.request_repaint()),
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.connection = LiveConnection::Connecting;
                self.reason = None;
                true
            }
            Err(_) => {
                self.connection = LiveConnection::Offline;
                self.reason = Some(
                    "Chưa khởi tạo được gửi nhận. Hãy thử kết nối lại; bản nháp được giữ nguyên."
                        .into(),
                );
                false
            }
        }
    }

    #[cfg(feature = "messaging")]
    fn observe_status(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::Connection { state, .. } => {
                self.connection = *state;
                if *state == LiveConnection::Online {
                    self.reason = None;
                }
            }
            LiveEvent::Diagnostic { diagnostic, .. } => self.reason = Some(diagnostic.to_string()),
            LiveEvent::Fault { error, .. } => {
                if self.connection != LiveConnection::Expired {
                    self.connection = LiveConnection::Offline;
                }
                // A terminal generic fault must not hide the preceding typed diagnostic.
                self.reason.get_or_insert_with(|| error.clone());
                self.delivery.interrupted();
            }
            _ => {}
        }
    }

    #[cfg(feature = "messaging")]
    fn worker_closed(&mut self, model: &mut Model, drafts: &mut DraftBook, latest_page: bool) {
        let reason = self
            .reason
            .get_or_insert_with(|| {
                "Tiến trình gửi nhận đã dừng. Chọn Kết nối lại; bản nháp được giữ nguyên.".into()
            })
            .clone();
        if let Some(account) = &self.account {
            let event = LiveEvent::Fault {
                account_id: account.id.clone(),
                error: reason,
            };
            drafts.apply_live(&event, &account.id);
            model.apply_live(event, &account.id, latest_page);
        }
        self.delivery.interrupted();
        self.directory.worker_closed();
        self.sender_names.worker_closed();
        self.hydration.worker_closed();
        if self.connection != LiveConnection::Expired {
            self.connection = LiveConnection::Offline;
        }
        self.worker = None;
    }

    pub fn connection_label(&self) -> &'static str {
        #[cfg(feature = "messaging")]
        return match self.connection {
            LiveConnection::Connecting => "Đang kết nối…",
            LiveConnection::Online => "Đã kết nối Zalo",
            LiveConnection::Reconnecting => "Đang kết nối lại…",
            LiveConnection::Offline => "Ngoại tuyến",
            LiveConnection::Expired => "Phiên đã hết hạn",
        };
        #[cfg(not(feature = "messaging"))]
        "Đã đăng nhập · gửi nhận chưa được biên dịch"
    }

    pub fn can_send(&self, thread: Option<&str>) -> bool {
        #[cfg(feature = "messaging")]
        return self.account.is_some()
            && self.worker.is_some()
            && self.connection == LiveConnection::Online
            && thread.is_some_and(|thread| self.delivery.can_send(thread));
        #[cfg(not(feature = "messaging"))]
        {
            let _ = thread;
            false
        }
    }
}

impl NativeApp {
    #[cfg(feature = "messaging")]
    pub(super) fn live_connection_notice(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let Some(state) = &mut self.live else {
            return;
        };
        if state.account.is_none() || state.connection == LiveConnection::Online {
            return;
        }
        let mut action = None;
        egui::TopBottomPanel::top("live_connection_notice")
            .min_height(90.0)
            .frame(
                egui::Frame::none()
                    .fill(colors.selected)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                action = recovery_controls(
                    ui,
                    state.connection,
                    state.worker.is_some(),
                    state.reason.as_deref(),
                );
            });
        match action {
            Some(RecoveryAction::Retry) => {
                if let Some(worker) = &state.worker {
                    match worker.submit(LiveCommand::Reconnect) {
                        Ok(()) => {
                            state.connection = LiveConnection::Reconnecting;
                            state.reason = None;
                        }
                        Err(error) => self.model.status = error.to_string(),
                    }
                } else {
                    state.ensure_worker(self._profile.clone());
                }
            }
            Some(RecoveryAction::Open) => {
                state.ensure_worker(self._profile.clone());
                state.open_dialog = true;
            }
            Some(RecoveryAction::Logout) => state.logout_dialog = true,
            None => {}
        }
    }

    pub(super) fn live_information(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(state) = self.live.as_mut() else {
            return false;
        };
        let Some(account) = &state.account else {
            return false;
        };
        ui.label(
            egui::RichText::new(&account.name)
                .family(egui::FontFamily::Name("Zalo Semibold".into())),
        );
        ui.label(format!("ID Zalo: {}", account.id));
        ui.label(state.connection_label());
        ui.add_space(10.0);
        ui.label("Tin nhắn chỉ hiện xác nhận khi Zalo đã nhận và lưu được trên thiết bị. Xác nhận này chưa có nghĩa người nhận đã xem.");
        ui.add_space(8.0);
        ui.label(
            "Gọi điện, ảnh, tệp và nhập lịch sử chưa được hỗ trợ. Dữ liệu Electron được giữ riêng.",
        );
        ui.add_space(10.0);
        if ui.button("Đăng xuất").clicked() {
            state.logout_dialog = true;
        }
        true
    }

    pub(super) fn live_account_footer(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
        let colors = crate::theme::palette(ui.ctx());
        let Some(state) = self.live.as_mut() else {
            return false;
        };
        let Some(account) = state.account.as_ref() else {
            return false;
        };
        let mut content =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(14.0, 8.0))));
        let mut logout = false;
        content.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((rect.width() - 115.0).max(80.0));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if account.name.is_empty() {
                            &account.id
                        } else {
                            &account.name
                        })
                        .size(13.0),
                    )
                    .truncate(),
                )
                .on_hover_text(format!("{} · ID {}", account.name, account.id));
                ui.label(
                    egui::RichText::new(state.connection_label())
                        .size(10.0)
                        .color(colors.text_secondary),
                );
            });
            if ui.small_button("Đăng xuất").clicked() {
                logout = true;
            }
        });
        if logout {
            state.logout_dialog = true;
        }
        true
    }

    pub fn new_live(
        cc: &eframe::CreationContext<'_>,
        profile: Profile,
        trust: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        Self::new_live_with_factory(cc, profile, trust, LoginController::new)
    }

    fn new_live_with_factory(
        cc: &eframe::CreationContext<'_>,
        profile: Profile,
        trust: Option<PathBuf>,
        factory: impl FnOnce(
            Arc<Profile>,
            Option<PathBuf>,
            Arc<dyn Fn() + Send + Sync>,
        ) -> anyhow::Result<LoginController>,
    ) -> anyhow::Result<Self> {
        let mut app = Self::new(cc, profile, false)?;
        let repaint = cc.egui_ctx.clone();
        let controller = factory(
            app._profile.clone(),
            trust,
            Arc::new(move || repaint.request_repaint()),
        )?;
        let mut state = LiveState::with_controller(controller, &cc.egui_ctx);
        let generation = state.controller.startup()?;
        state.login.start(generation, true);
        app.live = Some(state);
        app.model.status.clear();
        Ok(app)
    }

    pub(super) fn live_signed_in(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|state| state.account.is_some())
    }

    pub(super) fn show_login(&mut self, ctx: &egui::Context) -> bool {
        let colors = crate::theme::palette(ctx);
        let Some(state) = self.live.as_mut() else {
            return false;
        };
        if state.account.is_some() {
            return false;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(colors.surface))
            .show(ctx, |ui| {
                super::visuals::background(ui, ui.max_rect(), "--layer-background", colors.surface);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::visuals::icon_button(
                        ui,
                        super::visuals::Icon::Settings,
                        "Cài đặt giao diện",
                        true,
                    )
                    .clicked()
                    {
                        self.settings_open = true;
                    }
                });
                if let Some(action) = state.login.show(ui, true) {
                    state.action(action);
                }
            });
        true
    }

    pub(super) fn poll_live(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.live.take() else {
            return;
        };
        #[cfg(feature = "messaging")]
        if let Some(account) = state.account.as_ref() {
            state.sender_names.sync_scope(&account.id, &self.model, ctx);
            state.hydration.sync_scope(&account.id, &self.model, ctx);
            if let Some(thread) = self.model.selected.as_deref() {
                state
                    .media
                    .scope(&account.id, thread, self.model.generation);
            }
        }
        let deadline = Instant::now() + Duration::from_millis(3);
        let mut remaining = EVENTS_PER_FRAME;
        while remaining > 0 && Instant::now() < deadline {
            let Ok(event) = state.controller.events.try_recv() else {
                break;
            };
            remaining -= 1;
            match event {
                LoginEvent::Progress { generation, stage } => state.login.progress(
                    generation,
                    match stage {
                        LoginStage::CreatingQr => Phase::CreatingQr,
                        LoginStage::WaitingForScan => Phase::WaitingForScan,
                        LoginStage::WaitingForPhone => Phase::WaitingForPhone,
                        LoginStage::VerifyingTrust => Phase::VerifyingTrust,
                        LoginStage::Bootstrapping => Phase::Bootstrapping,
                        LoginStage::Restoring => Phase::Restoring,
                    },
                ),
                LoginEvent::Qr { generation, pixels } => {
                    if generation == state.login.generation()
                        && !state.login.pixels(
                            ctx,
                            generation,
                            pixels.width,
                            pixels.height,
                            &pixels.rgba,
                        )
                    {
                        state.login.failed(
                            state.controller.cancel(),
                            "Mã QR không hợp lệ. Hãy tạo mã mới.",
                        );
                    }
                }
                LoginEvent::Ready {
                    generation,
                    session,
                } => {
                    if generation != state.login.generation() || state.account.is_some() {
                        continue;
                    }
                    self.model = Model::default();
                    self.drafts = DraftBook::default();
                    self.search.clear();
                    self.search_folded.clear();
                    self.latest_page = true;
                    self.ime_composing = false;
                    state.account = Some(Account {
                        id: session.account_id().to_owned(),
                        name: session.display_name().to_owned(),
                    });
                    #[cfg(feature = "messaging")]
                    {
                        state.directory = Default::default();
                        state.stickers = Default::default();
                        state.media = Default::default();
                        state.sender_names = Default::default();
                        state.hydration = Default::default();
                        state.session = Some(session);
                        state.ensure_worker(self._profile.clone());
                    }
                }
                LoginEvent::SignedOut { generation } => {
                    if generation != state.login.generation() {
                        continue;
                    }
                    state.login.signed_out(generation);
                    state.account = None;
                    #[cfg(feature = "messaging")]
                    {
                        state.worker = None;
                        state.session = None;
                        state.reason = None;
                        state.connection = LiveConnection::Offline;
                        state.delivery = Default::default();
                        state.directory = Default::default();
                        state.stickers = Default::default();
                        state.media = Default::default();
                        state.sender_names = Default::default();
                        state.hydration = Default::default();
                        state.review_dialog = false;
                    }
                    self.model = Model::default();
                    self.drafts = DraftBook::default();
                }
                LoginEvent::Failed { generation, error } => {
                    state.login.failed(generation, error.to_string())
                }
            }
        }
        #[cfg(feature = "messaging")]
        state.directory.poll_request(state.worker.as_ref(), ctx);
        #[cfg(feature = "messaging")]
        while remaining > 0 && Instant::now() < deadline {
            let event = state
                .worker
                .as_mut()
                .and_then(|worker| worker.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            remaining -= 1;
            let Some(account_id) = state.account.as_ref().map(|account| account.id.clone()) else {
                continue;
            };
            if event.account_id() != account_id {
                continue;
            }
            state.sender_names.apply(&event, &account_id, &self.model);
            state
                .hydration
                .apply(&event, &account_id, &mut self.model, ctx);
            if matches!(&event, LiveEvent::DirectoryChanged { .. }) {
                state.hydration.invalidate(&account_id, &self.model, ctx);
            }
            let refresh_senders = matches!(
                &event,
                LiveEvent::Page { .. }
                    | LiveEvent::Received { .. }
                    | LiveEvent::Sent { .. }
                    | LiveEvent::DirectoryChanged { .. }
                    | LiveEvent::Ready { .. }
            );
            let force_names = matches!(&event, LiveEvent::DirectoryChanged { .. });
            state.directory.apply(&event, &account_id, ctx);
            state.observe_status(&event);
            match &event {
                LiveEvent::SendFailed {
                    thread,
                    request_id,
                    unknown_delivery: true,
                    server_accepted,
                    ..
                } => {
                    state.delivery.failed(thread, *request_id, *server_accepted);
                    if self.model.selected.as_deref() == Some(thread) {
                        state.delivery.edited(thread, &self.drafts.active.text);
                        state.review_dialog = true;
                    }
                }
                LiveEvent::Sent {
                    thread, request_id, ..
                } => state.delivery.sent(thread, *request_id),
                _ => {}
            }
            let opened = match &event {
                LiveEvent::Opened {
                    generation,
                    conversation,
                    ..
                } if *generation == self.model.generation => Some(conversation.id.clone()),
                _ => None,
            };
            self.drafts.apply_live(&event, &account_id);
            if self.model.apply_live(event, &account_id, self.latest_page) && refresh_senders {
                state
                    .sender_names
                    .prepare(&account_id, &self.model, force_names, ctx);
            }
            if let Some(thread) = opened {
                let _ = self.drafts.select(&thread);
                if let Some(worker) = &state.worker {
                    let _ = self
                        .model
                        .select_live_if_accepted(thread, |command| worker.submit(command));
                }
            }
            if self.model.selected.is_none() {
                if let Some(thread) = self
                    .model
                    .conversations
                    .first()
                    .map(|thread| thread.id.clone())
                {
                    if let Some(worker) = &state.worker {
                        if self
                            .model
                            .select_live_if_accepted(thread.clone(), |command| {
                                worker.submit(command)
                            })
                            .is_ok()
                        {
                            let _ = self.drafts.select(&thread);
                        }
                    }
                }
            }
        }
        #[cfg(feature = "messaging")]
        state.sender_names.poll(state.worker.as_ref(), ctx);
        #[cfg(feature = "messaging")]
        {
            if let Some(account) = state.account.as_ref() {
                state.hydration.sync_scope(&account.id, &self.model, ctx);
            }
            state.hydration.poll(state.worker.as_ref(), ctx);
        }
        #[cfg(feature = "messaging")]
        if state
            .worker
            .as_ref()
            .is_some_and(|worker| worker.events.is_closed() && worker.events.is_empty())
        {
            state.worker_closed(&mut self.model, &mut self.drafts, self.latest_page);
        }
        #[cfg(feature = "messaging")]
        state.stickers.poll(ctx, deadline, &mut remaining);
        #[cfg(feature = "messaging")]
        state.media.poll(ctx, deadline, &mut remaining);
        if !state.controller.events.is_empty() {
            ctx.request_repaint();
        }
        #[cfg(feature = "messaging")]
        if state
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.events.is_empty())
        {
            ctx.request_repaint();
        }
        self.live = Some(state);
    }

    pub(super) fn live_dialogs(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.live else {
            return;
        };
        if state.logout_dialog {
            let mut open = true;
            let mut accepted = false;
            egui::Window::new("Đăng xuất Zalo").open(&mut open).collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                    ui.set_max_width(350.0);
                    ui.label("Đăng xuất và xóa phiên đã lưu trên thiết bị này? Lịch sử được giữ riêng trong hồ sơ hiện tại.");
                    ui.add_space(8.0);
                    if ui.button("Đăng xuất").clicked() { accepted = true; }
                });
            if accepted {
                match state.controller.logout() {
                    Ok(generation) => {
                        state.login.signed_out(generation);
                        state.account = None;
                        #[cfg(feature = "messaging")]
                        {
                            state.worker = None;
                            state.session = None;
                            state.reason = None;
                            state.connection = LiveConnection::Offline;
                            state.delivery = Default::default();
                            state.directory = Default::default();
                            state.stickers = Default::default();
                            state.media = Default::default();
                            state.sender_names = Default::default();
                            state.hydration = Default::default();
                            state.review_dialog = false;
                        }
                        self.model = Model::default();
                        self.drafts = DraftBook::default();
                        open = false;
                    }
                    Err(_) => self.model.status = "Chưa thể đăng xuất. Vui lòng thử lại.".into(),
                }
            }
            state.logout_dialog = open;
        }
        #[cfg(feature = "messaging")]
        if state.open_dialog
            && show_target_dialog(
                state,
                &mut self.model,
                &mut self.drafts,
                &mut self.latest_page,
                ctx,
            )
        {
            self.compact_list = false;
        }
        #[cfg(feature = "messaging")]
        if state.review_dialog {
            if let Some(thread) = self.model.selected.as_deref() {
                if let Some(review) = state.delivery.review(thread) {
                    let mut open = true;
                    let mut discard = false;
                    egui::Window::new("Kiểm tra trạng thái gửi tin").open(&mut open).collapsible(false).resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                            ui.set_max_width(380.0);
                            ui.label(if review.accepted {
                                "Zalo đã nhận tin nhắn, nhưng chưa lưu được xác nhận trên thiết bị. Không gửi lại nội dung này. Bản nháp được giữ để bạn kiểm tra."
                            } else {
                                "Yêu cầu gửi bị ngắt giữa chừng. Tin nhắn có thể đã tới Zalo; bản nháp được giữ lại. Client sẽ không tự gửi lại nội dung chưa rõ trạng thái."
                            });
                            ui.add_space(8.0);
                            ui.label("Bạn có thể sửa bản nháp để soạn tin khác, hoặc kiểm tra hội thoại trước khi xóa bản nháp này.");
                            if review.same_body && ui.button("Xóa bản nháp này").clicked() { discard = true; }
                        });
                    if discard {
                        self.drafts.active.text.clear();
                        self.drafts.active.edited();
                        state.delivery.edited(thread, "");
                        open = false;
                    }
                    state.review_dialog = open;
                } else {
                    state.review_dialog = false;
                }
            }
        }
    }
}

#[cfg(feature = "messaging")]
fn show_target_dialog(
    state: &mut LiveState,
    model: &mut Model,
    drafts: &mut DraftBook,
    latest: &mut bool,
    ctx: &egui::Context,
) -> bool {
    let colors = crate::theme::palette(ctx);
    let mut accepted = false;
    let mut open = state.open_dialog;
    let mut requested = false;
    egui::Window::new("Mở hội thoại")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(350.0)
        .show(ctx, |ui| {
            ui.label("Mở bằng ID Zalo khi danh bạ chưa được đồng bộ.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.radio_value(&mut state.group, false, "Cá nhân");
                ui.radio_value(&mut state.group, true, "Nhóm");
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.target)
                    .hint_text(if state.group {
                        "ID nhóm"
                    } else {
                        "ID người nhận"
                    })
                    .char_limit(24)
                    .desired_width(320.0),
            );
            let valid = crate::protocol::messaging::Target::new(
                if state.group {
                    ThreadKind::Group
                } else {
                    ThreadKind::Direct
                },
                state.target.trim(),
            )
            .is_ok();
            if !state.target.trim().is_empty() && !valid {
                ui.label(
                    egui::RichText::new("ID chỉ gồm chữ số và không bắt đầu bằng 0.")
                        .size(11.0)
                        .color(colors.text_secondary),
                );
            }
            if ui
                .add_enabled(
                    valid && state.worker.is_some(),
                    egui::Button::new("Mở hội thoại"),
                )
                .clicked()
            {
                requested = true;
            }
        });
    if requested {
        let kind = if state.group {
            ThreadKind::Group
        } else {
            ThreadKind::Direct
        };
        if let Ok(target) = crate::protocol::messaging::Target::new(kind, state.target.trim()) {
            let thread = target.thread_id();
            if !drafts.can_select(&thread) {
                model.status = "Đã đạt giới hạn bản nháp đang mở.".into();
            } else if let Some(worker) = &state.worker {
                let generation = model.generation.wrapping_add(1);
                match worker.submit(LiveCommand::Open {
                    generation,
                    kind,
                    remote_id: target.remote_id,
                }) {
                    Ok(()) => {
                        model.generation = generation;
                        model.selected = Some(thread.clone());
                        model.messages.clear();
                        model.has_older = false;
                        let _ = drafts.select(&thread);
                        state.delivery.edited(&thread, &drafts.active.text);
                        model.pending = true;
                        *latest = true;
                        accepted = true;
                        open = false;
                    }
                    Err(error) => model.status = error.to_string(),
                }
            }
        }
    }
    state.open_dialog = open;
    accepted
}

#[cfg(feature = "messaging")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryAction {
    Retry,
    Open,
    Logout,
}

/// The same recovery controls remain reachable when the compact sidebar is hidden.
#[cfg(feature = "messaging")]
fn recovery_controls(
    ui: &mut egui::Ui,
    connection: LiveConnection,
    worker_alive: bool,
    reason: Option<&str>,
) -> Option<RecoveryAction> {
    let colors = crate::theme::palette(ui.ctx());
    let mut action = None;
    let active = matches!(
        connection,
        LiveConnection::Connecting | LiveConnection::Reconnecting
    );
    let expired = connection == LiveConnection::Expired;
    ui.spacing_mut().item_spacing.y = 5.0;
    ui.label(
        egui::RichText::new(if expired {
            "Phiên đăng nhập đã hết hạn"
        } else if !worker_alive {
            "Gửi nhận đã dừng · vẫn giữ bản nháp"
        } else if active {
            "Đang kết nối tới Zalo…"
        } else {
            "Ngoại tuyến · vẫn có thể đọc tin đã lưu và soạn bản nháp"
        })
        .size(12.0)
        .family(egui::FontFamily::Name("Zalo Semibold".into()))
        .color(colors.text_primary),
    );
    if let Some(reason) = reason {
        ui.add(egui::Label::new(egui::RichText::new(reason).size(11.0).color(colors.error)).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !active && !expired,
                egui::Button::new("Kết nối lại").small(),
            )
            .clicked()
        {
            action = Some(RecoveryAction::Retry);
        }
        if ui.small_button("Mở hội thoại").clicked() {
            action = Some(RecoveryAction::Open);
        }
        if ui
            .small_button(if expired {
                "Đăng nhập lại"
            } else {
                "Đăng xuất"
            })
            .clicked()
        {
            action = Some(RecoveryAction::Logout);
        }
    });
    action
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::{
        domain::Conversation,
        protocol::{bootstrap::ServerConfig, session::synthetic_session},
    };

    fn fixture(ctx: &egui::Context) -> (NativeApp, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "zalo-offline-ui-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let profile = Arc::new(Profile::at_root(&root, "fixture").unwrap());
        super::super::visuals::install(ctx);
        let mut state = LiveState::new(profile.clone(), None, ctx).unwrap();
        // No endpoint, cookie or real account can escape this test-only capability.
        let session = Arc::new(synthetic_session(
            profile.clone(),
            ServerConfig {
                trusted_qr: false,
                server_time_seconds: None,
                parameter_encryption: None,
                services: Default::default(),
                chat_poll: vec![],
                control_poll: vec![],
                websocket: vec![],
            },
        ));
        state.account = Some(Account {
            id: session.account_id().into(),
            name: session.display_name().into(),
        });
        state.session = Some(session);
        state.reason = Some("Kết nối TLS chưa hoàn tất.".into());
        (
            NativeApp {
                worker: None,
                model: Model::default(),
                drafts: DraftBook::default(),
                demo: false,
                search: String::new(),
                search_folded: String::new(),
                compact_list: false,
                details: false,
                settings_open: false,
                theme: crate::theme::ThemeController::new(
                    profile.paths.config_dir.clone(),
                    Arc::new(|| {}),
                )
                .unwrap(),
                settings: super::super::settings::Settings::default(),
                fixture_system_theme: None,
                focus_composer: false,
                latest_page: true,
                ime_composing: false,
                live: Some(state),
                _profile: profile,
            },
            root,
        )
    }

    fn render(app: &mut NativeApp, ctx: &egui::Context, width: f32) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 480.0),
                )),
                ..Default::default()
            },
            |ctx| app.workspace(ctx),
        )
    }

    fn text_rect(output: &egui::FullOutput, expected: &str) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
    }

    #[test]
    fn signed_in_empty_offline_and_stopped_worker_have_visible_recovery_at_both_widths() {
        for width in [640.0, 1200.0] {
            let ctx = egui::Context::default();
            let (mut app, root) = fixture(&ctx);
            for worker_alive in [false, true] {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 480.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            recovery_controls(
                                ui,
                                LiveConnection::Offline,
                                worker_alive,
                                Some("Kết nối TLS chưa hoàn tất."),
                            );
                        });
                    },
                );
                for label in [
                    "Kết nối lại",
                    "Mở hội thoại",
                    "Đăng xuất",
                    "Kết nối TLS chưa hoàn tất.",
                ] {
                    let rect = text_rect(&output, label)
                        .unwrap_or_else(|| panic!("missing {label} at {width}"));
                    assert!(rect.bottom() <= 480.0 && rect.right() <= width);
                }
            }
            let output = render(&mut app, &ctx, width);
            for label in [
                "Kết nối lại",
                "Mở hội thoại",
                "Đăng xuất",
                "Mở hội thoại bằng ID",
            ] {
                let rect = text_rect(&output, label).unwrap_or_else(|| {
                    panic!(
                        "workspace missing {label} at {width}; texts={:?}",
                        output
                            .shapes
                            .iter()
                            .filter_map(|s| match &s.shape {
                                egui::epaint::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    )
                });
                assert!(rect.bottom() <= 480.0 && rect.right() <= width);
            }
            assert!(app.is_signed_in());
            assert!(!app.live_can_send());
            assert!(app.model.conversations.is_empty());
            assert!(
                app.live.as_ref().unwrap().worker.is_none(),
                "rendering must not start network"
            );
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn offline_editor_keeps_cached_thread_and_draft_without_enabling_send() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let thread = "d:22";
        app.model.conversations.push(Conversation {
            id: thread.into(),
            title: "Hội thoại kiểm thử".into(),
            last_message_at_ms: 0,
            last_text: String::new(),
            last_sender: String::new(),
        });
        app.model.selected = Some(thread.into());
        app.drafts.select(thread).unwrap();
        app.drafts.active.text = "Bản nháp riêng".into();
        for width in [640.0, 1200.0] {
            let output = render(&mut app, &ctx, width);
            assert!(
                text_rect(&output, "Bản nháp riêng").is_some(),
                "offline draft must remain in actual editor at {width}"
            );
            assert_eq!(app.model.selected.as_deref(), Some(thread));
            assert_eq!(app.drafts.active.text, "Bản nháp riêng");
            assert!(!app.live_can_send());
        }
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn theme_switch_keeps_selected_draft_and_editor_scope_in_wide_and_compact_views() {
        use crate::theme::ThemeMode;
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Tiếng Việt đang soạn\nDòng thứ hai".into();
        let expected = app.drafts.active.text.clone();
        for mode in [
            ThemeMode::Light,
            ThemeMode::Dark,
            ThemeMode::System,
            ThemeMode::Custom,
        ] {
            app.theme.set_mode(mode);
            if mode == ThemeMode::Custom {
                app.theme.set_override("--text-primary", "#6F3FCF").unwrap();
            }
            for width in [640.0, 1200.0] {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 480.0),
                        )),
                        system_theme: Some(egui::Theme::Dark),
                        ..Default::default()
                    },
                    |ctx| {
                        app.theme.poll(ctx);
                        app.workspace(ctx);
                    },
                );
                assert_eq!(app.drafts.active.text, expected);
                assert_eq!(app.model.selected.as_deref(), Some("d:22"));
                assert!(!app.live_can_send());
                assert!(text_rect(&output, &expected).is_some());
                let palette = crate::theme::palette(&ctx);
                assert_eq!(
                    palette.dark,
                    matches!(mode, ThemeMode::Dark | ThemeMode::System)
                );
                if mode == ThemeMode::Custom {
                    assert_eq!(palette.text_primary, egui::Color32::from_rgb(111, 63, 207));
                }
            }
        }
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn offline_recovery_buttons_respond_to_pointer_clicks() {
        let ctx = egui::Context::default();
        super::super::visuals::install(&ctx);
        for (label, expected) in [
            ("Kết nối lại", RecoveryAction::Retry),
            ("Mở hội thoại", RecoveryAction::Open),
            ("Đăng xuất", RecoveryAction::Logout),
        ] {
            let mut action = None;
            let mut draw = |events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 480.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            action = recovery_controls(ui, LiveConnection::Offline, false, None);
                        });
                    },
                )
            };
            let output = draw(vec![]);
            let pos = text_rect(&output, label).unwrap().center();
            draw(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            draw(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
            assert_eq!(action, Some(expected));
        }
    }
    #[test]
    fn terminal_worker_close_preserves_typed_reason_fault_and_expiration() {
        use crate::app::live::{LiveCategory, LiveDiagnostic, LiveStage};
        for expired in [false, true] {
            let ctx = egui::Context::default();
            let (mut app, root) = fixture(&ctx);
            let state = app.live.as_mut().unwrap();
            state.reason = None;
            let diagnostic = LiveDiagnostic {
                stage: LiveStage::Authentication,
                category: LiveCategory::Http,
                http_status: Some(403),
                server_code: None,
                parser_reason: None,
            };
            let typed = LiveEvent::Diagnostic {
                account_id: "11".into(),
                diagnostic,
            };
            state.observe_status(&typed);
            app.model.apply_live(typed, "11", true);
            if expired {
                let event = LiveEvent::Connection {
                    account_id: "11".into(),
                    state: LiveConnection::Expired,
                };
                state.observe_status(&event);
                app.model.apply_live(event, "11", true);
            }
            let fault = LiveEvent::Fault {
                account_id: "11".into(),
                error: "Tiến trình đã dừng".into(),
            };
            state.observe_status(&fault);
            app.model.apply_live(fault, "11", true);
            state.worker_closed(&mut app.model, &mut app.drafts, true);
            assert_eq!(
                state.reason.as_deref(),
                Some(diagnostic.to_string().as_str())
            );
            assert_eq!(app.model.status, diagnostic.to_string());
            assert_eq!(
                state.connection,
                if expired {
                    LiveConnection::Expired
                } else {
                    LiveConnection::Offline
                }
            );
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let state = app.live.as_mut().unwrap();
        state.reason = None;
        let fault = LiveEvent::Fault {
            account_id: "11".into(),
            error: "Không mở được kho tin nhắn trên thiết bị".into(),
        };
        state.observe_status(&fault);
        app.model.apply_live(fault, "11", true);
        state.worker_closed(&mut app.model, &mut app.drafts, true);
        assert_eq!(app.model.status, "Không mở được kho tin nhắn trên thiết bị");
        assert_eq!(state.reason.as_deref(), Some(app.model.status.as_str()));
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn committed_directory_names_refresh_sidebar_without_replacing_selected_history_or_draft() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let thread = "d:22";
        let original = crate::domain::Message {
            sticker: None,
            media: None,
            account_id: "11".into(),
            thread_id: thread.into(),
            id: "501".into(),
            sender: "22".into(),
            text: "Lịch sử đã lưu".into(),
            sent_at_ms: 100,
        };
        app.model.selected = Some(thread.into());
        app.model.messages = vec![original.clone()];
        app.drafts.select(thread).unwrap();
        app.drafts.active.text = "Bản nháp không đổi".into();
        let event = LiveEvent::DirectoryChanged {
            account_id: "11".into(),
            conversations: vec![Conversation {
                id: thread.into(),
                title: "Đặng Minh".into(),
                last_message_at_ms: 100,
                last_text: original.text.clone(),
                last_sender: "22".into(),
            }],
        };
        app.live
            .as_mut()
            .unwrap()
            .directory
            .apply(&event, "11", &ctx);
        app.model.apply_live(event, "11", true);
        assert_eq!(app.model.conversations[0].title, "Đặng Minh");
        assert_eq!(app.model.selected.as_deref(), Some(thread));
        assert_eq!(app.model.messages, vec![original]);
        assert_eq!(app.drafts.active.text, "Bản nháp không đổi");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn named_directory_open_works_offline_and_restores_the_previous_draft() {
        use crate::store::directory::{DirectoryBatch, DirectoryRecord};
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let entry = DirectoryRecord {
            remote_id: "33".into(),
            is_group: false,
            display_name: "Nguyễn Hà".into(),
            avatar_url: None,
            is_friend: Some(true),
            is_blocked: None,
            member_count: None,
        };
        let mut store =
            crate::store::MessageStore::open(&app._profile.paths.data_dir.join("messages.sqlite"))
                .unwrap();
        store
            .apply_directory_batch(&DirectoryBatch {
                message_info: Vec::new(),
                message_media: Vec::new(),
                account_id: "11".into(),
                entries: vec![entry.clone()],
                messages: vec![],
                observed_at_ms: 100,
            })
            .unwrap();
        drop(store);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Bản nháp hội thoại trước".into();
        app.live.as_mut().unwrap().directory.visible = true;
        app.open_directory_entry(entry);
        assert!(!app.directory_visible());
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            app.poll_live(&ctx);
            if !app.model.pending
                && app
                    .model
                    .conversations
                    .iter()
                    .any(|c| c.id == "d:33" && c.title == "Nguyễn Hà")
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(app.model.selected.as_deref(), Some("d:33"));
        assert!(app
            .model
            .conversations
            .iter()
            .any(|c| c.id == "d:33" && c.title == "Nguyễn Hà"));
        assert!(
            app.model.messages.is_empty(),
            "a named contact must not manufacture history"
        );
        assert!(
            !app.live_can_send(),
            "cache operations must not imply Online"
        );
        assert!(app.load_latest("d:22".into()));
        assert_eq!(app.drafts.active.text, "Bản nháp hội thoại trước");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn online_incoming_chat_accepts_pointer_focus_and_keyboard_input() {
        for (width, height) in [(1200.0, 780.0), (640.0, 480.0)] {
            for connection in [LiveConnection::Online, LiveConnection::Offline] {
                let ctx = egui::Context::default();
                let (mut app, root) = fixture(&ctx);
                app.live.as_mut().unwrap().connection = connection;
                let message = crate::domain::Message {
                    sticker: None,
                    media: None,
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    id: "incoming-1".into(),
                    sender: "22".into(),
                    text: "Tin nhận kiểm thử".into(),
                    sent_at_ms: 1700000000000,
                };
                app.model.apply_live(
                    LiveEvent::Received {
                        account_id: "11".into(),
                        message: message.clone(),
                        conversation: Conversation {
                            id: "d:22".into(),
                            title: "Liên hệ kiểm thử".into(),
                            last_message_at_ms: message.sent_at_ms,
                            last_text: message.text.clone(),
                            last_sender: "22".into(),
                        },
                    },
                    "11",
                    true,
                );
                app.model.selected = Some("d:22".into());
                app.drafts.select("d:22").unwrap();
                let mut frame = |events| {
                    ctx.run(
                        egui::RawInput {
                            events,
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.workspace(ctx),
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                let rect = text_rect(&output, "Nhập tin nhắn…").expect("editor must be visible");
                let pos = egui::pos2(width - 250.0, height - 25.0);
                frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ]);
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }]);
                frame(vec![egui::Event::Text("Có thể gõ tin nhắn".into())]);
                assert_eq!(
                    app.drafts.active.text, "Có thể gõ tin nhắn",
                    "typing at {width}x{height}, {connection:?}, editor={rect:?}"
                );
                assert!(
                    !app.live_can_send(),
                    "no worker must never become a verified send route"
                );
                drop(app);
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }

    #[test]
    fn composition_and_commit_frames_cannot_send_but_later_fresh_shortcut_can() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        app.live = None;
        app.demo = true;
        app.worker =
            Some(crate::app::Worker::demo(root.join("ime-demo.sqlite"), Arc::new(|| {})).unwrap());
        app.model.selected = Some("welcome".into());
        app.drafts.select("welcome").unwrap();
        app.focus_composer = true;
        fn frame(app: &mut NativeApp, ctx: &egui::Context, events: Vec<egui::Event>) {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ctx.input(|input| {
                        super::super::update_ime_state(&mut app.ime_composing, &input.events)
                    });
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx, 150.0));
                },
            );
        }
        fn shortcut(repeat: bool) -> egui::Event {
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat,
                modifiers: egui::Modifiers::COMMAND,
            }
        }
        frame(&mut app, &ctx, vec![]);
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("việt".into())),
            ],
        );
        frame(&mut app, &ctx, vec![shortcut(false)]);
        assert!(!app.drafts.active.in_flight(), "active preedit cannot send");
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Commit("việt".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
                shortcut(false),
            ],
        );
        assert!(
            !app.drafts.active.in_flight(),
            "commit and shortcut in the same frame cannot send"
        );
        frame(&mut app, &ctx, vec![shortcut(true)]);
        assert!(
            !app.drafts.active.in_flight(),
            "held key after commit cannot send"
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        frame(&mut app, &ctx, vec![shortcut(false)]);
        assert!(
            app.drafts.active.in_flight(),
            "fresh shortcut outside composition must still submit"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_drafts_have_distinct_ime_cursor_and_undo_state() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Bản nháp A ".into();
        fn frame(app: &mut NativeApp, ctx: &egui::Context, events: Vec<egui::Event>) {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx, 150.0));
                },
            );
        }
        app.focus_composer = true;
        frame(&mut app, &ctx, vec![]);
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("việt".into())),
            ],
        );
        let first = ctx.memory(|m| m.focused()).unwrap();
        app.model.selected = Some("d:33".into());
        app.drafts.select("d:33").unwrap();
        app.drafts.active.text = "Bản nháp B nguyên vẹn".into();
        app.focus_composer = true;
        frame(&mut app, &ctx, vec![]);
        let second = ctx.memory(|m| m.focused()).unwrap();
        assert_ne!(
            first, second,
            "IME and undo must belong to the selected draft"
        );
        // The focus-aware bridge turns the old widget's delayed cancellation
        // into Disabled only, never empty Preedit against the new selection.
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
        );
        assert_eq!(app.drafts.active.text, "Bản nháp B nguyên vẹn");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn long_multiline_draft_is_clipped_to_editor_and_does_not_cover_actions() {
        for (width, height) in [(1200.0, 780.0), (640.0, 480.0)] {
            let ctx = egui::Context::default();
            let (mut app, root) = fixture(&ctx);
            app.live.as_mut().unwrap().connection = LiveConnection::Online;
            app.model.selected = Some("d:22".into());
            app.drafts.select("d:22").unwrap();
            app.drafts.active.text = (1..=20).map(|i| format!("Dòng kiểm thử {i}\n")).collect();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                },
                |ctx| app.workspace(ctx),
            );
            let shape = output
                .shapes
                .iter()
                .find(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) => {
                        text.galley.job.text.contains("Dòng kiểm thử 1")
                    }
                    _ => false,
                })
                .expect("long draft text must render");
            assert!(
                shape.clip_rect.bottom() <= height - 20.0,
                "draft cannot cover bottom shortcut hint"
            );
            assert!(
                shape.clip_rect.right() <= width - 116.0,
                "draft cannot cover send button"
            );
            assert_eq!(app.drafts.active.text.lines().count(), 20);
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    /// Native-window fixture exists only in the test binary, with no external endpoints.
    #[test]
    #[ignore = "requires isolated X11 display; QA closes the window via WM_DELETE_WINDOW"]
    fn signed_in_offline_native_window_fixture() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let width = std::env::var("ZALO_UI_FIXTURE_WIDTH")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| (640.0..=1600.0).contains(v))
            .unwrap_or(1200.0);
        let sticker = std::env::var_os("ZALO_UI_FIXTURE_STICKER").is_some();
        let online = std::env::var_os("ZALO_UI_FIXTURE_ONLINE").is_some();
        let directory = std::env::var_os("ZALO_UI_FIXTURE_DIRECTORY").is_some();
        let sender_mode = std::env::var("ZALO_UI_FIXTURE_SENDERS").ok();
        let info_mode = std::env::var_os("ZALO_UI_FIXTURE_INFO").is_some();
        let media_mode = std::env::var("ZALO_UI_FIXTURE_MEDIA").ok();
        let cached = media_mode.is_some()
            || sticker
            || directory
            || sender_mode.is_some()
            || std::env::var_os("ZALO_UI_FIXTURE_CACHED").is_some();
        let root = Arc::new(std::sync::Mutex::new(None));
        let cleanup = root.clone();
        eframe::run_native(
            "Zalo Native · Offline UI fixture",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([width, if width < 940.0 { 480.0 } else { 780.0 }])
                    .with_min_inner_size([640.0, 480.0]),
                event_loop_builder: Some(Box::new(|builder| {
                    builder.with_any_thread(true);
                })),
                ..Default::default()
            },
            Box::new(move |cc| {
                if let Ok(mode) = std::env::var("ZALO_UI_FIXTURE_LOGIN") {
                    let path = std::env::temp_dir().join(format!(
                        "native-startup-gui-{}",
                        crate::domain::local_message_id().unwrap()
                    ));
                    let profile = Profile::at_root(&path, "fixture")?;
                    *root.lock().unwrap() = Some(path);
                    return NativeApp::new_live_with_factory(
                        cc,
                        profile,
                        None,
                        move |profile, _, repaint| {
                            LoginController::startup_fixture(profile, repaint, &mode)
                        },
                    )
                    .map(|mut app| {
                        fixture_theme(&mut app, &cc.egui_ctx);
                        Box::new(app) as Box<dyn eframe::App>
                    })
                    .map_err(Into::into);
                }
                let (mut app, path) = fixture(&cc.egui_ctx);
                *root.lock().unwrap() = Some(path);
                if online {
                    app.live.as_mut().unwrap().connection = LiveConnection::Online;
                }
                if cached {
                    let messages: Vec<_> = [
                        ("d:22", "22", "Tin nhắn kiểm thử đã lưu"),
                        ("d:33", "33", "Hội thoại kiểm thử thứ hai"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (thread, sender, text))| crate::domain::Message {
                        sticker: None,
                        media: None,
                        account_id: "11".into(),
                        thread_id: thread.into(),
                        id: format!("fixture-{}", i + 1),
                        sender: sender.into(),
                        text: text.into(),
                        sent_at_ms: 1700000000000 + i as i64,
                    })
                    .collect();
                    let mut store = crate::store::MessageStore::open(
                        &app._profile.paths.data_dir.join("messages.sqlite"),
                    )
                    .unwrap();
                    store.insert_batch(&messages).unwrap();
                    app.model.conversations = messages
                        .iter()
                        .map(|m| Conversation {
                            id: m.thread_id.clone(),
                            title: m.sender.clone(),
                            last_message_at_ms: m.sent_at_ms,
                            last_text: m.text.clone(),
                            last_sender: m.sender.clone(),
                        })
                        .collect();
                    app.model.selected = Some("d:22".into());
                    app.model.messages = vec![messages[0].clone()];
                    app.drafts.select("d:22").unwrap();
                    app.drafts.active.text = "Bản nháp ngoại tuyến".into();
                }
                if sticker {
                    use crate::domain::{Message, StickerDescriptor, LEGACY_STICKER_PLACEHOLDER};
                    let descriptor = StickerDescriptor {
                        sticker_id: 1,
                        category_id: 0,
                        sticker_type: 7,
                    };
                    let unavailable = StickerDescriptor {
                        sticker_id: 1,
                        category_id: 10,
                        sticker_type: 3,
                    };
                    let mut pixels = vec![0u8; 130 * 130 * 4];
                    for y in 0..130i32 {
                        for x in 0..130i32 {
                            let r = (x - 65).pow(2) + (y - 65).pow(2);
                            if r <= 58i32.pow(2) {
                                let eyes = (x - 45).pow(2) + (y - 52).pow(2) < 25
                                    || (x - 85).pow(2) + (y - 52).pow(2) < 25;
                                let mouth = y > 72
                                    && y < 91
                                    && ((x - 65).pow(2) + (y - 60).pow(2)) > 760
                                    && ((x - 65).pow(2) + (y - 60).pow(2)) < 1000;
                                let color = if eyes || mouth {
                                    [8, 27, 58, 255]
                                } else {
                                    [255, 210, 70, 255]
                                };
                                pixels[((y * 130 + x) * 4) as usize
                                    ..((y * 130 + x) * 4 + 4) as usize]
                                    .copy_from_slice(&color);
                            }
                        }
                    }
                    let messages: Vec<_> = [descriptor, unavailable]
                        .into_iter()
                        .enumerate()
                        .map(|(i, sticker)| Message {
                            account_id: "11".into(),
                            thread_id: "d:22".into(),
                            id: format!("fixture-sticker-{i}"),
                            sender: "22".into(),
                            text: LEGACY_STICKER_PLACEHOLDER.into(),
                            sticker: Some(sticker),
                            media: None,
                            sent_at_ms: 1700000010000 + i as i64,
                        })
                        .collect();
                    let mut store = crate::store::MessageStore::open(
                        &app._profile.paths.data_dir.join("messages.sqlite"),
                    )
                    .unwrap();
                    store.insert_batch(&messages).unwrap();
                    app.model.messages.extend(messages);
                    app.model.conversations = store.conversations("11", 200).unwrap();
                    let state = app.live.as_mut().unwrap();
                    let mut encoded = Vec::new();
                    {
                        let mut encoder = png::Encoder::new(&mut encoded, 130, 130);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(&pixels)
                            .unwrap();
                    }
                    let (width, height, decoded) =
                        crate::app::stickers::decode_fixture_png(&encoded).unwrap();
                    state.stickers.seed(
                        &cc.egui_ctx,
                        descriptor,
                        width,
                        height,
                        decoded.as_ref().clone(),
                    );
                    state.stickers.seed_failure(&cc.egui_ctx, unavailable);
                }
                if directory {
                    use crate::store::directory::{DirectoryBatch, DirectoryRecord};
                    let mut entries: Vec<_> = [
                        ("22", "Đặng Minh", false),
                        ("33", "Nguyễn Hà", false),
                        ("22", "Nhóm dự án Việt", true),
                    ]
                    .into_iter()
                    .map(|(id, name, is_group)| DirectoryRecord {
                        remote_id: id.into(),
                        is_group,
                        display_name: name.into(),
                        avatar_url: None,
                        is_friend: if is_group { None } else { Some(true) },
                        is_blocked: None,
                        member_count: if is_group { Some(3) } else { None },
                    })
                    .collect();
                    entries.extend((1001..=1105).map(|id| DirectoryRecord {
                        remote_id: id.to_string(),
                        is_group: false,
                        display_name: format!("Liên hệ kiểm thử {id}"),
                        avatar_url: None,
                        is_friend: Some(true),
                        is_blocked: None,
                        member_count: None,
                    }));
                    let mut store = crate::store::MessageStore::open(
                        &app._profile.paths.data_dir.join("messages.sqlite"),
                    )
                    .unwrap();
                    store
                        .apply_directory_batch(&DirectoryBatch {
                            message_info: Vec::new(),
                            message_media: Vec::new(),
                            account_id: "11".into(),
                            entries,
                            messages: vec![],
                            observed_at_ms: 1700000000100,
                        })
                        .unwrap();
                    app.model.conversations = store.conversations("11", 200).unwrap();
                    app.activate_directory(&cc.egui_ctx);
                }
                if let Some(mode) = sender_mode.as_deref() {
                    use crate::{
                        domain::Message,
                        store::directory::{DirectoryBatch, DirectoryRecord},
                    };
                    let thread = if mode == "group" { "g:90" } else { "d:22" };
                    let entries = [
                        ("22", "Đặng Minh".to_owned(), false),
                        ("33", "Nguyễn Thị Ánh Hồng ".repeat(12), false),
                        ("90", "Nhóm dự án kiểm thử".to_owned(), true),
                    ]
                    .into_iter()
                    .map(|(id, name, is_group)| DirectoryRecord {
                        remote_id: id.into(),
                        is_group,
                        display_name: name,
                        avatar_url: None,
                        is_friend: if is_group { None } else { Some(true) },
                        is_blocked: None,
                        member_count: if is_group { Some(4) } else { None },
                    })
                    .collect();
                    let messages: Vec<_> = if mode == "group" {
                        vec![
                            ("22", "Tin nhắn từ người đã có trong danh bạ"),
                            ("33", "Tên dài vẫn nằm trong bong bóng tin nhắn"),
                            ("44", "Người chưa có tên trong danh bạ"),
                        ]
                    } else {
                        vec![("22", "Tin nhắn trực tiếp hiển thị đúng tên liên hệ")]
                    }
                    .into_iter()
                    .enumerate()
                    .map(|(i, (sender, text))| Message {
                        account_id: "11".into(),
                        thread_id: thread.into(),
                        id: format!("sender-fixture-{i}"),
                        sender: sender.into(),
                        text: text.into(),
                        sticker: None,
                        media: None,
                        sent_at_ms: 1700000000200 + i as i64,
                    })
                    .collect();
                    let mut store = crate::store::MessageStore::open(
                        &app._profile.paths.data_dir.join("messages.sqlite"),
                    )
                    .unwrap();
                    store
                        .apply_directory_batch(&DirectoryBatch {
                            message_info: Vec::new(),
                            message_media: Vec::new(),
                            account_id: "11".into(),
                            entries,
                            messages: vec![],
                            observed_at_ms: 1700000000300,
                        })
                        .unwrap();
                    store.insert_batch(&messages).unwrap();
                    app.model.conversations = store.conversations("11", 200).unwrap();
                    app.model.selected = Some(thread.into());
                    app.model.messages = messages;
                    app.drafts.select(thread).unwrap();
                    drop(store);
                    app.live
                        .as_mut()
                        .unwrap()
                        .ensure_worker(app._profile.clone());
                }
                if info_mode {
                    use crate::store::directory::{DirectoryBatch, DirectoryRecord};
                    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
                        "../../tests/fixtures/messaging/info.json"
                    ))
                    .unwrap();
                    let case = fixtures["cases"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|case| case["name"] == "self_and_named_member")
                        .unwrap();
                    let payload = serde_json::json!({"error_code":0,"data":{"groupMsgs":[{
                        "uidFrom":"22","idTo":"90","msgId":"843","ts":"1700000000200",
                        "msgType":"webchat","content":case["content"]
                    }]}});
                    let incoming =
                        crate::protocol::messaging::incoming_messages("11", &payload).unwrap();
                    assert_eq!(incoming[0].message.text, case["text"].as_str().unwrap());
                    let messages: Vec<_> = incoming
                        .into_iter()
                        .map(|incoming| (incoming.message, incoming.client_id, incoming.info))
                        .collect();
                    let entries = [
                        ("22", "Đặng Minh", false),
                        ("90", "Nhóm kiểm thử thông báo", true),
                    ]
                    .into_iter()
                    .map(|(id, name, is_group)| DirectoryRecord {
                        remote_id: id.into(),
                        is_group,
                        display_name: name.into(),
                        avatar_url: None,
                        is_friend: if is_group { None } else { Some(true) },
                        is_blocked: None,
                        member_count: if is_group { Some(4) } else { None },
                    })
                    .collect();
                    let mut store = crate::store::MessageStore::open(
                        &app._profile.paths.data_dir.join("messages.sqlite"),
                    )
                    .unwrap();
                    store
                        .apply_directory_batch(&DirectoryBatch {
                            message_info: Vec::new(),
                            message_media: Vec::new(),
                            account_id: "11".into(),
                            entries,
                            messages: vec![],
                            observed_at_ms: 1700000000300,
                        })
                        .unwrap();
                    store
                        .receive_batch_with_info("11", &messages, Some(("511_1", "100")))
                        .unwrap();
                    let retained = store.message_info("11", "g:90", "843").unwrap().unwrap();
                    assert_eq!(
                        serde_json::from_str::<serde_json::Value>(retained.as_json()).unwrap(),
                        case["content"]
                    );
                    app.model.conversations = store.conversations("11", 200).unwrap();
                    app.model.selected = Some("g:90".into());
                    app.model.messages = store.page("11", "g:90", None, 50).unwrap();
                    app.drafts.select("g:90").unwrap();
                    drop(store);
                    app.live
                        .as_mut()
                        .unwrap()
                        .ensure_worker(app._profile.clone());
                }
                if let Some(mode) = media_mode.as_deref() {
                    fixture_media(&mut app, &cc.egui_ctx, mode);
                }
                fixture_theme(&mut app, &cc.egui_ctx);
                Ok(Box::new(app))
            }),
        )
        .unwrap();
        if let Some(path) = cleanup.lock().unwrap().take() {
            let _ = std::fs::remove_dir_all(path);
        };
    }

    fn fixture_media(app: &mut NativeApp, ctx: &egui::Context, mode: &str) {
        use crate::domain::{MediaDescriptor, MediaKind, MediaMessageContent, Message};
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messaging/media.json"))
                .unwrap();
        let content = |name: &str| {
            cases["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["name"] == name)
                .unwrap()["content"]
                .clone()
        };
        let sources = match mode {
            "webcontent" => vec![(MediaKind::WebContent, content("webcontent_object"), false)],
            "opaque" => vec![(
                MediaKind::WebContent,
                serde_json::json!({"params":{"item":{"bundle_data":"{\"remote\":true}"}}}),
                false,
            )],
            "failure" => vec![(MediaKind::Photo, content("photo_object"), true)],
            "mixed" => vec![
                (MediaKind::Photo, content("photo_object"), false),
                (MediaKind::WebContent, content("webcontent_object"), false),
            ],
            _ => vec![(MediaKind::Photo, content("photo_object"), false)],
        };
        let records: Vec<_> = sources
            .into_iter()
            .enumerate()
            .map(|(i, (kind, source, failed))| {
                let content = MediaMessageContent::new(kind, &source).unwrap();
                let message = Message {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    id: format!("fixture-media-{i}"),
                    sender: "22".into(),
                    text: kind.legacy_placeholder().into(),
                    sent_at_ms: 1700000000500 + i as i64,
                    sticker: None,
                    media: Some(content.descriptor().clone()),
                };
                (message, content, failed)
            })
            .collect();
        let mut store =
            crate::store::MessageStore::open(&app._profile.paths.data_dir.join("messages.sqlite"))
                .unwrap();
        let messages: Vec<_> = records
            .iter()
            .map(|(m, content, _)| (m.clone(), None, None, Some(content.clone())))
            .collect();
        store
            .receive_batch_with_media("11", &messages, None)
            .unwrap();
        app.model.conversations = store.conversations("11", 200).unwrap();
        app.model.selected = Some("d:22".into());
        app.model.messages = store.page("11", "d:22", None, 50).unwrap();
        app.drafts.select("d:22").unwrap();
        app.compact_list = false;
        let (width, height, rgba) = std::thread::spawn(|| {
            crate::app::media::decode_fixture(include_bytes!(
                "../../tests/fixtures/media/photo.jxl"
            ))
            .unwrap()
        })
        .join()
        .unwrap();
        let state = app.live.as_mut().unwrap();
        for (message, _, failed) in records {
            let url = match message.media.as_ref().unwrap() {
                MediaDescriptor::Photo(p) => p.thumbnail_url.as_ref().or(p.original_url.as_ref()),
                MediaDescriptor::WebContent(c) => c.image_url.as_ref(),
            };
            if let Some(url) = url {
                let key = crate::app::media::MediaKey {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    message_id: message.id,
                    url: url.clone(),
                };
                if failed {
                    state.media.seed_failure(key, app.model.generation);
                } else {
                    state
                        .media
                        .seed(ctx, key, app.model.generation, width, height, rgba.clone());
                }
            }
        }
    }

    fn fixture_theme(app: &mut NativeApp, ctx: &egui::Context) {
        use crate::theme::ThemeMode;
        let mode = match std::env::var("ZALO_UI_FIXTURE_THEME").as_deref() {
            Ok("dark") => ThemeMode::Dark,
            Ok("system") => ThemeMode::System,
            Ok("custom") => ThemeMode::Custom,
            _ => ThemeMode::Light,
        };
        app.theme.set_mode(mode);
        app.fixture_system_theme = match std::env::var("ZALO_UI_FIXTURE_SYSTEM_THEME").as_deref() {
            Ok("dark") => Some(egui::Theme::Dark),
            Ok("light") => Some(egui::Theme::Light),
            _ => None,
        };
        if mode == ThemeMode::Custom {
            app.theme
                .set_override("--layer-background-navbar-normal", "#6F3FCF")
                .unwrap();
        }
        app.theme.poll(ctx);
        app.settings_open = std::env::var_os("ZALO_UI_FIXTURE_SETTINGS").is_some();
    }
}
