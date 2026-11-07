//! Original native desktop UI. File, database, and network work stays off the render thread.
mod account;
#[cfg(all(test, feature = "messaging"))]
mod account_tests;
mod attachments;
#[cfg(feature = "messaging")]
mod avatars;
#[cfg(test)]
mod conversation_tests;
#[cfg(feature = "messaging")]
mod delivery;
mod dialog;
#[cfg(feature = "messaging")]
mod directory;
#[cfg(feature = "messaging")]
mod hydration;
mod information;
#[cfg(all(test, feature = "messaging"))]
mod layout_tests;
#[cfg(feature = "live-login")]
mod live;
#[cfg(feature = "live-login")]
mod login;
#[cfg(feature = "messaging")]
mod media;
#[cfg(all(test, feature = "messaging"))]
mod modal_tests;
#[cfg(feature = "messaging")]
mod privacy;
#[cfg(all(test, feature = "messaging"))]
mod privacy_native_fixture;
#[cfg(all(test, feature = "messaging"))]
mod privacy_tests;
mod senders;
mod settings;
#[cfg(test)]
mod shot_fixture;
#[cfg(feature = "messaging")]
mod stickers;
mod visuals;

use crate::app::{Command, DraftBook, Event, Model, Worker, EVENTS_PER_FRAME};
use crate::domain::{fold_search, Conversation, Message};
use crate::profile::Profile;
use eframe::egui::{self, pos2, vec2, Align, Align2, FontId, Layout, RichText, Sense, Stroke};
use std::sync::Arc;
use visuals::{divider, icon, icon_button, Icon};

// Original source dimensions are logical points, independent of window size and native DPI.
const RAIL_WIDTH: f32 = 68.0;
const CONVERSATION_LIST_WIDTH: f32 = 344.0;
const CONVERSATION_LIST_MIN_WIDTH: f32 = 280.0;
const CONVERSATION_LIST_MAX_WIDTH: f32 = 420.0;
const CHAT_HEADER_HEIGHT: f32 = 76.0;
const CONVERSATION_ROW_HEIGHT: f32 = 72.0;
/// Longest side of a sticker, in logical points.
const STICKER_SIZE: f32 = 130.0;
const CONVERSATION_INFO_WIDTH: f32 = 344.0;
const MIN_CHAT_WIDTH_WITH_INFO: f32 = 360.0;
const COMPACT_BREAKPOINT: f32 = 940.0;

#[derive(Clone)]
struct ModalFocus {
    id: egui::Id,
    account: Option<String>,
    thread: Option<String>,
    generation: u64,
    auth_generation: Option<u64>,
}

pub struct NativeApp {
    worker: Option<Worker>,
    model: Model,
    drafts: DraftBook,
    demo: bool,
    search: String,
    search_folded: String,
    compact_list: bool,
    details: bool,
    conversation_details: Option<(String, String)>,
    modal_blocked: bool,
    modal_focus: Option<ModalFocus>,
    settings_open: bool,
    account_popup: Option<account::Popup>,
    account_trigger_focus: Option<ModalFocus>,
    theme: crate::theme::ThemeController,
    settings: settings::Settings,
    #[cfg(test)]
    fixture_system_theme: Option<egui::Theme>,
    #[cfg(all(test, feature = "messaging"))]
    fixture_connection: Option<crate::app::live::LiveConnection>,
    focus_composer: bool,
    latest_page: bool,
    ime_composing: bool,
    info_tab: information::InfoTab,
    attachments: attachments::AttachmentPicker,
    #[cfg(feature = "live-login")]
    live: Option<live::LiveState>,
    // The worker shuts down before the profile lock is released.
    _profile: Arc<Profile>,
}

