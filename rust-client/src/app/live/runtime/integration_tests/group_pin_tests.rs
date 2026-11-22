use super::*;

const TEXT: &str = "Tin nhắn ghim tổng hợp 👋";

pub(super) fn respond(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    lost: bool,
    path: &str,
    headers: &str,
    body: &[u8],
) -> bool {
    let (route, query) = path.split_once('?').unwrap();
    let parameters = |encoded| {
        let (_, encrypted) = url::form_urlencoded::parse(encoded)
            .find(|(name, _)| name == "params")
            .unwrap();
        serde_json::from_slice::<Value>(&cipher.decrypt(&encrypted).unwrap()).unwrap()
    };
    let data = match route {
        "/api/conv/autodelete/getConvers" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(parameters(query.as_bytes()), json!({}));
            json!({"convers":[]})
        }
        "/api/group/getgi/v2" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(parameters(query.as_bytes())["grid"], "22");
            json!({"groupId":"22","e2ee":false,"visibility":0})
        }
        "/api/group/getmg-v2" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(parameters(body), json!({"gridVerMap":"{\"22\":0}"}));
            json!({"gridInfoMap":{"22":{
                "groupId":"22","type":1,"creatorId":"11","totalMember":2,
                "memVerList":["11_1","33_1"],"adminIds":[],"setting":{"setTopicOnly":1}
            }}})
        }
        "/api/board/pin/list" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(
                parameters(query.as_bytes()),
                json!({"groupId":"22","boardVersion":0,"imei":"native-fixture"})
            );
            let phase = state.pin_fixture_board.load(Ordering::SeqCst);
            let mut items = vec![json!({"type":0,"id":8})];
            if phase == 1 {
                items.push(json!({"type":2,"id":12,"params":{
                    "global_msg_id":"910","client_msg_id":"990","senderUid":"33","msg_type":1
                }}));
            }
            json!({"groupId":"22","boardVersion":phase+1,"pinLimit":3,"items":items})
        }
        "/api/board/topic/createv2" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(query, "zpw_ver=681&zpw_type=24");
            let mut params = parameters(body);
            let content = params.as_object_mut().unwrap().remove("params").unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(content.as_str().unwrap()).unwrap(),
                json!({
                    "client_msg_id":"990","global_msg_id":"910","senderUid":"33",
                    "senderName":"","title":TEXT,"msg_type":1
                })
            );
            assert_eq!(
                params,
                json!({
                    "grid":"22","type":2,"color":-16777216,"emoji":"📌","startTime":-1,
                    "duration":-1,"repeat":0,"src":-1,"imei":"native-fixture","pinAct":1
                })
            );
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            if lost {
                return true;
            }
            assert_eq!(state.pin_fixture_board.swap(1, Ordering::SeqCst), 0);
            json!({"topicId":12,"boardVersion":2})
        }
        "/api/board/unpinv2" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(
                parameters(query.as_bytes()),
                json!({
                    "grid":"22","imei":"native-fixture","topic":{"topicId":12,"topicType":2},"boardVersion":2
                })
            );
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            assert_eq!(state.pin_fixture_board.swap(2, Ordering::SeqCst), 1);
            json!({"boardVersion":3})
        }
        _ => return false,
    };
    let inner = json!({"error_code":0,"data":data});
    let response = serde_json::to_vec(&json!({
        "error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()
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

fn session(fixture: &Fixture) -> Arc<VerifiedSession> {
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3": {
            "group_board":["https://wpa.zaloapp.com"],
            "group":["https://wpa.zaloapp.com"],
            "chat":["https://wpa.zaloapp.com"],
            "conversation":["https://wpa.zaloapp.com"],
        },
        "zpw_ws":["wss://wpa.zaloapp.com"],
        "settings":{"features":{"group_topics":{"enable":1}}}
    }))
    .unwrap();
    assert!(config.group_board_v2);
    Arc::new(synthetic_session(fixture.profile.clone(), config))
}

fn seed(fixture: &Fixture) -> PathBuf {
    let path = fixture.profile.paths.data_dir.join("messages.sqlite");
    let mut store = MessageStore::open(&path).unwrap();
    store
        .insert_batch(&[Message {
            account_id: "11".into(),
            thread_id: "g:22".into(),
            id: "910".into(),
            sender: "33".into(),
            text: TEXT.into(),
            sent_at_ms: 1760000000079,
            source_client_id: Some("990".into()),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            media: None,
            sticker: None,
        }])
        .unwrap();
    path
}

async fn ready(events: &mut mpsc::Receiver<LiveEvent>) -> (u64, u64) {
    let (mut online, mut revisions) = (false, Vec::new());
    while !online || revisions.len() < 2 {
        match events.recv().await.unwrap() {
            LiveEvent::Connection {
                state: LiveConnection::Online,
                ..
            } => online = true,
            LiveEvent::PrivacyChanged {
                status: LivePrivacyStatus::Ready,
                revision,
                ..
            } => revisions.push(revision),
            LiveEvent::Fault { error, .. } => panic!("synthetic pin setup: {error}"),
            _ => {}
        }
    }
    assert_ne!(revisions[0], revisions[1]);
    (revisions[0], revisions[1])
}

