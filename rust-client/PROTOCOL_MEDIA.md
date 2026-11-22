# Native photos, files, cards, polls and avatars

## Source contracts

The original `_normalizeMessageType` maps `chat.photo` to photo and
`chat.webcontent` to ZInstant V1. The photo normalizer in
`pc-dist/compact-app-pc.e08d0d44f38873747a6b.js` reads title, description,
thumb, href and JSON-string/object params, preferring `params.hd` for the
original image. The V1 reader in `pc-dist/lazy/18.2736ee958ea402bb72d6.js`
parses params; `getPreviewMsgZNS` supplies the readable custom-message preview.
`tests/fixtures/messaging/media_oracle.cjs` executes those original functions
against four synthetic string/object fixtures. It also verifies the original
HTTP preview caller's nullable collection behavior.

`chat.webcontent` is an OA/business-template message. Native rendering supports
the source-backed Vietnamese `customMsg` preview, title, description and image
metadata. Remote `item`/`pcItem` templates do not have a universal local schema.
Unknown template data remains fully stored, with an explicit native fallback;
JavaScript, HTML, actions and remote template URLs are never executed or fetched.
This does not implement every interactive OA layout.

`chat.recommended` uses the original `recommened.*` action spelling. Native
readers project link titles/descriptions, contact names/phone numbers and source
thumbnails; only `recommened.link` exposes an explicit browser link. Contact IDs,
call identifiers and sticker-set addresses are retained without being treated
as executable actions or image URLs. Unknown actions have an honest fallback.

`group.poll` renders the embedded Vietnamese notice, question and explicit
anonymity. The exact-source `rich_cards_oracle.cjs` exercises six synthetic
original-reader cases. Native recovers a positive `params.pollId` from the
saved notice without changing the stored projection or schema.

An explicit click in a verified visible ordinary group loads details from
the advertised `group_poll` service. The original bundle builds GET
`/api/poll/detail` with AES `params` containing `poll_id` and `imei`
(N490384–490390). Native verifies current group membership and rechecks the
account, group, message and privacy revision before applying the reply. The
response must match the requested poll and any explicit group ID. It provides
up to 30 options, the explicit closed and anonymous flags, and counts when
the preview is public or the viewer has joined a restricted poll. The
detail projection is transient and omits voter identities. Creation, voting
and background updates remain outside the read-only flow.

`share.file` maps to the original file type. Its card reads `content.title`,
`content.href` and string/object `params.fileSize`, `fileExt`, `fType`; `fType=2`
identifies a folder. String-param title normalization follows the original
extension handling, including its unsupported-extension list. Unknown size stays
unknown; zero is a valid size. `files_oracle.cjs` retains eight synthetic original
reader cases. File URLs are exposed only as an explicit HTTPS browser action;
HTTP links upgrade to HTTPS. No file is automatically downloaded, opened locally
or executed. Nonempty `secureKey`/`e2eeKey` markers withhold the plaintext action.
Ordinary single-file, single-photo HD, HD album and entitled original JPEG
sending are implemented as described below. Animation, transfer resume, folder
extraction and E2EE attachment decryption remain unavailable.

## Ordinary single-file sending

The optional live messaging path uses the configured FILE service, not the chat
service or the legacy whole-file endpoint. Direct/group chunks POST to
`/api/message/asyncfile/upload` or `/api/group/asyncfile/upload`, with multipart
`chunkContent` and encrypted parameters `totalChunk`, original `fileName`,
`clientId`, `totalSize`, `imei`, one-based `chunkId`, and `toid`/`grid`.
Server-configured chunk sizes are bounded to 64000–1048576 bytes (default 256000).
Files must be nonempty and at most 100 MiB; a private immediately unlinked snapshot
and streaming MD5 bind the selected bytes without retaining them in the profile.

