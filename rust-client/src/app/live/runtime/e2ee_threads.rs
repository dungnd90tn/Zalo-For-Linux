//! Lists the account's end-to-end encrypted conversations (command 563) so they appear in the
//! conversation list. Read-only on the account: it only asks for the list the original also fetches.
//! Their messages are not readable here yet; each shows as an empty, locked conversation.
use super::*;

const PAGES: usize = 20;
const PAGE_WAIT: Duration = Duration::from_secs(10);

impl Connection {
    /// Best effort: any failure leaves the conversation list as it was.
    pub(super) async fn sync_encrypted_threads(&mut self, db: &Database, events: &Events) {
        let Ok(peers) = self.encrypted_peers().await else {
            return;
        };
        if peers.is_empty() {
            return;
        }
        let account = events.account.clone();
        let listed = db
            .call(move |store| {
                let added = store.record_encrypted_threads(&account, &peers)?;
                let conversations = store.conversations(&account, crate::domain::MAX_PAGE_SIZE)?;
                Ok((added, conversations))
            })
            .await;
        let Ok((added, conversations)) = listed else {
            return;
        };
        if added == 0 {
            return;
        }
        let _ = events
            .emit(LiveEvent::DirectoryChanged {
                account_id: events.account.clone(),
                conversations,
            })
            .await;
        (events.repaint)();
    }
    pub(super) async fn encrypted_peers(&mut self) -> Result<Vec<String>> {
        let mut peers = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut last: Option<String> = None;
        for _ in 0..PAGES {
            let request = self.request();
            let data = match &last {
                Some(id) => json!({"reqIid":1,"first":false,"lastId":id}),
                None => json!({"reqIid":1,"first":true}),
            };
            self.send(563, 0, json!({"reqId":request,"data":data}))
                .await?;
            let deadline = tokio::time::Instant::now() + PAGE_WAIT;
            let page = loop {
                let frame = tokio::time::timeout_at(deadline, self.next())
                    .await
                    .map_err(|_| anyhow::anyhow!("E2EE list timed out"))??;
                if let Some(frame) = frame.filter(|frame| frame.cmd == 563) {
                    break wire::e2ee_list_page(&frame.payload, &request).map_err(|failure| {
                        anyhow::anyhow!("E2EE list rejected: {:?}", failure.reason)
                    })?;
                }
            };
            peers.extend(page.enabled);
            match page.next {
                None => return Ok(peers),
                Some(id) => {
                    if !seen.insert(id.clone()) {
                        return Ok(peers);
                    }
                    last = Some(id);
                }
            }
        }
        Ok(peers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two pages like the live service: page one has more, with `threads` and `uids` entries.
    #[tokio::test]
    async fn lists_enabled_peers_across_pages_and_shows_them_as_conversations() {
        use futures_util::{SinkExt, StreamExt};
        use std::io::Write;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            use base64::{engine::general_purpose::STANDARD, Engine};
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}}"#)
                .unwrap();
            let auth = wire::encode_frame(
                1,
                0,
                &json!({"error_code":0,"key":STANDARD.encode([7u8; 32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())}),
            )
            .unwrap();
            socket
                .send(tungstenite::Message::Binary(auth))
                .await
                .unwrap();
            let mut requests = Vec::new();
            for page in 0..2 {
                let Some(Ok(tungstenite::Message::Binary(bytes))) = socket.next().await else {
                    panic!("expected a 563 request");
                };
                let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                requests.push(body["data"].clone());
                let data = if page == 0 {
                    json!({"threads":[{"uid":"700"}],"waitUids":["800"],"uids":["701"],"pendingUids":[{"uid":"900"}],"hasMore":1,"lastId":"55"})
                } else {
                    json!({"threads":[{"uid":"702"}],"waitUids":[],"uids":[],"pendingUids":[]})
                };
                let frame = wire::encode_frame(
                    563,
                    0,
                    &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":data}}}),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(frame))
                    .await
                    .unwrap();
            }
            requests
        });
        let scratch =
            std::env::temp_dir().join(format!("zalo-e2ee-list-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let events = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.sync_encrypted_threads(&db, &events).await;
        let requests = server.await.unwrap();
        assert_eq!(requests[0], json!({"reqIid":1,"first":true}));
        assert_eq!(requests[1], json!({"reqIid":1,"first":false,"lastId":"55"}));
        // Threads and uids are listed; waiting and pending peers are not shown as conversations.
        let Some(LiveEvent::DirectoryChanged { conversations, .. }) = rx.recv().await else {
            panic!("expected the refreshed conversation list");
        };
        let mut ids: Vec<_> = conversations.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["d:700", "d:701", "d:702"]);
        assert!(conversations
            .iter()
            .all(|c| c.last_text == "[Cuộc trò chuyện mã hóa đầu cuối]"));
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
