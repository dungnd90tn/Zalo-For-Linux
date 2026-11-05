//! Native read-only media cards, with explicit external navigation and scoped background assets.
mod cache;
use crate::{
    app::media::MediaKey,
    domain::{MediaDescriptor, Message, PhotoDescriptor, WebContentDescriptor},
};
pub(super) use cache::MediaCache;
use cache::{Presentation, Viewer};
use eframe::egui::{self, vec2, RichText, Sense};

pub(super) fn show(
    ui: &mut egui::Ui,
    message: &Message,
    generation: u64,
    cache: Option<&mut MediaCache>,
    max_width: f32,
) {
    let Some(media) = &message.media else {
        return;
    };
    ui.set_max_width(max_width.min(350.0));
    match media {
        MediaDescriptor::Photo(photo) => {
            photo_card(ui, message, generation, photo, cache, max_width)
        }
        MediaDescriptor::WebContent(card) => {
            web_card(ui, message, generation, card, cache, max_width)
        }
    }
}
fn photo_card(
    ui: &mut egui::Ui,
    message: &Message,
    generation: u64,
    photo: &PhotoDescriptor,
    cache: Option<&mut MediaCache>,
    max_width: f32,
) {
    let colors = crate::theme::palette(ui.ctx());
    let url = photo
        .thumbnail_url
        .as_deref()
        .or(photo.original_url.as_deref());
    if let Some(url) = url {
        thumbnail(
            ui,
            message,
            generation,
            url,
            photo.width.zip(photo.height),
            cache,
            max_width.min(330.0),
            Some((&photo.caption, photo.original_url.as_deref())),
        );
    } else {
        placeholder(
            ui,
            max_width.min(280.0),
            "Ảnh chưa có địa chỉ xem trước",
            None,
        );
    }
    if !photo.title.is_empty() {
        ui.add(
            egui::Label::new(
                RichText::new(&photo.title)
                    .size(12.0)
                    .color(colors.text_secondary),
            )
            .truncate(),
        )
        .on_hover_text(&photo.title);
    }
    if !photo.caption.is_empty() {
        ui.add(egui::Label::new(&photo.caption).wrap());
    }
    if let Some(url) = photo.original_url.as_deref().and_then(external_url) {
        if ui
            .link("Mở ảnh gốc trong trình duyệt")
            .on_hover_text(
                "Mở địa chỉ ảnh gốc ngoài ứng dụng; ảnh trong chat là bản xem trước có giới hạn.",
            )
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
}
fn web_card(
    ui: &mut egui::Ui,
    message: &Message,
    generation: u64,
    card: &WebContentDescriptor,
    cache: Option<&mut MediaCache>,
    max_width: f32,
) {
    let colors = crate::theme::palette(ui.ctx());
    if let Some(url) = card.image_url.as_deref() {
        thumbnail(
            ui,
            message,
            generation,
            url,
            None,
            cache,
            max_width.min(330.0),
            None,
        );
    }
    if !card.title.is_empty() {
        ui.add(
            egui::Label::new(
                RichText::new(&card.title)
                    .family(egui::FontFamily::Name("Zalo Semibold".into()))
                    .size(14.0)
                    .color(colors.text_primary),
            )
            .wrap(),
        );
    }
    if !card.description.is_empty() {
        ui.add(egui::Label::new(&card.description).wrap());
    }
    for row in &card.rows {
        ui.horizontal_top(|ui| {
            let width = ui.available_width();
            ui.add_sized(
                [width * 0.4, 0.0],
                egui::Label::new(
                    RichText::new(&row.label)
                        .size(12.0)
                        .color(colors.text_secondary),
                )
                .wrap(),
            );
            ui.add(egui::Label::new(&row.value).wrap());
        });
    }
    for button in &card.buttons {
        let url = button.url.as_deref().and_then(external_url);
        let response = ui.add_enabled(url.is_some(), egui::Button::new(&button.label));
        if response.clicked() {
            if let Some(url) = url {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        response
            .on_hover_text("Mở liên kết trong trình duyệt")
            .on_disabled_hover_text("Thao tác này chưa được hỗ trợ trong ứng dụng native.");
    }
    if let Some(url) = card.url.as_deref().and_then(external_url) {
        let label = url::Url::parse(&url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "Mở liên kết".into());
        if ui
            .link(label)
            .on_hover_text(
                "Chỉ mở liên kết khi bạn bấm; ứng dụng không tải hay chạy nội dung trang web.",
            )
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    if (card.title.is_empty() || card.title == crate::domain::media::WEBCONTENT_PREVIEW)
        && card.description.is_empty()
        && card.rows.is_empty()
        && card.image_url.is_none()
    {
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Mẫu tương tác của tài khoản dịch vụ chưa được hiển thị trong ứng dụng native.",
                )
                .size(12.0)
                .color(colors.text_secondary),
            )
            .wrap(),
        );
    }
}
#[allow(clippy::too_many_arguments)]
fn thumbnail(
    ui: &mut egui::Ui,
    message: &Message,
    generation: u64,
    url: &str,
    dimensions: Option<(u32, u32)>,
    mut cache: Option<&mut MediaCache>,
    max_width: f32,
    viewer: Option<(&str, Option<&str>)>,
) {
    let width = max_width.min(ui.available_width()).max(40.0);
    let key = MediaKey {
        account_id: message.account_id.clone(),
        thread_id: message.thread_id.clone(),
        message_id: message.id.clone(),
        url: url.into(),
    };
    let guessed = dimensions
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| vec2(width, (width * h as f32 / w as f32).clamp(80.0, 240.0)))
        .unwrap_or(vec2(width, 160.0));
    let visible = ui.is_rect_visible(egui::Rect::from_min_size(ui.cursor().min, guessed));
    let presentation = if visible {
        cache
            .as_deref_mut()
            .map(|cache| cache.visible(ui.ctx(), &key, generation))
            .unwrap_or_else(|| {
                Presentation::Unavailable("Ảnh xem trước chỉ tải trong phiên đã đăng nhập.".into())
            })
    } else {
        Presentation::Loading
    };
    match presentation {
        Presentation::Image(texture) => {
            let source = texture.size_vec2();
            let scale = (width / source.x).min(240.0 / source.y).min(1.0);
            let mut image = egui::Image::new(&texture)
                .fit_to_exact_size(source * scale)
                .rounding(4.0);
            if viewer.is_some() {
                image = image.sense(Sense::click());
            }
            let response = ui.add(image);
            if let Some((caption, original)) = viewer {
                if response.clicked() {
                    if let Some(cache) = &mut cache {
                        cache.viewer = Some(Viewer {
                            key,
                            caption: caption.into(),
                            original_url: original.map(str::to_owned),
                        });
                    }
                }
                response.on_hover_text("Xem ảnh xem trước · không tải thêm ảnh gốc");
            }
        }
        Presentation::Loading => {
            placeholder(ui, width, "Đang tải ảnh…", Some(guessed.y));
        }
        Presentation::Unavailable(reason) => {
            placeholder(ui, width, "Chưa tải được ảnh", Some(guessed.y.min(140.0)))
                .on_hover_text(reason);
            if visible && ui.small_button("Thử tải lại").clicked() {
                if let Some(cache) = &mut cache {
                    cache.retry(&key);
                    ui.ctx().request_repaint();
                }
            }
        }
    }
}
fn placeholder(ui: &mut egui::Ui, width: f32, label: &str, height: Option<f32>) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::none()
        .fill(colors.hover)
        .rounding(4.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(20.0));
            ui.set_min_height(height.unwrap_or(100.0) - 20.0);
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                super::visuals::icon(ui, rect, super::visuals::Icon::Image, colors.text_secondary);
                ui.add(
                    egui::Label::new(RichText::new(label).size(12.0).color(colors.text_secondary))
                        .wrap(),
                );
            });
        })
        .response
}
pub(super) fn viewer(ctx: &egui::Context, cache: &mut MediaCache) {
    let Some(viewer) = cache.viewer.as_ref() else {
        return;
    };
    let key = viewer.key.clone();
    let caption = viewer.caption.clone();
    let original = viewer.original_url.as_deref().and_then(external_url);
    let texture = cache.texture(&key);
    let mut open = true;
    egui::Window::new("Ảnh xem trước")
        .id(egui::Id::new("scoped_media_viewer"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width((ctx.screen_rect().width() - 80.0).min(780.0))
        .max_size((ctx.screen_rect().size() - vec2(48.0, 64.0)).max(vec2(220.0, 180.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .vscroll(true)
        .show(ctx, |ui| {
            if let Some(texture) = texture {
                let size = texture.size_vec2();
                let max = vec2(
                    ui.available_width(),
                    (ctx.screen_rect().height() - 190.0).max(100.0),
                );
                let scale = (max.x / size.x).min(max.y / size.y);
                ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
            } else {
                ui.label("Ảnh không còn trong cache. Đóng và mở lại từ hội thoại để tải lại.");
            }
            if !caption.is_empty() {
                ui.add(egui::Label::new(caption).wrap());
            }
            ui.label(RichText::new("Bản xem trước tĩnh có giới hạn kích thước.").size(11.0));
            if let Some(url) = original {
                if ui.link("Mở ảnh gốc trong trình duyệt").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
    if !open {
        cache.viewer = None;
    }
}
fn external_url(raw: &str) -> Option<String> {
    if raw.len() > 8192 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly_card_does_not_navigate_or_execute_remote_actions() {
        let ctx = egui::Context::default();
        super::super::visuals::install(&ctx);
        let card = WebContentDescriptor {
            title: "Thông báo kiểm thử".into(),
            description: "Nội dung được đọc từ snapshot".into(),
            image_url: None,
            url: Some("https://example.invalid/synthetic".into()),
            rows: vec![],
            buttons: vec![
                crate::domain::CardButton {
                    label: "Mở tài liệu".into(),
                    url: Some("https://example.invalid/synthetic".into()),
                },
                crate::domain::CardButton {
                    label: "Thao tác chưa hỗ trợ".into(),
                    url: None,
                },
            ],
        };
        let message = Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "1".into(),
            sender: "22".into(),
            text: "[Tin nhắn chat.webcontent chưa được hỗ trợ]".into(),
            sent_at_ms: 1,
            sticker: None,
            media: Some(MediaDescriptor::WebContent(card)),
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| show(ui, &message, 1, None, 300.0));
        });
        assert!(output.platform_output.open_url.is_none());
        for url in [
            "javascript:alert(1)",
            "file:///private",
            "data:text/html,test",
            "https://u:p@example.invalid/",
            "\nhttps://example.invalid/",
        ] {
            assert!(external_url(url).is_none());
        }
        assert!(external_url("https://example.invalid/synthetic?x=1").is_some());
    }
}
