//! Native Zalo theme roles, original token/icon catalogs, and asynchronous profile preferences.
pub mod catalog;
mod preferences;
pub use preferences::ThemeController;

use eframe::egui::{self, Color32, Id, Stroke};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
    System,
    Custom,
}

pub const BACKGROUND_TOKENS: [&str; 3] = [
    "--surface-background",
    "--layer-background-navbar-normal",
    "--layer-background",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    pub angle_degrees: f32,
    pub start: (f32, Color32),
    pub end: (f32, Color32),
}
/// The exact two-hex-stop grammar used by the original palette. This is not a CSS parser.
pub fn parse_gradient(raw: &str) -> Option<LinearGradient> {
    if raw.len() > 256 {
        return None;
    }
    let inner = raw
        .trim()
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?;
    let parts: Vec<_> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let angle = parts[0].strip_suffix("deg")?.trim().parse::<f32>().ok()?;
    if !angle.is_finite() {
        return None;
    }
    let stop = |raw: &str, default| -> Option<(f32, Color32)> {
        let parts: Vec<_> = raw.split_whitespace().collect();
        if parts.is_empty() || parts.len() > 2 || !parts[0].starts_with('#') {
            return None;
        }
        let color = parse_color(parts[0])?;
        let position = if parts.len() == 2 {
            parts[1].strip_suffix('%')?.parse::<f32>().ok()? / 100.0
        } else {
            default
        };
        if !position.is_finite() || !(0.0..=1.0).contains(&position) {
            return None;
        }
        Some((position, color))
    };
    let start = stop(parts[1], 0.0)?;
    let end = stop(parts[2], 1.0)?;
    if start.0 >= end.0 {
        return None;
    }
    Some(LinearGradient {
        angle_degrees: angle.rem_euclid(360.0),
        start,
        end,
    })
}
pub fn background(ctx: &egui::Context, token: &str) -> Option<LinearGradient> {
    let index = BACKGROUND_TOKENS
        .iter()
        .position(|candidate| *candidate == token)?;
    ctx.data(|data| data.get_temp::<[Option<LinearGradient>; 3]>(Id::new("zalo-native-gradients")))?
        [index]
}

