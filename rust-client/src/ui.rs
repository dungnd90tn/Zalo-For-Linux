//! Original native desktop UI. File, database, and network work stays off the render thread.
#[cfg(feature = "messaging")]
mod delivery;
#[cfg(feature = "messaging")]
mod directory;
#[cfg(feature = "messaging")]
mod hydration;
#[cfg(feature = "live-login")]
mod live;
#[cfg(feature = "live-login")]
mod login;
#[cfg(feature = "messaging")]
mod media;
mod senders;
mod settings;
#[cfg(feature = "messaging")]
mod stickers;
mod visuals;

use crate::app::{Command, DraftBook, Event, Model, Worker, EVENTS_PER_FRAME};
use crate::domain::{fold_search, Conversation, Message};
use crate::profile::Profile;
use eframe::egui::{self, pos2, vec2, Align, Align2, FontId, Layout, RichText, Sense, Stroke};
use std::sync::Arc;
use visuals::{avatar, divider, icon, icon_button, Icon};

pub struct NativeApp {
    worker: Option<Worker>,
    model: Model,
    drafts: DraftBook,
    demo: bool,
    search: String,
    search_folded: String,
    compact_list: bool,
    details: bool,
    settings_open: bool,
    theme: crate::theme::ThemeController,
    settings: settings::Settings,
    #[cfg(test)]
    fixture_system_theme: Option<egui::Theme>,
    focus_composer: bool,
    latest_page: bool,
    ime_composing: bool,
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
            settings_open: false,
            theme,
            settings: settings::Settings::default(),
            #[cfg(test)]
            fixture_system_theme: None,
            focus_composer: false,
            latest_page: true,
            ime_composing: false,
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
            return if account.name.is_empty() {
                &account.id
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
        if !self.drafts.can_select(&thread) {
            self.model.status = "Đã đạt giới hạn bản nháp hội thoại đang mở.".into();
            return false;
        }
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            if state.account.is_some() && !state.ensure_worker(self._profile.clone()) {
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
        if self.drafts.active.in_flight() || self.drafts.active.text.trim().is_empty() {
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
            .exact_width(64.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(colors.nav_background))
            .show(ctx, |ui| {
                visuals::background(
                    ui,
                    ui.max_rect(),
                    "--layer-background-navbar-normal",
                    colors.nav_background,
                );
                ui.add_space(21.0);
                let (rect, _) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::hover());
                avatar(
                    ui,
                    egui::Rect::from_center_size(rect.center(), vec2(40.0, 40.0)),
                    self.account_label(),
                );
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
                            ui.allocate_exact_size(vec2(64.0, 56.0), Sense::click())
                        })
                        .inner;
                    if selected || response.hovered() || response.is_pointer_button_down_on() {
                        let fill = if response.is_pointer_button_down_on() {
                            colors.nav_pressed
                        } else if selected {
                            colors.nav_selected
                        } else {
                            colors.nav_hover
                        };
                        ui.painter().rect_filled(rect, 0.0, fill);
                    }
                    icon(
                        ui,
                        egui::Rect::from_center_size(rect.center(), vec2(24.0, 24.0)),
                        kind,
                        if enabled {
                            colors.nav_text
                        } else {
                            colors.nav_text_disabled
                        },
                    );
                    visuals::nav_focus_outline(ui, &response, rect);
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
                            }
                            _ => {}
                        }
                    }
                    response.on_hover_text(title).on_disabled_hover_text(title);
                }
                let bottom = ui.max_rect().bottom();
                let y = (bottom - 118.0).max(ui.cursor().top() + 12.0);
                positioned_ui(
                    ui,
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                        pos2(ui.max_rect().left(), y),
                        vec2(64.0, 118.0),
                    )),
                    |ui| {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(64.0, 46.0), Sense::click());
                        if self.details
                            || response.hovered()
                            || response.is_pointer_button_down_on()
                        {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                if response.is_pointer_button_down_on() {
                                    colors.nav_pressed
                                } else if self.details {
                                    colors.nav_selected
                                } else {
                                    colors.nav_hover
                                },
                            );
                        }
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Info,
                            colors.nav_text,
                        );
                        visuals::nav_focus_outline(ui, &response, rect);
                        if response.clicked() {
                            self.details = !self.details;
                        }
                        response.on_hover_text("Thông tin bản xem trước");
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(64.0, 46.0), Sense::click());
                        if self.settings_open
                            || response.hovered()
                            || response.is_pointer_button_down_on()
                        {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                if response.is_pointer_button_down_on() {
                                    colors.nav_pressed
                                } else if self.settings_open {
                                    colors.nav_selected
                                } else {
                                    colors.nav_hover
                                },
                            );
                        }
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Settings,
                            colors.nav_text,
                        );
                        visuals::nav_focus_outline(ui, &response, rect);
                        if response.clicked() {
                            self.settings_open = !self.settings_open;
                        }
                        response.on_hover_text("Cài đặt giao diện");
                    },
                );
            });
    }

    fn conversations(&mut self, ui: &mut egui::Ui, compact: bool) {
        let colors = crate::theme::palette(ui.ctx());
        let width = ui.available_width();
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(16.0, 16.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let search_width = (ui.available_width() - 40.0).max(100.0);
                    egui::Frame::none()
                        .fill(colors.canvas)
                        .rounding(5.0)
                        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
                        .show(ui, |ui| {
                            ui.set_width(search_width - 18.0);
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(16.0, 26.0), Sense::hover());
                                icon(ui, rect, Icon::Search, colors.text_primary);
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
                    #[cfg(feature = "messaging")]
                    let open_live = self
                        .live
                        .as_ref()
                        .is_some_and(|state| state.account.is_some());
                    #[cfg(not(feature = "messaging"))]
                    let open_live = false;
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
                ui.add_space(13.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Tất cả")
                            .color(colors.primary)
                            .family(egui::FontFamily::Name("Zalo Semibold".into()))
                            .size(13.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} hội thoại{}",
                                self.model.conversations.len(),
                                if self.demo { " demo" } else { "" }
                            ))
                            .color(colors.text_secondary)
                            .size(11.0),
                        );
                    });
                });
            });
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation_list")
            .max_height((ui.available_height() - 64.0).max(60.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut matches = 0;
                for thread in &self.model.conversations {
                    let title = display_title(thread);
                    if !self.search_folded.is_empty()
                        && !fold_search(&title).contains(&self.search_folded)
                    {
                        continue;
                    }
                    matches += 1;
                    if conversation_row(
                        ui,
                        width,
                        thread,
                        &title,
                        self.model.selected.as_deref() == Some(&thread.id),
                        self.account_id(),
                    )
                    .clicked()
                    {
                        selection = Some(thread.id.clone());
                    }
                }
                if matches == 0 {
                    ui.add_space(42.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Không tìm thấy hội thoại").color(colors.text_secondary),
                        );
                        ui.add_space(5.0);
                        ui.label(
                            RichText::new("Thử một tên khác.")
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
        let rect = ui.max_rect();
        let footer =
            egui::Rect::from_min_max(pos2(rect.left(), rect.bottom() - 55.0), rect.right_bottom());
        visuals::background(ui, footer, "--layer-background", colors.surface);
        divider(ui, footer.left_top(), footer.right_top());
        #[cfg(feature = "live-login")]
        if self.live_account_footer(ui, footer) {
            return;
        }
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
            } else if self.is_signed_in() {
                self.connection_label()
            } else {
                "Bản xem trước · chưa đăng nhập"
            },
            FontId::proportional(10.5),
            colors.text_secondary,
        );
    }

    fn chat_header(&mut self, ui: &mut egui::Ui, compact: bool) {
        let colors = crate::theme::palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.0), Sense::hover());
        visuals::background(ui, rect, "--layer-background", colors.surface);
        divider(ui, rect.left_bottom(), rect.right_bottom());
        let actions_width = if rect.width() < 490.0 { 42.0 } else { 140.0 };
        let mut left = rect.left() + 20.0;
        if compact {
            let back = egui::Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.top() + 18.0),
                vec2(28.0, 32.0),
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
                self.compact_list = true;
            }
            response.on_hover_text("Danh sách trò chuyện");
            left += 28.0;
        }
        let title = self
            .model
            .selected
            .as_deref()
            .and_then(|id| {
                self.model
                    .conversations
                    .iter()
                    .find(|thread| thread.id == id)
            })
            .map(display_title)
            .unwrap_or_else(|| "Zalo Native".into());
        let photo = egui::Rect::from_min_size(pos2(left, rect.top() + 13.0), vec2(42.0, 42.0));
        avatar(ui, photo, &title);
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                pos2(left + 55.0, rect.top() + 12.0),
                pos2(rect.right() - actions_width - 10.0, rect.bottom() - 10.0),
            )),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(18.0)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    )
                    .truncate(),
                );
                ui.add_space(1.0);
                ui.label(
                    RichText::new(if self.demo {
                        "Hội thoại minh họa · ngoại tuyến"
                    } else {
                        self.connection_label()
                    })
                    .color(colors.text_secondary)
                    .size(10.5),
                );
            },
        );
        positioned_ui(
            ui,
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    pos2(rect.right() - actions_width - 12.0, rect.top() + 18.0),
                    vec2(actions_width, 34.0),
                ))
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                if icon_button(ui, Icon::Info, "Thông tin hội thoại và bản xem trước", true)
                    .clicked()
                {
                    self.details = !self.details;
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
        let enabled = (self.demo || self.is_signed_in()) && self.model.selected.is_some();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        visuals::background(ui, rect, "--layer-background", colors.surface);
        divider(ui, rect.left_top(), rect.right_top());
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
            egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 8.0))),
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
                    icon_button(
                        ui,
                        Icon::Paperclip,
                        "Đính kèm · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if self.demo {
                                "CHỈ LƯU CỤC BỘ"
                            } else {
                                self.connection_label()
                            })
                            .size(9.0)
                            .color(colors.text_secondary),
                        );
                    });
                });
                let send_rect = egui::Rect::from_min_size(
                    pos2(rect.right() - 102.0, rect.top() + 51.0),
                    vec2(82.0, 38.0),
                );
                let editor_rect = egui::Rect::from_min_max(
                    pos2(rect.left() + 20.0, rect.top() + 48.0),
                    pos2(send_rect.left() - 14.0, rect.bottom() - 20.0),
                );
                positioned_ui(ui, egui::UiBuilder::new().max_rect(editor_rect), |ui| {
                    // Cursor, undo, and provisional IME selection belong to this draft.
                    let composer_id = ui.make_persistent_id((
                        "message_composer",
                        self.account_id(),
                        self.model.selected.as_deref(),
                    ));
                    // Consume the shortcut before TextEdit can turn Enter into a newline.
                    if !ime_active
                        && ctx.memory(|memory| memory.has_focus(composer_id))
                        && ctx.input(|input| input.events.iter().any(|event| matches!(event,egui::Event::Key {key:egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if modifiers.matches_logically(egui::Modifiers::COMMAND))))
                        && ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                        })
                    {
                        send = true;
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
                    if response.changed() {
                        self.drafts.active.edited();
                        #[cfg(feature = "messaging")]
                        if let (Some(state), Some(thread)) =
                            (&mut self.live, self.model.selected.as_deref())
                        {
                            state.delivery.edited(thread, &self.drafts.active.text);
                        }
                    }
                    if self.focus_composer {
                        response.request_focus();
                        self.focus_composer = false;
                    }
                });
                let can_send = enabled
                    && !ime_active
                    && (self.demo || self.live_can_send())
                    && !self.drafts.active.in_flight()
                    && !self.drafts.active.text.trim().is_empty();
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
                ui.painter().rect_filled(send_rect, 8.0, send_fill);
                ui.painter().text(
                    send_rect.center(),
                    Align2::CENTER_CENTER,
                    if self.demo { "Lưu" } else { "Gửi" },
                    FontId::proportional(13.0),
                    if can_send {
                        colors.button_primary_text
                    } else {
                        colors.text_placeholder
                    },
                );
                visuals::focus_outline(ui, &response, send_rect, 8.0);
                if response.clicked() {
                    send = true;
                }
                response.on_hover_text(if self.demo {
                    "Lưu tin nhắn vào demo trên máy này · Ctrl+Enter"
                } else {
                    "Gửi tin nhắn tới Zalo · Ctrl+Enter"
                });
                if height > 110.0 {
                    ui.painter().text(
                        pos2(rect.left() + 20.0, rect.bottom() - 13.0),
                        Align2::LEFT_BOTTOM,
                        if self.demo {
                            "Ctrl+Enter để lưu · Enter để xuống dòng"
                        } else {
                            "Ctrl+Enter để gửi · Enter để xuống dòng"
                        },
                        FontId::proportional(9.5),
                        colors.text_secondary,
                    );
                }
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
        let composer_height = if ui.available_height() < 340.0 {
            100.0
        } else {
            132.0
        };
        let timeline_height = (ui.available_height() - composer_height - 29.0).max(60.0);
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
                                    ui.label(
                                        RichText::new(if self.demo {
                                            "Dữ liệu minh họa · không gửi tới Zalo"
                                        } else {
                                            self.connection_label()
                                        })
                                        .size(10.0)
                                        .color(colors.text_secondary),
                                    );
                                    ui.add_space(12.0);
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
                                    let (sticker_cache,media_cache) = match self.live.as_mut() {
                                        Some(state) if state.account.is_some() => (Some(&mut state.stickers),Some(&mut state.media)),
                                        _ => (None,None),
                                    };
                                    message_bubble(ui, message, compact, own_account.as_deref(), &sender_name,&sender_avatar,
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
        let (status_rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 29.0), Sense::hover());

        let status = if self.model.status.is_empty() {
            if self.model.pending {
                "Đang mở hội thoại…"
            } else {
                if self.demo {
                    "Demo ngoại tuyến. Tin nhắn chỉ được lưu trên thiết bị này."
                } else {
                    self.connection_label()
                }
            }
        } else if self.model.status.starts_with("Stored locally") {
            "Đã lưu trên thiết bị · không gửi tới Zalo"
        } else {
            &self.model.status
        };
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(status_rect.shrink2(vec2(20.0, 4.0))),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(status)
                            .size(10.0)
                            .color(colors.text_secondary),
                    )
                    .truncate(),
                )
                .on_hover_text(status);
            },
        );
        self.composer(ui, ctx, composer_height);
    }

    fn workspace(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let compact = ctx.screen_rect().width() < 940.0;
        self.navigation(ctx, compact);
        #[cfg(feature = "messaging")]
        self.live_connection_notice(ctx);
        #[cfg(feature = "messaging")]
        let show_directory = self.directory_visible();
        #[cfg(not(feature = "messaging"))]
        let show_directory = false;
        if compact && show_directory {
            #[cfg(feature = "messaging")]
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.surface))
                .show(ctx, |ui| {
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
                    .default_width(344.0)
                    .width_range(280.0..=420.0)
                    .resizable(true)
                    .frame(
                        egui::Frame::none()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0_f32, colors.border)),
                    )
                    .show(ctx, |ui| {
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
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(colors.canvas))
                .show(ctx, |ui| self.chat(ui, ctx, compact));
        }
        if self.details {
            self.information(ctx);
        }
        #[cfg(feature = "live-login")]
        self.live_dialogs(ctx);
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            media::viewer(ctx, &mut state.media);
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
        let mut open = self.details;
        egui::Window::new("Thông tin bản xem trước").open(&mut open).collapsible(false).resizable(false)
            .default_width(370.0).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Zalo Native · Rust").size(20.0).family(egui::FontFamily::Name("Zalo Semibold".into())));
                ui.add_space(12.0);
                #[cfg(feature = "live-login")]
                if self.live_information(ui) { return; }
                ui.label("Giao diện và mã Rust được xây dựng độc lập. Bản demo sử dụng hội thoại giả lập, lưu riêng trong profile demo trên máy này.");
                ui.add_space(10.0);
                ui.label("Đã hoạt động: tìm hội thoại, chuyển bản nháp, đọc từng trang và lưu tin nhắn cục bộ.");
                ui.add_space(10.0);
                ui.label("Đang phát triển: đăng nhập, gửi nhận thật, gọi điện, ảnh và tệp. Các nút tương ứng được vô hiệu hóa.");
                ui.add_space(12.0);
                ui.label(RichText::new("Dữ liệu của ứng dụng Electron được giữ riêng.").size(11.0).color(colors.text_secondary));
                ui.add_space(8.0);
            });
        self.details = open;
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
        if self.settings_open {
            self.settings
                .show(ctx, &mut self.settings_open, &mut self.theme);
        }
        #[cfg(feature = "live-login")]
        {
            self.poll_live(ctx);
            if self.show_login(ctx) {
                return;
            }
        }
        ctx.input(|input| update_ime_state(&mut self.ime_composing, &input.events));
        self.workspace(ctx);
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
        _ => thread.title.clone(),
    }
}