impl NativeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        profile: Profile,
        demo: bool,
    ) -> anyhow::Result<Self> {
        visuals::install(&cc.egui_ctx);
        let repaint = cc.egui_ctx.clone();
        let mut theme = crate::theme::ThemeController::new(
            profile.paths.config_dir.clone(),
            Arc::new(move || repaint.request_repaint()),
        )?;
        theme.poll(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let worker = if demo {
            Some(Worker::demo(
                profile.paths.data_dir.join("messages.sqlite3"),
                std::sync::Arc::new(move || ctx.request_repaint()),
            )?)
        } else {
            None
        };
        let mut model = Model::default();
        if !demo {
            model.status = crate::protocol::AUTH_LIMITATION.into();
        }
        Ok(Self {
            worker,
            model,
            drafts: DraftBook::default(),
            demo,
            search: String::new(),
            search_folded: String::new(),
            compact_list: false,
            details: false,
            conversation_details: None,
            modal_blocked: false,
            modal_focus: None,
            settings_open: false,
            account_popup: None,
            account_trigger_focus: None,
            theme,
            settings: settings::Settings::default(),
            #[cfg(test)]
            fixture_system_theme: None,
            #[cfg(all(test, feature = "messaging"))]
            fixture_connection: None,
            focus_composer: false,
            latest_page: true,
            ime_composing: false,
            info_tab: Default::default(),
            attachments: Default::default(),
            #[cfg(feature = "live-login")]
            live: None,
            _profile: Arc::new(profile),
        })
    }

    fn is_signed_in(&self) -> bool {
        #[cfg(feature = "live-login")]
        {
            self.live_signed_in()
        }
        #[cfg(not(feature = "live-login"))]
        {
            false
        }
    }

    fn modal_open(&self) -> bool {
        if self.settings_open || self.details || self.account_popup.is_some() {
            return true;
        }
        #[cfg(feature = "live-login")]
        if let Some(state) = &self.live {
            #[cfg(feature = "messaging")]
            if state.privacy.prompt.is_some() {
                return true;
            }
            if state.logout_dialog {
                return true;
            }
            #[cfg(feature = "messaging")]
            if state.open_dialog || state.review_dialog {
                return true;
            }
        }
        false
    }

    fn latch_modal_input(&mut self, ctx: &egui::Context) {
        if self.modal_open() && !self.modal_blocked {
            self.modal_focus = self
                .account_popup
                .as_mut()
                .and_then(|popup| popup.focus.take())
                .or_else(|| self.modal_focus_snapshot(ctx));
            if let Some(focus) = &self.modal_focus {
                ctx.memory_mut(|memory| memory.surrender_focus(focus.id));
            }
            self.modal_blocked = true;
        }
    }

    fn update_modal_input(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerButton { pressed: true, .. }
                        | egui::Event::Key { pressed: true, .. }
                )
            })
        }) {
            self.account_trigger_focus = None;
        }
        if !self.modal_open() && self.modal_blocked {
            if let Some(focus) = self.modal_focus.take() {
                if focus.account.as_deref() == self.account_id()
                    && focus.thread == self.model.selected
                    && focus.generation == self.model.generation
                    && focus.auth_generation == self.auth_generation()
                {
                    ctx.memory_mut(|memory| memory.request_focus(focus.id));
                }
            }
            self.modal_blocked = false;
        }
        self.latch_modal_input(ctx);
    }
    fn account_id(&self) -> Option<&str> {
        #[cfg(feature = "live-login")]
        {
            self.live
                .as_ref()
                .and_then(|state| state.account.as_ref())
                .map(|account| account.id.as_str())
        }
        #[cfg(not(feature = "live-login"))]
        {
            None
        }
    }
    fn account_label(&self) -> &str {
        #[cfg(feature = "live-login")]
        if let Some(account) = self.live.as_ref().and_then(|state| state.account.as_ref()) {
            return if account.name.trim().is_empty() {
                "Tài khoản Zalo"
            } else {
                &account.name
            };
        }
        if self.demo {
            "Demo"
        } else {
            "Zalo"
        }
    }
    fn profile_avatar(&mut self, ui: &egui::Ui, rect: egui::Rect, title: &str, identity: &str) {
        #[cfg(feature = "messaging")]
        let texture = self.live.as_mut().and_then(|state| {
            (state.account.is_some()
                && state.privacy.ready()
                && state.privacy.visible(identity)
                && ui.is_rect_visible(rect))
            .then(|| state.avatars.visible(ui.ctx(), identity))
            .flatten()
        });
        #[cfg(not(feature = "messaging"))]
        let texture: Option<egui::TextureHandle> = {
            let _ = identity;
            None
        };
        visuals::avatar_photo(ui, rect, title, texture.as_ref());
    }
    fn connection_label(&self) -> &str {
        #[cfg(feature = "live-login")]
        if let Some(state) = &self.live {
            return state.connection_label();
        }
        "Chưa đăng nhập"
    }
    fn live_can_send(&self) -> bool {
        #[cfg(feature = "live-login")]
        {
            !self.model.pending
                && self
                    .live
                    .as_ref()
                    .is_some_and(|state| state.can_send(self.model.selected.as_deref()))
        }
        #[cfg(not(feature = "live-login"))]
        {
            false
        }
    }

    fn load_latest(&mut self, thread: String) -> bool {
        #[cfg(feature = "messaging")]
        {
            if !self.privacy_visible(&thread) {
                return false;
            }
            if self.model.selected.as_deref() != Some(&thread) {
                self.relock_hidden();
            }
        }
        if !self.drafts.can_select(&thread) {
            self.model.status = "Đã đạt giới hạn bản nháp hội thoại đang mở.".into();
            return false;
        }
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            if state.account.is_some()
                && (!state.ensure_worker(self._profile.clone()) || !state.privacy.visible(&thread))
            {
                self.model.status =
                    "Chưa mở được hội thoại. Hãy chọn Kết nối lại; bản nháp được giữ nguyên."
                        .into();
                return false;
            }
        }
        #[cfg(feature = "messaging")]
        if let Some(worker) = self.live.as_ref().and_then(|state| state.worker.as_ref()) {
            match self
                .model
                .select_live_if_accepted(thread.clone(), |command| worker.submit(command))
            {
                Ok(()) => {
                    let _ = self.drafts.select(&thread);
                    self.latest_page = true;
                    if let Some(state) = &mut self.live {
                        state.delivery.edited(&thread, &self.drafts.active.text);
                        if state.delivery.review(&thread).is_some() {
                            state.review_dialog = true;
                        }
                    }
                    return true;
                }
                Err(error) => {
                    self.model.status = error.to_string();
                    return false;
                }
            }
        }
        if let Some(worker) = &self.worker {
            match self
                .model
                .select_if_accepted(thread.clone(), |command| worker.submit(command))
            {
                Ok(()) => {
                    let _ = self.drafts.select(&thread);
                    self.latest_page = true;
                    return true;
                }
                Err(error) => {
                    self.model.status = error.to_string();
                }
            }
        }
        false
    }

    fn older_page(&mut self) {
        #[cfg(feature = "messaging")]
        if self
            .model
            .selected
            .as_deref()
            .is_some_and(|thread| !self.privacy_visible(thread))
        {
            return;
        }
        if self.model.pending {
            return;
        }
        #[cfg(feature = "messaging")]
        if let (Some(worker), Some(thread), Some(cursor)) = (
            self.live.as_ref().and_then(|state| state.worker.as_ref()),
            self.model.selected.clone(),
            self.model.messages.first().map(Message::cursor),
        ) {
            let generation = self.model.generation.wrapping_add(1);
            match worker.submit(crate::app::live::LiveCommand::Load {
                generation,
                thread,
                before: Some(cursor),
            }) {
                Ok(()) => {
                    self.model.generation = generation;
                    self.model.pending = true;
                    self.latest_page = false;
                }
                Err(error) => self.model.status = error.to_string(),
            }
            return;
        }
        if let (Some(worker), Some(thread), Some(cursor)) = (
            self.worker.as_ref(),
            self.model.selected.clone(),
            self.model.messages.first().map(Message::cursor),
        ) {
            let generation = self.model.generation.wrapping_add(1);
            match worker.submit(Command::Load {
                generation,
                thread,
                before: Some(cursor),
            }) {
                Ok(()) => {
                    self.model.generation = generation;
                    self.model.pending = true;
                    self.latest_page = false;
                }
                Err(error) => {
                    self.model.status = error.to_string();
                }
            }
        }
    }

    fn send_local(&mut self) {
        if self.modal_blocked || self.modal_open() {
            return;
        }
        #[cfg(feature = "messaging")]
        if !self.privacy_ready()
            || self
                .model
                .selected
                .as_deref()
                .is_some_and(|thread| !self.privacy_visible(thread))
        {
            return;
        }
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            if !state.can_send(self.model.selected.as_deref()) || self.model.pending {
                return;
            }
            if let (Some(worker), Some(thread)) = (&state.worker, self.model.selected.clone()) {
                if self.drafts.active.in_flight() || self.drafts.active.text.trim().is_empty() {
                    return;
                }
                let mut submitted = None;
                let result = self.drafts.active.send_live(
                    self.model.generation,
                    thread.clone(),
                    |command| {
                        let request = match &command {
                            crate::app::live::LiveCommand::Send { request_id, .. } => {
                                Some(*request_id)
                            }
                            _ => None,
                        };
                        let result = worker.submit(command);
                        if result.is_ok() {
                            submitted = request;
                        }
                        result
                    },
                );
                match result {
                    Ok(()) => {
                        if let Some(request) = submitted {
                            state
                                .delivery
                                .submitted(&thread, request, &self.drafts.active.text);
                        }
                    }
                    Err(error) => self.model.status = error.to_string(),
                }
                self.focus_composer = true;
            }
            return;
        }
        let (Some(worker), Some(thread)) = (self.worker.as_ref(), self.model.selected.clone())
        else {
            return;
        };
        if self.drafts.active.in_flight()
            || (self.drafts.active.text.trim().is_empty()
                && self.drafts.active.attachment().is_none())
        {
            return;
        }
        if let Err(error) = self
            .drafts
            .active
            .send(self.model.generation, thread, |command| {
                worker.submit(command)
            })
        {
            self.model.status = error.to_string();
        }
        self.focus_composer = true;
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        for _ in 0..EVENTS_PER_FRAME {
            let event = self
                .worker
                .as_mut()
                .and_then(|worker| worker.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            let stored = matches!(&event, Event::DemoStored { generation, thread, .. }
                if *generation == self.model.generation && self.model.selected.as_deref() == Some(thread));
            self.drafts.apply(&event);
            self.model.apply(event);
            if stored {
                if let Some(thread) = self.model.selected.clone() {
                    self.load_latest(thread);
                }
            }
            if self.model.selected.is_none() {
                if let Some(thread) = self.model.conversations.first() {
                    self.load_latest(thread.id.clone());
                }
            }
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.events.is_empty())
        {
            ctx.request_repaint();
        }
    }

    fn navigation(&mut self, ctx: &egui::Context, compact: bool) {
        let colors = crate::theme::palette(ctx);
        egui::SidePanel::left("navigation")
            .exact_width(RAIL_WIDTH)
            .resizable(false)
            .frame(egui::Frame::none().fill(colors.nav_background))
            .show(ctx, |ui| {
                if self.modal_blocked || self.modal_open() {
                    ui.disable();
                }
                visuals::background(
                    ui,
                    ui.max_rect(),
                    "--layer-background-navbar-normal",
                    colors.nav_background,
                );
                ui.add_space(21.0);
                self.account_avatar(ui);
                ui.add_space(27.0);
                #[cfg(feature = "messaging")]
                let directory = self.directory_visible();
                #[cfg(not(feature = "messaging"))]
                let directory = false;
                #[cfg(feature = "messaging")]
                let contacts_enabled = self.is_signed_in();
                #[cfg(not(feature = "messaging"))]
                let contacts_enabled = false;
                for (kind, title, selected, enabled) in [
                    (Icon::Chat, "Tin nhắn", !directory, true),
                    (
                        Icon::Contacts,
                        if contacts_enabled {
                            "Danh bạ"
                        } else {
                            "Danh bạ · cần đăng nhập với gửi nhận"
                        },
                        directory,
                        contacts_enabled,
                    ),
                    (
                        Icon::Cloud,
                        "Lưu trữ đám mây · chưa hỗ trợ trong bản xem trước",
                        false,
                        false,
                    ),
                ] {
                    let (rect, response) = ui
                        .add_enabled_ui(enabled, |ui| {
                            ui.allocate_exact_size(vec2(RAIL_WIDTH, 56.0), Sense::click())
                        })
                        .inner;
                    visuals::rail_button(ui, rect, &response, selected);
                    icon(
                        ui,
                        egui::Rect::from_center_size(rect.center(), vec2(24.0, 24.0)),
                        kind,
                        if !enabled {
                            colors.nav_text_disabled
                        } else if selected {
                            colors.on_color
                        } else {
                            colors.nav_text
                        },
                    );
                    if enabled && response.clicked() {
                        match kind {
                            Icon::Contacts => {
                                #[cfg(feature = "messaging")]
                                self.activate_directory(ctx);
                            }
                            Icon::Chat => {
                                #[cfg(feature = "messaging")]
                                if let Some(state) = &mut self.live {
                                    state.directory.visible = false;
                                }
                                self.compact_list |= compact;
                                #[cfg(feature = "messaging")]
                                if compact {
                                    self.relock_hidden();
                                }
                            }
                            _ => {}
                        }
                    }
                    response.on_hover_text(title).on_disabled_hover_text(title);
                }
                let bottom = ui.max_rect().bottom();
                let y = (bottom - 160.0).max(ui.cursor().top() + 12.0);
                positioned_ui(
                    ui,
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                        pos2(ui.max_rect().left(), y),
                        vec2(RAIL_WIDTH, 160.0),
                    )),
                    |ui| {
                        // No original glyph exists for a light/dark switch, so the control
                        // names the mode it selects instead of drawing a substitute symbol.
                        let to_dark = !colors.dark;
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(RAIL_WIDTH, 52.0), Sense::click());
                        visuals::rail_button(ui, rect, &response, false);
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            if to_dark { "Tối" } else { "Sáng" },
                            FontId::new(
                                visuals::TEXT_SMALL,
                                egui::FontFamily::Name("Zalo Semibold".into()),
                            ),
                            colors.nav_text,
                        );
                        let theme_label = if to_dark {
                            "Chuyển sang giao diện Tối"
                        } else {
                            "Chuyển sang giao diện Sáng"
                        };
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                theme_label,
                            )
                        });
                        if response.clicked() {
                            self.theme.set_mode(if to_dark {
                                crate::theme::ThemeMode::Dark
                            } else {
                                crate::theme::ThemeMode::Light
                            });
                        }
                        response.on_hover_text(format!(
                            "{theme_label} · Hệ thống và Tùy chỉnh chọn trong Cài đặt"
                        ));
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(RAIL_WIDTH, 52.0), Sense::click());
                        visuals::rail_button(ui, rect, &response, self.details);
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Info,
                            if self.details {
                                colors.on_color
                            } else {
                                colors.nav_text
                            },
                        );
                        if response.clicked() {
                            self.details = !self.details;
                        }
                        response.on_hover_text("Thông tin bản xem trước");
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(RAIL_WIDTH, 52.0), Sense::click());
                        let settings_selected = self.settings_open
                            || self.account_popup_is(account::PopupKind::Settings);
                        visuals::rail_button(ui, rect, &response, settings_selected);
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Settings,
                            if settings_selected {
                                colors.on_color
                            } else {
                                colors.nav_text
                            },
                        );
                        self.prepare_account_popup_trigger(
                            account::PopupKind::Settings,
                            rect,
                            &response,
                        );
                        if response.clicked() {
                            self.toggle_account_popup(account::PopupKind::Settings, rect);
                        }
                        response.on_hover_text("Cài đặt và tài khoản");
                    },
                );
            });
    }

    fn conversations(&mut self, ui: &mut egui::Ui, compact: bool) {
        if self.modal_blocked || self.modal_open() {
            ui.disable();
        }
        #[cfg(feature = "messaging")]
        if !self.privacy_ready() {
            self.show_privacy_neutral(ui);
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        let width = ui.available_width();
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(16.0, 16.0))
            .show(ui, |ui| {
                #[cfg(feature = "messaging")]
                let open_live = self
                    .live
                    .as_ref()
                    .is_some_and(|state| state.account.is_some());
                #[cfg(not(feature = "messaging"))]
                let open_live = false;
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(
                        RichText::new("Tin nhắn")
                            .size(visuals::TEXT_HEADING)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(
                            ui,
                            Icon::Plus,
                            if open_live {
                                "Mở hội thoại Zalo"
                            } else {
                                "Tạo hội thoại · chưa hỗ trợ trong bản xem trước"
                            },
                            open_live,
                        )
                        .clicked()
                        {
                            #[cfg(feature = "messaging")]
                            if let Some(state) = &mut self.live {
                                state.ensure_worker(self._profile.clone());
                                state.open_dialog = true;
                            }
                        }
                    });
                });
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(colors.canvas)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(16.0, 26.0), Sense::hover());
                            icon(ui, rect, Icon::Search, colors.text_secondary);
                            let response = ui.add_sized(
                                [ui.available_width(), 26.0],
                                egui::TextEdit::singleline(&mut self.search)
                                    .id_source("conversation_search")
                                    .hint_text("Tìm kiếm")
                                    .char_limit(256)
                                    .frame(false)
                                    .font(egui::TextStyle::Body),
                            );
                            if response.changed() {
                                self.search_folded = fold_search(&self.search);
                            }
                        });
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    visuals::chip(ui, "Tất cả", true, true, "Tất cả hội thoại");
                    visuals::chip(
                        ui,
                        "Chưa đọc",
                        false,
                        false,
                        "Chưa hỗ trợ: bản xem trước chưa lưu trạng thái chưa đọc",
                    );
                    let count = self
                        .model
                        .conversations
                        .iter()
                        .filter(|thread| {
                            #[cfg(feature = "messaging")]
                            {
                                self.privacy_list_visible(&thread.id)
                            }
                            #[cfg(not(feature = "messaging"))]
                            {
                                let _ = thread;
                                true
                            }
                        })
                        .count();
                    let label =
                        format!("{count} hội thoại{}", if self.demo { " demo" } else { "" });
                    let (count_rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                    ui.painter().text(
                        count_rect.right_center(),
                        Align2::RIGHT_CENTER,
                        &label,
                        FontId::proportional(visuals::TEXT_SMALL),
                        colors.text_secondary,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &label)
                    });
                });
            });
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation_list")
            .max_height(
                (ui.available_height() - if self.is_signed_in() { 0.0 } else { 64.0 }).max(60.0),
            )
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut matches = 0;
                for thread in &self.model.conversations {
                    #[cfg(feature = "messaging")]
                    if !self.privacy_list_visible(&thread.id) {
                        continue;
                    }
                    let title = display_title(thread);
                    if !self.search_folded.is_empty()
                        && !fold_search(&title).contains(&self.search_folded)
                    {
                        continue;
                    }
                    matches += 1;
                    #[cfg(feature = "messaging")]
                    let texture = self.live.as_mut().and_then(|state| {
                        (state.account.is_some()
                            && state.privacy.ready()
                            && ui.is_rect_visible(egui::Rect::from_min_size(
                                ui.cursor().min,
                                vec2(width, CONVERSATION_ROW_HEIGHT),
                            )))
                        .then(|| state.avatars.visible(ui.ctx(), &thread.id))
                        .flatten()
                    });
                    #[cfg(not(feature = "messaging"))]
                    let texture: Option<egui::TextureHandle> = None;
                    if conversation_row(
                        ui,
                        width,
                        thread,
                        &title,
                        self.model.selected.as_deref() == Some(&thread.id),
                        self.account_id(),
                        texture.as_ref(),
                    )
                    .clicked()
                    {
                        selection = Some(thread.id.clone());
                    }
                }
                #[cfg(feature = "messaging")]
                let hidden_matches = self.hidden_search_rows(ui);
                #[cfg(not(feature = "messaging"))]
                let hidden_matches = false;
                if matches == 0 && !hidden_matches {
                    ui.add_space(42.0);
                    ui.vertical_centered(|ui| {
                        let searching = !self.search_folded.is_empty();
                        ui.label(
                            RichText::new(if searching {
                                "Không tìm thấy hội thoại"
                            } else {
                                "Chưa có cuộc trò chuyện"
                            })
                            .color(colors.text_secondary),
                        );
                        ui.add_space(5.0);
                        ui.label(
                            RichText::new(if searching {
                                "Thử tìm bằng tên khác hoặc bỏ dấu tiếng Việt."
                            } else if self.is_signed_in() {
                                "Mở Danh bạ để bắt đầu trò chuyện."
                            } else {
                                "Các cuộc trò chuyện sẽ xuất hiện tại đây."
                            })
                            .size(12.0)
                            .color(colors.text_secondary),
                        );
                    });
                }
            });
        if let Some(thread) = selection {
            if self.load_latest(thread) && compact {
                self.compact_list = false;
            }
        }
        if self.is_signed_in() {
            return;
        }
        let rect = ui.max_rect();
        let footer =
            egui::Rect::from_min_max(pos2(rect.left(), rect.bottom() - 55.0), rect.right_bottom());
        visuals::background(ui, footer, "--layer-background", colors.surface);
        divider(ui, footer.left_top(), footer.right_top());
        icon(
            ui,
            egui::Rect::from_min_size(footer.min + vec2(16.0, 17.0), vec2(18.0, 18.0)),
            Icon::Shield,
            colors.text_secondary,
        );
        ui.painter().text(
            footer.min + vec2(43.0, 20.0),
            Align2::LEFT_TOP,
            if self.demo {
                "Dữ liệu demo · chỉ lưu trên máy"
            } else {
                "Bản xem trước · chưa đăng nhập"
            },
            FontId::proportional(visuals::TEXT_SMALL),
            colors.text_secondary,
        );
    }

    fn chat_header(&mut self, ui: &mut egui::Ui, compact: bool) {
        let colors = crate::theme::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), CHAT_HEADER_HEIGHT),
            Sense::hover(),
        );
        visuals::background(ui, rect, "--layer-background", colors.surface);
        divider(ui, rect.left_bottom(), rect.right_bottom());
        let actions_width = if rect.width() < 490.0 { 48.0 } else { 152.0 };
        let mut left = rect.left() + 24.0;
        if compact {
            let back = egui::Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.top() + 16.0),
                vec2(28.0, 44.0),
            );
            let response = ui.interact(back, ui.id().with("back_to_threads"), Sense::click());
            if response.hovered() || response.is_pointer_button_down_on() {
                ui.painter().rect_filled(
                    back,
                    4.0,
                    if response.is_pointer_button_down_on() {
                        colors.pressed
                    } else {
                        colors.hover
                    },
                );
            }
            icon(ui, back.shrink(4.0), Icon::Back, colors.text_secondary);
            visuals::focus_outline(ui, &response, back, 4.0);
            if response.clicked() {
                #[cfg(feature = "messaging")]
                self.relock_hidden();
                self.compact_list = true;
                self.conversation_details = None;
            }
            response.on_hover_text("Danh sách trò chuyện");
            left += 28.0;
        }
        let title = self
            .model
            .selected
            .as_deref()
            .and_then(|id| {
                #[cfg(feature = "messaging")]
                {
                    self.visible_conversation(id)
                }
                #[cfg(not(feature = "messaging"))]
                {
                    self.model
                        .conversations
                        .iter()
                        .find(|thread| thread.id == id)
                }
            })
            .map(display_title)
            .unwrap_or_else(|| "Zalo Native".into());
        let photo = egui::Rect::from_min_size(pos2(left, rect.top() + 16.0), vec2(44.0, 44.0));
        let identity = self.model.selected.clone().unwrap_or_default();
        self.profile_avatar(ui, photo, &title, &identity);
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                pos2(left + 58.0, rect.top() + 15.0),
                pos2(rect.right() - actions_width - 10.0, rect.bottom() - 10.0),
            )),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                ui.add(
                    egui::Label::new(
                        RichText::new(&title)
                            .size(visuals::TEXT_HEADING)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    )
                    .truncate(),
                )
                .on_hover_text(&title);
                ui.add_space(1.0);
                ui.label(
                    RichText::new(if self.demo {
                        "Hội thoại minh họa · ngoại tuyến"
                    } else if self
                        .model
                        .selected
                        .as_deref()
                        .is_some_and(|id| id.starts_with("g:"))
                    {
                        "Nhóm trò chuyện"
                    } else {
                        "Trò chuyện riêng"
                    })
                    .color(colors.text_secondary)
                    .size(visuals::TEXT_SMALL),
                );
            },
        );
        positioned_ui(
            ui,
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    pos2(rect.right() - actions_width - 12.0, rect.top() + 16.0),
                    vec2(actions_width, 44.0),
                ))
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                if icon_button(
                    ui,
                    Icon::Info,
                    "Thông tin hội thoại",
                    self.model.selected.is_some(),
                )
                .clicked()
                {
                    if let Some(thread) = self.model.selected.clone() {
                        let scope = (
                            self.account_id().unwrap_or(crate::app::DEMO_ACCOUNT).into(),
                            thread,
                        );
                        self.conversation_details =
                            if self.conversation_details.as_ref() == Some(&scope) {
                                None
                            } else {
                                Some(scope)
                            };
                    }
                }
                if actions_width > 50.0 {
                    icon_button(
                        ui,
                        Icon::Video,
                        "Gọi video · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                    icon_button(
                        ui,
                        Icon::Phone,
                        "Gọi thoại · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                }
            },
        );
    }

    fn composer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, height: f32) {
        let colors = crate::theme::palette(ctx);
        #[cfg(feature = "messaging")]
        let privacy_enabled = self.privacy_ready()
            && self
                .model
                .selected
                .as_deref()
                .is_none_or(|thread| self.privacy_visible(thread));
        #[cfg(not(feature = "messaging"))]
        let privacy_enabled = true;
        let enabled = privacy_enabled
            && ui.is_enabled()
            && !self.modal_blocked
            && !self.modal_open()
            && (self.demo || self.is_signed_in())
            && self.model.selected.is_some();
        let (outer, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        // The composer is a rounded card floating on the chat canvas: toolbar on top, editor
        // and send control below.
        let rect = egui::Rect::from_min_max(
            pos2(outer.left() + 24.0, outer.top() + 8.0),
            pos2(outer.right() - 24.0, outer.bottom() - 16.0),
        );
        ui.painter().rect_filled(rect, 16.0, colors.surface);
        ui.painter()
            .rect_stroke(rect, 16.0, Stroke::new(1.0_f32, colors.border_strong));
        // Files can only be attached offline, in the demo: sending one to Zalo needs the upload
        // protocol, which has not been verified against the real server (PROTOCOL_FILES.md).
        let attach_here = enabled && self.demo;
        self.poll_attachments(
            ctx,
            attach_here,
            "Gửi tệp tới Zalo chưa được hỗ trợ: giao thức tải lên chưa được kiểm chứng với máy chủ thật.",
        );
        let tray_height = self.attachment_tray_height();
        if tray_height > 0.0 {
            self.attachment_tray(ui, rect);
        }
        if attach_here && ctx.input(|input| !input.raw.hovered_files.is_empty()) {
            ui.painter()
                .rect_stroke(rect.shrink(1.0), 16.0, Stroke::new(2.0_f32, colors.primary));
        }
        let mut send = false;
        let ime_active = self.ime_composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
            });
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(
                egui::Rect::from_min_max(pos2(rect.left(), rect.top() + tray_height), rect.max)
                    .shrink2(vec2(8.0, 4.0)),
            ),
            |ui| {
                ui.horizontal(|ui| {
                    icon_button(
                        ui,
                        Icon::Smile,
                        "Biểu cảm · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                    icon_button(
                        ui,
                        Icon::Image,
                        "Gửi ảnh · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                    if icon_button(
                        ui,
                        Icon::Paperclip,
                        if self.demo {
                            "Đính kèm tệp · chỉ lưu cục bộ trong bản demo"
                        } else {
                            "Đính kèm · chưa hỗ trợ: giao thức tải tệp lên Zalo chưa được kiểm chứng"
                        },
                        attach_here && !self.attachments.busy(),
                    )
                    .clicked()
                    {
                        let thread = self.model.selected.clone();
                        self.attachments.open_dialog(ctx, thread.as_deref());
                    }
                    if rect.width() >= 480.0 {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(if self.demo {
                                    "CHỈ LƯU CỤC BỘ · Enter để lưu · Ctrl+Enter xuống dòng"
                                } else {
                                    "Enter để gửi · Ctrl+Enter để xuống dòng"
                                })
                                .size(visuals::TEXT_SMALL)
                                .color(colors.text_secondary),
                            );
                        });
                    }
                });
                let send_rect = egui::Rect::from_min_size(
                    pos2(rect.right() - 12.0 - 72.0, rect.bottom() - 10.0 - 44.0),
                    vec2(72.0, 44.0),
                );
                let editor_rect = egui::Rect::from_min_max(
                    pos2(rect.left() + 16.0, rect.top() + 52.0 + tray_height),
                    pos2(send_rect.left() - 12.0, rect.bottom() - 10.0),
                );
                positioned_ui(ui, egui::UiBuilder::new().max_rect(editor_rect), |ui| {
                    // Cursor, undo, and provisional IME selection belong to this draft.
                    let composer_id = ui.make_persistent_id((
                        "message_composer",
                        self.account_id(),
                        self.model.selected.as_deref(),
                    ));
                    // Enter sends; Ctrl+Enter and Shift+Enter insert a line break. Handle both before
                    // TextEdit sees them: a plain Enter is swallowed here (it never becomes a
                    // newline) and a modified Enter is handed on as the plain key TextEdit turns
                    // into "\n". Nothing sends while an input method is composing, in the same
                    // frame as a commit, or for a held (repeating) key.
                    if enabled && ctx.memory(|memory| memory.has_focus(composer_id)) {
                        ctx.input_mut(|input| {
                            input.events.retain_mut(|event| {
                                let egui::Event::Key {
                                    key: egui::Key::Enter,
                                    pressed: true,
                                    repeat,
                                    modifiers,
                                    ..
                                } = event
                                else {
                                    return true;
                                };
                                if modifiers.command || modifiers.ctrl || modifiers.shift {
                                    // Line break: no newline while composing (the IME owns Enter).
                                    if ime_active {
                                        return false;
                                    }
                                    *modifiers = egui::Modifiers::NONE;
                                    return true;
                                }
                                if modifiers.is_none() {
                                    if !*repeat && !ime_active {
                                        send = true;
                                    }
                                    return false;
                                }
                                true
                            });
                        });
                    }
                    ui.set_clip_rect(ui.clip_rect().intersect(editor_rect));
                    let response = egui::ScrollArea::vertical()
                        .id_salt("composer_scroll")
                        .auto_shrink([false, false])
                        .min_scrolled_height(editor_rect.height())
                        .max_height(editor_rect.height())
                        .show(ui, |ui| {
                            let size = vec2(ui.available_width(), editor_rect.height());
                            ui.add_enabled(
                                enabled,
                                egui::TextEdit::multiline(&mut self.drafts.active.text)
                                    .id(composer_id)
                                    .hint_text("Nhập tin nhắn…")
                                    .desired_rows(1)
                                    .min_size(size)
                                    .desired_width(size.x)
                                    .frame(false)
                                    .char_limit(crate::domain::MAX_TEXT_BYTES)
                                    .margin(egui::Margin::ZERO),
                            )
                        })
                        .inner;
                    let response =
                        response.on_hover_text("Enter để gửi · Ctrl+Enter để xuống dòng");
                    if response.changed() {
                        self.drafts.active.edited();
                        #[cfg(feature = "messaging")]
                        if let (Some(state), Some(thread)) =
                            (&mut self.live, self.model.selected.as_deref())
                        {
                            state.delivery.edited(thread, &self.drafts.active.text);
                        }
                    }
                    if enabled && self.focus_composer {
                        response.request_focus();
                        self.focus_composer = false;
                    }
                });
                let can_send = enabled
                    && !ime_active
                    && (self.demo || self.live_can_send())
                    && !self.drafts.active.in_flight()
                    && (!self.drafts.active.text.trim().is_empty()
                        || self.drafts.active.attachment().is_some());
                let response = ui
                    .add_enabled_ui(can_send, |ui| {
                        ui.interact(send_rect, ui.id().with("send_local"), Sense::click())
                    })
                    .inner;
                let send_fill = if !can_send {
                    colors.button_primary_disabled
                } else if response.is_pointer_button_down_on() {
                    colors.button_primary_pressed
                } else if response.hovered() {
                    colors.button_primary_hover
                } else {
                    colors.primary
                };
                ui.painter().rect_filled(send_rect, 12.0, send_fill);
                ui.painter().text(
                    send_rect.center(),
                    Align2::CENTER_CENTER,
                    if self.demo { "Lưu" } else { "Gửi" },
                    FontId::new(
                        visuals::TEXT_BODY,
                        egui::FontFamily::Name("Zalo Semibold".into()),
                    ),
                    if can_send {
                        colors.button_primary_text
                    } else {
                        colors.text_placeholder
                    },
                );
                visuals::focus_outline(ui, &response, send_rect, 12.0);
                if response.clicked() {
                    send = true;
                }
                let unavailable = if !enabled {
                    "Đăng nhập và chọn một cuộc trò chuyện để soạn tin."
                } else if ime_active {
                    "Hoàn tất phần nhập đang soạn trước khi gửi."
                } else if self.drafts.active.in_flight() {
                    "Đang chờ kết quả lượt gửi trước; bản nháp được giữ nguyên."
                } else if self.drafts.active.text.trim().is_empty()
                    && self.drafts.active.attachment().is_none()
                {
                    "Nhập nội dung tin nhắn hoặc chọn một tệp."
                } else {
                    "Chưa thể gửi. Kiểm tra kết nối và trạng thái lượt gửi trước."
                };
                response
                    .on_hover_text(if self.demo {
                        "Lưu tin nhắn vào demo trên máy này · Enter"
                    } else {
                        "Gửi tin nhắn tới Zalo · Enter"
                    })
                    .on_disabled_hover_text(unavailable);
            },
        );
        if send {
            self.send_local();
        }
    }

    fn chat(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, compact: bool) {
        let colors = crate::theme::palette(ctx);
        visuals::background(ui, ui.max_rect(), "--surface-background", colors.canvas);
        ui.spacing_mut().item_spacing.y = 0.0;
        self.chat_header(ui, compact);
        self.latch_modal_input(ctx);
        if self.modal_blocked || self.modal_open() {
            ui.disable();
        }
        let composer_height =
            composer_height(ui.available_height(), &self.drafts.active.text, compact)
                + self.attachment_tray_height();
        let status = timeline_status(
            &self.model.status,
            self.model.pending,
            self.demo,
            self.connection_label(),
        )
        .map(str::to_owned);
        let status_height = if status.is_some() { 29.0 } else { 0.0 };
        let timeline_height = (ui.available_height() - composer_height - status_height).max(60.0);
        let timeline_rect =
            egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), timeline_height));

        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), timeline_height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(timeline_height);
                let mut latest = false;
                let mut older = false;
                egui::ScrollArea::vertical()
                    .id_salt((
                        "messages",
                        self.model.selected.clone(),
                        self.model.generation,
                    ))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(timeline_height)
                    .show(ui, |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::symmetric(
                                if compact { 18.0 } else { 30.0 },
                                18.0,
                            ))
                            .show(ui, |ui| {
                                ui.set_min_width(
                                    (timeline_rect.width() - if compact { 38.0 } else { 62.0 })
                                        .max(100.0),
                                );
                                if !self.demo && !self.is_signed_in() {
                                    self.welcome(ui);
                                    return;
                                }
                                ui.vertical_centered(|ui| {
                                    if self.demo {
                                        ui.label(
                                        RichText::new("Dữ liệu minh họa · không gửi tới Zalo")
                                        .size(visuals::TEXT_SMALL)
                                        .color(colors.text_secondary),
                                        );
                                        ui.add_space(12.0);
                                    }
                                    if self.model.has_older
                                        || self.model.pending
                                        || !self.latest_page
                                    {
                                        ui.horizontal(|ui| {
                                            older = ui
                                                .add_enabled(
                                                    self.model.has_older && !self.model.pending,
                                                    egui::Button::new("Xem trang trước").small(),
                                                )
                                                .clicked();
                                            latest = ui
                                                .add_enabled(
                                                    !self.model.pending,
                                                    egui::Button::new("Về mới nhất").small(),
                                                )
                                                .clicked();
                                            if self.model.pending {
                                                ui.spinner();
                                            }
                                        });
                                    }
                                });
                                let own_account = self.account_id().map(str::to_owned);
                                let own_name = self.account_label().to_owned();
                                #[cfg(feature="messaging")]
                                if let (Some(state),Some(account),Some(thread))=(&mut self.live,own_account.as_deref(),self.model.selected.as_deref()) {state.media.scope(account,thread,self.model.generation);}
                                let mut previous_day = None;
                                for message in &self.model.messages {
                                    let day = local_parts(message.sent_at_ms)
                                        .map(|parts| (parts.tm_year, parts.tm_yday));
                                    if previous_day != day || previous_day.is_none() {
                                        ui.add_space(8.0);
                                        date_separator(ui, message.sent_at_ms);
                                        ui.add_space(18.0);
                                        previous_day = day;
                                    }
                                    #[cfg(feature = "messaging")]
                                    let resolved=self.live.as_ref().and_then(|state|state.account.as_ref().and_then(|account|state.sender_names.get(&account.id,self.model.generation,&message.thread_id,&message.sender)));
                                    #[cfg(not(feature = "messaging"))]
                                    let resolved=None;
                                    let sender=senders::label(message,own_account.as_deref(),Some(&own_name),self.model.conversations.iter().find(|thread|thread.id==message.thread_id),resolved);
                                    // Own the small label before independently borrowing the texture cache.
                                    let sender_name=sender.name.to_owned();let sender_avatar=sender.avatar.to_owned();
                                    #[cfg(feature = "messaging")]
                                    let avatar_texture = self.live.as_mut().and_then(|state| {
                                        let identity = format!("d:{}",message.sender);
                                        (state.privacy.ready() && state.privacy.visible(&identity)
                                            && ui.is_rect_visible(egui::Rect::from_min_size(ui.cursor().min,vec2(30.0,30.0))))
                                            .then(||state.avatars.visible(ui.ctx(), &identity)).flatten()
                                    });
                                    #[cfg(not(feature = "messaging"))]
                                    let avatar_texture: Option<egui::TextureHandle> = None;
                                    #[cfg(feature = "messaging")]
                                    let (sticker_cache,media_cache) = match self.live.as_mut() {
                                        Some(state) if state.account.is_some() => (Some(&mut state.stickers),Some(&mut state.media)),
                                        _ => (None,None),
                                    };
                                    message_bubble(ui, message, compact, own_account.as_deref(), &sender_name,&sender_avatar,avatar_texture.as_ref(),
                                        #[cfg(feature = "messaging")]
                                        sticker_cache,
                                        #[cfg(feature = "messaging")]
                                        media_cache,
                                        self.model.generation,
                                    );
                                    ui.add_space(13.0);
                                }
                                #[cfg(feature = "messaging")]
                                if self.is_signed_in() && self.model.selected.is_none() {
                                    ui.add_space(18.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new("Chưa có hội thoại trên thiết bị này").family(egui::FontFamily::Name("Zalo Semibold".into())));
                                        ui.add_space(8.0);
                                        ui.label("Chọn Danh bạ để mở liên hệ hoặc nhóm đã đồng bộ. Lịch sử cũ có thể chưa được nhập.");
                                        ui.add_space(10.0);
                                        if ui.button("Mở danh bạ").clicked() {
                                            self.activate_directory(ctx);
                                        }
                                        if ui.small_button("Mở hội thoại bằng ID").clicked() {
                                            if let Some(state) = &mut self.live {
                                                state.ensure_worker(self._profile.clone());
                                                state.open_dialog = true;
                                            }
                                        }
                                    });
                                }
                                if self.model.messages.is_empty() && !self.model.pending && self.model.selected.is_some() {
                                    ui.add_space(30.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new("Bắt đầu một cuộc trò chuyện")
                                                .color(colors.text_secondary),
                                        );
                                    });
                                }
                                ui.add_space(8.0);
                            });
                    });
                if older {
                    self.older_page();
                }
                if latest {
                    if let Some(thread) = self.model.selected.clone() {
                        self.load_latest(thread);
                    }
                }
            },
        );
        if let Some(status) = status {
            let (status_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), status_height), Sense::hover());
            positioned_ui(
                ui,
                egui::UiBuilder::new().max_rect(status_rect.shrink2(vec2(20.0, 4.0))),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&status)
                                .size(visuals::TEXT_SMALL)
                                .color(colors.text_secondary),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&status);
                },
            );
        }
        self.composer(ui, ctx, composer_height);
    }

    fn workspace(&mut self, ctx: &egui::Context) {
        self.sync_account_popup();
        let colors = crate::theme::palette(ctx);
        let compact = ctx.screen_rect().width() < COMPACT_BREAKPOINT;
        self.navigation(ctx, compact);
        self.latch_modal_input(ctx);
        #[cfg(feature = "messaging")]
        let privacy_pending = !self.privacy_ready();
        #[cfg(not(feature = "messaging"))]
        let privacy_pending = false;
        #[cfg(feature = "messaging")]
        let show_directory = self.directory_visible();
        #[cfg(not(feature = "messaging"))]
        let show_directory = false;
        if show_directory || (compact && self.compact_list) || self.bound_conversation().is_none() {
            self.conversation_details = None;
        }
        if privacy_pending {
            #[cfg(feature = "messaging")]
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.canvas).inner_margin(24.0))
                .show(ctx, |ui| self.show_privacy_neutral(ui));
        } else if compact && show_directory {
            #[cfg(feature = "messaging")]
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.surface))
                .show(ctx, |ui| {
                    if self.modal_blocked || self.modal_open() {
                        ui.disable();
                    }
                    #[cfg(feature = "messaging")]
                    if !self.privacy_ready() {
                        self.show_privacy_neutral(ui);
                        return;
                    }
                    visuals::background(ui, ui.max_rect(), "--layer-background", colors.surface);
                    self.directory_panel(ui);
                });
        } else if compact && self.compact_list {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.surface))
                .show(ctx, |ui| {
                    visuals::background(ui, ui.max_rect(), "--layer-background", colors.surface);
                    self.conversations(ui, true);
                });
        } else {
            if !compact {
                egui::SidePanel::left("conversations")
                    .default_width(CONVERSATION_LIST_WIDTH)
                    .width_range(CONVERSATION_LIST_MIN_WIDTH..=CONVERSATION_LIST_MAX_WIDTH)
                    .resizable(true)
                    .frame(
                        egui::Frame::none()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0_f32, colors.border)),
                    )
                    .show(ctx, |ui| {
                        if self.modal_blocked || self.modal_open() {
                            ui.disable();
                        }
                        visuals::background(
                            ui,
                            ui.max_rect(),
                            "--layer-background",
                            colors.surface,
                        );
                        #[cfg(feature = "messaging")]
                        if show_directory {
                            self.directory_panel(ui);
                            return;
                        }
                        self.conversations(ui, false);
                    });
            }
            // Original source aside is 344px. Keep at least 360px for the chat.
            let dock_details =
                ctx.available_rect().width() >= CONVERSATION_INFO_WIDTH + MIN_CHAT_WIDTH_WITH_INFO;
            if dock_details && self.bound_conversation().is_some() {
                self.conversation_sidebar(ctx);
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.canvas))
                .show(ctx, |ui| {
                    if self.modal_blocked || self.modal_open() {
                        ui.disable();
                    }
                    #[cfg(feature = "messaging")]
                    if !self.privacy_ready() {
                        self.show_privacy_neutral(ui);
                        return;
                    }
                    if !dock_details && self.bound_conversation().is_some() {
                        self.conversation_information(ui, true);
                    } else {
                        self.chat(ui, ctx, compact);
                    }
                });
        }
        let application_information = self.details && !self.settings_open;
        if application_information {
            self.information(ctx);
        }
        #[cfg(feature = "live-login")]
        if !self.settings_open && !application_information {
            self.live_dialogs(ctx);
        }
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            media::viewer(ctx, &mut state.media);
        }
        self.show_account_popup(ctx);
        #[cfg(feature = "messaging")]
        if !self.settings_open && !application_information {
            self.hidden_pin_dialog(ctx);
        }
    }

    fn welcome(&self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        ui.add_space(42.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Không gian trò chuyện của bạn").size(24.0).family(egui::FontFamily::Name("Zalo Semibold".into())));
            ui.add_space(12.0);
            ui.label(RichText::new("Giao diện native, được xây dựng bằng Rust.").color(colors.text_secondary));
            ui.add_space(25.0);
            egui::Frame::none().fill(colors.surface).rounding(12.0).inner_margin(22.0).show(ui, |ui| {
                ui.set_max_width(390.0);
                ui.label(RichText::new("Bản xem trước đang được phát triển").family(egui::FontFamily::Name("Zalo Semibold".into())));
                ui.add_space(10.0);
                ui.label("Đăng nhập và gửi nhận Zalo chưa được bật. Chạy với --demo để trải nghiệm các hội thoại minh họa và lưu tin nhắn cục bộ.");
            });
        });
    }

    fn information(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let mut close = false;
        let response = dialog::show(
            ctx,
            "application_information",
            "Thông tin ứng dụng",
            400.0,
            false,
            |ui| {
                dialog::body(ui, |ui| {
                    ui.label(
                        RichText::new("Zalo Native")
                            .size(20.0)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    );
                    #[cfg(feature = "live-login")]
                    if self.live_information(ui) {
                        return;
                    }
                    ui.label("Bản xem trước sử dụng các cuộc trò chuyện minh họa, lưu riêng trên thiết bị này.");
                    ui.label("Bạn có thể tìm hội thoại, chuyển bản nháp, đọc từng trang và lưu tin nhắn cục bộ.");
                    ui.label("Bản demo không gửi tin tới Zalo. Gọi điện và gửi ảnh, tệp chưa được hỗ trợ.");
                    ui.label(
                        RichText::new("Dữ liệu của ứng dụng Electron được giữ riêng.")
                            .size(12.0)
                            .color(colors.text_secondary),
                    );
                });
                dialog::footer(ui, |ui| {
                    close = ui.button("Đóng").clicked();
                });
            },
        );
        if response.closed || close {
            self.details = false;
        }
    }

    fn bound_conversation(&self) -> Option<&Conversation> {
        let (account, selected) = self.conversation_details.as_ref()?;
        #[cfg(feature = "messaging")]
        if !self.privacy_visible(selected) {
            return None;
        }
        if account != self.account_id().unwrap_or(crate::app::DEMO_ACCOUNT)
            || self.model.selected.as_deref() != Some(selected.as_str())
        {
            return None;
        }
        #[cfg(feature = "messaging")]
        {
            self.visible_conversation(selected)
        }
        #[cfg(not(feature = "messaging"))]
        {
            self.model
                .conversations
                .iter()
                .find(|thread| thread.id == *selected)
        }
    }

    fn conversation_sidebar(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        egui::SidePanel::right("conversation_information_sidebar")
            .exact_width(CONVERSATION_INFO_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0_f32, colors.divider)),
            )
            .show(ctx, |ui| self.conversation_information(ui, false));
    }

    fn conversation_information(&mut self, ui: &mut egui::Ui, compact: bool) {
        if self.modal_blocked || self.modal_open() {
            ui.disable();
        }
        let Some(thread) = self.bound_conversation().cloned() else {
            self.conversation_details = None;
            return;
        };
        let colors = crate::theme::palette(ui.ctx());
        visuals::background(ui, ui.max_rect(), "--layer-background", colors.surface);
        ui.spacing_mut().item_spacing.y = 0.0;
        let (header, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), CHAT_HEADER_HEIGHT),
            Sense::hover(),
        );
        divider(ui, header.left_bottom(), header.right_bottom());
        ui.painter().text(
            header.center(),
            Align2::CENTER_CENTER,
            "Thông tin hội thoại",
            FontId::new(14.0, egui::FontFamily::Name("Zalo Semibold".into())),
            colors.text_primary,
        );
        let close = egui::Rect::from_min_size(
            if compact {
                header.left_top() + vec2(12.0, 16.0)
            } else {
                header.right_top() + vec2(-56.0, 16.0)
            },
            vec2(44.0, 44.0),
        );
        let response = ui.interact(
            close,
            ui.id().with("close_conversation_information"),
            Sense::click(),
        );
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                close,
                12.0,
                if response.is_pointer_button_down_on() {
                    colors.pressed
                } else {
                    colors.hover
                },
            );
        }
        if compact {
            icon(ui, close.shrink(12.0), Icon::Back, colors.text_secondary);
        } else {
            visuals::source_icon(ui, close.shrink(12.0), "fa-close", colors.text_secondary);
        }
        visuals::focus_outline(ui, &response, close, 12.0);
        if response.clicked() {
            self.conversation_details = None;
            self.focus_composer = true;
        }
        response.on_hover_text("Đóng thông tin hội thoại");
        egui::ScrollArea::vertical()
            .id_salt(("conversation_information_body", &thread.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none().inner_margin(16.0).show(ui, |ui| {
                    self.information_body(ui, &thread);
                });
            });
    }
}

