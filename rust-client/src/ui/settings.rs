//! Presentation settings; preference I/O stays in the bounded theme worker.
use crate::theme::{ThemeController, ThemeMode};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub(super) struct Settings {
    query: String,
    selected_token: String,
    override_value: String,
    token_page: usize,
    error: Option<String>,
    show_icons: bool,
    icon_query: String,
    icon_page: usize,
    token_filter: Option<String>,
    token_matches: Vec<&'static crate::theme::catalog::Token>,
    icon_filter: Option<String>,
    icon_matches: Vec<&'static crate::theme::catalog::Glyph>,
}

impl Settings {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, theme: &mut ThemeController) {
        let mut done = false;
        let response = super::dialog::show(
            ctx,
            "appearance_settings",
            "Cài đặt giao diện",
            450.0,
            true,
            |ui| {
                super::dialog::body(ui, |ui| {
                    let colors = crate::theme::palette(ctx);
                    ui.label(
                        RichText::new("Chủ đề Zalo")
                            .family(egui::FontFamily::Name("Zalo Semibold".into()))
                            .size(18.0),
                    );
                    ui.add_space(8.0);
                    let mut mode = theme.mode();
                    ui.horizontal_wrapped(|ui| {
                        for (value, label) in [
                            (ThemeMode::Light, "Sáng"),
                            (ThemeMode::Dark, "Tối"),
                            (ThemeMode::System, "Hệ thống"),
                            (ThemeMode::Custom, "Tùy chỉnh"),
                        ] {
                            ui.selectable_value(&mut mode, value, label);
                        }
                    });
                    if mode != theme.mode() {
                        theme.set_mode(mode);
                        theme.poll(ctx);
                    }
                    ui.label(RichText::new("Giao diện được lưu riêng cho hồ sơ này. Chế độ Hệ thống theo giao diện sáng/tối của hệ điều hành.").size(12.0).color(colors.text_secondary));
                    egui::CollapsingHeader::new("Nâng cao: màu và biểu tượng").id_salt("advanced_appearance").default_open(false).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut self.show_icons, false, "Màu sắc");
                        ui.selectable_value(&mut self.show_icons, true, "Bộ biểu tượng gốc");
                    });
                    if self.show_icons {
                        self.icons(ui);
                    } else if mode == ThemeMode::Custom {
                        self.custom(ui, theme);
                    } else {
                        ui.label(RichText::new("Chọn Tùy chỉnh để thay màu; Sáng và Tối dùng bảng màu thiết kế đã duyệt.").size(12.0).color(colors.text_secondary));
                    }
                });
                    if let Some(error) = theme.persistence_error().or(self.error.as_deref()) {
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new(error).color(colors.error)).wrap());
                    }
                });
                super::dialog::footer(ui, |ui| {
                    if super::visuals::primary_button(ui, true, "Xong", egui::vec2(80.0, 32.0))
                        .clicked()
                    {
                        done = true;
                    }
                });
            },
        );
        if response.closed || done {
            *open = false;
        }
    }

    fn custom(&mut self, ui: &mut egui::Ui, theme: &mut ThemeController) {
        let colors = crate::theme::palette(ui.ctx());
        ui.separator();
        ui.label("Tùy chỉnh màu dựa trên chủ đề Sáng. Một số màu chỉ áp dụng khi thành phần tương ứng được hiển thị.");
        if !self.selected_token.is_empty() {
            ui.add(
                egui::Label::new(RichText::new(&self.selected_token).color(colors.text_secondary))
                    .truncate(),
            )
            .on_hover_text(&self.selected_token);
            let token = crate::theme::catalog::token(&self.selected_token);
            let gradient_role = matches!(
                self.selected_token.as_str(),
                "--surface-background" | "--layer-background-navbar-normal" | "--layer-background"
            );
            let editable = gradient_role || theme.color(&self.selected_token).is_some();
            if let Some(token) = token {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 26.0),
                    egui::Sense::hover(),
                );
                let default_light = crate::theme::default_value(&token.name, false)
                    .unwrap_or_else(|| token.light.clone());
                let raw = theme
                    .overrides()
                    .get(&self.selected_token)
                    .map(String::as_str)
                    .unwrap_or(&default_light);
                if gradient_preview(ui, rect, raw) {
                    // Shared background renderer shows the actual source/custom stop positions.
                } else if let Some(color) = theme.color(&self.selected_token) {
                    ui.painter().rect_filled(rect, 4.0, color);
                    ui.painter()
                        .rect_stroke(rect, 4.0, egui::Stroke::new(1.0_f32, colors.border));
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&default_light)
                                .size(super::visuals::TEXT_SMALL)
                                .color(colors.text_secondary),
                        )
                        .wrap(),
                    );
                }
                if !editable {
                    ui.label("Giá trị nguồn được giữ nguyên; trình chỉnh sửa hiện nhận màu đơn.");
                }
                if gradient_role {
                    ui.label("Nền hỗ trợ màu hoặc linear-gradient hai điểm màu; giữ đúng góc và vị trí nguồn.");
                }
            }
            ui.horizontal(|ui| {
                ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut self.override_value)
                        .hint_text(if gradient_role { "#RRGGBB hoặc linear-gradient(…)" } else { "#RRGGBB hoặc #RRGGBBAA" })
                        .desired_width((ui.available_width()-155.0).max(80.0))
                        .char_limit(256),
                );
                if ui
                    .add_enabled(editable, egui::Button::new("Lưu token"))
                    .clicked()
                {
                    self.error = theme
                        .set_override(&self.selected_token, &self.override_value)
                        .err()
                        .map(|_| {
                            "Giá trị không hợp lệ: dùng màu hex; nền hỗ trợ thêm linear-gradient hai điểm màu."
                                .into()
                        });
                    theme.poll(ui.ctx());
                }
                if ui.button("Kế thừa").clicked() {
                    theme.clear_override(&self.selected_token);
                    theme.poll(ui.ctx());
                    self.error = None;
                }
            });
        }
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Tìm token màu Zalo…")
                    .char_limit(128),
            )
            .changed()
        {
            self.token_page = 0;
        }
        self.filter_tokens();
        let count = self.token_matches.len();
        let pages = count.div_ceil(24).max(1);
        self.token_page = self.token_page.min(pages - 1);
        egui::ScrollArea::vertical()
            .id_salt("custom_token_list")
            .auto_shrink([false, false])
            .min_scrolled_height(150.0)
            .max_height(150.0)
            .show(ui, |ui| {
                for token in self
                    .token_matches
                    .iter()
                    .skip(self.token_page * 24)
                    .take(24)
                {
                    let response = ui.add(
                        egui::Button::new(&token.name)
                            .selected(self.selected_token == token.name)
                            .frame(false),
                    );
                    let clicked = response.clicked();
                    response.on_hover_text(format!(
                        "Sáng: {}\nTối: {}",
                        crate::theme::default_value(&token.name, false)
                            .unwrap_or_else(|| token.light.clone()),
                        crate::theme::default_value(&token.name, true)
                            .unwrap_or_else(|| token.dark.clone())
                    ));
                    if clicked {
                        self.selected_token = token.name.clone();
                        self.override_value = theme
                            .overrides()
                            .get(&token.name)
                            .cloned()
                            .or_else(|| {
                                theme.color(&token.name).map(|color| {
                                    let [r, g, b, a] = color.to_srgba_unmultiplied();
                                    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
                                })
                            })
                            .unwrap_or_default();
                        self.error = None;
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.token_page > 0, egui::Button::new("Trước"))
                .clicked()
            {
                self.token_page -= 1;
            }
            ui.label(format!(
                "{}/{} · {} token",
                self.token_page + 1,
                pages,
                count
            ));
            if ui
                .add_enabled(self.token_page + 1 < pages, egui::Button::new("Tiếp"))
                .clicked()
            {
                self.token_page += 1;
            }
        });
        if ui.button("Đặt lại tùy chỉnh").clicked() {
            theme.reset_custom();
            theme.poll(ui.ctx());
            self.error = None;
        }
    }

    fn icons(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        ui.label("Toàn bộ glyph/alias đã đối chiếu với font gốc. Danh mục này không bật các tính năng chưa hỗ trợ.");
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.icon_query)
                    .hint_text("Tìm tên icon…")
                    .char_limit(128),
            )
            .changed()
        {
            self.icon_page = 0;
        }
        self.filter_icons();
        let count = self.icon_matches.len();
        let pages = count.div_ceil(24).max(1);
        self.icon_page = self.icon_page.min(pages - 1);
        egui::ScrollArea::vertical()
            .id_salt("source_icon_catalog")
            .auto_shrink([false, false])
            .min_scrolled_height(230.0)
            .max_height(230.0)
            .show(ui, |ui| {
                for glyph in self.icon_matches.iter().skip(self.icon_page * 24).take(24) {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                        super::visuals::source_icon(
                            ui,
                            rect.shrink(2.0),
                            &glyph.class,
                            colors.text_primary,
                        );
                        ui.add(egui::Label::new(&glyph.class).truncate())
                            .on_hover_text(format!(
                                "{} · U+{:04X}",
                                glyph.family.name(),
                                glyph.codepoint
                            ));
                    });
                }
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.icon_page > 0, egui::Button::new("Trước"))
                .clicked()
            {
                self.icon_page -= 1;
            }
            ui.label(format!(
                "{}/{} · {} biểu tượng/alias",
                self.icon_page + 1,
                pages,
                count
            ));
            if ui
                .add_enabled(self.icon_page + 1 < pages, egui::Button::new("Tiếp"))
                .clicked()
            {
                self.icon_page += 1;
            }
        });
    }

    fn filter_tokens(&mut self) {
        if self.token_filter.as_deref() == Some(self.query.as_str()) {
            return;
        }
        let query = self.query.to_lowercase();
        self.token_matches = crate::theme::catalog::tokens()
            .filter(|token| query.is_empty() || token.name.to_lowercase().contains(&query))
            .collect();
        self.token_filter = Some(self.query.clone());
    }

    fn filter_icons(&mut self) {
        if self.icon_filter.as_deref() == Some(self.icon_query.as_str()) {
            return;
        }
        let query = self.icon_query.to_lowercase();
        self.icon_matches = crate::theme::catalog::glyphs()
            .filter(|glyph| query.is_empty() || glyph.class.to_lowercase().contains(&query))
            .collect();
        self.icon_filter = Some(self.icon_query.clone());
    }
}

