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

### Stage 4a: receive pipeline (implemented offline; not wired to the live socket)

`Coordinator::plan_receive` → `prepare_receive` → `MessageStore::e2ee_commit_receive`, mirroring the send pipeline:
one committed snapshot is read, the engine decrypts one personal ciphertext (prekey type 3 or ordinary type 1),
and the ratchet change, the consumed one-time prekey, the queue cursor and the plaintext message commit in one SQL
transaction. A first-contact sender is recorded from the prekey header (`prekey_sender`) before decrypting; the
header identity is unauthenticated until the engine verifies the message. `personal_ciphertext_from_wire` parses
`base64(type || body)`. Test `tests/signal_receive.rs` runs the real send pipeline in one store and the receive
pipeline in another: plaintext equals the original, the conversation appears with the real preview, and the
same ciphertext cannot be accepted again.

Not done yet (live wiring, each account-affecting): reading the 551 pushes (`msgs[]`: `content` ciphertext,
`e2ee` extension string, `uidFrom`, `paramsExt.platformType`, `msgId`, `ts`) and the `eesession` init pushes
(543/544/545) from the socket; trust for ordinary messages (needs the stored peer identity key); answering
`pendingUids` with 544; acknowledging queues (545/1 `info[]`) only after the commit; groups.

### Wire facts for the live receive wiring (from the Electron capture, `decode_ws_capture`; ids anonymized)

- **Pushed chat messages (`<- 551/0`, `data.msgs[]`)**: the ciphertext is `msgs[].content` (base64). Its leading
  byte is **2**, then `0x33`, then a protobuf (`0a`…): an ordinary message, 67/83/99 bytes (16-byte steps). The
  original treats `type == 3` as a prekey message and **anything else as ordinary**; `personal_ciphertext_from_wire`
  now does the same (it used to accept only 1 and 3, which would have rejected every phone message). The sender
  is `uidFrom`, the sender device is `paramsExt.platformType` (0 mobile, 1 web, 2 PC), the sender's registration id
  is in `msgs[].e2ee` (a JSON string: `{"version":1,"compat_version":0,"iid":<registration id>}`; this client
  writes `version 0`). `msgType` is `webchat` for text; a sticker arrives as `chat.sticker` with empty content.
  `msgs[].propertyExt.ext` (string) carries the separately encrypted text properties.
- **Receipts** (`<- 553/0`, `data.delivereds[]`) carry `deviceId`, `err`, `msgId`, `threadId`, `seen`; no cipher.
- **Session events** are acknowledged with **`545/1`** `{info:[{cmd, deviceId, gid, sid, subCmd}], queueName:""}`
  where `cmd` is the event's command (543, 544 or 548), `deviceId` is *this* device (2), `gid` is the event's id
  (11-digit) and `sid` the sending user id. The reply is `{err 0, reqId}`.
- **Pending sessions are acknowledged with 544** `{type:0, uidTo, data:[{deviceId, iid, ts}]}`: no ciphertext, just
  the peer device, its registration id (`iid`) and the `ts` from the `pendingUids` entry. The reply is
  `{err 0, reqId, ts}`.
- **The empty queues**: at 2.8 s the original requested 513, 516 and 518 (`{first:true, lastId, preIds:[], reqId}`);
  replies carry `eesession`, `msgs`, `delivereds`, `queueStatus.<cmd>_<sub>{evict,ids,lastId}`.

