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
    EchoAfterAck(bool),
    ConflictingReceive,
    Stickers,
    RuntimeStall,
    ControlHeartbeats,
    OfflinePages,
    OfflineCycle,
    NullableReceive(u8),
    PeerRouting(bool),
    InfoPages(bool),
    MediaPages(bool),
    RichCards,
    UpgradeForbidden,
    DesktopHeaders,
    UpgradeStall,
    AuthExpired,
    AuthMalformed,
    AuthFixture(usize, bool),
    Directory,
    DirectoryFail,
    DirectoryStall,
    Privacy,
}
#[derive(Default)]
struct State {
    requests: Mutex<Vec<(String, String)>>,
    sni: Mutex<Vec<String>>,
    closed: AtomicUsize,
    socket_requests: Mutex<Vec<(u16, u64)>>,
    upgrades: AtomicUsize,
    directory_stalled: AtomicBool,
    directory_enabled: AtomicBool,
    observed_commands: Mutex<Vec<u16>>,
    pings: AtomicUsize,
    http_sends: AtomicUsize,
    echoed: AtomicBool,
    privacy_requests: AtomicUsize,
    privacy_hold: AtomicBool,
    privacy_policy: AtomicUsize,
    eligibility_mode: AtomicUsize,
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
        let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"]})).unwrap();
        Arc::new(synthetic_session(self.profile.clone(), config))
    }
    fn directory_session(&self, websocket: bool) -> Arc<VerifiedSession> {
        self.state.directory_enabled.store(true, Ordering::SeqCst);
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
            account: "11".into(),
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
        let auth = json!({"error_code":0,"data":{"qCmds":if matches!(reply,Reply::InfoPages(_)|Reply::MediaPages(_)|Reply::RichCards){json!([{"cmd":511,"subCmd":1}])}else if matches!(reply,Reply::PeerRouting(_)){json!([{"cmd":510,"subCmd":1}])}else if matches!(reply,Reply::Runtime|Reply::Stickers|Reply::RuntimeStall|Reply::DirectoryStall|Reply::ControlHeartbeats){json!([{"cmd":510,"subCmd":0},{"cmd":511,"subCmd":0}])}else{json!([{"cmd":510,"subCmd":0}])}}});
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
                    if let Reply::InfoPages(conflict) = reply {
                        assert_eq!((cmd, bytes[3]), (511, 1));
                        let first = data["first"].as_bool().unwrap();
                        let last_id = data["lastId"].as_u64().unwrap();
                        assert_eq!(last_id, if first { 1 } else { 100 });
                        state.socket_requests.lock().unwrap().push((cmd, last_id));
                        let bytes = wire::encode_frame(511, 1, &json!({"error_code":0,"encrypt":0,"data":information_batch(first, conflict)})).unwrap();
                        ws.send(tungstenite::Message::Binary(bytes.clone()))
                            .unwrap();
                        if !first && !conflict {
                            ws.send(tungstenite::Message::Binary(bytes)).unwrap();
                        }
                    }
                    if matches!(reply, Reply::MediaPages(_) | Reply::RichCards) {
                        let conflict = matches!(reply, Reply::MediaPages(true));
                        let rich = matches!(reply, Reply::RichCards);
                        assert_eq!((cmd, bytes[3]), (511, 1));
                        let first = data["first"].as_bool().unwrap();
                        let last_id = data["lastId"].as_u64().unwrap();
                        assert_eq!(last_id, if first { 1 } else { 100 });
                        state.socket_requests.lock().unwrap().push((cmd, last_id));
                        let bytes = wire::encode_frame(511, 1, &json!({"error_code":0,"encrypt":0,"data":if rich { rich_card_batch(first) } else { media_batch(first, conflict) }})).unwrap();
                        ws.send(tungstenite::Message::Binary(bytes.clone()))
                            .unwrap();
                        if !first && !conflict {
                            ws.send(tungstenite::Message::Binary(bytes)).unwrap();
                        }
                    }
                    if let Reply::PeerRouting(invalid) = reply {
                        assert_eq!((cmd, bytes[3]), (510, 1));
                        let first = data["first"].as_bool().unwrap();
                        let last_id = data["lastId"].as_u64().unwrap();
                        assert_eq!(last_id, if first { 1 } else { 100 });
                        state.socket_requests.lock().unwrap().push((cmd, last_id));
                        ws.send(tungstenite::Message::Binary(wire::encode_frame(510, 1, &json!({"error_code":0,"encrypt":0,"data":peer_addressed_batch(first, invalid)})).unwrap())).unwrap();
                    }
                    if cmd == 510 {
                        if let Reply::NullableReceive(malformed) = reply {
                            let last_id = data["lastId"].as_u64().unwrap();
                            let first = data["first"].as_bool().unwrap();
                            assert_eq!(last_id, if first { 1 } else { 100 });
                            state.socket_requests.lock().unwrap().push((cmd, last_id));
                            let mut messages = if first {
                                json!([
                                    {"uidFrom":"22","idTo":"11","msgId":"821","cliMsgId":0,"ts":"1760000000001","msgType":"webchat","content":"nullable-one"},
                                    {"uidFrom":"22","idTo":"11","msgId":"822","cliMsgId":null,"ts":"1760000000002","msgType":"webchat","content":"nullable-two"}
                                ])
                            } else {
                                json!([
                                    {"uidFrom":"22","idTo":"11","msgId":"823","cliMsgId":"0","ts":"1760000000003","msgType":"webchat","content":"nullable-three"}
                                ])
                            };
                            if !first && [1, 2].contains(&malformed) {
                                let mut invalid = json!({"uidFrom":"22","idTo":"11","msgId":"824","cliMsgId":"1760000000004","ts":"1760000000004","msgType":"webchat","content":"must-not-partially-commit"});
                                if malformed == 1 {
                                    invalid["cliMsgId"] =
                                        json!({"private-fixture":"never-log-this"});
                                } else {
                                    invalid["uidFrom"] = json!("not-a-user");
                                }
                                messages.as_array_mut().unwrap().push(invalid);
                            }
                            let continuation = if first {
                                json!("100")
                            } else {
                                match malformed {
                                    3 | 6 => Value::Null,
                                    4 | 7 => json!(0),
                                    5 | 8 => json!("0"),
                                    9 => json!({"private-cursor":"never-log-this"}),
                                    _ => json!("200"),
                                }
                            };
                            let payload = json!({"error_code":0,"data":{"msgs":messages,"groupMsgs":null,"pageMsgs":null,"more":first||[1,2,6,7,8].contains(&malformed),"lastActionId":continuation}});
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
                    }
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
                        if let Reply::EchoAfterAck(different_client_id) = reply {
                            if state.http_sends.load(Ordering::SeqCst) > 0
                                && !state.echoed.swap(true, Ordering::SeqCst)
                            {
                                let mut message = json!({"uidFrom":"11","idTo":"22","msgId":"901","ts":"1760000000999","msgType":"webchat","content":"Xin chào Linux"});
                                if different_client_id {
                                    message["cliMsgId"] = json!("999999999999");
                                }
                                let payload = json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"msgs":[message]}}});
                                let bytes = wire::encode_frame(501, 0, &payload).unwrap();
                                ws.send(tungstenite::Message::Binary(bytes.clone()))
                                    .unwrap();
                                ws.send(tungstenite::Message::Binary(bytes)).unwrap();
                            }
                        }
                        let mode = state.eligibility_mode.load(Ordering::SeqCst);
                        let mut page = json!({"err":0,"hasMore":false,"uids":[],"threads":[],"waitUids":[],"pendingUids":[]});
                        match mode {
                            1 => {
                                page.as_object_mut().unwrap().remove("hasMore");
                            }
                            2 => page["hasMore"] = Value::Null,
                            3 => page["hasMore"] = json!(0),
                            10..=13 => {
                                page.as_object_mut().unwrap().remove(
                                    ["threads", "waitUids", "uids", "pendingUids"][mode - 10],
                                );
                            }
                            14 => page["threads"] = json!({}),
                            20 => page["threads"] = json!([{"uid":"22"}]),
                            21 => page["waitUids"] = json!(["22"]),
                            22 => page["uids"] = json!(["22"]),
                            23 => page["pendingUids"] = json!([{"uid":"22","deviceId":1}]),
                            30 => {
                                // Arrives after eligibility request, before its answer.
                                let changed = json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{}}});
                                ws.send(tungstenite::Message::Binary(
                                    wire::encode_frame(551, 0, &changed).unwrap(),
                                ))
                                .unwrap();
                            }
                            _ => {}
                        }
                        let response =
                            json!({"error_code":0,"data":{"reqId":data["reqId"],"data":page}});
                        ws.send(tungstenite::Message::Binary(
                            wire::encode_frame(563, 0, &json!({"error_code":0,"encrypt":0,"data":serde_json::to_string(&response).unwrap()})).unwrap(),
                        ))
                        .unwrap();
                    }
                    if [510, 511].contains(&cmd)
                        && matches!(
                            reply,
                            Reply::Runtime
                                | Reply::ConflictingReceive
                                | Reply::Stickers
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
                        if matches!(reply, Reply::Stickers) {
                            let sticker = json!({"uidFrom":"22","idTo":if group{"22"}else{"11"},"msgId":if group{"805"}else{"803"},"ts":"1760000000002","msgType":"chat.sticker","content":{"id":20695,"catId":10047,"type":3}});
                            payload["data"][field]
                                .as_array_mut()
                                .unwrap()
                                .push(sticker.clone());
                            if !group {
                                let mut bad = sticker;
                                bad["msgId"] = json!("804");
                                bad["content"] = json!({"id":"encrypted-private-token","catId":"not-a-number","type":{}});
                                payload["data"][field].as_array_mut().unwrap().push(bad);
                            }
                        }
                        payload["data"]["queueStatus"][queue] =
                            json!({"lastId":if group{"200"}else{"100"},"ids":[],"evict":0});
                        let outer = json!({"error_code":0,"encrypt":0,"data":payload});
                        let frame = wire::encode_frame(cmd, 0, &outer).unwrap();
                        ws.send(tungstenite::Message::Binary(frame.clone()))
                            .unwrap();
                        if matches!(reply, Reply::ConflictingReceive) {
                            let mut conflict = outer;
                            conflict["data"]["data"][field][0]["content"] =
                                json!("conflicting synthetic content");
                            conflict["data"]["data"]["queueStatus"][queue]["lastId"] = json!("999");
                            ws.send(tungstenite::Message::Binary(
                                wire::encode_frame(cmd, 0, &conflict).unwrap(),
                            ))
                            .unwrap();
                        } else {
                            ws.send(tungstenite::Message::Binary(frame)).unwrap();
                        }
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
        "/api/hiddenconvers/get-all",
        "/api/group/getlg/v3",
        "/api/social/friend/getfriends",
        "/api/social/profile/me-v2",
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
            "/api/social/profile/me-v2" => {
                assert_eq!(parameters["avatar_size"], 120);
                let query = url::form_urlencoded::parse(path.split_once('?').unwrap().1.as_bytes());
                assert_eq!(
                    query.into_iter().find(|(name, _)| name == "os").unwrap().1,
                    "8"
                );
                json!({"profile":{"userId":"11","displayName":"Synthetic","avatar":"https://s120.avatar.talk.zdn.vn/self.png"}})
            }
            "/api/hiddenconvers/get-all" => {
                assert_eq!(parameters, json!({"imei":"native-fixture"}));
                state.privacy_requests.fetch_add(1, Ordering::SeqCst);
                let started = std::time::Instant::now();
                while state.privacy_hold.load(Ordering::SeqCst)
                    && started.elapsed() < Duration::from_secs(3)
                {
                    std::thread::park_timeout(Duration::from_millis(2));
                }
                if matches!(reply, Reply::Privacy) {
                    match state.privacy_policy.load(Ordering::SeqCst) {
                        0 => {
                            json!({"pin":"81dc9bdb52d04dc20036dbd8313ed055","threads":[{"is_group":0,"thread_id":"22"},{"is_group":1,"thread_id":"22"}]})
                        }
                        1 => {
                            json!({"pin":"4a7d1ed414474e4033ac29ccb8653d9b","threads":[{"is_group":0,"thread_id":"22"},{"is_group":1,"thread_id":"22"}]})
                        }
                        2 => {
                            let _=stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            let _ = stream.flush();
                            return;
                        }
                        _ => json!({"pin":null,"threads":[]}),
                    }
                } else {
                    json!({"pin":null,"threads":[]})
                }
            }
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
                // A generic session advertises conversation solely for mandatory privacy checks.
                // Only an explicitly requested directory fixture seeds preview history/sidecars.
                if !state.directory_enabled.load(Ordering::SeqCst) {
                    json!({"msgs":[]})
                } else if matches!(reply, Reply::MediaPages(_) | Reply::RichCards) {
                    let rich = matches!(reply, Reply::RichCards);
                    let mut data = if rich {
                        rich_card_batch(true)
                    } else {
                        media_batch(true, false)
                    }["data"]
                        .clone();
                    let mut direct = Vec::new();
                    for (id, kind) in if rich {
                        [("841", "chat.recommended"), ("843", "group.poll")]
                    } else {
                        [("841", "chat.photo"), ("843", "chat.webcontent")]
                    } {
                        let mut content = if rich {
                            rich_card_content(kind)
                        } else {
                            media_content(kind, false)
                        };
                        content["unknownOuter"]["threadVariant"] = json!("direct");
                        direct.push(json!({"uidFrom":"22","idTo":"33","msgId":id,"ts":"1760000000002","msgType":kind,"content":content}));
                    }
                    direct.extend((9000..9200).map(|id| json!({"uidFrom":"22","idTo":"11","msgId":id.to_string(),"ts":"1760000000001","msgType":"webchat","content":"bounded-media-preview-chunk"})));
                    data["msgs"] = json!(direct);
                    data["pageMsgs"] = Value::Null;
                    data["eeMsgs"] = Value::Null;
                    data
                } else if matches!(reply, Reply::InfoPages(_)) {
                    let mut data = information_batch(true, false)["data"].clone();
                    let mut content = information_content();
                    content["unknownOuter"]["threadVariant"] = json!("direct");
                    let mut direct = vec![
                        json!({"uidFrom":"22","idTo":"11","msgId":"841","ts":"1760000000002","msgType":"webchat","content":content}),
                    ];
                    direct.extend((9000..9200).map(|id| json!({"uidFrom":"22","idTo":"11","msgId":id.to_string(),"ts":"1760000000001","msgType":"webchat","content":"bounded-preview-chunk-fixture"})));
                    data["msgs"] = json!(direct);
                    data
                } else {
                    json!({"msgs":[{"uidFrom":"22","idTo":"11","msgId":"910","ts":"1760000000020","msgType":"webchat","content":"recent preview"}]})
                }
            }
        };
        let inner = json!({"error_code":0,"data":data});
        let response=serde_json::to_vec(&json!({"error_code":0,"data":cipher.encrypt(&serde_json::to_vec(&inner).unwrap()).unwrap()})).unwrap();
        if let Err(error) = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        ) {
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::UnexpectedEof
                ),
                "unexpected synthetic response write failure: {error}"
            );
            return;
        }
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
        let server_id = if matches!(reply, Reply::EchoAfterAck(_)) {
            if state.http_sends.fetch_add(1, Ordering::SeqCst) == 0 {
                "901"
            } else {
                "902"
            }
        } else if route == "/api/group/sendmsg" {
            "902"
        } else {
            "901"
        };
        json!({"error_code":0,"data":{"msgId":server_id,"ts":"1760000000001"}})
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

