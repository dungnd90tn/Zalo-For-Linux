use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VoteReply {
    Accepted,
    Lost,
}

pub(super) fn poll_notice() -> Value {
    json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"groupMsgs":[
        {"uidFrom":"33","idTo":"22","msgId":"910","cliMsgId":"990",
         "ts":1760000000999_i64,"msgType":"group.poll","content":{"params":{
            "msg":{"vi":"%1$s đã tạo bình chọn %2$s"},
            "question":"Nhóm chọn ngày nào?","pollId":71,"isAnonymous":true
         }}}
    ]}}})
}

pub(super) fn respond(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    path: &str,
    headers: &str,
    body: &[u8],
    vote: Option<VoteReply>,
) -> bool {
    let (route, query) = path.split_once('?').unwrap();
    let parameters = |encoded| {
        let (_, encrypted) = url::form_urlencoded::parse(encoded)
            .find(|(name, _)| name == "params")
            .unwrap();
        serde_json::from_slice::<Value>(&cipher.decrypt(&encrypted).unwrap()).unwrap()
    };
    let data = match route {
        "/api/group/getgi/v2" => {
            assert!(headers.starts_with("GET "));
            assert_eq!(parameters(query.as_bytes())["grid"], "22");
            json!({"groupId":"22","e2ee":false,"visibility":0})
        }
        "/api/group/getmg-v2" => {
            assert!(headers.starts_with("POST "));
            assert_eq!(parameters(body), json!({"gridVerMap":"{\"22\":0}"}));
            json!({"gridInfoMap":{"22":{"groupId":"22","type":1,"creatorId":"33",
                "totalMember":2,"memVerList":["33_1","11_1"],"adminIds":[]}}})
        }
        "/api/poll/detail" => {
            assert!(headers.starts_with("GET "));
            assert!(query.starts_with("zpw_ver=681&zpw_type=24&"));
            assert!(headers
                .to_ascii_lowercase()
                .contains("cookie: zpw_sek=synthetic"));
            assert_eq!(
                parameters(query.as_bytes()),
                json!({"poll_id":71,"imei":"native-fixture"})
            );
            let (wrong_id, joined) = match vote {
                None => {
                    let response = state.http_sends.fetch_add(1, Ordering::SeqCst);
                    (response == 2, response == 1)
                }
                Some(mode) => (
                    false,
                    mode == VoteReply::Accepted && state.http_sends.load(Ordering::SeqCst) != 0,
                ),
            };
            json!({"poll_id":if wrong_id {72} else {71},"group_id":"22",
                "question":"Nhóm chọn ngày nào?","closed":false,"is_anonymous":true,
                "is_hide_vote_preview":true,"joined":joined,"allow_multi_choices":false,"expired_time":0,
                "options":[{"option_id":1,"content":"Thứ bảy","votes":2,"voted":joined},
                           {"option_id":2,"content":"Chủ nhật","votes":0,"voted":false}]})
        }
        "/api/poll/vote" if vote.is_some() => {
            assert!(headers.starts_with("GET "));
            assert!(query.starts_with("zpw_ver=681&zpw_type=24&"));
            assert!(headers
                .to_ascii_lowercase()
                .contains("cookie: zpw_sek=synthetic"));
            assert_eq!(
                parameters(query.as_bytes()),
                json!({"poll_id":71,"option_ids":[1],"imei":"native-fixture"})
            );
            assert_eq!(state.http_sends.fetch_add(1, Ordering::SeqCst), 0);
            if vote == Some(VoteReply::Lost) {
                return true;
            }
            json!({"options":[{"option_id":1,"content":"Thứ bảy","votes":2,"voted":true}]})
        }
        _ => return false,
    };
    let inner = serde_json::to_vec(&json!({"error_code":0,"data":data})).unwrap();
    let response = serde_json::to_vec(&json!({
        "error_code":0,"data":cipher.encrypt(&inner).unwrap()
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
    let origin = json!(["https://wpa.zaloapp.com"]);
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3":{
            "group":origin,"chat":origin,"conversation":origin,"group_poll":origin
        },"zpw_ws":["wss://wpa.zaloapp.com"]
    }))
    .unwrap();
    Arc::new(synthetic_session(fixture.profile.clone(), config))
}