impl ThemeMode {
    pub fn effective_dark(self, system_dark: bool) -> bool {
        self == Self::Dark || self == Self::System && system_dark
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub primary: Color32,
    pub nav_background: Color32,
    pub nav_selected: Color32,
    pub nav_hover: Color32,
    pub nav_pressed: Color32,
    pub nav_text: Color32,
    pub nav_text_disabled: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_placeholder: Color32,
    pub text_disabled: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub curtain: Color32,
    pub canvas: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selected: Color32,
    pub bubble_own: Color32,
    pub bubble_incoming: Color32,
    pub button_primary_hover: Color32,
    pub button_primary_pressed: Color32,
    pub button_primary_disabled: Color32,
    pub button_primary_text: Color32,
    pub focus: Color32,
    pub error: Color32,
    pub date_background: Color32,
    pub date_text: Color32,
    pub avatar_accents: [Color32; 5],
    pub on_color: Color32,
    pub status_online: Color32,
    pub status_connecting: Color32,
    pub status_offline: Color32,
    pub status_ring: Color32,
}

impl Palette {
    pub fn from_mode(
        mode: ThemeMode,
        system_dark: bool,
        overrides: &BTreeMap<String, String>,
    ) -> Self {
        let dark = mode.effective_dark(system_dark);
        let custom = if mode == ThemeMode::Custom {
            Some(overrides)
        } else {
            None
        };
        let color = |name| {
            resolve_color(name, dark, custom, 0)
                .unwrap_or_else(|| panic!("missing original semantic color: {name}"))
        };
        Self {
            dark,
            primary: color("--button-primary-normal"),
            nav_background: color("--layer-background-navbar-normal"),
            nav_selected: color("--layer-background-leftmenu-selected"),
            nav_hover: color("--layer-background-navbar-hover"),
            nav_pressed: color("--layer-background-navbar-selected"),
            nav_text: color("--icon-navbar-normal"),
            nav_text_disabled: color("--icon-navbar-disabled"),
            text_primary: color("--text-primary"),
            text_secondary: color("--text-secondary"),
            text_placeholder: color("--text-placeholder"),
            text_disabled: color("--text-disabled"),
            border: color("--border"),
            divider: color("--divider"),
            curtain: color("--curtain"),
            canvas: color("--surface-background"),
            surface: color("--layer-background"),
            hover: color("--layer-background-hover"),
            pressed: color("--button-secondary-neutral-pressed"),
            selected: color("--layer-background-selected"),
            bubble_own: color("--blue-message"),
            bubble_incoming: color("--white-message"),
            button_primary_hover: color("--button-primary-hover"),
            button_primary_pressed: color("--button-primary-pressed"),
            button_primary_disabled: color("--button-primary-disabled"),
            button_primary_text: color("--button-primary-text"),
            focus: color("--border-focused"),
            error: color("--text-errors"),
            date_background: color("--bg-message-info"),
            date_text: color("--text-secondary"),
            avatar_accents: [
                color("--accent-blue-bg"),
                color("--accent-green-bg"),
                color("--accent-orange-bg"),
                color("--accent-purple-bg"),
                color("--accent-stealblue-bg"),
            ],
            on_color: color("--text-on-color"),
            // Original .avatar-online-dot:after uses G60 with a WA100 ring.
            status_online: color("--G60"),
            status_connecting: color("--icon-warning"),
            status_offline: color("--grey-dot"),
            status_ring: color("--WA100"),
        }
    }
}

pub fn palette(ctx: &egui::Context) -> Palette {
    static LIGHT: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();
    ctx.data(|data| data.get_temp::<Palette>(Id::new("zalo-native-palette")))
        .unwrap_or_else(|| {
            *LIGHT.get_or_init(|| Palette::from_mode(ThemeMode::Light, false, &BTreeMap::new()))
        })
}
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    apply_custom(ctx, mode, &BTreeMap::new());
}
fn apply_custom(ctx: &egui::Context, mode: ThemeMode, overrides: &BTreeMap<String, String>) {
    let system_dark = ctx.system_theme() == Some(egui::Theme::Dark);
    let colors = Palette::from_mode(mode, system_dark, overrides);
    let gradients = BACKGROUND_TOKENS.map(|token| {
        if mode == ThemeMode::Custom {
            overrides.get(token).and_then(|raw| parse_gradient(raw))
        } else {
            None
        }
    });
    let mut style = (*ctx.style()).clone();
    style.visuals = if colors.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let visuals = &mut style.visuals;
    visuals.override_text_color = Some(colors.text_primary);
    visuals.panel_fill = colors.surface;
    visuals.window_fill = colors.surface;
    visuals.extreme_bg_color = colors.surface;
    visuals.faint_bg_color = colors.hover;
    visuals.hyperlink_color = colors.primary;
    visuals.error_fg_color = colors.error;
    visuals.selection.bg_fill = colors.selected;
    visuals.selection.stroke = Stroke::new(1.0_f32, colors.focus);
    for (widget, background, text, border) in [
        (
            &mut visuals.widgets.noninteractive,
            colors.surface,
            colors.text_disabled,
            colors.border,
        ),
        (
            &mut visuals.widgets.inactive,
            colors.surface,
            colors.text_primary,
            colors.border,
        ),
        (
            &mut visuals.widgets.hovered,
            colors.hover,
            colors.text_primary,
            colors.border,
        ),
        (
            &mut visuals.widgets.active,
            colors.pressed,
            colors.text_primary,
            colors.focus,
        ),
        (
            &mut visuals.widgets.open,
            colors.selected,
            colors.text_primary,
            colors.focus,
        ),
    ] {
        widget.bg_fill = background;
        widget.weak_bg_fill = background;
        widget.fg_stroke = Stroke::new(1.0_f32, text);
        widget.bg_stroke = Stroke::new(1.0_f32, border);
        widget.rounding = egui::Rounding::same(5.0_f32);
    }
    ctx.set_style(style);
    ctx.data_mut(|data| data.insert_temp(Id::new("zalo-native-palette"), colors));
    ctx.data_mut(|data| data.insert_temp(Id::new("zalo-native-gradients"), gradients));
}