fn peer_addressed_batch(first: bool, invalid: bool) -> Value {
    if first {
        json!({"error_code":0,"data":{"more":true,"lastActionId":"100","msgs":[
            {"uidFrom":"22","idTo":"33","msgId":"831","cliMsgId":null,"ts":"1760000000001","msgType":"chat.photo","content":{"description":"private-photo-fixture"}},
            {"uidFrom":"0","idTo":"22","msgId":"832","cliMsgId":0,"ts":"1760000000002","msgType":"webchat","content":"own-sentinel-fixture"}
        ],"groupMsgs":[
            {"uidFrom":"33","idTo":"22","msgId":"833","ts":"1760000000003","msgType":"webchat","content":"group-fixture"}
        ],"pageMsgs":[
            {"uidFrom":"44","idTo":"55","msgId":"834","ts":"1760000000004","msgType":"webchat","content":"page-fixture"}
        ]}})
    } else {
        let mut payload = json!({"error_code":0,"data":{"more":false,"lastActionId":"200","msgs":[
            {"uidFrom":"22","idTo":"33","msgId":"835","ts":"1760000000005","msgType":"webchat","content":"second-peer-fixture"}
        ]}});
        if invalid {
            payload["data"]["msgs"].as_array_mut().unwrap().push(json!(
                {"uidFrom":"22","idTo":"bad-recipient","msgId":"836","ts":"1760000000006","msgType":"webchat","content":"must-not-commit-prefix"}
            ));
        }
        payload
    }
}

fn rich_card_content(kind: &str) -> Value {
    let params = if kind == "group.poll" {
        json!({"msg":{"vi":"Bình chọn mới"},"question":"Nhóm chọn ngày nào?","isAnonymous":true,"pollId":"synthetic-poll","unknownInner":{"retained":[null,true]}})
    } else {
        json!({"mediaTitle":"Tiêu đề dự phòng","unknownInner":{"retained":[null,true]}})
    };
    json!({"action":"recommened.link","title":"Nội dung chia sẻ tổng hợp","description":"Mô tả tiếng Việt","href":"https://example.invalid/article","thumb":"https://photo-talk.zadn.vn/synthetic.jpg","params":serde_json::to_string(&params).unwrap(),"unknownOuter":{"retained":[7,false,{"full":"metadata"}]}})
}
fn rich_card_batch(first: bool) -> Value {
    let mut batch = media_batch(first, false);
    for message in batch["data"]["groupMsgs"].as_array_mut().unwrap() {
        if let Some(kind) = match message["msgType"].as_str() {
            Some("chat.photo") => Some("chat.recommended"),
            Some("chat.webcontent") => Some("group.poll"),
            _ => None,
        } {
            message["msgType"] = json!(kind);
            message["content"] = rich_card_content(kind);
        }
    }
    batch
}

