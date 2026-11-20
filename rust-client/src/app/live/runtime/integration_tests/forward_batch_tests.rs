use super::*;

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
    let config = ServerConfig::parse(&json!({
        "zpw_service_map_v3":{
            "chat":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"],
            "file":["https://wpa.zaloapp.com"]
        },
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
