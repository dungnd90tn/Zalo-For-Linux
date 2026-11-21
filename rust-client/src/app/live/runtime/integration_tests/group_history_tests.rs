use super::*;
use crate::domain::Cursor;

pub(super) fn respond(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    path: &str,
    headers: &str,
    body: &[u8],
) {
    let (route, query) = path.split_once('?').unwrap();
    assert!(query.starts_with("zpw_ver=681&zpw_type=24"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("cookie: zpw_sek=synthetic"));
    let roster = route == "/api/group/getmg-v2";
    assert!(headers.starts_with(if roster { "POST " } else { "GET " }));
    let encrypted = url::form_urlencoded::parse(if roster { body } else { query.as_bytes() })
        .find(|(key, _)| key == "params")
        .unwrap()
        .1;
    let parameters: Value = serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
    let (data, next) = match route {
        "/api/group/getgi/v2" => {
            assert_eq!(parameters["grid"], "22");
            (json!({"groupId":"22","e2ee":false,"visibility":0}), None)
        }
        "/api/group/getmg-v2" => {
            assert_eq!(parameters, json!({"gridVerMap":"{\"22\":0}"}));
            (
                json!({"gridInfoMap":{"22":{"groupId":"22","type":1,"creatorId":"33",
                "totalMember":2,"memVerList":["33_1","11_1"],"adminIds":[]}}}),
                None,
            )
        }
        "/api/cm/getrecentv2" => (
            json!({"isOld":1,"hasMore":1,"lastMsgId":"890","groupMsgs":[
                {"uidFrom":"33","msgId":"900","cliMsgId":"980",
                 "ts":1760000000900_i64,"msgType":"webchat","content":"Tin đã lưu từ đoạn gần"},
                {"uidFrom":"33","msgId":"890","cliMsgId":"970",
                 "ts":1760000000890_i64,"msgType":"webchat","content":"Tin tự xóa","ttl":60000}
            ]}),
            Some(910),
        ),
        "/api/cm/getoldv2" => (
            json!({"groupId":"22","isOld":true,"hasMore":false,"lastMsgId":880,"groupMsgs":[
                {"uidFrom":"0","idTo":"22","msgId":"880","cliMsgId":"960",
                 "ts":1760000000880_i64,"msgType":"webchat","content":"Tin từ kho cũ"}
            ]}),
            Some(890),
        ),
        _ => panic!("unexpected fixture route {route}"),
    };
    let data = if let Some(global_id) = next {
        assert_eq!(
            parameters,
            json!({
                "groupId":"22","globalMsgId":global_id,"count":50,"msgIds":[],
                "imei":"native-fixture","src":3
            })
        );
        state.http_sends.fetch_add(1, Ordering::SeqCst);
        Value::String(data.to_string())
    } else {
        data
    };
    let inner = serde_json::to_vec(&json!({"error_code":0,"data":data})).unwrap();
    let data = cipher.encrypt(&inner).unwrap();
    let body = serde_json::to_vec(&json!({"error_code":0,"data":data})).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
}

#[tokio::test]
async fn remote_group_history_restores_archive_cursor_across_local_paging() {
    let fixture = Fixture::new(Reply::GroupHistory);
    let origin = json!(["https://wpa.zaloapp.com"]);
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3": {
            "group":origin,"chat":origin,"conversation":origin,"group_cloud_message":origin
        },
        "zpw_ws":["wss://wpa.zaloapp.com"],
        "settings":{"features":{"cloud_setting":{"enable":true,"scroll_top_enable":1}}}
    }))
    .unwrap();
    let session = Arc::new(synthetic_session(fixture.profile.clone(), config));
    let (commands, requests) = mpsc::channel(16);
    let (events, mut incoming) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let actor = tokio::spawn(run_with_transports(
        fixture.profile.clone(),
        session,
        requests,
        events,
        Arc::new(|| {}),
        cancel,
        fixture.transports(),
    ));
    tokio::time::timeout(Duration::from_secs(8), async {
        let (mut online, mut anchor, mut revision, mut advertised) = (false, None, None, false);
        let mut unexpected_live = 0;
        while !online || anchor.is_none() || revision.is_none() || !advertised {
            match incoming.recv().await.unwrap() {
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => online = true,
                LiveEvent::RemoteHistoryCapability { available, .. } => advertised = available,
                LiveEvent::PrivacyChanged {
                    status: LivePrivacyStatus::Ready,
                    revision: id,
                    ..
                } if id > 1 => revision = Some(id),
                LiveEvent::Received { message, .. } if message.id == "910" => {
                    anchor = Some(message.cursor())
                }
                _ => {}
            }
        }
        let mut anchor = anchor.unwrap();
        let revision = revision.unwrap();
        let request =
            |request_id, generation, thread: &str, anchor: Cursor| LiveCommand::LoadRemoteHistory {
                request_id,
                generation,
                thread: thread.into(),
                anchor,
                privacy_revision: revision,
            };
        for (id, generation, thread, expected) in [
            (1, 1, "g:22", RemoteHistoryOutcome::Unavailable),
            (2, 1, "g:33", RemoteHistoryOutcome::Unavailable),
            (
                3,
                1,
                "g:22",
                RemoteHistoryOutcome::Imported {
                    count: 1,
                    has_more: true,
                },
            ),
            (
                4,
                2,
                "g:22",
                RemoteHistoryOutcome::Imported {
                    count: 1,
                    has_more: false,
                },
            ),
        ] {
            if id == 2 || id == 4 {
                commands
                    .send(LiveCommand::Load {
                        generation,
                        thread: "g:22".into(),
                        before: if id == 4 { Some(anchor.clone()) } else { None },
                    })
                    .await
                    .unwrap();
                if id == 4 {
                    loop {
                        if let LiveEvent::Page {
                            generation: 2,
                            messages,
                            ..
                        } = incoming.recv().await.unwrap()
                        {
                            assert_eq!(messages.len(), 1);
                            assert_eq!(messages[0].id, "900");
                            anchor = messages[0].cursor();
                            break;
                        }
                    }
                }
            }
            commands
                .send(request(id, generation, thread, anchor.clone()))
                .await
                .unwrap();
            loop {
                match incoming.recv().await.unwrap() {
                    LiveEvent::RemoteHistoryFinished {
                        account_id,
                        request_id,
                        generation: actual,
                        thread: actual_thread,
                        privacy_revision,
                        outcome,
                    } => {
                        assert_eq!(
                            (
                                account_id.as_str(),
                                request_id,
                                actual,
                                actual_thread.as_str(),
                                privacy_revision
                            ),
                            ("11", id, generation, thread, revision)
                        );
                        assert_eq!(outcome, expected);
                        break;
                    }
                    LiveEvent::Received { .. } | LiveEvent::Sent { .. } => unexpected_live += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(unexpected_live, 0);
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 2);
    let store =
        MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    let group = store.page("11", "g:22", None, 10).unwrap();
    let ids: Vec<_> = group.iter().map(|message| message.id.as_str()).collect();
    assert_eq!(ids, ["880", "900", "910"]);
    assert!(store.page("11", "g:33", None, 10).unwrap().is_empty());
}
