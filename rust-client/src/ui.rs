//! Original native desktop UI. File, database, and network work stays off the render thread.
mod visuals;

use crate::app::{Command, DraftBook, Event, Model, Worker, EVENTS_PER_FRAME};
use crate::domain::{Conversation, Message};
use crate::profile::Profile;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke,
};
use visuals::{
    avatar, divider, icon, icon_button, Icon, BLUE, CANVAS, HOVER, INK, LINE, MUTED, NAV_BLUE,
    NAV_SELECTED, PALE_BLUE, PLACEHOLDER,
};

pub struct NativeApp {
    worker: Option<Worker>,
    model: Model,
    drafts: DraftBook,
    demo: bool,
    search: String,
    search_folded: String,
    compact_list: bool,
    details: bool,
    focus_composer: bool,
    latest_page: bool,
    ime_composing: bool,
    // The worker shuts down before the profile lock is released.
    _profile: Profile,
}

impl NativeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        profile: Profile,
        demo: bool,
    ) -> anyhow::Result<Self> {
        visuals::install(&cc.egui_ctx);
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
            focus_composer: false,
            latest_page: true,
            ime_composing: false,
            _profile: profile,
        })
    }

    fn load_latest(&mut self, thread: String) -> bool {
        if !self.drafts.can_select(&thread) {
            self.model.status = "Đã đạt giới hạn bản nháp hội thoại đang mở.".into();
            return false;
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
        egui::SidePanel::left("navigation")
            .exact_width(64.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(NAV_BLUE))
            .show(ctx, |ui| {
                ui.add_space(21.0);
                let (rect, _) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::hover());
                avatar(
                    ui,
                    egui::Rect::from_center_size(rect.center(), vec2(40.0, 40.0)),
                    "Demo",
                );
                ui.add_space(27.0);
                for (kind, title, selected) in [
                    (Icon::Chat, "Tin nhắn", true),
                    (
                        Icon::Contacts,
                        "Danh bạ · chưa hỗ trợ trong bản xem trước",
                        false,
                    ),
                    (
                        Icon::Cloud,
                        "Lưu trữ đám mây · chưa hỗ trợ trong bản xem trước",
                        false,
                    ),
                ] {
                    let (rect, response) = ui
                        .add_enabled_ui(selected, |ui| {
                            ui.allocate_exact_size(vec2(64.0, 56.0), Sense::click())
                        })
                        .inner;
                    if selected {
                        ui.painter().rect_filled(rect, 0.0, NAV_SELECTED);
                    }
                    icon(
                        ui,
                        egui::Rect::from_center_size(rect.center(), vec2(24.0, 24.0)),
                        kind,
                        if selected {
                            Color32::WHITE
                        } else {
                            Color32::from_white_alpha(180)
                        },
                    );
                    if selected && response.clicked() && compact {
                        self.compact_list = true;
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
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Info,
                            Color32::WHITE,
                        );
                        if response.clicked() {
                            self.details = !self.details;
                        }
                        response.on_hover_text("Thông tin bản xem trước");
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(64.0, 46.0), Sense::hover());
                        icon(
                            ui,
                            egui::Rect::from_center_size(rect.center(), vec2(23.0, 23.0)),
                            Icon::Settings,
                            Color32::from_white_alpha(180),
                        );
                        response.on_hover_text("Cài đặt · chưa hỗ trợ trong bản xem trước");
                    },
                );
            });
    }

    fn conversations(&mut self, ui: &mut egui::Ui, compact: bool) {
        let width = ui.available_width();
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(16.0, 16.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let search_width = (ui.available_width() - 40.0).max(100.0);
                    egui::Frame::none()
                        .fill(CANVAS)
                        .rounding(5.0)
                        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
                        .show(ui, |ui| {
                            ui.set_width(search_width - 18.0);
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(16.0, 26.0), Sense::hover());
                                icon(ui, rect, Icon::Search, INK);
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
                    icon_button(
                        ui,
                        Icon::Plus,
                        "Tạo hội thoại · chưa hỗ trợ trong bản xem trước",
                        false,
                    );
                });
                ui.add_space(13.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tất cả").color(BLUE).strong().size(13.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} hội thoại demo",
                                self.model.conversations.len()
                            ))
                            .color(MUTED)
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
                    )
                    .clicked()
                    {
                        selection = Some(thread.id.clone());
                    }
                }
                if matches == 0 {
                    ui.add_space(42.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Không tìm thấy hội thoại").color(MUTED));
                        ui.add_space(5.0);
                        ui.label(RichText::new("Thử một tên khác.").size(12.0).color(MUTED));
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
        ui.painter().rect_filled(footer, 0.0, Color32::WHITE);
        divider(ui, footer.left_top(), footer.right_top());
        icon(
            ui,
            egui::Rect::from_min_size(footer.min + vec2(16.0, 17.0), vec2(18.0, 18.0)),
            Icon::Shield,
            MUTED,
        );
        ui.painter().text(
            footer.min + vec2(43.0, 20.0),
            Align2::LEFT_TOP,
            if self.demo {
                "Dữ liệu demo · chỉ lưu trên máy"
            } else {
                "Bản xem trước · chưa đăng nhập"
            },
            FontId::proportional(10.5),
            MUTED,
        );
    }

    fn chat_header(&mut self, ui: &mut egui::Ui, compact: bool) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
        divider(ui, rect.left_bottom(), rect.right_bottom());
        let actions_width = if rect.width() < 490.0 { 42.0 } else { 140.0 };
        let mut left = rect.left() + 20.0;
        if compact {
            let back = egui::Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.top() + 18.0),
                vec2(28.0, 32.0),
            );
            let response = ui.interact(back, ui.id().with("back_to_threads"), Sense::click());
            icon(ui, back.shrink(4.0), Icon::Back, MUTED);
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
                ui.add(egui::Label::new(RichText::new(title).size(18.0).strong()).truncate());
                ui.add_space(1.0);
                ui.label(
                    RichText::new(if self.demo {
                        "Hội thoại minh họa · ngoại tuyến"
                    } else {
                        "Ứng dụng Rust · bản xem trước"
                    })
                    .color(MUTED)
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
        let enabled = self.demo && self.model.selected.is_some();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
        divider(ui, rect.left_top(), rect.right_top());
        let mut send = false;
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
                        ui.label(RichText::new("CHỈ LƯU CỤC BỘ").size(9.0).color(MUTED));
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
                    let composer_id = ui.make_persistent_id("message_composer");
                    let ime_active = self.ime_composing
                        || ctx.input(|input| {
                            input
                                .events
                                .iter()
                                .any(|event| matches!(event, egui::Event::Ime(_)))
                        });
                    // Consume the shortcut before TextEdit can turn Enter into a newline.
                    if !ime_active
                        && ctx.memory(|memory| memory.has_focus(composer_id))
                        && ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                        })
                    {
                        send = true;
                    }
                    let response = ui.add_enabled(
                        enabled,
                        egui::TextEdit::multiline(&mut self.drafts.active.text)
                            .id(composer_id)
                            .hint_text("Nhập tin nhắn…")
                            .desired_rows(2)
                            .desired_width(editor_rect.width())
                            .frame(false)
                            .char_limit(crate::domain::MAX_TEXT_BYTES)
                            .margin(egui::Margin::ZERO),
                    );
                    if response.changed() {
                        self.drafts.active.edited();
                    }
                    if self.focus_composer {
                        response.request_focus();
                        self.focus_composer = false;
                    }
                });
                let can_send = enabled
                    && !self.drafts.active.in_flight()
                    && !self.drafts.active.text.trim().is_empty();
                let response = ui
                    .add_enabled_ui(can_send, |ui| {
                        ui.interact(send_rect, ui.id().with("send_local"), Sense::click())
                    })
                    .inner;
                ui.painter()
                    .rect_filled(send_rect, 8.0, if can_send { BLUE } else { CANVAS });
                ui.painter().text(
                    send_rect.center(),
                    Align2::CENTER_CENTER,
                    "Lưu",
                    FontId::proportional(13.0),
                    if can_send {
                        Color32::WHITE
                    } else {
                        PLACEHOLDER
                    },
                );
                if response.clicked() {
                    send = true;
                }
                response.on_hover_text("Lưu tin nhắn vào demo trên máy này · Ctrl+Enter");
                if height > 110.0 {
                    ui.painter().text(
                        pos2(rect.left() + 20.0, rect.bottom() - 13.0),
                        Align2::LEFT_BOTTOM,
                        "Ctrl+Enter để lưu · Enter để xuống dòng",
                        FontId::proportional(9.5),
                        MUTED,
                    );
                }
            },
        );
        if send {
            self.send_local();
        }
    }

    fn chat(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, compact: bool) {
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
        ui.painter().rect_filled(timeline_rect, 0.0, CANVAS);
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
                                if !self.demo {
                                    self.welcome(ui);
                                    return;
                                }
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("Dữ liệu minh họa · không gửi tới Zalo")
                                            .size(10.0)
                                            .color(MUTED),
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
                                    message_bubble(ui, message, compact);
                                    ui.add_space(13.0);
                                }
                                if self.model.messages.is_empty() && !self.model.pending {
                                    ui.add_space(30.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new("Bắt đầu một cuộc trò chuyện")
                                                .color(MUTED),
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
        ui.painter().rect_filled(status_rect, 0.0, CANVAS);
        let status = if self.model.status.is_empty() {
            if self.model.pending {
                "Đang mở hội thoại…"
            } else {
                "Demo ngoại tuyến. Tin nhắn chỉ được lưu trên thiết bị này."
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
                ui.add(egui::Label::new(RichText::new(status).size(10.0).color(MUTED)).truncate())
                    .on_hover_text(status);
            },
        );
        self.composer(ui, ctx, composer_height);
    }

    fn welcome(&self, ui: &mut egui::Ui) {
        ui.add_space(42.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Không gian trò chuyện của bạn").size(24.0).strong());
            ui.add_space(12.0);
            ui.label(RichText::new("Giao diện native, được xây dựng bằng Rust.").color(MUTED));
            ui.add_space(25.0);
            egui::Frame::none().fill(Color32::WHITE).rounding(12.0).inner_margin(22.0).show(ui, |ui| {
                ui.set_max_width(390.0);
                ui.label(RichText::new("Bản xem trước đang được phát triển").strong());
                ui.add_space(10.0);
                ui.label("Đăng nhập và gửi nhận Zalo chưa được bật. Chạy với --demo để trải nghiệm các hội thoại minh họa và lưu tin nhắn cục bộ.");
            });
        });
    }

    fn information(&mut self, ctx: &egui::Context) {
        let mut open = self.details;
        egui::Window::new("Thông tin bản xem trước").open(&mut open).collapsible(false).resizable(false)
            .default_width(370.0).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Zalo Native · Rust").size(20.0).strong());
                ui.add_space(12.0);
                ui.label("Giao diện và mã Rust được xây dựng độc lập. Bản demo sử dụng hội thoại giả lập, lưu riêng trong profile demo trên máy này.");
                ui.add_space(10.0);
                ui.label("Đã hoạt động: tìm hội thoại, chuyển bản nháp, đọc từng trang và lưu tin nhắn cục bộ.");
                ui.add_space(10.0);
                ui.label("Đang phát triển: đăng nhập, gửi nhận thật, gọi điện, ảnh và tệp. Các nút tương ứng được vô hiệu hóa.");
                ui.add_space(12.0);
                ui.label(RichText::new("Dữ liệu của ứng dụng Electron được giữ riêng.").size(11.0).color(MUTED));
                ui.add_space(8.0);
            });
        self.details = open;
    }
}

