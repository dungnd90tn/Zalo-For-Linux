//! Synthetic HTTPS and WebSocket transport witness for the direct voice endpoint.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ReplyKind {
    Direct,
    Group,
    Rejected,
    Lost,
}

const MP3: &[u8] = include_bytes!("../../../../../tests/fixtures/messaging/voice_synthetic.mp3");

pub(super) fn respond(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    state: &State,
    mode: ReplyKind,
    path: &str,
    headers: &str,
    body: &[u8],
) -> bool {
    let Some((route, query)) = path.split_once('?') else {
        return false;
    };
    if !matches!(route, "/api/message/voicemp3" | "/api/group/sendmp3") {
        return false;
    }
    let group = matches!(mode, ReplyKind::Group);
    let (route_expected, scope, recipient) = if group {
        ("/api/group/sendmp3", "11", "grid")
    } else {
        ("/api/message/voicemp3", "2", "toid")
    };
    assert_eq!(route, route_expected);
    assert!(headers.starts_with("POST "));
    let fields: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
    assert_eq!(fields.len(), 4);
    let field = |expected: &str| {
        fields
            .iter()
            .find(|(name, _)| name == expected)
            .unwrap()
            .1
            .as_ref()
    };
    assert_eq!(field("zpw_ver"), "681");
    assert_eq!(field("zpw_type"), "24");
    assert_eq!(field("type"), scope);
    let cipher =
        SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
    let parameters: Value =
        serde_json::from_slice(&cipher.decrypt(field("params")).unwrap()).unwrap();
    let id = parameters["clientId"].as_str().unwrap();
    assert!(id.parse::<u64>().unwrap() > 0);
    assert_eq!(parameters, json!({"clientId":id,(recipient):"22"}));
    let content_type = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .unwrap()
        .1
        .trim();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let prefix = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"fileContent\"; filename=\"voice.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n"
    );
    let suffix = format!("\r\n--{boundary}--\r\n");
    let uploaded = body.strip_prefix(prefix.as_bytes()).unwrap();
    assert_eq!(uploaded.strip_suffix(suffix.as_bytes()).unwrap(), MP3);
    state.http_sends.fetch_add(1, Ordering::SeqCst);
    if matches!(mode, ReplyKind::Lost) {
        return true;
    }
    let inner = if matches!(mode, ReplyKind::Rejected) {
        json!({"error_code":229})
    } else if group {
        json!({"error_code":0,"data":{"msgId":"901","ts":1760000000001_i64,"grid":"22","cliMsgId":id}})
    } else {
        json!({"error_code":0,"data":{"msgId":"901","ts":1760000000001_i64}})
    };
    let value = cipher
        .encrypt(&serde_json::to_vec(&inner).unwrap())
        .unwrap();
    let response = serde_json::to_vec(&json!({"error_code":0,"data":value})).unwrap();
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

async fn terminal(
    incoming: &mut mpsc::Receiver<LiveEvent>,
    thread: &str,
    wanted: u64,
) -> FileTransferPhase {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match incoming.recv().await.unwrap() {
                LiveEvent::FileTransfer {
                    kind: TransferKind::Voice,
                    account_id,
                    thread: subject,
                    request_id,
                    phase,
                    ..
                } if request_id == wanted
                    && matches!(
                        phase,
                        FileTransferPhase::Accepted
                            | FileTransferPhase::Unknown
                            | FileTransferPhase::Failed
                            | FileTransferPhase::AcceptedLocalFailure
                            | FileTransferPhase::Cancelled
                    ) =>
                {
                    assert_eq!(account_id, "11");
                    assert_eq!(subject, thread);
                    return phase;
                }
                LiveEvent::Fault { error, .. } => panic!("synthetic voice runtime: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn voice_tls_posts_only_bound_mp3_and_never_retries_unknown_dispatch() {
    for (mode, thread, expected) in [
        (ReplyKind::Direct, "d:22", FileTransferPhase::Accepted),
        (ReplyKind::Group, "g:22", FileTransferPhase::Accepted),
        (ReplyKind::Rejected, "d:22", FileTransferPhase::Failed),
        (ReplyKind::Lost, "d:22", FileTransferPhase::Unknown),
    ] {
        let f = Fixture::new(Reply::VoiceSend(mode));
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
        tokio::time::timeout(Duration::from_secs(5), async {
            let (mut online, mut confirmed, mut capable) = (false, false, false);
            while !online || !confirmed || !capable {
                match events.recv().await.unwrap() {
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } => online = true,
                    LiveEvent::PrivacyChanged {
                        revision,
                        status: LivePrivacyStatus::Ready,
                        ..
                    } => confirmed |= revision > 1,
                    LiveEvent::VoiceCapability { available, .. } => capable = available,
                    LiveEvent::Fault { error, .. } => panic!("synthetic voice startup: {error}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        for request_id in 1..=if matches!(mode, ReplyKind::Lost) {
            2
        } else {
            1
        } {
            commands
                .send(LiveCommand::SendVoice {
                    generation: 1,
                    thread: thread.to_owned(),
                    recording: PreparedVoice::fixture(MP3.to_vec(), 1_000),
                    request_id,
                })
                .await
                .unwrap();
            assert_eq!(
                terminal(&mut events, thread, request_id).await,
                if request_id == 1 {
                    expected
                } else {
                    FileTransferPhase::Failed
                }
            );
        }
        stop.send(true).unwrap();
        task.await.unwrap();
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 1);
        let db =
            rusqlite::Connection::open(f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        let (owner, scope, phase, server, checksum): (
            String,
            String,
            String,
            Option<String>,
            Vec<u8>,
        ) = db
            .query_row(
                "SELECT account_id,thread_id,state,server_id,checksum FROM voice_outbox",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(owner, "11");
        assert_eq!(scope, thread);
        assert_eq!(
            phase,
            match mode {
                ReplyKind::Direct | ReplyKind::Group => "accepted",
                ReplyKind::Rejected => "rejected",
                ReplyKind::Lost => "unknown",
            }
        );
        assert_eq!(
            server.as_deref(),
            (expected == FileTransferPhase::Accepted).then_some("901")
        );
        assert_eq!(
            checksum.as_slice(),
            <sha2::Sha256 as sha2::Digest>::digest(MP3).as_slice()
        );
        let voice_messages: i64 = db
            .query_row(
                "SELECT count(*) FROM message_media WHERE kind='chat.voice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(voice_messages, 0, "no guessed media resource from an ACK");
    }
}