The final `/api/message/asyncfile/msg` or `/api/group/asyncfile/msg` request binds
`fileId`, lowercase MD5 `checksum`, `checksumSha:""`, `extension`, `totalSize`,
NFC-normalized `fileName`, `clientId`, `fType:1`, `fileCount:0`, `fdata:"{}"`,
`fileUrl`, `ttl:0` and `toid`/`grid`. Only direct final requests include `imei`.
The source extension keeps its original case, including Unicode; native cards
compare its Unicode-lowercase projection without changing wire spelling.
Extensions of seven or more UTF-16 units become empty, matching the source.
A final chunk response or matching online command-601 `file_done` supplies the
resource URL. This observation adds no control ACK and leaves command 603
untouched. Resource completion is not message acceptance. URLs are bounded HTTPS
opaque values, never automatically fetched; no CDN hostname is invented for them.

Schema eleven adds a separate account/thread/client-scoped `attachment_outbox`.
Resource metadata and exact final JSON plus SHA-256 commit before the final POST;
the actor rechecks current privacy, session and encryption capability before
claiming dispatch. Restart maps uploading/resource-ready to known-unsent
interruption and final sending to unknown delivery. Unknown or acknowledged-but-
unrepaired duplicates are blocked by filename/size/MD5 fingerprint, with no
automatic reupload or resend. ACK persistence precedes transactional synthetic
`share.file` insertion; restart repairs a known ACK without network access.
Matching own file echoes reconcile by typed metadata, never by placeholder text.
The additive migration preserves schema-ten quotes and existing history/E2EE
records; older binaries refuse schema eleven and no downgrade is provided.

## Ordinary single-photo HD sending

The worker reads authenticated feature settings from the source-supported
`settings/features` and legacy `setttings/features` paths, with nullable/alias
fallback. Route and limits come from those settings rather than an assumed
photo endpoint. Upload uses `/api/{message|group}/photo_original/upload` on FILE
or `/api/{message|group}/upload/photo` on PHOTO as configured. Final submission
uses `/api/{message|group}/photo_original/send` on FILE. A matching online-601
`photo_success`/`act:photo` resource or finished chunk result supplies URLs;
resource completion never settles the photo message or a FILE send.

Source input is limited to the smaller server/native bound: at most 20 MiB and
16 megapixels. Preparation applies EXIF orientation, strips metadata and composites
alpha on black, then downsizes without upscaling to at most 2560×2560 and encodes
JPEG with bounded quality attempts to at most 5 MiB. This is HD recompression,
not original-byte sending. The local preview decodes those exact processed bytes.
Combined prepared JPEG bytes and thumbnail allocations use a 32 MiB lease
budget; private unlinked files close when their scope/lease is dropped.

Final parameters bind `photoId`, `clientId`, `desc`, `width`, `height`, `rawUrl`,
`thumbUrl`, `hdUrl`, `language:"VN"`, `imei` and `ttl:0`. Direct sends add `toid`
and `normalUrl`; group sends add `grid` and `oriUrl`, and also retain `imei`.
Optional `thumbSize`, `fileSize` and `hdSize` stay bounded. URLs remain opaque
HTTPS resources. The complete caption is bound before POST even when the native
history projection displays only its bounded preview.

Schema twelve adds the separate typed `photo_outbox`, preserving schema-eleven
FILE records, quotes, history and E2EE intent. FILE and PHOTO share a transactional
one-active-transfer/account guard and monotonic client counter, but cannot settle
each other's resources or ACKs. Exact immutable final JSON plus SHA-256 and raw
`chat.photo` metadata commit before dispatch; no personal path or source pixels
are stored. Unknown/acknowledged duplicates use account/thread/JPEG-MD5/dimensions,
so changing a caption cannot authorize an uncertain resend. Known-unsent restart,
unknown final dispatch, durable ACK/local-echo repair and no automatic resend
follow the FILE contract. Older binaries refuse schema twelve; no downgrade is
provided. Real-service photo acceptance remains pending.

## Sending an original JPEG

