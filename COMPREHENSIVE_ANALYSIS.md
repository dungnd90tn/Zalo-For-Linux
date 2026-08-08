# Zalo Linux Port — Comprehensive Reverse Engineering Analysis

## Executive Summary

This is an **unofficial port of the Zalo macOS desktop client to Linux**, created by extracting the `.asar` archive from the macOS `.dmg`, then running it with Electron v22.3.27. It is NOT a web wrapper — it runs the actual Zalo macOS Electron app code. The project is maintained at `github.com/realdtn2/zalo-linux-2026`.

**Current version:** v1.1.1 (project) / Zalo 26.3.20 (app version)
**Electron version:** v22.3.27 (CRITICAL — newer versions cause errors)
**App name:** Zalo — Nhắn gửi yêu thương (by VNG Corp.)

---

## 1. Repo Directory Structure

```
app-extracted/
├── bootstrap.js                    # Entry point: decides compact-app vs main path
├── package.json                    # Zalo v26.3.20 - 100+ dependencies
├── version.txt                     # "v1.1.0" (project version)
├── README.md                       # Installation & usage docs
│
├── start.sh                        # Launcher: downloads Electron, checks version, runs app
├── install.sh                      # Copies app files to ~/.local/share/zalo, creates .desktop
├── update.sh                       # Git-clones latest repo and replaces files
├── build-appimage.sh               # Creates self-contained AppImage with bundled Electron
├── toggle-devtools.sh              # Toggles devTools on/off in main.js and compact-app.js
├── generate-addon.py               # Generates & builds the Linux db-cross-v4-native.node C++ addon
│
├── libs/
│   └── perf-tracing/
│       └── runtime.js              # Global perf.record() timing infrastructure
│
├── main-dist/                      # Electron MAIN PROCESS code (Node.js side)
│   ├── main.js                     # 155K lines - core main process (Electron app lifecycle)
│   ├── compact-app.js              # 147K lines - compact app mode main process
│   ├── migration.js                # 25K lines - data migration subsystem
│   ├── sentry.js                   # 12K lines - Sentry integration wrapper
│   ├── second-instance.js          # 0.08K - single-instance lock handler (quits 2nd instance)
│   ├── mainless-worker.js          # 35K lines - background worker
│   ├── utility-process-sqlite.js   # 54K lines - SQLite utility process
│   ├── preload-render.js           # 55K lines - preload for renderer window
│   ├── preload-noti.js             # 55K lines - preload for notification window
│   ├── preload-shared-worker.js    # 56K lines - preload for shared worker
│   ├── preload-sqlite.js           # 85K lines - preload for SQLite process
│   ├── compact-app-preload.js      # 69K lines - preload for compact app mode
│   ├── 0.js / 8.js / 9.js         # Small code-split chunks
│   ├── stast.json                  # Webpack stats: entrypoints and warnings
│   └── assets/                     # 3 emoji sprite PNGs
│
├── pc-dist/                        # RENDERER CODE (Chromium/window side)
│   ├── compact-app-pc.js           # 37.7 MB - main compact app bundle
│   ├── search-worker.js            # 38.3 MB - search worker (largest bundle)
│   ├── zd-worker.js                # 8.5 MB - Zalo Data worker
│   ├── shared-worker.js            # 0.3 MB - main shared worker
│   ├── mainless-worker.js          # 0.5 MB
│   ├── trust-worker.js             # 0.4 MB
│   ├── preview-thumb-worker.js     # 0.4 MB
│   ├── cpu-heavy-worker.js         # 0.3 MB
│   ├── dal-worker.js               # 0.3 MB
│   ├── opfs-worker.js              # 0.3 MB
│   ├── pdf-worker.js               # 1.4 MB
│   ├── render.js                   # 0.01 MB - main renderer entry
│   ├── login.js                    # 0.009 MB - login page
│   ├── znotification.js            # 0.009 MB - notification bundle
│   ├── sqlite.js                   # 0.002 MB - SQLite worker entry
│   ├── *.css                       # Various CSS bundles
│   ├── index.html                  # Main window HTML with CSP headers
│   ├── compact-app.html            # Compact app HTML
│   ├── login.html                  # Login page HTML
│   ├── child.html                  # Child window HTML
│   ├── popup-viewer.html           # Popup viewer HTML
│   ├── shared-worker.html          # Shared worker HTML
│   ├── znotification.html          # Notification HTML
│   ├── sqlite.html                 # SQLite worker HTML
│   ├── lazy/                       # ~48 code-split chunks + 2 lang files (vi, en)
│   │   ├── default-login-main-startup-shared-worker-znotification.js  # 30.5 MB - biggest lazy chunk
│   │   ├── vendors-login-main-startup-shared-worker-znotification.js  # 6 MB
│   │   ├── main-startup.js         # 3.3 MB
│   │   ├── login-startup.js        # 0.6 MB
│   │   └── ...                     # 30+ smaller chunks
│   ├── libs/
│   │   ├── libsignal-protocol.static.js  # 1.8 MB - Signal protocol
│   │   └── trusted_device_protocol_bg.wasm  # 0.35 MB - WASM trusted device module
│   ├── signal/
│   │   └── MessageProtocol.proto   # Protocol Buffer definition for Zalo message format
│   ├── emoji/
│   │   ├── sg_emo.json            # 304 KB - emoji catalog
│   │   ├── emoji-data.json        # 54 KB
│   │   ├── emoji-rc.json          # 51 KB
│   │   └── emoji-{16,24,26,32,40,64}/  # Emoji image sets at various sizes
│   ├── fonts/
│   │   ├── Zalo-Segoe-UI-*.ttf    # Custom Zalo fonts (6 variants)
│   │   ├── zalo-font.{ttf,woff2}  # Zalo icon font
│   │   ├── Unbounded-Bold.ttf     # 371 KB
│   │   ├── SVN-Hand-Of-Sean-Pro.ttf  # 171 KB
│   │   └── Roboto-Regular.ttf     # 126 KB
│   ├── assets/                    # ~60 image assets (PNG, GIF, JPG, ICO)
│   │   ├── favicon-*.png          # Various favicon sizes
│   │   ├── apple-icon-*.png       # Apple touch icons
│   │   ├── android-icon-*.png     # Android icons
│   │   ├── inapp-welcome-screen-*.{png,jpg}  # Onboarding assets
│   │   ├── QM_Guide_*.gif         # Quick message guide animations
│   │   ├── todo-onboarding-*.png  # Todo feature onboarding
│   │   ├── kiki-ai-onboarding.png # AI assistant onboarding
│   │   ├── sync_db_popup.gif      # Sync database popup
│   │   └── ...                    # Various UI assets
│   └── cocos2d-js-v3.13.jsapi     # 2 MB - Cocos2D game engine API (mini-games)
│
├── native/
│   └── nativelibs/
│       ├── index.js               # Multi-module aggregator (13 native modules)
│       ├── package.json           # "nativelibs" - Zalo PC native modules
│       ├── logger/                # Custom logger with IPC transport
│       ├── sqlite3/               # SQLite3 bindings (Linux x64 .node included!)
│       ├── db-cross-v4/           # Backup decrypt/restore addon (Linux RE implementation!)
│       │   ├── dist/binding.js    # Platform-aware loader
│       │   └── prebuilt/
│       │       ├── darwin/electron/{,arm64,x64}/  # Original macOS .node files
│       │       └── linux/electron/x64/            # Custom Linux RE .node (128 KB)
│       ├── file-utils/            # File system utilities (darwin + win, NO linux)
│       ├── file-utilities/        # NAPI-RS dir size scanner (darwin only, NO linux)
│       ├── zcall/                 # Audio/video calls native module (DARWIN ONLY)
│       │   ├── zcall_mac.node     # 7.5 MB - macOS native call binary
│       │   ├── binding.js         # Platform selector (MAC ONLY)
│       │   ├── binding-stub.js    # Linux stub (all no-ops)
│       │   ├── vcmac.js           # Video call mac JS wrapper (454 lines)
│       │   ├── screen-object.js   # Canvas-based screen renderer
│       │   └── screen-object-canvas.js  # Canvas screen object variant
│       ├── zimage/               # Image processing (vips-based, macOS only)
│       │   └── darwin_{arm64,x64}/zimage.node + libvips-cpp.42.dylib
│       ├── zjxl/                 # JPEG-XL codec (macOS only, NO linux)
│       │   └── darwin_{arm64,x64}/  # jxl.node + OpenCV + highway + brotli dylibs
│       ├── zwalker/              # Directory scanner (macOS only, Linux stub)
│       │   ├── darwin-{arm64,x64}/  # zwalker.node + libzwalker.dylib
│       │   └── index-stub.js     # Linux stub (all no-ops)
│       ├── zfile/                # File stat/disk info (Windows only)
│       │   └── win{32,64}/addon.node
│       ├── mp4thumb/             # MP4 thumbnail generator (macOS only)
│       │   └── darwin-{arm64,x64}/mp4thumb.node
│       └── v8-profiles/          # V8 CPU profiler (macOS only)
│           └── profiler_electron1.8_mac.node
│
├── reverse-engineering/          # Reverse engineering artifacts
│   ├── README.md                 # How to regenerate disassembly dumps
│   ├── darwin/
│   │   ├── db-cross-v4-native.disasm.txt  # 40K lines - full llvm-objdump disassembly
│   │   └── machofile/
│   │       ├── darwin-universal.json       # 2K lines - full Mach-O structure report
│   │       ├── darwin-x64.json             # x64 slice symbols/imports/exports
│   │       └── darwin-arm64.json           # ARM64 slice symbols/imports/exports
│   ├── old-linux/
│   │   └── db-cross-v4-native.old.node     # 92 KB - older Linux addon binary (reference)
│   ├── tools/
│   │   ├── check_decrypted_db.py           # Validate decrypted SQLite DBs
│   │   ├── debug_parse_binnet.js           # Debug native parseBinNet TLV parser
│   │   ├── offline_decrypt_check.py        # Offline decryption verification
│   │   └── build_old_addon.py              # Rebuild old Linux addon (274 lines)
│   └── vendor/
│       └── pydeps/machofile/               # machofile Python package (Mach-O parser)
│
├── dist/                         # AppImage build output directory (gitignored)
│   └── Zalo.AppDir/              # AppDir structure for AppImage
│       ├── AppRun                # Entry script
│       ├── electron/             # Bundled Electron v22.3.27 runtime
│       ├── bootstrap.js          # (copy of root)
│       ├── main-dist/            # (copy of root)
│       ├── native/               # (copy of root)
│       ├── pc-dist/              # (copy of root)
│       ├── libs/                 # (copy of root)
│       └── start.sh              # (copy of root, with bundled electron path)
│
├── .github/workflows/
│   └── build-appimage.yml        # GitHub Actions: builds AppImage on release publish
│
└── .gitignore                    # Ignores dist/, .claw/, .clawd-todos.json
```

