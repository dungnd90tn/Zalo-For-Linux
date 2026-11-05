//! Original Zalo typography, semantic icons, and theme-aware component states.
//! Source selectors and asset provenance are recorded in UI_THEME.md.
use eframe::egui::{self, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui};

// Source body is .875rem and message timestamps are .75rem (16px root).
// Keep metadata readable instead of shrinking individual labels below this scale.
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_BODY: f32 = 14.0;
pub const TEXT_MESSAGE: f32 = 15.0;
pub const TEXT_HEADING: f32 = 18.0;

/// Fit cached physical pixels inside a logical frame without enlarging the bitmap.
/// Window resize changes available space, never the frame's upper bound or DPI.
#[cfg(feature = "messaging")]
pub fn fit_image(
    source_pixels: egui::Vec2,
    bounds: egui::Vec2,
    pixels_per_point: f32,
) -> egui::Vec2 {
    let native_size = source_pixels / pixels_per_point;
    let scale = (bounds.x.max(0.0) / native_size.x)
        .min(bounds.y.max(0.0) / native_size.y)
        .min(1.0);
    native_size * scale
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Reuse existing repository assets directly, never a third-party client's artwork.
    fonts.font_data.insert(
        "Zalo Segoe UI".into(),
        egui::FontData::from_static(include_bytes!(
            "../../../pc-dist/fonts/Zalo-Segoe-UI.dc1a7d868a0b3a0e2daa8b36567e9ea9.ttf"
        )),
    );
    fonts.font_data.insert(
        "Zalo icons".into(),
        egui::FontData::from_static(include_bytes!(
            "../../../pc-dist/fonts/zalo-font.7f2d95cebfac22d16e486bfd1716c591.ttf"
        )),
    );
    for (name, data) in [
        ("Zalo screencap icons", include_bytes!("../../../pc-dist/fonts/zalo-pc-screencap.26b46cc0d8549b2db2c3314913a3b308.ttf").as_slice()),
        ("Zalo Semibold", include_bytes!("../../../pc-dist/fonts/Zalo-Segoe-UI-Semibold.8de26695631b05cf79e17378f6d1f134.ttf").as_slice()),
        ("Zalo Bold", include_bytes!("../../../pc-dist/fonts/Zalo-Segoe-UI-Bold.18b859523696ff3330adac79b99ef554.ttf").as_slice()),
    ] {
        fonts.font_data.insert(name.into(), egui::FontData::from_static(data));
        let mut family = vec![name.into()];
        if name != "Zalo screencap icons" {
            family.push("Zalo Segoe UI".into());
            family.extend(fonts.families[&egui::FontFamily::Proportional].clone());
        }
        fonts.families.insert(egui::FontFamily::Name(name.into()), family);
    }
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Zalo Segoe UI".into());
    fonts.families.insert(
        egui::FontFamily::Name("Zalo icons".into()),
        vec!["Zalo icons".into()],
    );
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 7.0);
    style.spacing.interact_size = vec2(32.0, 32.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(TEXT_BODY));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(TEXT_BODY));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(TEXT_SMALL));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(TEXT_HEADING));
    ctx.set_style(style);
    // Palette installation does not rebuild fonts or rasterize unused glyphs.
    crate::theme::apply(ctx, crate::theme::ThemeMode::Light);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Chat,
    Contacts,
    Cloud,
    Settings,
    Search,
    Plus,
    Phone,
    Video,
    Smile,
    Image,
    Paperclip,
    Back,
    Info,
    Check,
    Shield,
}

pub fn icon(ui: &Ui, rect: Rect, kind: Icon, color: Color32) {
    let class = match kind {
        Icon::Chat => "fa-tab-msg",
        Icon::Contacts => "fa-tab-contact",
        Icon::Cloud => "fa-cloud",
        Icon::Settings => "fa-tab-icon-setting",
        Icon::Search => "fa-outline-search",
        Icon::Plus => "fa-outline-plus",
        Icon::Phone => "fa-outline-call",
        Icon::Video => "fa-outline-video",
        Icon::Smile => "fa-chatbar-emoji",
        Icon::Image => "fa-chatbar-photo",
        Icon::Paperclip => "fa-chatbar-attach",
        Icon::Back => "fa-back",
        Icon::Info => "fa-outline-info",
        Icon::Check => "fa-check",
        Icon::Shield => "fa-icon-outline-lock",
    };
    source_icon(ui, rect, class, color);
}

/// Resolve every verified original glyph/alias through its original font family.
/// A missing source class is not replaced by an invented symbol.
pub fn source_icon(ui: &Ui, rect: Rect, class: &str, color: Color32) -> bool {
    let Some(glyph) = crate::theme::catalog::glyph(class) else {
        return false;
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        &glyph.text,
        FontId::new(rect.height().min(rect.width()), glyph.family.egui_family()),
        color,
    );
    true
}

pub fn icon_button(ui: &mut Ui, kind: Icon, label: &str, enabled: bool) -> egui::Response {
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click())
        })
        .inner;
    let colors = crate::theme::palette(ui.ctx());
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        ui.painter().rect_filled(
            rect,
            4.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else {
                colors.hover
            },
        );
    }
    icon(
        ui,
        rect.shrink(5.0),
        kind,
        if enabled {
            colors.text_primary
        } else {
            colors.text_disabled
        },
    );
    focus_outline(ui, &response, rect, 4.0);
    response.on_hover_text(label).on_disabled_hover_text(label)
}