impl eframe::App for NativeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);
        ctx.input(|input| update_ime_state(&mut self.ime_composing, &input.events));
        let compact = ctx.screen_rect().width() < 940.0;
        self.navigation(ctx, compact);
        if compact && self.compact_list {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::WHITE))
                .show(ctx, |ui| self.conversations(ui, true));
        } else {
            if !compact {
                egui::SidePanel::left("conversations")
                    .default_width(344.0)
                    .width_range(280.0..=420.0)
                    .resizable(true)
                    .frame(
                        egui::Frame::none()
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0_f32, LINE)),
                    )
                    .show(ctx, |ui| self.conversations(ui, false));
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(CANVAS))
                .show(ctx, |ui| self.chat(ui, ctx, compact));
        }
        if self.details {
            self.information(ctx);
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
        _ => thread.title.clone(),
    }
}

fn conversation_row(
    ui: &mut egui::Ui,
    width: f32,
    thread: &Conversation,
    title: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 79.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(vec2(8.0, 4.0)),
            6.0,
            if selected { PALE_BLUE } else { HOVER },
        );
    }
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
                ui.add(egui::Label::new(RichText::new(title).size(13.0).strong()).truncate());
            },
        );
        ui.painter().text(
            content.right_top() + vec2(0.0, 4.0),
            Align2::RIGHT_TOP,
            clock_label(thread.last_message_at_ms),
            FontId::proportional(9.5),
            MUTED,
        );
        let preview = if thread.last_text.is_empty() {
            "Chưa có tin nhắn".into()
        } else if thread.last_sender == "You (local demo)" {
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
                            .color(MUTED),
                    )
                    .truncate(),
                );
            },
        );
    });
    response
}

