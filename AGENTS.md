# Repository Guidelines

## Project Structure & Module Organization

This repository contains an Electron Linux port and an independently authored Rust native preview.

- `bootstrap.js`: Electron entry point and Linux startup/relaunch patches.
- `main-dist/`: main-process and preload bundles; `pc-dist/`: renderer bundles, HTML, and assets.
- `native/nativelibs/`: native addon loaders and binaries; `native/zalo-cap-linux/`: screenshot helper.
- `generate-addon.py`, `generate-jxl-addon.py`: addon builders containing embedded C++ source.
- `rust-client/src/`: Rust domain, storage, background worker, protocol, profiles, and native UI; unit tests live beside modules.
- `scripts/rust/`: developer helpers and Python tooling tests; `rust-client/examples/`: synthetic benchmarks.
- Root scripts package Electron into ignored `dist/`; `changelogs/` records releases. `reverse-engineering/` contains research helpers.

## Build, Test, and Development Commands

- `./start.sh`: download and verify the pinned Electron runtime, then launch locally.
- `./build-appimage.sh`: produce `dist/Zalo-<version>-x86_64.AppImage`.
- `./build-deb.sh`: produce `dist/zalo_<version>_amd64.deb`.
- `python3 generate-addon.py`: rebuild the backup-decryption addon.
- `python3 generate-jxl-addon.py`: rebuild the JPEG XL addon.

- `./scripts/rust/build.sh --offline`: build the locked release preview after fetching dependencies.
- `./scripts/rust/run.sh --demo`: launch synthetic offline conversations in a separate profile.
- `./scripts/rust/check.sh`: Rust formatting, tests, Clippy, and displayless CLI/tooling checks.
- `python3 scripts/rust/smoke-release.py`: verify the built binary with disposable profiles.

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