impl eframe::App for NativeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(test)]
        if let Some(theme) = self.fixture_system_theme {
            ctx.input_mut(|input| input.raw.system_theme = Some(theme));
        }
        self.theme.poll(ctx);
        self.poll_events(ctx);
        #[cfg(feature = "live-login")]
        self.poll_live(ctx);
        #[cfg(all(test, feature = "messaging"))]
        if let Some(connection) = self.fixture_connection {
            if let Some(state) = &mut self.live {
                state.connection = connection;
            }
        }
        self.sync_account_popup();
        self.update_modal_input(ctx);
        #[cfg(feature = "live-login")]
        {
            if self.show_login(ctx) {
                self.conversation_details = None;
                self.latch_modal_input(ctx);
                self.show_account_popup(ctx);
                if self.settings_open {
                    self.settings
                        .show(ctx, &mut self.settings_open, &mut self.theme);
                }
                return;
            }
        }
        ctx.input(|input| update_ime_state(&mut self.ime_composing, &input.events));
        self.workspace(ctx);
        if self.settings_open {
            self.latch_modal_input(ctx);
            self.settings
                .show(ctx, &mut self.settings_open, &mut self.theme);
        }
    }
}

/// Position content inside an already allocated region without advancing its parent's cursor twice.
fn positioned_ui<R>(
    ui: &mut egui::Ui,
    builder: egui::UiBuilder,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut child = ui.new_child(builder);
    contents(&mut child)
}

