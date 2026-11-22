use super::*;
use crate::domain::mention::Mention;

const TEXT: &str = "👋 @Ánh 😀 và @Bình";
const EVERYONE: &str = "👋 @All xem nhé";

fn selected_members() -> Vec<Mention> {
    vec![
        Mention {
            uid: "33".into(),
            pos: 3,
            len: 7,
        },
        Mention {
            uid: "44".into(),
            pos: 14,
            len: 5,
        },
    ]
}

pub(super) fn respond(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    path: &str,
    headers: &str,
    body: &[u8],
    all: bool,
) -> bool {
    let (route, query) = path.split_once('?').unwrap();
    let parameters = |encoded| {
        let (_, params) = url::form_urlencoded::parse(encoded)
            .find(|(key, _)| key == "params")
            .unwrap();
        serde_json::from_slice::<Value>(&cipher.decrypt(&params).unwrap()).unwrap()
    };
    let data = match route {
        "/api/conv/autodelete/getConvers" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(parameters(query.as_bytes()), json!({}));
            json!({"convers":[]})
        }
        "/api/group/getmg-v2" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(parameters(body), json!({"gridVerMap": "{\"22\":0}"}));
            json!({"gridInfoMap":{"22":{
                "groupId":"22","type":1,"creatorId":"11","totalMember":3,
                "memVerList":["11_1","33_1","44_1"],"adminIds":[],"setting":{"lockViewMember":false}
            }}})
        }
        "/api/group/getgi/v2" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(parameters(query.as_bytes())["grid"], "22");
            json!({"groupId":"22","e2ee":false,"visibility":0})
        }
        "/api/group/mention" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(query, "zpw_ver=681&zpw_type=24");
            let mut posted = parameters(body);
            let encoded = posted
                .as_object_mut()
                .unwrap()
                .remove("mentionInfo")
                .unwrap();
            let actual: Value = serde_json::from_str(encoded.as_str().unwrap()).unwrap();
            let expected = if all {
                json!([{"pos":3,"len":4,"uid":"-1","type":1}])
            } else {
                json!([
                    {"pos":3,"len":7,"uid":"33","type":0},
                    {"pos":14,"len":5,"uid":"44","type":0}
                ])
            };
            assert_eq!(actual, expected);
            let client_id = posted["clientId"].as_u64().unwrap();
            assert_eq!(
                posted,
                json!({
                    "grid":"22","message":if all { EVERYONE } else { TEXT },
                    "clientId":client_id,"language":"VN","visibility":0,"ttl":0
                })
            );
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            json!({"msgId":"950","ts":"1760000000080","clientId":client_id,"groupId":"22"})
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

#[tokio::test]
async fn group_mentions_reuse_the_scoped_roster_and_dispatch_exact_utf16_spans() {
    let fixture = Fixture::new(Reply::Mentions);
    let database = fixture.profile.paths.data_dir.join("messages.sqlite");
    MessageStore::open(&database)
        .unwrap()
        .open_conversation("11", "g:22", "Nhóm tổng hợp")
        .unwrap();
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
    tokio::time::timeout(Duration::from_secs(10), async {
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
                LiveEvent::Fault { error, .. } => panic!("synthetic mention setup: {error}"),
                _ => {}
            }
        }
        let current = revisions[1];
        assert_ne!(current, revisions[0]);
        let submit = |id, thread: &str, revision, mentions| LiveCommand::SendMentions {
            generation: 7,
            thread: thread.into(),
            text: TEXT.into(),
            mentions,
            privacy_revision: revision,
            request_id: id,
        };
        for (id, command) in [
            (1, submit(1, "d:22", current, selected_members())),
            (2, submit(2, "g:22", revisions[0], selected_members())),
        ] {
            commands.send(command).await.unwrap();
            assert!(settled(&mut events, id).await.is_none());
        }
        assert!(!fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.starts_with("/api/group/getgi/v2?")
                || path.starts_with("/api/group/getmg-v2?")));
        commands
            .send(LiveCommand::GroupMembers {
                request_id: 91,
                thread: "g:22".into(),
                privacy_revision: current,
                after: None,
            })
            .await
            .unwrap();
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::GroupMembers {
                    request_id: 91,
                    page: Some(page),
                    ..
                } => {
                    assert!(page.members.iter().any(|member| member.id == "33"));
                    break;
                }
                LiveEvent::GroupMembers { page: None, .. } => {
                    panic!("failed to verify synthetic group roster")
                }
                LiveEvent::Fault { error, .. } => panic!("synthetic roster fault: {error}"),
                _ => {}
            }
        }
        let mut removed = selected_members();
        removed[0].uid = "99".into();
        commands
            .send(submit(3, "g:22", current, removed))
            .await
            .unwrap();
        assert!(settled(&mut events, 3).await.is_none());
        commands
            .send(submit(4, "g:22", current, selected_members()))
            .await
            .unwrap();
        let message = settled(&mut events, 4).await.unwrap();
        assert_eq!(message.mentions, selected_members());
        assert_eq!(message.text, TEXT);
        assert_eq!(message.id, "950");
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 1);
    let requests = fixture.state.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.starts_with("/api/group/getmg-v2?"))
            .count(),
        1
    );
    assert!(!requests
        .iter()
        .any(|(path, _)| path.contains("/sendmsg") || path.contains("/message/sms")));
    let after_restart = MessageStore::open(&database).unwrap();
    let messages = after_restart.page("11", "g:22", None, 10).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].mentions, selected_members());
}

