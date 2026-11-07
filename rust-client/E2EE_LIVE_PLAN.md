# Live end-to-end encryption: rollout plan

Status: **plan only, nothing here is active.** Today a conversation whose peer has E2EE enabled is refused
with "Hội thoại này đã bật mã hóa đầu cuối. Client chưa hỗ trợ gửi ở chế độ này" and the draft is kept.
That refusal is correct until the stages below are verified; it must never fall back to plain sending
silently, because the server would then hold the plaintext of a conversation the user believes is encrypted.

## What already exists (offline, tested, not connected to the run loop)

Embedded engine running the original Zalo Signal fork (`crates/zalo-signal-engine`), the E2EE store
(`STORE_E2EE.md`), the personal-send coordinator (`SIGNAL_CRYPTO.md`), pending-provisioning staging and the
query-only reconciliation child (`PROTOCOL_SIGNAL.md`), wire codecs for commands 540–564 and the SMS
envelopes, and original-JS oracles for interoperability. The production build uses `--features live`,
which excludes the engine.

## What is missing (from `PROTOCOL_SIGNAL.md`, "Provisioning and remaining integration")

Coordinator-owned WSS request routing, registration reconciliation, peer trust/epoch transitions, ordered
session queues, engine revision retries, atomic message/cursor/ratchet/outbox commits, receipt-gated ACKs,
exact-ciphertext dispatch, unknown-delivery reconciliation, group capability validation.

## Stages

Each stage ends with a gate that needs a real server, because the E2EE eligibility fix showed the service
differs from what offline reading predicts (it omits zero-valued fields). Later stages change account state
and are **not reversible**; they are the reason a dedicated test account is required.

| # | Stage | Touches the account? | Gate |
|---|---|---|---|
| 1 | **Read-only ground truth.** Route 564 and own-541 through the existing query-only child behind a CLI probe (`--e2ee-probe`, off by default). Log only field names and types. | Reads only | Probe output from a real account matches the parsers; fixtures updated from the observed shapes. |
| 2 | **Provisioning.** CreateBundle → pending epoch → 540 upload → activate on a correlated ACK. Opt-in switch, default off. | **Yes: registers a PC bundle** | Test account shows one active PC identity; restart does not re-upload; official clients still work. |
| 3 | **Personal send.** Fetch peer bundles (541), encrypt per destination device, `POST /api/e2ee/pc/message/sms`, durable outbox with unknown-delivery reconciliation. | Yes: sends messages | One message between two test accounts is read by the peer; retry never re-encrypts an accepted send. |
| 4 | **Personal receive.** 551/552 plus offline queues 516–519, decrypt, one transaction for message + ratchet + cursor, ACK 557/558 **only after** that commit. | **Yes: ACK deletes server queue entries** | A message sent from another client appears once; a forced crash before commit loses nothing. |
| 5 | **UX.** Lock indicator, identity-change prompt, clear states for "not provisioned", "peer has no bundle", "unknown delivery". Remove the refusal only for provisioned accounts. | No | Screenshot and flow tests; refusal still shown when the switch is off. |
| 6 | Groups (sender keys, 561, `/api/e2ee/pc/group/sms`) and E2EE files (`E2EEFile`, `secureKey`). | Yes | Separate plan after stage 5. |

## Rules that stay in force

- Never derive keys from IDs, never accept replacement identities silently, and reject the group signature
  branch that the original gets wrong (`E2EE_CONTRACT.md`).
- ACK or delete a server queue only after the durable commit of what it carries.
- No plaintext, ciphertext, key or URL in logs or diagnostics; categories and status codes only.
- Licensing: the engine's native primitives come from `libsignal-core` (AGPL-3.0). Settle the distribution
  obligation before shipping a package that includes it.

## Stage 1: how to run the read-only probe

Implemented in `src/app/live/runtime/e2ee_probe.rs`. It is off unless `ZALO_E2EE_PROBE` is an absolute path:

```sh
ZALO_E2EE_PROBE="$HOME/e2ee-probe.txt" ./scripts/rust/run.sh --live
```