fn display_title(thread: &Conversation) -> String {
    match thread.id.as_str() {
        "welcome" => "Bắt đầu với Zalo Native".into(),
        "architecture" => "Không gian phát triển".into(),
        _ if thread.title.trim().is_empty() || thread.title == thread.id => {
            if thread.id.starts_with("g:") {
                "Nhóm trò chuyện".into()
            } else {
                "Cuộc trò chuyện".into()
            }
        }
        _ => thread.title.clone(),
    }
}

fn timeline_status<'a>(
    status: &'a str,
    pending: bool,
    demo: bool,
    connection: &'a str,
) -> Option<&'a str> {
    // Suppress only these exact routine labels; faults and delivery notices remain visible.
    if !matches!(
        status,
        "" | "Ngoại tuyến" | "Đã kết nối Zalo" | "Chưa đăng nhập"
    ) {
        return Some(
            if status == "Stored locally in the offline demo; nothing was sent to Zalo." {
                "Đã lưu trên thiết bị · không gửi tới Zalo"
            } else {
                status
            },
        );
    }
    if pending {
        Some("Đang mở hội thoại…")
    } else if demo {
        Some("Demo ngoại tuyến. Tin nhắn chỉ được lưu trên thiết bị này.")
    } else if matches!(
        connection,
        "Ngoại tuyến" | "Đã kết nối Zalo" | "Chưa đăng nhập"
    ) {
        None
    } else {
        Some(connection)
    }
}