Remaining wiring, in order (each acknowledging step only after the commit): (1) handle `<- 551` pushes and the
`eesession` events: record the sender, decrypt, commit, emit the message; (2) send `545/1` acks for session
events and `544` for `pendingUids`; (3) ordinary messages need the stored peer identity key (read the
engine's `PeerIdentity` record); (4) the queues 513/516/518 on connect.

### Stage 4b: passive live receive (implemented; not yet run for real)

`src/app/live/runtime/e2ee_receive.rs`, off unless `ZALO_E2EE_RECEIVE` is an absolute report path:

```sh
ZALO_E2EE_RECEIVE="$HOME/e2ee-receive.txt" ./scripts/rust/run.sh --live
```

Each pushed `551` message from a peer is decrypted with the engine and committed in one SQL transaction, then
appears in the conversation list/chat like any received message. **It never sends an acknowledgement** (no 544,
no 545/1), so the service keeps redelivering; a redelivered ciphertext is recognised (`e2ee_ciphertext_seen`)
and skipped. A first-contact sender is recorded from the prekey header; a device already trusted with a
*different* identity is refused (identity-change handling is stage 5). Own-device messages, non-text messages
and the `eesession` init events are reported and left alone. For ordinary (type 1/2) messages the sender's
identity key is read back from the engine's `PeerIdentity` record (`stored_peer_identity`). The report lists only
outcomes (decrypted, skipped with a reason, not decrypted with the reason) and never content or ids.

How to try it: run the command above, then ask a contact who has E2EE with you to send you a text. Messages
sent before this client's key was registered (epoch 3) were encrypted to other keys and cannot be read; only new
ones can. Offline test: real engine and SQLite, a local socket pushing five frames; it asserts two messages are
shown in order with their text, a redelivery, an own-device message and a sticker are skipped, and the client
writes nothing back.

### Stage 4b, first real run: `uidFrom "0"` is a message from this account's own phone

The first run reported `message skipped: not a text message` and `message not decrypted: Invalid crypto account`.
The second was the sender id: in the Electron capture every pushed message that this account sent from another
device has **`uidFrom: "0"`** with the conversation's peer in **`idTo`** (messages from peers carry their own
uid). `"0"` is not a valid account id, hence the error. Handled now: `uidFrom "0"` means the sender is this
account (device from `paramsExt.platformType`, normally the phone), and the conversation is `d:<idTo>`; the
message is stored with this account as sender. `plan_receive` takes the conversation `thread` explicitly and only
refuses a message from *this device itself*. Own-device sessions were created by the init step, so they decrypt
with the existing session. The report now also prints a safe descriptor per message (`msgType`, id lengths,
platform, cipher kind, extension field names) and the whole error chain, to show what an unhandled message was.

### Stage 4b, second real run: `iid` is the recipient's registration id

The own-phone message (`uidFrom "0"`, an ordinary cipher of 132 chars) was refused with "this device presents a
different identity than the trusted one". Cause: the handler compared the extension's `iid` with the *sender's*
registration id. `iid` in `msgs[].e2ee` is the registration id of the key the message was encrypted **for** (the
recipient's, i.e. this device's; this client writes it the same way, plus `sender_iid` for its own). It is now
compared with this device's registration id instead: a mismatch is reported as "encrypted for a different key of
this device" (typically a message sent before the current key existed), not as an identity change. The sender's
own registration id comes from the prekey header or from the stored binding.

### Stage 3c: the handshake that starts a conversation (implemented; not yet run for real)

Receiving works because the peer opens the session: its init message reaches us and we answer. Starting a
conversation ourselves needs the other half, which the original sends before the first message to a new peer and
which this client lacked. `src/app/live/runtime/e2ee_init_peer.rs`, off unless `ZALO_E2EE_INIT_PEER` is an
absolute report path (`ZALO_E2EE_PEER="name"` picks the contact as the app shows it; unset means the first usable
contact of the account's E2EE list):

```sh
ZALO_E2EE_INIT_PEER="$HOME/e2ee-peer.txt" ZALO_E2EE_PEER="ban than" ./scripts/rust/run.sh --live
```

As captured from the original: 560 for the contact's devices, one 541 per device with `isReInit`, the engine
creates a session with every device while encrypting `{"subDevice":[]}`, sessions and exact bytes commit in one
SQL transaction, then **one 543** `{type:0, uidTo: contact, data:[{deviceId, data}]}` (one entry per device). The
contact's devices answer with a 544 push. A device that already has a session is left alone, so a repeat sends
nothing; a refused or unanswered 543 leaves the row `unknown` and is never resent. The init text never becomes a
chat message. This is the account-touching step the send probe deliberately avoided (the probe's `isReInit`
requests start sessions on the service without ever sending this 543).

Still missing for sending chat messages themselves: the signed HTTP POST to `/api/e2ee/pc/message/sms`, its reply
shape, the local echo, the composer routing and the lock UI.

### Stage 3c, first real run: a refused device must not drop the whole contact

Contact #1 answered `-7` for one device (the service's "user inactive for a long time"; the original ignores such
a device) and the run stopped with "no usable device", because the first service error ended the contact. A
refused device is now skipped with a report line and the contact's other devices still get the handshake; only a
contact with no usable device fails. Test: one device answers `-7`, the handshake carries only the other.

