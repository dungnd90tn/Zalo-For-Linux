use super::*;
use crate::domain::StickerDescriptor;

pub(super) fn respond(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    mode: u8,
    path: &str,
    headers: &str,
    body: &[u8],
) -> bool {
    let (route, query) = path.split_once('?').unwrap();
    let data = match route {
        "/api/conv/autodelete/getConvers" => {
            assert!(headers.starts_with("GET "));
            let (_, encrypted) = url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == "params")
                .unwrap();
            assert_eq!(cipher.decrypt(&encrypted).unwrap(), b"{}");
            if mode == 1 {
                json!({"convers":[{"destId":"22","isGroup":false,"ttl":60}]})
            } else {
                json!({"convers":[]})
            }
        }
        "/api/message/sticker" | "/api/group/sticker" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(query, "zpw_ver=681&zpw_type=24");
            let (_, encrypted) = url::form_urlencoded::parse(body)
                .find(|(name, _)| name == "params")
                .unwrap();
            let payload: Value =
                serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
            let id = payload["clientId"].as_u64().unwrap();
            let direct = route == "/api/message/sticker";
            let address = if direct { "toid" } else { "grid" };
            let mut expected = json!({
                "stickerId":30,"cateId":0,"type":7,"clientId":id,
                "imei":"native-fixture","zsource":-1,"ttl":0
            });
            expected[address] = json!("22");
            assert_eq!(payload, expected);
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            if mode == 2 {
                return true; // The service receives the request and gives no response.
            }
            let mut ack =
                json!({"clientId":id,"msgId":if direct {"931"} else {"932"},"ts":"1760000000080"});
            ack[address] = json!("22");
            ack
        }
        _ => return false,
    };
    let message = json!({"error_code":0,"data":data});
    let response = serde_json::to_vec(&json!({
        "error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&message).unwrap()).unwrap()
    }))
    .unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .unwrap();
    stream.write_all(&response).unwrap();
    stream.flush().unwrap();
    true
}

#[tokio::test]
async fn actual_sticker_submit_is_typed_and_reserves_ambiguous_delivery_across_restart() {
    for (mode, thread, expected) in [
        (0, "d:22", StickerOutcome::Accepted),
        (0, "g:22", StickerOutcome::Accepted),
        (1, "d:22", StickerOutcome::Unavailable),
        (2, "d:22", StickerOutcome::Unknown),
    ] {
        let fixture = Fixture::new(Reply::StickerSend(mode));
        let path = fixture.profile.paths.data_dir.join("messages.sqlite");
        let sticker = StickerDescriptor::builtin(30);
        let runs = if mode == 2 { 2 } else { 1 };
        for request_id in 1..=runs {
            let (commands, rx) = mpsc::channel(16);
            let (tx, mut events) = mpsc::channel(64);
            let (stop, cancel) = watch::channel(false);
            let actor = tokio::spawn(run_with_transports(
                fixture.profile.clone(),
                fixture.session(),
                rx,
                tx,
                Arc::new(|| {}),
                cancel,
                fixture.transports(),
            ));
            tokio::time::timeout(Duration::from_secs(5), async {
                let (mut online, mut privacy_snapshots) = (false, 0_u8);
                while !online || privacy_snapshots < 2 {
                    match events.recv().await.unwrap() {
                        LiveEvent::Connection {
                            state: LiveConnection::Online,
                            ..
                        } => online = true,
                        LiveEvent::PrivacyChanged {
                            status: LivePrivacyStatus::Ready,
                            ..
                        } => privacy_snapshots += 1,
                        LiveEvent::Fault { error, .. } => panic!("synthetic setup fault: {error}"),
                        _ => {}
                    }
                }
                commands
                    .send(LiveCommand::SendSticker {
                        request_id,
                        thread: thread.into(),
                        sticker,
                    })
                    .await
                    .unwrap();
                let mut projected = false;
                loop {
                    match events.recv().await.unwrap() {
                        LiveEvent::Received {
                            message,
                            conversation,
                            ..
                        } if message.sticker.is_some() => {
                            assert_eq!(message.sticker, Some(sticker));
                            assert_eq!(message.sender, "11");
                            assert_eq!(message.thread_id, thread);
                            assert_eq!(conversation.last_text, crate::domain::STICKER_PREVIEW);
                            projected = true;
                        }
                        LiveEvent::StickerFinished {
                            thread: actual,
                            request_id: request,
                            outcome,
                            ..
                        } => {
                            assert_eq!(actual, thread);
                            assert_eq!(request, request_id);
                            assert_eq!(outcome, expected, "mode {mode}");
                            assert_eq!(projected, outcome == StickerOutcome::Accepted);
                            break;
                        }
                        LiveEvent::Sent { .. } | LiveEvent::SendSettled { .. } => {
                            panic!("Sticker must not settle a text draft");
                        }
                        LiveEvent::Fault { error, .. } => {
                            panic!("synthetic sticker fault: {error}")
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
            stop.send(true).unwrap();
            actor.await.unwrap();
        }
        assert_eq!(
            fixture.state.http_sends.load(Ordering::SeqCst),
            usize::from(mode != 1)
        );
        let store = MessageStore::open(&path).unwrap();
        let messages = store.page("11", thread, None, 5).unwrap();
        if mode == 0 {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].sticker, Some(sticker));
        } else {
            assert!(messages.is_empty());
        }
        let conn = rusqlite::Connection::open(path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM outgoing", [], |row| row
                .get::<_, usize>(0))
                .unwrap(),
            0
        );
    }
}