fn composer_height(available_height: f32, draft: &str, compact: bool) -> f32 {
    // The card holds a 44px toolbar above the editor and send control.
    // Keep the compact geometry; expand only for a multiline draft.
    if !compact
        && available_height >= 340.0
        && draft.bytes().filter(|byte| *byte == b'\n').take(2).count() >= 2
    {
        168.0
    } else {
        136.0
    }
}

fn conversation_row(
    ui: &mut egui::Ui,
    width: f32,
    thread: &Conversation,
    title: &str,
    selected: bool,
    account: Option<&str>,
    texture: Option<&egui::TextureHandle>,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, CONVERSATION_ROW_HEIGHT), Sense::hover());
    let response = ui.interact(
        rect,
        ui.id().with(("conversation", account, &thread.id)),
        Sense::click(),
    );
    if response.clicked() {
        response.request_focus();
    }
    if selected || response.hovered() || response.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect.shrink2(vec2(8.0, 2.0)),
            14.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else if selected {
                colors.selected
            } else {
                colors.hover
            },
        );
    }
    visuals::focus_outline(ui, &response, rect.shrink2(vec2(8.0, 2.0)), 14.0);
    let photo = egui::Rect::from_min_size(rect.min + vec2(20.0, 12.0), vec2(48.0, 48.0));
    visuals::avatar_photo(ui, photo, title, texture);
    let content =
        egui::Rect::from_min_max(rect.min + vec2(80.0, 15.0), rect.max - vec2(20.0, 12.0));
    // The complete card is one widget. Child Labels create competing pointer
    // regions, so painting text keeps preview/title/avatar/padding on this hitbox.
    row_text(
        ui,
        content.min,
        (content.width() - 40.0).max(0.0),
        title,
        FontId::new(
            visuals::TEXT_BODY,
            egui::FontFamily::Name("Zalo Semibold".into()),
        ),
        if ui.is_enabled() {
            colors.text_primary
        } else {
            colors.text_disabled
        },
    );
    ui.painter().text(
        content.right_top() + vec2(0.0, 4.0),
        Align2::RIGHT_TOP,
        clock_label(thread.last_message_at_ms),
        FontId::proportional(visuals::TEXT_SMALL),
        colors.text_secondary,
    );
    let preview = if thread.last_text.is_empty() {
        "Chưa có tin nhắn".into()
    } else if is_own_sender(&thread.last_sender, account) {
        format!("Bạn: {}", thread.last_text)
    } else {
        thread.last_text.clone()
    };
    row_text(
        ui,
        content.min + vec2(0.0, 26.0),
        content.width().max(0.0),
        &preview.replace(['\n', '\r'], " "),
        FontId::proportional(visuals::TEXT_SMALL),
        if ui.is_enabled() {
            colors.text_secondary
        } else {
            colors.text_disabled
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            title,
        )
    });
    response.on_hover_text(title)
}