An authenticated paid Zalo Cloud plan and the session's original-upload
capability are required. The client rechecks the plan before sending bytes.
It preserves JPEG image data and removes APP1 metadata, then uploads through
the Cloud service and sends through the configured photo service. The response
must identify an original URL at the prepared size. See
[original JPEG verification](VALIDATION.md#sending-one-original-jpeg-206-development-branch).

## Sending an HD album

The image picker prepares 2–6 HD JPEGs in the chosen order and honors smaller
session limits. The existing HD service sends each image with a distinct client
ID, shared decimal-string `groupLayoutId`, zero-based `idInGroup`,
`isGroupLayout:1`, and `totalItemInGroup`. When advertised by session settings,
`extMsgProp` also carries the layout ID. See PC bundle N136681–136724 and
N401554–401571. Draft text stays in the composer.

Each image uses the scoped photo ledger, including its group metadata through
receipt reconciliation. The next image starts only after durable acceptance of
the previous one. Failure or an uncertain response stops the send. A restart
can reconcile a recorded receipt and does not resume the album. Schema 21
adds the album projection and prevents earlier clients from opening the
upgraded profile. The timeline groups complete contiguous runs and keeps the
separate photo actions.

## Recording and sending a voice message

The recorder holds one MP3 clip in memory, up to 60 seconds or 2 MiB. A user
action starts FFmpeg on the local PulseAudio-compatible socket. Switching
thread or account cancels capture and playback. Ordinary direct and group
sends use FILE paths `/api/message/voicemp3` and `/api/group/sendmp3`
respectively. Encrypted URL parameters carry the scoped target and decimal
client ID. The multipart body contains the MP3 as `fileContent`. See
`compact-app-pc` lines 489489–489503.

Schema 22 adds a voice receipt ledger and preserves existing media rows.
The ledger stores a digest and scoped send state. A typed HTTP receipt or
matching own echo settles a send. A send that may have reached the server is
never retried automatically. The timeline uses the actual incoming
`chat.voice` sidecar. Voice playback fetches up to 8 MiB from an approved
HTTPS CDN only after a click, then decodes MP3, AAC or AMR via a pipe.
Encrypted audio and service-side format conversion remain unavailable.

## Durable content and replay

**Schema eight** retains the entire canonical source content in scoped
`message_media`, plus a separate validated presentation snapshot of at most
16 KiB. The original params string and unknown nested fields survive restart.
Full rich content is limited to 4 MiB per item and to a combined 4 MiB of
information/media content per receive batch or preview response. Normal pages
load bounded projections, never the full source object on the UI thread.

Authenticated socket receive and verified-session HTTP previews carry matching full
account/thread/message bindings through each 200-row chunk. Only the authenticated
HTTP caller uses source peer routing; generic untrusted parsing retains strict
recipient membership. Empty/control-only preview objects and nullable source
collections do not fail synchronization. Rich metadata,
messages, conversations and the owning socket cursor commit atomically. Identical replay
is idempotent; changed raw content conflicts even if its label is unchanged.
Photo, file, OA, recommended, poll and information events never acknowledge an outgoing text intent.

An already stored exact legacy placeholder can gain a sidecar only when its
account, thread, server ID, sender and timestamp match, with no existing rich
sidecar. Its stored body and outbox remain unchanged. A bounded recent-preview
refresh can enrich only messages actually returned by the server. A parser
upgrade cannot recover absent old payloads or rewind an advanced queue cursor.
When directory synchronization enriches a visible legacy message, the pane
refreshes only its bounded displayed IDs through the worker. Exact immutable
identity, account/thread and request epochs gate media-only merges; current page,
message order and drafts remain intact. Stale results, saturation and read failure
do not publish partial history or mark a failed refresh complete.
The v6/v7→v8 migration extends only the media-kind CHECK constraint to include
`share.file`, within the existing transaction. It copies every stored
metadata/projection/preview byte unchanged. History, information/sticker sidecars,
account/thread keys, foreign keys, cursors and outbox survive; a failed copy rolls
back the table and schema version. Older executables reject schema eight;
no binary/database downgrade is provided.

## Native images and explicit actions

Only visible live thumbnails are loaded, on a worker without cookies or session
credentials. Source CDN subdomains under `.zdn.vn` and `.zadn.vn` are allowed;
source HTTP CDN URLs upgrade to HTTPS. Credentials, unusual ports, arbitrary
origins and redirects are refused. URLs and message/account identities never
enter diagnostics. Unsupported secure-photo markers suppress plain asset loading
while retaining the complete content.

PNG, JPEG, WebP and a bounded static JPEG XL subset decode locally. Encoded input
is capped at 8 MiB; headers reject dimensions above 8192 or 16 megapixels before
pixel allocation. Codec allocation budgets are 64 MiB; these are not total RAM
measurements. JXL ignores ancillary boxes and refuses ICC expansion, animation,
preview frames and unsupported extra channels. One process-wide decode permit
prevents concurrent decoders across rapid pane changes. Thumbnails fit 1024².

Worker and GPU caches each retain at most 32 entries/16 MiB of pixel payload;
request/result queues hold 16/2 items. Cache entries/results retain account,
thread, message and generation scope. Pixel caches are memory-only. Failure has
a 30-second retry cooldown and explicit retry; closed workers can be recreated.
Cached thumbnails open a native viewer without downloading an original. **Tải ảnh gốc**
on the photo card and viewer shares one transfer keyed by account/thread/message/original
URL. The source `saveImageData` queue (`compact-app-pc.*.js:153214`) likewise takes a
URL and destination separately from displayed pixels. Native saves the original bytes
through `FileDownloader`, without decoding/re-encoding
or falling back to the thumbnail. Only an available original URL passing the CDN policy
offers this action; secure-photo markers continue to suppress it. Progress, cancellation,
retry and the saved location use the same controls as file cards. Original URL image
extensions are preserved in a sanitized `anh-<message-id>` filename; unknown extensions
stay absent, and signed query strings/captions never become filenames. Downloads are
capped at 100 MiB, never overwrite existing files, and cancel even during a stalled
response, cleaning the private partial file. An unsolicited HTTP 206 is refused.
The viewer footer scrolls within its bound so save actions remain reachable in compact
windows and with long captions. Browser navigation still requires a separate explicit
click. Full-resolution in-app viewing and universal OA actions remain pending.
See [file transfer limits](PROTOCOL_FILES.md#receiving-files-implemented-unverified-against-the-real-service).
Local HTTP and synthetic UI checks establish these behaviors, not production CDN acceptance.

## Identity avatars

Friends use the original `userId`/`avatar` contract; groups use `groupId`/`avt`.
The authenticated account profile uses the original encrypted
`GET /api/social/profile/me-v2` request with `{avatar_size:120, imei}` and the
outer Linux `os=8`; only a matching `data.profile.userId` is accepted. This
optional metadata step follows directory synchronization and may wait behind
other discovery requests. Its failure does not invalidate messaging or a verified
hidden policy. The account record does not become an extra ordinary contact.

Visible identities request at most 64 account-scoped primary-key lookups per
snapshot, using distinct `d:` user and `g:` group keys. A group image never
supplies a member image. Unknown or failed privacy checks suppress metadata;
hidden images require the current authorized thread. UI epochs and URL binding
reject stale account, policy and photo results. There is no complete-directory
scan per frame.

Avatars reuse the media transport, CDN policy, decoder limits and process-wide
decode permit, with output capped at 256 physical pixels and no upscaling of
small sources. Their worker cache remains bounded at 32 entries/16 MiB; the
separate UI avatar cache holds at most 64 textures/4 MiB, with at most 256 metadata
entries. Logout or privacy-scope changes clear presentation state and cancel the
loader. Initials remain the fallback when a verified image is missing or cannot
be loaded. Demo fixtures use prepared pixels and do not fetch remote avatars.

## Verification boundary

Synthetic tests cover source projections, complete raw retention, both HTTP/WS
orders across scoped chunks, restart, advanced-cursor legacy enrichment, immutable
conflicts, storage rollback/retry, outgoing isolation, malformed/budgeted codecs,
stale results, bounded texture uploads, cancellation and closed-worker recovery. Native
Light/Dark, compact/wide, cached-viewer, failure and opaque-template screenshots
are recorded in [VALIDATION.md](VALIDATION.md). Production CDN compatibility and
every real OA template require separate acceptance. File actions and the account
profile/avatar path likewise require real-account acceptance; synthetic source
fixtures and local TLS do not establish production availability.