#[tokio::test]
async fn mention_all_requires_current_group_limit_and_preserves_the_exact_span() {
    for limit in [None, Some(2), Some(99)] {
        let fixture = Fixture::new(Reply::MentionAll);
        let database = fixture.profile.paths.data_dir.join("messages.sqlite");
        MessageStore::open(&database)
            .unwrap()
            .open_conversation("11", "g:22", "Nhóm tổng hợp")
            .unwrap();
        let origin = json!(["https://wpa.zaloapp.com"]);
        let mut raw = json!({
            "zpw_service_map_v3":{"chat":origin,"group":origin,"conversation":origin},
            "zpw_ws":["wss://wpa.zaloapp.com"]
        });
        if let Some(limit) = limit {
            raw["setttings"] = json!({"features":{"max_member_mention_all":limit}});
        }
        let config = ServerConfig::parse(&raw).unwrap();
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
        let allowed = limit == Some(99);
        let span = Mention {
            uid: "-1".into(),
            pos: 3,
            len: 4,
        };
        tokio::time::timeout(Duration::from_secs(12), async {
            let (mut online, mut revision) = (false, None);
            while !online || revision.is_none() {
                match events.recv().await.unwrap() {
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } => online = true,
                    LiveEvent::PrivacyChanged {
                        status: LivePrivacyStatus::Ready,
                        revision: id,
                        ..
                    } if id > 1 => revision = Some(id),
                    LiveEvent::Fault { error, .. } => {
                        panic!("synthetic mention-all setup: {error}")
                    }
                    _ => {}
                }
            }
            let revision = revision.unwrap();
            commands
                .send(LiveCommand::GroupMembers {
                    request_id: 91,
                    thread: "g:22".into(),
                    privacy_revision: revision,
                    after: None,
                })
                .await
                .unwrap();
            loop {
                if let LiveEvent::GroupMembers {
                    request_id: 91,
                    page,
                    ..
                } = events.recv().await.unwrap()
                {
                    let page = page.expect("fixture roster loads independently of mention-all");
                    assert_eq!(page.mention_all, allowed);
                    assert!(page.members.iter().all(|member| member.id != span.uid));
                    break;
                }
            }
            for (id, revision, permitted) in [(1, revision - 1, false), (2, revision, allowed)] {
                commands
                    .send(LiveCommand::SendMentions {
                        generation: 7,
                        thread: "g:22".into(),
                        text: EVERYONE.into(),
                        mentions: vec![span.clone()],
                        privacy_revision: revision,
                        request_id: id,
                    })
                    .await
                    .unwrap();
                let message = settled(&mut events, id).await;
                assert_eq!(message.is_some(), permitted);
                if let Some(message) = message {
                    assert_eq!(message.mentions.as_slice(), std::slice::from_ref(&span));
                }
            }
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        actor.await.unwrap();
        assert_eq!(
            fixture.state.http_sends.load(Ordering::SeqCst),
            usize::from(allowed)
        );
        let store = MessageStore::open(&database).unwrap();
        let messages = store.page("11", "g:22", None, 10).unwrap();
        assert_eq!(messages.len(), usize::from(allowed));
        if let Some(message) = messages.first() {
            assert_eq!(message.mentions, [span]);
        }
    }
}

async fn settled(events: &mut mpsc::Receiver<LiveEvent>, id: u64) -> Option<Message> {
    loop {
        match events.recv().await.unwrap() {
            LiveEvent::SendFailed {
                request_id,
                generation,
                unknown_delivery,
                server_accepted,
                ..
            } => {
                assert_eq!((request_id, generation), (id, 7));
                assert!(!unknown_delivery && !server_accepted);
                return None;
            }
            LiveEvent::Sent {
                request_id,
                generation,
                message,
                ..
            } => {
                assert_eq!((request_id, generation), (id, 7));
                return Some(message);
            }
            LiveEvent::Fault { error, .. } => panic!("synthetic mention failed: {error}"),
            _ => {}
        }
    }
}