### Stage 3d: send one encrypted text (implemented; not yet run for real)

`src/app/live/runtime/e2ee_send.rs`, off unless `ZALO_E2EE_SEND` (report path) **and** `ZALO_E2EE_SEND_TEXT` are
set. **The text really goes to the chosen contact.** `ZALO_E2EE_PEER` selects the contact (name or `#number`);
without it the first contact that already has a session is used. It needs sessions with the contact's devices
(run `ZALO_E2EE_INIT_PEER` first; one run can set both, the handshake goes first):

```sh
ZALO_E2EE_INIT_PEER="$HOME/e2ee-peer.txt" ZALO_E2EE_SEND="$HOME/e2ee-send-text.txt" \
ZALO_E2EE_SEND_TEXT="Xin chào" ZALO_E2EE_PEER="#1" ./scripts/rust/run.sh --live
```

Pipeline: reserve the draft → plan one snapshot → the engine encrypts for every device that has a session (the
contact's, plus this account's other devices so the phone shows the message) → one SQL commit → authorize the
durable dispatch → POST the exact committed bytes, form-encoded as `params` (the session's AES layer, like the
legacy send), to `/api/e2ee/pc/message/sms` on the chat service → read `data.msgId`/`data.ts` → record the ack and
the local echo, then show the message. No reply, an unreadable reply or an unexpected shape leaves the row
`unknown`; the message is never encrypted or sent again. The report prints the reply's shape, which has never
been observed for this endpoint. Offline tests cover everything up to the POST (real engine and SQLite): sessions
come from the handshake, the plan covers the contact's device, the authorized path is
`/api/e2ee/pc/message/sms`, `destType 3`, `threadId`, one record, and the row is `sending`.

Not done: routing the composer to this path, the lock indicator, identity-change prompts, acknowledgements
(544, 545/1) and E2EE groups/files.

### Stage 3d, first real run: the post went to the wrong service

The report said the post failed with no readable reply. The original serves the E2EE endpoints from their own
service-map entries (`CHAT_E2EE: "chat_e2e"`, `CHAT_E2EE_GROUP: "group_e2e"`, N470113), not from `chat`; this
client posted to the plain chat service, and its config parser also dropped those two keys. They are now parsed
like the other optional services (validated origin, no effect when absent) and the post goes to `chat_e2e`; the
failure report now names the cause. Note: that first attempt left its row `unknown` (it is never resent), and
the duplicate-draft guard refuses the *same text* again in that conversation: use a different text for the retry.

### Parity read of the original's E2EE text send (so the next run is not a guess)

Read from the bundle (`sendZText` N331625–331650, `sendMsgE2ee` N399769, `_request`/`_post` N398344, `buildMsgParams`
N331488, `_enrichE2eeVersion`/`_enrichAdditionalParams` N331497–331510, `VERSION_MSG` N103754) and compared field by field:

