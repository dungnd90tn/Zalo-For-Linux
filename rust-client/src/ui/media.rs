//! Native read-only media cards, with explicit external navigation and scoped background assets.
mod cache;
use crate::{
    app::media::MediaKey,
    domain::{
        FileDescriptor, MediaDescriptor, Message, PhotoDescriptor, PollDescriptor,
        WebContentDescriptor,
    },
};
pub(super) use cache::MediaCache;
use cache::{DownloadView, Presentation, Viewer};
use eframe::egui::{self, vec2, RichText, Sense};

const THUMBNAIL_FRAME: egui::Vec2 = vec2(330.0, 240.0);
const VIEWER_FRAME: egui::Vec2 = vec2(740.0, 540.0);
// Reserve room for the bounded caption, explicit save controls and external link.
const VIEWER_FOOTER_HEIGHT: f32 = 200.0;

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
        MediaDescriptor::File(file) => file_card(ui, message, file, cache),
    }
}
fn file_card(
    ui: &mut egui::Ui,
    message: &Message,
    file: &FileDescriptor,
    cache: Option<&mut MediaCache>,
) {
    let colors = crate::theme::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(38.0, 42.0), Sense::hover());
        super::visuals::source_icon(
            ui,
            rect.shrink(4.0),
            if file.is_folder {
                "fa-file-menu-icon-folder"
            } else {
                "fa-icon-outline-file"
            },
            colors.primary,
        );
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(&file.title)
                        .family(egui::FontFamily::Name("Zalo Semibold".into()))
                        .size(super::visuals::TEXT_BODY)
                        .color(colors.text_primary),
                )
                .wrap(),
            );
            let mut details = Vec::new();
            if !file.extension.is_empty() {
                details.push(file.extension.to_uppercase());
            }
            if let Some(size) = file.size_bytes {
                details.push(file_size(size));
            }
            if file.is_folder {
                details.push("Thư mục".into());
            }
            if !details.is_empty() {
                ui.label(
                    RichText::new(details.join(" · "))
                        .size(super::visuals::TEXT_SMALL)
                        .color(colors.text_secondary),
                );
            }
        });
    });
    if let (Some(cache), Some(url), false, false) =
        (cache, file.url.as_deref(), file.encrypted, file.is_folder)
    {
        let key = MediaKey {
            account_id: message.account_id.clone(),
            thread_id: message.thread_id.clone(),
            message_id: message.id.clone(),
            url: url.into(),
        };
        download_controls(ui, key, &file.title, "Tải về", cache);
    }
    if let Some(url) = file
        .url
        .as_deref()
        .filter(|_| !file.encrypted)
        .and_then(external_url)
    {
        if ui
            .link(if file.is_folder {
                "Mở thư mục trong trình duyệt"
            } else {
                "Mở tệp trong trình duyệt"
            })
            .on_hover_text("Chỉ mở liên kết khi bạn bấm. Ứng dụng không tự tải hoặc thực thi tệp.")
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    } else {
        ui.add(
            egui::Label::new(
                RichText::new(if file.encrypted {
                    "Tệp được mã hóa chưa hỗ trợ mở trong ứng dụng native."
                } else {
                    "Tin nhắn chưa có liên kết mở tệp khả dụng."
                })
                .size(super::visuals::TEXT_SMALL)
                .color(colors.text_secondary),
            )
            .wrap(),
        );
    }
}
/// Shared explicit save action for files and original photos, keyed by the full message scope.
fn download_controls(
    ui: &mut egui::Ui,
    key: MediaKey,
    name: &str,
    label: &str,
    cache: &mut MediaCache,
) {
    // Same origin policy the worker enforces; an off-CDN link only gets the browser link below.
    if crate::app::media::thumbnail_url(&key.url).is_err() {
        return;
    }
    let colors = crate::theme::palette(ui.ctx());
    let note = |ui: &mut egui::Ui, text: String| {
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .size(super::visuals::TEXT_SMALL)
                    .color(colors.text_secondary),
            )
            .wrap(),
        );
    };
    match cache.download_view(&key) {
        DownloadView::Idle => {
            if super::visuals::chip(
                ui,
                label,
                false,
                true,
                "Lưu vào thư mục Tải xuống/Zalo Native, tối đa 100 MiB. Không ghi đè, mở hay chạy tệp.",
            )
            .clicked()
            {
                cache.start_download(ui.ctx(), key, name);
            }
        }
        DownloadView::Running { received, total } => {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
            let fraction = total.map(|total| (received as f32 / total as f32).clamp(0.0, 1.0));
            let mut bar = egui::ProgressBar::new(fraction.unwrap_or(0.0));
            if fraction.is_none() {
                bar = bar.animate(true);
            }
            ui.add(bar.desired_width(ui.available_width().min(240.0)));
            note(
                ui,
                match total {
                    Some(total) => format!("{} / {}", file_size(received), file_size(total)),
                    None => file_size(received),
                },
            );
            if super::visuals::chip(ui, "Hủy", false, true, "Dừng tải và xóa phần đã tải").clicked()
            {
                cache.cancel_download(&key);
            }
        }
        DownloadView::Saved(path) => {
            note(ui, format!("Đã lưu: {}", display_path(&path)));
            let folder = path
                .parent()
                .and_then(|dir| url::Url::from_directory_path(dir).ok());
            if let Some(folder) = folder {
                if ui
                    .link("Mở thư mục chứa tệp")
                    .on_hover_text(
                        "Mở thư mục trong trình quản lý tệp. Tệp không được mở hay chạy.",
                    )
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(folder));
                }
            }
        }
        DownloadView::Failed(reason) => {
            note(ui, reason.to_string());
            if super::visuals::chip(ui, "Thử lại", false, true, "Tải lại tệp").clicked() {
                cache.forget_download(&key);
                cache.start_download(ui.ctx(), key, name);
            }
        }
    }
}
/// The extension describes the original bytes; a CDN URL without a known image extension
/// stays extensionless. Never invent JPEG, use a signed query as a name, or use the caption.
fn photo_download_name(key: &MediaKey) -> String {
    let extension = url::Url::parse(&key.url).ok().and_then(|url| {
        let leaf = url.path_segments()?.next_back()?;
        let (_, extension) = leaf.rsplit_once('.')?;
        let extension = extension.to_ascii_lowercase();
        matches!(
            extension.as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "jxl" | "avif" | "bmp" | "tif" | "tiff"
        )
        .then_some(extension)
    });
    let name = format!(
        "anh-{}{}",
        key.message_id,
        extension
            .map(|extension| format!(".{extension}"))
            .unwrap_or_default()
    );
    crate::app::download::safe_file_name(&name)
}

