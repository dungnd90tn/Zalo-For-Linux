use super::*;

pub(super) fn respond_group(
    stream: &mut impl Write,
    state: &State,
    cipher: &SessionCipher,
    path: &str,
    headers: &str,
    body: &[u8],
) -> bool {
    let (route, query) = path.split_once('?').unwrap();
    if !matches!(
        route,
        "/api/group/getgi/v2" | "/api/group/getmg-v2" | "/api/group/mforward"
    ) {
        return false;
    }
    assert!(query.starts_with("zpw_ver=681&zpw_type=24"));
    let get = route == "/api/group/getgi/v2";
    assert!(headers.starts_with(if get { "GET " } else { "POST " }));
    let (_, encrypted) = url::form_urlencoded::parse(if get { query.as_bytes() } else { body })
        .find(|(key, _)| key == "params")
        .unwrap();
    let params: Value = serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
    let data = match route {
        "/api/group/getgi/v2" => {
            let id = params["grid"].as_str().unwrap();
            assert!(matches!(id, "22" | "44" | "55" | "66"));
            json!({"groupId":id,"visibility":0,"e2ee":id=="66"})
        }
        "/api/group/getmg-v2" => {
            let map: Value = serde_json::from_str(params["gridVerMap"].as_str().unwrap()).unwrap();
            assert_eq!(map.as_object().unwrap().len(), 1);
            let id = map.as_object().unwrap().keys().next().unwrap();
            assert!(matches!(id.as_str(), "22" | "44" | "55" | "66"));
            json!({"gridInfoMap":{(id): {"groupId":id,"type":1,"creatorId":"33",
                "totalMember":2,"memVerList":["33_1","11_1"],"adminIds":[]}}})
        }
        "/api/group/mforward" => {
            assert_eq!(query, "zpw_ver=681&zpw_type=24");
            assert!(headers
                .to_ascii_lowercase()
                .contains("cookie: zpw_sek=synthetic"));
            let rows = params["grids"].as_array().unwrap();
            assert_eq!(rows.len(), 1);
            let grid = rows[0]["grid"].as_str().unwrap();
            let client = rows[0]["clientId"].as_str().unwrap();
            assert!(wire::numeric_id(client).is_ok());
            assert_eq!(rows[0], json!({"grid":grid,"clientId":client,"ttl":0}));
            assert!(matches!(grid, "22" | "55"));
            assert_eq!(params["ttl"], 0);
            assert_eq!(params["msgType"], "1");
            assert_eq!(params["totalIds"], 1);
            assert!(params.get("imei").is_none() && params.get("toIds").is_none());
            let info: Value = serde_json::from_str(params["msgInfo"].as_str().unwrap()).unwrap();
            assert_eq!(info["message"], "Tin tổng hợp được chuyển tiếp");
            let reference: Value =
                serde_json::from_str(info["reference"].as_str().unwrap()).unwrap();
            assert!(matches!(
                crate::protocol::forward::ForwardMarker::parse(&reference),
                Some(crate::protocol::forward::ForwardMarker::Parsed(_))
            ));
            state.http_sends.fetch_add(1, Ordering::SeqCst);
            if grid == "55" {
                return true;
            }
            json!({"success":[{"clientId":client,"msgId":"991","ts":1760000000099_i64}],"failed":[]})
        }
        _ => unreachable!(),
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

async fn expect_forward_batch(
    incoming: &mut mpsc::Receiver<LiveEvent>,
    request_id: u64,
    expected: &[(&str, ForwardOutcome)],
) {
    loop {
        match incoming.recv().await.unwrap() {
            LiveEvent::ForwardBatchFinished {
                account_id,
                request_id: actual,
                source_thread,
                source_message_id,
                results,
            } => {
                assert_eq!(actual, request_id);
                assert_eq!(
                    (
                        account_id.as_str(),
                        source_thread.as_str(),
                        source_message_id.as_str()
                    ),
                    ("11", "d:33", "910"),
                );
                let actual: Vec<_> = results
                    .iter()
                    .map(|result| (result.target_thread.as_str(), result.outcome))
                    .collect();
                assert_eq!(actual, expected);
                break;
            }
            LiveEvent::Sent { .. } | LiveEvent::SendSettled { .. } => {
                panic!("Forwarding must not settle composer drafts");
            }
            _ => {}
        }
    }
}

fn batch_session(fixture: &Fixture) -> (Arc<VerifiedSession>, PathBuf) {
    session_with_targets(fixture, false)
}

fn session_with_targets(fixture: &Fixture, groups: bool) -> (Arc<VerifiedSession>, PathBuf) {
    let mut services = json!({
        "chat":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"],
        "file":["https://wpa.zaloapp.com"]
    });
    if groups {
        services["group"] = json!(["https://wpa.zaloapp.com"]);
    }
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3":services,
        "zpw_ws":["wss://wpa.zaloapp.com"]
    }))
    .unwrap();
    let session = Arc::new(synthetic_session(fixture.profile.clone(), config));
    let db_path = fixture.profile.paths.data_dir.join("messages.sqlite");
    {
        let mut store = MessageStore::open(&db_path).unwrap();
        store
            .apply_directory_batch(&DirectoryBatch {
                account_id: "11".into(),
                entries: ["22", "44", "55", "66"]
                    .into_iter()
                    .map(|remote_id| crate::store::directory::DirectoryRecord {
                        remote_id: remote_id.into(),
                        is_group: false,
                        display_name: format!("Bạn {remote_id}"),
                        avatar_url: None,
                        is_friend: Some(true),
                        is_blocked: Some(false),
                        member_count: None,
                    })
                    .chain(["22", "44", "55", "66"].into_iter().filter(|_| groups).map(
                        |remote_id| crate::store::directory::DirectoryRecord {
                            remote_id: remote_id.into(),
                            is_group: true,
                            display_name: format!("Nhóm {remote_id}"),
                            avatar_url: None,
                            is_friend: None,
                            is_blocked: None,
                            member_count: Some(2),
                        },
                    ))
                    .collect(),
                messages: vec![],
                message_info: vec![],
                message_media: vec![],
                message_quotes: vec![],
                observed_at_ms: now_ms(),
            })
            .unwrap();
    }

    (session, db_path)
}