| Original | This client |
|---|---|
| URL = domain of `CHAT_E2EE` (`chat_e2e`) + `/api/e2ee/pc/message/sms` + `?` + common params (`zpw_ver`, `zpw_type`) | same since the `chat_e2e` fix |
| body `params=` + `encodeURIComponent(encodeAES(JSON.stringify(u)))`, `Content-Type: application/x-www-form-urlencoded`, credentials | same (session AES layer, cookie) |
| `u = {listParams: JSON.stringify(records), destType: 3, threadId, clientId}` | same (`personal_sms`) |
| record = `{message, textProperties?, e2eeExt, toId, platform}` (`buildMsgParams`) | same |
| `e2eeExt` = `{version:0, compat_version:0}` for text, plus `iid` (recipient registration id) and `sender_iid` when the setting is on | same (always writes `sender_iid`) |
| encrypts `[text, stylesJSON-or-""]`; empty styles omitted | same |

No remaining difference found in the request. The reply handling (`msgId`, `ts`) is still unobserved, so the report prints
its shape.

### Correction (from the parity catalogs, `E2EE_PARITY.md`)

An earlier note in this file said pushed chat messages are acknowledged with `545/1`. That is wrong: `545` acknowledges **session events only**
(543/544/547-548/549/580/581). A received chat message is acknowledged with **`532 VERIFY_MSG`** `{nretry, queueName, ids:[{actionId}],
reqId}` (batched 20 ids / 30 s), and a *delivered receipt* goes out over HTTP (`/api/e2ee/pc/t/message/delivered`). Use `E2EE_PARITY.md` as
the checklist; this file stays as the chronological log.

### Send confirmation and local storage failure (2026-11-11)

Continued D10/D11 from `E2EE_PARITY.md`. The `minGlobalMsgId` parser and composer routing already existed before this milestone; earlier statements above that they were missing are historical. The synchronous send now checks a present response `clientId` against the durable attempt, accepts normalized numeric/string IDs, and refuses contradictory server IDs. An absent client echo keeps the existing HTTP-request binding. Invalid replies remain unknown and are never resent or re-encrypted automatically.

After a valid ACK, a failed local ACK/echo write now reports **Zalo accepted the message, local confirmation could not be saved**. The matching request enters delivery review with the draft retained, instead of presenting the outcome as unconfirmed. A persisted ACK survives echo failure; reconciliation still produces exactly one local message. Targeted offline evidence: 50 E2EE tests, two independent acknowledgement integration tests and ten exact original-reader cases pass. No real account was opened or sent to for this checkpoint.

At this checkpoint, automatic startup recovery and D2 per-device readiness were still pending. The following milestone implements the local recovery portion. This checkpoint does not enable provisioning, rotate identities, acknowledge receive queues, establish production interoperability or complete the rollout.

### Startup recovery of accepted local echoes (2026-11-11)

Startup now scans only the selected account's acknowledged chat sends, in client-ID keyset batches of at most 64, before publishing history. It excludes session-init IDs (`init-*`) and acknowledgements without a server ID. Each row uses the existing idempotent echo transaction; previously committed rows remain committed if a later row fails. The failed ACK stays available for a later startup, and the current pass stops instead of retrying indefinitely. This path never sends a packet, re-encrypts, retires receive queues or changes ready/sending/unknown rows.

The ACK stores no server timestamp, so the repaired echo retains an existing message time or uses the original draft creation time. Recovery validates bounded body/ID sizes before materializing content and returns progress only. The ordinary startup projection still controls account and hidden-conversation visibility; the existing remembered privacy policy is unchanged. No special recovery event exposes repaired content before that projection.

Targeted checks pass three store cases, three independent default-feature disk/restart cases and two local runtime cases (130 acknowledged rows across batches, privacy-filtered publication, and a SQLite failure with no network/publication). These are synthetic/local proofs, not real-server acceptance. `VALIDATION.md` records the broader gates. Next is D2 authoritative session coverage for each destination device, including stale/registration-ID changes and offline-queue ordering; the current any-session shortcut is still incomplete. The full E2EE rollout remains in progress.