fn row_text(
    ui: &egui::Ui,
    position: egui::Pos2,
    width: f32,
    text: &str,
    font: FontId,
    color: egui::Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(position, galley, color);
}

fn date_separator(ui: &mut egui::Ui, milliseconds: i64) {
    let colors = crate::theme::palette(ui.ctx());
    let text = date_label(milliseconds);
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::new(
            visuals::TEXT_SMALL,
            egui::FontFamily::Name("Zalo Semibold".into()),
        ),
        colors.date_text,
    );
    let (row, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 14.0),
        Sense::hover(),
    );
    let pill = egui::Rect::from_center_size(row.center(), galley.size() + vec2(24.0, 8.0));
    ui.painter().rect_filled(pill, 12.0, colors.date_background);
    ui.painter()
        .galley(pill.min + vec2(12.0, 4.0), galley, colors.date_text);
}

fn is_own_sender(sender: &str, account: Option<&str>) -> bool {
    account.map_or_else(|| sender == "You (local demo)", |account| sender == account)
}

#[allow(clippy::too_many_arguments)]
fn message_bubble(
    ui: &mut egui::Ui,
    message: &Message,
    compact: bool,
    account: Option<&str>,
    sender_name: &str,
    sender_avatar: &str,
    avatar_texture: Option<&egui::TextureHandle>,
    #[cfg(feature = "messaging")] sticker_cache: Option<&mut stickers::StickerCache>,
    #[cfg(feature = "messaging")] media_cache: Option<&mut media::MediaCache>,
    generation: u64,
) {
    let colors = crate::theme::palette(ui.ctx());
    let own = is_own_sender(&message.sender, account);
    // Own plain text gets the solid accent bubble with on-color text. Media, stickers and cards
    // draw their own labels in the standard text roles, so they keep the surface bubble.
    let accent = own && message.media.is_none() && message.sticker.is_none();
    // A sticker is the picture alone: no bubble fill, border or padding around it.
    let bare = message.sticker.is_some();
    let ink = if accent {
        colors.on_color
    } else {
        colors.text_primary
    };
    let available = ui.available_width();
    let outside = 24.0
        + if own {
            0.0
        } else {
            30.0 + ui.spacing().item_spacing.x
        };
    let max_bubble = (available * if compact { 0.82 } else { 0.76 })
        .min(490.0)
        .min((available - outside).max(1.0));
    let body = (message.media.is_none() && message.sticker.is_none()).then(|| {
        ui.painter().layout(
            message.text.clone(),
            FontId::proportional(visuals::TEXT_MESSAGE),
            ink,
            max_bubble,
        )
    });
    let content_width = body
        .as_ref()
        .map(|body| {
            let sender_width = if own {
                0.0
            } else {
                ui.painter()
                    .layout_no_wrap(
                        sender_name.into(),
                        FontId::proportional(visuals::TEXT_SMALL),
                        colors.text_secondary,
                    )
                    .size()
                    .x
                    .min(max_bubble)
            };
            let (time, status) = status_galleys(
                ui,
                message.sent_at_ms,
                own,
                account.is_some(),
                colors.text_secondary,
            );
            let status_width = time.size().x
                + status.as_ref().map_or(0.0, |status| {
                    ui.spacing().item_spacing.x * 2.0 + 12.0 + status.size().x
                });
            body.size()
                .x
                .max(sender_width)
                .max(status_width)
                .min(max_bubble)
        })
        .unwrap_or_else(|| {
            if !bare {
                return max_bubble;
            }
            // Fit the sticker: as wide as the picture, or the sender/status line if that is wider.
            let sender_width = if own {
                0.0
            } else {
                ui.painter()
                    .layout_no_wrap(
                        sender_name.into(),
                        FontId::proportional(visuals::TEXT_SMALL),
                        colors.text_secondary,
                    )
                    .size()
                    .x
            };
            let (time, status) = status_galleys(
                ui,
                message.sent_at_ms,
                own,
                account.is_some(),
                colors.text_secondary,
            );
            let status_width = time.size().x
                + status.as_ref().map_or(0.0, |status| {
                    ui.spacing().item_spacing.x * 2.0 + 12.0 + status.size().x
                });
            STICKER_SIZE
                .max(sender_width)
                .max(status_width)
                .min(max_bubble)
        });
    ui.with_layout(
        if own {
            Layout::right_to_left(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        },
        |ui| {
            if !own {
                let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                visuals::avatar_photo(ui, rect, sender_avatar, avatar_texture);
            }
            egui::Frame::none()
                .fill(if bare {
                    egui::Color32::TRANSPARENT
                } else if accent {
                    colors.bubble_own
                } else {
                    colors.bubble_incoming
                })
                .stroke(if bare || accent {
                    Stroke::NONE
                } else {
                    Stroke::new(1.0_f32, colors.border)
                })
                .rounding(egui::Rounding {
                    nw: 18.0,
                    ne: 18.0,
                    sw: if own { 18.0 } else { 4.0 },
                    se: if own { 4.0 } else { 18.0 },
                })
                .inner_margin(if bare {
                    egui::Margin::ZERO
                } else {
                    egui::Margin::symmetric(14.0, 10.0)
                })
                .show(ui, |ui| {
                    // A right-to-left frame starts with its minimum rectangle at
                    // the right edge. Give plain content its measured width before
                    // entering a top-down layout, so that origin cannot reserve
                    // the entire maximum bubble width for a short message.
                    ui.set_max_width(content_width);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        if !own {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(sender_name)
                                        .size(visuals::TEXT_SMALL)
                                        .family(egui::FontFamily::Name("Zalo Semibold".into()))
                                        .color(colors.text_tertiary),
                                )
                                .truncate(),
                            )
                            .on_hover_text(sender_name);
                        }
                        if message.media.is_some() {
                            #[cfg(feature = "messaging")]
                            media::show(ui, message, generation, media_cache, max_bubble);
                            #[cfg(not(feature = "messaging"))]
                            {
                                let _ = generation;
                                ui.add(egui::Label::new(message.preview_text()).wrap());
                            }
                        } else if let Some(sticker) = &message.sticker {
                            #[cfg(feature = "messaging")]
                            let rendered = match sticker_cache {
                                Some(cache)
                                    if ui.is_rect_visible(egui::Rect::from_min_size(
                                        ui.cursor().min,
                                        vec2(130.0, 150.0),
                                    )) =>
                                {
                                    match cache.visible(ui.ctx(), *sticker) {
                                        stickers::Presentation::Image { texture, uv } => {
                                            // `uv` is the visible frame; its pixel size, not the
                                            // atlas size, decides the display size.
                                            let size = vec2(
                                                uv.width() * texture.size_vec2().x,
                                                uv.height() * texture.size_vec2().y,
                                            );
                                            let display = visuals::fit_image(
                                                size,
                                                vec2(
                                                    ui.available_width().min(STICKER_SIZE),
                                                    STICKER_SIZE,
                                                ),
                                                ui.ctx().pixels_per_point(),
                                            );
                                            ui.add(
                                                egui::Image::new(&texture)
                                                    .uv(uv)
                                                    // `display` already has the frame's
                                                    // aspect; the atlas's own is different.
                                                    .maintain_aspect_ratio(false)
                                                    .fit_to_exact_size(display),
                                            )
                                            .on_hover_text("Nhãn dán");
                                        }
                                        stickers::Presentation::Loading => {
                                            sticker_placeholder(ui, "Đang tải nhãn dán…");
                                        }
                                        stickers::Presentation::Unavailable(reason) => {
                                            sticker_placeholder(ui, "Chưa tải được nhãn dán")
                                                .on_hover_text(reason);
                                        }
                                    }
                                    true
                                }
                                _ => false,
                            };
                            #[cfg(not(feature = "messaging"))]
                            let rendered = false;
                            if !rendered {
                                let _ = sticker;
                                sticker_placeholder(ui, "Nhãn dán");
                            }
                        } else if let Some(body) = &body {
                            ui.add(egui::Label::new(body.clone()));
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&message.text)
                                        .size(visuals::TEXT_MESSAGE)
                                        .color(ink),
                                )
                                .wrap(),
                            );
                        }
                        message_status(ui, message.sent_at_ms, own, account.is_some(), accent);
                    });
                });
        },
    );
}

