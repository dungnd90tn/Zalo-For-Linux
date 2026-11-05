# Zalo Native for Linux

**A native Zalo client for Linux, built in Rust.**

The application combines an `egui`/`eframe` interface, SQLite-backed local history and asynchronous networking workers. It renders without a WebView and uses the repository's existing Zalo theme, fonts and icons to keep the familiar messaging experience.

**Status: active native preview.** QR login, saved-session restore, incoming text and photos have reached real-account use. The client remains a partial implementation: complete bidirectional messaging, synchronization and long-running behavior still need acceptance. Performance comparisons against equivalent workloads have not been completed.

This is a community project, unaffiliated with Zalo or VNG Corporation.

## What the client does today

- **Sign in with QR.** Startup restores a saved session automatically or opens QR login. Confirmed expiry requests a new QR; transient connection errors preserve the session and offer Retry.
- **Keep local conversations usable.** Cached history, contact/group names, recent previews and per-conversation drafts remain available while offline. Unknown senders use neutral labels.
- **Display incoming content.** Text, structured notices, static sticker previews, photos and bounded OA/shared-card previews have native presentations. PNG/JPEG/WebP and a static JPEG XL subset are supported; cached images open in a native viewer.
- **Adapt to the window.** Wide and compact layouts preserve drafts and selection. Font roles and bounded image frames stay consistent as the window grows.
- **Use the existing Zalo appearance.** Light, Dark, System and Custom modes, original glyphs and per-profile appearance settings are included.
- **Expose actionable diagnostics.** Connection faults show a stage/category, with bounded structural logs for investigation.

Poll messages show embedded notices/questions; poll details, results and voting are unavailable. Unknown card templates retain an explicit fallback rather than pretending to support their actions.

## Build and run

### Linux build dependencies

This is a userspace application: choose dependencies by distribution, rather than kernel version. No custom kernel or kernel headers are required. The GUI needs a desktop session and a working OpenGL graphics driver; keep the driver appropriate for your hardware.

Current validation targets Linux x86_64. The package names below have been checked against distribution indexes; this does not mean an end-to-end build and desktop test has been completed on every listed distribution.