---

## 2. Architecture and Application Flow

### 2.1 Bootstrap Sequence

```
start.sh → electron → bootstrap.js → require('./main-dist/main')
```

1. **`start.sh`** downloads Electron v22.3.27 (if not cached), checks for updates via GitHub, then launches: `electron <app_directory>`
2. **`bootstrap.js`** is the entry point (`package.json` → `"main": "bootstrap.js"`)
   - Loads performance tracing (`libs/perf-tracing/runtime.js`)
   - Runs migration (`main-dist/migration.js`) — data schema upgrades
   - Checks for `--launch-compact-app` flag → compact app mode (child popup windows)
   - Uses `app.requestSingleInstanceLock()` for single-instance enforcement
   - Requires `main-dist/main.js` as the main process
3. If already running, `second-instance.js` just logs and calls `app.quit()`

### 2.2 Renderer Architecture

- **Main window:** `pc-dist/index.html` → loads `render.js` + lazy code-split chunks
- **Compact app windows:** `pc-dist/compact-app.html` → loads `compact-app-pc.js`
- **Notification window:** `pc-dist/znotification.html`
- **Shared worker:** `pc-dist/shared-worker.html`
- **SQLite worker:** `pc-dist/sqlite.html`
- **Login page:** `pc-dist/login.html`
- **Child popups:** `pc-dist/child.html`

