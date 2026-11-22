# Native messaging wire evidence

Implementation is independently authored from the current local PC JavaScript bundles. No external Rust client source is included. The network feature is opt-in (`--features live`, `--live`); offline demo never constructs a live session.

## Local evidence

All line references below refer to `pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js` in this checkout.

- `399824–399841`: legacy text POST `/api/message/sms` (command 11151), `/api/group/sendmsg` (11163). Session AES-CBC encrypted `params` form field; plaintext JSON contains `message`, numeric `clientId`, `imei`, and either `toid` or `grid` plus server-derived `visibility` (ordinary-group default 0; getter evidence `156861–156865`). Common query `zpw_ver` / `zpw_type`: `398136–398140`, construction `398186–398196`. This format has no extra request signature.
- `440600–440625`: HTTP success requires both outer `error_code: 0` and a successfully session-decrypted inner `error_code: 0`. An HTTP 200 alone is insufficient. Native send acceptance requires an actual `data.msgId`; acceptance does not claim delivery/read by the recipient.
- `37661–37667`, `37971–37994`: socket binary header is version byte, little-endian u16 command, subcommand byte, followed by UTF-8 JSON.
- `400376–400379`, `37903`: preserve the advertised WebSocket path, append `zpw_ver` / `zpw_type` and then timestamp `t`; the endpoint itself must have no query. HTTP chat/group service entries remain origin roots. Desktop headers follow `main-dist/main.js:110701` (`ZaloPC-linux-24v681`, with the existing Linux type mapping at `109415–109433`), and both the login and main renderer receive type 24 (`main-dist/main.js:110055`, `110085`, `110223`; renderer URL override `228138`). Authentication parameter encryption, signed bootstrap, directory/message HTTP and WebSocket queries now share this explicit client identity. Historical native type-30 records require fresh phone approval before transport; their cookie is never silently retargeted.
- `37712–37745`, `37824–37870`: outer encryption 1 is base64 + zlib; 2 is URL-decoded base64 AES-GCM + zlib; 3 is AES-GCM without zlib. GCM uses 16-byte IV, 16-byte additional authenticated data, remaining ciphertext plus 16-byte tag. The socket AUTH response supplies its own key, distinct from the HTTP session key. The inner success envelope establishes the socket session; key presence alone does not.
- `37661–37667`, `37860–37875`: the first incoming header byte is decoded as metadata and passed onward without requiring value 1. Commands remain little-endian u16. `37793–37831` calls bundled Pako `inflate`, whose default gzip/zlib autodetection is explicit at `vendors-login-main-startup-shared-worker-znotification.9115fa5ca1824a42c293.js:94406`. Native accepts bounded gzip and zlib, checks checksum and rejects trailing bytes. `tests/fixtures/messaging/auth_oracle.cjs` executes the original socket methods, original queue-auth handler and bundled Pako: header 0/2, gzip mode 1, GCM+gzip mode 2 and GCM mode 3 fixtures succeed. Missing keys, rejected inner/outer success codes and plaintext AUTH do not authenticate. `404771–404784` requires nonempty `data.qCmds` for usable chat initialization; missing, empty and non-array queues never publish Online.
- `37714–37745`: PONG 2 and lightweight command 4 dispatch before ordinary envelope/decryption gates. Only those controls accept absent/null `error_code`; nonnull status remains validated. Command 4 is the sole lightweight API entry (`248976`), correlated as `cmd_4` (`464369–464390`); native does not originate it or acknowledge queues from it. Command 563 follows normal decryption and inner-success parsing (`92313–92324`, `464383–464394`). `post_auth_oracle.cjs` executes the original methods for 18 control, encrypted message, layered API and rejection cases. This reproduces a compatibility bug without capturing the user's actual failing frame.
- `248922–248940`: AUTH 1, heartbeat 2, personal push 501, group push 521, missing-message 534, message retrieval 570. `404409–404425` defines `msgs` / `groupMsgs` / `pageMsgs`, sender and recipient routing. Native threads use `d:<uid>` and `g:<grid>` to avoid kind collisions.
- `116859–116931`: server-advertised offline queues, `first`, `reqId`, `lastId`, `preIds`, `more`. `48716–48740`: per-queue `queueStatus.lastId`. For `more` pages, `116903–116918` explicitly passes `data.lastActionId` into the next request. Native continuation honors that value even when `queueStatus` is absent, persists a fallback cursor only after message commit, rejects nonadvancing cursors, and caps one offline queue at 10,000 pages. A failed message commit cannot advance a cursor or request the next page.