**Ubuntu / Debian** — [Ubuntu package index](https://packages.ubuntu.com/noble/libgl1-mesa-dev), [Debian package index](https://packages.debian.org/trixie/libxkbcommon-x11-0):

```bash
sudo apt update
sudo apt install build-essential pkg-config git curl ca-certificates python3 \
  libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libx11-dev libgl1-mesa-dev
```

**Fedora** — [OpenGL development package](https://packages.fedoraproject.org/pkgs/libglvnd/libglvnd-devel/):

```bash
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config git curl ca-certificates python3 \
  libxkbcommon-devel libxkbcommon-x11 wayland-devel libX11-devel libglvnd-devel
```

**Arch Linux** — [build tools](https://archlinux.org/packages/core/any/base-devel/), [X11 keyboard library](https://archlinux.org/packages/extra/x86_64/libxkbcommon-x11/):

```bash
sudo pacman -Syu --needed base-devel git curl ca-certificates python \
  libxkbcommon libxkbcommon-x11 wayland libx11 libglvnd
```

### Get the source

```bash
git clone https://github.com/dungnd90tn/Zalo-For-Linux.git
cd Zalo-For-Linux
```

Commands below run from the repository root unless stated otherwise.

### Install the pinned toolchain and build

Install `rustup` using the [official Rust installation guide](https://rust-lang.org/tools/install/), then reopen your terminal so its tools are on `PATH`. The repository pins **Rust 1.99.1** with `rustfmt` and `clippy` in [rust-toolchain.toml](rust-client/rust-toolchain.toml):

```bash
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy
(cd rust-client && cargo fetch --locked)
./scripts/rust/build.sh --offline
```

Fetch needs network access; subsequent build/check commands can work offline once dependencies are cached. Build helpers select the pinned toolchain automatically. Run direct Cargo commands inside `rust-client/` so the pin applies. With a custom Cargo target directory, use that directory instead of the default binary paths shown below.

### Explore without an account

```bash
./scripts/rust/run.sh --demo
```

Demo mode uses synthetic conversations and does not contact Zalo. For a displayless storage/worker check:

```bash
./scripts/rust/run.sh --demo --headless
```

### Sign in to Zalo

```bash
./scripts/rust/run.sh --live
```

The script builds the optional live features and launches the app. On first use, scan the QR code and approve login on your phone; later launches check the saved session automatically. The phone-number tab is explanatory: **phone-number login is not implemented**.

After a live build, run the executable directly without Cargo:

```bash
./rust-client/target/release/zalo-rust --live
```

Live mode contacts Zalo using the selected native profile. Default/demo builds exclude the optional HTTP, WebSocket and trust engines. Live builds also leave the experimental Signal engine inactive.

### Install the application launcher

Build live mode immediately before installing: the desktop entry launches `--live`, and a later default/demo build can replace the workspace executable.

```bash
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/install-desktop.py --dry-run
python3 scripts/rust/install-desktop.py
```

Open **Zalo Native** from your application menu. The installer copies the executable, creates a user-scoped desktop entry and installs the existing Zalo app icon. It does not launch the app, enable autostart or change profiles.

Run the installer again after rebuilding to update the installed copy, then close the old window and reopen Zalo Native. See [desktop integration](rust-client/linux/README.md) for managed paths, custom binaries and disposable staging.

## Current boundaries

The [feature parity checklist](rust-client/FEATURE_PARITY.md) separates implemented behavior from verified acceptance. In particular:

| Area | Remaining work |
| --- | --- |
| Messaging | Complete real-account personal/group send–receive acceptance, reconnect behavior and long-running operation |
| Synchronization | Full remote history, complete member discovery and downloaded avatars |
| Content | Media uploads, file/voice attachments, animated stickers and remaining OA layouts |
| Message actions | Replies, forwards, mentions, reactions, recall/delete and read receipts |
| Polls and search | Poll detail/results/voting and full message search |
| Linux integration | Tray/notifications, screenshot integration and broader desktop acceptance |
| Login and encryption | Phone-number authentication and production Signal/E2EE messaging |
| Distribution | Native release packages, updater and backup/history import |

Voice/video calls are unsupported. Unsupported encryption is never treated as ordinary text. Shared-card, poll and remaining OA behavior still require real-account verification; passing synthetic fixtures does not establish universal template support or full parity.

X11 input/rendering and an IME adapter have test coverage. Native Wayland font clarity, window integration and IME behavior still need desktop acceptance. A local test suite does not establish compatibility with every compositor or input method.

## Profiles and privacy

The client keeps data, configuration and cache in their respective XDG directories under `zalo-rust/profiles/<name>`. Default profile names are:

| Mode | Profile |
| --- | --- |
| `--demo` | `rust-demo` |
| `--live` | `rust-live` |
| Neither flag | `rust-pilot` |

Use `--profile-root` and `--profile` for explicit locations, or `ZALO_RUST_PROFILE_ROOT` and `ZALO_RUST_PROFILE` with the run script. Explicit CLI flags take precedence. Use disposable profiles for development; never share credentials, session files or history databases.

SQLite migrations preserve supported existing data. Older executables may not understand a newer schema; replacing a binary does not downgrade its database. Existing desktop history is not imported automatically.

### Diagnose a problem

Inspect capabilities and locate the live profile's diagnostics without starting login or writing profile data:

```bash
./rust-client/target/release/zalo-rust --doctor --profile rust-live
```

Normal launches write rotating structural logs at the selected cache profile's `diagnostics/native.log`; the terminal and doctor output show the path. Add bounded protocol metadata when reproducing a connection problem:

```bash
./rust-client/target/release/zalo-rust --live --debug-log
```

Logs omit message text, account identifiers, URLs, cookies, credentials and raw responses. Review any report before sharing it. A connection error should be investigated through its diagnostic stage/category; clearing a profile is not a routine recovery step.

## Development and verification

Run the default checks from the repository root:

```bash
./scripts/rust/check.sh
```

This checks formatting, Rust/Python tooling tests, Clippy, dependency boundaries and informational/headless CLI behavior. For optional-feature integration coverage:

```bash
(
  cd rust-client
  cargo test --locked --offline --all-features
  cargo clippy --locked --offline --all-targets --all-features -- -D warnings
)
```

Protocol fixtures require the `openssl` executable and permission to bind local loopback sockets. To verify a live release binary with disposable synthetic profiles:

```bash
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/smoke-release.py --expect-live
```

Run that block from the repository root. Release smoke does not sign in to a real account. GUI, IME and real-account checks remain separate; [validation](rust-client/VALIDATION.md) records reproducible procedures and their limits. The root `package.json` is not the native application's build or test entry point.

## Project structure

| Location | Purpose |
| --- | --- |
| `rust-client/src/` | Domain models, SQLite storage, workers, protocol adapters, profiles and native UI |
| `rust-client/src/ui/` | Login, conversation, media, settings and shared UI components |
| `rust-client/examples/` | Synthetic performance and storage harnesses |
| `scripts/rust/` | Build/run/check helpers, desktop installer, smoke checks and tooling tests |
| `rust-client/vendor/egui-winit/` | Pinned Linux IME adapter changes |
| `pc-dist/` | Existing Zalo fonts, icons, theme assets and protocol reference bundles |

Network requests, storage work and media decoding belong off the render path. Keep queues/caches bounded, preserve account/conversation scope, and guard against stale asynchronous results. See [architecture](rust-client/ARCHITECTURE.md) for the implementation model.

## Contributing and documentation

Start with [Repository Guidelines](AGENTS.md) for contribution rules. Describe the changed behavior and verification, and attach screenshots for UI changes. Native UI work follows [UI Design](rust-client/UI_DESIGN.md) and [UI agent rules](rust-client/src/ui/AGENTS.md), including the existing theme/icon system and consistent sizing.

- [Architecture](rust-client/ARCHITECTURE.md): application boundaries and worker/storage design.
- [Feature parity](rust-client/FEATURE_PARITY.md): implemented flows, gaps and acceptance criteria.
- [UI theme](rust-client/UI_THEME.md): appearance modes, source assets and token coverage.
- [Media contracts](rust-client/PROTOCOL_MEDIA.md): supported content, decoding and resource limits.
- [Validation](rust-client/VALIDATION.md): test commands and desktop acceptance evidence.
- [Migration status](rust-client/MIGRATION_STATUS.md): implementation checkpoints and current verification history.

Zalo branding and bundled assets remain attributable to their respective owners. Integration work does not imply affiliation or endorsement; bundled third-party code retains its own notices.

Report reproducible bugs or propose improvements through [GitHub Issues](https://github.com/dungnd90tn/Zalo-For-Linux/issues). Include the build/version, desktop environment, display backend, reproduction steps and sanitized diagnostics when relevant.
