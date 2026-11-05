//! Generated source declarations and terminal pseudo-element glyphs verified against font cmaps.
use eframe::egui;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Token {
    pub name: String,
    pub light: String,
    pub dark: String,
    /// Every unique source value, including selector-specific values and gradients.
    /// Selector/source provenance is retained in UI_ASSET_INVENTORY.json.
    pub raw_values: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum IconFont {
    Zalo,
    Screencap,
}
impl IconFont {
    pub fn egui_family(self) -> egui::FontFamily {
        egui::FontFamily::Name(self.name().into())
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Zalo => "Zalo icons",
            Self::Screencap => "Zalo screencap icons",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Glyph {
    pub class: String,
    pub family: IconFont,
    pub codepoint: u32,
    pub glyph_index: u32,
    #[serde(default)]
    pub text: String,
}

#[derive(Deserialize)]
struct SourceCatalog {
    tokens: BTreeMap<String, Vec<String>>,
    light: BTreeMap<String, String>,
    dark: BTreeMap<String, String>,
    glyphs: Vec<Glyph>,
}
struct Catalog {
    tokens: BTreeMap<String, Token>,
    glyphs: BTreeMap<String, Glyph>,
}
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let source: SourceCatalog = serde_json::from_str(include_str!("catalog_data.json"))
            .expect("generated original UI catalog must be valid");
        Catalog {
            tokens: source
                .tokens
                .into_iter()
                .map(|(name, raw_values)| {
                    let light = source.light.get(&name).cloned().unwrap_or_default();
                    let dark = source
                        .dark
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| light.clone());
                    (
                        name.clone(),
                        Token {
                            name,
                            light,
                            dark,
                            raw_values,
                        },
                    )
                })
                .collect(),
            glyphs: source
                .glyphs
                .into_iter()
                .map(|mut glyph| {
                    glyph.text = char::from_u32(glyph.codepoint)
                        .expect("generated glyph must be a Unicode scalar")
                        .to_string();
                    (glyph.class.clone(), glyph)
                })
                .collect(),
        }
    })
}
pub fn tokens() -> impl Iterator<Item = &'static Token> {
    catalog().tokens.values()
}
pub fn token(name: &str) -> Option<&'static Token> {
    catalog().tokens.get(name)
}
pub fn glyphs() -> impl Iterator<Item = &'static Glyph> {
    catalog().glyphs.values()
}
pub fn glyph(class: &str) -> Option<&'static Glyph> {
    catalog().glyphs.get(class.trim_start_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_catalog_keeps_alias_family_and_real_font_glyphs() {
        assert!(tokens().count() > 700);
        assert!(glyphs().count() > 1000);
        for glyph in glyphs() {
            assert!(glyph.glyph_index > 0);
            assert_eq!(glyph.text.chars().count(), 1);
            assert_eq!(glyph.text.chars().next().unwrap() as u32, glyph.codepoint);
        }
        assert_eq!(glyph("fa-chatbar-emoji").unwrap().codepoint, 0xea5c);
        assert!(glyphs().any(|g| g.family == IconFont::Screencap));
        assert!(glyph("fa-nonexistent").is_none());
        assert!(token("--H_Gradient_B55B30")
            .unwrap()
            .light
            .starts_with("linear-gradient("));
    }
}
