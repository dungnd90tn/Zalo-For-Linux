//! Body of the conversation information panel: identity, quick actions, a summary of the saved
//! history and the pictures, files and links that appeared in it. Everything shown comes from
//! stored data delivered by the background worker; nothing here reads a database or the network.
use super::*;
use crate::domain::{SharedItem, SharedKind, OVERVIEW_ITEM_LIMIT};
use std::time::Duration;

/// Which list of the saved history is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InfoTab {
    #[default]
    Images,
    Files,
    Links,
}

const ROW_HEIGHT: f32 = 60.0;
const TILE_HEIGHT: f32 = 76.0;

/// Human size: `812 B`, `2 KB`, `1,5 MB` (decimal comma, binary units as file managers show).
pub(super) fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if value >= 10.0 || (value - value.round()).abs() < 0.05 {
        format!("{}", value.round() as u64)
    } else {
        format!("{value:.1}").replace('.', ",")
    };
    format!("{text} {}", UNITS[unit])
}

fn item_subtitle(item: &SharedItem) -> String {
    let when = format!(
        "{} · {}",
        super::date_label(item.sent_at_ms),
        super::clock_label(item.sent_at_ms)
    );
    match item.kind {
        SharedKind::Image => when,
        SharedKind::File => {
            let mut parts = Vec::new();
            if !item.extension.is_empty() {
                parts.push(item.extension.to_uppercase());
            }
            if let Some(size) = item.size_bytes {
                parts.push(size_label(size));
            }
            parts.push(when);
            parts.join(" · ")
        }
        SharedKind::Link => {
            if item.host.is_empty() {
                when
            } else {
                format!("{} · {}", item.host, super::date_label(item.sent_at_ms))
            }
        }
    }
}

fn item_title(item: &SharedItem) -> String {
    if !item.title.is_empty() {
        return item.title.clone();
    }
    match item.kind {
        SharedKind::Image => "Ảnh".into(),
        SharedKind::File => "Tệp đính kèm".into(),
        SharedKind::Link => {
            if item.host.is_empty() {
                "Liên kết".into()
            } else {
                item.host.clone()
            }
        }
    }
}

/// A whole row is one hit target; text is painted inside it, like the conversation list.
fn shared_row(ui: &mut egui::Ui, item: &SharedItem, width: f32) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let clickable = item.url.is_some();
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, ROW_HEIGHT),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if clickable && (response.hovered() || response.is_pointer_button_down_on()) {
        ui.painter().rect_filled(
            rect,
            12.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else {
                colors.hover
            },
        );
    }
    visuals::focus_outline(ui, &response, rect, 12.0);
    let badge = egui::Rect::from_min_size(rect.min + vec2(8.0, 10.0), vec2(40.0, 40.0));
    ui.painter().rect_filled(badge, 10.0, colors.accent_soft);
    visuals::source_icon(
        ui,
        egui::Rect::from_center_size(badge.center(), vec2(22.0, 22.0)),
        match item.kind {
            SharedKind::Image => "fa-icon-outline-photo",
            SharedKind::File => "fa-chatbar-file",
            SharedKind::Link => "fa-icon-outline-link",
        },
        colors.accent_ink,
    );
    let text_left = rect.left() + 60.0;
    let text_width = (rect.right() - text_left - 8.0).max(0.0);
    super::row_text(
        ui,
        egui::pos2(text_left, rect.top() + 11.0),
        text_width,
        &item_title(item),
        FontId::new(
            visuals::TEXT_BODY,
            egui::FontFamily::Name("Zalo Semibold".into()),
        ),
        colors.text_primary,
    );
    super::row_text(
        ui,
        egui::pos2(text_left, rect.top() + 33.0),
        text_width,
        &item_subtitle(item),
        FontId::proportional(visuals::TEXT_SMALL),
        colors.text_secondary,
    );
    let hint = match (&item.url, item.kind) {
        (Some(_), SharedKind::File) => "Mở tệp trong trình duyệt · chỉ mở khi bạn bấm",
        (Some(_), SharedKind::Image) => "Mở ảnh gốc trong trình duyệt · chỉ mở khi bạn bấm",
        (Some(_), SharedKind::Link) => "Mở liên kết trong trình duyệt · chỉ mở khi bạn bấm",
        (None, SharedKind::File) => "Tệp này không có liên kết mở được (hoặc đã được mã hóa)",
        (None, _) => "Mục này không có liên kết mở được",
    };
    response.on_hover_text(hint)
}

