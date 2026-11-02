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
redirects. It streams at most 1 MiB, decodes bounded PNG dimensions up to 512×512
and at most 1 MiB RGBA, and shows the first PNG frame. Requests are deduplicated
and capped at 32; decoded LRU cache is capped at 64 entries / 8 MiB; the result
queue holds four images. UI textures have their own 64-entry / 8-MiB budget.
Failures cool down for 30 seconds and emit closed `sticker_assets` diagnostics,
including numeric HTTP status. Successful downloads emit debug-only Ready with RGBA
byte count; no IDs, URLs or payloads enter the log. Drop cancels
stalled requests and queue waits without joining on the rendering thread.

Full animation, GIF/WebP/sprite resources, effect stickers, catalog refresh and
sticker sending remain separate work. An unavailable thumbnail remains a truthful
local placeholder and does not change messaging connection state.

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
