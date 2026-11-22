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
        "/api/message/sticker/category/list/v2" => {
            assert_eq!(mode, 4);
            let params = read_catalog_params(headers, query, cipher);
            assert!(params.is_empty());
            json!({
                "data":[
                    {"id":10047,"name":"Gói thử","iconUrl":"https://outside.invalid/cover"},
                    {"id":10048,"name":"Gói bị loại"},
                    {"id":10049,"name":"Gói riêng tư","isHidden":6},
                    {"id":10050,"name":"Gói ẩn","is_hidden":5},
                    {"id":0,"name":"Mặc định"}
                ],
                "exCateIds":[10048],
                "dataUrl":"https://wpa.zaloapp.com/untrusted-catalog", "cs":"only-inline-data"
            })
        }
        "/api/message/sticker/category/sticker_detail" => {
            assert_eq!(mode, 4);
            let params = read_catalog_params(headers, query, cipher);
            assert_eq!(
                serde_json::from_slice::<Value>(&params).unwrap(),
                json!({"cid":"10047"})
            );
            json!([
                {"id":20695,"cateId":10047,"type":3,"stickerUrl":"https://outside.invalid/image"},
                {"id":20696,"cateId":10047,"type":3,"status":1},
                {"id":20697,"cateId":10048,"type":3},
                {"id":20698,"cateId":10047,"type":3,"effectId":7},
                {"id":20695,"cateId":10047,"type":3}
            ])
        }
        "/api/message/sticker/search" | "/api/message/sticker/sticker_detail" => {
            assert_eq!(mode, 3);
            let params: Value =
                serde_json::from_slice(&read_catalog_params(headers, query, cipher)).unwrap();
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
            if matches!(mode, 3 | 4) {
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

fn read_catalog_params(headers: &str, query: &str, cipher: &SessionCipher) -> Vec<u8> {
    assert!(headers.starts_with("GET "));
    let params: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].0, "zpw_ver");
    assert_eq!(params[0].1, "681");
    assert_eq!(params[1].0, "zpw_type");
    assert_eq!(params[1].1, "24");
    assert_eq!(params[2].0, "params");
    cipher.decrypt(&params[2].1).unwrap()
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

#[tokio::test]
async fn catalog_pack_browsing_only_authorizes_current_scoped_ordinary_stickers() {
    let fixture = Fixture::new(Reply::StickerSend(4));
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
    let list = |request_id, thread: &str| LiveCommand::ListStickerPacks {
        request_id,
        thread: thread.into(),
    };
    let browse =
        |request_id, thread: &str, pack_list_id, category_id| LiveCommand::BrowseStickerPack {
            request_id,
            thread: thread.into(),
            pack_list_id,
            category_id,
        };
    let send = |request_id, search_id, thread: &str, sticker| LiveCommand::SendCatalogSticker {
        request_id,
        search_id,
        thread: thread.into(),
        sticker,
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
                LiveEvent::Fault { error, .. } => panic!("synthetic pack setup fault: {error}"),
                _ => {}
            }
        }

        commands.send(list(1, "d:22")).await.unwrap();
        assert_pack_list(&mut events, 1, "d:22").await;
        for (id, thread, token, category) in [
            (2, "g:22", 1, 10047),
            (3, "d:22", 99, 10047),
            (4, "d:22", 1, 10048),
        ] {
            commands
                .send(browse(id, thread, token, category))
                .await
                .unwrap();
            assert_eq!(pack_result(&mut events, id, thread, category).await, None);
        }

        commands.send(browse(5, "d:22", 1, 10047)).await.unwrap();
        assert_eq!(
            pack_result(&mut events, 5, "d:22", 10047).await,
            Some(vec![sticker])
        );
        commands.send(send(6, 5, "g:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 6).await,
            (StickerOutcome::Unavailable, None)
        );
        let hidden = StickerDescriptor {
            sticker_id: 20696,
            ..sticker
        };
        commands.send(send(7, 5, "d:22", hidden)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 7).await,
            (StickerOutcome::Unavailable, None)
        );

        commands.send(list(8, "d:22")).await.unwrap();
        assert_pack_list(&mut events, 8, "d:22").await;
        commands.send(send(9, 5, "d:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 9).await,
            (StickerOutcome::Unavailable, None)
        );

        commands.send(browse(10, "d:22", 8, 10047)).await.unwrap();
        assert_eq!(
            pack_result(&mut events, 10, "d:22", 10047).await,
            Some(vec![sticker])
        );
        commands.send(send(11, 10, "d:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 11).await,
            (StickerOutcome::Accepted, Some(sticker))
        );

        commands.send(list(12, "g:22")).await.unwrap();
        assert_pack_list(&mut events, 12, "g:22").await;
        commands.send(browse(13, "g:22", 12, 10047)).await.unwrap();
        assert_eq!(
            pack_result(&mut events, 13, "g:22", 10047).await,
            Some(vec![sticker])
        );
        commands.send(send(14, 13, "g:22", sticker)).await.unwrap();
        assert_eq!(
            outcome(&mut events, 14).await,
            (StickerOutcome::Accepted, Some(sticker))
        );
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 2);
    let requests = fixture.state.requests.lock().unwrap();
    for route in [
        "/api/message/sticker/category/list/v2?",
        "/api/message/sticker/category/sticker_detail?",
    ] {
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.starts_with(route))
                .count(),
            3
        );
    }
    assert!(!requests
        .iter()
        .any(|(path, _)| path.contains("untrusted-catalog")));
    let store =
        MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    for thread in ["d:22", "g:22"] {
        assert_eq!(
            store.page("11", thread, None, 2).unwrap()[0].sticker,
            Some(sticker)
        );
    }
}

async fn assert_pack_list(events: &mut mpsc::Receiver<LiveEvent>, id: u64, thread: &str) {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::StickerPacksFound {
                account_id,
                request_id,
                thread: actual,
                packs,
            } => {
                assert_eq!(account_id, "11");
                assert_eq!(request_id, id);
                assert_eq!(actual, thread);
                let packs = packs.expect("inline pack catalog");
                assert_eq!(packs.len(), 1);
                assert_eq!(packs[0].category_id, 10047);
                assert_eq!(packs[0].name, "Gói thử");
                return;
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic pack list fault: {error}"),
            _ => {}
        }
    }
}

async fn pack_result(
    events: &mut mpsc::Receiver<LiveEvent>,
    id: u64,
    thread: &str,
    category: u64,
) -> Option<Vec<StickerDescriptor>> {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::StickerPackFound {
                account_id,
                request_id,
                thread: actual,
                category_id,
                stickers,
            } => {
                assert_eq!(account_id, "11");
                assert_eq!(request_id, id);
                assert_eq!(actual, thread);
                assert_eq!(category_id, category);
                return stickers;
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic pack detail fault: {error}"),
            _ => {}
        }
    }
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