fn update(id: u64, revision: u64, message: &str, pinned: bool) -> LiveCommand {
    LiveCommand::SetGroupPin {
        request_id: id,
        thread: "g:22".into(),
        privacy_revision: revision,
        message_id: message.into(),
        pinned,
    }
}

async fn outcome(
    events: &mut mpsc::Receiver<LiveEvent>,
    id: u64,
    expected: GroupPinOutcome,
) -> Option<GroupPinSnapshot> {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::GroupPinFinished {
                request_id,
                account_id,
                thread,
                outcome,
                snapshot,
                ..
            } => {
                assert_eq!(
                    (request_id, account_id.as_str(), thread.as_str()),
                    (id, "11", "g:22")
                );
                assert_eq!(outcome, expected);
                return snapshot;
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic pin state fault: {error}"),
            _ => {}
        }
    }
}

async fn snapshot(events: &mut mpsc::Receiver<LiveEvent>, id: u64) -> GroupPinSnapshot {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::GroupPins {
                request_id,
                snapshot,
                ..
            } => {
                assert_eq!(request_id, id);
                return snapshot.expect("authoritative board snapshot");
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic board list fault: {error}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn group_pin_and_unpin_use_authenticated_board_state_and_bound_source() {
    let fixture = Fixture::new(Reply::GroupPins(false));
    let path = seed(&fixture);
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let actor = tokio::spawn(run_with_transports(
        fixture.profile.clone(),
        session(&fixture),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        fixture.transports(),
    ));
    tokio::time::timeout(Duration::from_secs(10), async {
        let (stale, revision) = ready(&mut events).await;
        commands.send(update(1, stale, "910", true)).await.unwrap();
        assert!(outcome(&mut events, 1, GroupPinOutcome::Unavailable)
            .await
            .is_none());
        commands
            .send(update(2, revision, "911", true))
            .await
            .unwrap();
        assert!(outcome(&mut events, 2, GroupPinOutcome::Unavailable)
            .await
            .is_none());
        assert!(!fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.contains("/api/board/")));

        commands
            .send(LiveCommand::GroupPins {
                request_id: 3,
                thread: "g:22".into(),
                privacy_revision: revision,
            })
            .await
            .unwrap();
        let before = snapshot(&mut events, 3).await;
        assert!(before.can_mutate && before.entries.is_empty());
        commands
            .send(update(4, revision, "910", true))
            .await
            .unwrap();
        let after = outcome(&mut events, 4, GroupPinOutcome::Accepted)
            .await
            .unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].message_id, "910");
        assert_eq!(after.entries[0].hit.as_ref().unwrap().snippet, TEXT);
        assert!(after.unresolved.is_empty());

        commands
            .send(update(5, revision, "910", false))
            .await
            .unwrap();
        let after = outcome(&mut events, 5, GroupPinOutcome::Accepted)
            .await
            .unwrap();
        assert!(after.entries.is_empty() && after.unresolved.is_empty());
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 2);
    let connection = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM group_pin_intent", [], |row| row
                .get::<_, usize>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn ambiguous_pin_remains_blocked_after_restart_until_a_newer_board_confirms_it() {
    let fixture = Fixture::new(Reply::GroupPins(true));
    let path = seed(&fixture);
    for retry in [false, true] {
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let actor = tokio::spawn(run_with_transports(
            fixture.profile.clone(),
            session(&fixture),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            fixture.transports(),
        ));
        tokio::time::timeout(Duration::from_secs(10), async {
            let (_, revision) = ready(&mut events).await;
            commands
                .send(update(1, revision, "910", true))
                .await
                .unwrap();
            let uncertain = outcome(&mut events, 1, GroupPinOutcome::Unknown)
                .await
                .unwrap();
            assert_eq!(uncertain.unresolved, ["910"]);
            assert!(uncertain.entries.is_empty());
            assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 1);
            if retry {
                fixture.state.pin_fixture_board.store(1, Ordering::SeqCst);
                commands
                    .send(LiveCommand::GroupPins {
                        request_id: 2,
                        thread: "g:22".into(),
                        privacy_revision: revision,
                    })
                    .await
                    .unwrap();
                let reconciled = snapshot(&mut events, 2).await;
                assert!(reconciled.unresolved.is_empty());
                assert_eq!(reconciled.entries[0].message_id, "910");
            }
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        actor.await.unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM group_pin_intent", [], |row| row
                    .get::<_, usize>(0))
                .unwrap(),
            usize::from(!retry)
        );
    }
}