## Encryption capability boundary

Personal plaintext eligibility is derived afresh before each send from request-correlated command 563, using the original paginated `reqIid`, `first`, `lastId` request (`404176–404220`). Native requires explicit `err: 0` and all four arrays `threads`, `waitUids`, `uids`, `pendingUids`. Any occurrence of the target forbids a legacy send. The original reader treats absent/null/false/zero `hasMore` as terminal; native accepts those forms and true/one with a valid continuation ID, rejecting strings and other unknown types. Cursor values are forwarded without assuming numeric order; repeats, incomplete pagination, the 100-page limit or the 15-second response budget grant no permission. An intervening encrypted event conservatively invalidates eligibility and is neither consumed nor acknowledged.

`tests/fixtures/messaging/eligibility_oracle.cjs` executes the exact original pagination method for eleven synthetic cases and verifies command 563 against the original enum. First-page missing/null arrays fail the original terminal reader; later-page source merges can supply defaults, but native conservatively requires all four arrays on every page. Empty/unknown objects never establish plaintext permission. `164623–164679` confirms server threads and waiting identities define enabled sessions. No session deletion, downgrade or bundle replacement is requested.

Eligibility failures carry fixed reasons for confirmed encrypted target, unsupported schema/field, correlation, timeout, intervening event and pagination. The UI distinguishes verified E2EE from an unknown state instead of labeling both as encrypted. Verification happens before reserving an outbox item or issuing the text HTTP request; rejection retains the draft and does not retry automatically. These source and local-TLS regressions do not identify which branch caused a prior real-account failure whose old log recorded only `send/unsupported`.

Group eligibility uses session-encrypted GET `/api/group/getgi/v2` (`398677–398692`), matching target `groupId` and explicit false/zero `e2ee`. Group state checks at `161956–161960` show that the server flag controls encryption. Signal messages and queues are not consumed or acknowledged; supported text in a mixed legacy batch is retained and unsupported media/encryption receives an explicit placeholder without displaying ciphertext.

## Runtime and validation

The desktop renderer loads `file:` HTML (`main-dist/main.js:92800–92809`, `110085`) and registers an `onSendHeaders` listener (`76534`). The installed Electron 43.3.0 runtime was independently captured with a disposable file renderer and pinned loopback WSS certificate: `Origin: file://`, `User-Agent: ZaloPC-linux-24v681`, HTTP/1.1, query type 24, no subprotocol. Reproduce with `tests/fixtures/messaging/capture_electron.cjs`; the sanitized capture is `electron_headers.json`. This replaces an earlier incorrect `Origin: null` inference from plain Chromium. Electron's [browser client](https://github.com/electron/electron/blob/v43.3.0/shell/browser/electron_browser_client.cc) intercepts WebSockets when this listener exists; its [proxy](https://github.com/electron/electron/blob/v43.3.0/shell/browser/net/proxying_websocket.cc) uses the committed frame origin. Native follows the captured Origin and client identity while retaining strict TLS, host and cookie scope. Captured `permessage-deflate` is deliberately not advertised without a bounded native codec. Local fixtures reject the previous null Origin/type-30 combination with 403 and accept the captured contract; this does not prove the cause or resolution of the user's live 403.

A private SQLite actor and a separate Tokio network thread communicate through bounded queues. Socket frames are capped at 1 MiB, decompression at 4 MiB, database batches at 200; HTTP bodies at 1 MiB. Redirects are disabled, TLS remains verified, credentials are sent only to verified session origins, and transport errors are replaced with secret-free messages. Cancellation interrupts nested network requests and saturated UI event publication; render-thread drop does not join disk/network I/O.

Optional message collections may be absent or null, matching the original queue handlers' truthiness gates. Optional `cliMsgId` may be absent, null, numeric zero or string `"0"`; these mean no client correlation and never identify an outbox row. Server message IDs, sender/recipient identities, routing and account scope remain strict. With `more: false`, an absent/null/zero `lastActionId` ends the page without creating a cursor; a valid positive terminal ID remains a durable fallback when queue status is absent. With `more: true`, a valid advancing continuation is mandatory. Malformed non-sentinel values still fail closed. A rejected mixed page commits no message prefix, advances no cursor, requests no next page and leaves local history available. These compatibility cases are synthetic regressions, not identification of the user's private failing payload.

