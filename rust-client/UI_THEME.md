# Native UI and existing Zalo theme

The Rust view uses egui with the repository's **Light/Dark** palettes, **System**
selection and source-backed **Custom** overlays. It does
not load a WebView, redesign the theme, or use ctdozlite UI code or assets.

The source of truth is the `:root`, `.light`, `.dark` declarations and the matching
component selectors in
`pc-dist/default-login-startup-main-startup.e08d0d44f38873747a6b.css`.
`src/theme.rs` resolves semantic roles against the generated original token
catalog; `src/ui/visuals.rs` paints components. Full source coverage, registry
verification and component states are tracked in
[UI_ASSET_INVENTORY.md](UI_ASSET_INVENTORY.md). These are example **Light** values:

| Existing token | Native value |
| --- | --- |
| `--layer-background-navbar-normal` | `#005AE0` |
| `--layer-background-leftmenu-selected` | `#00378A` |
| `--button-primary-normal` | `#0068FF` |
| `--text-primary` / `--text-secondary` | `#081B3A` / `#5A6981` |
| `--text-placeholder` | `#77859C` |
| `--layer-background` / `--white-message` | `#FFFFFF` |
| `--surface-background` | `#E5E7EB` |
| `--blue-message` / `--layer-background-selected` | `#E5F1FF` |
| `--layer-background-hover` | `#F1F2F4` |
| `--border` | black at 10% opacity |

Native layout follows the same rail/list/chat structure: 56 px navigation rows,
32 px search field, 344 px default conversation panel plus the 64 px rail
(`--nav-width:408px`, `.nav-content>.nav` subtracts 64 px), rounded 6 px conversation
selection, and 68 px chat header. Small windows collapse the list behind back
navigation, preserving the selected recipient and drafts.

Existing Segoe UI regular/Semibold/Bold and the Zalo/Screencap icon TTFs are
embedded directly from `pc-dist/fonts/`. All 1,034 terminal pseudo-element
classes/aliases in the generated registry are verified against their original
font cmaps; the 15 semantic controls resolve through this registry. Font assets
retain their repository provenance. Font setup happens once, with no frame-time
file reads or eager glyph rasterization.

Open appearance settings from the bottom rail gear or login-screen gear. Four
modes are persisted independently in the native profile's `config/theme.json` by
a bounded worker, without reading Electron preferences. Custom inherits Light;
System follows the signal reported by egui/winit and falls back to Light when
absent; not every Linux desktop/native Wayland path has been validated.
Color overrides use original token names. Two-stop linear gradients are actually
painted for `--surface-background`, `--layer-background-navbar-normal` and
`--layer-background`; other gradient/layout tokens remain inspectable source
facts. Reset restores inheritance. Token editing and the searchable, paged icon
registry are optional Advanced sections, closed initially.

Custom-painted controls explicitly handle hover, press, selection, disabled and
keyboard focus. Navigation focus uses its contrasting original foreground;
standard controls use source border/interaction roles. QR background/image tint
remain white intentionally. Avatar accent and on-color text roles are resolved
through the active palette rather than hardcoded Light colors.

Typography uses the original Segoe assets at fixed logical roles: metadata 12,
body/button 14, chat text 15, headings 18 and modal titles 16. Native DPI and font
antialiasing remain toolkit controlled; resizing a window does not change these
roles. Shared modal chrome binds `--curtain` and `--divider`, with a 48 px header,
original close glyph and a bounded action row. Original-source inventory now
tracks 31 palette roles/34 source tokens; all 758 tokens remain available in the
catalog. This does not imply every source component is implemented.

Avatars currently use generated initials for both cached live names and synthetic
contacts; profile photos are not downloaded. Unknown live senders use a neutral
label/avatar, and group titles are never member names. Unavailable live actions
remain disabled. The demo composer saves only locally; live text sends require a
verified session and supported conversation capability. Ctrl+Enter submits when
eligible and outside IME composition; plain Enter inserts a newline. Media
upload, sticker sending/animation and production E2EE remain unsupported. Current
flows and agent acceptance rules are documented in
[UI_DESIGN.md](UI_DESIGN.md).