The renderer is a massive React 16 + Redux + Recoil SPA. Workers are used for:
- **search-worker** (38 MB) — Full-text search indexing
- **shared-worker** — Cross-window state sharing
- **zd-worker** (8.5 MB) — Zalo Data sync worker
- **trust-worker** — Trusted device verification (uses WASM)
- **pdf-worker** — PDF rendering
- **cpu-heavy-worker** — CPU-intensive offloading
- **dal-worker** — Data access layer worker
- **opfs-worker** — Origin Private File System operations
- **preview-thumb-worker** — Thumbnail generation
- **mainless-worker** — Background processing when main window is closed

### 2.3 Key Technology Stack

| Technology | Usage |
|---|---|
| Electron v22.3.27 | Runtime framework |
| React 16.14.0 | UI framework |
| Redux 4.0.5 + Recoil 0.7.1 | State management |
| Webpack | Bundle system (code-split, lazy loading) |
| SQLite3 (native) | Local database |
| Signal Protocol | End-to-end encryption |
| Protobuf | Message serialization |
| LZMA2/XZ | Backup compression |
| AES-256-CBC | Backup encryption |
| TensorFlow.js | ML features |
| Cocos2D JSAPI | Mini-games engine |
| Sentry | Error monitoring |
| VIPS | Image processing |
| JPEG-XL + OpenCV | Image codec processing |
| WebSockets + HTTP | Network communication |