Authenticated legacy queues additionally use the original peer-address contract: `msgs`/`pageMsgs` select a non-self sender as the direct conversation key, even when `idTo` names another peer; self-sentinel echoes select the destination, and `groupMsgs` select `g:<idTo>`. Source preprocessing at worker `404394` and default consumers at `382867`, `382941` and `383024` establish this behavior. The independent `routing_oracle.cjs` executes those original preprocessing/key-selection expressions for eight synthetic cases. Socket processing requires both an authenticated decoder and the connection's verified-session account matching the local event account before invoking this path. Parsed rows always use that bound account; payload identities cannot select another account. The generic/untrusted parser retains its stricter recipient-membership check, and Signal parsing is unchanged. Real loopback `510_1` tests verify peer-addressed text/unsupported-media placeholders, page/self/group routing, numeric group/direct collisions, account isolation, authentication/account guards, and atomic rejection of a mixed page containing a malformed recipient.

## Structured information messages

The exact `webchat` object content with `action: "msginfo.actionlist"` is a supported information-message type, not plain text or an unsupported-media placeholder. Original worker `26300` maps it to `MSG_INFO_CHAT`; `239311–239345` projects Vietnamese `params.msg.vi`, ordered `highLightsV2` substitutions and the title fallback. `418575–418603` additionally retains structured message/image/action data. The offline `info_oracle.cjs` executes the original preview methods for nine synthetic cases.

Native storage retains the **entire canonical content object** in additive schema-five `message_info`, including the original params string, actions and unknown nested fields. The human-readable projection is stored as `Message.text` for existing native rendering. It uses provided names and a fixed self label; date fallback uses supplied text or deterministic UTC, independent of later nickname/timezone changes. Projection work is bounded to 64 KiB of text and 1,024 highlights with indexed markers; full retained content is limited to 4 MiB per item and aggregate batch, with a decoder-compatible depth limit. Unknown object formats and encrypted info remain fail-closed.

Socket message, sidecar, conversation and replay cursor commit together. Identical replay is idempotent; changed raw metadata conflicts even when visible text is identical. Information events never acknowledge an outgoing text intent from a matching client ID. HTTP recent previews carry the same complete metadata through scoped `(account, thread, message)` bindings into 200-message transactions; validation and chunk distribution occur before sink writes. Preview-first/socket-first arrival cannot flatten rich content or change its type. Previews do not acknowledge socket queues.

Native rendering currently displays the readable text. Actions, URLs, avatars and other structured fields remain preserved for future support; they do not execute or cause downloads. Schema-five migration preserves history, outbox, sticker sidecars and cursors. A pre-v5 executable backup cannot reopen a migrated profile and is **not a database downgrade path**.

The information table remains unchanged in current schema six. Photo/OA messages have their own complete-content sidecar and bounded native projection; see [media contracts](PROTOCOL_MEDIA.md) for image formats, CDN limits, exact legacy enrichment and template/action boundaries. None of these rich message types acknowledge plaintext sends.

## Ordinary text replies (2026-11-12)

The source builders at shared-worker N399886–399944 use `/api/message/quote` for
personal replies and `/api/group/quote` for group replies. Both carry `message`,
`clientId`, `qmsgOwner`, `qmsgId`, `qmsgCliId`, `qmsgType`, `qmsgTs`, `qmsg`,
`qmsgTTL` and `ttl`. Personal replies add `toid` and `imei`; group replies add
`grid`, `language: "VN"` and `visibility`, without `imei`. Native uses the existing
session AES/form encoding, origin validation, encryption capability check and
single-attempt ACK/outbox path. Text source type is `1`; supported reply/source
TTL values are zero. The eight-case `reply_oracle.cjs` executes the exact original
builders, transform and TTL helpers with synthetic data.