/// Card and viewer share the original URL's transfer; the thumbnail is never a fallback download.
fn photo_download_controls(
    ui: &mut egui::Ui,
    key: &MediaKey,
    original: Option<&str>,
    cache: &mut MediaCache,
) {
    let Some(url) = original.filter(|url| crate::app::media::thumbnail_url(url).is_ok()) else {
        return;
    };
    let original_key = MediaKey {
        url: url.into(),
        ..key.clone()
    };
    let name = photo_download_name(&original_key);
    download_controls(ui, original_key, &name, "Tải ảnh gốc", cache);
}

fn display_path(path: &std::path::Path) -> String {
    match std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| path.strip_prefix(home).ok().map(|rest| rest.to_owned()))
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
fn file_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KB", bytes as f64 / KIB as f64)
    } else if bytes < KIB * KIB * KIB {
        format!("{:.1} MB", bytes as f64 / (KIB * KIB) as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / (KIB * KIB * KIB) as f64)
    }
}
fn photo_card(
    ui: &mut egui::Ui,
    message: &Message,
    generation: u64,
    photo: &PhotoDescriptor,
    mut cache: Option<&mut MediaCache>,
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
            cache.as_deref_mut(),
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
    if let Some(cache) = cache {
        let key = MediaKey {
            account_id: message.account_id.clone(),
            thread_id: message.thread_id.clone(),
            message_id: message.id.clone(),
            url: String::new(),
        };
        photo_download_controls(ui, &key, photo.original_url.as_deref(), cache);
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
    let original_url = viewer.original_url.clone();
    let original = original_url.as_deref().and_then(external_url);
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
                egui::ScrollArea::vertical()
                    .id_salt((
                        "media_viewer_actions",
                        &key.account_id,
                        &key.thread_id,
                        &key.message_id,
                    ))
                    .max_height(VIEWER_FOOTER_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
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
                        photo_download_controls(ui, &key, original_url.as_deref(), cache);
                    });
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

    #[test]
    fn file_cards_show_metadata_and_navigate_only_after_explicit_click_on_a_valid_link() {
        assert!(crate::theme::catalog::glyph("fa-icon-outline-file").is_some());
        assert!(crate::theme::catalog::glyph("fa-file-menu-icon-folder").is_some());
        let make = |url: Option<&str>, encrypted| FileDescriptor {
            title: "Báo cáo kiểm thử với tên tài liệu dài bằng tiếng Việt.pdf".into(),
            size_bytes: Some(12_345_678),
            extension: "pdf".into(),
            url: url.map(str::to_owned),
            is_folder: false,
            encrypted,
        };
        for (url, encrypted, clickable) in [
            (Some("https://example.invalid/report.pdf"), false, true),
            (None, false, false),
            (Some("javascript:alert(1)"), false, false),
            (Some("https://example.invalid/secret.zip"), true, false),
        ] {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let file = make(url, encrypted);
            let message = message(MediaDescriptor::File(file.clone()));
            let render = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(320.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| file_card(ui, &message, &file, None));
                    },
                )
            };
            let _ = render(vec![]);
            let output = render(vec![]);
            assert!(output.platform_output.open_url.is_none());
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("PDF · 11.8 MB"));
            assert!(!text.contains("chưa được hỗ trợ]"));
            let link = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Mở tệp trong trình duyệt" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()).center())
                }
                _ => None,
            });
            assert_eq!(link.is_some(), clickable);
            if let Some(position) = link {
                let _ = render(vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ]);
                let output = render(vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }]);
                assert_eq!(output.platform_output.open_url.unwrap().url, url.unwrap());
            }
        }
    }

    #[test]
    fn photo_download_names_preserve_formats_without_caption_or_signed_query() {
        let mut key = MediaKey {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            message_id: "7".into(),
            url: String::new(),
        };
        for (url, expected) in [
            (
                "https://photo-talk.zadn.vn/a/image.JXL?token=secret&name=bad.exe",
                "anh-7.jxl",
            ),
            ("https://f1.zdn.vn/a/image.png", "anh-7.png"),
            ("https://f1.zdn.vn/a/blob?format=jpeg", "anh-7"),
            ("https://f1.zdn.vn/a/unsafe.svg", "anh-7"),
        ] {
            key.url = url.into();
            assert_eq!(photo_download_name(&key), expected);
        }
        key.message_id = "../a\u{202e}\n".into();
        let name = photo_download_name(&key);
        assert!(!name.contains(['/', '\n', '\u{202e}']));
    }

    #[test]
    fn original_photo_save_is_explicit_and_shared_between_card_and_viewer() {
        use crate::app::download::{DownloadEvent, DownloadFailure};
        for dark in [false, true] {
            for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
                let ctx = egui::Context::default();
                super::super::visuals::install(&ctx);
                crate::theme::apply(
                    &ctx,
                    if dark {
                        crate::theme::ThemeMode::Dark
                    } else {
                        crate::theme::ThemeMode::Light
                    },
                );
                let original = "https://photo-talk.zadn.vn/source/original.jxl?token=synthetic";
                let key = MediaKey {
                    account_id: "11".into(),
                    thread_id: "g:22".into(),
                    message_id: "1".into(),
                    url: "https://f1.zdn.vn/thumb.png".into(),
                };
                let original_key = MediaKey {
                    url: original.into(),
                    ..key.clone()
                };
                let photo = PhotoDescriptor {
                    title: "Ảnh tổng hợp".into(),
                    caption: "Chú thích tiếng Việt".into(),
                    thumbnail_url: Some(key.url.clone()),
                    original_url: Some(original.into()),
                    width: Some(16),
                    height: Some(16),
                };
                let message = message(MediaDescriptor::Photo(photo.clone()));
                let mut cache = MediaCache::default();
                cache.seed(&ctx, key.clone(), 1, 16, 16, vec![120; 16 * 16 * 4]);
                let render = |cache: &mut MediaCache, photo: &PhotoDescriptor, viewing| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ctx| {
                            if viewing {
                                viewer(ctx, cache);
                            } else {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    photo_card(ui, &message, 1, photo, Some(cache), 300.0)
                                });
                            }
                        },
                    )
                };
                let text = |out: egui::FullOutput| {
                    out.shapes
                        .into_iter()
                        .filter_map(|shape| match shape.shape {
                            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                let first = text(render(&mut cache, &photo, false));
                assert!(first.contains("Tải ảnh gốc"));
                assert!(
                    matches!(cache.download_view(&original_key), DownloadView::Idle),
                    "drawing must not start a transfer"
                );
                cache.apply_download(DownloadEvent::Done {
                    key: original_key.clone(),
                    path: "/tmp/Zalo Native/anh-1.jxl".into(),
                    bytes: 42,
                });
                assert!(text(render(&mut cache, &photo, false)).contains("Đã lưu:"));
                cache.viewer = Some(Viewer {
                    key: key.clone(),
                    caption: photo.caption.clone(),
                    original_url: photo.original_url.clone(),
                });
                let _ = render(&mut cache, &photo, true);
                assert!(text(render(&mut cache, &photo, true)).contains("Đã lưu:"));
                cache.apply_download(DownloadEvent::Failed {
                    key: original_key.clone(),
                    reason: DownloadFailure::Http(403),
                });
                assert!(text(render(&mut cache, &photo, false)).contains("Thử lại"));
                cache.apply_download(DownloadEvent::Failed {
                    key: original_key.clone(),
                    reason: DownloadFailure::Cancelled,
                });
                assert!(text(render(&mut cache, &photo, false)).contains("Tải ảnh gốc"));
                for url in [
                    None,
                    Some("http://f1.zdn.vn:8080/a.jpg"),
                    Some("https://example.invalid/a.jpg"),
                ] {
                    let unavailable = PhotoDescriptor {
                        original_url: url.map(str::to_owned),
                        ..photo.clone()
                    };
                    let rendered = text(render(&mut cache, &unavailable, false));
                    assert!(!rendered.contains("Tải ảnh gốc"), "{url:?}: {rendered}");
                }
            }
        }
    }

    #[test]
    fn download_button_appears_only_for_cdn_file_links_and_state_follows_worker_events() {
        use crate::app::download::{DownloadEvent, DownloadFailure};
        let render_text = |file: &FileDescriptor, cache: &mut MediaCache| {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let message = message(MediaDescriptor::File(file.clone()));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(320.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| file_card(ui, &message, file, Some(&mut *cache)));
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let file = |url: &str, encrypted, is_folder| FileDescriptor {
            title: "bao-cao.pdf".into(),
            size_bytes: Some(10),
            extension: "pdf".into(),
            url: Some(url.into()),
            is_folder,
            encrypted,
        };
        let mut cache = MediaCache::default();
        cache.scope("11", "g:22", 1);
        let cdn = file("https://f1-zpc.zdn.vn/abc/bao-cao.pdf", false, false);
        assert!(render_text(&cdn, &mut cache).contains("Tải về"));
        for hidden in [
            file("https://example.invalid/bao-cao.pdf", false, false),
            file("https://f1-zpc.zdn.vn/abc/bao-cao.pdf", true, false),
            file("https://f1-zpc.zdn.vn/abc/folder", false, true),
        ] {
            assert!(!render_text(&hidden, &mut cache).contains("Tải về"));
        }
        let key = MediaKey {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            message_id: "1".into(),
            url: "https://f1-zpc.zdn.vn/abc/bao-cao.pdf".into(),
        };
        let saved = std::path::PathBuf::from("/tmp/Zalo Native/bao-cao.pdf");
        cache.apply_download(DownloadEvent::Done {
            key: key.clone(),
            path: saved,
            bytes: 10,
        });
        assert!(matches!(cache.download_view(&key), DownloadView::Saved(_)));
        cache.apply_download(DownloadEvent::Failed {
            key: key.clone(),
            reason: DownloadFailure::Http(403),
        });
        assert!(matches!(
            cache.download_view(&key),
            DownloadView::Failed(DownloadFailure::Http(403))
        ));
        assert!(render_text(&cdn, &mut cache).contains("HTTP 403"));
        cache.apply_download(DownloadEvent::Failed {
            key: key.clone(),
            reason: DownloadFailure::Cancelled,
        });
        assert!(matches!(cache.download_view(&key), DownloadView::Idle));
    }
}
