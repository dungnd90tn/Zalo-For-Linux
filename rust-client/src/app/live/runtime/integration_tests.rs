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
    ControlHeartbeats,
    OfflinePages,
    OfflineCycle,
    UpgradeForbidden,
    DesktopHeaders,
    UpgradeStall,
    AuthExpired,
    AuthMalformed,
    AuthFixture(usize, bool),
    Directory,
    DirectoryFail,
    DirectoryStall,
}
#[derive(Default)]
struct State {
    requests: Mutex<Vec<(String, String)>>,
    sni: Mutex<Vec<String>>,
    closed: AtomicUsize,
    socket_requests: Mutex<Vec<(u16, u64)>>,
    upgrades: AtomicUsize,
    directory_stalled: AtomicBool,
    observed_commands: Mutex<Vec<u16>>,
    pings: AtomicUsize,
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
    fn directory_session(&self, websocket: bool) -> Arc<VerifiedSession> {
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"profile":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"]},"zpw_ws":if websocket{json!(["wss://wpa.zaloapp.com"])}else{json!([])}})).unwrap();
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
        let url = Url::parse("wss://wpa.zaloapp.com?zpw_ver=681&zpw_type=24").unwrap();
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
        let capture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/messaging/electron_headers.json"
        ))
        .unwrap();
        let desktop_headers = field("origin").as_deref() == capture["origin"].as_str()
            && field("user-agent").as_deref() == capture["userAgent"].as_str()
            && path.contains("zpw_type=24")
            && field("sec-websocket-version").as_deref() == capture["version"].as_str();
        if matches!(reply, Reply::DesktopHeaders) && !desktop_headers {
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            stream.flush().unwrap();
            return;
        }
        state.upgrades.fetch_add(1, Ordering::SeqCst);
        if matches!(
            reply,
            Reply::Runtime | Reply::RuntimeStall | Reply::DirectoryStall | Reply::ControlHeartbeats
        ) {
            assert!(
                desktop_headers,
                "handshake must match independently captured Electron headers"
            );
            assert!(path.starts_with("/?zpw_ver=681&zpw_type=24&t="));
        }
        if matches!(reply, Reply::UpgradeForbidden) {
            stream.write_all(b"HTTP/1.1 403 Forbidden\r\nLocation: https://private.invalid/?cookie=synthetic-secret\r\nContent-Length: 16\r\nConnection: close\r\n\r\nsynthetic-secret").unwrap();
            let _ = stream.flush();
            return;
        }
        if matches!(reply, Reply::UpgradeStall) {
            let mut byte = [0];
            let _ = stream.read(&mut byte);
            state.closed.fetch_add(1, Ordering::SeqCst);
            return;
        }
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
        if matches!(reply, Reply::AuthExpired | Reply::AuthMalformed) {
            let payload = if matches!(reply, Reply::AuthExpired) {
                json!({"error_code":102})
            } else {
                json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":2,"data":"invalid-ciphertext"})
            };
            let _ = ws.send(tungstenite::Message::Binary(
                wire::encode_frame(1, 0, &payload).unwrap(),
            ));
            let _ = ws.read();
            return;
        }
        let auth = json!({"error_code":0,"data":{"qCmds":if matches!(reply,Reply::Runtime|Reply::RuntimeStall|Reply::DirectoryStall|Reply::ControlHeartbeats){json!([{"cmd":510,"subCmd":0},{"cmd":511,"subCmd":0}])}else{json!([{"cmd":510,"subCmd":0}])}}});
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&serde_json::to_vec(&auth).unwrap()).unwrap();
        let auth_frame = if matches!(reply, Reply::ControlHeartbeats) {
            let fixture: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/messaging/auth_frames.json"
            ))
            .unwrap();
            STANDARD
                .decode(fixture["cases"][3]["frame"].as_str().unwrap())
                .unwrap()
        } else if let Reply::AuthFixture(index, negative) = reply {
            let fixture: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/messaging/auth_frames.json"
            ))
            .unwrap();
            STANDARD
                .decode(
                    fixture[if negative { "queueFailures" } else { "cases" }][index]["frame"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap()
        } else {
            wire::encode_frame(1,0,&json!({"error_code":0,"key":STANDARD.encode([7;32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())})).unwrap()
        };
        ws.send(tungstenite::Message::Binary(auth_frame)).unwrap();
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
                    state.observed_commands.lock().unwrap().push(cmd);
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
                            wire::encode_frame(563, 0, &json!({"error_code":0,"encrypt":0,"data":serde_json::to_string(&response).unwrap()})).unwrap(),
                        ))
                        .unwrap();
                    }
                    if [510, 511].contains(&cmd)
                        && matches!(
                            reply,
                            Reply::Runtime
                                | Reply::RuntimeStall
                                | Reply::DirectoryStall
                                | Reply::ControlHeartbeats
                        )
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
                        let ping = state.pings.fetch_add(1, Ordering::SeqCst) + 1;
                        let mut pong = json!({"eventId":data["eventId"]});
                        if !matches!(reply, Reply::ControlHeartbeats) {
                            pong["error_code"] = json!(0)
                        } else if ping == 2 {
                            pong["error_code"] = Value::Null
                        } else if ping == 3 {
                            pong["error_code"] = json!(0)
                        }
                        ws.send(tungstenite::Message::Binary(
                            wire::encode_frame(2, 1, &pong).unwrap(),
                        ))
                        .unwrap();
                        if matches!(reply, Reply::ControlHeartbeats) {
                            ws.send(tungstenite::Message::Binary(
                                wire::encode_frame(4, 0, &json!({"data":{"at":1700000000000_i64}}))
                                    .unwrap(),
                            ))
                            .unwrap();
                            if ping <= 3 {
                                let fixture: Value = serde_json::from_str(include_str!(
                                    "../../../../tests/fixtures/messaging/post_auth_frames.json"
                                ))
                                .unwrap();
                                let name = format!("after_heartbeat_{ping}");
                                let case = fixture["cases"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .find(|case| case["name"] == name)
                                    .unwrap();
                                ws.send(tungstenite::Message::Binary(
                                    STANDARD.decode(case["frame"].as_str().unwrap()).unwrap(),
                                ))
                                .unwrap();
                            } else {
                                ws.send(tungstenite::Message::Binary(wire::encode_frame(501,0,&json!({"encrypt":0,"data":"private-synthetic-message https://secret.invalid/?key=private-synthetic-token"})).unwrap())).unwrap();
                            }
                        }
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
    let route = path.split('?').next().unwrap();
    if [
        "/api/group/getlg/v3",
        "/api/social/friend/getfriends",
        "/api/preloadconvers/get-last-msgs",
    ]
    .contains(&route)
    {
        let cipher =
            SessionCipher::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
        let encrypted = url::form_urlencoded::parse(path.split_once('?').unwrap().1.as_bytes())
            .find(|(name, _)| name == "params")
            .unwrap()
            .1
            .into_owned();
        let parameters: Value =
            serde_json::from_slice(&cipher.decrypt(&encrypted).unwrap()).unwrap();
        assert_eq!(parameters["imei"], "native-fixture");
        if route == "/api/group/getlg/v3" && matches!(reply, Reply::DirectoryStall) {
            state.directory_stalled.store(true, Ordering::SeqCst);
            let mut byte = [0];
            let _ = stream.read(&mut byte);
            state.closed.fetch_add(1, Ordering::SeqCst);
            return;
        }
        if route == "/api/group/getlg/v3" && matches!(reply, Reply::DirectoryFail) {
            let _ = stream.write_all(
                b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = stream.flush();
            return;
        }
        let rich = matches!(
            reply,
            Reply::Directory | Reply::DirectoryFail | Reply::DirectoryStall
        );
        let data = match route {
            "/api/social/friend/getfriends" => {
                assert_eq!(parameters["count"], 20_000);
                let mut entries =
                    vec![json!({"userId":"22","displayName":"Nguyễn An","isFr":1,"isBlocked":0})];
                entries.extend((1000..1204).map(|id|json!({"userId":id.to_string(),"displayName":format!("Bạn {id}"),"isFr":1})));
                json!(entries)
            }
            "/api/group/getlg/v3" => {
                assert_eq!(parameters["page"], 1);
                json!({"groups":if rich{json!([{"groupId":"22","name":"Nhóm Linux","totalMember":3}])}else{json!([])},"hasMore":0})
            }
            _ => {
                assert_eq!(parameters["threadIdLocalMsgId"], "{}");
                json!({"msgs":[{"uidFrom":"22","idTo":"11","msgId":"910","ts":"1760000000020","msgType":"webchat","content":"recent preview"}]})
            }
        };
        let inner = json!({"error_code":0,"data":data});
        let response=serde_json::to_vec(&json!({"error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()})).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        let _ = stream.write_all(&response);
        let _ = stream.flush();
        return;
    }
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
        while !f
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.starts_with("/api/message/sms?"))
        {
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
    assert_eq!(
        f.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with("/api/message/sms?"))
            .count(),
        1
    );
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
    let (commands, rx) = mpsc::channel(16);
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
            if matches!(
                events.recv().await,
                Some(LiveEvent::Connection {
                    state: LiveConnection::Offline,
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished());
    commands.send(LiveCommand::Shutdown).await.unwrap();
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

#[tokio::test]
async fn actual_runtime_reports_safe_upgrade_status_and_reconnects_without_blocking_local_history()
{
    let f = Fixture::new(Reply::UpgradeForbidden);
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
        f.transports(),
    ));
    let observed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Diagnostic {
                    diagnostic,
                    account_id,
                } => {
                    assert_eq!(account_id, "11");
                    break diagnostic;
                }
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => panic!("rejected upgrade cannot be online"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(observed.stage, LiveStage::Connect);
    assert_eq!(observed.category, LiveCategory::Http);
    assert_eq!(observed.http_status, Some(403));
    let rendered = format!("{observed} {observed:?}");
    assert!(!rendered.contains("synthetic-secret"));
    assert!(!rendered.contains("private.invalid"));
    commands
        .send(LiveCommand::Open {
            generation: 7,
            kind: ThreadKind::Direct,
            remote_id: "22".into(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if let LiveEvent::Opened {
                generation,
                conversation,
                ..
            } = events.recv().await.unwrap()
            {
                assert_eq!(generation, 7);
                assert_eq!(conversation.id, "d:22");
                break;
            }
        }
    })
    .await
    .unwrap();
    commands.send(LiveCommand::Reconnect).await.unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        while f.state.upgrades.load(Ordering::SeqCst) < 2 {
            let _ = events.recv().await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn actual_runtime_serves_local_commands_and_cancel_while_upgrade_is_stalled() {
    let f = Fixture::new(Reply::UpgradeStall);
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
        f.transports(),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.state.upgrades.load(Ordering::SeqCst) == 0 {
            tokio::select! { _=events.recv()=>{},_=tokio::time::sleep(Duration::from_millis(5))=>{} }
        }
    }).await.unwrap();
    commands
        .send(LiveCommand::Open {
            generation: 9,
            kind: ThreadKind::Group,
            remote_id: "22".into(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if let LiveEvent::Opened {
                generation,
                conversation,
                ..
            } = events.recv().await.unwrap()
            {
                assert_eq!(generation, 9);
                assert_eq!(conversation.id, "g:22");
                break;
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
}

#[test]
fn diagnostic_drops_http_headers_body_and_close_reasons() {
    let response = tungstenite::http::Response::builder()
        .status(302)
        .header(
            "Location",
            "https://private.invalid/?secret=synthetic-secret",
        )
        .body(Some(b"synthetic-secret".to_vec()))
        .unwrap();
    let failure = NetworkFailure::socket(LiveStage::Connect, tungstenite::Error::Http(response));
    assert_eq!(failure.0.http_status, Some(302));
    assert_eq!(failure.0.category, LiveCategory::Http);
    for rendered in [format!("{failure}"), format!("{failure:?}")] {
        assert!(!rendered.contains("synthetic-secret"));
        assert!(!rendered.contains("private.invalid"));
    }
    let failure = diagnostic(&WireError::Service(9876).into(), LiveStage::Authentication);
    assert_eq!(failure.server_code, Some(9876));
    assert_eq!(failure.category, LiveCategory::Server);
}

#[test]
fn desktop_handshake_preserves_advertised_path_cookie_and_negotiated_api_type() {
    let f = Fixture::new(Reply::Normal);
    let config =
        ServerConfig::parse(&json!({"zpw_ws":["wss://wpa.zaloapp.com/socket/v1%2Fmessages"]}))
            .unwrap();
    let session = synthetic_session(f.profile.clone(), config);
    let request = Connection::handshake_request(&session, 0).unwrap();
    assert_eq!(request.uri().path(), "/socket/v1%2Fmessages");
    let pairs: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(pairs.get("zpw_type").unwrap(), "24");
    assert_eq!(pairs.get("zpw_ver").unwrap(), "681");
    assert!(pairs.get("t").unwrap().parse::<i64>().unwrap() > 0);
    let capture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/messaging/electron_headers.json"
    ))
    .unwrap();
    assert_eq!(
        request.headers()["Origin"],
        capture["origin"].as_str().unwrap()
    );
    assert_eq!(
        request.headers()["User-Agent"],
        capture["userAgent"].as_str().unwrap()
    );
    // Captured Electron negotiates compression; native must not advertise it
    // until a bounded permessage-deflate implementation actually exists.
    assert!(request.headers().get("Sec-WebSocket-Extensions").is_none());
    assert_eq!(request.headers()["Cookie"], "zpw_sek=synthetic");
}

#[tokio::test]
async fn terminal_transport_failures_keep_one_local_actor_alive_until_explicit_shutdown() {
    for (reply, expected_state, expected_category) in [
        (None, LiveConnection::Offline, LiveCategory::Unsupported),
        (
            Some(Reply::AuthExpired),
            LiveConnection::Expired,
            LiveCategory::Authentication,
        ),
        (
            Some(Reply::AuthMalformed),
            LiveConnection::Offline,
            LiveCategory::Crypto,
        ),
        (
            Some(Reply::UpgradeForbidden),
            LiveConnection::Offline,
            LiveCategory::Http,
        ),
    ] {
        let f = Fixture::new(reply.unwrap_or(Reply::Normal));
        let cached = crate::domain::Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "700".into(),
            sender: "22".into(),
            text: "cached before the failed connection".into(),
            sent_at_ms: 1,
        };
        {
            let mut store =
                MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
            store.insert_batch(std::slice::from_ref(&cached)).unwrap();
        }
        let session = if reply.is_some() {
            f.session()
        } else {
            Arc::new(synthetic_session(
                f.profile.clone(),
                ServerConfig::parse(&json!({})).unwrap(),
            ))
        };
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(4);
        let (_stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            session,
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        for attempt in 0..2 {
            let observed = tokio::time::timeout(Duration::from_secs(2), async {
                let mut reason = None;
                let mut directory_done = attempt > 0;
                let mut terminal = false;
                loop {
                    match events.recv().await.expect("local worker must stay alive") {
                        LiveEvent::Diagnostic { diagnostic, .. } => reason = Some(diagnostic),
                        LiveEvent::Connection { state, .. } if state == expected_state => {
                            terminal = true
                        }
                        LiveEvent::DirectoryStatus { syncing: false, .. } => directory_done = true,
                        LiveEvent::Connection {
                            state: LiveConnection::Online,
                            ..
                        } => panic!("failed authentication cannot be online"),
                        LiveEvent::Fault { error, .. } => {
                            panic!("transport failure must not destroy local storage: {error}")
                        }
                        _ => {}
                    }
                    if terminal && directory_done {
                        break reason.expect("terminal state must have a diagnostic");
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(observed.category, expected_category);
            assert!(!task.is_finished());
            // Advancing beyond all reconnect backoffs must not retry poison data.
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(60)).await;
            tokio::task::yield_now().await;
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            tokio::time::resume();
            commands
                .send(LiveCommand::Open {
                    generation: 7,
                    kind: ThreadKind::Direct,
                    remote_id: "22".into(),
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_millis(500), async {
                match events.recv().await.unwrap() {
                    LiveEvent::Opened {
                        generation,
                        conversation,
                        ..
                    } => {
                        assert_eq!(generation, 7);
                        assert_eq!(conversation.id, "d:22");
                    }
                    event => panic!("local Open failed: {event:?}"),
                }
            })
            .await
            .unwrap();
            commands
                .send(LiveCommand::Load {
                    generation: 7,
                    thread: "d:22".into(),
                    before: None,
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_millis(500), async {
                match events.recv().await.unwrap() {
                    LiveEvent::Page {
                        generation,
                        messages,
                        has_older,
                        ..
                    } => {
                        assert_eq!(generation, 7);
                        assert_eq!(messages.as_slice(), std::slice::from_ref(&cached));
                        assert!(!has_older);
                    }
                    event => panic!("cached page failed: {event:?}"),
                }
            })
            .await
            .unwrap();
            commands
                .send(LiveCommand::Send {
                    generation: 7,
                    thread: "d:22".into(),
                    text: "keep this offline draft".into(),
                    request_id: 77,
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_millis(500), async {
                assert!(matches!(
                    events.recv().await.unwrap(),
                    LiveEvent::SendFailed {
                        request_id: 77,
                        unknown_delivery: false,
                        server_accepted: false,
                        ..
                    }
                ));
            })
            .await
            .unwrap();
            if attempt == 0 {
                commands.send(LiveCommand::Reconnect).await.unwrap();
            }
        }
        assert_eq!(
            f.state.upgrades.load(Ordering::SeqCst),
            if reply.is_some() { 2 } else { 0 }
        );
        assert!(f
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _)| path.starts_with("/api/group/getlg/v3?")));
        commands.send(LiveCommand::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn directory_page_event(
    events: &mut mpsc::Receiver<LiveEvent>,
    generation: u64,
) -> (Vec<DirectoryRecord>, Option<DirectoryCursor>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::DirectoryPage {
                    generation: current,
                    account_id,
                    entries,
                    next,
                    ..
                } if current == generation => {
                    assert_eq!(account_id, "11");
                    break (entries, next);
                }
                LiveEvent::DirectoryFailed { error, .. } => {
                    panic!("cached directory failed: {error}")
                }
                LiveEvent::Fault { error, .. } => panic!("local worker failed: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn actual_runtime_discovers_named_cached_directory_and_previews_without_websocket() {
    for reply in [Reply::Directory, Reply::DirectoryFail] {
        let f = Fixture::new(reply);
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(4);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.directory_session(false),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        let report=tokio::time::timeout(Duration::from_secs(3),async{loop{match events.recv().await.unwrap(){
            LiveEvent::DirectoryStatus{generation:0,syncing:false,report,..}=>break report,
            LiveEvent::DirectoryChanged{account_id,conversations}=>{
                assert_eq!(account_id,"11");
                let observer=rusqlite::Connection::open_with_flags(f.profile.paths.data_dir.join("messages.sqlite"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
                let count:i64=observer.query_row("SELECT count(*) FROM directory_entries WHERE account_id='11'",[],|row|row.get(0)).unwrap();
                assert!(count>=200,"publication must follow incremental durable commit");
                for conversation in conversations{let durable:String=observer.query_row("SELECT title FROM conversations WHERE account_id='11' AND thread_id=?1",[&conversation.id],|row|row.get(0)).unwrap();assert_eq!(durable,conversation.title);}
            },
            LiveEvent::Connection{state:LiveConnection::Online,..}=>panic!("missing WebSocket cannot be online"),
            LiveEvent::Fault{error,..}=>panic!("discovery must preserve local worker: {error}"),_=>{},
        }}}).await.unwrap();
        assert_eq!(report.contacts, 205);
        assert_eq!(report.preview_messages, 1);
        assert!(report.completed_friends && report.completed_previews);
        if matches!(reply, Reply::DirectoryFail) {
            assert!(!report.completed_groups);
            assert_eq!(report.failures.len(), 1);
            assert_eq!(report.failures[0].http_status, Some(503));
        } else {
            assert_eq!(report.groups, 1);
            assert!(report.completed_groups);
            assert!(report.failures.is_empty());
        }
        commands
            .send(LiveCommand::Directory {
                generation: 42,
                is_group: false,
                query: "nguyen".into(),
                after: None,
            })
            .await
            .unwrap();
        let (entries, next) = directory_page_event(&mut events, 42).await;
        assert!(next.is_none());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].remote_id, "22");
        assert_eq!(entries[0].display_name, "Nguyễn An");
        let mut after = None;
        let mut all = std::collections::HashSet::new();
        for (generation, expected) in [(43, 100), (44, 100), (45, 5)] {
            commands
                .send(LiveCommand::Directory {
                    generation,
                    is_group: false,
                    query: String::new(),
                    after,
                })
                .await
                .unwrap();
            let (entries, next) = directory_page_event(&mut events, generation).await;
            assert_eq!(entries.len(), expected);
            for entry in entries {
                assert!(all.insert(entry.thread_id()));
            }
            after = next;
        }
        assert_eq!(all.len(), 205);
        assert!(after.is_none());
        commands
            .send(LiveCommand::Open {
                generation: 7,
                kind: ThreadKind::Direct,
                remote_id: "22".into(),
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let LiveEvent::Opened { conversation, .. } = events.recv().await.unwrap() {
                    assert_eq!(conversation.title, "Nguyễn An");
                    assert_eq!(conversation.last_text, "recent preview");
                    break;
                }
            }
        })
        .await
        .unwrap();
        commands
            .send(LiveCommand::Load {
                generation: 7,
                thread: "d:22".into(),
                before: None,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let LiveEvent::Page { messages, .. } = events.recv().await.unwrap() {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].text, "recent preview");
                    break;
                }
            }
        })
        .await
        .unwrap();
        commands
            .send(LiveCommand::RefreshDirectory { generation: 99 })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let LiveEvent::DirectoryStatus {
                    generation: 99,
                    syncing: false,
                    report,
                    ..
                } = events.recv().await.unwrap()
                {
                    assert_eq!(report.contacts, 205);
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(f.state.requests.lock().unwrap().len(), 6);
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn actual_runtime_receives_and_serves_partial_directory_during_stalled_sync_then_cancels() {
    let f = Fixture::new(Reply::DirectoryStall);
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(4);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.directory_session(true),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    let mut received = std::collections::HashSet::new();
    tokio::time::timeout(Duration::from_secs(2),async{loop{
        if received.len()==2&&f.state.directory_stalled.load(Ordering::SeqCst){break;}
        tokio::select!{event=events.recv()=>match event.unwrap(){LiveEvent::Received{message,..}=>{received.insert(message.id);},LiveEvent::Fault{error,..}=>panic!("stalled directory blocked worker: {error}"),_=>{}},_=tokio::time::sleep(Duration::from_millis(5))=>{}}
    }}).await.unwrap();
    commands
        .send(LiveCommand::Directory {
            generation: 5,
            is_group: false,
            query: "nguyen".into(),
            after: None,
        })
        .await
        .unwrap();
    let (entries, _) = directory_page_event(&mut events, 5).await;
    assert_eq!(entries[0].display_name, "Nguyễn An");
    for generation in [7, 8] {
        commands
            .send(LiveCommand::RefreshDirectory { generation })
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                events.recv().await,
                Some(LiveEvent::DirectoryStatus {
                    generation: 8,
                    syncing: true,
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with("/api/social/friend/getfriends?"))
            .count(),
        1,
        "refreshes must not spawn concurrent directory fetchers"
    );
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
}

#[tokio::test]
async fn delayed_directory_notification_reads_after_received_commit_instead_of_replaying_a_stale_snapshot(
) {
    let f = Fixture::new(Reply::Normal);
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let mut directory = DirectorySync::new();
    let mut sink = RuntimeDirectorySink {
        account: "11".into(),
        db: db.handle().unwrap(),
        updates: directory.updates_tx.clone(),
    };
    sink.apply(DirectoryBatch {
        account_id: "11".into(),
        entries: vec![DirectoryRecord {
            remote_id: "22".into(),
            is_group: false,
            display_name: "Nguyễn An".into(),
            avatar_url: None,
            is_friend: Some(true),
            is_blocked: None,
            member_count: None,
        }],
        messages: Vec::new(),
        observed_at_ms: 1,
    })
    .await
    .unwrap();
    directory.changed();
    let message = crate::domain::Message {
        account_id: "11".into(),
        thread_id: "d:33".into(),
        id: "999".into(),
        sender: "33".into(),
        text: "arrived after the directory commit".into(),
        sent_at_ms: 100,
    };
    let stored = message.clone();
    let conversation = db
        .call(move |store| store.insert_with_conversation(&stored))
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
    };
    events
        .emit(LiveEvent::Received {
            account_id: "11".into(),
            message,
            conversation,
        })
        .await
        .unwrap();
    assert!(matches!(rx.recv().await, Some(LiveEvent::Received { .. })));
    let permit = events.tx.reserve().await.unwrap();
    directory
        .publish_notification(permit, &db, &events)
        .await
        .unwrap();
    match rx.recv().await.unwrap() {
        LiveEvent::DirectoryChanged { conversations, .. } => {
            assert_eq!(conversations.len(), 1);
            assert_eq!(conversations[0].id, "d:33");
            assert_eq!(
                conversations[0].last_text,
                "arrived after the directory commit"
            );
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[tokio::test]
async fn directory_refresh_keeps_only_the_newest_pending_generation_and_never_finishes_it_with_an_old_report(
) {
    let f = Fixture::new(Reply::Directory);
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let session = f.directory_session(false);
    let transports = f.transports();
    let mut directory = DirectorySync::new();
    directory.request(0, session.clone(), db.handle().unwrap(), &transports);
    directory.request(7, session.clone(), db.handle().unwrap(), &transports);
    directory.request(8, session.clone(), db.handle().unwrap(), &transports);
    assert_eq!(directory.pending, Some(8));
    assert_eq!(directory.running.as_ref().unwrap().generation, 0);
    let completed = tokio::time::timeout(
        Duration::from_secs(2),
        &mut directory.running.as_mut().unwrap().task,
    )
    .await
    .unwrap();
    directory.completed(
        completed,
        session.clone(),
        db.handle().unwrap(),
        &transports,
    );
    assert_eq!(directory.running.as_ref().unwrap().generation, 8);
    assert_eq!(directory.pending, None);
    assert!(
        matches!(directory.status, Some((8, true, _))),
        "old completion must not clear the newest request's pending state"
    );
    let completed = tokio::time::timeout(
        Duration::from_secs(2),
        &mut directory.running.as_mut().unwrap().task,
    )
    .await
    .unwrap();
    directory.completed(completed, session, db.handle().unwrap(), &transports);
    assert!(directory.running.is_none());
    match directory.status.as_ref().unwrap() {
        (8, false, report) => assert_eq!(report.contacts, 205),
        other => panic!("wrong completion generation: {other:?}"),
    }
    assert_eq!(
        f.state.requests.lock().unwrap().len(),
        6,
        "one auto sync plus the newest coalesced refresh"
    );
}

#[tokio::test]
async fn captured_desktop_handshake_passes_real_tls_upgrade_where_null_origin_and_type30_fail() {
    let f = Fixture::new(Reply::DesktopHeaders);
    for invalid in [Some("origin"), Some("type"), None] {
        let session = f.session();
        let mut request = Connection::handshake_request(&session, 0).unwrap();
        if invalid == Some("origin") {
            request
                .headers_mut()
                .insert("Origin", "null".parse().unwrap());
        } else if invalid == Some("type") {
            let uri = request
                .uri()
                .to_string()
                .replace("zpw_type=24", "zpw_type=30");
            *request.uri_mut() = uri.parse().unwrap();
        }
        let (_, tls) = f.transports().local.unwrap();
        let tcp = tokio::net::TcpStream::connect(f.addr).await.unwrap();
        let result = tokio_tungstenite::client_async_tls_with_config(
            request,
            tcp,
            Some(Connection::socket_config()),
            Some(tokio_tungstenite::Connector::Rustls(tls)),
        )
        .await;
        if invalid.is_some() {
            let error = result.unwrap_err();
            let safe = NetworkFailure::socket(LiveStage::Connect, error);
            assert_eq!(safe.0.http_status, Some(403));
            assert_eq!(safe.0.category, LiveCategory::Http);
        } else {
            let (mut socket, response) = result.unwrap();
            assert_eq!(response.status(), 101);
            let _ = socket.next().await.unwrap().unwrap();
            drop(socket);
        }
    }
    assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn actual_runtime_authenticates_original_header_and_gzip_fixtures_but_never_invalid_queue_schema(
) {
    for (index, negative) in (0..5).map(|i| (i, false)).chain((0..3).map(|i| (i, true))) {
        let f = Fixture::new(Reply::AuthFixture(index, negative));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(16);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.session(),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } => {
                        assert!(!negative, "invalid queue data cannot be Online");
                        break;
                    }
                    LiveEvent::Diagnostic { diagnostic, .. }
                        if diagnostic.parser_reason.is_some() =>
                    {
                        assert!(negative, "valid original AUTH failed: {diagnostic}");
                        assert_eq!(diagnostic.stage, LiveStage::Authentication);
                        assert_eq!(
                            diagnostic.parser_reason,
                            Some(wire::FrameReason::QueueCommands)
                        );
                        break;
                    }
                    LiveEvent::Fault { .. } => {
                        panic!("AUTH fixture cannot destroy cached command actor")
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        commands
            .send(LiveCommand::Open {
                generation: 42,
                kind: ThreadKind::Direct,
                remote_id: "22".into(),
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Opened {
                        generation: 42,
                        conversation,
                        ..
                    } => {
                        assert_eq!(conversation.id, "d:22");
                        break;
                    }
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } if negative => panic!("bad AUTH became Online"),
                    LiveEvent::Fault { .. } => {
                        panic!("local commands must survive rejected AUTH schema")
                    }
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
    }
}

#[test]
fn typed_parser_failure_keeps_auth_expiry_and_diagnostic_reason_without_payloads() {
    let error: anyhow::Error =
        wire::FrameFailure::new(WireError::Authentication, wire::FrameReason::InnerEnvelope).into();
    let safe = diagnostic(&error, LiveStage::Authentication);
    assert_eq!(safe.category, LiveCategory::Authentication);
    assert_eq!(safe.parser_reason, Some(wire::FrameReason::InnerEnvelope));
    assert_eq!(
        terminal_transport_state(&error),
        Some(LiveConnection::Expired)
    );
    assert_eq!(wire_error(&error), Some(&WireError::Authentication));
    assert!(safe.to_string().contains("parser InnerEnvelope"));
}

#[tokio::test]
async fn actual_runtime_survives_three_source_pongs_then_sends_via_layered_563_and_logs_invalid_message(
) {
    let f = Fixture::new(Reply::ControlHeartbeats);
    let logger = crate::diagnostics::Logger::start(&f.profile.paths.cache_dir, false).unwrap();
    let log_path = logger.path().to_path_buf();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(32);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut received = 0;
        let mut online = false;
        let mut directory_done = false;
        while received < 2 || !online || !directory_done {
            match events.recv().await.unwrap() {
                LiveEvent::Received { .. } => received += 1,
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => online = true,
                LiveEvent::DirectoryStatus { syncing: false, .. } => directory_done = true,
                LiveEvent::Diagnostic { diagnostic, .. } => {
                    panic!("unexpected startup failure: {diagnostic}")
                }
                LiveEvent::Fault { .. } => panic!("runtime stopped"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    for ping in 1..=3 {
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(31)).await;
        tokio::time::resume();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Received { message, .. }
                        if message.id == (902 + ping).to_string() =>
                    {
                        assert_eq!(message.text, "Synthetic encrypted transport message");
                        // Receiving this later encrypted frame proves that the preceding
                        // source PONG and cmd4 completed on the real runtime socket.
                        break;
                    }
                    LiveEvent::Diagnostic { diagnostic, .. } => {
                        panic!("legal heartbeat disconnected: {diagnostic}")
                    }
                    LiveEvent::Connection {
                        state: LiveConnection::Offline | LiveConnection::Expired,
                        ..
                    } => panic!("legal control became offline"),
                    LiveEvent::Fault { .. } => panic!("runtime stopped"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(f.state.pings.load(Ordering::SeqCst), ping as usize);
    }
    commands
        .send(LiveCommand::Send {
            generation: 71,
            thread: "d:22".into(),
            text: "Xin chào Linux".into(),
            request_id: 77,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Sent {
                    request_id: 77,
                    message,
                    ..
                } => {
                    assert_eq!(message.id, "901");
                    break;
                }
                LiveEvent::SendFailed { error, .. } => {
                    panic!("layered capability/HTTP send failed: {error}")
                }
                LiveEvent::Diagnostic { diagnostic, .. } => {
                    panic!("unexpected send failure: {diagnostic}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(f.state.observed_commands.lock().unwrap().contains(&563));
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Diagnostic { diagnostic, .. } => {
                    assert_eq!(
                        diagnostic.parser_reason,
                        Some(wire::FrameReason::OuterEnvelope)
                    );
                    break;
                }
                LiveEvent::Received { message, .. }
                    if message.text.contains("private-synthetic") =>
                {
                    panic!("invalid envelope became plaintext")
                }
                LiveEvent::Fault { .. } => panic!("parser fault must preserve local storage actor"),
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
    assert!(
        !f.state
            .observed_commands
            .lock()
            .unwrap()
            .iter()
            .any(|command| [532, 545, 557, 558].contains(command)),
        "invalid or unsupported data cannot acknowledge queues"
    );
    let database = rusqlite::Connection::open_with_flags(
        f.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id='11'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 6);
    drop(database);
    drop(logger);
    let text = fs::read_to_string(log_path).unwrap();
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let failure = rows
        .iter()
        .find(|row| {
            row["kind"] == "failure"
                && row["reason"] == "outer_envelope"
                && row["command_id"] == 501
        })
        .expect("actual runtime parser failure must reach disk");
    assert_eq!(failure["stage"], "synchronization");
    assert_eq!(failure["error_code_type"], "absent");
    assert_eq!(failure["data_type"], "string");
    assert_eq!(failure["encryption"], 0);
    for private in [
        "private-synthetic-message",
        "private-synthetic-token",
        "secret.invalid",
        "zpw_sek",
        "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        "Synthetic encrypted transport message",
        "Xin chào Linux",
    ] {
        assert!(
            !text.contains(private),
            "logs cannot retain fixture payloads or credentials"
        )
    }
}