fn batch(request_id: u64, targets: &[&str]) -> LiveCommand {
    LiveCommand::ForwardBatch {
        request_id,
        source_thread: "d:33".into(),
        source_message_id: "910".into(),
        target_threads: targets.iter().copied().map(String::from).collect(),
    }
}

async fn await_source(incoming: &mut mpsc::Receiver<LiveEvent>, restored: bool) {
    let (mut online, mut source) = (false, restored);
    while !online || !source {
        match incoming.recv().await.unwrap() {
            LiveEvent::Connection {
                state: LiveConnection::Online,
                ..
            } => online = true,
            LiveEvent::Received { message, .. } if message.id == "910" => source = true,
            _ => {}
        }
    }
}

#[tokio::test]
async fn mixed_forward_batch_keeps_groups_scoped_and_does_not_retry_unknown() {
    use ForwardOutcome::{Accepted, Unavailable, Unknown};

    let fixture = Fixture::new(Reply::Forward(5));
    let (session, db_path) = session_with_targets(&fixture, true);
    for run in 0..2 {
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
        tokio::time::timeout(Duration::from_secs(12), async {
            await_source(&mut incoming, run > 0).await;
            if run == 0 {
                commands
                    .send(batch(1, &["d:22", "g:22", "g:44", "g:55", "g:66"]))
                    .await
                    .unwrap();
                expect_forward_batch(
                    &mut incoming,
                    1,
                    &[
                        ("d:22", Accepted),
                        ("g:22", Accepted),
                        ("g:44", Unavailable),
                        ("g:55", Unknown),
                        ("g:66", Unavailable),
                    ],
                )
                .await;
            } else {
                commands.send(batch(2, &["g:55"])).await.unwrap();
                expect_forward_batch(&mut incoming, 2, &[("g:55", Unknown)]).await;
            }
            assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 3);
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        actor.await.unwrap();
    }
    let store = MessageStore::open(&db_path).unwrap();
    for thread in ["d:22", "g:22"] {
        let page = store.page("11", thread, None, 10).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, "991");
        assert_eq!(page[0].thread_id, thread);
    }
    for thread in ["g:44", "g:55", "g:66"] {
        assert!(store.page("11", thread, None, 10).unwrap().is_empty());
    }
}

