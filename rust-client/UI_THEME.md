# Native UI and existing Zalo theme

The Rust view uses egui, with the existing repository's **Light** theme. It does
not load a WebView, redesign the theme, or use ctdozlite UI code or assets.

The source of truth is the `:root,:root .light` declaration and the matching
component selectors in
`pc-dist/default-login-startup-main-startup.e08d0d44f38873747a6b.css`.
`src/ui/visuals.rs` maps these existing values into native colors:

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

The existing `pc-dist/fonts/Zalo-Segoe-UI.dc1a7d868a0b3a0e2daa8b36567e9ea9.ttf`
and `zalo-font.7f2d95cebfac22d16e486bfd1716c591.ttf` are embedded directly from
their repository paths. Icon codepoints match the corresponding `.fa-*` CSS
declarations. These assets retain their existing provenance; no replacement font
or decorative artwork was added. Fonts are initialized once, without frame-time
file reads.

Only synthetic contacts use generated initials. Unavailable live actions remain
disabled. The composer explicitly saves locally, using the real SQLite worker;
Ctrl+Enter saves and plain Enter inserts a newline. Light theme mapping does not
yet implement persisted Dark/System/Custom selection from the Electron profile.