fn media_content(kind: &str, changed: bool) -> Value {
    let params = if kind == "chat.photo" {
        json!({"width":640,"height":480,"hd":"https://photo-talk.zadn.vn/synthetic-hd.jpg","unknownInner":[null,true,{"retain":"full"}]})
    } else {
        json!({"customMsg":{"msg":{"vi":"Thông báo đơn hàng tổng hợp"}},"pcItem":{"bundle_data":{"template":"opaque-remote-template","unknown":[1,true,null]},"data_url":"https://fixture.invalid/never-fetch-template.js"},"unknownInner":{"keep":"full"}})
    };
    json!({"title":"Nội dung tổng hợp","description":"Mô tả tiếng Việt","thumb":"https://photo-talk.zadn.vn/synthetic-thumb.jpg","href":"https://photo-talk.zadn.vn/synthetic.jpg","params":serde_json::to_string(&params).unwrap(),"unknownOuter":{"revision":if changed{2}else{1},"mustRetain":[7,false,{"key":"private-synthetic-value"}]}})
}
fn media_batch(first: bool, conflict: bool) -> Value {
    let photo = json!({"uidFrom":"33","idTo":"22","msgId":"841","ts":"1760000000002","msgType":"chat.photo","content":media_content("chat.photo",!first && conflict)});
    let card = json!({"uidFrom":"33","idTo":"22","msgId":"843","ts":"1760000000002","msgType":"chat.webcontent","content":media_content("chat.webcontent",false)});
    let plain = json!({"uidFrom":"44","idTo":"22","msgId":if first{"840"}else{"842"},"ts":if first{"1760000000001"}else{"1760000000003"},"msgType":"webchat","content":if first{"plain-before-media"}else{"plain-after-media"}});
    json!({"error_code":0,"data":{"more":first,"lastActionId":if first{"100"}else{"200"},"groupMsgs":[plain,photo,card]}})
}

fn information_content() -> Value {
    json!({
        "action":"msginfo.actionlist",
        "title":"Tiêu đề dự phòng",
        "params":serde_json::to_string(&json!({
            "msg":{"vi":"%1$s đã tham gia nhóm","en":"%1$s joined the group"},
            "highLightsV2":[{"type":0,"uid":"33","dpn":"Nguyễn An","ignoreNickname":true}],
            "actions":[{"actionType":"action.open.url","actionLabel":"Xem","actionData":"https://fixture.invalid/private-action"}],
            "unknownInner":{"list":[1,true,null,{"value":"private-rich-fixture"}]}
        })).unwrap(),
        "unknownOuter":{"mustRetain":[7,false,"outer"]}
    })
}

fn information_batch(first: bool, conflict: bool) -> Value {
    let mut content = information_content();
    if !first && conflict {
        let mut params: Value = serde_json::from_str(content["params"].as_str().unwrap()).unwrap();
        params["actions"][0]["actionData"] = json!("https://fixture.invalid/changed-action");
        content["params"] = json!(serde_json::to_string(&params).unwrap());
    }
    let info = json!({"uidFrom":"33","idTo":"22","msgId":"841","ts":"1760000000002","msgType":"webchat","content":content});
    let plain = json!({"uidFrom":"44","idTo":"22","msgId":if first{"840"}else{"842"},"ts":if first{"1760000000001"}else{"1760000000003"},"msgType":"webchat","content":if first{"plain-before-info"}else{"plain-after-info"}});
    json!({"error_code":0,"data":{"more":first,"lastActionId":if first{"100"}else{"200"},"groupMsgs":[plain,info]}})
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
        privacy: Default::default(),
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
async fn actual_runtime_persists_mixed_sticker_metadata_before_publish_and_replay_is_idempotent() {
    let fixture = Fixture::new(Reply::Stickers);
    let (commands, rx) = mpsc::channel(8);
    let (tx, mut events) = mpsc::channel(8);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        fixture.profile.clone(),
        fixture.session(),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        fixture.transports(),
    ));
    let mut ids = std::collections::HashSet::new();
    let mut stickers = 0;
    tokio::time::timeout(Duration::from_secs(5),async {
        while ids.len()<5 {match events.recv().await.unwrap() {
            LiveEvent::Received {message,..}=>{
                assert!(ids.insert(message.id.clone()),"duplicate receive event");
                let connection=rusqlite::Connection::open_with_flags(fixture.profile.paths.data_dir.join("messages.sqlite"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
                let count:i64=connection.query_row("SELECT count(*) FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",rusqlite::params![message.account_id,message.thread_id,message.id],|row|row.get(0)).unwrap();assert_eq!(count,1);
                if let Some(sticker)=&message.sticker {
                    stickers+=1;let raw:String=connection.query_row("SELECT metadata FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",rusqlite::params![message.account_id,message.thread_id,message.id],|row|row.get(0)).unwrap();assert_eq!(serde_json::from_str::<crate::domain::StickerDescriptor>(&raw).unwrap(),*sticker);assert_eq!(message.text,crate::domain::LEGACY_STICKER_PLACEHOLDER);
                } else if message.id=="804" {assert!(!message.text.contains("private-token"));}
            },LiveEvent::Fault{error,..}=>panic!("sticker runtime fault: {error}"),_=>{}
        }}
    }).await.unwrap();
    assert_eq!(stickers, 2);
    commands
        .send(LiveCommand::Load {
            generation: 9,
            thread: "d:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Page {
                    generation: 9,
                    messages,
                    ..
                } => {
                    assert_eq!(messages.len(), 3);
                    assert_eq!(messages.iter().filter(|m| m.sticker.is_some()).count(), 1);
                    break;
                }
                LiveEvent::Fault { error, .. } => panic!("{error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    stop.send_replace(true);
    task.await.unwrap();
    let connection = rusqlite::Connection::open_with_flags(
        fixture.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM messages WHERE account_id='11'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 5);
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
async fn info_preview_and_socket_orders_keep_identical_full_metadata_without_ack_or_type_conflict()
{
    for preview_first in [false, true] {
        let f = Fixture::new(Reply::InfoPages(false));
        let db = Database::open(f.profile.clone(), "11".into())
            .await
            .unwrap();
        let (updates, _revision) = watch::channel(0);
        let mut sink = RuntimeDirectorySink {
            account: "11".into(),
            db: db.handle().unwrap(),
            updates,
        };
        let fetcher = DirectoryFetcher::with_client(f.client());
        let session = f.directory_session(false);
        let (_cancel, cancel) = watch::channel(false);
        let (tx, mut events) = mpsc::channel(4);
        let output = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let mut socket = f.connection().await;
        if preview_first {
            let report = fetcher
                .synchronize(&session, &mut sink, cancel.clone())
                .await
                .unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(report.completed_previews);
            assert_eq!(report.preview_messages, 203);
            assert_eq!(
                db.call(|store| store.receive_cursor("11", "511_1"))
                    .await
                    .unwrap(),
                "1",
                "preview does not acknowledge a queue"
            );
            assert!(f.state.socket_requests.lock().unwrap().is_empty());
        }
        let auth = socket.next().await.unwrap().unwrap();
        socket.process(auth, &db, &output).await.unwrap();
        let first = socket.next().await.unwrap().unwrap();
        socket.process(first, &db, &output).await.unwrap();
        if !preview_first {
            let report = fetcher
                .synchronize(&session, &mut sink, cancel)
                .await
                .unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(report.completed_previews);
            assert_eq!(report.preview_messages, 203);
            assert_eq!(
                db.call(|store| store.receive_cursor("11", "511_1"))
                    .await
                    .unwrap(),
                "100",
                "preview cannot advance the socket cursor"
            );
        }
        let second = socket.next().await.unwrap().unwrap();
        socket.process(second, &db, &output).await.unwrap();
        let duplicate = socket.next().await.unwrap().unwrap();
        socket.process(duplicate, &db, &output).await.unwrap();
        let mut published = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(event, LiveEvent::Received { .. }) {
                published += 1;
            }
        }
        assert_eq!(published, if preview_first { 1 } else { 3 });
        assert_eq!(
            f.state.socket_requests.lock().unwrap().as_slice(),
            [(511, 1), (511, 100)]
        );
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
        drop(socket);
        drop(sink);
        drop(db);
        let store = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 3);
        let retained = store.message_info("11", "g:22", "841").unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(retained.as_json()).unwrap(),
            information_content()
        );
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "200");
        let direct = store.message_info("11", "d:22", "841").unwrap().unwrap();
        let mut expected_direct = information_content();
        expected_direct["unknownOuter"]["threadVariant"] = json!("direct");
        assert_eq!(
            serde_json::from_str::<Value>(direct.as_json()).unwrap(),
            expected_direct,
            "bindings sharing a message ID must stay in their own chunk/thread"
        );
        assert!(store.message_info("33", "g:22", "841").unwrap().is_none());
    }
}