---

## 3. Native Modules Deep Dive

### 3.1 Module Index (`native/nativelibs/index.js`)

The aggregator lazily loads 13 native modules:

```javascript
fileUtils()      → file-utils (darwin + win, NO linux → returns {error:'not support'})
winUtils()       → win-utils (Windows only)
zcall()          → Audio/video calls (macOS binary + Linux stub)
sqlite3()        → SQLite3 bindings (Linux x64 .node included!)
dbUtils()        → db-cross-v4 (backup decrypt, Linux RE implementation!)
v8Profiles()     → V8 profiler (macOS only)
zimage()         → Image processing via VIPS (macOS only)
zaloLogger()     → Custom logger with IPC transport (pure JS, cross-platform)
zjxl()           → JPEG-XL codec (macOS only)
zwalker()        → Directory scanner (macOS only + Linux stub)
zfile()          → File stat/disk info (Windows only)
fileUtilities()  → Directory size scanner via NAPI-RS (macOS only)
mp4thumb()       → MP4 thumbnail generation (macOS only)
```

### 3.2 Linux Implementation Status

| Module | Linux Support | Status |
|---|---|---|
| **sqlite3** | ✅ Native .node included | Working (napi-v6-linux-x64/node_sqlite3.node, 2.4 MB) |
| **db-cross-v4** | ✅ RE implementation | Custom C++ addon via `generate-addon.py` |
| **zaloLogger** | ✅ Pure JS | Works everywhere |
| **zcall** | ⚠️ Stub only | All functions return no-op/null (no calls on Linux) |
| **zwalker** | ⚠️ Stub only | All functions return empty (no file scanning) |
| **file-utils** | ❌ Not supported | Returns `{error: 'not support'}` on Linux |
| **file-utilities** | ❌ Not supported | Throws on Linux (no binding available) |
| **zimage** | ❌ Not supported | VIPS binary macOS-only |
| **zjxl** | ❌ Not supported | JPEG-XL/OpenCV macOS-only |
| **zfile** | ⚠️ Stub only | Returns no-op functions |
| **mp4thumb** | ❌ Not supported | macOS-only binary |
| **v8-profiles** | ❌ Not supported | macOS-only binary |

### 3.3 db-cross-v4 (Most Critical Linux RE)

**Purpose:** Decrypt and decompress Zalo backup files (ZDB4.0 container format).

**Original:** macOS Mach-O universal binary → 3 slices (x86_64, arm64, "fat" universal)

**RE Process:**
1. Full `llvm-objdump` disassembly dump (40,785 lines in `reverse-engineering/darwin/db-cross-v4-native.disasm.txt`)
2. `machofile` JSON reports for symbol/import/export analysis
3. Branch-by-branch reconstruction into C++ (`generate-addon.py` embeds ~850 lines of C++ source)
4. Built with `node-gyp` against Electron v22.3.27 ABI using `node-addon-api`

**Implemented functions:**

| Export | Purpose | Status |
|---|---|---|
| `decompressAndDecryptDb` | V1 decrypt (legacy) | Returns -1 (not implemented) |
| `decompressAndDecryptDb_V2` | V2 decrypt (current) | **Fully implemented** |
| `parseBinNet` | Binary TLV message parser | **Fully implemented** |

**V2 Decrypt Algorithm (fully reverse-engineered):**
1. Read ZDB4.0 container file
2. Derive AES-256 key from private key string:
   - JS passes `privateKey.toUpperCase()` 
   - First 32 ASCII chars used directly as AES key bytes (NOT hex-decoded)
