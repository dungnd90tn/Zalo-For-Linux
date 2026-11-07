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

### Stage 2 progress

Done: 564 no longer requires `submitTs` (`OwnKey::Present.submitted_at` is `Option<u64>`, as the original reads
it as `undefined`); the reconciliation state machine and coordinator prove the upload from the own 541 bundle
plus an unchanged `keyId`, and a proof simply carries no timestamp. Tests cover the real-service shape and a
changed `keyId`. User decision (2026-11-07): the account's existing PC key may be **replaced**; the Electron
profile is no longer needed.

Still missing before a key can be replaced from the live app:

1. **Activation transaction** in the E2EE store: after an `IdentityConfirmed` proof, move the pending records
   into the active namespace and make the new epoch current, in one SQL transaction guarded by the operation
   ID, payload hash and the previous epoch (`e2ee_register_local` is the existing verified-transition
   primitive; `VerifiedBinding.session_timestamp` must come from the 540 ACK `ts` when present, else the
   staged creation time, never a fabricated server time).
2. **Live driver** (opt-in, e.g. `ZALO_E2EE_PROVISION=replace-pc-key`, default off): reserve candidate →
   engine `CreateBundle` → stage → claim upload receipt → send 540 on the authenticated socket → on ACK or
   `Unknown`, run 564 → 541 → 564 reconciliation → activate. Restart while `Sending`/`Unknown` must reconcile
   query-only and never upload again.
3. Build: the engine lives behind `signal-crypto`; the shipped packages use `--features live` only.
