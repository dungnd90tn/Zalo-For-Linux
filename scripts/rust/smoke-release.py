#!/usr/bin/env python3
"""Check the built native binary using synthetic, automatically removed profiles."""

import argparse
import fcntl
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
        doctor = json.loads(run("--doctor").stdout)
        require(not any(root.iterdir()), "informational commands created profile files")
        require(not any(doctor[name] for name in ("live_auth_available", "live_send_available", "history_import_available", "display_available")), "capability diagnostics falsely claim availability")
        results = [json.loads(run("--demo", "--headless").stdout) for _ in range(2)]
        for result in results:
            require(result["mode"] == "offline-demo" and result["network_used"] is False and result["live_messages_sent"] == 0, "demo reports an unexpected live capability")
        require(results[0]["messages_loaded"] == results[1]["messages_loaded"] == 2, "restart duplicated or lost seeded messages")
        data = root / "release-smoke/data"
        with sqlite3.connect(f"file:{data / 'messages.sqlite3'}?mode=ro", uri=True) as conn:
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
        print(json.dumps({"binary_bytes": binary.stat().st_size, "informational_commands_no_writes": True, "restart_idempotent": True, "seeded_messages": count, "conversations": threads, "legacy_upgrade_preserved_originals_and_local_history": True, "external_lock_refused": True, "profile_permissions_private": True, "network_used": False}))


if __name__ == "__main__":
    main()