fn conversation_row(
    ui: &mut egui::Ui,
    width: f32,
    thread: &Conversation,
    title: &str,
    selected: bool,
    account: Option<&str>,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(width, 79.0), Sense::click());
    if selected || response.hovered() || response.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect.shrink2(vec2(8.0, 4.0)),
            6.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else if selected {
                colors.selected
            } else {
                colors.hover
            },
        );
    }
    visuals::focus_outline(ui, &response, rect.shrink2(vec2(8.0, 4.0)), 6.0);
    let photo = egui::Rect::from_min_size(rect.min + vec2(17.0, 14.0), vec2(48.0, 48.0));
    avatar(ui, photo, title);
    let content =
        egui::Rect::from_min_max(rect.min + vec2(78.0, 15.0), rect.max - vec2(18.0, 12.0));
    positioned_ui(ui, egui::UiBuilder::new().max_rect(content), |ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                content.min,
                vec2(content.width() - 40.0, 23.0),
            )),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(13.0)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    )
                    .truncate(),
                );
            },
        );
        ui.painter().text(
            content.right_top() + vec2(0.0, 4.0),
            Align2::RIGHT_TOP,
            clock_label(thread.last_message_at_ms),
            FontId::proportional(9.5),
            colors.text_secondary,
        );
        let preview = if thread.last_text.is_empty() {
            "Chưa có tin nhắn".into()
        } else if is_own_sender(&thread.last_sender, account) {
            format!("Bạn: {}", thread.last_text)
        } else {
            thread.last_text.clone()
        };
        positioned_ui(
            ui,
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                content.min + vec2(0.0, 30.0),
                content.max + vec2(0.0, 5.0),
            )),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(preview.replace(['\n', '\r'], " "))
                            .size(11.5)
                            .color(colors.text_secondary),
                    )
                    .truncate(),
                );
            },
        );
    });
    response
}