#[tokio::test]
async fn media_preview_and_socket_orders_preserve_complete_metadata_across_chunks_and_restart() {
    for (rich, preview_first) in [(false, false), (false, true), (true, false), (true, true)] {
        let f = Fixture::new(if rich {
            Reply::RichCards
        } else {
            Reply::MediaPages(false)
        });
        let db = Database::open(f.profile.clone(), "11".into())
            .await
            .unwrap();
        let (updates, _revision) = watch::channel(0);
        let mut sink = RuntimeDirectorySink {
            account: "11".into(),
            db: db.handle().unwrap(),
            updates,
        };
        let fetcher = DirectoryFetcher::with_client(f.client());
        let session = f.directory_session(false);
        let (_cancel, cancel) = watch::channel(false);
        let (tx, mut events) = mpsc::channel(8);
        let output = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let mut socket = f.connection().await;
        if preview_first {
            let report = fetcher
                .synchronize(&session, &mut sink, cancel.clone())
                .await
                .unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(report.completed_previews);
            assert_eq!(report.preview_messages, 205);
            assert_eq!(
                db.call(|store| store.receive_cursor("11", "511_1"))
                    .await
                    .unwrap(),
                "1",
                "preview does not acknowledge a queue"
            );
            assert!(f.state.socket_requests.lock().unwrap().is_empty());
        }
        let auth = socket.next().await.unwrap().unwrap();
        socket.process(auth, &db, &output).await.unwrap();
        let first = socket.next().await.unwrap().unwrap();
        socket.process(first, &db, &output).await.unwrap();
        if !preview_first {
            let report = fetcher
                .synchronize(&session, &mut sink, cancel)
                .await
                .unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(report.completed_previews);
            assert_eq!(report.preview_messages, 205);
            assert_eq!(
                db.call(|store| store.receive_cursor("11", "511_1"))
                    .await
                    .unwrap(),
                "100",
                "preview cannot advance the socket cursor"
            );
        }
        let second = socket.next().await.unwrap().unwrap();
        socket.process(second, &db, &output).await.unwrap();
        let duplicate = socket.next().await.unwrap().unwrap();
        socket.process(duplicate, &db, &output).await.unwrap();
        let mut published = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(event, LiveEvent::Received { .. }) {
                published += 1;
            }
        }
        assert_eq!(published, if preview_first { 1 } else { 4 });
        assert_eq!(
            f.state.socket_requests.lock().unwrap().as_slice(),
            [(511, 1), (511, 100)]
        );
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
        drop(socket);
        drop(sink);
        drop(db);
        let store = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 4);
        for (id, kind) in if rich {
            [("841", "chat.recommended"), ("843", "group.poll")]
        } else {
            [("841", "chat.photo"), ("843", "chat.webcontent")]
        } {
            let retained = store.message_media("11", "g:22", id).unwrap().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(retained.as_json()).unwrap(),
                if rich {
                    rich_card_content(kind)
                } else {
                    media_content(kind, false)
                }
            );
            let direct = store.message_media("11", "d:22", id).unwrap().unwrap();
            let mut expected_direct = if rich {
                rich_card_content(kind)
            } else {
                media_content(kind, false)
            };
            expected_direct["unknownOuter"]["threadVariant"] = json!("direct");
            assert_eq!(
                serde_json::from_str::<Value>(direct.as_json()).unwrap(),
                expected_direct,
                "same IDs in different chunks must retain their own account/thread binding"
            );
            let page = store.page("11", "g:22", None, 10).unwrap();
            let message = page.iter().find(|message| message.id == id).unwrap();
            assert_eq!(message.media.as_ref(), Some(retained.descriptor()));
            assert!(store.message_media("33", "g:22", id).unwrap().is_none());
        }
        assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "200");
        assert!(store.message_media("33", "g:22", "841").unwrap().is_none());
    }
}

#[tokio::test]
async fn http_media_refresh_enriches_durable_legacy_placeholders_without_rewinding_cursor_or_outbox(
) {
    let f = Fixture::new(Reply::MediaPages(false));
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let original = wire::incoming_messages("11", &media_batch(true, false)).unwrap();
    let legacy: Vec<_> = original
        .into_iter()
        .filter(|row| row.media.is_some())
        .map(|mut row| {
            row.message.media = None;
            (row.message, None)
        })
        .collect();
    db.call(move |store| {
        store.receive_batch("11", &legacy, Some(("511_1", "900")))?;
        store.prepare_outgoing("11", "d:22", "Unrelated unsent draft", 1760000000004)?;
        Ok(())
    })
    .await
    .unwrap();
    let path = f.profile.paths.data_dir.join("messages.sqlite");
    let observer =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let legacy_rows = |connection: &rusqlite::Connection| {
        connection.prepare("SELECT message_id,sender,body,sent_at_ms FROM messages WHERE account_id='11' AND thread_id='g:22' ORDER BY message_id").unwrap()
            .query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?)))
            .unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    let outbox = |connection: &rusqlite::Connection| {
        connection
            .prepare("SELECT client_id,body,state,server_id FROM outgoing ORDER BY client_id")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let old_rows = legacy_rows(&observer);
    let old_outbox = outbox(&observer);
    // The open pane keeps its existing IDs/range. Its refresh request does not
    // select a conversation or ask for the newest history page.
    let (tx, mut refreshed) = mpsc::channel(4);
    let events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
        privacy: Default::default(),
    };
    let mut connection = None;
    let session = f.session();
    let client = f.client();
    handle_command(
        LiveCommand::RefreshMessages {
            generation: 71,
            thread: "g:22".into(),
            message_ids: vec!["841".into(), "843".into()],
        },
        &mut connection,
        &db,
        &events,
        &session,
        &client,
    )
    .await
    .unwrap();
    let old_pane = match refreshed.recv().await.unwrap() {
        LiveEvent::MessagesRefreshed {
            generation: 71,
            account_id,
            thread,
            messages,
        } => {
            assert_eq!((account_id.as_str(), thread.as_str()), ("11", "g:22"));
            assert_eq!(messages.len(), 2);
            assert!(messages.iter().all(|message| message.media.is_none()));
            messages
        }
        other => panic!("unexpected refresh event {other:?}"),
    };
    let (updates, _) = watch::channel(0);
    let mut sink = RuntimeDirectorySink {
        account: "11".into(),
        db: db.handle().unwrap(),
        updates,
    };
    let (_stop, cancel) = watch::channel(false);
    let report = DirectoryFetcher::with_client(f.client())
        .synchronize(&f.directory_session(false), &mut sink, cancel)
        .await
        .unwrap();
    assert!(report.failures.is_empty(), "{report:?}");
    assert!(report.completed_previews);
    handle_command(
        LiveCommand::RefreshMessages {
            generation: 72,
            thread: "g:22".into(),
            message_ids: vec!["841".into(), "843".into()],
        },
        &mut connection,
        &db,
        &events,
        &session,
        &client,
    )
    .await
    .unwrap();
    match refreshed.recv().await.unwrap() {
        LiveEvent::MessagesRefreshed {
            generation: 72,
            account_id,
            thread,
            messages,
        } => {
            assert_eq!((account_id.as_str(), thread.as_str()), ("11", "g:22"));
            assert_eq!(messages.len(), old_pane.len());
            for message in messages {
                let previous = old_pane.iter().find(|old| old.id == message.id).unwrap();
                let mut identity = message.clone();
                identity.media = None;
                assert_eq!(
                    &identity, previous,
                    "rehydration changes only the bounded media projection"
                );
                assert!(message.media.is_some());
            }
        }
        other => panic!("unexpected post-directory refresh event {other:?}"),
    }
    assert!(
        connection.is_none(),
        "pane refresh is an indexed local read, not a transport reconnect"
    );
    let after = legacy_rows(&observer);
    for row in &old_rows {
        assert!(
            after.contains(row),
            "legacy body/sender/timestamp must remain unchanged"
        );
    }
    assert_eq!(outbox(&observer), old_outbox);
    assert_eq!(
        db.call(|store| store.receive_cursor("11", "511_1"))
            .await
            .unwrap(),
        "900"
    );
    assert!(f.state.socket_requests.lock().unwrap().is_empty());
    assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
    drop(sink);
    drop(db);
    drop(observer);
    let mut store = MessageStore::open(&path).unwrap();
    for (id, kind) in [("841", "chat.photo"), ("843", "chat.webcontent")] {
        assert_eq!(
            serde_json::from_str::<Value>(
                store
                    .message_media("11", "g:22", id)
                    .unwrap()
                    .unwrap()
                    .as_json()
            )
            .unwrap(),
            media_content(kind, false)
        );
        assert!(store
            .page("11", "g:22", None, 10)
            .unwrap()
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .media
            .is_some());
    }
    let replay: Vec<_> = wire::incoming_messages("11", &media_batch(true, false))
        .unwrap()
        .into_iter()
        .map(|row| (row.message, row.client_id, row.info, row.media))
        .collect();
    assert!(store
        .receive_batch_with_media("11", &replay, Some(("511_1", "100")))
        .unwrap()
        .is_empty());
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "900");
}

#[tokio::test]
async fn actual_socket_media_raw_conflict_rolls_back_entire_following_page_and_cursor() {
    let f = Fixture::new(Reply::MediaPages(true));
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let (tx, mut events) = mpsc::channel(4);
    let output = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
        privacy: Default::default(),
    };
    let mut socket = f.connection().await;
    let auth = socket.next().await.unwrap().unwrap();
    socket.process(auth, &db, &output).await.unwrap();
    let first = socket.next().await.unwrap().unwrap();
    socket.process(first, &db, &output).await.unwrap();
    let second = socket.next().await.unwrap().unwrap();
    assert!(socket.process(second, &db, &output).await.is_err());
    let mut published = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, LiveEvent::Received { .. }) {
            published += 1;
        }
    }
    assert_eq!(published, 3);
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(511, 1), (511, 100)]
    );
    drop(socket);
    drop(db);
    let store = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "100");
    assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 3);
    assert_eq!(
        serde_json::from_str::<Value>(
            store
                .message_media("11", "g:22", "841")
                .unwrap()
                .unwrap()
                .as_json()
        )
        .unwrap(),
        media_content("chat.photo", false)
    );
    assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
}