fn resolve_color(
    name: &str,
    dark: bool,
    custom: Option<&BTreeMap<String, String>>,
    depth: u8,
) -> Option<Color32> {
    if depth > 12 {
        return None;
    }
    let token = catalog::token(name)?;
    let raw = custom
        .and_then(|m| m.get(name))
        .map(String::as_str)
        .unwrap_or(if dark { &token.dark } else { &token.light });
    if let Some(reference) = raw.strip_prefix("var(").and_then(|s| s.strip_suffix(')')) {
        return resolve_color(reference.trim(), dark, custom, depth + 1);
    }
    parse_color(raw).or_else(|| {
        // A gradient is drawn separately by the native background painter.
        // Its source solid remains the text/layout fallback; no endpoint is
        // falsely represented as the applied gradient.
        if custom.is_some() && BACKGROUND_TOKENS.contains(&name) && parse_gradient(raw).is_some() {
            resolve_color(name, dark, None, depth + 1)
        } else {
            None
        }
    })
}

pub fn parse_color(raw: &str) -> Option<Color32> {
    let raw = raw.trim().trim_end_matches("!important").trim();
    if let Some(hex) = raw.strip_prefix('#') {
        let expanded = match hex.len() {
            3 | 4 => hex.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
            6 | 8 => hex.into(),
            _ => return None,
        };
        let number = u32::from_str_radix(&expanded, 16).ok()?;
        return Some(if expanded.len() == 6 {
            Color32::from_rgb((number >> 16) as u8, (number >> 8) as u8, number as u8)
        } else {
            Color32::from_rgba_unmultiplied(
                (number >> 24) as u8,
                (number >> 16) as u8,
                (number >> 8) as u8,
                number as u8,
            )
        });
    }
    if raw == "transparent" {
        return Some(Color32::TRANSPARENT);
    }
    let (function, values) = raw.split_once('(')?;
    let values: Vec<&str> = values
        .strip_suffix(')')?
        .split(',')
        .map(str::trim)
        .collect();
    if values.len() != 3 && values.len() != 4 {
        return None;
    }
    let alpha = if values.len() == 4 {
        values[3].parse::<f32>().ok()?
    } else {
        1.0
    };
    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return None;
    }
    let rgb = match function {
        "rgb" | "rgba" => {
            let mut rgb = [0.; 3];
            for (i, value) in values[..3].iter().enumerate() {
                rgb[i] = if let Some(percent) = value.strip_suffix('%') {
                    percent.parse::<f32>().ok()? / 100.0
                } else {
                    value.parse::<f32>().ok()? / 255.0
                };
            }
            rgb
        }
        "hsl" | "hsla" => {
            let h = values[0].parse::<f32>().ok()?.rem_euclid(360.0) / 360.0;
            let s = values[1].strip_suffix('%')?.parse::<f32>().ok()? / 100.0;
            let l = values[2].strip_suffix('%')?.parse::<f32>().ok()? / 100.0;
            let a = s * l.min(1.0 - l);
            [0., 8., 4.].map(|n| {
                let k = (n + h * 12.0) % 12.0;
                l - a * (-1.0_f32).max((k - 3.0).min(9.0 - k).min(1.0))
            })
        }
        _ => return None,
    };
    if rgb
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return None;
    }
    Some(Color32::from_rgba_unmultiplied(
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8,
        (alpha * 255.0).round() as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_palettes_and_all_control_states() {
        let light = Palette::from_mode(ThemeMode::Light, false, &BTreeMap::new());
        let dark = Palette::from_mode(ThemeMode::Dark, false, &BTreeMap::new());
        assert_eq!(light.primary, Color32::from_rgb(0, 104, 255));
        assert_eq!(dark.canvas, Color32::from_rgb(18, 20, 22));
        assert_eq!(dark.text_primary, Color32::from_rgb(223, 226, 231));
        assert_ne!(light.button_primary_hover, light.button_primary_pressed);
        assert_ne!(dark.button_primary_disabled, dark.primary);
        assert_eq!(
            Palette::from_mode(ThemeMode::System, true, &BTreeMap::new()),
            dark
        );
        assert_eq!(
            Palette::from_mode(ThemeMode::System, false, &BTreeMap::new()),
            light
        );
        assert_eq!(
            Palette::from_mode(ThemeMode::Custom, true, &BTreeMap::new()),
            light
        );
    }
    #[test]
    fn custom_overlay_changes_only_requested_tokens_and_reset_inherits_original() {
        let mut overrides = BTreeMap::new();
        overrides.insert("--text-primary".into(), "#123456".into());
        overrides.insert("--curtain".into(), "#11223388".into());
        overrides.insert("--divider".into(), "#33445566".into());
        let custom = Palette::from_mode(ThemeMode::Custom, false, &overrides);
        assert_eq!(custom.text_primary, Color32::from_rgb(18, 52, 86));
        assert_eq!(
            custom.curtain,
            Color32::from_rgba_unmultiplied(17, 34, 51, 136)
        );
        assert_eq!(
            custom.divider,
            Color32::from_rgba_unmultiplied(51, 68, 85, 102)
        );
        assert_eq!(
            custom.canvas,
            Palette::from_mode(ThemeMode::Light, false, &BTreeMap::new()).canvas
        );
        assert_ne!(
            Palette::from_mode(ThemeMode::Dark, false, &overrides).text_primary,
            custom.text_primary
        );
    }
    #[test]
    fn css_alpha_and_invalid_values_are_not_silently_approximated() {
        assert_eq!(
            parse_color("hsla(0,0%,100%,0.1)"),
            Some(Color32::from_white_alpha(26))
        );
        assert_eq!(
            parse_color("rgba(0,0,0,0.1)"),
            Some(Color32::from_black_alpha(26))
        );
        assert!(parse_color("linear-gradient(90deg,#fff,#000)").is_none());
        assert!(parse_color("rgba(0,0,0,NaN)").is_none());
    }
    #[test]
    fn system_theme_changes_and_windows_keep_independent_cached_palettes() {
        let first = egui::Context::default();
        let second = egui::Context::default();
        first.begin_pass(egui::RawInput {
            system_theme: Some(egui::Theme::Dark),
            ..Default::default()
        });
        apply(&first, ThemeMode::System);
        let _ = first.end_pass();
        apply(&second, ThemeMode::Light);
        assert!(palette(&first).dark);
        assert!(!palette(&second).dark);
        first.begin_pass(egui::RawInput {
            system_theme: Some(egui::Theme::Light),
            ..Default::default()
        });
        apply(&first, ThemeMode::System);
        let _ = first.end_pass();
        assert_eq!(palette(&first), palette(&second));
    }
    #[test]
    fn source_gradients_preserve_stops_and_only_supported_backgrounds_apply() {
        for token in catalog::tokens() {
            for raw in &token.raw_values {
                if raw.starts_with("linear-gradient(") {
                    assert!(parse_gradient(raw).is_some(), "{}", token.name);
                }
            }
        }
        let ctx = egui::Context::default();
        let mut custom = BTreeMap::new();
        custom.insert(
            "--surface-background".into(),
            "linear-gradient(45deg,#06f 14.64%,#529aff 85.36%)".into(),
        );
        apply_custom(&ctx, ThemeMode::Custom, &custom);
        let gradient = background(&ctx, "--surface-background").unwrap();
        assert_eq!(gradient.start.0, 0.1464);
        assert_eq!(gradient.end.0, 0.8536);
        assert_eq!(
            palette(&ctx).canvas,
            Palette::from_mode(ThemeMode::Light, false, &BTreeMap::new()).canvas
        );
        apply(&ctx, ThemeMode::Dark);
        assert!(background(&ctx, "--surface-background").is_none());
        assert!(parse_gradient("linear-gradient(45deg,#fff 90%,#000 10%)").is_none());
    }
}