Log in as usual (the same profile as the installed app). Once per process, right after authentication and
before any offline queue is requested, it sends command 564 (`{"platformType":2}`) and command 541 for the
account's own UID (`deviceId:-1`), waits up to 12 s, and writes `e2ee-probe.txt` (mode 0600): one line per
field as `path: type`, array lengths, and the value of only `error_code`, `err`, `deviceId`, `platformType`,
`isReInit` and `hasMore`. Keys, signatures, ids and timestamps are never written. A line starting `##` names
the command or says the reply was rejected before parsing (which is also an answer). The probe changes
nothing on the account; the only side effect is that frames arriving in those ≤12 s other than the two
replies are dropped, and the offline queues are fetched afterwards as usual.

What to look for: `data.data.err: number = -1` under command 564 means no key is uploaded for the PC
platform; a `keyId` and `submitTs` mean one is. Under command 541, `data.data.keys[0].keys` lists the
bundles per platform (`deviceId` 0 mobile, 1 web, 2 PC). Send the file back before stage 2 starts.

## Stage 1 result (first real-account run, 2026-11-07)

Both commands were answered with outer `error_code: 0`, so the frame decoder accepts them as-is.

| Observation | Consequence |
|---|---|
| 564 (`platformType:2`) returned `data.data = {err:0, keyId}` and **no `submitTs`**. | `own_key_reply` requires `submitTs` and would fail with `Invalid` on this server. The provisioning proof (`provisioning.rs`: positive, unchanged 564 timestamp before and after upload) cannot be satisfied as designed; stage 2 needs another proof, for example the account's own 541 bundle matching the locally generated public identity. Left unchanged until stage 2. |
| 564 reports a PC `keyId`, so **a PC key is already registered** for this account (most likely by the removed Electron app). | Stage 2 is a *rotation* of an existing identity, not a first registration. The old private keys live only in the Electron profile (`~/.config/ZaloData`) and are not readable by this client, so peers' sessions to the old identity will be replaced. Do not delete `~/.config/ZaloData` until that is decided. |
| 541 for the own UID returned one entry with `err:0`, `uid` and **two bundles**; the first has `deviceId:1`. Shape of a bundle: `preKey:{iKey:{id,key}, signature, spKey:{id,key}}`, `opKeys[{id,key}]`. | Matches `parse_bundle` exactly. The probe now lists up to four array elements, so a second run shows both `deviceId` values. |
| 541 `data.data` also carries `errData: array(0)`; the envelope has `error_message: ""`. | Extra fields; ignored by the current parsers. |

### Stage 2: how to run it (replaces the account's PC key)

Implemented, tested against a fake service with the real engine and SQLite; **not yet run against the real
service.** It is off unless `ZALO_E2EE_REPLACE_PC_KEY` is an absolute path, which is also where the report goes:

```sh
ZALO_E2EE_REPLACE_PC_KEY="$HOME/e2ee-replace.txt" ./scripts/rust/run.sh --live
```

`run.sh` then builds `--features live,signal-crypto` (the shipped packages still use `--features live`; a build
without the engine writes "not run" to the report instead of doing nothing silently). Once per process, right
after authentication and before any offline queue is requested:

1. If this client already holds the key (an active local epoch) it stops: nothing is sent.
2. Reserve a candidate → engine `CreateBundle` (OS CSPRNG, 100 one-time prekeys) → stage private keys and the
   exact public payload in SQLite → claim the upload (row becomes `sending`) → send command 540
   (`delSS:0`, like the original's renew path).
3. Read-only reconciliation: 564, then own 541, then 564. Only a 541 bundle for PC matching the new identity
   plus an unchanged `keyId` confirms it (no `submitTs` needed). An ACK alone never activates.
4. `e2ee_activate_pending`: one SQL transaction moves the pending records into the active namespace and makes
   the new epoch current, if the epoch planned against is still current. The candidate is then retired.

If the process dies or the socket drops after step 2, the row stays `unknown`. The next run **only reads** the
account to settle it (steps 3–4) and can never send a second 540. A server rejection or a mismatch ends in
`RESULT: stopped` with the reason; nothing is activated and no retry is attempted automatically.

The report lists the stages and, for each server reply, its *shape* (same redaction as the probe), so the 540
ACK shape is captured for the record. After activation this client holds the PC key but still cannot send or
receive E2EE messages (stages 3–4); messages encrypted to the new key wait unacknowledged in the server queue.

Consequences for others: peers' sessions to the old PC identity (the removed Electron app's) become invalid
and are re-established by them; the old Electron profile can no longer decrypt.