#[test]
fn directory_runtime_mapping_records_precise_preview_failure_without_private_payload() {
    use crate::protocol::directory::{parse_previews_detailed, PreviewDiagnostic, PreviewField};
    let root = std::env::temp_dir().join(format!(
        "preview-log-{}",
        crate::domain::local_message_id().unwrap()
    ));
    crate::profile::create_private_dir(&root).unwrap();
    let logger = crate::diagnostics::Logger::isolated_for_test(&root, false).unwrap();
    let raw = json!({"msgs":[{"msgType":"chat.photo","uidFrom":"22","idTo":"11","msgId":"731","ts":1700000000000_i64,"content":[{"private":"https://private.invalid/token"}]}]});
    let parser = parse_previews_detailed("11", &raw)
        .err()
        .unwrap()
        .diagnostic;
    for diagnostic in [
        parser,
        PreviewDiagnostic {
            reason: wire::FrameReason::OuterJson,
            field: PreviewField::ResponseJson,
            field_type: wire::FrameValueType::Absent,
            field_bytes: None,
            message_kind: None,
        },
    ] {
        logger.record(directory_failure_event(&DirectoryFailure {
            stage: DirectoryStage::Previews,
            category: DirectoryCategory::Schema,
            http_status: None,
            server_code: None,
            parser: Some(diagnostic),
        }));
    }
    let path = logger.path().to_owned();
    drop(logger);
    let text = fs::read_to_string(path).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[0]["preview_field"], "msgs");
    assert_eq!(lines[0]["reason"], "message_content");
    assert_eq!(lines[0]["field_type"], "array");
    assert_eq!(lines[0]["message_kind"], "photo");
    assert_eq!(lines[1]["preview_field"], "response_json");
    assert_eq!(lines[1]["reason"], "outer_json");
    assert!(lines[1].get("message_kind").is_none());
    for event in lines.iter().take(2) {
        assert_eq!(event["directory_stage"], "previews");
        assert_eq!(event["directory_category"], "schema");
        assert!(event.as_object().unwrap().keys().all(|key| matches!(
            key.as_str(),
            "ts_ms"
                | "kind"
                | "stage"
                | "reason"
                | "directory_stage"
                | "directory_category"
                | "preview_field"
                | "field_type"
                | "message_kind"
        )));
    }
    assert!(!text.contains("private.invalid") && !text.contains("token"));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn info_batches_commit_full_content_before_publish_and_reject_changed_action_data_atomically()
{
    for conflict in [false, true] {
        let f = Fixture::new(Reply::InfoPages(conflict));
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
            f.transports(),
        ));
        let mut received = Vec::new();
        let mut rejected = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Received { message, .. } => {
                        assert_eq!(message.account_id, "11");
                        if message.id == "841" {
                            assert_eq!(message.text, "Nguyễn An đã tham gia nhóm");
                            let db = rusqlite::Connection::open_with_flags(f.profile.paths.data_dir.join("messages.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
                            let metadata: String = db.query_row("SELECT metadata FROM message_info WHERE account_id='11' AND thread_id='g:22' AND message_id='841'", [], |row| row.get(0)).unwrap();
                            assert_eq!(serde_json::from_str::<Value>(&metadata).unwrap(), information_content(), "entire structure must commit before UI publication");
                        }
                        received.push(message.id);
                        if !conflict && received.len() == 3 { break; }
                    }
                    LiveEvent::Diagnostic { diagnostic, .. } => {
                        assert!(conflict, "valid info event must not pause messaging");
                        assert_eq!(diagnostic.category, LiveCategory::Storage);
                        rejected = true;
                    }
                    LiveEvent::Connection { state: LiveConnection::Offline, .. } if rejected => break,
                    LiveEvent::Fault { error, .. } => panic!("local actor stopped: {error}"),
                    _ => {}
                }
            }
        }).await.unwrap();
        commands
            .send(LiveCommand::Load {
                generation: 94,
                thread: "g:22".into(),
                before: None,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Received { .. } => {
                        panic!("info replay must not duplicate publication")
                    }
                    LiveEvent::Page {
                        generation: 94,
                        messages,
                        ..
                    } => {
                        assert_eq!(messages.len(), if conflict { 2 } else { 3 });
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        commands.send(LiveCommand::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.len(), if conflict { 2 } else { 3 });
        assert_eq!(
            f.state.socket_requests.lock().unwrap().as_slice(),
            [(511, 1), (511, 100)]
        );
        assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 1);
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
        let request_paths: Vec<_> = f
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.split('?').next().unwrap().to_owned())
            .collect();
        assert_eq!(
            request_paths
                .iter()
                .filter(|path| path.as_str() == "/api/hiddenconvers/get-all")
                .count(),
            1
        );
        for optional_bootstrap in ["/api/group/getlg/v3", "/api/preloadconvers/get-last-msgs"] {
            assert!(
                request_paths
                    .iter()
                    .filter(|path| path.as_str() == optional_bootstrap)
                    .count()
                    <= 1
            );
        }
        assert!(
            request_paths.len() <= 3 && request_paths.iter().all(|path| matches!(path.as_str(),
                "/api/hiddenconvers/get-all" | "/api/group/getlg/v3" | "/api/preloadconvers/get-last-msgs")),
            "only bounded privacy/group/neutral-preview bootstrap reads are permitted: {request_paths:?}"
        );
        assert!(
            !request_paths
                .iter()
                .any(|path| path.contains("private-action") || path.contains("changed-action")),
            "stored action URLs must never execute"
        );
        for _ in 0..2 {
            let store =
                MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
            let info = store.message_info("11", "g:22", "841").unwrap().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(info.as_json()).unwrap(),
                information_content()
            );
            assert_eq!(
                store.receive_cursor("11", "511_1").unwrap(),
                if conflict { "100" } else { "200" }
            );
            assert_eq!(
                store.page("11", "g:22", None, 10).unwrap().len(),
                if conflict { 2 } else { 3 }
            );
            assert!(store.message_info("11", "d:22", "841").unwrap().is_none());
            assert!(store.message_info("33", "g:22", "841").unwrap().is_none());
            assert!(store.page("33", "g:22", None, 10).unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn failed_info_sidecar_write_keeps_batch_cursor_and_actor_then_explicit_retry_succeeds() {
    let f = Fixture::new(Reply::InfoPages(false));
    let path = f.profile.paths.data_dir.join("messages.sqlite");
    drop(MessageStore::open(&path).unwrap());
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_info BEFORE INSERT ON message_info BEGIN SELECT RAISE(ABORT,'private-rich-storage-fixture'); END;").unwrap();
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
        f.transports(),
    ));
    let mut failed = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Received { .. } => {
                    panic!("sidecar failure must roll back the entire message batch")
                }
                LiveEvent::Diagnostic { diagnostic, .. }
                    if diagnostic.category == LiveCategory::Storage =>
                {
                    failed = true
                }
                LiveEvent::Connection {
                    state: LiveConnection::Offline,
                    ..
                } if failed => break,
                LiveEvent::Fault { error, .. } => panic!("local actor stopped: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(511, 1)]
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM message_info", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    commands
        .send(LiveCommand::Load {
            generation: 95,
            thread: "g:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let LiveEvent::Page {
                generation: 95,
                messages,
                ..
            } = events.recv().await.unwrap()
            {
                assert!(messages.is_empty());
                break;
            }
        }
    })
    .await
    .unwrap();
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "1");
    drop(store);
    sql.execute_batch("DROP TRIGGER fail_info").unwrap();
    commands.send(LiveCommand::Reconnect).await.unwrap();
    let mut received = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        while received < 3 {
            match events.recv().await.unwrap() {
                LiveEvent::Received { message, .. } => {
                    if message.id == "841" {
                        assert_eq!(message.text, "Nguyễn An đã tham gia nhóm");
                    }
                    received += 1;
                }
                LiveEvent::Diagnostic { diagnostic, .. } => panic!("retry failed: {diagnostic}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    commands.send(LiveCommand::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(511, 1), (511, 1), (511, 100)]
    );
    assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 2);
    assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 3);
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "200");
    assert_eq!(
        serde_json::from_str::<Value>(
            store
                .message_info("11", "g:22", "841")
                .unwrap()
                .unwrap()
                .as_json()
        )
        .unwrap(),
        information_content()
    );
}

#[tokio::test]
async fn failed_media_sidecar_write_keeps_batch_cursor_and_actor_then_explicit_retry_succeeds() {
    let f = Fixture::new(Reply::MediaPages(false));
    let path = f.profile.paths.data_dir.join("messages.sqlite");
    drop(MessageStore::open(&path).unwrap());
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_media BEFORE INSERT ON message_media BEGIN SELECT RAISE(ABORT,'private-rich-storage-fixture'); END;").unwrap();
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
        f.transports(),
    ));
    let mut failed = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Received { .. } => {
                    panic!("sidecar failure must roll back the entire message batch")
                }
                LiveEvent::Diagnostic { diagnostic, .. }
                    if diagnostic.category == LiveCategory::Storage =>
                {
                    failed = true
                }
                LiveEvent::Connection {
                    state: LiveConnection::Offline,
                    ..
                } if failed => break,
                LiveEvent::Fault { error, .. } => panic!("local actor stopped: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(511, 1)]
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM message_media", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    commands
        .send(LiveCommand::Load {
            generation: 95,
            thread: "g:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let LiveEvent::Page {
                generation: 95,
                messages,
                ..
            } = events.recv().await.unwrap()
            {
                assert!(messages.is_empty());
                break;
            }
        }
    })
    .await
    .unwrap();
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "1");
    drop(store);
    sql.execute_batch("DROP TRIGGER fail_media").unwrap();
    commands.send(LiveCommand::Reconnect).await.unwrap();
    let mut received = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        while received < 4 {
            match events.recv().await.unwrap() {
                LiveEvent::Received { message, .. } => {
                    if message.id == "841" {
                        assert!(matches!(message.media.as_ref(), Some(crate::domain::MediaDescriptor::Photo(_))));
                    }
                    if let Some(descriptor) = &message.media {
                        let raw: String = sql.query_row(
                            "SELECT metadata FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                            rusqlite::params![message.account_id,message.thread_id,message.id],
                            |row|row.get(0),
                        ).unwrap();
                        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(),media_content(descriptor.kind().as_str(),false),
                            "complete raw content must be durable before publishing a media snapshot");
                    }
                    received += 1;
                }
                LiveEvent::Diagnostic { diagnostic, .. } => panic!("retry failed: {diagnostic}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    commands.send(LiveCommand::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.state.socket_requests.lock().unwrap().as_slice(),
        [(511, 1), (511, 1), (511, 100)]
    );
    assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 2);
    assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 4);
    assert_eq!(store.receive_cursor("11", "511_1").unwrap(), "200");
    assert_eq!(
        serde_json::from_str::<Value>(
            store
                .message_media("11", "g:22", "841")
                .unwrap()
                .unwrap()
                .as_json()
        )
        .unwrap(),
        media_content("chat.photo", false)
    );
}