pub fn primary_button(ui: &mut Ui, enabled: bool, text: &str, size: egui::Vec2) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(if enabled {
            colors.button_primary_text
        } else {
            colors.text_disabled
        });
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.bg_fill = colors.primary;
        widgets.inactive.weak_bg_fill = colors.primary;
        widgets.inactive.fg_stroke.color = colors.button_primary_text;
        widgets.hovered.bg_fill = colors.button_primary_hover;
        widgets.hovered.weak_bg_fill = colors.button_primary_hover;
        widgets.hovered.fg_stroke.color = colors.button_primary_text;
        widgets.active.bg_fill = colors.button_primary_pressed;
        widgets.active.weak_bg_fill = colors.button_primary_pressed;
        widgets.active.fg_stroke.color = colors.button_primary_text;
        widgets.noninteractive.bg_fill = colors.button_primary_disabled;
        widgets.noninteractive.weak_bg_fill = colors.button_primary_disabled;
        widgets.noninteractive.fg_stroke.color = colors.text_disabled;
        let response = ui.add_enabled(enabled, egui::Button::new(text).min_size(size));
        focus_outline(ui, &response, response.rect, 5.0);
        response
    })
    .inner
}

pub fn avatar(ui: &Ui, rect: Rect, title: &str) {
    // Initials represent known live names or synthetic contacts; photos are not fetched.
    // These are existing accent token values, not an additional app palette.
    let palette = crate::theme::palette(ui.ctx());
    let colors = palette.avatar_accents;
    let index = title.bytes().fold(0_usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte.into())
    }) % colors.len();
    ui.painter()
        .circle_filled(rect.center(), rect.width() / 2.0, colors[index]);
    let initials = title
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect::<String>()
        .to_uppercase();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials,
        FontId::proportional(rect.width() * 0.32),
        palette.on_color,
    );
}

pub fn divider(ui: &Ui, start: Pos2, end: Pos2) {
    ui.painter().line_segment(
        [start, end],
        Stroke::new(1.0_f32, crate::theme::palette(ui.ctx()).border),
    );
}

/// Explicit keyboard focus treatment for custom-painted interactive controls.
pub fn focus_outline(ui: &Ui, response: &egui::Response, rect: Rect, rounding: f32) {
    focus_outline_color(
        ui,
        response,
        rect,
        rounding,
        crate::theme::palette(ui.ctx()).focus,
    );
}

/// Navigation focus uses its contrasting original on-navigation foreground.
pub fn nav_focus_outline(ui: &Ui, response: &egui::Response, rect: Rect) {
    focus_outline_color(
        ui,
        response,
        rect,
        0.0,
        crate::theme::palette(ui.ctx()).nav_text,
    );
}

fn focus_outline_color(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    rounding: f32,
    color: Color32,
) {
    if response.enabled() && response.has_focus() {
        ui.painter()
            .rect_stroke(rect.shrink(1.0), rounding, Stroke::new(1.5_f32, color));
    }
}

/// Background roles accept only validated, bounded source-compatible gradients.
pub fn background(ui: &Ui, rect: Rect, token: &str, fallback: Color32) {
    ui.painter().rect_filled(rect, 0.0, fallback);
    if let Some(gradient) = crate::theme::background(ui.ctx(), token) {
        paint_gradient(ui, rect, gradient);
    }
}

pub fn paint_gradient(ui: &Ui, rect: Rect, gradient: crate::theme::LinearGradient) {
    if !rect.width().is_finite()
        || !rect.height().is_finite()
        || rect.width() <= 0.0
        || rect.height() <= 0.0
        || !gradient.angle_degrees.is_finite()
        || !gradient.start.0.is_finite()
        || !gradient.end.0.is_finite()
        || gradient.end.0 <= gradient.start.0
    {
        return;
    }
    let radians = gradient.angle_degrees.to_radians();
    let axis = egui::vec2(radians.sin(), -radians.cos());
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let span = rect.width() * axis.x.abs() + rect.height() * axis.y.abs();
    if !span.is_finite() || span <= f32::EPSILON {
        return;
    }
    let distance = |point: egui::Pos2| (point - rect.center()).dot(axis) / span + 0.5;
    let clip = |points: Vec<egui::Pos2>, boundary: f32, above: bool| {
        let mut clipped = Vec::with_capacity(6);
        if points.is_empty() {
            return clipped;
        }
        let mut previous = *points.last().unwrap();
        let mut previous_inside = if above {
            distance(previous) >= boundary
        } else {
            distance(previous) <= boundary
        };
        for current in points {
            let inside = if above {
                distance(current) >= boundary
            } else {
                distance(current) <= boundary
            };
            if inside != previous_inside {
                let fraction =
                    (boundary - distance(previous)) / (distance(current) - distance(previous));
                clipped.push(previous + (current - previous) * fraction);
            }
            if inside {
                clipped.push(current);
            }
            previous = current;
            previous_inside = inside;
        }
        clipped
    };
    let mut mesh = egui::Mesh::default();
    for (lower, upper) in [
        (0.0, gradient.start.0),
        (gradient.start.0, gradient.end.0),
        (gradient.end.0, 1.0),
    ] {
        if upper <= lower {
            continue;
        }
        let polygon = clip(clip(corners.to_vec(), lower, true), upper, false);
        let offset = mesh.vertices.len() as u32;
        for point in &polygon {
            let t = ((distance(*point) - gradient.start.0) / (gradient.end.0 - gradient.start.0))
                .clamp(0.0, 1.0);
            let blend = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t).round() as u8;
            mesh.colored_vertex(
                *point,
                egui::Color32::from_rgba_premultiplied(
                    blend(gradient.start.1.r(), gradient.end.1.r()),
                    blend(gradient.start.1.g(), gradient.end.1.g()),
                    blend(gradient.start.1.b(), gradient.end.1.b()),
                    blend(gradient.start.1.a(), gradient.end.1.a()),
                ),
            );
        }
        for i in 1..polygon.len().saturating_sub(1) {
            mesh.add_triangle(offset, offset + i as u32, offset + i as u32 + 1);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}
