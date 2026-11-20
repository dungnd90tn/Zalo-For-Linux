use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    Accepted,
    Dropped,
    LateNotice,
    Encrypted,
    NotMember,
    Restricted,
    Peer,
    Rejected,
}

pub(super) fn messages(state: &State, mode: Mode) -> Value {
    let sent_at = *state.recall_source_at.get_or_init(now_ms);
    let sender = if matches!(mode, Mode::Peer) {
        "33"
    } else {
        "0"
    };
    let source = json!({"uidFrom":sender,"idTo":"22","msgId":"910","cliMsgId":"990",
        "ts":sent_at,"msgType":"webchat","content":"synthetic group recall"});
    let mut rows = vec![source];
    if matches!(mode, Mode::LateNotice) && state.http_sends.load(Ordering::SeqCst) > 0 {
        let notice = json!({"uidFrom":"0","idTo":"22","msgId":"999","ts":sent_at+1,
            "msgType":"chat.undo","content":{"globalMsgId":"910","cliMsgId":"990",
                "srcId":"11","destId":"22"}});
        rows.extend([notice.clone(), notice]);
    }
    json!({"error_code":0,"data":{"groupMsgs":rows}})
}

pub(super) fn respond(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    state: &State,
    cipher: &SessionCipher,
    mode: Mode,
    path: &str,
    headers: &str,
    body: &[u8],
) {
    let (route, query) = path.split_once('?').unwrap();
    assert!(query.starts_with("zpw_ver=681&zpw_type=24"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("cookie: zpw_sek=synthetic"));
    let is_get = route == "/api/group/getgi/v2";
    assert!(headers.starts_with(if is_get { "GET " } else { "POST " }));
    let params = url::form_urlencoded::parse(if is_get { query.as_bytes() } else { body })
        .find(|(key, _)| key == "params")
        .unwrap()
        .1;
    let params: Value = serde_json::from_slice(&cipher.decrypt(&params).unwrap()).unwrap();
    let mut code = 0;
    let data = match route {
        "/api/group/getgi/v2" => {
            assert_eq!(
                params,
                json!({"grid":"22","avatar_size":120,"member_avatar_size":120,
                "mpage":1,"mcount":100,"imei":"native-fixture"})
            );
            json!({"groupId":"22","e2ee":matches!(mode, Mode::Encrypted),
                "visibility":usize::from(matches!(mode, Mode::Restricted))})
        }
        "/api/group/getmg-v2" => {
            assert_eq!(params, json!({"gridVerMap":"{\"22\":0}"}));
            let member = if matches!(mode, Mode::NotMember) {
                "33_1"
            } else {
                "11_1"
            };
            json!({"gridInfoMap":{"22":{"groupId":"22","type":1,"creatorId":"77",
                "totalMember":2,"memVerList":["77_1",member],"adminIds":[]}}})
        }
        "/api/group/undomsg" => {
            assert_eq!(query, "zpw_ver=681&zpw_type=24");
            let client_id = params["clientId"].as_u64().unwrap();
            assert!(client_id > 990);
            assert_eq!(
                params,
                json!({"grid":"22","msgId":"910","cliMsgIdUndo":"990",
                "clientId":client_id,"visibility":0,"imei":"native-fixture"})
            );
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            if matches!(mode, Mode::Dropped | Mode::LateNotice) {
                return;
            }
            if matches!(mode, Mode::Rejected) {
                code = 211;
            }
            Value::Null
        }
        _ => panic!("unexpected synthetic recall route {route}"),
    };
    let encrypted = cipher
        .encrypt(&serde_json::to_vec(&json!({"error_code":code,"data":data})).unwrap())
        .unwrap();
    let response = serde_json::to_vec(&json!({"error_code":0,"data":encrypted})).unwrap();
    // Either preflight may be cancelled when the other authentication check fails.
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .and_then(|()| stream.write_all(&response))
    .and_then(|()| stream.flush());
}

#[tokio::test]
async fn group_recall_uses_scoped_proofs_and_recovers_ambiguous_ack() {
    use RecallOutcome::{Accepted, Rejected, Unavailable, Unknown};

    for (mode, outcome) in [
        (Mode::Accepted, Accepted),
        (Mode::Dropped, Unknown),
        (Mode::LateNotice, Unknown),
        (Mode::Encrypted, Unavailable),
        (Mode::NotMember, Unavailable),
        (Mode::Restricted, Unavailable),
        (Mode::Peer, Unavailable),
        (Mode::Rejected, Rejected),
    ] {
        let fixture = Fixture::new(Reply::GroupRecall(mode));
        let session = fixture.session();
        let runs = if matches!(mode, Mode::Dropped | Mode::LateNotice) {
            2
        } else {
            1
        };
        for run in 0..runs {
            let (commands, requests) = mpsc::channel(8);
            let (events, mut incoming) = mpsc::channel(32);
            let (stop, cancel) = watch::channel(false);
            let actor = tokio::spawn(run_with_transports(
                fixture.profile.clone(),
                session.clone(),
                requests,
                events,
                Arc::new(|| {}),
                cancel,
                fixture.transports(),
            ));
            tokio::time::timeout(Duration::from_secs(5), async {
                let notice_only = matches!(mode, Mode::LateNotice) && run == 1;
                let (mut online, mut source, mut redacted) = (false, run > 0, false);
                while !online || !source || (notice_only && !redacted) {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::Connection {
                            state: LiveConnection::Online,
                            ..
                        } => online = true,
                        LiveEvent::Received { message, .. } if message.id == "910" => {
                            assert_eq!(message.thread_id, "g:22");
                            source = true;
                        }
                        LiveEvent::Recalled {
                            thread,
                            message_id,
                            actor_id,
                            message,
                            ..
                        } => {
                            assert_eq!(
                                (thread.as_str(), message_id.as_str(), actor_id.as_str()),
                                ("g:22", "910", "11")
                            );
                            assert!(message.unwrap().is_recalled());
                            redacted = true;
                        }
                        _ => {}
                    }
                }
                if notice_only {
                    return;
                }
                let request_id = run + 1;
                commands
                    .send(LiveCommand::Recall {
                        request_id,
                        thread: "g:22".into(),
                        message_id: "910".into(),
                    })
                    .await
                    .unwrap();
                loop {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::Recalled {
                            thread,
                            message_id,
                            actor_id,
                            message,
                            ..
                        } => {
                            assert_eq!(
                                (thread.as_str(), message_id.as_str(), actor_id.as_str()),
                                ("g:22", "910", "11")
                            );
                            assert!(message.unwrap().is_recalled());
                            redacted = true;
                        }
                        LiveEvent::RecallFinished {
                            account_id,
                            request_id: actual_id,
                            thread,
                            message_id,
                            outcome: actual,
                        } => {
                            assert_eq!(
                                (
                                    account_id.as_str(),
                                    actual_id,
                                    thread.as_str(),
                                    message_id.as_str()
                                ),
                                ("11", request_id, "g:22", "910")
                            );
                            assert_eq!(actual, outcome, "{mode:?} run {run}");
                            assert_eq!(redacted, outcome == Accepted);
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap_or_else(|_| panic!("synthetic group recall timed out for {mode:?}, run {run}"));
            stop.send(true).unwrap();
            actor.await.unwrap();
        }
        assert_eq!(
            fixture.state.http_sends.load(Ordering::SeqCst),
            usize::from(outcome != Unavailable)
        );
        let store =
            MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        let group = store.page("11", "g:22", None, 10).unwrap();
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].id, "910");
        assert_eq!(
            group[0].is_recalled(),
            outcome == Accepted || matches!(mode, Mode::LateNotice)
        );
        assert!(store.page("11", "d:22", None, 10).unwrap().is_empty());
    }
}