The original serializer omits an undefined source `cliMsgId`. Native deliberately
requires the actual stored source ID instead of inventing a fallback from `msgId`.
The UI passes only a scoped target message ID; the SQLite worker rereads its
account/thread-bound canonical text, sender, timestamp and client ID while
reserving the outgoing reply. Missing/unsupported targets return a deterministic
unavailable result before reservation. TTL/format/info/media/encryption markers
explicitly revoke reply eligibility, persistently and conservatively across
replay, while an ordinary replay merely missing optional client metadata preserves
known IDs. E2EE replies are rejected without plaintext fallback or draft loss.

Schema ten adds `message_reply` and `outgoing_reply` sidecars. Source client IDs
and normalized quote projections hydrate history/refresh/ACK results; full quote
JSON remains in the worker-side store, limited to **128 KiB**, with a **256 KiB**
projection ceiling and the existing aggregate rich-batch budget. Bounded unknown
quote fields are retained; unsupported layouts show a neutral projection. Incoming
metadata, message and receive cursor commit atomically. Consistent missing metadata
may enrich a replay; conflicting known IDs/references abort without advancing its
cursor. A wire quote can enrich the locally synthesized outgoing snapshot without
discarding an already known optional quoted client ID.

Outgoing quote data commits with its reservation, survives unknown delivery and
restart, and is retained through ACK/echo repair even when an own-device echo omits
the quote. ACKs clear only the matching composer revision; newer edits or a changed
quote remain. The additive schema-nine upgrade preserves history, cursors,
ordinary/E2EE outbox bytes and typed init markers. Older binaries refuse schema ten;
no downgrade is provided. This implements ordinary text replies only, without
forwarding, mentions, reactions, recall/delete or real-server acceptance.

Connection diagnostics expose only fixed stage/category enums and optional numeric HTTP status or server/close codes, plus fixed parser reasons (for example `FrameLength`, `Compression`, `InnerEnvelope`, `QueueCommands`). URLs, cookies, keys, headers, close reasons and response bodies are discarded at the transport boundary. Explicit reconnect resets backoff without retrying an outgoing message. Local Open/Load commands remain available during a stalled handshake; cancellation drops that handshake immediately.

The bounded on-disk diagnostics writer records lifecycle and failures, including send, storage and partial directory-sync failures. Parser failures include only numeric command/subcommand/encryption tags, bounded byte counts, structural JSON types and closed message-kind/field-rejection labels; never envelope contents. An actual local TLS runtime regression survives three heartbeat intervals with absent/null/zero-status PONGs and encrypted messages, sends through layered 563 and encrypted HTTP ACK, then verifies a malformed message produces a redacted JSONL failure and no queue ACK. A ten-case actual-loopback receive regression additionally checks nullable metadata, terminal sentinels, rejected mixed pages, retained cursor/history, account isolation and absence of automatic send replay. See `DIAGNOSTICS.md` for paths and retention.

Missing endpoints, rejected authentication and deterministic protocol/crypto/HTTP failures pause the network until explicit reconnect. They keep the same local SQLite actor alive, so opening conversations and reading cached pages still work. Database failure, shutdown, cancellation or a disconnected UI ends the worker; an offline send cannot create an outgoing network request or clear its draft.

HTTP operations execute serially in the network worker, each with a 15-second deadline. Socket processing can pause during these requests; this limits throughput during a slow send. Durable receive cursors allow replay after a disconnect. This stage does not claim uninterrupted high-traffic receipt during stalled HTTP. Heartbeats use a matching event ID, a 30-second interval, and a 5-second pong deadline when the worker is processing sockets; reconnect delay caps at 30 seconds plus bounded jitter and resets only after 30 seconds of authenticated uptime.

Outbox client IDs are durable, monotonically increasing, account-scoped JavaScript-safe integers. A send is never retried automatically. Unknown delivery blocks sending the identical draft again. A verified ACK is recorded separately before message persistence; failed local insertion is retried locally and reconciled on restart. Server echo before/after ACK converges to one server message ID. Cursor and message replay remain account/thread scoped.

Tests include independently generated Node AES-GCM/zlib known answers; scoped SQLite/outbox/ACK-failure regressions; and actual local TLS/RFC6455 servers exercising cookie/SNI validation, complete capability resolution, personal/group encrypted HTTP requests and ACK persistence, redirects, oversized responses, stalled requests, and event backpressure cancellation. Three tests drive the actual `run_inner` command loop through local TLS/WebSocket injection, checking personal/group offline batches, cursor/deduplication, SQLite-before-Received publication, real Send commands through encrypted HTTP ACK to Sent, stalled-send cancellation, and saturated Received-event cancellation. These synthetic fixtures do not prove compatibility with a live account. Live login and production send/receive remain unverified until an authorized manual account check; Signal/E2EE messaging and media are not implemented.

