import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[3]


class LauncherTests(unittest.TestCase):
    def test_live_selects_build_feature_preserves_cli_and_never_runs_a_failed_build(self):
        with tempfile.TemporaryDirectory(prefix="rust live launcher ") as directory:
            root = Path(directory)
            mock_bin = root / "bin"
            mock_bin.mkdir()
            target = root / "custom target"
            (target / "release").mkdir(parents=True)
            app = target / "release/zalo-rust"
            app.write_text("#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\n")
            app.chmod(0o755)
            cargo = mock_bin / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\nimport json,os,sys\n"
                "with open(os.environ['MOCK_LOG'], 'a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')\n"
                "if sys.argv[1] == 'build' and os.environ.get('MOCK_BUILD_FAILURE'): sys.exit(42)\n"
                "if sys.argv[1] == 'metadata': print(json.dumps({'target_directory':os.environ['CARGO_TARGET_DIR']}))\n"
            )
            cargo.chmod(0o755)
            log = root / "cargo.jsonl"
            env = os.environ.copy()
            env.update(PATH=str(mock_bin) + os.pathsep + env["PATH"], MOCK_LOG=str(log), CARGO_TARGET_DIR=str(target), ZALO_RUST_PROFILE="env-profile", ZALO_RUST_PROFILE_ROOT=str(root / "env root"))
            launcher = str(REPO_ROOT / "scripts/rust/run.sh")
            for options in (["--live"], ["--gui"], ["--profile", "cli-profile", "--live", "--trust-module", str(root / "trust module.wasm")], ["--demo", "--headless"]):
                log.unlink(missing_ok=True)
                result = subprocess.run([launcher, *options], cwd=root, env=env, check=True, capture_output=True, text=True)
                self.assertEqual(json.loads(result.stdout), ["--profile-root", str(root / "env root"), "--profile", "env-profile", *options])
                build, metadata = [json.loads(line) for line in log.read_text().splitlines()]
                self.assertEqual(build[0], "build")
                self.assertIn("--locked", build)
                self.assertIn("--release", build)
                if "--live" in options:
                    # Live builds carry the Signal engine so end-to-end encrypted chats can be used.
                    self.assertEqual(build[build.index("--features") + 1], "live,signal-crypto")
                else:
                    self.assertNotIn("--features", build, "ordinary/demo builds must keep network features disabled")
                self.assertEqual(metadata[0], "metadata")
                self.assertIn("--offline", metadata)
            # ZALO_RUST_NO_E2EE=1 opts out of the engine.
            log.unlink(missing_ok=True)
            subprocess.run([launcher, "--live"], cwd=root, env={**env, "ZALO_RUST_NO_E2EE": "1"}, check=True, capture_output=True, text=True)
            build = json.loads(log.read_text().splitlines()[0])
            self.assertEqual(build[build.index("--features") + 1], "live")
            env["MOCK_BUILD_FAILURE"] = "1"
            log.unlink()
            failed = subprocess.run([launcher, "--live"], cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(failed.returncode, 42)
            self.assertEqual(failed.stdout, "", "a failed live build must never execute a stale binary")
            self.assertEqual(len(log.read_text().splitlines()), 1)

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
