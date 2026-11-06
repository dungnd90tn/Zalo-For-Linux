//! Approved redesign values for Light and Dark (see UI_REDESIGN_GUIDE.md).
//!
//! The generated catalog stays the record of the original Zalo declarations. This table is
//! layered over it when a `Palette` is resolved: a Custom override still wins, then the value
//! here, then the original token. Names starting with `--redesign-` have no original
//! counterpart and therefore cannot be edited as source tokens.

/// `(token, light, dark)`. Values are CSS colors accepted by `parse_color`.
const TABLE: &[(&str, &str, &str)] = &[
    // Canvas, surfaces and lines.
    ("--surface-background", "#f3f5f9", "#0e1420"),
    ("--layer-background", "#ffffff", "#151d2b"),
    ("--layer-background-hover", "#f3f5f9", "#1b2536"),
    ("--layer-background-selected", "#e8f1ff", "#1c2b47"),
    ("--button-secondary-neutral-pressed", "#e3e8f0", "#223049"),
    ("--border", "#e3e8f0", "#263043"),
    ("--divider", "#e3e8f0", "#263043"),
    ("--border-focused", "#0a2540", "#8db8ff"),
    // Text.
    ("--text-primary", "#121826", "#eaf0fa"),
    ("--text-secondary", "#5b6577", "#a3aec2"),
    ("--text-placeholder", "#6e7789", "#8a96ab"),
    ("--text-disabled", "#8b96a7", "#66728a"),
    // Primary action and messages.
    ("--button-primary-normal", "#0068ff", "#0068ff"),
    ("--button-primary-text", "#ffffff", "#ffffff"),
    ("--blue-message", "#0068ff", "#0068ff"),
    ("--white-message", "#ffffff", "#151d2b"),
    ("--bg-message-info", "#e3e8f0", "#263043"),
    // Navigation rail.
    ("--layer-background-navbar-normal", "#0a2540", "#070b12"),
    ("--layer-background-leftmenu-selected", "#1b3a5c", "#1b2638"),
    (
        "--layer-background-navbar-hover",
        "hsla(0,0%,100%,0.08)",
        "hsla(0,0%,100%,0.08)",
    ),
    (
        "--layer-background-navbar-selected",
        "hsla(0,0%,100%,0.14)",
        "hsla(0,0%,100%,0.14)",
    ),
    ("--icon-navbar-normal", "#b9c6d8", "#8fa0b8"),
    ("--icon-navbar-disabled", "#6f819b", "#566277"),
    // Roles the original palette did not have.
    ("--redesign-text-tertiary", "#3f4a5c", "#c3ccdc"),
    ("--redesign-border-strong", "#d5dce8", "#34415a"),
    ("--redesign-accent-soft", "#eaf2ff", "#1a2d4f"),
    ("--redesign-accent-ink", "#0050c8", "#8db8ff"),
    ("--redesign-chip-on-bg", "#0a2540", "#eaf0fa"),
    ("--redesign-chip-on-text", "#ffffff", "#0e1420"),
    // Avatar accents: the same six fills in both modes, white initials on top.
    ("--redesign-avatar-1", "#c2185b", "#c2185b"),
    ("--redesign-avatar-2", "#0068ff", "#0068ff"),
    ("--redesign-avatar-3", "#0b7a55", "#0b7a55"),
    ("--redesign-avatar-4", "#7a4bc9", "#7a4bc9"),
    ("--redesign-avatar-5", "#b4531a", "#b4531a"),
    ("--redesign-avatar-6", "#00798c", "#00798c"),
];

/// The redesign value for `name`, or `None` when the original token stands.
pub fn value(name: &str, dark: bool) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(token, _, _)| *token == name)
        .map(|(_, light, dark_value)| if dark { *dark_value } else { *light })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_value_parses_and_tokens_are_unique() {
        for (token, light, dark) in TABLE {
            assert!(super::super::parse_color(light).is_some(), "{token} light");
            assert!(super::super::parse_color(dark).is_some(), "{token} dark");
            assert_eq!(
                TABLE.iter().filter(|(other, _, _)| other == token).count(),
                1,
                "{token} listed twice"
            );
        }
    }

    #[test]
    fn overridden_tokens_exist_in_the_original_catalog() {
        for (token, _, _) in TABLE {
            if !token.starts_with("--redesign-") {
                assert!(
                    super::super::catalog::token(token).is_some(),
                    "{token} is not an original token"
                );
            }
        }
    }
}
