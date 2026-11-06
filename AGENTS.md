# Repository Guidelines

## Rule #0 — Git and shared state

1. **Think before acting:** Fully understand the requirements, processing flow, and business impact before making any changes.
2. **Simplify the code:** Write only what is necessary to fulfill the business logic; avoid unnecessarily complex solutions.
3. **Limit the scope of changes:** Modify only what is required; do not alter code or formatting in unrelated areas.
4. **Do not commit/push without authorization:** Once a task is complete, keep the changes in your workspace and report your status; only commit or push when explicitly requested.
5. **Prioritize code, then documentation:** When implementing features or fixing bugs, focus on ensuring the code functions correctly and verifying the results before finalizing any documentation. Requests specifically for documentation updates should be handled directly.
6. **A hook/classifier/permission denial is final:** stop, report the action/reason and obtain the right permission. Never bypass via another tool, script, encoding, file, agent or later turn.

## Project Structure & Module Organization

This repository contains the Electron Linux port and independent Rust native preview.

- `bootstrap.js`: Electron entry and Linux startup/relaunch patches.
- `main-dist/`: main-process and preload bundles; `pc-dist/`: renderer bundles, HTML, and assets.
- `native/nativelibs/`: native addon loaders and binaries; `native/zalo-cap-linux/`: screenshot helper.
- `generate-addon.py`, `generate-jxl-addon.py`: addon builders containing embedded C++ source.
- `rust-client/src/`: Rust domain, storage, workers, protocols, profiles, UI, and inline tests.
- `scripts/rust/`: developer helpers and Python tooling tests; `rust-client/examples/`: synthetic benchmarks.
- Root packaging writes ignored `dist/`; `changelogs/` records releases; `reverse-engineering/` holds research.

For native UI changes in `rust-client/src/ui.rs`, `rust-client/src/ui/`, or UI-facing `rust-client/src/app.rs` and `rust-client/src/app/`, read [UI_DESIGN.md](rust-client/UI_DESIGN.md), the [redesign guide](rust-client/UI_REDESIGN_GUIDE.md) and [UI agent rules](rust-client/src/ui/AGENTS.md).

## Build, Test, and Development Commands

- `./start.sh`: verify/download pinned Electron and launch.
- `./build-appimage.sh`: produce `dist/Zalo-<version>-x86_64.AppImage`.
- `./build-deb.sh`: produce `dist/zalo_<version>_amd64.deb`.
- `python3 generate-addon.py`: rebuild backup-decryption addon.
- `python3 generate-jxl-addon.py`: rebuild JPEG XL addon.

- `./scripts/rust/build.sh --offline`: build locked release after fetching dependencies.
- `./scripts/rust/run.sh --demo`: launch offline synthetic conversations.
- `./scripts/rust/run.sh --live`: build/launch optional login/messaging.
- `./scripts/rust/check.sh`: Rust formatting, tests, Clippy, and displayless CLI/tooling checks.
- `./scripts/rust/package.sh`: build `dist/rust/zalo-native_*.deb` and `*.rpm` in pinned containers (needs Docker); `./scripts/rust/package/verify.sh` installs them in clean containers.
- `python3 scripts/rust/smoke-release.py`: verify binary using disposable profiles.

See `README.md` and `rust-client/VALIDATION.md` for prerequisites. Root `package.json` has no npm workflow.

## Coding Style & Naming Conventions

Use `cargo fmt` for Rust and keep Clippy clean; Rust/Python names use snake_case. Handwritten JavaScript uses four spaces, CommonJS, single quotes, semicolons, and camelCase. Shell configuration variables use UPPER_SNAKE_CASE.

Keep bundle patches surgical. Apply shared behavior changes to both `main-dist/main.js` and `main-dist/compact-app.js`. Edit embedded C++ in the generator scripts. Preserve single-line, double-quoted configuration assignments: packaging scripts extract them with `grep`/`cut`.

## Testing Guidelines

Rust uses inline `#[test]`/Tokio tests; Python uses unittest. Verify scoped storage, deduplication, pagination, backpressure, cancellation, and draft preservation. Test optional modules with `cargo test --locked --offline --all-features` inside `rust-client/`. No coverage threshold exists.

For Electron, run `./start.sh` and manually verify affected login, restart, images, screenshot, and restoration flows, including compact windows. Record results and run affected packaging builders. Synthetic Rust tests cannot verify live messaging.

## Commit & Pull Request Guidelines

Use imperative subjects, e.g. `Fix crash on restart`. Target `latest`. Explain changed behavior and verification, link issues, and include screenshots for UI changes. Update `version.txt` and `changelogs/` for releases.

## Security & Configuration

Preserve sandboxing and pinned checksums. Use disposable profiles; launcher and CLI share XDG defaults. Keep credentials, messages, backups, and profile data out of commits.
