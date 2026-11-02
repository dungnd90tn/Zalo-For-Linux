# Linux IME adapter patch

This directory vendors `egui-winit` **0.29.1** from the official crates.io
archive (SHA-256 `0a9c430f4f816340e8e8c1b20eec274186b1be6bc4c7dfc467ed50d57abc36c6`).
Its `.cargo_vcs_info.json` identifies upstream commit
`fe368bacc4a15697e347607e73a56c0299b3d42d`, `crates/egui-winit`.
The MIT and Apache-2.0 licenses are included from that exact upstream commit:
[upstream source](https://github.com/emilk/egui/tree/fe368bacc4a15697e347607e73a56c0299b3d42d/crates/egui-winit).
`Cargo.toml` only adjusts license include paths to their local locations;
versions, dependencies, and features are unchanged.

Upstream 0.29.1 discards every Linux `WindowEvent::Ime` after
[issue #5008](https://github.com/emilk/egui/issues/5008). That avoids a stuck
composition flag suppressing ordinary Backspace/arrows, but also loses actual
IBus/XIM committed text (reproduced with x-unikey 1.0.4-16).

Our Linux-only change forwards actual preedit/commit payloads, while treating
context `Enabled` as availability rather than composition. Each payload
initializes the current egui selection, including commit-only input. Empty
preedit/disabled ends composition; cancellation clears provisional text only
in its original focused widget. Window focus loss also cancels. Switching between editable widget IDs cycles
the Linux platform input context off/on once, resetting the input engine buffer
without resetting stable widgets every frame. Non-Linux
translation remains unchanged. `src/linux_ime.rs` contains the implementation
and real egui TextEdit regression tests; `src/lib.rs` only wires this state.
No input contents are logged by this adapter.

Run tests explicitly (dependency tests are not part of the application suite):

```sh
cargo test --locked --offline -p egui-winit --lib linux_ime
```

Native Wayland uses the same winit payload lifecycle, but isolated X11/IBus
acceptance does not establish native Wayland acceptance. Keep this patch
until a compatible upstream version fixes both commit delivery and navigation.
