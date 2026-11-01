# Directory and recent-preview protocol

This independent implementation derives facts from the repository's shared renderer/worker bundle (`pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`). It reads no Electron profile, credentials or reference Rust implementation. No production account request has been made for this work.

| Contract | Local evidence |
| --- | --- |
| Session-CBC encrypted GET profile `/api/social/friend/getfriends`; payload `incInvalid:1,page:1,count:20000,avatar_size:120,actiontime:0,imei`; optional first-call `init=1` | N398578–398591; avatar constant N398139 |
| Friend result is the decrypted profile array, using `userId`, `displayName`, `avatar`, `isFr`, `isBlocked` | N469623–469640; N469728–469755 |
| Group GET `/api/group/getlg/v3`; payload `page,count,avatar_size,member_avatar_size,imei` | N398630–398642 |
| Group response `groups`, `hasMore`, optional `total`; original manager requests 100 per page and bounds pages below 100 | N437910–437923 |
| Group normalization uses `groupId`, `name`, `avt`, `totalMember`; direct/group IDs remain different | N437849–437861 |
| Conversation service GET `/api/preloadconvers/get-last-msgs`; payload `threadIdLocalMsgId:JSON.stringify(localMap),imei` | N402175–402181 |
| Previews decrypt the full inner envelope, then merge `msgs`, `groupMsgs`, `pageMsgs`; `eeMsgs` belongs to separate Signal processing | N101765–101781; N404392; N469972–470016 |

Fresh discovery starts with an empty preview map (`"{}"`). Friend retrieval follows the observed single full-list request; it does not invent undocumented completion cursors or infer removals from missing records. Group pagination follows explicit `hasMore` with a fixed page/request budget. Partial, canceled or unsupported snapshots never delete cached contacts, groups, messages or outbox entries.

New modules separate pure wire parsing (`protocol/directory.rs`), bounded asynchronous HTTP orchestration (`app/directory.rs`) and atomic account-scoped SQLite metadata (`store/directory.rs`). Integration must use the runtime's existing bounded database actor; no second writer may open the live database. Batches contain at most 200 metadata rows/messages. Metadata updates rename existing conversations; contacts without messages remain directory entries rather than invented conversation history.

HTTP requests require validated consumed profile/group/conversation HTTPS origins, session-encrypted parameters and isolated authenticated headers. The source service names are explicit at N470085/N470091/N470098. Directory requests use `ZaloPC-linux-24v681`. Limits are 15 seconds/request, 90 seconds/synchronization, 16 MiB/body including encrypted envelope, 128/32 KiB parsed headers, 99 group pages and 30,000 metadata rows/account. Authentication and ordinary session decryption retain their existing 2 MiB budgets. Error values retain fixed stage/category and numeric status only. Avatar URLs are cached metadata without cookie or automatic-download authority.

SQLite schema version 3 adds metadata and normalized search columns without deleting version-2 history/outgoing rows. Account/kind/name indexes bound keyset pages; literal substring search scans the account's bounded cache using persisted Vietnamese case/accent folding shared with the UI. Searches include numeric IDs and return at most 100 UI rows with a name/thread cursor; they do not search only the visible page. A partial record cannot clear previously known friendship/block flags. Missing presentation names are counted as unsupported, retain cached identity and make friend completion false; malformed identity/authority or conflicting duplicate IDs reject that response. A snapshot reaching the 20,000-friend cap reports possible truncation. No absence implies removal or revoked permissions.

Recent-preview loading is not full-history migration or cross-device/cloud synchronization. Legacy message IDs, timestamps, account routing and deduplication reuse the existing codec. Unsupported encrypted previews remain explicitly pending and do not consume or acknowledge a Signal queue. Older cloud/mobile history, permissioned group history and E2EE still require their own independently verified contracts and acceptance.

The modules are linked. `cargo test --locked --offline --all-features directory` passed 25 targeted tests at this checkpoint (protocol, real SQLite, real loopback TLS plus UI). TLS fixtures use a separate Node `crypto` oracle for AES-CBC request decoding and encrypted responses, never production sessions. Coverage includes multi-page groups, named cached conversations, repeat deduplication, cross-account isolation, version-2 migration, capacity rollback, partial names, full-cache Vietnamese search, >2 MiB metadata, redirects/size limits, cancellation and sink backpressure. Runtime actor integration has its own acceptance checks; no real-account directory or history acceptance has been performed.