3. AES-256-CBC decrypt with NULL IV, chunked mode (reset IV to zeros per 65536-byte chunk)
4. Verify `ZDB4.0` magic header
5. Parse file table: count + [name_length, name, size] per file
6. LZMA2 (XZ) decompress the payload
7. Write extracted .db files to output directory
8. Call JS progress callback after each file completes
9. Validate at least one extracted .db is a valid SQLite database

**TLV Parser (parseBinNet):**
- Implements TLV (Type-Length-Value) box parsing matching macOS behavior
- Auto-detects big-endian vs little-endian encoding
- Key mappings reverse-engineered from macOS:
  - `0x05` = property (size, color, type, subType, ext)
  - `0x06` = attachments array (with sub-TLV: title, href, thumb, size, ext, type, etc.)
  - `0x07` = quote/reference message
  - `0x08` = mentions array (uid, pos, len, type)
  - `0x09` = quoteStatus
  - `0x0a` = dataSource
  - `0x0b` = data

---

## 4. Critical Patches Applied to macOS Code

### 4.1 Tray Icon Fix (commits `859fe46`)

The macOS app's tray icon system was patched for Linux:
- `main-dist/main.js`: Tray icon path adjusted for Linux
- `main-dist/compact-app.js`: Same tray fix applied

### 4.2 Linux Menu Fix

In `main.js` (~line 81,548):
```javascript
process.platform === "linux" 
    ? i.setApplicationMenu(null)  // Linux: no app menu bar
    : i.setApplicationMenu(i.buildFromTemplate(a))  // Mac/Win: standard menu
```

### 4.3 devTools Toggle (commits `5d5d633`, `cc2bb18`)

The `toggle-devtools.sh` script patches `main.js` and `compact-app.js`:
- Sets `devTools: !0` (enabled) or `devTools: !1` (disabled)
- Patches `oe()` function (a dev-mode check) to always return true/false
- By default devTools is OFF

### 4.4 Image Pasting Fix (commit `cc2bb18`)

Patched in 7 files (`preload-render.js`, `preload-noti.js`, `preload-shared-worker.js`, `preload-sqlite.js`, `compact-app-preload.js`, `main.js`, `compact-app.js`) to fix image file pasting on Linux.

### 4.5 Auto-launch / Auto-startup

The code uses `auto-launch` npm package (v5.0.1) for auto-start on login. Linux auto-launch paths are handled in `main.js` at line ~36,247:
```javascript
if (/linux/.test(process.platform)) this.api = /* Linux auto-launch implementation */
```

### 4.6 Update System

The app has a built-in auto-updater (`electron-updater` 4.0.0). On Linux, it falls back to `AppImageUpdater` (line ~20,241):
```javascript
"linux" === process.platform ? new AppImageUpdater()
```
But in practice, the port's `start.sh` and `update.sh` scripts provide the actual update mechanism (git clone + replace files).

---

## 5. App Data and Storage

### 5.1 Data Directory

```javascript
// From second-instance.js:
appFolder: "ZaloData"    // Current
appFolderOld: "ZaloApp"  // Legacy
```

On Linux, data is stored in Electron's `app.getPath('userData')` or `~/.config/ZaloApp` or `~/.local/share/ZaloData`.

### 5.2 Database Architecture

Zalo uses multiple SQLite databases:
- **Core/Message/`<convId>`.db** — Per-conversation message databases
- Various shard databases for different data types
- SQLite accessed through both native bindings and a dedicated `utility-process-sqlite.js` worker
- **OPFS worker** provides Origin Private File System access for web-compatible storage

### 5.3 Backup System

Backups are encrypted ZDB4.0 containers containing:
1. File table (metadata + sizes)
2. LZMA2-compressed SQLite databases
3. AES-256-CBC encrypted payloads
4. The `db-cross-v4` native module handles decrypt/restore

---

## 6. Scripts Overview

### 6.1 `start.sh` (85 lines)
- Downloads Electron v22.3.27 to `~/.local/electron-v22.3.27/`
- Supports AppImage (bundled electron at `$INSTALL_DIR/electron/electron`)
- Checks for updates via GitHub `version.txt` (uses zenity dialogs)
- Runs: `electron <app_directory>`

