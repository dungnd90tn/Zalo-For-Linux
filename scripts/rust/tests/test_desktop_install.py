import ctypes
import ctypes.util
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "native_desktop_install", Path(__file__).resolve().parents[1] / "install-desktop.py")
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class DesktopInstallTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="zalo-desktop-test-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.binary = self.root / "source-native"
        self.binary.write_bytes(b"\x7fELFsynthetic-not-executed")
        self.binary.chmod(0o755)
        self.data = self.root / "xdg-data"

    def test_native_identity_original_icons_and_repeat_install_preserve_electron(self):
        electron = self.data / "applications/Zalo.desktop"
        electron.parent.mkdir(parents=True)
        electron.write_text("Existing Electron entry")
        icon = self.data / "icons/zalo.png"
        icon.parent.mkdir()
        icon.write_bytes(b"Existing Electron icon")
        result = installer.install(self.binary, self.data, refresh=False)
        self.assertFalse(result["launches_app"])
        desktop = self.data / "applications/zalo-linux-rust-preview.desktop"
        self.assertIn("Icon=zalo-linux-rust-preview\n", desktop.read_text())
        self.assertIn("StartupWMClass=zalo-linux-rust-preview\n", desktop.read_text())
        self.assertEqual(electron.read_text(), "Existing Electron entry")
        self.assertEqual(icon.read_bytes(), b"Existing Electron icon")
        for size in (256, 512):
            original = installer.ROOT / "pc-dist" / f"favicon-{size}x{size}.png"
            installed = self.data / "icons/hicolor" / f"{size}x{size}/apps/zalo-linux-rust-preview.png"
            self.assertEqual(hashlib.sha256(original.read_bytes()).digest(),
                             hashlib.sha256(installed.read_bytes()).digest())
        self.binary.write_bytes(b"\x7fELFupdated-synthetic")
        installer.install(self.binary, self.data, refresh=False)
        self.assertEqual((self.data / "zalo-linux-rust-preview/bin/zalo-rust").read_bytes(),
                         self.binary.read_bytes())
        self.assertEqual(electron.read_text(), "Existing Electron entry")

    def test_dry_run_does_not_create_data_or_launch_an_executable(self):
        result = installer.install(self.binary, self.data, dry_run=True)
        self.assertEqual(len(result["files"]), 4)
        self.assertFalse(self.data.exists())
        self.assertFalse(result["launches_app"])

    def test_symlink_and_unmanaged_entry_are_rejected_before_any_install(self):
        target = self.root / "unrelated"
        target.mkdir()
        self.data.mkdir()
        (self.data / "icons").symlink_to(target, target_is_directory=True)
        with self.assertRaises(ValueError):
            installer.install(self.binary, self.data, refresh=False)
        self.assertFalse((self.data / "zalo-linux-rust-preview").exists())
        (self.data / "icons").unlink()
        desktop = self.data / "applications/zalo-linux-rust-preview.desktop"
        desktop.parent.mkdir()
        desktop.write_text("User-managed entry")
        with self.assertRaises(ValueError):
            installer.install(self.binary, self.data, refresh=False)
        self.assertEqual(desktop.read_text(), "User-managed entry")

    def test_rendered_exec_quoting_matches_real_glib_launch_of_inert_fixture(self):
        data = self.root / 'tên space $dollar `tick` \\back "quote"'
        compiler = shutil.which("cc")
        if compiler is None:
            self.skipTest("C compiler required for the inert ELF launch fixture")
        # The tiny ELF only checks argv; it has no client/account/network code.
        fixture = '#include <stdio.h>\n#include <string.h>\nint main(int n,char**v){if(n!=2||strcmp(v[1],"--live"))return 9;puts(v[1]);return 0;}\n'
        subprocess.run([compiler, "-x", "c", "-o", str(self.binary), "-"],
                       input=fixture, text=True, check=True, capture_output=True, timeout=15)
        installer.install(self.binary, data, refresh=False)
        desktop = data / "applications/zalo-linux-rust-preview.desktop"
        if shutil.which("desktop-file-validate"):
            subprocess.run(["desktop-file-validate", str(desktop)], check=True,
                           capture_output=True, timeout=5)
        library = ctypes.util.find_library("gio-2.0")
        if library is None:
            self.skipTest("GLib desktop parser is unavailable")
        gio = ctypes.CDLL(library)
        gio.g_desktop_app_info_new_from_filename.argtypes = [ctypes.c_char_p]
        gio.g_desktop_app_info_new_from_filename.restype = ctypes.c_void_p
        gio.g_app_info_get_executable.argtypes = [ctypes.c_void_p]
        gio.g_app_info_get_executable.restype = ctypes.c_char_p
        gio.g_object_unref.argtypes = [ctypes.c_void_p]
        info = gio.g_desktop_app_info_new_from_filename(os.fsencode(desktop))
        self.assertTrue(info, "GLib rejected generated desktop identity/Exec")
        try:
            self.assertTrue(gio.g_app_info_get_executable(info))
            callback_type = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p)
            children = []
            callback = callback_type(lambda _info, pid, _data: children.append(pid))
            launch = gio.g_desktop_app_info_launch_uris_as_manager_with_fds
            launch.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                               ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p,
                               ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int,
                               ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_void_p)]
            launch.restype = ctypes.c_int
            error = ctypes.c_void_p()
            with tempfile.TemporaryFile() as output, mock.patch.dict(
                    os.environ, {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent"}):
                self.assertTrue(launch(info, None, None, 2, None, None,
                                       ctypes.cast(callback, ctypes.c_void_p), None,
                                       -1, output.fileno(), output.fileno(), ctypes.byref(error)))
                self.assertEqual(len(children), 1)
                status = os.waitpid(children[0], 0)[1]
                output.seek(0)
                captured = output.read()
                self.assertEqual(status, 0, captured)
                self.assertEqual(captured, b"--live\n")
        finally:
            gio.g_object_unref(info)

    def test_unsupported_exec_path_rejects_before_writing(self):
        for suffix in ("bad\npath", "bad\tpath", "bad%path", "bad=path"):
            data = self.root / suffix
            with self.assertRaises(ValueError):
                installer.install(self.binary, data, refresh=False)
            self.assertFalse(data.exists())


if __name__ == "__main__":
    unittest.main()