fn message_status(ui: &mut egui::Ui, sent_at_ms: i64, own: bool, live: bool, accent: bool) {
    let colors = crate::theme::palette(ui.ctx());
    // Time and delivery text sit on the accent fill for own plain text; keep them readable there.
    let meta = if accent {
        colors.on_color
    } else {
        colors.text_secondary
    };
    let (time, status) = status_galleys(ui, sent_at_ms, own, live, meta);
    let gap = ui.spacing().item_spacing.x;
    let width = time.size().x
        + status
            .as_ref()
            .map_or(0.0, |status| gap + 12.0 + gap + status.size().x);
    let height = time
        .size()
        .y
        .max(status.as_ref().map_or(0.0, |status| status.size().y))
        .max(12.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().galley(rect.left_top(), time.clone(), meta);
    if let Some(status) = status {
        let check = egui::Rect::from_min_size(
            rect.left_top() + vec2(time.size().x + gap, (height - 12.0) * 0.5),
            vec2(12.0, 12.0),
        );
        icon(ui, check, Icon::Check, meta);
        ui.painter().galley(
            rect.left_top() + vec2(time.size().x + gap + 12.0 + gap, 0.0),
            status,
            meta,
        );
    }
}

fn status_galleys(
    ui: &egui::Ui,
    sent_at_ms: i64,
    own: bool,
    live: bool,
    color: egui::Color32,
) -> (Arc<egui::Galley>, Option<Arc<egui::Galley>>) {
    let font = FontId::proportional(visuals::TEXT_SMALL);
    let time = ui
        .painter()
        .layout_no_wrap(clock_label(sent_at_ms), font.clone(), color);
    let status = own.then(|| {
        ui.painter().layout_no_wrap(
            if live {
                "Zalo đã nhận"
            } else {
                "Đã lưu trên máy"
            }
            .into(),
            font,
            color,
        )
    });
    (time, status)
}

fn sticker_placeholder(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::none()
        .fill(colors.selected)
        .rounding(8.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(106.0);
            ui.set_min_height(106.0);
            ui.vertical_centered(|ui| {
                ui.add_space(25.0);
                let (rect, _) = ui.allocate_exact_size(vec2(25.0, 25.0), Sense::hover());
                icon(ui, rect, Icon::Smile, colors.text_secondary);
                ui.add(
                    egui::Label::new(
                        RichText::new(label)
                            .size(visuals::TEXT_SMALL)
                            .color(colors.text_secondary),
                    )
                    .wrap(),
                );
            });
        })
        .response
}