## Ordinary unread, seen and desktop notifications

The source command mapping N248922–248940 identifies personal/group live
pushes (501/521), delivery updates (502/522) and clear-unread updates
(504/524). N404414–404472 maps `delivereds`, `groupSeens`, `seens` and
`clearUnreads`; see also N254965–255020 and N382882–382930 for seen handling
and the separate subchatbox clear-unread shapes. Native binds status to the
verified account, thread and an owned stored message. A supplied source
`cliMsgId` must match the stored ID. Group seen counts use distinct known
readers and do not imply the entire group read the message. Older receipt
updates cannot demote an already verified seen state. Unresolved delivery
updates are bounded so receipt-before-echo can converge without keeping
unlimited unmatched state.

Unread counts start from fresh ordinary 501/521 receive commits on this device.
N228973 excludes an explicitly zero `paramsExt.countUnread`; absent hints allow
eligible messages to count. Own, hidden, silent/system, historical and duplicate
rows do not create unread. References, new unread keys, message and the associated
receive cursor commit together. Authenticated `/api/preloadconvers/get-last-msgs`
(N402180) can enrich references and carry scoped status and watermarks without
counting cached history. Normal clear-unread updates advance the last read ID
monotonically and remove covered local unread and pending receipts. The subchatbox
`type:2` with `sct:1` or `sct:2` is excluded.

The original request builder N381580–381629 uses up to 50 source references
with N399199–399255. Native sends the session-encrypted `params` form through
CHAT POST `/api/message/seenv2` or GROUP POST `/api/group/seenv2`. Parameters
contain `imei` and stringified `msgInfos` with `senderId` or `grid` and
`data` items `{cmi,gmi,si,di,mt,st,at,cmd,ts}`. The source client ID and
original wire message type are retained. The same sender/destination direct
route uses `di:"0"`; missing `st`, `at`, `cmd` or timestamp use `-1`.
Only persisted, eligible ordinary references can be submitted. Local read
works when the reference is incomplete or the server disables
`ack_seen.v2_enable` (default at N227722). Native does not substitute legacy
V1 or Signal receipt traffic. The receipt worker dispatches bounded batches
outside socket processing and keeps pending rows until server success or a
covering clear-unread watermark.

Notification eligibility is checked separately from unread storage. The client
reads profile GET `/api/social/profile/getmute` (N381872, N399450), using
encrypted `imei` and the session's profile service. The source mute manager
in `pc-dist/lazy/main-startup.171f144ab54f0da65752.js` uses direct and group
entries and server times to determine remaining seconds. Native suppresses
notification for currently muted conversations, including mention variants,
and suppresses all notification when mute state is unavailable or refreshing.
It also requires a confirmed normal conversation and skips the focused visible
conversation. Only a newly committed unread push can produce a notification.

On Linux the session `org.freedesktop.Notifications` service receives the
app label and icon plus the generic summary. The message body is empty by
default. A per-account option adds up to 128 characters of plain preview;
markup and control characters are neutralized and no URLs are fetched.
Background D-Bus work is bounded and uses replacement IDs per conversation.
Desktop activation depends on the server's actions capability and is checked
again against current privacy and settings before opening a conversation. It
does not itself mark a message read. The client must be running to receive
these alerts.

## Ordinary direct text forwarding (2026-11-19)

Native forwards one ordinary direct text message to one eligible direct friend.
The independent `forward_oracle.cjs` exercises the original source transform,
reference creation and request builder. Native uses the FILE service POST
`/api/message/mforward` with session-encrypted parameters. It sends one
`toIds` entry, `imei`, zero TTL, type `"1"`, count one, nested stringified
`msgInfo.reference` and `decorLog`. The reference identifies a first-generation
source and carries its original source client ID-derived hash and verified
timestamp. The HTTP reply must bind exactly one result to the actual submitted
client ID and, when supplied, the expected recipient.

