# Zalo Linux Port 2026

⚠️ **Work in Progress** - This project is under active development.

A Linux port of Zalo, bringing the popular Vietnamese messaging application to the Linux platform.

<img width="1280" height="799" alt="image" src="https://github.com/user-attachments/assets/7f3000e2-6d5d-4bc1-a4c1-d334f4d7a3e9" />

## How It Works

This is an unofficial port of the **Zalo macOS desktop client** to Linux — not a web wrapper. Calls are not supported yet.

The starting point was:

1. Extracting the `.dmg` from the macOS version
2. Locating `app.asar` at `/Applications/Zalo.app/Contents/Resources/`
3. Extracting it with `asar extract app.asar app`
4. Running the extracted app with Electron

Running the bundle is only half of it. The macOS client leans on native addons shipped as Mach-O binaries, which do nothing on Linux, so several of them had to be reverse-engineered and rebuilt. That work — plus the launcher, packaging and hardening layer — is what this repository actually contains.

### Electron version

The port runs on **Electron 43.3.0** (Chromium 150, Node 24), pinned by version *and* SHA-256 in `start.sh`.

It previously shipped Electron 22.3.27 (Chromium 108), which had been end-of-life since 2023. The upgrade took two fixes:

- **Login hung after the QR scan.** `webContents.incrementCapturerCount()` was removed in Electron 23 and sat in a comma expression *before* the `did-finish-load` handler was attached, so the handler that closes the login window was never registered. Both bundles now guard the call.
- **Chat photos stopped loading.** Zalo serves message photos as JPEG XL; Chromium dropped its JXL decoder in version 110, and upstream's `zjxl` fallback is macOS/Windows only. A Linux `zjxl` addon now covers it (see below).

Set `ZALO_ELECTRON_VERSION` to try another build. Always pair it with a throwaway `HOME` — a different Chromium major rewrites the IndexedDB schema in `~/.config/ZaloData` and going back deletes it as corrupt, destroying your stored session.

## Installation

| | Prefix | Self-update | Sandbox |
|---|---|---|---|
| **`.deb`** | `/opt/zalo` | no — your package manager owns it | SUID `chrome-sandbox`, works everywhere |
| **AppImage** | AppImage mount | prompts, opens the releases page | user-namespace sandbox |
| **`install.sh`** | `~/.local/share/zalo` | yes, via `update.sh` | user-namespace sandbox |

The `.deb` is the most robust of the three: its `postinst` installs `chrome-sandbox` setuid root, so the Chromium sandbox works even on distributions that restrict unprivileged user namespaces (Ubuntu 24.04+ does by default).

### Option 1: `.deb` package