/// Preview uses the same parser and paint path as actual native backgrounds.
fn gradient_preview(ui: &egui::Ui, rect: egui::Rect, raw: &str) -> bool {
    let Some(gradient) = crate::theme::parse_gradient(raw) else {
        return false;
    };
    super::visuals::paint_gradient(ui, rect, gradient);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use std::sync::Arc;

    #[test]
    fn advanced_catalog_search_reuses_results_until_query_changes() {
        let mut settings = Settings::default();
        settings.filter_tokens();
        settings.filter_icons();
        assert!(settings.token_matches.len() > 700);
        assert!(settings.icon_matches.len() > 1000);
        let tokens = settings.token_matches.as_ptr();
        let icons = settings.icon_matches.as_ptr();
        settings.filter_tokens();
        settings.filter_icons();
        assert_eq!(settings.token_matches.as_ptr(), tokens);
        assert_eq!(settings.icon_matches.as_ptr(), icons);
        settings.query = "TEXT-PRIMARY".into();
        settings.icon_query = "poll".into();
        settings.filter_tokens();
        settings.filter_icons();
        assert!(!settings.token_matches.is_empty());
        assert!(settings
            .token_matches
            .iter()
            .all(|token| token.name.contains("text-primary")));
        assert!(!settings.icon_matches.is_empty());
        assert!(settings
            .icon_matches
            .iter()
            .all(|glyph| glyph.class.to_lowercase().contains("poll")));
        assert_eq!(settings.token_filter.as_deref(), Some("TEXT-PRIMARY"));
    }

    fn frame(
        settings: &mut Settings,
        theme: &mut ThemeController,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                theme.poll(ctx);
                settings.show(ctx, &mut true, theme);
            },
        )
    }
    fn visible_text_position(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text_shape) if text_shape.galley.job.text == text => {
                let center = text_shape.pos + text_shape.galley.size() / 2.0;
                shape.clip_rect.contains(center).then_some(center)
            }
            _ => None,
        })
    }
    fn text_position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape) if shape.galley.job.text == text => {
                    Some(shape.pos + shape.galley.size() / 2.0)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing rendered control {text}; visible={:?}",
                    output
                        .shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            })
    }
    fn click(
        settings: &mut Settings,
        theme: &mut ThemeController,
        ctx: &egui::Context,
        width: f32,
        label: &str,
    ) {
        for _ in 0..3 {
            frame(settings, theme, ctx, width, vec![]);
        }
        let mut output = frame(settings, theme, ctx, width, vec![]);
        // Advanced content now scrolls inside the bounded modal body. Exercise
        // actual wheel navigation before clicking, including controls above it.
        for delta in std::iter::repeat_n(-80.0, 8).chain(std::iter::repeat_n(80.0, 16)) {
            if visible_text_position(&output, label).is_some() {
                break;
            }
            output = frame(
                settings,
                theme,
                ctx,
                width,
                vec![
                    egui::Event::PointerMoved(egui::pos2(width / 2.0 + 215.0, 220.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, delta),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..3 {
                output = frame(settings, theme, ctx, width, vec![]);
            }
        }
        let pos = visible_text_position(&output, label).unwrap_or_else(|| {
            panic!(
                "Control {label} remains clipped at {:?}",
                text_position(&output, label)
            );
        });
        for pressed in [true, false] {
            frame(
                settings,
                theme,
                ctx,
                width,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn settings_selector_and_custom_apply_are_real_controls_at_both_widths() {
        for width in [640.0, 1200.0] {
            let root = std::env::temp_dir().join(format!(
                "zalo-theme-controls-{}",
                crate::domain::local_message_id().unwrap()
            ));
            let profile = Profile::at_root(&root, "fixture").unwrap();
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let mut theme =
                ThemeController::new(profile.paths.config_dir.clone(), Arc::new(|| {})).unwrap();
            let mut settings = Settings::default();
            frame(&mut settings, &mut theme, &ctx, width, vec![]);
            click(&mut settings, &mut theme, &ctx, width, "Tối");
            assert_eq!(theme.mode(), ThemeMode::Dark);
            assert!(crate::theme::palette(&ctx).dark);
            click(&mut settings, &mut theme, &ctx, width, "Tùy chỉnh");
            assert_eq!(theme.mode(), ThemeMode::Custom);
            click(
                &mut settings,
                &mut theme,
                &ctx,
                width,
                "Nâng cao: màu và biểu tượng",
            );
            for _ in 0..10 {
                frame(&mut settings, &mut theme, &ctx, width, vec![]);
            }
            settings.query = "text-primary".into();
            click(&mut settings, &mut theme, &ctx, width, "--text-primary");
            assert_eq!(
                settings.selected_token, "--text-primary",
                "token button did not activate at {width}"
            );
            settings.override_value = "#6F3FCF".into();
            click(&mut settings, &mut theme, &ctx, width, "Lưu token");
            assert_eq!(
                theme.overrides().get("--text-primary").map(String::as_str),
                Some("#6F3FCF")
            );
            assert_eq!(
                crate::theme::palette(&ctx).text_primary,
                egui::Color32::from_rgb(111, 63, 207)
            );
            assert!(settings.error.is_none());
            drop(theme);
            drop(profile);
            let _ = std::fs::remove_dir_all(root);
        }
    }
    #[test]
    fn every_original_linear_gradient_can_be_painted_without_inventing_stops() {
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = egui::Rect::from_min_size(egui::pos2(5.0, 5.0), egui::vec2(130.0, 26.0));
                let mut count = 0;
                for token in crate::theme::catalog::tokens() {
                    for raw in &token.raw_values {
                        if raw.starts_with("linear-gradient(") {
                            assert!(
                                gradient_preview(ui, rect, raw),
                                "unsupported original gradient {raw}"
                            );
                            count += 1;
                        }
                    }
                }
                assert!(count > 30);
                assert!(!gradient_preview(ui, rect, "not a gradient"));
            });
        });
        assert!(!output.shapes.is_empty());
    }
}