#[tokio::test]
async fn authenticated_subqueue_routes_peer_addresses_without_crossing_accounts_or_partial_commit()
{
    for invalid in [false, true] {
        let f = Fixture::new(Reply::PeerRouting(invalid));
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
            f.transports(),
        ));
        let mut received = Vec::new();
        let mut rejected = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Received { message, .. } => {
                        assert_eq!(message.account_id, "11");
                        received.push(message);
                        if !invalid && received.len() == 5 {
                            break;
                        }
                    }
                    LiveEvent::Diagnostic { diagnostic, .. } => {
                        assert!(
                            invalid,
                            "valid peer addresses must not reject synchronization"
                        );
                        assert_eq!(
                            diagnostic.parser_reason,
                            Some(wire::FrameReason::MessageRecipient)
                        );
                        assert_eq!(diagnostic.stage, LiveStage::Synchronization);
                        rejected = true;
                    }
                    LiveEvent::Connection {
                        state: LiveConnection::Offline,
                        ..
                    } if rejected => break,
                    LiveEvent::Fault { error, .. } => panic!("local actor stopped: {error}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let direct = received.iter().find(|m| m.id == "831").unwrap();
        assert_eq!((&*direct.thread_id, &*direct.sender), ("d:22", "22"));
        assert_eq!(direct.text, "[Tin nhắn chat.photo chưa được hỗ trợ]");
        assert!(!direct.text.contains("private-photo"));
        let own = received.iter().find(|m| m.id == "832").unwrap();
        assert_eq!((&*own.thread_id, &*own.sender), ("d:22", "11"));
        let group = received.iter().find(|m| m.id == "833").unwrap();
        assert_eq!((&*group.thread_id, &*group.sender), ("g:22", "33"));
        let page = received.iter().find(|m| m.id == "834").unwrap();
        assert_eq!((&*page.thread_id, &*page.sender), ("d:44", "44"));
        if invalid {
            assert_eq!(received.len(), 4);
            commands
                .send(LiveCommand::Load {
                    generation: 92,
                    thread: "d:22".into(),
                    before: None,
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if let LiveEvent::Page {
                        generation: 92,
                        messages,
                        ..
                    } = events.recv().await.unwrap()
                    {
                        assert_eq!(messages.len(), 2);
                        break;
                    }
                }
            })
            .await
            .unwrap();
        }
        commands.send(LiveCommand::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            f.state.socket_requests.lock().unwrap().as_slice(),
            [(510, 1), (510, 100)]
        );
        assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 1);
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
        let store = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        assert_eq!(
            store.receive_cursor("11", "510_1").unwrap(),
            if invalid { "100" } else { "200" }
        );
        assert_eq!(
            store.page("11", "d:22", None, 10).unwrap().len(),
            if invalid { 2 } else { 3 }
        );
        assert_eq!(store.page("11", "g:22", None, 10).unwrap().len(), 1);
        assert_eq!(store.page("11", "d:44", None, 10).unwrap().len(), 1);
        for account in ["12", "22", "33", "44", "55"] {
            assert!(store.conversations(account, 10).unwrap().is_empty());
            assert_eq!(store.receive_cursor(account, "510_1").unwrap(), "1");
        }
    }
}

#[tokio::test]
async fn connection_account_and_authenticated_decoder_gate_peer_routing_before_storage() {
    let f = Fixture::new(Reply::Normal);
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let (tx, mut received) = mpsc::channel(4);
    let mut events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
        privacy: Default::default(),
    };
    let mut connection = f.connection().await;
    for authenticated in [false, true] {
        if authenticated {
            connection.next().await.unwrap();
            assert!(connection.decoder.authenticated());
            events.account = "12".into();
        }
        let error = connection
            .process(
                DecodedFrame {
                    cmd: 510,
                    sub_cmd: 1,
                    payload: peer_addressed_batch(true, false),
                },
                &db,
                &events,
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<wire::FrameFailure>().unwrap().reason,
            wire::FrameReason::MessageScope
        );
        assert!(received.try_recv().is_err());
        assert!(f.state.socket_requests.lock().unwrap().is_empty());
        assert_eq!(
            db.call(|store| store.receive_cursor("11", "510_1"))
                .await
                .unwrap(),
            "1"
        );
        for account in ["11", "12"] {
            assert!(db
                .call(move |store| store.conversations(account, 10))
                .await
                .unwrap()
                .is_empty());
        }
    }
    drop(connection);
}