### 6.2 `install.sh` (154 lines)
- Installs to `~/.local/share/zalo/`
- Installs missing dependencies (wget, unzip, curl, git, zenity)
- Creates `.desktop` entries in `~/.local/share/applications/`
- Excludes: `.git`, `install.sh`, `reverse-engineering/`, `generate-addon.py`

### 6.3 `update.sh` (154 lines)
- Git clones latest repo to `/tmp/`
- Compares versions (semver)
- Shows progress via zenity with FIFO-based IPC
- Replaces all files except excluded items

### 6.4 `build-appimage.sh` (186 lines)
- Reads config from `start.sh`, `install.sh`, `version.txt`
- Creates AppDir structure with bundled Electron v22.3.27
- Creates `AppRun`, `.desktop`, sets icon
- Uses `appimagetool` to build final AppImage
- Output: `dist/Zalo-<version>-x86_64.AppImage`

### 6.5 `generate-addon.py` (1076 lines)
- Generates C++ source for `db-cross-v4-native` Linux implementation
- Embeds full TLV parser, AES-256-CBC decrypt, LZMA2 decompression
- Auto-builds with `node-gyp` against Electron v22.3.27 ABI
- Requires: `liblzma-dev`, `libssl-dev`, `node-addon-api`
- Output: `native/nativelibs/db-cross-v4/prebuilt/linux/electron/x64/db-cross-v4-native.node`

---

## 7. Git History Summary

### Timeline

| Phase | Commits | Description |
|---|---|---|
| **Initial** (Apr 2026) | `09999c3` | "original" — Initial extraction from macOS Zalo v24.x, bare app.asar contents |
| **Porting fixes** (Apr 2026) | `859fe46`, `469ba43`, `8e97f58` | Tray icon fixes, start.sh creation, experimental Linux node |
| **Sync fixed** (Apr 2026) | `08046b8` | **Completely fixed sync process** — Updated db-cross-v4 addon, patched shared-worker.js, added full reverse engineering notes and tools |
| **Release 24.8.4–24.9.1** | `9946baa`, `de19aba`, `2e1d4b8` | Desktop entry fixes, update.sh fixes |
| **Merge cleanup** | `204f628` | Revamped installation process |
| **Script rewrite** (May 2026) | `5f86883` | Complete rewrite of install.sh, start.sh, update.sh |
| **bump to v1.1.0** | `046eb35` | CI/CD improvements (build-appimage.yml) |
| **v1.1.1 fixes** | `5d5d633`, `cc2bb18` | devTools off by default, image file pasting fixes |

### Current branch: `latest`

---

## 8. Unsupported Features on Linux

### 8.1 Not Working (needs native module port)

| Feature | Missing Module | Effort Estimate |
|---|---|---|
| **Audio/video calls** | zcall (7.5 MB native binary) | Very High — proprietary VoIP stack |
| **Image processing** (thumbnails, resizing) | zimage (VIPS-based) | High — need to compile VIPS for Linux + NAPI binding |
| **JPEG-XL support** | zjxl (OpenCV + highway + JXL) | High — complex native dependency chain |
| **File scanning/stat** | file-utils, file-utilities, zwalker | Medium — could reimplement in pure JS or compile from Rust |
| **MP4 thumbnails** | mp4thumb | Medium — FFmpeg-based, could use system FFmpeg |
| **V8 profiling** | v8-profiles | Low — Electron has built-in profiler now |
| **Windows disk info** | zfile | N/A on Linux (not needed) |

### 8.2 Working Features

- ✅ Message sync (send/receive)
- ✅ Login/authentication
- ✅ File transfer (images, documents)
- ✅ Backup restore
- ✅ Notifications
- ✅ Multi-window
- ✅ Emoji/stickers
- ✅ Search
- ✅ Dark mode
- ✅ Tray icon / system tray
- ✅ Auto-launch on startup
- ✅ Version check bypass
- ✅ Zalo AI (Kiki) onboarding UI (present but AI calls may need native modules)

---

## 9. Key Code Locations Reference

### 9.1 Files You Need to Patch for Linux