/// Quick action tile: glyph over a label. A disabled tile keeps its reason in the hover text.
fn tile(
    ui: &mut egui::Ui,
    width: f32,
    glyph: &str,
    label: &str,
    enabled: bool,
    hover: &str,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(vec2(width, TILE_HEIGHT), Sense::click())
        })
        .inner;
    let fill = if enabled && response.is_pointer_button_down_on() {
        colors.pressed
    } else if enabled && response.hovered() {
        colors.hover
    } else {
        colors.canvas
    };
    ui.painter().rect_filled(rect, 14.0, fill);
    let ink = if enabled {
        colors.text_primary
    } else {
        colors.text_disabled
    };
    visuals::source_icon(
        ui,
        egui::Rect::from_center_size(rect.center_top() + vec2(0.0, 24.0), vec2(24.0, 24.0)),
        glyph,
        ink,
    );
    ui.painter().text(
        rect.center_bottom() - vec2(0.0, 14.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(visuals::TEXT_SMALL),
        ink,
    );
    visuals::focus_outline(ui, &response, rect, 14.0);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(hover).on_disabled_hover_text(hover)
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_space(14.0);
    ui.label(
        RichText::new(text)
            .size(visuals::TEXT_BODY)
            .family(egui::FontFamily::Name("Zalo Semibold".into()))
            .color(colors.text_primary),
    );
    ui.add_space(6.0);
}

fn fact(ui: &mut egui::Ui, label: &str, value: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(visuals::TEXT_SMALL)
                .color(colors.text_secondary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).size(visuals::TEXT_BODY));
        });
    });
}