Remaining in stage 2: a real-account run and a look at the report before stage 3.

### Stage 2, first real run (2026-11-07): close code 1009

The first attempt sent command 540 with **100** one-time prekeys and the server closed the socket with
`1009` (message too big). The original uploads **20** (`_genBatchOPKeys(t, 20)`); the driver now does too. Nothing
was installed (the account kept its old PC key), but the candidate was left `unknown`, which blocked a retry.

New rule, implemented and tested: a restored (`unknown`) attempt may be **discarded** only when a read of the
account proves it was not installed (564 shows a different `keyId` or none, or 541 shows another identity under
our id). The store then deletes just the staged pending keys, their outbox row and the candidate
(`e2ee_abandon_unknown_candidate`, which requires state `unknown` and matching payload/attempt hashes); the
driver starts one fresh attempt in the same run and never a second. A fresh attempt that is ACKed but then not
confirmed by the reads is never auto-discarded. Run the same command again to continue.

### Stage 3a: send dry run (implemented; not yet run against the real service)

`src/app/live/runtime/e2ee_send_probe.rs`. Off unless both variables are set; needs the key replacement done:

```sh
ZALO_E2EE_SEND_PROBE="$HOME/e2ee-send.txt" ./scripts/rust/run.sh --live
```

No UID is needed. The peer is picked from this account's own E2EE list: the first contact by default, or add
`ZALO_E2EE_PEER="ban than"` with part of the name as the app shows it (alias included, accents and case ignored;
a numeric UID also works). The report never prints names or UIDs.

It reads the peer's bundles and this account's other devices' bundles (541, read-only; shapes go in the report),
records their identities on first use (an existing different identity is *refused*, never replaced), reserves a
draft, and lets the real engine encrypt `"e2ee probe"` for every device. The result is **not** committed, not
stored as a message and not sent; the draft row is removed. It proves the engine accepts the live service's
bundles (signatures, key derivation) before any real message exists. Offline test: real engine and SQLite
against a local socket, asserting only two 541 reads on the wire and no leftover draft.

Still to build for stage 3: commit + dispatch (`e2ee_commit_verified_send`, `e2ee_begin_dispatch`), the signed
HTTP POST to `/api/e2ee/pc/message/sms` with the service's reply shape, local echo (`e2ee_commit_echo`), routing
the composer to this path for provisioned accounts, and identity-change handling.

### Stage 3a, first real run (2026-11-07): every peer answers 541 with `-11`

The account's E2EE list has 32 contacts; a 541 for each of the first ten returned `err: -11` and `keys: []`
(own-account 541 works). In the original, `-11` belongs to the compatibility error table
(`ERR_COMPATIBLE`: `-11` I_AM_IN_BLACKLIST, `-10` USER_IN_BLACKLIST, `-9` my sub-devices have no bundle, `-7`
user inactive for a long time, `-5` my sub-devices unsupported, `-3` version, `-2` not in beta), which suggests
the service refuses *this requester* for peers. Because it is uniform, the probe now follows an all-refused run
with a read-only diagnosis: command 560 (compatibility) and a bundle request per device type for the first
three contacts. Hypotheses to separate: a requester-side gate (client type/version/platform) versus a
per-contact state; whether it began after the PC key was replaced is unknown. Run the probe again and read the
`## diagnosis` section before building the real send.

### What the original (Electron) client does, read from the bundle (N = shared worker bundle)

`reverse-engineering/` only covers the `db-cross-v4` backup addon; nothing about E2EE. Everything below is from
`default-login-main-startup-shared-worker-znotification.*.js`.

Device initialisation (`initialize`, N166020–166130), in this order:
1. 564 own key id → compare with the local key.
2. If new or renewed: 540 submit bundle (`delSS` = `enable_notify_delete_session`, default unknown). The 540 reply
   carries `ts`, which the client keeps as `_tsSubmitKey`; `keyExists`/`iid`/`devices` are ignored.