The worker validates source/account/thread and recipient against SQLite, then
checks the target's authenticated ordinary-send capability and auto-delete
policy. Schema fourteen adds a typed forward outbox and a source marker table,
without treating legacy text as verified forwardable. A typed forward push
cannot settle a plain send or reply. Unknown final delivery is not retried
automatically. A matching own echo can settle an unknown intent, and an
already recorded ACK can be repaired locally on restart. The picker does not
alter conversation choice, the text draft or its attachments.

The focused suite passed 11 cases, including original-source parameter
comparison, strict ACK binding, private/blocked target exclusion, schema
preservation, cursor rollback, native picker scope and a worker-level
synthetic TLS/WSS dispatch and restart case. Group/album/attachment forwarding,
re-forwarding, multiple recipients and real-service interoperability are
outside this verified slice.

## Forwarding direct text to groups

The ordinary group route is FILE POST `/api/group/mforward` with one
session-encrypted `grids` entry containing `grid`, string `clientId`, and
zero `ttl`. The outer request carries type `"1"`, zero TTL and count one.
It uses the original direct-text reference and decoration format. The
bundled client constructs the request at N489377–489394 and binds group
results to submitted client IDs at `lazy/main-startup` N25522–25543.

The group must have fewer than 100 members. Before dispatch the worker
checks current group membership, ordinary plaintext capability, visibility,
send permission and disabled auto-delete. Each result binds the exact
client ID and any explicit destination in the reply. The existing forward
outbox scopes receipts by account and complete direct or group thread ID.
One picker can mix up to ten recipients. Each recipient has its own request
and outcome. Unknown delivery does not redispatch after restart. The source
is a saved ordinary direct text message, and the profile remains on schema 25.

## Forwarding text from an ordinary group

The source can also be a saved, verified first-generation text message in a
visible ordinary group of fewer than 100 members. The worker verifies active
membership, plaintext capability and disabled auto-delete in the source
group before dispatch. Source read access is checked independently of the
destination's send permission. A stale proof is renewed between recipients,
and the committed source is revalidated immediately before sending.

The original reference builder (N480089–480156) uses the group source type
`2` and hashes the source client ID, effective sender ID and `g<grid>`.
Native uses that reference with the existing direct or group destination
route and the account/thread-scoped forward outbox. Schema 25 is compatible.

## Ordinary direct text recall (2026-11-19)

The original direct composer checks message age at N136319. N401877–401897
builds CHAT POST `/api/message/undo` with an AES-encrypted `params` form.
The direct body contains the actual server `msgId`, `toid`, newly allocated
numeric `clientId` and original `cliMsgIdUndo`. Native reserves the request
only for verified own ordinary direct text in a confirmed visible chat,
checks current direct plaintext capability and rechecks the recall deadline.
The default deadline is 300,000 ms when the setting is absent. An unavailable
or malformed setting can disable the action.

Acceptance requires success of the authenticated, decrypted endpoint reply.
The source does not require a new message ID in its success body. Native
recognizes the source rejection code 211. A response without positive
acceptance or that explicit rejection stays unknown after dispatch. An
unknown request is never submitted again automatically. A matching verified
push can settle it. The typed outbox saves an acknowledged result first, so
projection can be repaired locally after a restart without a second request.

The source `chat.undo` handler at N382999–383014 distinguishes the event
envelope from the recalled message. Native accepts an authenticated ordinary
direct notice bound to the account, thread, actor and original
`content.globalMsgId`. It checks the original client ID when available.
Native does not use the event ID or a client-only fallback as the target.
Scoped tombstones and receive cursors commit together. Known tombstones
suppress later same-owner history and echo copies. Unmaterialized notices
have a 4,096-entry account cap and a 30-day observed-age retention policy.
A verified late original can promote its notice to a durable tombstone.

Schema fifteen replaces the recalled body in saved history and its matching
ordinary local outgoing record. It clears affected unread/receipt metadata
and removes raw content from scoped saved quotes, which show the neutral
recall label. Quote hydration and replay use the same scoped proof. The
explicit recall flag distinguishes the real tombstone from a user message
with the same words. Saved search and conversation previews read the
redacted rows. These are logical storage changes. Separately forwarded
messages remain independent. Group, media and E2EE recall and local deletion
are outside this slice.

## Ordinary photo and file recall