fn date_separator(ui: &mut egui::Ui, milliseconds: i64) {
    let colors = crate::theme::palette(ui.ctx());
    let text = date_label(milliseconds);
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(13.0), colors.date_text);
    let (row, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 5.0),
        Sense::hover(),
    );
    let pill = egui::Rect::from_center_size(row.center(), galley.size() + vec2(20.0, 5.0));
    let line = Stroke::new(1.0_f32, colors.date_background);
    ui.painter().line_segment(
        [
            pos2(row.left() + 30.0, row.center().y),
            pos2(pill.left(), row.center().y),
        ],
        line,
    );
    ui.painter().line_segment(
        [
            pos2(pill.right(), row.center().y),
            pos2(row.right() - 30.0, row.center().y),
        ],
        line,
    );
    ui.painter().rect_filled(pill, 10.0, colors.date_background);
    ui.painter()
        .galley(pill.min + vec2(10.0, 2.0), galley, colors.date_text);
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
    #[cfg(feature = "messaging")] sticker_cache: Option<&mut stickers::StickerCache>,
    #[cfg(feature = "messaging")] media_cache: Option<&mut media::MediaCache>,
    generation: u64,
) {
    let colors = crate::theme::palette(ui.ctx());
    let own = is_own_sender(&message.sender, account);
    let max_bubble = (ui.available_width() * if compact { 0.82 } else { 0.76 }).min(490.0);
    ui.with_layout(
        if own {
            Layout::right_to_left(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        },
        |ui| {
            if !own {
                let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                avatar(ui, rect, sender_avatar);
            }
            egui::Frame::none()
                .fill(if own {
                    colors.bubble_own
                } else {
                    colors.bubble_incoming
                })
                .stroke(Stroke::new(0.65_f32, colors.border))
                .rounding(8.0)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        if !own {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(sender_name)
                                        .size(10.5)
                                        .color(colors.text_secondary),
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
                                        stickers::Presentation::Image(texture) => {
                                            let size = texture.size_vec2();
                                            let scale = 130.0 / size.x.max(size.y);
                                            ui.add(
                                                egui::Image::new(&texture)
                                                    .fit_to_exact_size(size * scale),
                                            )
                                            .on_hover_text("Nhãn dán · ảnh xem trước tĩnh");
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
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&message.text)
                                        .size(14.0)
                                        .color(colors.text_primary),
                                )
                                .wrap(),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(clock_label(message.sent_at_ms))
                                    .size(9.0)
                                    .color(colors.text_secondary),
                            );
                            if own {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                icon(ui, rect, Icon::Check, colors.text_secondary);
                                ui.label(
                                    RichText::new(if account.is_some() {
                                        "Zalo đã nhận"
                                    } else {
                                        "Đã lưu trên máy"
                                    })
                                    .size(9.0)
                                    .color(colors.text_secondary),
                                );
                            }
                        });
                    });
                });
        },
    );
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
                    egui::Label::new(RichText::new(label).size(11.0).color(colors.text_secondary))
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