#[tokio::test]
async fn actual_runtime_accepts_nullable_optional_fields_but_rejects_invalid_identity_atomically() {
    for malformed in 0..=9 {
        let accepted = malformed == 0 || [3, 4, 5].contains(&malformed);
        let f = Fixture::new(Reply::NullableReceive(malformed));
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
            f.transports(),
        ));
        let mut received = Vec::new();
        let mut failure = None;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Received { message, .. } => {
                        received.push(message.id);
                        if accepted && received.len() == 3 {
                            break;
                        }
                    }
                    LiveEvent::Diagnostic { diagnostic, .. } => {
                        assert!(!accepted, "valid optional fields must remain online");
                        assert_eq!(diagnostic.stage, LiveStage::Synchronization);
                        assert_eq!(diagnostic.category, LiveCategory::Protocol);
                        assert_eq!(
                            diagnostic.parser_reason,
                            Some(match malformed {
                                1 => wire::FrameReason::MessageClientId,
                                2 => wire::FrameReason::MessageSender,
                                _ => wire::FrameReason::QueueContinuation,
                            })
                        );
                        failure = Some(diagnostic);
                    }
                    LiveEvent::Connection {
                        state: LiveConnection::Offline,
                        ..
                    } if failure.is_some() => break,
                    LiveEvent::Fault { error, .. } => {
                        panic!("protocol error must preserve local actor: {error}")
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            received,
            if accepted {
                vec!["821", "822", "823"]
            } else {
                vec!["821", "822"]
            }
        );
        if !accepted {
            assert!(!task.is_finished());
            commands
                .send(LiveCommand::Load {
                    generation: 91,
                    thread: "d:22".into(),
                    before: None,
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if let LiveEvent::Page {
                        generation: 91,
                        messages,
                        ..
                    } = events.recv().await.unwrap()
                    {
                        assert_eq!(
                            messages.len(),
                            2,
                            "the valid prefix of a rejected page must not commit"
                        );
                        break;
                    }
                }
            })
            .await
            .unwrap();
        }
        commands.send(LiveCommand::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            f.state.socket_requests.lock().unwrap().as_slice(),
            [(510, 1), (510, 100)],
            "no cursor ACK/continuation for rejected page"
        );
        assert_eq!(
            f.state.upgrades.load(Ordering::SeqCst),
            1,
            "no automatic poison reconnect"
        );
        assert_eq!(
            f.state.http_sends.load(Ordering::SeqCst),
            0,
            "receive recovery must not replay sends"
        );
        let store = MessageStore::open(&f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        assert_eq!(
            store.receive_cursor("11", "510_0").unwrap(),
            if malformed == 0 { "200" } else { "100" }
        );
        assert_eq!(
            store.page("11", "d:22", None, 10).unwrap().len(),
            if accepted { 3 } else { 2 }
        );
        assert_eq!(store.receive_cursor("12", "510_0").unwrap(), "1");
        assert!(store.page("12", "d:22", None, 10).unwrap().is_empty());
    }
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
    assert!(
        !task.is_finished(),
        "local actor remains available after failed receive"
    );
    commands.send(LiveCommand::Shutdown).await.unwrap();
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
        privacy: Default::default(),
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
    let error = result.unwrap_err();
    assert_eq!(wire_error(&error), Some(&WireError::Oversize));
    assert_eq!(
        error.downcast_ref::<wire::FrameFailure>().unwrap().reason,
        wire::FrameReason::QueuePageLimit
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
            sticker: None,
            media: None,
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
                ServerConfig::parse(
                    &json!({"zpw_service_map_v3":{"conversation":["https://wpa.zaloapp.com"]}}),
                )
                .unwrap(),
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
            if attempt == 0 {
                // Test every terminal class beyond all reconnect backoffs. Startup
                // privacy is already verified before this first transport attempt.
                // The explicit second reconnect may have a real TLS privacy fetch
                // in flight, whose deadline must not be expired by artificial time.
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(60)).await;
                tokio::time::resume();
                tokio::task::yield_now().await;
                assert!(matches!(
                    events.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
            }
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
        {
            let requests = f.state.requests.lock().unwrap();
            let count = |route: &str| {
                requests
                    .iter()
                    .filter(|(path, _)| path.split('?').next() == Some(route))
                    .count()
            };
            assert_eq!(count("/api/group/getlg/v3"), usize::from(reply.is_some()));
            assert_eq!(count("/api/preloadconvers/get-last-msgs"), 1);
            // One startup check, one explicit reconnect and at most one periodic
            // check from the deliberate 60-second advance; in-flight requests coalesce.
            assert!((2..=3).contains(&count("/api/hiddenconvers/get-all")));
            assert!(requests.len() <= 5);
            assert!(requests.iter().all(|(path, body)| body.is_empty()
                && matches!(
                    path.split('?').next(),
                    Some(
                        "/api/hiddenconvers/get-all"
                            | "/api/group/getlg/v3"
                            | "/api/preloadconvers/get-last-msgs"
                    )
                )));
        }
        assert_eq!(f.state.http_sends.load(Ordering::SeqCst), 0);
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
        // Startup and explicit directory refresh each verify the hidden policy too.
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.state.privacy_requests.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        {
            let requests = f.state.requests.lock().unwrap();
            let hidden = requests
                .iter()
                .filter(|(path, _)| path.split('?').next() == Some("/api/hiddenconvers/get-all"))
                .count();
            assert_eq!(hidden, 2);
            assert_eq!(
                requests.len() - hidden,
                8,
                "two complete friend/group/preview/self-profile discovery passes"
            );
            assert_eq!(requests.len(), 10);
        }
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
        message_info: Vec::new(),
        message_media: Vec::new(),
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
        sticker: None,
        media: None,
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
        privacy: Default::default(),
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
        8,
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

#[tokio::test]
async fn sender_name_commands_use_the_existing_actor_and_echo_only_the_requested_scope() {
    let f = Fixture::new(Reply::Normal);
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(4);
    let events = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
        privacy: Default::default(),
    };
    let session = f.session();
    let client = f.client();
    let mut connection = None;
    for (generation, name) in [(41, "Nguyễn Hà"), (42, "Nguyễn Hà mới")] {
        let record = crate::store::directory::DirectoryRecord {
            remote_id: "22".into(),
            is_group: false,
            display_name: name.into(),
            avatar_url: None,
            is_friend: Some(true),
            is_blocked: None,
            member_count: None,
        };
        let mut group = record.clone();
        group.is_group = true;
        group.display_name = "Không được lấy tên nhóm cho thành viên".into();
        let mut foreign = record.clone();
        foreign.display_name = "Tên tài khoản khác".into();
        db.call(move |store| {
            store.apply_directory_batch(&DirectoryBatch {
                message_info: Vec::new(),
                message_media: Vec::new(),
                account_id: "11".into(),
                entries: vec![record, group],
                messages: vec![],
                observed_at_ms: generation as i64,
            })?;
            store.apply_directory_batch(&DirectoryBatch {
                message_info: Vec::new(),
                message_media: Vec::new(),
                account_id: "33".into(),
                entries: vec![foreign],
                messages: vec![],
                observed_at_ms: generation as i64,
            })?;
            Ok(())
        })
        .await
        .unwrap();
        handle_command(
            LiveCommand::ResolveSenders {
                generation,
                thread: "g:22".into(),
                sender_ids: vec!["22".into(), "99".into()],
            },
            &mut connection,
            &db,
            &events,
            &session,
            &client,
        )
        .await
        .unwrap();
        match rx.recv().await.unwrap() {
            LiveEvent::SenderNames {
                account_id,
                generation: actual,
                thread,
                names,
            } => {
                assert_eq!(account_id, "11");
                assert_eq!(actual, generation);
                assert_eq!(thread, "g:22");
                assert_eq!(
                    names,
                    vec![SenderName {
                        sender_id: "22".into(),
                        display_name: name.into()
                    }]
                );
            }
            other => panic!("Unexpected sender-name reply {other:?}"),
        }
    }
    assert!(connection.is_none());
    assert!(
        f.state.requests.lock().unwrap().is_empty(),
        "name resolution must not fetch remote profiles"
    );
    assert_eq!(f.state.upgrades.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn http_ack_then_own_websocket_echo_without_matching_client_id_remains_usable() {
    for different_client_id in [false, true] {
        let f = Fixture::new(Reply::EchoAfterAck(different_client_id));
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
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await.unwrap() {
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } => break,
                    LiveEvent::Fault { error, .. } => panic!("unexpected startup failure {error}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let mut echoes = 0;
        for request_id in [1, 2] {
            commands
                .send(LiveCommand::Send {
                    generation: 1,
                    thread: "d:22".into(),
                    // The real TLS fixture decrypts the request and requires the
                    // same canonical text as the durable outgoing row and echo.
                    text: "\u{feff} \tXin chào Linux \r\n".into(),
                    request_id,
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5),async {
                loop {match events.recv().await.unwrap() {
                    LiveEvent::Sent{request_id:actual,message,..}=>{assert_eq!(actual,request_id);assert_eq!(message.text,"Xin chào Linux");assert_eq!(message.id,if request_id==1{"901"}else{"902"});break;}
                    LiveEvent::Received{message,..} if message.id=="901"=>{
                        echoes+=1;assert_eq!(message.sent_at_ms,1760000000999);
                        let db=rusqlite::Connection::open_with_flags(f.profile.paths.data_dir.join("messages.sqlite"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
                        let durable:i64=db.query_row("SELECT sent_at_ms FROM messages WHERE account_id='11' AND thread_id='d:22' AND message_id='901'",[],|r|r.get(0)).unwrap();
                        assert_eq!(durable,message.sent_at_ms);
                    }
                    LiveEvent::Diagnostic{diagnostic,..} if diagnostic.category==LiveCategory::Storage=>panic!("valid own echo must not cause storage failure"),
                    LiveEvent::SendFailed{error,..}|LiveEvent::Fault{error,..}=>panic!("unexpected send failure {error}"),_=>{}
                }}
            }).await.unwrap();
        }
        assert_eq!(echoes, 1, "duplicate echo must not duplicate publication");
        let db = rusqlite::Connection::open_with_flags(
            f.profile.paths.data_dir.join("messages.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let canonical:bool=db.query_row("SELECT count(*)=2 AND min(body)=?1 AND max(body)=?1 FROM outgoing WHERE account_id='11'",["Xin chào Linux"],|row|row.get(0)).unwrap();
        assert!(
            canonical,
            "durable intents must match the decrypted HTTP payload and received echo"
        );
        drop(db);
        assert_eq!(
            f.state.http_sends.load(Ordering::SeqCst),
            2,
            "each explicit send dispatched exactly once"
        );
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn failed_atomic_receive_pauses_transport_keeps_cursor_and_local_history_available() {
    let f = Fixture::new(Reply::ConflictingReceive);
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
    let mut received = 0;
    let mut storage = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Received { .. } => received += 1,
                LiveEvent::Diagnostic { diagnostic, .. } => {
                    if diagnostic.category == LiveCategory::Storage {
                        storage = true;
                        assert_eq!(diagnostic.stage, LiveStage::Storage);
                    }
                }
                LiveEvent::Connection {
                    state: LiveConnection::Offline,
                    ..
                } if storage => break,
                LiveEvent::Fault { error, .. } => {
                    panic!("storage failure must not stop actor {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(received, 1, "failed batch cannot publish a changed row");
    commands
        .send(LiveCommand::Load {
            generation: 2,
            thread: "d:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Page {
                    generation: 2,
                    messages,
                    ..
                } => {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].text, "personal incoming");
                    break;
                }
                LiveEvent::Fault { error, .. } => panic!("local history actor closed {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let db = rusqlite::Connection::open_with_flags(
        f.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let cursor: String = db
        .query_row(
            "SELECT last_id FROM receive_cursors WHERE account_id='11' AND queue='510_0'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cursor, "100", "failed atomic batch must not advance cursor");
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        f.state.upgrades.load(Ordering::SeqCst),
        1,
        "storage failure requires explicit retry"
    );
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn sqlite_failure_log_is_structural_and_visible_without_debug_and_never_contains_trigger_text() {
    let root = std::env::temp_dir().join(crate::domain::local_message_id().unwrap());
    fs::create_dir(&root).unwrap();
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE incoming(body TEXT); CREATE TRIGGER reject_incoming BEFORE INSERT ON incoming BEGIN SELECT RAISE(ABORT, 'private_sql https://private.invalid/?cookie=private_cookie private_body /private/profile/path'); END;").unwrap();
    let raw = db
        .execute("INSERT INTO incoming VALUES ('private_body')", [])
        .unwrap_err();
    assert!(
        raw.to_string().contains("private_cookie"),
        "test must carry an actual SQLite trigger failure payload"
    );
    let error = storage_result::<()>(StorageOperation::ReceiveBatch, Err(raw.into())).unwrap_err();
    let logger = crate::diagnostics::Logger::isolated_for_test(&root, false).unwrap();
    let path = logger.path().to_owned();
    logger.record(storage_failure_event(
        &error,
        StorageOperation::ReceiveBatch,
    ));
    drop(logger);
    let log = fs::read_to_string(&path).unwrap();
    for private in [
        "private_sql",
        "private.invalid",
        "private_cookie",
        "private_body",
        "/private/profile/path",
        "INSERT INTO",
        "reject_incoming",
    ] {
        assert!(
            !log.contains(private),
            "storage diagnostics must not contain error payloads"
        );
    }
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let event = records
        .iter()
        .find(|event| event["storage_operation"] == "receive_batch")
        .expect("failure recorded even when verbose false");
    assert_eq!(event["storage_failure"], "sqlite");
    assert_eq!(
        event["sqlite_code"], 1811,
        "extended SQLITE_CONSTRAINT_TRIGGER code"
    );
    assert_eq!(event["kind"], "failure");
    assert_eq!(event["stage"], "storage");
    let detail = diagnostic(&error, LiveStage::Synchronization);
    assert_eq!(detail.stage, LiveStage::Storage);
    assert_eq!(detail.category, LiveCategory::Storage);
    let typed = storage_result::<()>(
        StorageOperation::ReceiveBatch,
        Err(crate::store::FailureReason::ConflictingMessage.into()),
    )
    .unwrap_err();
    let event = serde_json::to_value(storage_failure_event(
        &typed,
        StorageOperation::ReceiveBatch,
    ))
    .unwrap();
    assert_eq!(event["storage_failure"], "conflicting_message");
    assert!(event.get("sqlite_code").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn hidden_policy_real_tls_search_pin_cancel_scope_and_remote_revoke() {
    let f = Fixture::new(Reply::Privacy);
    let session = f.session();
    let transports = f.transports();
    let db = Database::open(f.profile.clone(), "11".into())
        .await
        .unwrap();
    db.call(|s| {
        for (thread, id) in [("d:22", "1"), ("g:22", "2"), ("d:44", "3")] {
            s.insert_batch(&[crate::domain::Message {
                account_id: "11".into(),
                thread_id: thread.into(),
                id: id.into(),
                sender: "22".into(),
                text: "synthetic private body".into(),
                sticker: None,
                media: None,
                sent_at_ms: 1,
            }])?;
        }
        s.set_title("11", "d:22", "Nguyễn An")?;
        s.begin_live_privacy("11")
    })
    .await
    .unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let output = Events {
        account: "11".into(),
        tx,
        repaint: Arc::new(|| {}),
        privacy: std::sync::RwLock::new(privacy::Projection {
            status: LivePrivacyStatus::Checking,
            ..Default::default()
        }),
    };
    let mut policy = privacy::Privacy::new();
    assert!(db
        .call(|s| s.conversations("11", 50))
        .await
        .unwrap()
        .is_empty());
    policy.start(session.clone(), &transports);
    let result = tokio::time::timeout(Duration::from_secs(3), policy.result())
        .await
        .unwrap();
    policy.completed(result, &db, &output).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::PrivacyChanged {
            status: LivePrivacyStatus::Ready,
            ..
        })
    ));
    let Some(LiveEvent::Ready { conversations, .. }) = rx.recv().await else {
        panic!("visible projection")
    };
    assert_eq!(
        conversations
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["d:44"]
    );
    policy
        .command(
            LiveCommand::SearchHidden {
                generation: 10,
                query: "nguyen an".into(),
            },
            &db,
            &output,
            &session,
            &transports,
        )
        .await
        .unwrap();
    let Some(LiveEvent::HiddenMatches { candidates, .. }) = rx.recv().await else {
        panic!("candidate search")
    };
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].thread_id, "d:22");
    let rev = output.privacy.read().unwrap().revision;
    policy
        .command(
            LiveCommand::UnlockHidden {
                generation: 11,
                revision: rev,
                thread: "d:22".into(),
                pin: HiddenPin::new("0000").unwrap(),
            },
            &db,
            &output,
            &session,
            &transports,
        )
        .await
        .unwrap();
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::HiddenUnlockResult {
            outcome: HiddenUnlockOutcome::IncorrectPin,
            ..
        })
    ));
    assert!(db
        .call(|s| s.page("11", "d:22", None, 50))
        .await
        .unwrap()
        .is_empty());
    // Cancel is delivered before the fresh request completes: it must invalidate the pending secret/authorization.
    f.state.privacy_hold.store(true, Ordering::SeqCst);
    policy
        .command(
            LiveCommand::UnlockHidden {
                generation: 12,
                revision: rev,
                thread: "d:22".into(),
                pin: HiddenPin::new("1234").unwrap(),
            },
            &db,
            &output,
            &session,
            &transports,
        )
        .await
        .unwrap();
    policy
        .command(
            LiveCommand::RelockHidden,
            &db,
            &output,
            &session,
            &transports,
        )
        .await
        .unwrap();
    f.state.privacy_hold.store(false, Ordering::SeqCst);
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert!(rx.try_recv().is_err());
    assert!(output.privacy.read().unwrap().unlocked.is_none());
    policy
        .command(
            LiveCommand::UnlockHidden {
                generation: 13,
                revision: rev,
                thread: "d:22".into(),
                pin: HiddenPin::new("1234").unwrap(),
            },
            &db,
            &output,
            &session,
            &transports,
        )
        .await
        .unwrap();
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert!(
        matches!(rx.recv().await,Some(LiveEvent::PrivacyChanged{unlocked_thread:Some(ref id),..}) if id=="d:22")
    );
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::HiddenUnlockResult {
            generation: 13,
            outcome: HiddenUnlockOutcome::Unlocked,
            ..
        })
    ));
    assert!(
        matches!(rx.recv().await,Some(LiveEvent::Opened{generation:13,conversation,..}) if conversation.id=="d:22")
    );
    assert_eq!(
        db.call(|s| s.page("11", "d:22", None, 50))
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.call(|s| s.page("11", "g:22", None, 50))
            .await
            .unwrap()
            .is_empty(),
        "same remote ID group is not globally unlocked"
    );
    // An unchanged background check retains the one open lease and emits no reset/duplicate projection.
    let unchanged_revision = output.privacy.read().unwrap().revision;
    policy.start(session.clone(), &transports);
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert_eq!(output.privacy.read().unwrap().revision, unchanged_revision);
    assert_eq!(
        output.privacy.read().unwrap().unlocked.as_deref(),
        Some("d:22")
    );
    assert_eq!(
        db.call(|s| s.page("11", "d:22", None, 50))
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(rx.try_recv().is_err());
    // A changed remote PIN revokes the open lease; messages and cursor remain untouched.
    f.state.privacy_policy.store(1, Ordering::SeqCst);
    policy.start(session.clone(), &transports);
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::PrivacyChanged {
            unlocked_thread: None,
            ..
        })
    ));
    let _ = rx.recv().await;
    assert!(db
        .call(|s| s.page("11", "d:22", None, 50))
        .await
        .unwrap()
        .is_empty());
    f.state.privacy_policy.store(2, Ordering::SeqCst);
    policy.start(session.clone(), &transports);
    let result = policy.result().await;
    policy.completed(result, &db, &output).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LiveEvent::PrivacyChanged {
            status: LivePrivacyStatus::Failed,
            ..
        })
    ));
    assert!(
        db.call(|s| s.page("11", "d:44", None, 50))
            .await
            .unwrap()
            .is_empty(),
        "failed policy cannot expose even a cached previously-visible conversation"
    );
    assert!(f.state.privacy_requests.load(Ordering::SeqCst) >= 6);
}