3. **`INIT_SUBDEVICE`** (`_initSessionSubDevices`, N164990s): for every other own device (`LIST_SUBDEVICES`
   minus self) → 541 for `(ownUid, deviceId)` with `isReInit` → build a session → encrypt an init message
   (`{"subDevice":[…]}`) → **543** `{type:1, uidTo: ownUid, data:[{deviceId, data:<cipher>}]}`. A failure here is
   retried by the client and is tracked as a pending point.
4. 563 list, then `ackSession` (544) for `pendingUids` (peers that already sent an init to this device).

Peer sessions (`initSessionWith`, N165170–165300): optionally 560 (`getCompatibleDevice`, replies
`data.compatible[{uid,deviceId}]`) then 541 either for `-1` (all devices; this is what the probe sends) or per
device, `buildSession`, then 543 `{type:1, uidTo: peer, data:[…]}`. Peers that initiated first show up in
`pendingUids` and need only the 544 ack, no bundle request.

Error tables: 541 (`ERROR_INIT`): `-1` unknown, `-2` key missing, `-3` client unsupported, `-5` my sub-devices
unsupported, `-7` user inactive, `-8` user blocked, `-9` my sub-devices have no bundle (with `errData`).
`ERR_COMPATIBLE` (560/562 family): `-11` I_AM_IN_BLACKLIST, `-10` USER_IN_BLACKLIST. `-11` is not a 541 code in
the client; the service returned it for every peer.

Difference from this client: the replacement stops after 540 and activation. It never performs step 3 (sessions
and 543 init messages with this account's other devices), and never 544-acks `pendingUids`. Whether the service
withholds peer bundles from a device that skipped step 3 is the leading hypothesis for `-11`, not a fact.

### Stage 3b: session initialization with this account's other devices (implemented; not yet run for real)

Added after reading the original (`INIT_SUBDEVICE` above). `src/app/live/runtime/e2ee_init_devices.rs`, off unless
`ZALO_E2EE_INIT_SUBDEVICES` is an absolute report path:

```sh
ZALO_E2EE_INIT_SUBDEVICES="$HOME/e2ee-init.txt" ./scripts/rust/run.sh --live
```

For each other device in the account's own bundle list: 541 with `isReInit`, the engine creates a session while
encrypting `{"subDevice":[]}`, the session and exact bytes are committed in one SQL transaction, then command
543 (`type 0`, `uidTo` = this account) delivers it. A 543 reply with an error, or none, leaves the row `unknown`
and nothing is resent. A device that already has a session is skipped, so a repeat run sends no 543. This touches
the account (your phone and web receive an init message), exactly as the original does for any new device.
The init rows never become chat messages (`e2ee_retire_init`). Run it, then run the send probe again to see
whether the peer `-11` goes away; if the init is refused, the report has the reply shape.

Both variables can be set in one run: the init runs first, then the send probe.

### Stage 3b, first real run (2026-11-07): init works, peers still `-11`

- Init with the account's other devices succeeded for both (web `1`, mobile `0`): 541 with `isReInit` returned
  one bundle each and 543 was accepted (`{data:{err:0,reqId,ts}}`, note: `err` is directly under `data`).
- The peer bundle requests still answered `-11` for every device (`0`, `1`, `2`) of every contact tried, and for
  the all-devices form. Command 560 for the same contacts returned `err:0`, `compatible:[{deviceId 0},{2},{1}]`,
  `incompatible:[]`. So the contacts are compatible with all device types, yet 541 refuses this requester.
- **The "skipped sub-device init" hypothesis is refuted.** Remaining candidates (none verified): a server-side
  gate on this client's identity (an "E2EE blacklist" of unofficial or flagged devices); an account-level E2EE
  setting; or state created by the key replacement (`delSS:0`). Reading the bundle cannot separate them; only a
  comparison with the official client's traffic on the same account can.
- Fixed while reading the log: the 543 refusal check looked at `data.data.err`; the real reply puts `err` at
  `data.err`, so a refusal would have been taken for success. Now both places are checked (test added).

### Ground truth: the original client on the wire (captured 2026-11-07)

Tool: `reverse-engineering/tools/electron_cdp_capture.mjs` records console output and every WebSocket frame of the
Electron client through the DevTools protocol (`--remote-debugging-port`); `cargo run --features live --example
decode_ws_capture -- <frames.jsonl>` decodes it with this repo's own frame decoder and prints shapes only (uids
become `uid#N`, no keys or ciphertext). The app's own logger does not reach the console. (Run Electron with
`env -u ELECTRON_RUN_AS_NODE`; the editor sets that variable and the binary then rejects its flags.)

