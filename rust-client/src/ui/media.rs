//! Native read-only media cards, with explicit external navigation and scoped background assets.
mod cache;
use crate::{
    app::media::MediaKey,
    domain::{MediaDescriptor, Message, PhotoDescriptor, PollDescriptor, WebContentDescriptor},
};
pub(super) use cache::MediaCache;
use cache::{Presentation, Viewer};
use eframe::egui::{self, vec2, RichText, Sense};

const THUMBNAIL_FRAME: egui::Vec2 = vec2(330.0, 240.0);
const VIEWER_FRAME: egui::Vec2 = vec2(740.0, 540.0);
// Bounded caption (56), three gaps (24), and two footer text rows (48).
const VIEWER_FOOTER_HEIGHT: f32 = 128.0;

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
            web_card(ui, message, generation, card, cache, max_width, false)
        }
        MediaDescriptor::Recommended(card) => {
            web_card(ui, message, generation, card, cache, max_width, true)
        }
        MediaDescriptor::Poll(poll) => poll_card(ui, poll),
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
    recommended: bool,
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
    if (card.title.is_empty()
        || card.title == crate::domain::media::WEBCONTENT_PREVIEW
        || (recommended && card.title == crate::domain::media::RECOMMENDED_PREVIEW))
        && card.description.is_empty()
        && card.rows.is_empty()
        && card.image_url.is_none()
    {
        ui.add(
            egui::Label::new(
                RichText::new(if recommended {
                    "Nội dung chia sẻ này chưa có bản xem trước trong ứng dụng native."
                } else {
                    "Mẫu tương tác của tài khoản dịch vụ chưa được hiển thị trong ứng dụng native."
                })
                .size(12.0)
                .color(colors.text_secondary),
            )
            .wrap(),
        );
    }
}
fn poll_card(ui: &mut egui::Ui, poll: &PollDescriptor) {
    let colors = crate::theme::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
        super::visuals::source_icon(ui, rect, "fa-icon-outline-poll", colors.text_secondary);
        ui.add(
            egui::Label::new(
                RichText::new(&poll.title)
                    .family(egui::FontFamily::Name("Zalo Semibold".into()))
                    .color(colors.text_primary),
            )
            .wrap(),
        );
    });
    if !poll.question.is_empty() {
        ui.add(egui::Label::new(&poll.question).wrap());
    }
    if let Some(anonymous) = poll.anonymous {
        ui.label(
            RichText::new(if anonymous {
                "Bình chọn ẩn danh"
            } else {
                "Bình chọn công khai"
            })
            .size(12.0)
            .color(colors.text_secondary),
        );
    }
    ui.add_enabled(false, egui::Button::new("Bình chọn"))
        .on_disabled_hover_text("Ứng dụng native chưa hỗ trợ tham gia bình chọn.");
    ui.add(
        egui::Label::new(
            RichText::new(
                "Chưa tải các lựa chọn và kết quả. Hiện chỉ xem được nội dung thông báo bình chọn.",
            )
            .size(12.0)
            .color(colors.text_secondary),
        )
        .wrap(),
    );
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
            let size = super::visuals::fit_image(
                texture.size_vec2(),
                vec2(width.min(THUMBNAIL_FRAME.x), THUMBNAIL_FRAME.y),
                ui.ctx().pixels_per_point(),
            );
            let mut image = egui::Image::new(&texture)
                .fit_to_exact_size(size)
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
        .default_height(
            (ctx.screen_rect().height() - 100.0)
                .clamp(180.0, VIEWER_FRAME.y + VIEWER_FOOTER_HEIGHT),
        )
        .max_size((ctx.screen_rect().size() - vec2(48.0, 64.0)).max(vec2(220.0, 180.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let frame = vec2(
                ui.available_width().min(VIEWER_FRAME.x),
                (ui.available_height() - VIEWER_FOOTER_HEIGHT).clamp(0.0, VIEWER_FRAME.y),
            );
            ui.vertical_centered(|ui| {
                if let Some(texture) = texture {
                    let (rect, _) = ui.allocate_exact_size(frame, Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 4.0, crate::theme::palette(ctx).hover);
                    let size = super::visuals::fit_image(
                        texture.size_vec2(),
                        frame,
                        ctx.pixels_per_point(),
                    );
                    let image_rect = egui::Rect::from_center_size(rect.center(), size);
                    egui::Image::new(&texture).paint_at(ui, image_rect);
                } else {
                    ui.label("Ảnh không còn trong cache. Đóng và mở lại từ hội thoại để tải lại.");
                }
                if !caption.is_empty() {
                    egui::ScrollArea::vertical()
                        .id_salt("media_viewer_caption")
                        .max_height(56.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(caption).wrap());
                        });
                }
                ui.label(
                    RichText::new("Bản xem trước tĩnh có giới hạn kích thước.")
                        .size(super::visuals::TEXT_SMALL),
                );
                if let Some(url) = original {
                    if ui.link("Mở ảnh gốc trong trình duyệt").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            });
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
    fn cached_images_keep_logical_frames_on_maximize_and_do_not_upscale_pixels() {
        for pixels_per_point in [1.0, 1.25, 1.5, 2.0] {
            for (pixel_width, pixel_height) in [(1024, 768), (80, 40)] {
                for open_viewer in [false, true] {
                    let ctx = egui::Context::default();
                    super::super::visuals::install(&ctx);
                    let key = MediaKey {
                        account_id: "11".into(),
                        thread_id: "g:22".into(),
                        message_id: "1".into(),
                        url: "https://example.invalid/synthetic.png".into(),
                    };
                    let mut cache = MediaCache::default();
                    cache.seed(
                        &ctx,
                        key.clone(),
                        1,
                        pixel_width,
                        pixel_height,
                        vec![128; pixel_width as usize * pixel_height as usize * 4],
                    );
                    let texture_id = cache.texture(&key).unwrap().id();
                    if open_viewer {
                        cache.viewer = Some(Viewer {
                            key,
                            caption: "Chú thích tổng hợp đủ dài để cuộn riêng. ".repeat(30),
                            original_url: Some("https://example.invalid/original.png".into()),
                        });
                    }
                    let message = message(MediaDescriptor::Photo(PhotoDescriptor {
                        title: String::new(),
                        caption: String::new(),
                        thumbnail_url: Some("https://example.invalid/synthetic.png".into()),
                        original_url: None,
                        width: Some(pixel_width),
                        height: Some(pixel_height),
                    }));
                    let mut sizes = Vec::new();
                    let mut frame_time = 0.0;
                    for (width, height) in [
                        (1200.0, 780.0),
                        (1920.0, 1080.0),
                        (1200.0, 780.0),
                        (640.0, 480.0),
                    ] {
                        let mut rendered_size = None;
                        let mut image_center = None;
                        let mut preview_frame = None;
                        let mut original_link = None;
                        // Settle both the remembered rectangle and window fade-in before
                        // checking the actual painted frame's source color and geometry.
                        for _ in 0..3 {
                            frame_time += 1.0;
                            let mut input = egui::RawInput {
                                time: Some(frame_time),
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    vec2(width, height),
                                )),
                                ..Default::default()
                            };
                            input
                                .viewports
                                .get_mut(&egui::ViewportId::ROOT)
                                .unwrap()
                                .native_pixels_per_point = Some(pixels_per_point);
                            let output = ctx.run(input, |ctx| {
                                if open_viewer {
                                    viewer(ctx, &mut cache);
                                } else {
                                    egui::CentralPanel::default().show(ctx, |ui| {
                                        let available_width = ui.available_width();
                                        show(ui, &message, 1, Some(&mut cache), available_width);
                                    });
                                }
                            });
                            rendered_size =
                                output.shapes.iter().find_map(|shape| match &shape.shape {
                                    egui::epaint::Shape::Rect(rect)
                                        if rect.fill_texture_id == texture_id =>
                                    {
                                        image_center = Some(rect.rect.center());
                                        Some(rect.rect.size())
                                    }
                                    egui::epaint::Shape::Mesh(mesh)
                                        if mesh.texture_id == texture_id =>
                                    {
                                        Some(mesh.calc_bounds().size())
                                    }
                                    _ => None,
                                });
                            if open_viewer {
                                let colors = crate::theme::palette(&ctx);
                                preview_frame =
                                    output.shapes.iter().find_map(|shape| match &shape.shape {
                                        egui::epaint::Shape::Rect(rect)
                                            if rect.fill == colors.hover
                                                && rect.rect.width() > 100.0 =>
                                        {
                                            Some(rect.rect)
                                        }
                                        _ => None,
                                    });
                                original_link =
                                    output.shapes.iter().find_map(|shape| match &shape.shape {
                                        egui::epaint::Shape::Text(text)
                                            if text.galley.job.text
                                                == "Mở ảnh gốc trong trình duyệt" =>
                                        {
                                            Some(egui::Rect::from_min_size(
                                                text.pos,
                                                text.galley.size(),
                                            ))
                                        }
                                        _ => None,
                                    });
                            }
                        }
                        let case = format!("DPI={pixels_per_point}, source={pixel_width}x{pixel_height}, window={width}x{height}, viewer={open_viewer}");
                        let size = rendered_size.unwrap_or_else(|| {
                            panic!("cached image must actually be painted: {case}")
                        });
                        if open_viewer {
                            let frame = preview_frame.unwrap_or_else(|| {
                                panic!("viewer must paint a bounded preview frame: {case}")
                            });
                            assert!(
                                (image_center.unwrap() - frame.center()).length() < 0.01,
                                "image must be centered on both axes: {case}"
                            );
                            let link = original_link.unwrap_or_else(|| panic!("original-image action must stay visible below the caption: {case}"));
                            assert!(
                                link.top() >= frame.bottom(),
                                "original-image action must follow preview frame: {case}"
                            );
                            assert!(
                                link.bottom() <= height && link.right() <= width,
                                "original-image action must fit screen: {case}, link={link:?}"
                            );
                        }
                        let physical_size = size * pixels_per_point;
                        assert!(physical_size.x <= pixel_width as f32 + 0.01);
                        assert!(physical_size.y <= pixel_height as f32 + 0.01);
                        assert!(
                            (size.x / size.y - pixel_width as f32 / pixel_height as f32).abs()
                                < 0.001
                        );
                        sizes.push(size);
                    }
                    assert!(
                        (sizes[0] - sizes[1]).length() < 0.01,
                        "maximizing must not enlarge cached images: DPI={pixels_per_point}, source={pixel_width}x{pixel_height}, viewer={open_viewer}, sizes={sizes:?}"
                    );
                    assert!(
                        (sizes[0] - sizes[2]).length() < 0.01,
                        "restore must recover the same frame: DPI={pixels_per_point}, source={pixel_width}x{pixel_height}, viewer={open_viewer}, sizes={sizes:?}"
                    );
                    assert!(sizes[3].x <= sizes[0].x + 0.01 && sizes[3].y <= sizes[0].y + 0.01);
                }
            }
        }
    }

    fn message(media: MediaDescriptor) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            id: "1".into(),
            sender: "22".into(),
            text: media.kind().legacy_placeholder().into(),
            sent_at_ms: 1,
            sticker: None,
            media: Some(media),
        }
    }
    #[test]
    fn recommended_card_only_offers_explicit_external_navigation() {
        let ctx = egui::Context::default();
        super::super::visuals::install(&ctx);
        let message = message(MediaDescriptor::Recommended(WebContentDescriptor {
            title: "Liên kết được chia sẻ".into(),
            description: "Nội dung chỉ đọc".into(),
            image_url: None,
            url: Some("https://example.invalid/shared".into()),
            rows: vec![],
            buttons: vec![],
        }));
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| show(ui, &message, 1, None, 300.0));
        });
        assert!(output.platform_output.open_url.is_none());
    }
    #[test]
    fn poll_shows_embedded_facts_without_inventing_options_votes_or_anonymity() {
        assert!(crate::theme::catalog::glyph("fa-icon-outline-poll").is_some());
        for anonymous in [None, Some(false), Some(true)] {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let message = message(MediaDescriptor::Poll(PollDescriptor {
                title: "Đã tạo bình chọn".into(),
                question: "Chúng ta họp vào lúc nào?".into(),
                anonymous,
            }));
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, &message, 1, None, 300.0));
            });
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("Đã tạo bình chọn"));
            assert!(text.contains("Chúng ta họp vào lúc nào?"));
            assert!(text.contains("Chưa tải các lựa chọn và kết quả"));
            assert_eq!(text.contains("Bình chọn ẩn danh"), anonymous == Some(true));
            assert_eq!(
                text.contains("Bình chọn công khai"),
                anonymous == Some(false)
            );
            assert!(!text.contains("0 phiếu"));
            assert!(!text.contains("chưa được hỗ trợ]"));
            assert!(output.platform_output.open_url.is_none());
        }
    }
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
