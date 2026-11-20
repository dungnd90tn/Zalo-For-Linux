//! Account-bound reaction catch-up on the same synthetic TLS/WSS as chat.
use super::*;

type ServerSocket = tungstenite::WebSocket<StreamOwned<ServerConnection, TcpStream>>;

fn send(socket: &mut ServerSocket, command: u16, data: Value) {
    let outer = json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":data}});
    socket
        .send(tungstenite::Message::Binary(
            wire::encode_frame(command, 0, &outer).unwrap(),
        ))
        .unwrap();
}

fn peer_notice(event: u64, kind: i64, icon: &str, group: bool) -> Value {
    let (target, client, peer, recipient) = if group {
        (911, 991, "44", "33")
    } else {
        (910, 990, "22", "0")
    };
    json!({
        "uidFrom":peer,"idTo":recipient,"msgId":event,"ts":1760000000000u64+event,
        "msgType":"chat.reaction",
        "content":json!({"rMsg":[{"gMsgID":target,"cMsgID":client,"msgType":1}],
            "rIcon":icon,"rType":kind}).to_string()
    })
}

pub(super) fn socket(mut socket: ServerSocket, state: Arc<State>) {
    let first_connection = state.upgrades.load(Ordering::SeqCst) == 1;
    socket
        .get_mut()
        .sock
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if state.release_reaction_removal.swap(false, Ordering::SeqCst) {
            send(
                &mut socket,
                612,
                json!({"reacts":[
                    peer_notice(81, 3, "/-strong", false),
                    peer_notice(83, -1, "", false)
                ]}),
            );
        }
        let bytes = match socket.read() {
            Ok(tungstenite::Message::Binary(bytes)) => bytes,
            Ok(tungstenite::Message::Close(_)) => return,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return,
            Ok(_) => continue,
        };
        let command = u16::from_le_bytes([bytes[1], bytes[2]]);
        assert_eq!(bytes[3], 0);
        let request: Value = serde_json::from_slice(&bytes[4..]).unwrap();
        let first = request["first"].as_bool().unwrap();
        let last = request["lastId"].as_u64().unwrap();
        let request_id = &request["reqId"];
        assert_eq!(request["preIds"], json!([]));
        assert!(request_id.as_str().unwrap().starts_with("native_req_"));
        state.socket_requests.lock().unwrap().push((command, last));
        match (first_connection, command, first) {
            (_, 510, true) => {}
            (true, 610, true) => {
                assert_eq!(last, 1);
                send(
                    &mut socket,
                    610,
                    json!({
                        "reqId":request_id,"more":true,"lastActionId":"100",
                        "queueStatus":{"610_0":{"lastId":"130"}},
                        "reacts":[peer_notice(81,3,"/-strong",false)]
                    }),
                );
            }
            (true, 611, true) => {
                assert_eq!(last, 1);
                send(
                    &mut socket,
                    611,
                    json!({
                        "reqId":request_id,"more":false,"lastActionId":210,
                        "queueStatus":{"610_0":{"lastId":"333"}},
                        "reactGroups":[peer_notice(82,5,"/-heart",true)]
                    }),
                );
            }
            (true, 610, false) => {
                assert_eq!(last, 100);
                send(
                    &mut socket,
                    610,
                    json!({
                        "reqId":request_id,"more":false,"lastActionId":199,"reacts":{"malformed":true}
                    }),
                );
                // Group source arrives after its reaction. The direct target
                // is deliberately withheld across the worker restart.
                send(
                    &mut socket,
                    510,
                    json!({"lastActionId":900,"groupMsgs":[
                        {"uidFrom":"44","idTo":"33","msgId":"911","cliMsgId":"991","ts":1760000000000_i64,
                         "msgType":"webchat","content":"synthetic group source"}
                    ]}),
                );
            }
            (false, 610, true) => {
                assert_eq!(last, 100);
                let mut other_device = peer_notice(82, 5, "/-heart", false);
                other_device["uidFrom"] = json!("11");
                other_device["idTo"] = json!("22");
                other_device["cliMsgId"] = json!("1212");
                send(
                    &mut socket,
                    610,
                    json!({
                        "reqId":request_id,"more":false,"lastActionId":200,"reacts":[other_device]
                    }),
                );
            }
            (false, 611, true) => {
                assert_eq!(last, 210);
                send(&mut socket, 611, json!({"reqId":request_id,"more":false}));
                send(
                    &mut socket,
                    510,
                    json!({"lastActionId":901,"msgs":[
                        {"uidFrom":"22","idTo":"11","msgId":"910","cliMsgId":"990","ts":1760000000000_i64,
                         "msgType":"webchat","content":"synthetic direct source"}
                    ]}),
                );
            }
            other => panic!("unexpected control queue request {other:?}"),
        }
    }
}

