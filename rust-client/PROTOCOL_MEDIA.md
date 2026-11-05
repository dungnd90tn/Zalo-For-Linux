# Native photo, OA, shared-content and poll messages

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

`group.poll` renders the embedded Vietnamese notice, question and an anonymity
label only when an explicit boolean is present. Options, votes, results and
poll updates belong to a separate authenticated detail API and are not invented
from the notice. Voting remains unavailable. The exact-source
`rich_cards_oracle.cjs` exercises six synthetic original-reader cases.

## Durable content and replay

**Schema seven** retains the entire canonical source content in scoped
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
Photo, OA, recommended, poll and information events never acknowledge an outgoing text intent.

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
The v6→v7 migration extends only the media-kind CHECK constraint in the existing
transaction, copying every stored metadata/projection/preview byte unchanged.
History, information/sticker sidecars, account/thread keys, foreign keys, cursors
and outbox survive; a failed copy rolls back the table and schema version.
Pre-v7 executables cannot reopen the migrated database and provide no downgrade.

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
Cached thumbnails open a native viewer without downloading an original. Opening
an original image or an available external link requires an explicit click.
Upload, full-resolution in-app viewing and universal OA actions remain pending.

## Verification boundary

Synthetic tests cover source projections, complete raw retention, both HTTP/WS
orders across scoped chunks, restart, advanced-cursor legacy enrichment, immutable
conflicts, storage rollback/retry, outgoing isolation, malformed/budgeted codecs,
stale results, bounded texture uploads, cancellation and closed-worker recovery. Native
Light/Dark, compact/wide, cached-viewer, failure and opaque-template screenshots
are recorded in [VALIDATION.md](VALIDATION.md). Production CDN compatibility and
every real OA template require separate acceptance.
