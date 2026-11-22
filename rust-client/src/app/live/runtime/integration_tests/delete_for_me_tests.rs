use super::*;
use crate::app::live::{DeleteForMeMode, DeleteForMeOutcome};

pub(super) fn respond(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    state: &State,
    cipher: &SessionCipher,
    drop_receipt: bool,
    path: &str,
    headers: &str,
    body: &[u8],
) {
    assert_eq!(path, "/api/group/deletemsg?zpw_ver=681&zpw_type=24");
    assert!(headers.starts_with("POST "));
    assert!(headers
        .to_ascii_lowercase()
        .contains("cookie: zpw_sek=synthetic"));
    let encrypted = url::form_urlencoded::parse(body)
        .find(|(key, _)| key == "params")
        .unwrap()
        .1;
    let params: Value = serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
    let action_id = params["cliMsgId"].as_u64().unwrap();
    assert!(action_id > 990);
    assert_eq!(
        params,
        json!({"grid":"22","cliMsgId":action_id,"onlyMe":1,"msgs":[
            {"cliMsgId":"990","globalMsgId":"910","ownerId":"33","destId":"22"}
        ]})
    );
    state.http_sends.fetch_add(1, Ordering::SeqCst);
    if drop_receipt {
        return;
    }
    let inner = json!({"error_code":0,"data":{"cliMsgId":action_id,"grid":"22","onlyMe":1}});
    let response = serde_json::to_vec(&json!({"error_code":0,"data":
        cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()}))
    .unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .unwrap();
    stream.write_all(&response).unwrap();
    stream.flush().unwrap();
}

#[tokio::test]
async fn delete_for_me_tls_binds_only_me_and_never_retries_unknown() {
    use DeleteForMeMode::{Account, Device};
    use DeleteForMeOutcome::{Accepted, Unavailable, Unknown};
    for (enabled, mode, drop_receipt, outcome, attempts) in [
        (true, Account, false, Accepted, 1),
        (true, Account, true, Unknown, 1),
        (false, Device, false, Accepted, 0),
        (false, Account, false, Unavailable, 0),
        (true, Device, false, Unavailable, 0),
    ] {
        let fixture = Fixture::new(Reply::DeleteForMe(drop_receipt));
        let config = ServerConfig::parse(&json!({
            "zpw_service_map_v3":{
                "chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],
                "conversation":["https://wpa.zaloapp.com"]
            },
            "zpw_ws":["wss://wpa.zaloapp.com"],
            "setttings":{"features":{"sync_del_msg_or_conv":{"enable_sync_del_msg":enabled}}}
        }))
        .unwrap();
        let session = Arc::new(synthetic_session(fixture.profile.clone(), config));
        let runs = if drop_receipt { 2 } else { 1 };
        for request_id in 1..=runs {
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
                let (mut online, mut source, mut privacy) = (false, request_id > 1, false);
                while !(online && source && privacy) {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::Connection {
                            state: LiveConnection::Online,
                            ..
                        } => online = true,
                        LiveEvent::Received { message, .. } if message.id == "910" => {
                            assert_eq!(
                                (message.thread_id.as_str(), message.sender.as_str()),
                                ("g:22", "33")
                            );
                            source = true;
                        }
                        LiveEvent::PrivacyChanged {
                            revision,
                            status: LivePrivacyStatus::Ready,
                            ..
                        } => privacy |= revision > 1,
                        LiveEvent::DeleteForMeCapability { account_sync, .. } => {
                            assert_eq!(account_sync, enabled)
                        }
                        _ => {}
                    }
                }
                commands
                    .send(LiveCommand::DeleteForMe {
                        request_id,
                        thread: "g:22".into(),
                        message_id: "910".into(),
                        mode,
                    })
                    .await
                    .unwrap();
                let mut seen_change = false;
                loop {
                    match incoming.recv().await.unwrap() {
                        LiveEvent::DeletedForMe {
                            account_id,
                            thread,
                            message_id,
                            sender_id,
                            ..
                        } => {
                            assert_eq!(
                                (
                                    account_id.as_str(),
                                    thread.as_str(),
                                    message_id.as_str(),
                                    sender_id.as_str()
                                ),
                                ("11", "g:22", "910", "33")
                            );
                            seen_change = true;
                        }
                        LiveEvent::DeleteForMeFinished {
                            request_id: actual,
                            outcome: result,
                            ..
                        } => {
                            assert_eq!((actual, result), (request_id, outcome));
                            assert_eq!(seen_change, outcome == Accepted);
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap_or_else(|_| panic!("synthetic delete timed out: {mode:?} {outcome:?}"));
            stop.send(true).unwrap();
            actor.await.unwrap();
        }
        assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), attempts);
        let store =
            MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        assert_eq!(
            store.page("11", "g:22", None, 10).unwrap().is_empty(),
            outcome == Accepted
        );
    }
}
