# Private diagnostic logs

Normal launches automatically record lifecycle, connection transitions and fixed
failure information. `--debug-log` additionally enables structural debug events;
it is not required to diagnose failures. The startup line on stderr prints the
selected log path.

Find the live profile's path without opening a profile or starting networking:

```sh
./rust-client/target/release/zalo-rust --doctor --profile rust-live
```

Read `diagnostic_log_path` in the JSON result. With XDG defaults it resolves to
`$XDG_CACHE_HOME/zalo-rust/profiles/rust-live/diagnostics/native.log`, or
`$HOME/.cache/zalo-rust/profiles/rust-live/diagnostics/native.log` when the XDG
variable is unset. Explicit `--profile-root /absolute/root` resolves to
`/absolute/root/rust-live/cache/diagnostics/native.log`. Use the same profile and
root options as the launch being diagnosed.

Logs are JSON lines. They contain fixed event/stage/reason tags, numeric protocol
command/subcommand, accepted encryption tags, bounded byte counts, HTTP/server
codes and envelope value types such as `absent`, `null`, or `object`. They never
accept raw error strings, URLs, cookies, QR material, account/message IDs, keys or
message bodies. Unknown numeric server codes outside the diagnostic range are
marked explicitly rather than confused with an absent error code.

The startup record identifies the actual executable by package version, feature
bits, byte size, modification time and SHA-256. Feature bits are network=1,
live-login=2, messaging=4 and signal-store=8. Hashing runs on the log writer thread.

The directory is owner-only (0700), files are owner-only (0600), and rotation keeps
`native.log` plus `.1`, `.2`, `.3`, each at most 256 KiB. One writer drains a bounded
256-event queue; overloaded logging drops events and reports a count. Filesystem
I/O never runs in event recording calls. Shutdown drains the remaining bounded
queue. Oversized, symlinked, hard-linked or public log files are refused without
following/truncating them. The client continues with a fixed stderr warning if
logging cannot initialize; a writer failure also prints a fixed warning.

`--help`, `--version`, `--doctor` and SQLite inspection do not initialize the
logger. Diagnostics do not establish authentication, delivery or feature parity.
