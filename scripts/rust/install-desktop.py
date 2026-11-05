#!/usr/bin/env python3
"""Install only the native executable, desktop entry and original Zalo icons."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
APPLICATION_ID = "zalo-linux-rust-preview"


def exec_quote(value):
    """Desktop Entry Exec quoting, including its preceding string-unescape pass."""
    # '=' is forbidden in desktop executable names. GLib cannot reliably launch
    # an executable containing literal '%' field-code syntax, even escaped.
    if any(ord(ch) < 32 or ord(ch) == 127 or ch in "%=" for ch in value):
        raise ValueError("Choose --data-home without control characters, '%' or '='")
    quoted = "".join("\\" + ch if ch in '\\"`$' else ch for ch in value)
    return '"' + quoted.replace("\\", "\\\\") + '"'


def regular_destination(path):
    if path.is_symlink() or (path.exists() and not path.is_file()):
        raise ValueError("A managed destination is not a regular file: " + str(path))


def atomic_write(path, data, mode):
    path.parent.mkdir(parents=True, exist_ok=True)
    regular_destination(path)
    descriptor, temporary = tempfile.mkstemp(prefix=".zalo-native-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
            os.fchmod(output.fileno(), mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def install(binary, data_home, dry_run=False, refresh=True):
    binary = binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("Build an executable native release before installing")
    with binary.open("rb") as source:
        if source.read(4) != b"\x7fELF":
            raise ValueError("The selected native executable is not an ELF binary")
    if not data_home.is_absolute():
        raise ValueError("XDG data directory must be absolute")
    data_home = data_home.resolve()
    executable = data_home / APPLICATION_ID / "bin/zalo-rust"
    desktop = data_home / "applications" / (APPLICATION_ID + ".desktop")
    if (data_home / APPLICATION_ID).is_symlink():
        raise ValueError("Native installation directory must not be a symlink")
    regular_destination(desktop)
    if desktop.exists() and "X-Zalo-Native-Managed=true" not in desktop.read_text():
        raise ValueError("Existing desktop entry is not managed by the native installer")
    content = (ROOT / "rust-client/linux" / (APPLICATION_ID + ".desktop.in")).read_text()
    content = content.replace("@EXECUTABLE@", exec_quote(str(executable)))
    files = [(executable, binary.read_bytes(), 0o755), (desktop, content.encode(), 0o644)]
    for size in (256, 512):
        icon = ROOT / "pc-dist" / f"favicon-{size}x{size}.png"
        image = icon.read_bytes()
        if (len(image) > 64 * 1024 or image[:16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
                or image[16:24] != size.to_bytes(4, "big") * 2):
            raise ValueError("Bundled original Zalo icon has unexpected PNG dimensions")
        files.append((data_home / "icons/hicolor" / f"{size}x{size}/apps" /
                      (APPLICATION_ID + ".png"), image, 0o644))
    for path, _, _ in files:
        parent = data_home
        for part in path.relative_to(data_home).parts[:-1]:
            parent /= part
            if parent.is_symlink() or (parent.exists() and not parent.is_dir()):
                raise ValueError("Managed parent is not a real directory: " + str(parent))
        regular_destination(path)
    record = {"application_id": APPLICATION_ID, "launches_app": False,
              "files": [{"path": str(path), "bytes": len(data),
                         "sha256": hashlib.sha256(data).hexdigest()}
                        for path, data, _ in files]}
    if dry_run:
        return record
    for path, data, mode in files:
        atomic_write(path, data, mode)
    if refresh:
        for tool, args in (("update-desktop-database", [str(desktop.parent)]),
                           ("gtk-update-icon-cache", ["--force", "--ignore-theme-index",
                                                      str(data_home / "icons/hicolor")])):
            if shutil.which(tool):
                result = subprocess.run([tool, *args], capture_output=True, timeout=15)
                if result.returncode:
                    print("Optional desktop cache refresh failed: " + tool, file=sys.stderr)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "rust-client/target/release/zalo-rust")
    parser.add_argument("--data-home", type=Path,
                        default=Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))))
    parser.add_argument("--dry-run", action="store_true", help="Show exact files/hashes without writes")
    parser.add_argument("--no-refresh", action="store_true", help="Skip optional desktop cache tools")
    args = parser.parse_args()
    try:
        print(json.dumps(install(args.binary, args.data_home, args.dry_run, not args.no_refresh), indent=2))
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        parser.exit(1, "Native desktop installation failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
