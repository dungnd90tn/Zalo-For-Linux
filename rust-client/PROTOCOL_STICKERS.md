# Legacy sticker reception

This implementation derives from this repository's own current PC bundle, not
another client. It adds still thumbnail reception; it does not send stickers or
activate the future Signal pipeline.

`chat.sticker` normalizes to the ordinary sticker type (N315674/N367947).
The original sender creates `content:{id,catId,type}` (N137013–137074); the receiver
resolves catalog resources by `(catId,id)` (N127728). IDs may be numeric strings or
safe JSON integers. The native descriptor bounds IDs to `0..2^53−1`, type to
`0..255`; malformed or encrypted descriptors stay explicit unsupported messages.
No message-provided URL is fetched. E2EE routes remain unsupported until their
cryptographic pipeline is ready.

The old body `[Tin nhắn chat.sticker chưa được hỗ trợ]` remains unchanged for
replay compatibility. A typed, account/thread/message-scoped sidecar is committed
with the message before publication. Replayed legacy placeholders can gain that
metadata; conflicting descriptors cannot overwrite existing content. Sidebar
previews use `[Nhãn dán]`; reopening history restores typed metadata.

Older releases stored only the unsupported-sticker body and discarded its catalog
IDs. Those existing rows remain unchanged; an image can appear only if the original
message is replayed with metadata or a future history fetch restores it. There is
no automatic refetch or invented ID. Test the update with a newly received sticker.

Two original fixed thumbnail namespaces are distinct:

- Built-in `catId=0,id=1..30` (the original factory emits type 7; lookup branches
  on category alone, N519754–519766): `https://stc-chat.zdn.vn/images/stickers/default/thumb/{id}.png`
  (N519573–519583, constants N545668/N545726, staticDomain N227164).
- Catalog stickers: `https://api.zaloapp.com/api/emoticon/sticker/webpc?size=130&eid={id}`
  (N227150 and actual image element N27412).

Both public source samples were independently fetched once without credentials:
20695 returned PNG 130×130 / 10,684 bytes; built-in 1 returned PNG 130×130 /
7,236 bytes. These probes establish those samples, not every sticker's availability.
No downloaded artwork or account data is committed.

The background loader uses TLS verification, no cookies/authentication and no
redirects. It streams at most 2 MiB and detects the format from the file signature, never
from `Content-Type` (the CDN labels some JPEG bytes `image/png`). Supported:

- PNG: a still (at most 512×512 / 1 MiB RGBA), or a **sprite strip** whose width is a whole
  number of square frames (frames = width / height, 2–64), the original client's
  `stickerSpriteUrl` + `totalFrames` model;
- JPEG and static WebP as stills;
- animated WebP and GIF with their own per-frame delays (clamped to 20–1000 ms).

Catalog stickers request `https://api.zaloapp.com/api/emoticon/sprite?size=130&eid={id}`
first: probed anonymously it returns a still PNG for static stickers, a PNG strip (2–22
frames seen) for sprite animations and an animated WebP for others. Any failure except a
transport error falls back to the still `webpc` thumbnail. Built-in `catId=0` stickers have no
animation resource. A zip package (effect stickers) is refused as an unsupported format.

Every animation is packed into one RGBA atlas no larger than 2048 px per side (egui panics on
a bigger texture; frames wrap onto more rows) and 4 MiB; a longer (over 64 frames) or heavier
animation degrades to its first frame instead of failing. The UI draws one frame as a
sub-rectangle of the atlas chosen from the frame clock, so every copy of a sticker shows the
same frame. It advances only while the window has focus and requests a repaint only for the next
frame boundary. Sprite strips play at a fixed 100 ms per frame: the real per-frame `duration`
comes from the authenticated sticker detail API, which this credential-free loader does not call.

Requests are deduplicated and capped at 32; the decoded LRU cache is capped at 64 entries /
16 MiB; the result queue holds four images. UI textures have their own 64-entry / 16-MiB budget
that counts the still and the atlas. Failures cool down for 30 seconds and emit closed
`sticker_assets` diagnostics, including numeric HTTP status. Successful downloads emit
debug-only Ready with the RGBA byte count; no IDs, URLs or payloads enter the log. Drop cancels
stalled requests and queue waits without joining on the rendering thread.

A sticker message is drawn as the picture alone: no bubble fill, border or padding, and exactly
as wide as the picture (or its sender/status line when that is wider). An unavailable image
remains a truthful local placeholder and does not change messaging connection state. Effect
stickers, catalog refresh and sticker sending remain separate work.

Offline checks:

```sh
node tests/fixtures/stickers/oracle.cjs
cargo test --locked --offline --features live sticker
```

The Node oracle executes original normalization, sender and built-in factory
methods. Local TLS tests exercise actual image decoding, credential-free requests,
redirect/size rejection and cancellation/backpressure. An actual messaging run-loop
fixture commits mixed personal/group text/stickers before UI events, preserves
malformed-media placeholders, and verifies duplicate replay and history loading.