fn date_separator(ui: &mut egui::Ui, milliseconds: i64) {
    let text = date_label(milliseconds);
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE);
    let (row, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 5.0),
        Sense::hover(),
    );
    let pill = egui::Rect::from_center_size(row.center(), galley.size() + vec2(20.0, 5.0));
    let line = Stroke::new(1.0_f32, Color32::from_black_alpha(51));
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
    ui.painter()
        .rect_filled(pill, 10.0, Color32::from_black_alpha(51));
    ui.painter()
        .galley(pill.min + vec2(10.0, 2.0), galley, Color32::WHITE);
}

fn message_bubble(ui: &mut egui::Ui, message: &Message, compact: bool) {
    let own = message.sender == "You (local demo)";
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
                avatar(
                    ui,
                    rect,
                    if message.sender == "Demo guide" {
                        "Hướng dẫn"
                    } else {
                        &message.sender
                    },
                );
            }
            egui::Frame::none()
                .fill(if own { PALE_BLUE } else { Color32::WHITE })
                .stroke(Stroke::new(0.65_f32, LINE))
                .rounding(8.0)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        if !own {
                            ui.label(
                                RichText::new(if message.sender == "Demo guide" {
                                    "Hướng dẫn demo"
                                } else {
                                    &message.sender
                                })
                                .size(10.5)
                                .color(MUTED),
                            );
                        }
                        ui.add(
                            egui::Label::new(RichText::new(&message.text).size(14.0).color(INK))
                                .wrap(),
                        );
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(clock_label(message.sent_at_ms))
                                    .size(9.0)
                                    .color(MUTED),
                            );
                            if own {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                icon(ui, rect, Icon::Check, MUTED);
                                ui.label(RichText::new("Đã lưu trên máy").size(9.0).color(MUTED));
                            }
                        });
                    });
                });
        },
    );
}

/// Vietnamese search is case- and tone-insensitive; arbitrary scripts remain searchable.
fn fold_search(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ('\u{0300}'..='\u{036f}').contains(&ch) {
                return None;
            }
            Some(match ch {
                'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
                | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
                'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => {
                    'e'
                }
                'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
                'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
                | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
                'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => {
                    'u'
                }
                'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
                'đ' => 'd',
                _ => ch,
            })
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn update_ime_state(composing: &mut bool, events: &[egui::Event]) {
    for event in events {
        if let egui::Event::Ime(event) = event {
            match event {
                egui::ImeEvent::Preedit(text) => *composing = !text.is_empty(),
                egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled => *composing = false,
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