Download the latest `.deb` from the [Releases](https://github.com/realdtn2/zalo-linux-2026/releases/latest) page, then:

```bash
sudo apt install ./zalo_*_amd64.deb
```

### Option 2: AppImage

Download the latest AppImage from the [Releases](https://github.com/realdtn2/zalo-linux-2026/releases/latest) page, then:

```bash
chmod +x Zalo-*.AppImage
./Zalo-*.AppImage
```

No installation needed — fully self-contained, and **no `libfuse2` required**.

### Option 3: Install script

```bash
git clone https://github.com/realdtn2/zalo-linux-2026.git
cd zalo-linux-2026
./install.sh
```

## Usage

**Launch the application**: open Zalo from your application launcher or desktop launcher.

**Start manually**:

```bash
./start.sh
```

`start.sh` downloads and verifies Electron on first run, then launches **with the Chromium sandbox**, falling back to `--no-sandbox` only if the sandbox genuinely cannot start (and telling you how to fix it properly). `ZALO_NO_SANDBOX=1` is the deliberate escape hatch.

**Update the application** (install-script target only):

```bash
./update.sh
```

## Building from source

```bash
./build-deb.sh                 # → dist/zalo_<version>_amd64.deb
./build-appimage.sh            # → dist/Zalo-<version>-x86_64.AppImage
python3 generate-addon.py      # rebuild the Linux db-cross-v4 addon
python3 generate-jxl-addon.py  # rebuild the Linux zjxl (JPEG XL) addon
```

Requirements:

- `build-appimage.sh` — `wget`, `unzip`, `sha256sum`
- `build-deb.sh` — the above plus `dpkg-deb`, `fakeroot`, `python3` with Pillow
- `generate-addon.py` — `liblzma-dev`, `libssl-dev`, node + npx
- `generate-jxl-addon.py` — `libjxl-dev`, `libjpeg-turbo8-dev`, node + npx

Every artifact these scripts download — Electron, appimagetool, the AppImage runtime — is pinned to an exact version *and* verified against a pinned SHA-256 before use. Nothing resolves a rolling tag.

## Features

### `zcall` (Audio & Video Calling)

**Status:** ❌ Unported (Stubbed)

**Description:**
A massive proprietary VoIP and WebRTC stack built around custom ZRTP-based encryption. Implemented through `zcall_mac.node` and responsible for all voice and video calling functionality.

---

### `db-cross-v4` (Backup Decryption Engine)

**Status:** ✅ Ported

**Description:**
Replaces the original macOS Mach-O binary. Intercepts backup restoration calls to decompress and decrypt proprietary ZDB4.0 backup containers using AES-256-CBC and LZMA2, enabling full chat history recovery.

**State:**
Fully reverse-engineered and reimplemented in C++ (`generate-addon.py`). The Linux replacement is complete and currently active. Entry names inside a container are attacker-controlled, so every path join is validated against the output directory — absolute paths, `..` and symlinked components are rejected.

---

### `zjxl` (JPEG-XL Codec Support)

**Status:** ✅ Ported

**Description:**
Decodes the JPEG XL images Zalo serves for message photos. Upstream ships this only as Mach-O/PE and otherwise relies on Chromium, which removed JPEG XL in version 110 — so on Electron 43 Linux had no decoder at all and every chat photo collapsed into a download placeholder that re-downloading could never fix.

**State:**
Rebuilt for Linux as an N-API addon over libjxl and libjpeg-turbo (`generate-jxl-addon.py`), exposing the same seven entry points the app calls. libjxl and its dependencies are vendored next to the addon, so it does not depend on the host distribution shipping a matching libjxl.

---

### Screen capture

**Status:** ✅ Ported

**Description:**
Upstream ships the capture UI as a per-platform Qt binary (`ZaloCap.exe` / `ZaloHelper.app`), so on Linux the app spawned a path that does not exist and the feature never worked.

**State:**
Replaced by `native/zalo-cap-linux`, a helper that re-execs the bundled Electron and speaks the same stdio protocol, with region select and annotation. It captures through `org.freedesktop.portal.Screenshot`, which the permission store remembers — granted once, then silent. (`desktopCapturer` was rejected: it reopens the "share your screen" picker on every single call.)

---

### `zimage` (Advanced Image Processing)

**Status:** ❌ Unported

**Description:**
Performs computationally intensive image operations such as thumbnail generation, resizing, and image transformations using the bundled `libvips-cpp` library.

---

### `mp4thumb` (Video Thumbnail Generation)

**Status:** ❌ Unported

**Description:**
Generates image thumbnails from `.mp4` attachments. Functions as a native macOS wrapper around a statically linked FFmpeg component that extracts preview frames.

---

### `file-utilities` (Fast Directory Sizing)

**Status:** ❌ Unported (Throws Error)

**Description:**
A Rust-based NAPI-RS module used for high-performance recursive directory size calculations.

---

### `zwalker` (Recursive Directory Scanner)

**Status:** ❌ Unported (Stubbed)

**Description:**
Traverses the filesystem to locate files, index content, and discover backups.

---

### `file-utils` (Low-Level File System Utilities)

**Status:** ❌ Unported (Returns "not support")

**Description:**
Provides native wrappers around common filesystem operations such as moving, copying, and manipulating files.

---

### `v8-profiles` (CPU Profiling)

**Status:** ❌ Unported

**Description:**
A macOS-specific profiling module used to analyze V8 JavaScript engine performance.

---

### `zfile` (Disk Information)

**Status:** ❌ Unported (Stubbed)

**Description:**
Retrieves disk usage statistics, storage capacity information, and available free space.

---

### `sqlite3` (Local Database Engine)

**Status:** ✅ Supported

**Description:**
The native database engine used to access local message shard databases (`.db` files).

**State:**
The original macOS binary has been replaced with a Linux ELF build (`node_sqlite3.node`) bundled within the application. Functionality is fully operational.

---

### `zaloLogger` (IPC Logging)

**Status:** ✅ Supported

**Description:**
Custom logging infrastructure utilizing an IPC transport layer.

**State:**
Implemented entirely in cross-platform JavaScript and requires no platform-specific porting work.

## Security notes

Beyond moving off an end-of-life Chromium, this port:

- launches **with** the Chromium sandbox rather than disabling it unconditionally, and installs the SUID sandbox helper in the `.deb`
- verifies every downloaded artifact against a pinned SHA-256, and stages extractions so an interrupted download never leaves a half-populated Electron directory
- disables the bundled `electron-updater`'s `autoDownload` / `autoInstallOnAppQuit`, which would otherwise download and install upstream builds behind your back
- applies the main window's Content-Security-Policy to the renderer pages that shipped without one

Note that the ~300k lines of minified vendor bundles in `main-dist/` and `pc-dist/` have not been audited.

## Troubleshooting

**"It asks me to log in every time."** Zalo encrypts its session with Electron `safeStorage`, which on Linux means Chromium OSCrypt backed by your keyring. If the keyring is not reachable — a bare TTY, an SSH session, a desktop where gnome-keyring never unlocked — Chromium falls back to a hardcoded key and the stored session can no longer be decrypted. Make sure `gnome-keyring` or `kwallet` is running; the `.deb` depends on `libsecret-1-0` for this reason.

**Zalo restarts itself and dies immediately** (an apport report in `/var/crash/` with `SIGTRAP`, and `traps: electron ... trap int3` in the journal). Fixed in v1.3.3. Electron's own `app.relaunch()` starts the new instance with `no_new_privs` set, which disables the setuid sandbox helper; on Ubuntu 24.04+ the user-namespace fallback is blocked by AppArmor, so the relaunched instance had no usable sandbox and aborted. The launcher now relaunches from the browser process instead, and the successor's output is kept in `~/.config/ZaloData/relaunch.log`.

**The screenshot permission prompt keeps appearing.** The first capture asks through the desktop portal; grant it and the answer is remembered. `xdg-desktop-portal` and its backend for your desktop need to be installed.

## Contributing

This is an active work-in-progress project. Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Commit your changes (`git commit -m 'Add your feature'`)
4. Push to the branch (`git push origin feature/your-feature`)
5. Open a Pull Request

Note that the main branch is `latest`, not `main`.

## Disclaimer

⚠️ This is a community port and is not officially affiliated with Zalo or VNG Corporation.

## Support

For issues, questions, or suggestions, please open an issue on the [GitHub Issues](https://github.com/realdtn2/zalo-linux-2026/issues) page.
