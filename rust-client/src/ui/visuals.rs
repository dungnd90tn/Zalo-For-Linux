//! Native mapping of the repository's existing Zalo light theme and icon font.
//! Source selectors and asset provenance are recorded in UI_THEME.md.
use eframe::egui::{self, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui};

pub const BLUE: Color32 = Color32::from_rgb(0, 104, 255); // --button-primary-normal
pub const NAV_BLUE: Color32 = Color32::from_rgb(0, 90, 224); // --layer-background-navbar-normal
pub const NAV_SELECTED: Color32 = Color32::from_rgb(0, 55, 138); // --layer-background-leftmenu-selected
pub const INK: Color32 = Color32::from_rgb(8, 27, 58); // --text-primary
pub const MUTED: Color32 = Color32::from_rgb(90, 105, 129); // --text-secondary
pub const PLACEHOLDER: Color32 = Color32::from_rgb(119, 133, 156); // --text-placeholder
pub const DISABLED: Color32 = Color32::from_rgb(139, 150, 167); // --text-disabled
pub const LINE: Color32 = Color32::from_black_alpha(26); // --border rgba(0,0,0,.1)
pub const CANVAS: Color32 = Color32::from_rgb(229, 231, 235); // --surface-background
pub const HOVER: Color32 = Color32::from_rgb(241, 242, 244); // --layer-background-hover
pub const PALE_BLUE: Color32 = Color32::from_rgb(229, 241, 255); // --blue-message / --layer-background-selected

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
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = Color32::WHITE;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.extreme_bg_color = Color32::WHITE;
    style.visuals.faint_bg_color = HOVER;
    style.visuals.selection.bg_fill = PALE_BLUE;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, BLUE);
    style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(5.0);
    style.visuals.widgets.hovered.bg_fill = HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = HOVER;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.active.bg_fill = PALE_BLUE;
    style.visuals.widgets.active.weak_bg_fill = PALE_BLUE;
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 7.0);
    style.spacing.interact_size = vec2(32.0, 32.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(18.0));
    ctx.set_style(style);
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
    // Codepoints copied from the corresponding .fa-* CSS declarations, using the existing font.
    let glyph = match kind {
        Icon::Chat => '\u{ed8a}',      // .fa-tab-msg
        Icon::Contacts => '\u{ed83}',  // .fa-tab-contact
        Icon::Cloud => '\u{eaa5}',     // .fa-cloud
        Icon::Settings => '\u{ed88}',  // .fa-tab-icon-setting
        Icon::Search => '\u{eca1}',    // .fa-outline-search
        Icon::Plus => '\u{ec97}',      // .fa-outline-plus
        Icon::Phone => '\u{ec83}',     // .fa-outline-call
        Icon::Video => '\u{ecac}',     // .fa-outline-video
        Icon::Smile => '\u{ea5c}',     // .fa-chatbar-emoji
        Icon::Image => '\u{ea6a}',     // .fa-chatbar-photo
        Icon::Paperclip => '\u{ea5a}', // .fa-chatbar-attach
        Icon::Back => '\u{ea25}',      // .fa-back
        Icon::Info => '\u{ec8d}',      // .fa-outline-info
        Icon::Check => '\u{ea7e}',     // .fa-check
        Icon::Shield => '\u{ebc5}',    // .fa-icon-outline-lock
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(
            rect.height().min(rect.width()),
            egui::FontFamily::Name("Zalo icons".into()),
        ),
        color,
    );
}

pub fn icon_button(ui: &mut Ui, kind: Icon, label: &str, enabled: bool) -> egui::Response {
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click())
        })
        .inner;
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4.0, HOVER);
    }
    icon(
        ui,
        rect.shrink(5.0),
        kind,
        if enabled { INK } else { DISABLED },
    );
    response.on_hover_text(label).on_disabled_hover_text(label)
}

pub fn avatar(ui: &Ui, rect: Rect, title: &str) {
    // Synthetic contacts have initials instead of impersonating a real person's profile picture.
    // These are existing accent token values, not an additional app palette.
    let colors = [
        Color32::from_rgb(0, 104, 255),
        Color32::from_rgb(75, 195, 119),
        Color32::from_rgb(255, 105, 5),
        Color32::from_rgb(111, 63, 207),
        Color32::from_rgb(80, 127, 180),
    ];
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
        Color32::WHITE,
    );
}

pub fn divider(ui: &Ui, start: Pos2, end: Pos2) {
    ui.painter()
        .line_segment([start, end], Stroke::new(1.0_f32, LINE));
}
