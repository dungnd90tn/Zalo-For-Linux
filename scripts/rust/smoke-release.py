#!/usr/bin/env python3
"""Check the built native binary using synthetic, automatically removed profiles."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path(__file__).resolve().parents[2] / "rust-client/target/release/zalo-rust")
    parser.add_argument("--expect-live", action="store_true", help="require optional login/messaging features, while exercising only diagnostics and offline demo")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    env = os.environ.copy()
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    with tempfile.TemporaryDirectory(prefix="zalo-release-smoke-") as directory:
        root = Path(directory)
        command = [str(binary), "--profile-root", str(root), "--profile", "release-smoke"]

        def run(*options, check=True):
            return subprocess.run(command + list(options), env=env, capture_output=True, text=True, check=check, timeout=15)

        run("--help")
        run("--version")
        doctor = json.loads(run("--doctor", "--debug-log").stdout)
        require(not any(root.iterdir()), "informational commands created profile files")
        log_path = root / "release-smoke/cache/diagnostics/native.log"
        require(Path(doctor["diagnostic_log_path"]).is_absolute() and Path(doctor["diagnostic_log_path"]) == log_path, "doctor did not report the selected profile's absolute log path")
        require(not any(doctor[name] for name in ("live_auth_available", "live_send_available", "history_import_available", "display_available")), "capability diagnostics falsely claim availability")
        require(not doctor["authenticated"] and not doctor["network_started"], "diagnostics activated a session or network")
        require(doctor["live_auth_compiled"] == doctor["live_messaging_compiled"] == args.expect_live, "binary feature configuration differs from the expected build")
        if not args.expect_live:
            unsupported = run("--live", check=False)
            require(unsupported.returncode != 0 and "does not include live login" in unsupported.stderr, "default binary accepted live mode")
            require(not any(root.iterdir()), "unsupported live mode created a profile")
        results = [json.loads(run("--demo", "--headless").stdout) for _ in range(2)]
        for result in results:
            require(result["mode"] == "offline-demo" and result["network_used"] is False and result["live_messages_sent"] == 0, "demo reports an unexpected live capability")
        require(results[0]["messages_loaded"] == results[1]["messages_loaded"] == 2, "restart duplicated or lost seeded messages")
        data = root / "release-smoke/data"
        with sqlite3.connect(f"file:{data / 'messages.sqlite3'}?mode=ro", uri=True) as conn:
            require(conn.execute("PRAGMA user_version").fetchone()[0] == 7, "release did not initialize the compatible schema-seven database")
            count, accounts = conn.execute("SELECT count(*), count(DISTINCT account_id) FROM messages").fetchone()
            threads = conn.execute("SELECT count(*) FROM conversations").fetchone()[0]
            originals = conn.execute("SELECT * FROM messages WHERE message_id IN ('demo-1','demo-2','demo-3') ORDER BY message_id").fetchall()
        require((count, accounts, threads) == (19, 1, 6), "repeated demo startup changed seeded message identity/count")
        require(len(originals) == 3, "legacy demo messages disappeared")
        # Simulate an existing v1 profile plus one user-created local message.
        # Reuse the real profile/schema and verify additive migration preserves
        # original immutable bodies and local history, rather than resetting it.
        with sqlite3.connect(data / "messages.sqlite3") as conn:
            conn.execute("DELETE FROM messages WHERE thread_id NOT IN ('welcome','architecture')")
            conn.execute("DELETE FROM conversations WHERE thread_id NOT IN ('welcome','architecture')")
            conn.execute("INSERT INTO messages(account_id,thread_id,message_id,sender,body,sent_at_ms) VALUES ('0','welcome','qa-legacy-preserved','Local smoke','Existing local history',1700000000002)")
        for _ in range(2):
            require(json.loads(run("--demo", "--headless").stdout)["messages_loaded"] == 3, "legacy local history was not loaded after upgrade")
        with sqlite3.connect(f"file:{data / 'messages.sqlite3'}?mode=ro", uri=True) as conn:
            upgraded_count = conn.execute("SELECT count(*) FROM messages").fetchone()[0]
            upgraded_threads = conn.execute("SELECT count(*) FROM conversations").fetchone()[0]
            upgraded_originals = conn.execute("SELECT * FROM messages WHERE message_id IN ('demo-1','demo-2','demo-3') ORDER BY message_id").fetchall()
            local_history = conn.execute("SELECT body FROM messages WHERE account_id='0' AND thread_id='welcome' AND message_id='qa-legacy-preserved'").fetchall()
        require((upgraded_count, upgraded_threads) == (20, 6), "legacy upgrade was not additive and idempotent")
        require(upgraded_originals == originals and local_history == [("Existing local history",)], "legacy upgrade changed existing message contents")
        with (data / ".profile.lock").open("r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            refused = run("--demo", "--headless", check=False)
            require(refused.returncode != 0 and "already in use" in refused.stderr, "second process did not refuse the profile lock")
        require((data.stat().st_mode & 0o777) == 0o700, "profile data directory is not private")
        require(((data / ".account-owner.json").stat().st_mode & 0o777) == 0o600, "account owner marker is not private")
        require((log_path.parent.stat().st_mode & 0o777) == 0o700, "diagnostic directory is not private")
        slots = list(log_path.parent.iterdir())
        require(1 <= len(slots) <= 4 and all(path.name in {"native.log", "native.log.1", "native.log.2", "native.log.3"} for path in slots), "unexpected diagnostic rotation files")
        events = []
        allowed_fields = {"ts_ms", "kind", "stage", "reason", "http_status", "server_code", "server_code_out_of_range", "bytes", "command", "command_id", "subcommand", "encryption", "error_code_type", "data_type", "field_type", "field_bytes", "message_kind", "storage_operation", "storage_failure", "sqlite_code", "storage_mismatch_mask", "directory_stage", "directory_category", "preview_field", "build"}
        for path in slots:
            require(path.is_file() and not path.is_symlink() and (path.stat().st_mode & 0o777) == 0o600, "diagnostic slot is not a private regular file")
            require(path.stat().st_size <= 256 * 1024, "diagnostic slot exceeds rotation budget")
            for line in path.read_text().splitlines():
                event = json.loads(line)
                require(set(event) <= allowed_fields and {"ts_ms", "kind", "stage", "reason"} <= set(event), "diagnostic record exposes unstructured fields")
                events.append(event)
        startup = [event for event in events if event["kind"] == "lifecycle" and event["stage"] == "startup"]
        require(startup and any(event["stage"] == "shutdown" for event in events), "normal process did not log its lifecycle")
        expected_build = {"version", "features", "executable_bytes", "executable_mtime_s", "executable_sha256"}
        executable_hash = hashlib.sha256()
        with binary.open("rb") as executable:
            for chunk in iter(lambda: executable.read(1024 * 1024), b""):
                executable_hash.update(chunk)
        digest = executable_hash.hexdigest()
        require(all(set(event["build"]) == expected_build and event["build"]["executable_bytes"] == binary.stat().st_size and event["build"]["executable_sha256"] == digest and event["build"]["features"] == (7 if args.expect_live else 0) for event in startup), "diagnostic executable identity does not match the actual build")
        require(all("Existing local history" not in path.read_text() for path in slots), "diagnostics leaked synthetic message contents")
        # Verify main's fallback, not merely the logger's refusal of unsafe paths.
        # The sentinel contains no account data and must never be replaced.
        log_path.parent.rename(log_path.parent.with_name("diagnostics-preserved"))
        sentinel = b"qa-path-must-remain-untouched"
        log_path.parent.write_bytes(sentinel)
        unavailable = run("--demo", "--headless")
        require(json.loads(unavailable.stdout)["network_used"] is False, "unavailable logging prevented local operation")
        require("Diagnostic log unavailable: private path or I/O failure. Intended log: " + str(log_path) in unavailable.stderr, "unavailable logging did not report its fixed warning and intended path")
        require(log_path.parent.read_bytes() == sentinel and sentinel.decode() not in unavailable.stderr, "logger fallback exposed or changed the obstructing path contents")
        print(json.dumps({"binary_bytes": binary.stat().st_size, "live_features_compiled": args.expect_live, "informational_commands_no_writes": True, "diagnostic_log_path_absolute": True, "diagnostic_lifecycle_and_build_identity": True, "diagnostic_files_private_and_bounded": True, "diagnostic_failure_keeps_local_app_usable": True, "schema_version": 7, "restart_idempotent": True, "seeded_messages": count, "conversations": threads, "legacy_upgrade_preserved_originals_and_local_history": True, "external_lock_refused": True, "profile_permissions_private": True, "network_used": False}))


if __name__ == "__main__":
    main()
