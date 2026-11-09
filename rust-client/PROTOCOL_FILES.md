# Sending files

Status: the **interface and local handling are done; uploading to Zalo is not implemented**. In the
live build the attach button stays disabled and a dropped file is refused with that reason. The
offline demo stores the file *message* locally (name, extension, size) and uploads nothing.

## Implemented

- `domain::Attachment::inspect`: a regular, non-empty file of at most **100 MiB** (the original's
  `small_file_size_limit`; larger files use Zalo Cloud, see below) whose name is valid UTF-8 without
  control characters and at most 255 bytes. Symbolic links are followed; directories and devices are
  refused. The content is never read.
- Choosing: `zenity` or `kdialog` run on a background thread, or drag-and-drop of one file onto the
  window. A result for another conversation than the one it was started in is dropped.
- A tray above the editor shows the file (name, `PDF · 1,8 MB`) with a remove button, a check in
  progress, or the reason a file was refused. The attachment belongs to the conversation's draft
  like the text: it survives switching conversations and is sent on its own, leaving typed text.
- Demo: `Command::SendDemoFile` stores a `share.file` message without an address, so the card
  says the file cannot be opened. Tested end to end through the real worker and SQLite.

## The original's upload flow (read from `pc-dist`, not verified against the server)

All paths are in `compact-app-pc.*.js` unless noted; line numbers are for this repository's copy.

1. **Chunks.** Default chunk 256 000 bytes (`getDefaultChunkSize`, settings `sharefile.chunk_size_file`
   if at least 64 000). Each chunk is a `multipart/form-data` POST with field `chunkContent` named
   after the file, sent with cookies (`withCredentials`) and `getUploadHeaders`.
2. **URL.** `<fileDomain>/api/message/asyncfile/upload?<common params>&params=<AES>&type=2`
   (groups: `/api/group/asyncfile/upload`, `type=11`; `getInfoUploadFileFromData`, ~148615).
   `params` is the AES-encrypted JSON `{totalChunk, fileName, clientId, totalSize, imei, chunkId,
   isE2EE:0, toid | grid}` (`buildSendChunksInfo`, ~328392; `getUrl`, ~328470). Chunk ids start at 1.
3. **Replies.** `{error_code:0, data:<AES JSON>}`; the inner `error_code` 0 means more chunks are
   wanted, 1 with a `fileId` means the same file was already uploaded and `url` is reusable
   (`handleSendChunkResp`, ~328510). The checksum is the MD5 of the whole file.
4. **Completion is asynchronous.** After the last chunk the client waits for a control event, not a
   reply: a websocket control message whose `controls[].content.act_type` is `file_done` carries
   `[{fileId, data:{url}}]` (`_processFileAsyncDoneEvent`, shared worker ~383226 and the
   `_processCtrlMessages` switch ~383329; control queues `603_0`/`603_1`, `queue_ctrl_actionid_map`).
5. **The message.** Only then `POST <fileDomain>/api/message/asyncfile/msg?<common params>`
   (groups `/api/group/asyncfile/msg`) with `params=<AES>` of `{fileId, checksum, checksumSha:"",
   extension, totalSize, fileName, clientId, fType, fileCount, fdata, toid | grid, imei}` plus the
   fields `buildParams` adds (`sendMsgFileAsync`, ~490776; `getFileAsyncItem`, ~148640).

Not covered by this client: files above 100 MiB (Zalo Cloud multi-site upload, 3 MiB chunks), the
end-to-end encrypted variant (`E2EEFile`, `isE2EE` chunking), voice and video uploads, photos.

## What blocks real sending

The Rust runtime has the signed, encrypted request machinery used for text, but three parts have
never been observed against the real service and cannot be checked offline:

- the exact upload headers and cookie handling (`getUploadHeaders`);
- subscribing to the control queues (`603_0`) and parsing `file_done`, since the receive loop only
  consumes the message queues today;
- the reply shapes (the signal service already surprised us once by omitting zero-valued fields, see
  the E2EE eligibility fix in the changelog).

Sending a malformed upload with a real account can fail the feature or the session, so the work
needs a test account and one captured successful upload to compare against. The planned steps are:
planner (chunk table, MD5, params, URL) as pure tested functions; HTTP chunk upload with a local TLS
fixture; control-queue subscription; `asyncfile/msg`; progress and cancel in the tray; all behind an
opt-in switch until verified.

## Receiving files (implemented, unverified against the real service)

`app::download::FileDownloader` runs when the user presses **Tải về** on a received `share.file` card:
a plain `GET` of the message's `href` with the thumbnail client's policy (HTTPS, `*.zdn.vn`/`*.zadn.vn`,
no cookies, no redirects), at most 100 MiB, streamed to a private `.part` file and renamed to a free name
under `$XDG_DOWNLOAD_DIR/Zalo Native`. The original also sends account headers for cloud (`zcloud`) files
(`getDownloadHeaders`) and uses end-to-end keys for encrypted ones; neither is reproduced, so a `401/403`
or `3xx` from the CDN is the signal that more headers are needed. The diagnostics log records only the
status code and byte count (`stage=media_assets`).


The same worker also handles **Tải ảnh gốc** from received photo cards and the native
preview viewer; it saves the original URL's bytes, never the thumbnail or a re-encoded
preview. Only HTTP 200 is accepted because these requests do not ask for a byte range.
Cancel interrupts waiting for a download slot, response headers or a stalled body;
the partial file is removed. Completed downloads survive navigation within one account;
account/privacy resets discard the presentation state and stop its worker. No resumable
transfer, cloud-specific credentials or encrypted attachment decoding is implemented.
