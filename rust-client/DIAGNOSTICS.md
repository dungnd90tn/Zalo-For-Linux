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

Receive processing also reports closed field/batch reasons, such as
`message_client_id`, `message_content`, `message_sender`, `queue_continuation`
and `queue_progress`. Optional `field_type` describes the rejected JSON type;
`field_bytes` is a text byte count capped at 65,537. `message_kind` is restricted
to `webchat`, `webchat_info`, `photo`, `web_content`, `recommended`, `poll`, `file`, `sticker` or `other`. These fields identify a parser
boundary without retaining the field value, arbitrary message type or content.
They are recorded for failures even without `--debug-log`. A prior generic
`synchronization/invalid` record cannot establish which field failed.

Partial directory failures include closed `directory_stage` labels (`account`, `friends`,
`groups`, `previews`, `storage`, `synchronization`) and `directory_category`
labels (`unavailable`, `network`, `timeout`, `http`, `server`, `crypto`, `schema`,
`resource`, `cancelled`, `storage`). Such a failure can leave the authenticated
socket Online while one directory stage remains incomplete; it does not by
itself prove an offline connection or identify the private rejected payload.
Media download/decode failures use the separate `media_assets` stage with fixed
reasons and bounded status/byte fields, never a URL or raw codec error.

Send eligibility uses fixed `capability_encrypted`, `capability_schema`,
`capability_correlation`, `capability_timeout`, `capability_changed` and
`capability_pagination` reasons. Optional `capability_field` identifies a closed
parser boundary: `envelope`, `request`, `status`, `more`, `threads`, `waiting`,
`uids`, `pending`, `cursor`, `group`, `encryption` or `visibility`. `field_type`
records only its JSON type; command 563 or an intervening command may be attached.
No target, request ID, list member, cursor value, PIN or message enters the event.
Confirmed E2EE is distinct from unknown metadata; `capability_changed` means an
intervening encrypted event invalidated this check, not proof that the selected
peer changed encryption. These failures are logged without `--debug-log`.
An older `send/unsupported` line cannot distinguish those causes retroactively.

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