impl NativeApp {
    /// Ask the worker for the saved-history summary when the panel needs one. At most one
    /// request is in flight; a full queue is retried shortly instead of every frame.
    fn refresh_overview(&mut self, ctx: &egui::Context, thread: &str) {
        if !self.model.overview_stale(thread) {
            return;
        }
        let owned = thread.to_owned();
        #[cfg(feature = "messaging")]
        if let Some(worker) = self.live.as_ref().and_then(|state| state.worker.as_ref()) {
            let _ = self.model.request_overview(thread, |generation| {
                worker.submit(crate::app::live::LiveCommand::Overview {
                    generation,
                    thread: owned,
                })
            });
            if self.model.overview_stale(thread) {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            return;
        }
        if let Some(worker) = &self.worker {
            let _ = self.model.request_overview(thread, |generation| {
                worker.submit(crate::app::Command::Overview {
                    generation,
                    thread: owned,
                })
            });
            if self.model.overview_stale(thread) {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    pub(super) fn information_body(&mut self, ui: &mut egui::Ui, thread: &Conversation) {
        let colors = crate::theme::palette(ui.ctx());
        self.refresh_overview(&ui.ctx().clone(), &thread.id);
        let title = display_title(thread);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            self.profile_avatar(ui, rect, &title, &thread.id);
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(&title)
                        .family(egui::FontFamily::Name("Zalo Semibold".into()))
                        .size(visuals::TEXT_HEADING),
                )
                .wrap(),
            );
            ui.label(
                RichText::new(if self.demo {
                    "Hội thoại minh họa"
                } else if thread.id.starts_with("g:") {
                    "Nhóm trò chuyện"
                } else {
                    "Trò chuyện riêng"
                })
                .color(colors.text_secondary),
            );
        });

        // Quick actions. Mute and pin are stored on Zalo's servers and sync across devices, so
        // they stay disabled with that reason until the native client can read and write them.
        ui.add_space(8.0);
        let gap = ui.spacing().item_spacing.x;
        let width = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
        ui.horizontal(|ui| {
            tile(
                ui,
                width,
                "fa-mute-outline",
                "Tắt thông báo",
                false,
                "Chưa hỗ trợ: cài đặt thông báo nằm trên máy chủ Zalo và đồng bộ giữa các thiết bị",
            );
            tile(
                ui,
                width,
                "fa-outline-pin",
                "Ghim",
                false,
                "Chưa hỗ trợ: ghim hội thoại nằm trên máy chủ Zalo và đồng bộ giữa các thiết bị",
            );
            if tile(
                ui,
                width,
                "fa-outline-copy",
                "Sao chép tên",
                true,
                "Sao chép tên hội thoại vào bộ nhớ tạm",
            )
            .clicked()
            {
                ui.ctx().copy_text(title.clone());
            }
        });

        let overview = self.model.overview(&thread.id).cloned();
        section_title(ui, "Thông tin đã lưu trên máy");
        match &overview {
            None => {
                ui.label(
                    RichText::new(if self.model.overview_pending(&thread.id) {
                        "Đang đọc lịch sử đã lưu…"
                    } else {
                        "Chưa đọc được lịch sử đã lưu của hội thoại này."
                    })
                    .size(visuals::TEXT_SMALL)
                    .color(colors.text_secondary),
                );
            }
            Some(overview) => {
                fact(ui, "Tin nhắn", &overview.total_messages.to_string());
                if let (Some(first), Some(last)) = (overview.first_at_ms, overview.last_at_ms) {
                    fact(ui, "Từ", &super::date_label(first));
                    fact(ui, "Đến", &super::date_label(last));
                }
                if overview.sticker_count > 0 {
                    fact(ui, "Nhãn dán", &overview.sticker_count.to_string());
                }
            }
        }

        section_title(ui, "Ảnh, tệp và liên kết đã chia sẻ");
        let counts = overview
            .as_ref()
            .map_or((0, 0, 0), |o| (o.image_count, o.file_count, o.link_count));
        ui.horizontal_wrapped(|ui| {
            for (tab, label, count) in [
                (InfoTab::Images, "Ảnh", counts.0),
                (InfoTab::Files, "Tệp", counts.1),
                (InfoTab::Links, "Liên kết", counts.2),
            ] {
                if visuals::chip(
                    ui,
                    &format!("{label} {count}"),
                    self.info_tab == tab,
                    true,
                    label,
                )
                .clicked()
                {
                    self.info_tab = tab;
                }
            }
        });
        let Some(overview) = overview else {
            return;
        };
        let (items, total, empty) = match self.info_tab {
            InfoTab::Images => (
                &overview.images,
                overview.image_count,
                "Chưa có ảnh nào trong lịch sử đã lưu.",
            ),
            InfoTab::Files => (
                &overview.files,
                overview.file_count,
                "Chưa có tệp nào trong lịch sử đã lưu.",
            ),
            InfoTab::Links => (
                &overview.links,
                overview.link_count,
                "Chưa có liên kết nào trong lịch sử đã lưu.",
            ),
        };
        ui.add_space(4.0);
        if items.is_empty() {
            ui.label(
                RichText::new(empty)
                    .size(visuals::TEXT_SMALL)
                    .color(colors.text_secondary),
            );
            return;
        }
        let width = ui.available_width();
        ui.spacing_mut().item_spacing.y = 2.0;
        for item in items {
            if shared_row(ui, item, width).clicked() {
                if let Some(url) = &item.url {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        }
        if total as usize > items.len() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Hiển thị {} mục gần nhất trong {} mục đã lưu.",
                    items.len().min(OVERVIEW_ITEM_LIMIT),
                    total
                ))
                .size(visuals::TEXT_SMALL)
                .color(colors.text_secondary),
            );
        }
    }
}

#[cfg(test)]
mod size_tests {
    use super::size_label;

