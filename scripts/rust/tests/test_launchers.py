import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[3]


class LauncherTests(unittest.TestCase):
    def test_launchers_preserve_arguments_pinned_toolchain_directory_and_target_path(self):
        with tempfile.TemporaryDirectory(prefix="rust launcher ") as directory:
            root = Path(directory)
            mock_bin = root / "bin"
            mock_bin.mkdir()
            target = root / "target with spaces"
            (target / "release").mkdir(parents=True)
            app = target / "release/zalo-rust"
            app.write_text("#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps({'pid':os.getpid(),'args':sys.argv[1:]}))\n")
            app.chmod(0o755)
            cargo = mock_bin / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\nimport json,os,sys\n"
                "with open(os.environ['MOCK_LOG'], 'a') as f: f.write(json.dumps({'cwd':os.getcwd(),'args':sys.argv[1:]})+'\\n')\n"
                "if sys.argv[1] == 'metadata': print(json.dumps({'target_directory':os.environ['CARGO_TARGET_DIR']}))\n"
            )
            cargo.chmod(0o755)
            env = os.environ.copy()
            env.update({
                "PATH": str(mock_bin) + os.pathsep + env["PATH"],
                "MOCK_LOG": str(root / "cargo.jsonl"),
                "CARGO_TARGET_DIR": str(target),
                "ZALO_RUST_PROFILE_ROOT": str(root / "profile root"),
                "ZALO_RUST_PROFILE": "pilot",
            })
            process = subprocess.Popen(
                [str(REPO_ROOT / "scripts/rust/run.sh"), "--demo", "--headless"],
                cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stderr)
            report = json.loads(stdout)
            self.assertEqual(report["pid"], process.pid, "launcher must exec the actual application")
            self.assertEqual(report["args"], ["--profile-root", str(root / "profile root"), "--profile", "pilot", "--demo", "--headless"])
            subprocess.run([str(REPO_ROOT / "scripts/rust/build.sh"), "--offline"], cwd=root, env=env, check=True, capture_output=True)
            calls = [json.loads(line) for line in (root / "cargo.jsonl").read_text().splitlines()]
            self.assertEqual([call["args"][0] for call in calls], ["build", "metadata", "build"])
            for call in calls:
                self.assertEqual(call["cwd"], str(REPO_ROOT / "rust-client"))
                self.assertIn("--locked", call["args"])
                manifest_index = call["args"].index("--manifest-path") + 1
                self.assertEqual(call["args"][manifest_index], str(REPO_ROOT / "rust-client/Cargo.toml"))
            self.assertIn("--offline", calls[-1]["args"])
            self.assertFalse((root / "profile root").exists(), "tooling itself must not mutate profile contents")
            env.pop("ZALO_RUST_PROFILE")
            demo = subprocess.run([str(REPO_ROOT / "scripts/rust/run.sh"), "--demo", "--headless"], cwd=root, env=env, check=True, capture_output=True, text=True)
            args = json.loads(demo.stdout)["args"]
            self.assertNotIn("--profile", args, "the CLI owns demo/pilot profile defaults")
            env.pop("ZALO_RUST_PROFILE_ROOT")
            default = subprocess.run([str(REPO_ROOT / "scripts/rust/run.sh"), "--demo", "--headless"], cwd=root, env=env, check=True, capture_output=True, text=True)
            self.assertEqual(json.loads(default.stdout)["args"], ["--demo", "--headless"], "launcher and direct binary must share the same XDG paths")
            env.update(ZALO_RUST_PROFILE="env-profile", ZALO_RUST_PROFILE_ROOT=str(root / "env-root"))
            explicit = subprocess.run([str(REPO_ROOT / "scripts/rust/run.sh"), "--profile", "cli-profile", "--profile-root", str(root / "cli-root"), "--doctor"], cwd=root, env=env, check=True, capture_output=True, text=True)
            self.assertEqual(json.loads(explicit.stdout)["args"], ["--profile-root", str(root / "env-root"), "--profile", "env-profile", "--profile", "cli-profile", "--profile-root", str(root / "cli-root"), "--doctor"], "explicit CLI flags must follow environment defaults")


if __name__ == "__main__":
    unittest.main()
