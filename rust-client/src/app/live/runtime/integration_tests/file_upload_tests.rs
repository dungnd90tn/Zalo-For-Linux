//! Independent end-to-end assertions against synthetic TLS services.
use super::*;
use crate::domain::Attachment;
use md5::{Digest, Md5};

async fn online(events: &mut mpsc::Receiver<LiveEvent>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut connected, mut private) = (false, false);
        while !connected || !private {
            match events.recv().await.unwrap() {
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => connected = true,
                LiveEvent::PrivacyChanged {
                    status: LivePrivacyStatus::Ready,
                    ..
                } => private = true,
                LiveEvent::Fault { error, .. } => panic!("file startup: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}
fn attachment(f: &Fixture) -> (Attachment, Vec<u8>) {
    let bytes: Vec<u8> = (0..70003).map(|i| (i % 256) as u8).collect();
    let path = f.scratch.join("Bao ca\u{301}o.PDF");
    fs::write(&path, &bytes).unwrap();
    (Attachment::inspect(&path).unwrap(), bytes)
}
async fn terminal(events: &mut mpsc::Receiver<LiveEvent>, request: u64) -> FileTransferPhase {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::FileAccepted {
                    kind: TransferKind::File,
                    request_id,
                    message,
                    ..
                } if request_id == request => {
                    assert_eq!(message.id, "901");
                    return FileTransferPhase::Accepted;
                }
                LiveEvent::FileTransfer {
                    kind: TransferKind::File,
                    request_id,
                    phase,
                    ..
                } if request_id == request
                    && matches!(
                        phase,
                        FileTransferPhase::Failed
                            | FileTransferPhase::Cancelled
                            | FileTransferPhase::Unknown
                            | FileTransferPhase::Accepted
                            | FileTransferPhase::AcceptedLocalFailure
                    ) =>
                {
                    return phase
                }
                LiveEvent::Fault { error, .. } => panic!("file runtime fault: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn file_tls_chunks_exact_bytes_controls_and_duplicate_resource_feed_one_canonical_final_post()
{
    for (mode, thread) in [(0, "d:22"), (1, "d:22"), (2, "d:22"), (0, "g:22")] {
        let f = Fixture::new(Reply::FileUpload(mode));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let (attachment, bytes) = attachment(&f);
        commands
            .send(LiveCommand::SendFile {
                generation: 1,
                thread: thread.into(),
                attachment,
                request_id: 71,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut events, 71).await,
            FileTransferPhase::Accepted,
            "mode {mode}, {thread}"
        );
        let uploads = f.state.file.uploads.lock().unwrap().clone();
        assert_eq!(uploads.len(), if mode == 2 { 1 } else { 2 });
        let actual: Vec<u8> = uploads
            .iter()
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect();
        assert_eq!(
            actual,
            if mode == 2 {
                bytes[..64000].to_vec()
            } else {
                bytes.clone()
            }
        );
        for (index, (p, _)) in uploads.iter().enumerate() {
            assert_eq!(p["chunkId"], index + 1);
            assert_eq!(p["fileName"], "Bao ca\u{301}o.PDF");
            assert!(p.get("checksum").is_none());
        }
        let finals = f.state.file.finals.lock().unwrap().clone();
        assert_eq!(finals.len(), 1);
        let p = &finals[0];
        assert_eq!(p["fileName"], "Bao cáo.PDF");
        assert_eq!(p["extension"], "PDF");
        assert_eq!(p["fileUrl"], "https://files.example.test/uploaded");
        assert_eq!(p["checksum"], format!("{:x}", Md5::digest(&bytes)));
        assert_eq!(p["totalSize"], bytes.len());
        assert_eq!(p["fType"], 1);
        assert_eq!(p["fileCount"], 0);
        assert_eq!(p["fdata"], "{}");
        assert_eq!(p.get("imei").is_some(), thread.starts_with("d:"));
        assert!(!f.state.observed_commands.lock().unwrap().contains(&603));
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn file_tls_own_echo_wins_lost_matching_and_contradictory_http_ack() {
    for mode in [3, 4, 5, 6] {
        let f = Fixture::new(Reply::FileUpload(mode));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let (a, _) = attachment(&f);
        commands
            .send(LiveCommand::SendFile {
                generation: 1,
                thread: "d:22".into(),
                attachment: a,
                request_id: 72,
            })
            .await
            .unwrap();
        let phase = terminal(&mut events, 72).await;
        assert_eq!(
            phase,
            if mode == 3 {
                FileTransferPhase::Unknown
            } else {
                FileTransferPhase::Accepted
            },
            "mode {mode}"
        );
        // Allow the deliberately late HTTP result to settle after the own push.
        tokio::time::sleep(Duration::from_millis(250)).await;
        stop.send(true).unwrap();
        task.await.unwrap();
        let c =
            rusqlite::Connection::open(f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        let state: String = c
            .query_row("SELECT state FROM attachment_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            state,
            if mode == 3 { "unknown" } else { "accepted" },
            "mode {mode}"
        );
        assert_eq!(f.state.file.finals.lock().unwrap().len(), 1);
        if mode != 3 {
            let id: String = c
                .query_row(
                    "SELECT message_id FROM messages WHERE thread_id='d:22'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(id, "901");
        }
    }
}

#[tokio::test]
async fn file_tls_cancel_remains_responsive_during_probe_or_chunk_and_encrypted_targets_never_upload(
) {
    for mode in [7, 8, 21] {
        let f = Fixture::new(Reply::FileUpload(if mode == 21 { 0 } else { mode }));
        if mode == 21 {
            f.state.eligibility_mode.store(21, Ordering::SeqCst);
        }
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let (a, _) = attachment(&f);
        commands
            .send(LiveCommand::SendFile {
                generation: 1,
                thread: "d:22".into(),
                attachment: a,
                request_id: 73,
            })
            .await
            .unwrap();
        if mode != 21 {
            tokio::time::timeout(Duration::from_secs(4), async {
                while !f.state.file.stalled.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            commands
                .send(LiveCommand::CancelFile { request_id: 73 })
                .await
                .unwrap();
        }
        assert_eq!(
            terminal(&mut events, 73).await,
            if mode == 21 {
                FileTransferPhase::Failed
            } else {
                FileTransferPhase::Cancelled
            },
            "mode {mode}"
        );
        assert!(f.state.file.finals.lock().unwrap().is_empty());
        if mode != 7 {
            assert!(f.state.file.uploads.lock().unwrap().is_empty());
        }
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn file_tls_final_dispatch_cancel_is_unknown_but_privacy_cancel_before_dispatch_is_unsent() {
    for mode in [7, 9] {
        let f = Fixture::new(Reply::FileUpload(mode));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let (a, _) = attachment(&f);
        commands
            .send(LiveCommand::SendFile {
                generation: 1,
                thread: "d:22".into(),
                attachment: a,
                request_id: 74,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            while !f.state.file.stalled.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        if mode == 7 {
            f.state.privacy_policy.store(1, Ordering::SeqCst);
            commands.send(LiveCommand::RefreshPrivacy).await.unwrap();
        } else {
            commands
                .send(LiveCommand::CancelFile { request_id: 74 })
                .await
                .unwrap();
        }
        let phase = terminal(&mut events, 74).await;
        assert_eq!(
            phase,
            if mode == 9 {
                FileTransferPhase::Unknown
            } else {
                FileTransferPhase::Cancelled
            }
        );
        assert_eq!(
            f.state.file.finals.lock().unwrap().len(),
            usize::from(mode == 9)
        );
        stop.send(true).unwrap();
        task.await.unwrap();
        let c =
            rusqlite::Connection::open(f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        let state: String = c
            .query_row("SELECT state FROM attachment_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, if mode == 9 { "unknown" } else { "interrupted" });
    }
}

#[tokio::test]
async fn file_tls_changed_source_and_unknown_retry_never_upload_or_automatically_redispatch() {
    for changed in [true, false] {
        let f = Fixture::new(Reply::FileUpload(3));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let (a, _) = attachment(&f);
        if changed {
            fs::write(&a.path, [8u8; 70003]).unwrap();
        }
        commands
            .send(LiveCommand::SendFile {
                generation: 1,
                thread: "d:22".into(),
                attachment: a.clone(),
                request_id: 75,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut events, 75).await,
            if changed {
                FileTransferPhase::Failed
            } else {
                FileTransferPhase::Unknown
            }
        );
        if !changed {
            let count = f.state.file.uploads.lock().unwrap().len();
            commands
                .send(LiveCommand::SendFile {
                    generation: 1,
                    thread: "d:22".into(),
                    attachment: a,
                    request_id: 76,
                })
                .await
                .unwrap();
            assert_eq!(terminal(&mut events, 76).await, FileTransferPhase::Failed);
            assert_eq!(f.state.file.uploads.lock().unwrap().len(), count);
        } else {
            assert!(f.state.file.uploads.lock().unwrap().is_empty());
        }
        stop.send(true).unwrap();
        task.await.unwrap();
        // Starting the real actor with the same profile is recovery, not consent to reupload.
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.file_session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            f.state.file.finals.lock().unwrap().len(),
            usize::from(!changed)
        );
        stop.send(true).unwrap();
        drop(commands);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn file_tls_heartbeat_history_and_reconnect_stay_responsive_while_chunk_http_is_pending() {
    let f = Fixture::new(Reply::FileUpload(7));
    let mut transports = f.transports();
    transports.http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_der(&material().root).unwrap())
        .resolve("wpa.zaloapp.com", f.addr)
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.file_session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    online(&mut events).await;
    let (a, _) = attachment(&f);
    commands
        .send(LiveCommand::SendFile {
            generation: 1,
            thread: "d:22".into(),
            attachment: a,
            request_id: 77,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        while !f.state.file.stalled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.state.pings.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    commands
        .send(LiveCommand::Load {
            generation: 991,
            thread: "d:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Page {
                    generation: 991, ..
                } => break,
                LiveEvent::FileTransfer {
                    kind: TransferKind::File,
                    phase:
                        phase @ (FileTransferPhase::Cancelled
                        | FileTransferPhase::Failed
                        | FileTransferPhase::Unknown
                        | FileTransferPhase::Accepted
                        | FileTransferPhase::AcceptedLocalFailure),
                    ..
                } => {
                    panic!("Upload terminated before heartbeat/history proof: {phase:?}")
                }
                LiveEvent::FileAccepted {
                    kind: TransferKind::File,
                    ..
                } => panic!("Stalled chunk cannot be accepted"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let c = rusqlite::Connection::open_with_flags(
        f.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let phase: String = c
        .query_row("SELECT state FROM attachment_outbox", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        phase, "uploading",
        "Heartbeat/history must run while the original job is active"
    );
    assert_eq!(f.state.file.uploads.lock().unwrap().len(), 1);
    assert!(!f.state.file.final_started.load(Ordering::SeqCst));
    drop(c);
    commands.send(LiveCommand::Reconnect).await.unwrap();
    assert_eq!(
        terminal(&mut events, 77).await,
        FileTransferPhase::Cancelled
    );
    assert!(f.state.file.finals.lock().unwrap().is_empty());
    stop.send(true).unwrap();
    task.await.unwrap();
}