    #[test]
    fn sizes_use_binary_units_and_a_decimal_comma() {
        assert_eq!(size_label(0), "0 B");
        assert_eq!(size_label(812), "812 B");
        assert_eq!(size_label(1024), "1 KB");
        assert_eq!(size_label(2048), "2 KB");
        assert_eq!(size_label(1536), "1,5 KB");
        assert_eq!(size_label(5 * 1024 * 1024), "5 MB");
        assert_eq!(size_label(1_572_864), "1,5 MB");
        assert_eq!(size_label(12 * 1024 * 1024 + 300_000), "12 MB");
        assert_eq!(size_label(3 * 1024 * 1024 * 1024), "3 GB");
        assert!(size_label(u64::MAX).ends_with(" GB"));
    }
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::domain::ConversationOverview;

    fn item(kind: SharedKind, id: &str, title: &str, url: Option<&str>, at: i64) -> SharedItem {
        SharedItem {
            message_id: id.into(),
            sender: "22".into(),
            sent_at_ms: at,
            kind,
            title: title.into(),
            extension: if kind == SharedKind::File { "pdf" } else { "" }.into(),
            size_bytes: (kind == SharedKind::File).then_some(2048),
            host: url
                .filter(|_| kind == SharedKind::Link)
                .and_then(|u| u.split("//").nth(1))
                .and_then(|rest| rest.split('/').next())
                .unwrap_or("")
                .into(),
            url: url.map(str::to_owned),
            thumbnail_url: None,
        }
    }
    fn overview() -> ConversationOverview {
        ConversationOverview {
            total_messages: 8,
            first_at_ms: Some(1_700_000_000_000),
            last_at_ms: Some(1_700_100_000_000),
            sticker_count: 0,
            image_count: 2,
            file_count: 1,
            link_count: 2,
            images: vec![
                item(
                    SharedKind::Image,
                    "p2",
                    "Chú thích ảnh hai",
                    Some("https://photo.example.com/2.jpg"),
                    1_700_000_300_000,
                ),
                item(SharedKind::Image, "p1", "", None, 1_700_000_200_000),
            ],
            files: vec![item(
                SharedKind::File,
                "f1",
                "bao-cao-quy-4.pdf",
                Some("https://files.example.com/bao-cao.pdf"),
                1_700_000_100_000,
            )],
            links: vec![
                item(
                    SharedKind::Link,
                    "t3",
                    "",
                    Some("https://example.com/huong-dan"),
                    1_700_000_400_000,
                ),
                item(
                    SharedKind::Link,
                    "r1",
                    "Bài viết hay",
                    Some("https://news.example.com/a"),
                    1_700_000_350_000,
                ),
            ],
        }
    }

