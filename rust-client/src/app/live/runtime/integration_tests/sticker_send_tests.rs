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
        "/api/message/sticker/search" | "/api/message/sticker/sticker_detail" => {
            assert_eq!(mode, 3);
            assert!(headers.starts_with("GET "));
            let (_, encrypted) = url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == "params")
                .unwrap();
            let params: Value =
                serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
            if route.ends_with("/search") {
                let keyword = params["keyword"].as_str().unwrap();
                assert_eq!(
                    params,
                    json!({"keyword":keyword,"limit":10,"srcType":0,"imei":"native-fixture"})
                );
                match keyword {
                    "chào" => json!([
                        {"sticker_id":20695,"cate_id":10047},
                        {"sticker_id":20696,"cate_id":10047},
                        {"sticker_id":0,"cate_id":10047},
                        {"sticker_id":"https://outside.invalid","cate_id":10047}
                    ]),
                    "rỗng" => json!([]),
                    "lỗi" => json!({"bad_shape":true}),
                    _ => unreachable!("fixture search keyword"),
                }
            } else {
                let id = params["sid"].as_u64().unwrap();
                assert_eq!(params, json!({"sid":id}));
                assert!(matches!(id, 20695 | 20696));
                json!({
                    "id":id, "cateId":if id == 20695 {10047} else {10048},
                    "type":3, "stickerUrl":"https://outside.invalid/path",
                })
            }
        }
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
            if mode == 3 {
                expected["stickerId"] = json!(20695);
                expected["cateId"] = json!(10047);
                expected["type"] = json!(3);
            }
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

#[test]
fn sticker_service_only_grants_credentials_to_verified_https_origin() {
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3":{
            "sticker":["https://wpa.zaloapp.com.outside.invalid","https://wpa.zaloapp.com/graft"],
            "chat":["https://wpa.zaloapp.com"],
        },
        "zpw_ws":["wss://wpa.zaloapp.com"],
    }))
    .unwrap();
    assert!(!config.services.contains_key("sticker"));
    for target in [
        "https://wpa.zaloapp.com.outside.invalid",
        "https://wpa.zaloapp.com/graft",
        "https://user@wpa.zaloapp.com/",
        "http://wpa.zaloapp.com/",
    ] {
        assert!(crate::protocol::sticker_catalog::url(
            &Url::parse(target).unwrap(),
            crate::protocol::sticker_catalog::SEARCH_PATH,
            crate::protocol::client::ClientType::PcLinux24,
            "synthetic"
        )
        .is_err());
    }
}

#[tokio::test]
async fn catalog_sticker_lookup_and_submit_require_exact_current_search_scope() {
    let fixture = Fixture::new(Reply::StickerSend(3));
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3":{
            "sticker":["https://wpa.zaloapp.com"],
            "chat":["https://wpa.zaloapp.com"],
            "group":["https://wpa.zaloapp.com"],
            "conversation":["https://wpa.zaloapp.com"],
        },
        "zpw_ws":["wss://wpa.zaloapp.com"],
    }))
    .unwrap();
    let session = Arc::new(synthetic_session(fixture.profile.clone(), config));
    assert!(session
        .authorization_cookie(&Url::parse("https://outside.invalid/").unwrap())
        .is_none());
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let actor = tokio::spawn(run_with_transports(
        fixture.profile.clone(),
        session,
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        fixture.transports(),
    ));
    let sticker = StickerDescriptor {
        sticker_id: 20695,
        category_id: 10047,
        sticker_type: 3,
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        let (mut online, mut ready, mut advertised) = (false, 0, false);
        while !online || ready < 2 || !advertised {
            match events.recv().await.unwrap() {
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => online = true,
                LiveEvent::PrivacyChanged {
                    status: LivePrivacyStatus::Ready,
                    ..
                } => ready += 1,
                LiveEvent::StickerSearchCapability { available, .. } => advertised = available,
                LiveEvent::Fault { error, .. } => panic!("synthetic catalog setup fault: {error}"),
                _ => {}
            }
        }
        commands
            .send(LiveCommand::SearchStickers {
                request_id: 1,
                thread: "d:22".into(),
                query: "chào".into(),
            })
            .await
            .unwrap();
        assert_eq!(lookup_result(&mut events, 1).await, Some(vec![sticker]));
        let select =
            |request_id, search_id, thread: &str, sticker| LiveCommand::SendCatalogSticker {
                request_id,
                search_id,
                thread: thread.into(),
                sticker,
            };
        for (id, command) in [
            (2, select(2, 1, "g:22", sticker)),
            (3, select(3, 99, "d:22", sticker)),
            (
                4,
                select(
                    4,
                    1,
                    "d:22",
                    StickerDescriptor {
                        sticker_id: 20696,
                        ..sticker
                    },
                ),
            ),
            (
                5,
                LiveCommand::SendSticker {
                    request_id: 5,
                    thread: "d:22".into(),
                    sticker,
                },
            ),
        ] {
            commands.send(command).await.unwrap();
            assert_eq!(
                outcome(&mut events, id).await,
                (StickerOutcome::Unavailable, None)
            );
        }
        commands.send(select(6, 1, "d:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 6).await,
            (StickerOutcome::Accepted, Some(sticker))
        );
        for (id, keyword, expected) in [(7, "rỗng", Some(vec![])), (8, "lỗi", None)] {
            commands
                .send(LiveCommand::SearchStickers {
                    request_id: id,
                    thread: "d:22".into(),
                    query: keyword.into(),
                })
                .await
                .unwrap();
            assert_eq!(lookup_result(&mut events, id).await, expected);
        }
        commands.send(select(9, 1, "d:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 9).await,
            (StickerOutcome::Unavailable, None)
        );
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with("/api/message/sticker/sticker_detail?"))
            .count(),
        2
    );
    let store =
        MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    assert_eq!(
        store.page("11", "d:22", None, 2).unwrap()[0].sticker,
        Some(sticker)
    );
}

async fn lookup_result(
    events: &mut mpsc::Receiver<LiveEvent>,
    id: u64,
) -> Option<Vec<StickerDescriptor>> {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::StickersFound {
                request_id,
                thread,
                stickers,
                ..
            } => {
                assert_eq!(request_id, id);
                assert_eq!(thread, "d:22");
                return stickers;
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic sticker lookup fault: {error}"),
            _ => {}
        }
    }
}

async fn outcome(
    events: &mut mpsc::Receiver<LiveEvent>,
    id: u64,
) -> (StickerOutcome, Option<StickerDescriptor>) {
    let mut received = None;
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::Received { message, .. } => received = message.sticker,
            LiveEvent::StickerFinished {
                request_id,
                outcome,
                ..
            } => {
                assert_eq!(request_id, id);
                return (outcome, received);
            }
            LiveEvent::Sent { .. } | LiveEvent::SendSettled { .. } => {
                panic!("sticker settled a draft")
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic sticker dispatch fault: {error}"),
            _ => {}
        }
    }
}