#[tokio::test]
async fn forward_batch_keeps_per_friend_results_and_reuses_the_ttl_snapshot() {
    use ForwardOutcome::{Accepted, Rejected, Unavailable, Unknown};

    let fixture = Fixture::new(Reply::Forward(3));
    let (session, db_path) = batch_session(&fixture);
    for run in 0..2 {
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
        tokio::time::timeout(Duration::from_secs(10), async {
            await_source(&mut incoming, run > 0).await;
            if run == 0 {
                commands.send(batch(1, &["d:22", "d:22"])).await.unwrap();
                expect_forward_batch(
                    &mut incoming,
                    1,
                    &[("d:22", Unavailable), ("d:22", Unavailable)],
                )
                .await;
                assert_eq!(fixture.state.forward_ttl_reads.load(Ordering::SeqCst), 0);
                assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 0);

                commands
                    .send(batch(2, &["d:22", "d:44", "d:55", "d:66"]))
                    .await
                    .unwrap();
                expect_forward_batch(
                    &mut incoming,
                    2,
                    &[
                        ("d:22", Accepted),
                        ("d:44", Rejected),
                        ("d:55", Unknown),
                        ("d:66", Unavailable),
                    ],
                )
                .await;
                assert_eq!(fixture.state.forward_ttl_reads.load(Ordering::SeqCst), 1);
                assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 3);

                commands.send(batch(3, &["d:44", "d:66"])).await.unwrap();
                expect_forward_batch(
                    &mut incoming,
                    3,
                    &[("d:44", Accepted), ("d:66", Unavailable)],
                )
                .await;
                assert_eq!(fixture.state.forward_ttl_reads.load(Ordering::SeqCst), 2);
            } else {
                commands.send(batch(4, &["d:55"])).await.unwrap();
                expect_forward_batch(&mut incoming, 4, &[("d:55", Unknown)]).await;
            }
            assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 4);
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        actor.await.unwrap();
    }

    let store = MessageStore::open(&db_path).unwrap();
    for (target, server_id) in [("d:22", "991"), ("d:44", "992")] {
        let messages = store.page("11", target, None, 10).unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.id == server_id)
                .count(),
            1
        );
    }
    let database = rusqlite::Connection::open(&db_path).unwrap();
    let ledger: (i64, i64) = database
        .query_row(
            "SELECT COUNT(*),COUNT(DISTINCT client_id) FROM forward_outbox
             WHERE account_id='11' AND source_thread_id='d:33' AND source_message_id='910'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(ledger, (4, 4));
}

#[tokio::test]
async fn cancelled_forward_batch_preserves_only_started_recipients() {
    let fixture = Fixture::new(Reply::Forward(4));
    let (session, db_path) = batch_session(&fixture);
    fixture.state.forward_hold.store(true, Ordering::SeqCst);
    for run in 0..2 {
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
        tokio::time::timeout(Duration::from_secs(5), async {
            await_source(&mut incoming, run > 0).await;
            if run == 0 {
                commands
                    .send(batch(1, &["d:22", "d:44", "d:55"]))
                    .await
                    .unwrap();
                while fixture.state.http_sends.load(Ordering::SeqCst) < 2 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } else {
                commands.send(batch(2, &["d:44"])).await.unwrap();
                expect_forward_batch(&mut incoming, 2, &[("d:44", ForwardOutcome::Unknown)]).await;
            }
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(5), actor)
            .await
            .unwrap()
            .unwrap();
        fixture.state.forward_hold.store(false, Ordering::SeqCst);
        assert_eq!(fixture.state.http_sends.load(Ordering::SeqCst), 2);
    }

    let database = rusqlite::Connection::open(&db_path).unwrap();
    let mut rows = database
        .prepare(
            "SELECT thread_id,state FROM forward_outbox WHERE account_id='11' ORDER BY thread_id",
        )
        .unwrap();
    let outcomes = rows
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        outcomes,
        [
            (String::from("d:22"), String::from("accepted")),
            (String::from("d:44"), String::from("unknown")),
        ]
    );
}
