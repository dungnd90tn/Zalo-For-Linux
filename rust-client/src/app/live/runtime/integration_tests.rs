//! Actual loopback TLS + RFC6455 integration. No request can leave the loopback listener.
use super::*;
use crate::protocol::{
    bootstrap::{ServerConfig, SessionCipher},
    http::tls_tests::material,
    session::synthetic_session,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConnection, StreamOwned,
};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
};

#[derive(Clone, Copy)]
enum Reply {
    Normal,
    Redirect,
    Oversize,
    Stall,
    Runtime,
    RuntimeStall,
    OfflinePages,
    OfflineCycle,
}
#[derive(Default)]
struct State {
    requests: Mutex<Vec<(String, String)>>,
    sni: Mutex<Vec<String>>,
    closed: AtomicUsize,
    socket_requests: Mutex<Vec<(u16, u64)>>,
}
struct Fixture {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
    profile: Arc<Profile>,
    scratch: PathBuf,
}
impl Fixture {
    fn new(reply: Reply) -> Self {
        let material = material();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(material.leaf.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(material.key.clone())),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0")
            .expect("loopback TLS messaging tests require socket permission");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(State::default());
        let flag = stop.clone();
        let data = state.clone();
        let config = Arc::new(config);
        let thread = std::thread::spawn(move || {
            let mut children = Vec::new();
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let config = config.clone();
                        let data = data.clone();
                        children.push(std::thread::spawn(move || {
                            serve(socket, config, data, reply)
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::park_timeout(Duration::from_millis(2))
                    }
                    Err(e) => panic!("local accept: {e}"),
                }
            }
            for child in children {
                child.join().unwrap();
            }
        });
        let scratch = std::env::temp_dir().join(format!(
            "native-message-wire-{}-{}",
            std::process::id(),
            crate::domain::local_message_id().unwrap()
        ));
        fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(Profile::at_root(&scratch, "fixture").unwrap());
        Self {
            addr,
            stop,
            state,
            thread: Some(thread),
            profile,
            scratch,
        }
    }
    fn session(&self) -> Arc<VerifiedSession> {
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"]})).unwrap();
        Arc::new(synthetic_session(self.profile.clone(), config))
    }
    fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .tls_built_in_root_certs(false)
            .add_root_certificate(reqwest::Certificate::from_der(&material().root).unwrap())
            .resolve("wpa.zaloapp.com", self.addr)
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    }
    fn transports(&self) -> Transports {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(material().root.clone()))
            .unwrap();
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Transports {
            http: self.client(),
            local: Some((self.addr, Arc::new(tls))),
        }
    }
    async fn connection(&self) -> Connection {
        let url = Url::parse("wss://wpa.zaloapp.com?zpw_ver=681&zpw_type=30").unwrap();
        let mut request = url.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Cookie", "zpw_sek=synthetic".parse().unwrap());
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(material().root.clone()))
            .unwrap();
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let config = WebSocketConfig {
            max_message_size: Some(wire::MAX_FRAME),
            max_frame_size: Some(wire::MAX_FRAME),
            ..Default::default()
        };
        let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
            request,
            tcp,
            Some(config),
            Some(tokio_tungstenite::Connector::Rustls(Arc::new(tls))),
        )
        .await
        .unwrap();
        Connection {
            socket,
            decoder: FrameDecoder::default(),
            queues: Default::default(),
            offline_progress: Default::default(),
            request_id: 0,
            last_pong: tokio::time::Instant::now(),
            last_ping: None,
            authenticated_at: None,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.scratch);
    }
}
fn serve(
    mut socket: TcpStream,
    config: Arc<rustls::ServerConfig>,
    state: Arc<State>,
    reply: Reply,
) {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut tls = ServerConnection::new(config).unwrap();
    while tls.is_handshaking() {
        if tls.complete_io(&mut socket).is_err() {
            return;
        }
    }
    state
        .sni
        .lock()
        .unwrap()
        .push(tls.server_name().unwrap().into());
    let mut stream = StreamOwned::new(tls, socket);
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        if stream.read(&mut byte).ok() != Some(1) {
            return;
        }
        request.push(byte[0]);
        assert!(request.len() < 32 * 1024);
    }
    let headers = String::from_utf8(request).unwrap();
    let path = headers
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let field = |name: &str| {
        headers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_owned())
    };
    assert_eq!(field("host").as_deref(), Some("wpa.zaloapp.com"));
    assert_eq!(field("cookie").as_deref(), Some("zpw_sek=synthetic"));
    if let Some(key) = field("sec-websocket-key") {
        let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
        write!(stream,"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").unwrap();
        stream.flush().unwrap();
        let mut ws = tungstenite::WebSocket::from_raw_socket(
            stream,
            tungstenite::protocol::Role::Server,
            Some(WebSocketConfig {
                max_frame_size: Some(wire::MAX_FRAME),
                max_message_size: Some(wire::MAX_FRAME),
                ..Default::default()
            }),
        );
        let auth = json!({"error_code":0,"data":{"qCmds":if matches!(reply,Reply::Runtime|Reply::RuntimeStall){json!([{"cmd":510,"subCmd":0},{"cmd":511,"subCmd":0}])}else if matches!(reply,Reply::OfflinePages|Reply::OfflineCycle){json!([{"cmd":510,"subCmd":0}])}else{json!([])}}});
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&serde_json::to_vec(&auth).unwrap()).unwrap();
        ws.send(tungstenite::Message::Binary(wire::encode_frame(1,0,&json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())})).unwrap())).unwrap();
        loop {
            let message = match ws.read() {
                Ok(m) => m,
                Err(_) => {
                    state.closed.fetch_add(1, Ordering::SeqCst);
                    break;
                }
            };
            match message {
                tungstenite::Message::Binary(bytes) => {
                    let cmd = u16::from_le_bytes([bytes[1], bytes[2]]);
                    let data: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                    if cmd == 510 && matches!(reply, Reply::OfflinePages | Reply::OfflineCycle) {
                        let last_id = data["lastId"].as_u64().unwrap();
                        state.socket_requests.lock().unwrap().push((cmd, last_id));
                        let first = data["first"].as_bool().unwrap();
                        assert_eq!(last_id, if first { 1 } else { 100 });
                        let payload = json!({"error_code":0,"data":{"more":first||matches!(reply,Reply::OfflineCycle),"lastActionId":if first||matches!(reply,Reply::OfflineCycle){"100"}else{"200"},"msgs":[{"uidFrom":"22","idTo":"11","msgId":if first{"811"}else{"812"},"cliMsgId":if first{"1760000000011"}else{"1760000000012"},"ts":"1760000000001","msgType":"webchat","content":if first{"page-one"}else{"page-two"}}]}});
                        ws.send(tungstenite::Message::Binary(
                            wire::encode_frame(
                                510,
                                0,
                                &json!({"error_code":0,"encrypt":0,"data":payload}),
                            )
                            .unwrap(),
                        ))
                        .unwrap();
                    }
                    if cmd == 563 {
                        let response = json!({"error_code":0,"data":{"reqId":data["reqId"],"data":{"err":0,"hasMore":false,"uids":[],"threads":[],"waitUids":[],"pendingUids":[]}}});
                        ws.send(tungstenite::Message::Binary(
                            wire::encode_frame(563, 0, &response).unwrap(),
                        ))
                        .unwrap();
                    }
                    if [510, 511].contains(&cmd)
                        && matches!(reply, Reply::Runtime | Reply::RuntimeStall)
                    {
                        let group = cmd == 511;
                        let field = if group { "groupMsgs" } else { "msgs" };
                        let server_id = if group { "802" } else { "801" };
                        let queue = if group { "511_0" } else { "510_0" };
                        let mut payload =
                            json!({"error_code":0,"data":{"more":false,"queueStatus":{}}});
                        payload["data"][field] = json!([{"uidFrom":"22","idTo":if group{"22"}else{"11"},"msgId":server_id,"cliMsgId":if group{"1760000000002"}else{"1760000000001"},"ts":"1760000000001","msgType":"webchat","content":if group{"group incoming"}else{"personal incoming"}}]);
                        payload["data"]["queueStatus"][queue] =
                            json!({"lastId":if group{"200"}else{"100"},"ids":[],"evict":0});
                        let outer = json!({"error_code":0,"encrypt":0,"data":payload});
                        let frame = wire::encode_frame(cmd, 0, &outer).unwrap();
                        ws.send(tungstenite::Message::Binary(frame.clone()))
                            .unwrap();
                        ws.send(tungstenite::Message::Binary(frame)).unwrap();
                    }
                    if cmd == 2 {
                        ws.send(tungstenite::Message::Binary(
                            wire::encode_frame(
                                2,
                                1,
                                &json!({"error_code":0,"eventId":data["eventId"]}),
                            )
                            .unwrap(),
                        ))
                        .unwrap();
                    }
                }
                tungstenite::Message::Close(_) => break,
                _ => {}
            }
        }
        return;
    }
    let length = field("content-length")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length < wire::MAX_FRAME);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    state
        .requests
        .lock()
        .unwrap()
        .push((path.clone(), String::from_utf8(body.clone()).unwrap()));
    if matches!(reply, Reply::Stall | Reply::RuntimeStall) {
        let mut byte = [0];
        let _ = stream.read(&mut byte);
        state.closed.fetch_add(1, Ordering::SeqCst);
        return;
    }
    if matches!(reply, Reply::Redirect) {
        let _=stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://wpa.zaloapp.com/followed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = stream.flush();
        return;
    }
    if matches!(reply, Reply::Oversize) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            wire::MAX_FRAME + 1
        )
        .unwrap();
        let _ = stream.flush();
        return;
    }
    let route = path.split('?').next().unwrap();
    let cipher =
        SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
    let inner = if route == "/api/group/getgi/v2" {
        json!({"error_code":0,"data":{"groupId":"22","e2ee":false}})
    } else {
        let params = url::form_urlencoded::parse(&body)
            .find(|(name, _)| name == "params")
            .unwrap()
            .1
            .into_owned();
        let parameters: Value = serde_json::from_slice(&cipher.decrypt(&params).unwrap()).unwrap();
        assert_eq!(parameters["message"], "Xin chào Linux");
        assert_eq!(parameters["imei"], "native-fixture");
        assert!(parameters["clientId"].as_u64().unwrap() <= 9_007_199_254_740_991);
        if route == "/api/group/sendmsg" {
            assert_eq!(parameters["grid"], "22");
            assert_eq!(parameters["visibility"], 0);
        } else {
            assert_eq!(route, "/api/message/sms");
            assert_eq!(parameters["toid"], "22");
        }
        json!({"error_code":0,"data":{"msgId":if route=="/api/group/sendmsg"{"902"}else{"901"},"ts":"1760000000001"}})
    };
    let response=serde_json::to_vec(&json!({"error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()})).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .unwrap();
    stream.write_all(&response).unwrap();
    let _ = stream.flush();
}

#[tokio::test]
async fn real_tls_websocket_personal_and_group_text_use_production_codec_capability_and_ack_persistence(
) {
    let f = Fixture::new(Reply::Normal);
    let session = f.session();
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    let events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
    };
    let mut socket = f.connection().await;
    let auth = socket.next().await.unwrap().unwrap();
    socket.process(auth, &db, &events).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::Connection {
            state: LiveConnection::Online,
            ..
        })
    ));
    let direct = Target::new(ThreadKind::Direct, "22").unwrap();
    socket.verify_direct(&direct, &db, &events).await.unwrap();
    let http = f.client();
    for (kind, server_id) in [(ThreadKind::Direct, "901"), (ThreadKind::Group, "902")] {
        let target = Target::new(kind, "22").unwrap();
        if kind == ThreadKind::Group {
            verify_group(&http, &session, &target).await.unwrap();
        }
        let thread = target.thread_id();
        let stored_thread = thread.clone();
        let id = db
            .call(move |s| {
                s.prepare_outgoing("11", &stored_thread, "Xin chào Linux", 1760000000000)
            })
            .await
            .unwrap();
        let ack = send_text(&http, &session, &target, "Xin chào Linux", id, Some(0))
            .await
            .unwrap();
        assert_eq!(ack.server_id, server_id);
        let (message, _) = persist_ack(&db, "11", id, ack).await.unwrap();
        assert_eq!(message.thread_id, thread);
        assert_eq!(message.id, server_id);
    }
    socket.socket.close(None).await.unwrap();
    drop(socket);
    let paths: Vec<_> = f
        .state
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|(p, _)| p.split('?').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        paths,
        [
            "/api/message/sms",
            "/api/group/getgi/v2",
            "/api/group/sendmsg"
        ]
    );
    assert!(f
        .state
        .sni
        .lock()
        .unwrap()
        .iter()
        .all(|s| s == "wpa.zaloapp.com"));
}
#[tokio::test]
async fn real_https_refuses_redirect_and_oversize_without_logging_tokens() {
    for reply in [Reply::Redirect, Reply::Oversize] {
        let f = Fixture::new(reply);
        let session = f.session();
        let result = send_text(
            &f.client(),
            &session,
            &Target::new(ThreadKind::Direct, "22").unwrap(),
            "Xin chào Linux",
            1760000000000,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(!format!("{:?}", result.unwrap_err()).contains("synthetic"));
        assert_eq!(f.state.requests.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn stalled_real_tls_request_and_websocket_read_are_dropped_on_cancellation() {
    let f = Fixture::new(Reply::Stall);
    let session = f.session();
    let client = f.client();
    let target = Target::new(ThreadKind::Direct, "22").unwrap();
    {
        let request = send_text(
            &client,
            &session,
            &target,
            "Xin chào Linux",
            1760000000000,
            None,
        );
        tokio::pin!(request);
        tokio::select! {result=&mut request=>panic!("unexpected completion: {result:?}"),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
    }
    let mut connection = f.connection().await;
    connection.next().await.unwrap();
    {
        let read = connection.next();
        tokio::pin!(read);
        tokio::select! {result=&mut read=>panic!("unexpected socket completion: {result:?}"),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
    }
    drop(connection);
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.state.closed.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn worker_cancel_interrupts_a_saturated_critical_event_queue_without_contacting_network() {
    let f = Fixture::new(Reply::Normal);
    let session = f.session();
    let (_commands, rx) = mpsc::channel(1);
    let (tx, events) = mpsc::channel(1);
    tx.send(LiveEvent::Connection {
        account_id: "11".into(),
        state: LiveConnection::Offline,
    })
    .await
    .unwrap();
    let (stop, cancel) = watch::channel(false);
    let profile = f.profile.clone();
    let task = tokio::spawn(run(profile, session, rx, tx, Arc::new(|| {}), cancel));
    tokio::time::sleep(Duration::from_millis(20)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    drop(events);
    assert!(f.state.requests.lock().unwrap().is_empty());
    assert!(f.state.sni.lock().unwrap().is_empty());
}

#[tokio::test]
async fn actual_runtime_consumes_wss_batches_and_commands_then_publishes_only_durable_received_and_sent(
) {
    let fixture = Fixture::new(Reply::Runtime);
    let session = fixture.session();
    let profile = fixture.profile.clone();
    let transports = fixture.transports();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        profile,
        session,
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    let mut received = std::collections::HashSet::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while received.len() < 2 {
            match events.recv().await.unwrap() {
                LiveEvent::Received { message, .. } => {
                    received.insert(message.id.clone());
                    // A durability observer must not initialize schema or start a writer.
                    // The production worker remains the sole SQLite writer during this test.
                    let s = rusqlite::Connection::open_with_flags(
                        fixture.profile.paths.data_dir.join("messages.sqlite"),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    ).unwrap();
                    let durable:(String,String,i64)=s.query_row(
                        "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                        rusqlite::params![message.account_id,message.thread_id,message.id],
                        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
                    ).unwrap();
                    assert_eq!(durable,(message.sender,message.text,message.sent_at_ms));
                }
                LiveEvent::Fault { error, .. } => panic!("runtime fault: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(received.len(), 2);
    for (kind, thread, request_id, server_id) in [
        (ThreadKind::Direct, "d:22", 1, "901"),
        (ThreadKind::Group, "g:22", 2, "902"),
    ] {
        commands
            .send(LiveCommand::Send {
                generation: 1,
                thread: thread.into(),
                text: "Xin chào Linux".into(),
                request_id,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Sent {
                        message,
                        request_id: ack,
                        ..
                    } => {
                        assert_eq!(request_id, ack);
                        assert_eq!(message.id, server_id);
                        assert_eq!(
                            message.thread_id,
                            Target::new(kind, "22").unwrap().thread_id()
                        );
                        break;
                    }
                    LiveEvent::SendFailed { error, .. } | LiveEvent::Fault { error, .. } => {
                        panic!("runtime send fault: {error}")
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
    }
    let s = rusqlite::Connection::open_with_flags(
        fixture.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    for thread in ["d:22", "g:22"] {
        let count: i64 = s
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE account_id=?1 AND thread_id=?2",
                rusqlite::params!["11", thread],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
    for (queue, expected) in [("510_0", "100"), ("511_0", "200")] {
        let cursor: String = s
            .query_row(
                "SELECT last_id FROM receive_cursors WHERE account_id=?1 AND queue=?2",
                rusqlite::params!["11", queue],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursor, expected);
    }
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn actual_runtime_cancels_stalled_send_without_retry_and_preserves_durable_outbox() {
    let f = Fixture::new(Reply::RuntimeStall);
    let transports = f.transports();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    let mut received = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        while received < 2 {
            if matches!(events.recv().await, Some(LiveEvent::Received { .. })) {
                received += 1;
            }
        }
    })
    .await
    .unwrap();
    commands
        .send(LiveCommand::Send {
            generation: 1,
            thread: "d:22".into(),
            text: "Xin chào Linux".into(),
            request_id: 1,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while f.state.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.state.closed.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let mut s = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    s.recover_outgoing("11").unwrap();
    assert!(s
        .prepare_outgoing("11", "d:22", "Xin chào Linux", now_ms())
        .is_err());
    assert_eq!(f.state.requests.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn actual_runtime_cancels_when_received_event_backpressure_blocks_the_live_socket() {
    let f = Fixture::new(Reply::Runtime);
    let transports = f.transports();
    let (_commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(1);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                events.recv().await,
                Some(LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(events.len(), 1);
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn actual_runtime_offline_pages_use_last_action_id_without_queue_status_after_durable_insert()
{
    let f = Fixture::new(Reply::OfflinePages);
    let transports = f.transports();
    let (_commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while received.len() < 2 {
            match events.recv().await.unwrap() {
                LiveEvent::Received { message, .. } => received.push(message.id),
                LiveEvent::Fault { error, .. } => panic!("offline page fault: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(510, 1), (510, 100)]
    );
    let s = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "200");
    assert_eq!(
        s.page("11", "d:22", None, 10)
            .unwrap()
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["811", "812"]
    );
}
#[tokio::test]
async fn actual_runtime_offline_more_with_repeated_cursor_stops_instead_of_repeating_pull() {
    let f = Fixture::new(Reply::OfflineCycle);
    let transports = f.transports();
    let (_commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (_stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(events.recv().await, Some(LiveEvent::Fault { .. })) {
                break;
            }
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(510, 1), (510, 100)]
    );
    let s = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "100");
    assert_eq!(s.page("11", "d:22", None, 10).unwrap().len(), 1);
}
#[tokio::test]
async fn actual_runtime_does_not_advance_cursor_or_request_next_page_if_message_commit_fails() {
    let f = Fixture::new(Reply::OfflinePages);
    let path = f.profile.paths.data_dir.join("messages.sqlite");
    drop(MessageStore::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_page BEFORE INSERT ON messages WHEN NEW.body='page-one' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    drop(conn);
    let transports = f.transports();
    let (_commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (_stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(events.recv().await, Some(LiveEvent::Fault { .. })) {
                break;
            }
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(510, 1)]
    );
    let s = MessageStore::open(&path).unwrap();
    assert_eq!(s.receive_cursor("11", "510_0").unwrap(), "1");
    assert!(s.page("11", "d:22", None, 10).unwrap().is_empty());
}

#[tokio::test]
async fn offline_page_budget_rejects_excess_pages_before_persistence_or_next_request() {
    let f = Fixture::new(Reply::Normal);
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let (tx, _events) = mpsc::channel(4);
    let events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
    };
    let mut connection = f.connection().await;
    connection.next().await.unwrap();
    connection.queues.insert((510, 0));
    connection.offline_progress.insert(
        (510, 0),
        OfflineProgress {
            last_id: 1,
            pages: MAX_OFFLINE_PAGES,
        },
    );
    let result=connection.process(DecodedFrame{cmd:510,sub_cmd:0,payload:json!({"error_code":0,"data":{"more":true,"lastActionId":"100","msgs":[]}})},&db,&events).await;
    assert_eq!(
        result.unwrap_err().downcast_ref::<WireError>(),
        Some(&WireError::Oversize)
    );
    assert_eq!(
        db.call(|s| s.receive_cursor("11", "510_0")).await.unwrap(),
        "1"
    );
    assert!(f.state.socket_requests.lock().unwrap().is_empty());
    drop(connection);
}