The ordinary client dispatches both text and media through the existing
undo request. The direct endpoint is `/api/message/undo` and the group
endpoint is `/api/group/undomsg`. Native scopes the target to an accepted
PHOTO or FILE record for this account and thread, matching the actor, server
message ID and original client ID. Each album image has its own target.
The normal capability, group membership and recall deadline checks apply.

An authenticated own-account notice with the original client ID can wait
for the typed receipt and be applied in the same transaction as acceptance.
The local projection clears resource URLs and display metadata. Late typed
receipts and history copies use the redacted projection. Native retains the
receipt identity and scoped tombstone in schema 24. A recalled first album
member retains its layout ID, index and count so the remaining images can
finish sending. Previously saved local downloads and media sent from other
clients are outside this implementation.

## Deleting one ordinary text message for the account

The session must affirm `settings.features.sync_del_msg_or_conv.enable_sync_del_msg`
or its `setttings` alias. The native confirmation fixes the selected mode.
With sync disabled, a device-only action records local suppression without an
HTTP request. Account mode posts encrypted form parameters to CHAT
`/api/message/delete` or GROUP `/api/group/deletemsg` with `onlyMe:1`.
One message in `msgs` binds the stored global ID, client ID, sender and
destination. The action has a separate monotonic client ID. See PC bundle
lines 472907–472947 and 491041–491075.

Schema 23 binds a deletion to the account, thread, message and sender. A
confirmed action removes the local row, sanitizes matching quotes and
suppresses replay. Uncertain account requests retain the message and are
never retried automatically. An acknowledgement survives a failed projection
transaction.

The authenticated message queue accepts `chat.delete` with an own-account
actor, single-message type 1 or 3, explicit account-only action type 0, and an
exact target tuple. The ordinary text target must match account, conversation,
server message ID, source client ID and sender. Schema 24 stores up to 4,096
pending claims per account for notices arriving before the original. The
queue transaction commits notices and its cursor together. Matching history
can settle a pending claim, and an exact notice can settle an uncertain local
intent. See PC bundle lines 469779–469814 and 471398–471425.

## Ordinary group members, invite and leave (2026-11-19)

N398655 builds authenticated GROUP POST `/api/group/getmg-v2`. The client
sends a stringified `gridVerMap` with version zero and checks the returned
row against the exact group and authenticated account. The supported slice
requires an ordinary group, a complete internally consistent membership list,
and at most 100 members. A removed or unchanged entry is insufficient proof.
Group owner and deputy roles come from the bound response. Privileged roles
or explicit viewer settings authorize visibility. Otherwise the displayed
projection contains only the viewer, owner and deputies. Native filters the
roster before requesting PROFILE POST `/api/social/group/members` (N79255),
with at most 50 permitted IDs from the current page. Continuation is bound to
the current roster and permissions. A changed roster requires reloading.

N399563 and N399574 build the GROUP mutation endpoints. Native checks
ordinary unencrypted group eligibility, current membership, privacy, role
and settings before a one-shot authenticated POST. `/api/group/invite/v2`
sends one verified friend in `members`, with `memberTypes:[-1]`, `grid`,
`imei` and `clientLang:"VN"`. `/api/group/leave` sends one group in
`grids`, `imei`, `silent:0` and `language:"VN"` for a non-owner.
Neither endpoint includes a client ID. Native uses a local intent ID for
durable state and scoped callback routing.

The member-bound response distinguishes acceptance for processing,
approval required, an invitation sent, already invited, already a
member and rejection. Submission acceptance does not prove the friend
joined. The live client only updates the roster from a verified member
page. An uncertain result and approval/invitation state block an
identical invitation. An exact positive roster observation can resolve a
prior invite. A removed/absent roster entry or a transport failure is
insufficient to confirm departure. Native confirms leave from the
authenticated successful endpoint response.

Schema sixteen records action outcomes and confirmed departures. A
restart classifies outstanding dispatch as unknown without retrying the
request. A confirmed exit prevents a stale directory page from restoring
active membership or write ability. Saved messages and drafts remain
available in the archived conversation. When leave is still uncertain,
the client keeps the history but holds further writes for that group.
Group creation, owner transfer, removal, bulk invitation, rejoin,
large/community groups and encrypted mutations remain outside the slice.