fn ordinary_event(event: &LiveEvent) {
    assert!(!matches!(
        event,
        LiveEvent::Connection {
            state: LiveConnection::Offline | LiveConnection::Expired,
            ..
        } | LiveEvent::Fault { .. }
            | LiveEvent::Sent { .. }
            | LiveEvent::SendSettled { .. }
    ));
}

async fn snapshot(
    commands: &mpsc::Sender<LiveCommand>,
    incoming: &mut mpsc::Receiver<LiveEvent>,
    thread: &str,
    id: &str,
    generation: u64,
) -> Vec<ReactionSummary> {
    commands
        .send(LiveCommand::ReactionState {
            generation,
            thread: thread.into(),
            message_ids: vec![id.into()],
        })
        .await
        .unwrap();
    loop {
        let event = incoming.recv().await.unwrap();
        ordinary_event(&event);
        if let LiveEvent::ReactionState {
            generation: request,
            mut messages,
            ..
        } = event
        {
            assert_eq!(request, generation);
            assert_eq!(messages.len(), 1);
            let (source, summary) = messages.pop().unwrap();
            assert_eq!(source, id);
            return summary;
        }
    }
}

#[tokio::test]
async fn advertised_reaction_catchup_restarts_once_and_preserves_deferred_sources() {
    let fixture = Fixture::new(Reply::ReactionReplay);
    let db_path = fixture.profile.paths.data_dir.join("messages.sqlite");
    for run in 0..2 {
        let (commands, requests) = mpsc::channel(16);
        let (events, mut incoming) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let actor = tokio::spawn(run_with_transports(
            fixture.profile.clone(),
            fixture.session(),
            requests,
            events,
            Arc::new(|| {}),
            cancel,
            fixture.transports(),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            let (mut source, mut privacy) = (false, false);
            while !(source && privacy) {
                let event = incoming.recv().await.unwrap();
                ordinary_event(&event);
                match event {
                    LiveEvent::Received { message, .. } => {
                        assert_eq!(message.id, if run == 0 {"911"} else {"910"});
                        source = true;
                    }
                    LiveEvent::PrivacyChanged { revision, .. } => privacy |= revision > 1,
                    _ => {}
                }
            }
            let summary = snapshot(&commands, &mut incoming, "g:33", "911", 1).await;
            assert_eq!(summary, [ReactionSummary {kind: ReactionKind::Heart, count: 1, mine: false}]);
            if run == 1 {
                let summary = snapshot(&commands, &mut incoming, "d:22", "910", 2).await;
                assert_eq!(summary, [
                    ReactionSummary {kind: ReactionKind::Like, count: 1, mine: false},
                    ReactionSummary {kind: ReactionKind::Heart, count: 1, mine: true},
                ]);
                fixture.state.release_reaction_removal.store(true, Ordering::SeqCst);
                loop {
                    let event = incoming.recv().await.unwrap();
                    ordinary_event(&event);
                    if matches!(event, LiveEvent::ReactionsChanged { thread, .. } if thread == "d:22") {
                        break;
                    }
                }
                let summary = snapshot(&commands, &mut incoming, "d:22", "910", 3).await;
                assert_eq!(summary, [ReactionSummary {kind: ReactionKind::Heart, count: 1, mine: true}]);
            }
        }).await.unwrap();
        stop.send(true).unwrap();
        actor.await.unwrap();
        let store = MessageStore::open(&db_path).unwrap();
        assert_eq!(
            store.receive_cursor("11", "610_0").unwrap(),
            if run == 0 { "100" } else { "200" }
        );
        assert_eq!(store.receive_cursor("11", "611_0").unwrap(), "210");
        assert_eq!(
            store.receive_cursor("11", "510_0").unwrap(),
            if run == 0 { "900" } else { "901" }
        );
    }
    let mut requests = fixture.state.socket_requests.lock().unwrap().clone();
    requests.sort_unstable();
    assert_eq!(
        requests,
        [
            (510, 1),
            (510, 900),
            (610, 1),
            (610, 100),
            (610, 100),
            (611, 1),
            (611, 210)
        ]
    );
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 0);
}