fn update_ime_state(composing: &mut bool, events: &[egui::Event]) {
    for event in events {
        if let egui::Event::Ime(event) = event {
            match event {
                egui::ImeEvent::Preedit(text) => *composing = !text.is_empty(),
                egui::ImeEvent::Commit(text) => {
                    *composing = false;
                    if !text.is_empty() {
                        use crate::diagnostics::{Event, Kind, Reason, Stage};
                        crate::diagnostics::record(
                            Event::new(Kind::Debug, Stage::InputMethod, Reason::Ready)
                                .bytes(text.len()),
                        );
                    }
                }
                egui::ImeEvent::Disabled => *composing = false,
                egui::ImeEvent::Enabled => {}
            }
        }
    }
}

fn local_parts(milliseconds: i64) -> Option<libc::tm> {
    let seconds: libc::time_t = milliseconds / 1000;
    let mut result = std::mem::MaybeUninit::<libc::tm>::uninit();
    // localtime_r writes to our owned output and uses no shared static tm storage.
    let pointer = unsafe { libc::localtime_r(&seconds, result.as_mut_ptr()) };
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { result.assume_init() })
    }
}

fn clock_label(milliseconds: i64) -> String {
    local_parts(milliseconds)
        .map(|parts| format!("{:02}:{:02}", parts.tm_hour, parts.tm_min))
        .unwrap_or_else(|| "—".into())
}

fn date_label(milliseconds: i64) -> String {
    local_parts(milliseconds)
        .map(|parts| {
            format!(
                "{} tháng {}, {}",
                parts.tm_mday,
                parts.tm_mon + 1,
                parts.tm_year + 1900
            )
        })
        .unwrap_or_else(|| "Tin nhắn đã lưu".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routine_connection_labels_do_not_hide_faults_loading_or_delivery_uncertainty() {
        assert_eq!(timeline_status("", false, false, "Đã kết nối Zalo"), None);
        assert_eq!(
            timeline_status("Ngoại tuyến", false, false, "Ngoại tuyến"),
            None
        );
        assert_eq!(
            timeline_status("", true, false, "Đã kết nối Zalo"),
            Some("Đang mở hội thoại…")
        );
        for status in [
            "Ngoại tuyến · không lưu được lịch sử",
            "Định dạng kết nối nhận tin chưa được hỗ trợ.",
            "Zalo đã nhận tin nhưng chưa lưu được kết quả trên thiết bị.",
            "Chưa xác định kết quả gửi; không tự gửi lại.",
            "Background queue is full. Your draft is preserved; try again.",
        ] {
            assert_eq!(
                timeline_status(status, false, false, "Đã kết nối Zalo"),
                Some(status)
            );
            assert_eq!(
                timeline_status(status, true, false, "Đã kết nối Zalo"),
                Some(status)
            );
        }
        assert_eq!(
            timeline_status("", false, false, "Đang kết nối lại…"),
            Some("Đang kết nối lại…")
        );
        assert_eq!(
            timeline_status("", false, false, "Phiên đã hết hạn"),
            Some("Phiên đã hết hạn")
        );
        assert_eq!(
            timeline_status(
                "Stored locally in the offline demo; nothing was sent to Zalo.",
                false,
                true,
                "Chưa đăng nhập"
            ),
            Some("Đã lưu trên thiết bị · không gửi tới Zalo")
        );
        assert!(timeline_status("", false, true, "Chưa đăng nhập")
            .unwrap()
            .contains("Demo"));
    }
    #[test]
    fn unnamed_conversations_have_neutral_presentation_without_changing_identity() {
        let mut thread = Conversation {
            id: "d:123".into(),
            title: "d:123".into(),
            last_message_at_ms: 0,
            last_text: String::new(),
            last_sender: String::new(),
        };
        assert_eq!(display_title(&thread), "Cuộc trò chuyện");
        assert_eq!(thread.id, "d:123");
        assert_eq!(thread.title, "d:123");
        thread.id = "g:456".into();
        thread.title.clear();
        assert_eq!(display_title(&thread), "Nhóm trò chuyện");
        thread.title = "Nhóm Thiết kế – Nguyễn Thị Minh Anh".into();
        assert_eq!(display_title(&thread), thread.title);
    }
    #[test]
    fn live_sender_alignment_uses_the_verified_account_without_demo_aliases() {
        assert!(is_own_sender("123", Some("123")));
        assert!(!is_own_sender("456", Some("123")));
        assert!(!is_own_sender("You (local demo)", Some("123")));
        assert!(is_own_sender("You (local demo)", None));
    }
    #[test]
    fn vietnamese_search_accepts_tones_case_and_decomposed_input() {
        assert_eq!(fold_search("  NHÓM THIẾT KẾ  "), "nhom thiet ke");
        assert_eq!(fold_search("Đặng"), "dang");
        assert_eq!(fold_search("Ca\u{0300} phe\u{0302}"), "ca phe");
        assert_eq!(fold_search("日本語"), "日本語");
    }

    #[test]
    fn composer_card_grows_only_for_multiline_wide_drafts() {
        assert_eq!(composer_height(780.0, "", false), 136.0);
        assert_eq!(composer_height(780.0, "một\nhai", false), 136.0);
        assert_eq!(composer_height(780.0, "một\nhai\nba", false), 168.0);
        assert_eq!(composer_height(780.0, "một\nhai\nba", true), 136.0);
        assert_eq!(composer_height(300.0, "một\nhai\nba", false), 136.0);
    }

    #[test]
    fn ime_composition_survives_frames_without_new_events() {
        let mut composing = false;
        update_ime_state(
            &mut composing,
            &[egui::Event::Ime(egui::ImeEvent::Preedit("Vie".into()))],
        );
        assert!(composing);
        update_ime_state(&mut composing, &[]);
        assert!(
            composing,
            "a quiet frame must not enable send while composition is active"
        );
        update_ime_state(
            &mut composing,
            &[egui::Event::Ime(egui::ImeEvent::Commit("Việt".into()))],
        );
        assert!(!composing);
        update_ime_state(
            &mut composing,
            &[egui::Event::Ime(egui::ImeEvent::Preedit("a".into()))],
        );
        update_ime_state(
            &mut composing,
            &[egui::Event::Ime(egui::ImeEvent::Disabled)],
        );
        assert!(!composing);
    }

    #[test]
    fn fixed_header_children_do_not_move_the_timeline_cursor() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (header, _) = ui.allocate_exact_size(vec2(500.0, 68.0), Sense::hover());
                let after_header = ui.cursor().min;
                positioned_ui(
                    ui,
                    egui::UiBuilder::new().max_rect(header.shrink(12.0)),
                    |ui| {
                        ui.label("Nhóm thiết kế");
                    },
                );
                assert_eq!(ui.cursor().min, after_header);
            });
        });
    }
}
