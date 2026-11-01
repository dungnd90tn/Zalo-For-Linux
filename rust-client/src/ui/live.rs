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
    pub worker: Option<LiveWorker>,
    #[cfg(feature = "messaging")]
    pub connection: LiveConnection,
    #[cfg(feature = "messaging")]
    pub delivery: super::delivery::DeliveryBook,
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
    pub fn new(
        profile: Arc<Profile>,
        trust: Option<PathBuf>,
        ctx: &egui::Context,
    ) -> anyhow::Result<Self> {
        let repaint = ctx.clone();
        Ok(Self {
            controller: LoginController::new(
                profile,
                trust,
                Arc::new(move || repaint.request_repaint()),
            )?,
            login: LoginView::default(),
            account: None,
            #[cfg(feature = "messaging")]
            worker: None,
            #[cfg(feature = "messaging")]
            connection: LiveConnection::Offline,
            #[cfg(feature = "messaging")]
            delivery: Default::default(),
            #[cfg(feature = "messaging")]
            review_dialog: false,
            #[cfg(feature = "messaging")]
            open_dialog: false,
            #[cfg(feature = "messaging")]
            target: String::new(),
            #[cfg(feature = "messaging")]
            group: false,
            logout_dialog: false,
        })
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Begin | Action::Resume => {
                let result = if action == Action::Resume {
                    self.controller.resume()
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
    pub(super) fn live_information(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(state) = self.live.as_mut() else {
            return false;
        };
        let Some(account) = &state.account else {
            return false;
        };
        ui.label(egui::RichText::new(&account.name).strong());
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
                        .color(super::visuals::MUTED),
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
        let mut app = Self::new(cc, profile, false)?;
        app.live = Some(LiveState::new(app._profile.clone(), trust, &cc.egui_ctx)?);
        app.model.status.clear();
        Ok(app)
    }

    pub(super) fn live_signed_in(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|state| state.account.is_some())
    }

    pub(super) fn show_login(&mut self, ctx: &egui::Context) -> bool {
        let Some(state) = self.live.as_mut() else {
            return false;
        };
        if state.account.is_some() {
            return false;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::WHITE))
            .show(ctx, |ui| {
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
                        let repaint = ctx.clone();
                        match LiveWorker::start(
                            self._profile.clone(),
                            session,
                            Arc::new(move || repaint.request_repaint()),
                        ) {
                            Ok(worker) => {
                                state.worker = Some(worker);
                                state.connection = LiveConnection::Connecting;
                            }
                            Err(_) => {
                                state.connection = LiveConnection::Offline;
                                self.model.status = "Đã đăng nhập nhưng chưa khởi tạo được gửi nhận. Hãy đăng xuất và thử lại.".into();
                            }
                        }
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
                        state.connection = LiveConnection::Offline;
                        state.delivery = Default::default();
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
        while remaining > 0 && Instant::now() < deadline {
            let event = state
                .worker
                .as_mut()
                .and_then(|worker| worker.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            remaining -= 1;
            let Some(account) = &state.account else {
                continue;
            };
            if event.account_id() != account.id {
                continue;
            }
            if let LiveEvent::Connection {
                state: connection, ..
            } = &event
            {
                state.connection = *connection;
            }
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
                LiveEvent::Fault { .. } => state.connection = LiveConnection::Offline,
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
            self.drafts.apply_live(&event, &account.id);
            self.model.apply_live(event, &account.id, self.latest_page);
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
        if state
            .worker
            .as_ref()
            .is_some_and(|worker| worker.events.is_closed() && worker.events.is_empty())
        {
            if let Some(account) = &state.account {
                let event = LiveEvent::Fault {
                    account_id: account.id.clone(),
                    error: "Tiến trình gửi nhận đã dừng. Bản nháp được giữ lại; hãy đăng nhập lại."
                        .into(),
                };
                self.drafts.apply_live(&event, &account.id);
                self.model.apply_live(event, &account.id, self.latest_page);
            }
            state.connection = LiveConnection::Offline;
            state.worker = None;
        }
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
                            state.connection = LiveConnection::Offline;
                            state.delivery = Default::default();
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
        if state.open_dialog {
            show_target_dialog(
                state,
                &mut self.model,
                &mut self.drafts,
                &mut self.latest_page,
                ctx,
            );
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
) {
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
                        open = false;
                    }
                    Err(error) => model.status = error.to_string(),
                }
            }
        }
    }
    state.open_dialog = open;
}