| File | Size | Purpose | What to patch |
|---|---|---|---|
| `main-dist/main.js` | 155K lines | Main process | Tray icon, devTools, linux paths, auto-updater |
| `main-dist/compact-app.js` | 147K lines | Compact app | Same as main.js |
| `main-dist/preload-render.js` | 55K lines | Renderer preload | Image pasting, protocol handlers |
| `native/nativelibs/zcall/binding-stub.js` | 36 lines | Call stub | Return no-ops (already done) |
| `native/nativelibs/zwalker/index-stub.js` | 10 lines | Walker stub | Return no-ops (already done) |

### 9.2 Files You Must NOT Modify (extracted macOS originals)

| File | Reason |
|---|---|
| `native/nativelibs/zcall/zcall_mac.node` | macOS binary — can't run on Linux |
| `native/nativelibs/zimage/darwin_*/` | macOS VIPS binaries |
| `native/nativelibs/zjxl/build/darwin_*/` | macOS JPEG-XL binaries |
| All `.dylib` files | macOS dynamic libraries |

### 9.3 Files You CAN Build for Linux

| File | Built by |
|---|---|
| `db-cross-v4/prebuilt/linux/electron/x64/db-cross-v4-native.node` | `generate-addon.py` |
| `sqlite3/binding/napi-v6-linux-x64/node_sqlite3.node` | Pre-built (included) |

---

## 10. Development Workflow

### 10.1 Running for Development

```bash
# Direct launch
./start.sh

# With devTools enabled
./toggle-devtools.sh  # toggles devTools on
./start.sh
```

### 10.2 Debugging Main Process

The main process code is minified webpack bundles. To debug:
1. Enable devTools with `toggle-devtools.sh`
2. Console logs go to stdout since `ELECTRON_ENABLE_LOGGING=1` is set
3. Custom logger (`native/nativelibs/logger/`) sends logs to both console and file via IPC

### 10.3 Debugging Renderer

1. Enable devTools in main.js
2. Open Chromium DevTools via the window (if devTools enabled)
3. Renderer bundles are webpack chunks with sourcemaps (referenced but not included)

### 10.4 Rebuilding the db-cross-v4 Addon

```bash
# Edit generate-addon.py to modify the C++ source
python3 generate-addon.py
# This auto-builds and copies to:
# native/nativelibs/db-cross-v4/prebuilt/linux/electron/x64/db-cross-v4-native.node
```

---

## 11. Security & Network Observations

### 11.1 CSP Policy (from index.html)

The Content Security Policy allows connections to:
- `*.zalo.me`, `*.zaloapp.com`, `*.zdn.vn`, `*.zadn.vn`, `*.z-cdn.me`
- `*.zing.vn`, `zingmp3.vn`, `*.baomoi.com`
- `*.google.com`, `*.google-analytics.com`, `*.gstatic.com`
- `*.dlfl.me`, `*.dlfl.vn`, `*.flchat.vn`, `*.pchat.vn` (Zalo infrastructure)
- `wss://*.zavi.me`, `wss://*.chat.zalo.me`
- `*.microsoft.com`, `*.apple.com` (login federation)

### 11.2 Sentry

Sentry is integrated for error tracking (`main-dist/sentry.js`, 572 KB). May need to be disabled/configured for the port to avoid sending crash reports from unofficial builds.

---

## 12. Summary of Challenges for Full Linux Port

1. **Call functionality** — The biggest missing feature. The zcall native module is a 7.5 MB macOS binary implementing a proprietary VoIP stack (WebRTC-like with custom ZRTP config). Complete rewrite or emulation needed.

2. **Image processing pipeline** — VIPS (libvips) is used for thumbnail generation, image resizing, and conversion. Need to port the NAPI binding and ensure libvips is available on Linux. Similarly for JPEG-XL via OpenCV.

3. **File system utilities** — `file-utilities` (NAPI-RS), `zwalker`, `file-utils` are macOS/Windows native addons. Some could be replaced with pure JS `fs` operations, others may need Rust → NAPI-RS compilation for Linux.

4. **Auto-updater** — The built-in `electron-updater` with `AppImageUpdater` may work, but the custom update scripts (`update.sh`) provide a simpler git-based update mechanism that works well.

5. **Version check bypass** — The app normally checks Zalo servers for minimum version. This must be patched to prevent forced-update prompts. Currently works but may break with Zalo server-side changes.

6. **Electron version lock** — v22.3.27 is required. Newer versions break the app (likely due to API changes in the macOS-native module loading). This means the port is stuck on an older Electron with potential security implications.