    struct Panel {
        ctx: egui::Context,
        app: NativeApp,
        root: std::path::PathBuf,
    }
    impl Panel {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let (mut app, root) = super::super::live::tests::fixture(&ctx);
            app.live = None;
            app.demo = true;
            app.model.generation = 5;
            app.model.conversations = vec![Conversation {
                id: "d:22".into(),
                title: "Hội thoại A".into(),
                last_message_at_ms: 1_700_100_000_000,
                last_text: "xin chào".into(),
                last_sender: "22".into(),
            }];
            app.model.selected = Some("d:22".into());
            app.drafts.select("d:22").unwrap();
            app.conversation_details = Some(("0".into(), "d:22".into()));
            // Deliver a summary the way a worker reply does.
            app.model.request_overview("d:22", |_| Ok(())).unwrap();
            assert!(app.model.apply_overview(5, "d:22", overview()));
            Self { ctx, app, root }
        }
        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(1280.0, 900.0),
                )),
                events,
                focused: true,
                ..Default::default()
            };
            self.ctx.run(input, |ctx| self.app.workspace(ctx))
        }
        fn settle(&mut self) -> egui::FullOutput {
            let _ = self.frame(vec![]);
            self.frame(vec![])
        }
        fn tap(&mut self, at: egui::Pos2) -> egui::FullOutput {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let _ = self.frame(vec![egui::Event::PointerMoved(at), button(true)]);
            let out = self.frame(vec![button(false)]);
            let _ = self.frame(vec![]);
            out
        }
    }
    impl Drop for Panel {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }
    fn center_of(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing text {label:?}"))
    }

    #[test]
    fn panel_shows_the_saved_history_summary_and_the_image_tab_by_default() {
        let mut panel = Panel::new();
        let output = panel.settle();
        let all = texts(&output);
        for expected in [
            "Thông tin đã lưu trên máy",
            "Ảnh, tệp và liên kết đã chia sẻ",
            "Ảnh 2",
            "Tệp 1",
            "Liên kết 2",
            "Chú thích ảnh hai",
            // An image without a caption still gets a readable title.
            "Ảnh",
            "Sao chép tên",
            "Tắt thông báo",
            "Ghim",
        ] {
            assert!(
                all.iter().any(|t| t == expected),
                "missing {expected:?} in {all:?}"
            );
        }
        assert!(
            all.iter().any(|t| t == "8"),
            "the stored message count is shown"
        );
        // Other tabs' rows are not drawn until selected.
        assert!(!all.iter().any(|t| t.starts_with("bao-cao")));
        assert!(!all.iter().any(|t| t == "Bài viết hay"));
    }

    #[test]
    fn switching_tabs_lists_files_with_extension_and_size_and_links_with_their_host() {
        let mut panel = Panel::new();
        let output = panel.settle();
        let output = panel.tap(center_of(&output, "Tệp 1"));
        let all = texts(&output);
        assert!(all.iter().any(|t| t == "bao-cao-quy-4.pdf"), "{all:?}");
        assert!(
            all.iter().any(|t| t.starts_with("PDF · 2 KB · ")),
            "type, size and date: {all:?}"
        );
        let output = panel.tap(center_of(&output, "Liên kết 2"));
        let all = texts(&output);
        assert!(all.iter().any(|t| t == "Bài viết hay"), "{all:?}");
        // A link with no title shows its host as the title.
        assert!(all.iter().any(|t| t == "example.com"), "{all:?}");
        assert!(
            all.iter().any(|t| t.starts_with("news.example.com · ")),
            "{all:?}"
        );
    }

    #[test]
    fn a_row_opens_its_address_only_when_clicked_and_only_if_it_has_one() {
        let mut panel = Panel::new();
        let output = panel.settle();
        assert!(
            output.platform_output.open_url.is_none(),
            "nothing opens by itself"
        );
        let output = panel.tap(center_of(&output, "Chú thích ảnh hai"));
        let opened = output.platform_output.open_url.map(|open| open.url);
        assert_eq!(opened.as_deref(), Some("https://photo.example.com/2.jpg"));
        // The caption-less image has no address: tapping it opens nothing.
        let output = panel.settle();
        let output = panel.tap(center_of(&output, "Ảnh"));
        assert!(output.platform_output.open_url.is_none());
    }

    #[test]
    fn quick_actions_copy_the_name_and_keep_unsupported_ones_disabled() {
        let mut panel = Panel::new();
        let output = panel.settle();
        let output = panel.tap(center_of(&output, "Tắt thông báo"));
        assert!(output.platform_output.copied_text.is_empty());
        let output = panel.tap(center_of(&output, "Ghim"));
        assert!(output.platform_output.copied_text.is_empty());
        let output = panel.settle();
        let output = panel.tap(center_of(&output, "Sao chép tên"));
        assert_eq!(output.platform_output.copied_text, "Hội thoại A");
    }

    #[test]
    fn without_a_summary_the_panel_says_what_is_happening_and_never_invents_numbers() {
        let mut panel = Panel::new();
        panel.app.model.generation += 1; // a new selection drops the old summary
        let all = texts(&panel.settle());
        assert!(!all.iter().any(|t| t == "Chú thích ảnh hai"));
        assert!(
            all.iter().any(|t| t == "Đang đọc lịch sử đã lưu…")
                || all.iter().any(|t| t.starts_with("Chưa đọc được")),
            "{all:?}"
        );
        assert!(
            all.iter().any(|t| t == "Ảnh 0"),
            "counts fall back to zero, not stale values"
        );
    }
}
