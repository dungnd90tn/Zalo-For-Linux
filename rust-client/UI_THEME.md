# Native UI and existing Zalo theme

The Rust view uses egui with the repository's **Light/Dark** palettes, **System**
selection and source-backed **Custom** overlays. It does
not load a WebView or use ctdozlite UI code or assets. The look is the approved
redesign defined in [UI_REDESIGN_GUIDE.md](UI_REDESIGN_GUIDE.md): its Light and Dark
values live in `src/theme/redesign.rs` and are layered over the generated original
catalog when `Palette` is resolved (a Custom override wins, then the redesign value,
then the original token). The catalog itself is unchanged and stays the record of
the original declarations.

The source of truth is the `:root`, `.light`, `.dark` declarations and the matching
component selectors in
`pc-dist/default-login-startup-main-startup.e08d0d44f38873747a6b.css`.
`src/theme.rs` resolves semantic roles against the generated original token
catalog; `src/ui/visuals.rs` paints components. Full source coverage, registry
verification and component states are tracked in
[UI_ASSET_INVENTORY.md](UI_ASSET_INVENTORY.md). The table lists the **original**
Light source values for provenance; the effective Light/Dark values are the redesign
table in the guide:

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

Native layout keeps the rail/list/chat structure with the redesign's dimensions:
68 px rail with 48 px rounded navigation buttons, 344 px default conversation panel
(280-420 when dragged), 44 px search field and filter chips, 72 px conversation rows
with a 14 px rounded selection, and a 76 px chat header. Small windows collapse the
list behind back navigation, preserving the selected recipient and drafts.

Existing Segoe UI regular/Semibold/Bold and the Zalo/Screencap icon TTFs are
embedded directly from `pc-dist/fonts/`. All 1,034 terminal pseudo-element
classes/aliases in the generated registry are verified against their original
font cmaps; the 15 semantic controls resolve through this registry. Font assets
retain their repository provenance. Font setup happens once, with no frame-time
file reads or eager glyph rasterization.

The bottom rail/login gear opens an anchored menu; its appearance entry opens
settings. Signed-in logout has a separate menu entry and confirmation; signed-out
menus expose appearance only. The rail avatar uses original online `--G60`, ring
`--WA100`, offline `--grey-dot` and connecting `--icon-warning` roles. Its popup
holds connection details/recovery, replacing permanent status/account bars. Four
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
roles. The original CSS body uses `SegoeuiPc`; the alternative `SegoeuiPcText`
face is declared for `.ztext-font`, which the audited message components do not
use. egui uses its own grayscale font rasterizer, so these mappings do not imply
identical rendering to Chromium or GNOME. Shared modal chrome binds `--curtain`
and `--divider`, with a 48 px header,
original close glyph and a bounded action row. Original-source inventory now
tracks 35 palette roles/38 source tokens; all 758 tokens remain available in the
catalog. This does not imply every source component is implemented.

Avatars lazily download synced account/contact/group photos only for visible
positions, through the bounded media worker. Native presentation center-crops
circles without stretching the photo; missing photos keep the initials fallback.
Group and user keys remain distinct, and privacy/account changes discard stale
metadata and textures. Unknown live senders use a neutral label/avatar, and group
titles are never member names. The GPU avatar cache is capped at 64 images/4 MiB;
decoded output is at most 256 physical pixels per side.

Conversation rows use one full-card click target with painted text. Plain text
bubbles hug measured content/status instead of reserving their maximum width.
The composer uses 100 logical pixels while idle or compact, growing to 132 only
for at least three explicit lines in a wide window with sufficient height; long
input still scrolls. Source typography and theme colors remain unchanged.
Unavailable live actions
remain disabled. The demo composer saves only locally; live text sends require a
verified session and supported conversation capability. Enter submits when
eligible and outside IME composition; Ctrl+Enter (or Shift+Enter) inserts a newline. Media
upload, sticker sending/animation and production E2EE remain unsupported. Current
flows and agent acceptance rules are documented in
[UI_DESIGN.md](UI_DESIGN.md).
