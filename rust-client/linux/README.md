# Native Linux desktop identity

The native client uses `zalo-linux-rust-preview` for its application ID,
desktop filename, `StartupWMClass`, and hicolor icon name. Electron keeps its
existing `Zalo` identity and files. The window embeds the existing repository's
`pc-dist/favicon-512x512.png`; desktop integration installs that image and its
256-pixel counterpart without alteration. Icon decoding happens once before
the event loop, uses the existing eframe PNG dependency, and fails explicitly
if the compiled asset is invalid.

Build and install the optional LIVE executable:

```sh
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/install-desktop.py --dry-run
python3 scripts/rust/install-desktop.py
```

The installer copies the built executable to
`${XDG_DATA_HOME:-$HOME/.local/share}/zalo-linux-rust-preview/bin/zalo-rust`,
creates its own entry under `applications/` and installs the original PNGs under
`icons/hicolor/{256x256,512x512}/apps/`. Run it again after a new build to update
the installed copy. `--binary` and `--data-home` support explicit artifact and
disposable staging paths. It does not build, launch, set autostart, replace
Electron, or read/change profiles. The desktop entry launches `--live`, whose
startup restores a saved session or creates QR; avoid launching it for offline QA.

Existing unmanaged entries and symlink destinations are rejected before writes.
Executable paths may contain spaces, Unicode and quoted shell punctuation;
control characters, `%` and `=` are rejected to prevent invalid desktop launch
commands. Optional desktop/icon cache tools run only after installation. Tests
use temporary XDG directories, preserve Electron sentinel files and exercise
actual GLib launch quoting with a tiny inert ELF fixture.

The identity follows the [Desktop Entry keys](https://specifications.freedesktop.org/desktop-entry/latest/recognized-keys.html)
and [Exec quoting rules](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html).
X11 acceptance must inspect actual `_NET_WM_ICON` RGBA pixels and `WM_CLASS`;
the Wayland app ID must match the desktop file's stem. A matching installation
does not itself prove rendering/grouping on every Wayland compositor.


## Validated local installation (2026-11-02)

The final LIVE executable was installed under
`~/.local/share/zalo-linux-rust-preview/bin/zalo-rust`, with its separate desktop
entry and original 256/512-pixel hicolor icons. The executable is 17,027,920 bytes,
SHA-256 `1d07f96c73751ddf450e8a658576de0dd910ed915ea0e082d5d8c7f551241ef3`.
Independent post-install checks matched the executable/icon hashes to their
sources and verified Exec/Icon/StartupWMClass; desktop validation and optional
cache refresh succeeded. Installation did not launch the app or stop an existing
window. Electron's entry was preserved.

The same release was run only in a disposable offline demo for QA. Actual X11
`_NET_WM_ICON` matched every original 512×512 RGBA pixel, and `WM_CLASS` matched
`zalo-linux-rust-preview`. Native Wayland grouping and an actual desktop-menu
launch remain unverified; those actions may start automatic live authentication.

## Conflict with the system packages

The `.deb`/`.rpm` install the same desktop ID (`zalo-linux-rust-preview`) under `/usr/share`. A per-user entry created
by `install-desktop.py` lives in `~/.local/share/applications` and takes precedence, so it keeps launching its own,
possibly older, executable. When moving to a system package, delete the per-user files first (see the release notes for
the exact commands) and regenerate the user icon cache with `gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor`:
a stale user cache can still resolve the icon to the deleted per-user path and show a blank icon.