// Exercise source flag compatibility through the real socket, command dispatcher,
// durable actor and HTTP send path, rather than testing the parser alone.
async fn eligibility_send_case(mode: usize, accepted: bool) {
    let f = Fixture::new(Reply::Runtime);
    f.state.eligibility_mode.store(mode, Ordering::SeqCst);
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
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut received = 0;
        while received < 2 {
            match events.recv().await.unwrap() {
                LiveEvent::Received { .. } => received += 1,
                LiveEvent::Fault { error, .. } => panic!("synthetic startup failed: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    commands
        .send(LiveCommand::Send {
            generation: 101,
            thread: "d:22".into(),
            text: "Xin chào Linux".into(),
            request_id: 91,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Sent {
                    request_id: 91,
                    message,
                    ..
                } => {
                    assert!(accepted, "mode {mode} must not send plaintext");
                    assert_eq!(message.text, "Xin chào Linux");
                    break;
                }
                LiveEvent::SendFailed {
                    request_id: 91,
                    error,
                    unknown_delivery,
                    server_accepted,
                    ..
                } => {
                    assert!(
                        !accepted,
                        "mode {mode} should be source-compatible: {error}"
                    );
                    assert!(!unknown_delivery && !server_accepted);
                    assert!(error.to_lowercase().contains("bản nháp"));
                    if mode == 30 {
                        assert!(error.contains("sự kiện mã hóa"));
                    }
                    if (10..=14).contains(&mode) {
                        assert!(error.contains("định dạng"));
                    }
                    if (20..=23).contains(&mode) {
                        assert!(error.contains("đã bật mã hóa"));
                    }
                    break;
                }
                LiveEvent::Fault { error, .. } => {
                    panic!("eligibility must not stop local actor: {error}")
                }
                _ => {}
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
            .filter(|(path, _)| path.split('?').next() == Some("/api/message/sms"))
            .count(),
        usize::from(accepted),
        "mode {mode}"
    );
    let conn = rusqlite::Connection::open_with_flags(
        f.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM outgoing", [], |row| row
            .get::<_, usize>(0))
            .unwrap(),
        usize::from(accepted),
        "blocked eligibility must not prepare an outgoing row"
    );
    if mode == 30 {
        assert!(
            !f.state
                .observed_commands
                .lock()
                .unwrap()
                .iter()
                .any(|cmd| [532, 545, 557, 558].contains(cmd)),
            "intervening encrypted commands must not be acknowledged"
        );
    }
    drop(conn);
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn source_terminal_pagination_flags_allow_real_plaintext_send() {
    for mode in [1, 2, 3] {
        eligibility_send_case(mode, true).await;
    }
}

#[tokio::test]
async fn incomplete_or_encrypted_capability_never_sends_or_prepares_outbox() {
    for mode in [10, 11, 12, 13, 14, 20, 21, 22, 23] {
        eligibility_send_case(mode, false).await;
    }
}

#[tokio::test]
async fn encrypted_state_change_during_verification_keeps_draft_without_send_or_ack() {
    eligibility_send_case(30, false).await;
}