What the original did, in order, after start (the three identical copies are three windows/workers):

1. **540** with **`delSS: 1`** (this client sent 0), `deviceId 2`, 20 one-time prekeys. Reply `err 0` in ~120 ms.
2. **563** (list) and **541** for the account's own devices 0 and 1 with `isReInit: 1`, one request per device.
3. The 563 reply carried **`pendingUids`**: eleven peer devices (`{uid, deviceId}`) that had already sent an init
   message toward this device after the new key (their sessions were reset by `delSS: 1`). The client answered
   **each with 544** `{type 0, uidTo, data:[{deviceId, …}]}` (`err 0` back), then sent **543** init messages
   (`type 0`) to its own devices 0 and 1. The server pushed 545/1 frames (peer inits) and 551/553 later.
4. **Peer bundles, later, on first use of a chat (11 s in):** for one peer the client sent **541 once per device
   (0, 1, 2), each with `isReInit: 1`** (`isAutoUpgrade 0`, `msgInfoNoti 0`); every reply was `err 0` with keys
   (~230 ms each). Then **one 543** `{type 0, uidTo: peer, data:[device 0, 1, 2]}` (`err 0`), then the server
   pushed **544** `eesession[{sessionData:{deviceId, type 1, uidFrom: peer, uidTo: me}}]` (the peer's reply), and
   messages arrived as 551.

Findings that change this client:
- **`-11` on peer 541 was our request, not the account:** this client asked with `isReInit: 0` (and with
  deviceId `-1`). The original always asked **per device with `isReInit: 1`** and got `err 0`. The probe and
  its diagnosis now do the same (560 for the device list, then one 541 per device with `isReInit`). Whether
  `isReInit: 1` alone fixes it is the next thing to confirm.
- `delSS: 1` is what the original sends; the key replacement now does too.
- Peers that already initialized toward the new key are waiting in `pendingUids`: they need **544 acks**, which
  require *receiving* their init messages (545/551 pushes: decrypt the prekey message, create the session, ack).
  Fetching bundles is only needed to *start* a conversation; for existing E2EE chats receiving comes first, so
  stage 4 (receive) is now the critical path, ahead of stage 3 (send).
- `isReInit: 1` is **not read-only**: it asks the service to start a session with that device. The send probe
  therefore touches peers (it does not send a message, and it never sends 543).

### Result of the run after the capture (2026-11-07): stages 2 and 3a confirmed against the real service

- Key replacement: the local epoch 2 was found stale (the Electron client had replaced the key), so the driver
  rotated to **epoch 3** with `delSS: 1`; the reads then confirmed it.
- Device init: sessions with web (1) and mobile (0) created, 543 accepted for both.
- **Peer bundles now succeed**: 560 for the device list, then 541 per device with `isReInit: 1` returns `err 0`
  with keys. So `-11` was caused by `isReInit: 0` / `deviceId -1`, as the capture suggested. One contact answered
  `-7` for a device (user inactive for a long time; the original maps it to "ignore this device"), another
  contacts were fine. The first usable contact had devices 0 and 2.
- **The engine encrypted the probe for 4 devices** (2 of the peer, 2 of this account) from live bundles: the
  signature checks and key derivation interoperate with the service's real bundles. Nothing was sent.

Next, in this order (receive first, because peers that reset their sessions after `delSS: 1` already wait in
`pendingUids`): (4) receive init messages (545 and 551 pushes), create sessions, answer with 544; decrypt chat
messages; then (3) send via `/api/e2ee/pc/message/sms`, then the composer and the lock UI.