#[tokio::test]
async fn actual_group_poll_detail_is_scoped_and_results_obey_privacy() {
    let fixture = Fixture::new(Reply::GroupPoll);
    let session = session(&fixture);
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

    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut online, mut saved, mut revision, mut enabled) = (false, false, None, false);
        while !online || !saved || revision.is_none() || !enabled {
            match incoming.recv().await.unwrap() {
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => online = true,
                LiveEvent::PollDetailCapability { available, .. } => enabled = available,
                LiveEvent::PrivacyChanged {
                    status: LivePrivacyStatus::Ready,
                    revision: id,
                    ..
                } if id > 1 => revision = Some(id),
                LiveEvent::Received { message, .. } if message.id == "910" => saved = true,
                LiveEvent::Fault { error, .. } => panic!("synthetic poll startup failed: {error}"),
                _ => {}
            }
        }
        let revision = revision.unwrap();
        commands
            .send(LiveCommand::Load {
                generation: 1,
                thread: "g:22".into(),
                before: None,
            })
            .await
            .unwrap();
        while !matches!(
            incoming.recv().await.unwrap(),
            LiveEvent::Page { generation: 1, .. }
        ) {}

        for (id, thread, message, request_revision, expected) in [
            (1, "g:33", "910", revision, None),
            (2, "g:22", "911", revision, None),
            (3, "g:22", "910", revision - 1, None),
            (4, "g:22", "910", revision, Some([None, None])),
            (5, "g:22", "910", revision, Some([Some(2), Some(0)])),
            (6, "g:22", "910", revision, None),
        ] {
            commands
                .send(LiveCommand::LoadPollDetail {
                    generation: 1,
                    thread: thread.into(),
                    message_id: message.into(),
                    request_id: id,
                    privacy_revision: request_revision,
                })
                .await
                .unwrap();
            loop {
                match incoming.recv().await.unwrap() {
                    LiveEvent::PollDetail {
                        account_id,
                        thread: actual_thread,
                        message_id,
                        request_id,
                        generation,
                        poll_id,
                        detail,
                        ..
                    } => {
                        assert_eq!(
                            (
                                account_id.as_str(),
                                actual_thread.as_str(),
                                message_id.as_str(),
                                request_id,
                                generation
                            ),
                            ("11", thread, message, id, 1)
                        );
                        assert_eq!(poll_id, expected.map(|_| 71));
                        assert_eq!(
                            detail.as_ref().map(|details| {
                                assert_eq!(details.question, "Nhóm chọn ngày nào?");
                                assert_eq!(details.options[0].text, "Thứ bảy");
                                assert_eq!(details.options[1].text, "Chủ nhật");
                                assert!(details.anonymous);
                                [details.options[0].votes, details.options[1].votes]
                            }),
                            expected
                        );
                        break;
                    }
                    LiveEvent::Fault { error, .. } => {
                        panic!("synthetic poll request failed: {error}")
                    }
                    _ => {}
                }
            }
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    actor.await.unwrap();
    assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn actual_group_poll_vote_checks_fresh_choice_and_never_resends_after_restart() {
    use crate::app::live::PollVoteOutcome;

    for mode in [VoteReply::Accepted, VoteReply::Lost] {
        let fixture = Fixture::new(Reply::GroupPollVote(mode));
        let session = session(&fixture);
        let first = if mode == VoteReply::Accepted {
            PollVoteOutcome::Accepted
        } else {
            PollVoteOutcome::Unknown
        };
        let restarted = if mode == VoteReply::Lost {
            PollVoteOutcome::Unknown
        } else {
            PollVoteOutcome::Joined
        };
        for (restart, expected) in [first, restarted].into_iter().enumerate() {
            let (commands, requests) = mpsc::channel(16);
            let (events, mut incoming) = mpsc::channel(64);
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
            tokio::time::timeout(Duration::from_secs(20), async {
                let (mut online, mut revision) = (false, None);
                while !online || revision.is_none() {
                    match incoming.recv().await.unwrap() {
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
                            panic!("synthetic vote setup failed: {error}")
                        }
                        _ => {}
                    }
                }
                let revision = revision.unwrap();
                commands
                    .send(LiveCommand::Load {
                        generation: 1,
                        thread: "g:22".into(),
                        before: None,
                    })
                    .await
                    .unwrap();
                while !matches!(
                    incoming.recv().await.unwrap(),
                    LiveEvent::Page { generation: 1, .. }
                ) {}

                let choices = if restart == 0 {
                    vec![
                        ("Phương án cũ", PollVoteOutcome::Unavailable),
                        ("Thứ bảy", expected),
                    ]
                } else {
                    vec![("Thứ bảy", expected)]
                };
                for (index, (text, expected)) in choices.into_iter().enumerate() {
                    let id = index as u64 + 1;
                    commands
                        .send(LiveCommand::VotePoll {
                            generation: 1,
                            thread: "g:22".into(),
                            message_id: "910".into(),
                            request_id: id,
                            privacy_revision: revision,
                            poll_id: 71,
                            option_id: 1,
                            option_text: text.into(),
                        })
                        .await
                        .unwrap();
                    loop {
                        match incoming.recv().await.unwrap() {
                            LiveEvent::PollVoteFinished {
                                account_id,
                                thread,
                                message_id,
                                request_id,
                                generation,
                                outcome,
                                ..
                            } => {
                                assert_eq!(
                                    (
                                        account_id.as_str(),
                                        thread.as_str(),
                                        message_id.as_str(),
                                        request_id,
                                        generation
                                    ),
                                    ("11", "g:22", "910", id, 1)
                                );
                                assert_eq!(outcome, expected);
                                break;
                            }
                            LiveEvent::Fault { error, .. } => {
                                panic!("synthetic vote failed: {error}")
                            }
                            _ => {}
                        }
                    }
                }
            })
            .await
            .unwrap();
            stop.send(true).unwrap();
            actor.await.unwrap();
            assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 1);
        }
    }
}
