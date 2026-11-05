//! Real TLS, independently encrypted synthetic responses and the actual SQLite store.
use super::*;
use crate::{
    profile::Profile,
    protocol::{bootstrap::ServerConfig, http::tls_tests::material, session::synthetic_session},
    store::MessageStore,
};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConnection, StreamOwned,
};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};
use url::Url;
fn oracle(mode: &str, input: &str) -> Vec<u8> {
    let mut child = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/directory/session_wire.cjs"
        ))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "independent synthetic crypto fixture failed"
    );
    result.stdout
}
#[derive(Clone, Copy)]
enum Scenario {
    Normal,
    LargeMetadata,
    PartialMetadata,
    UnsafeRedirect,
    Oversize,
    Stall,
    Preview(u8),
}
#[derive(Default)]
struct State {
    requests: Mutex<Vec<(String, String, String)>>,
    connected: AtomicBool,
}
struct Fixture {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    state: Arc<State>,
    profile: Arc<Profile>,
    scratch: PathBuf,
}
impl Fixture {
    fn new(scenario: Scenario) -> Self {
        let cert = material();
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.leaf.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key.clone())),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0")
            .expect("directory TLS fixture requires loopback socket permission");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let state = Arc::new(State::default());
        let data = state.clone();
        let tls = Arc::new(tls);
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let tls = tls.clone();
                        let data = data.clone();
                        workers.push(std::thread::spawn(move || {
                            serve(socket, tls, data, scenario)
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::park_timeout(Duration::from_millis(2))
                    }
                    Err(e) => panic!("local directory accept: {e}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        let scratch = std::env::temp_dir().join(format!(
            "zalo-directory-{}",
            crate::domain::local_message_id().unwrap()
        ));
        fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(Profile::at_root(&scratch, "fixture").unwrap());
        Self {
            addr,
            stop,
            thread: Some(thread),
            state,
            profile,
            scratch,
        }
    }
    fn session(&self) -> VerifiedSession {
        synthetic_session(self.profile.clone(),ServerConfig::parse(&json!({"zpw_service_map_v3":{"profile":["https://wpa.zaloapp.com/"],"group":["https://wpa.zaloapp.com/"],"conversation":["https://wpa.zaloapp.com/"]}})).unwrap())
    }
    fn fetcher(&self) -> DirectoryFetcher {
        DirectoryFetcher::with_client(
            Client::builder()
                .no_proxy()
                .https_only(true)
                .http1_only()
                .redirect(reqwest::redirect::Policy::none())
                .tls_built_in_root_certs(false)
                .add_root_certificate(reqwest::Certificate::from_der(&material().root).unwrap())
                .resolve("wpa.zaloapp.com", self.addr)
                .timeout(Duration::from_secs(3))
                .user_agent("ZaloPC-linux-24v681")
                .build()
                .unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.scratch);
    }
}
fn serve(
    mut socket: TcpStream,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<State>,
    scenario: Scenario,
) {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut conn = ServerConnection::new(tls).unwrap();
    while conn.is_handshaking() {
        if conn.complete_io(&mut socket).is_err() {
            return;
        }
    }
    let mut stream = StreamOwned::new(conn, socket);
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read_exact(&mut byte).is_err() {
            return;
        }
        head.push(byte[0]);
        assert!(head.len() < 32 * 1024);
    }
    let head = String::from_utf8(head).unwrap();
    let target = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap();
    let url = Url::parse(&format!("https://wpa.zaloapp.com{target}")).unwrap();
    let cookie = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default();
    let ua = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default();
    let ciphertext = url
        .query_pairs()
        .find(|(key, _)| key == "params")
        .unwrap()
        .1
        .into_owned();
    let parameters = String::from_utf8(oracle("decrypt", &ciphertext)).unwrap();
    state
        .requests
        .lock()
        .unwrap()
        .push((url.path().to_owned(), parameters.clone(), cookie));
    state.connected.store(true, Ordering::Release);
    assert_eq!(ua, "ZaloPC-linux-24v681");
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "zpw_ver")
            .unwrap()
            .1,
        "681"
    );
    if matches!(scenario, Scenario::Stall) {
        let _ = stream.read(&mut byte);
        return;
    }
    if matches!(scenario, Scenario::UnsafeRedirect) {
        let _=stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://attacker.invalid/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    if matches!(scenario, Scenario::Oversize) {
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_BODY + 1
            )
            .as_bytes(),
        );
        return;
    }
    let params: Value = serde_json::from_str(&parameters).unwrap();
    assert_eq!(params["imei"], "native-fixture");
    let data = match url.path() {
        "/api/social/friend/getfriends" => {
            assert_eq!(params["page"], 1);
            assert_eq!(params["count"], 20000);
            assert_eq!(
                url.query_pairs().find(|(key, _)| key == "init").unwrap().1,
                "1"
            );
            if matches!(scenario, Scenario::LargeMetadata) {
                Value::Array((1000..6000).map(|id|json!({"userId":id.to_string(),"displayName":format!("{} {id}","X".repeat(480)),"isFr":1})).collect())
            } else if matches!(scenario, Scenario::PartialMetadata) {
                json!([{"userId":"12","displayName":"Đặng Lan","isFr":1},{"userId":"13","displayName":"","isBlocked":1}])
            } else {
                json!([{"userId":"12","displayName":"Đặng Lan","isFr":1,"isBlocked":0,"avatar":"https://avatar.talk.zdn.vn/synthetic"}])
            }
        }
        "/api/group/getlg/v3" => {
            assert_eq!(params["count"], 100);
            if params["page"] == 1 {
                json!({"groups":[{"groupId":"77","name":"Thiết kế","totalMember":4}],"hasMore":1})
            } else {
                assert_eq!(params["page"], 2);
                json!({"groups":[{"groupId":"78","name":"Dự án"}],"hasMore":0})
            }
        }
        "/api/preloadconvers/get-last-msgs" => {
            assert_eq!(params["threadIdLocalMsgId"], "{}");
            if let Scenario::Preview(mode) = scenario {
                preview_data(mode)
            } else {
                json!({"msgs":[{"msgId":"501","uidFrom":"12","idTo":"11","msgType":"webchat","content":"Xin chào","ts":"1700000000123"}],"groupMsgs":[{"msgId":"502","uidFrom":"12","idTo":"77","msgType":"webchat","content":"Lịch họp","ts":"1700000000124"}],"eeMsgs":[{"synthetic_encrypted_pending":true}]})
            }
        }
        _ => panic!("unexpected synthetic directory route"),
    };
    let body = if url.path() == "/api/preloadconvers/get-last-msgs" {
        match scenario {
            Scenario::Preview(10) => b"{".to_vec(),
            Scenario::Preview(11) => br#"{"error_code":0,"data":{}}"#.to_vec(),
            Scenario::Preview(12) => oracle("encrypt-plaintext", "not-json"),
            Scenario::Preview(13) => oracle(
                "encrypt-plaintext",
                r#"{"error_code":"bad-type","data":{}}"#,
            ),
            _ => oracle("encrypt", &data.to_string()),
        }
    } else {
        oracle("encrypt", &data.to_string())
    };
    let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn preview_data(mode: u8) -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/messaging/media.json")).unwrap();
    let content = |name: &str| {
        fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap()["content"]
            .clone()
    };
    let row = |id: &str, from: &str, to: &str, kind: &str, content: Value| {
        json!({
            "msgId":id,"uidFrom":from,"idTo":to,"msgType":kind,"content":content,"ts":"1700000000123"
        })
    };
    match mode {
        1 => json!({}),
        2 => json!({"clearUnreads":[{"idTo":"12","lastMsgId":"900"}],"lastActionId":"900"}),
        3 => json!({"msgs":null,"groupMsgs":null,"pageMsgs":null,"eeMsgs":null}),
        4 => {
            let mut rows: Vec<_> = (1000..1201)
                .map(|id| {
                    row(
                        &id.to_string(),
                        "12",
                        "99",
                        "webchat",
                        json!("valid-prefix"),
                    )
                })
                .collect();
            rows.last_mut().unwrap()["idTo"] = json!({"malformed":true});
            json!({"msgs":rows})
        }
        5 => {
            json!({"msgs":[row("501","12","99","webchat",json!("first")),row("501","12","99","webchat",json!("conflict"))]})
        }
        6 => json!({"msgs":[row("501","12","99","webchat",json!("prefix"))],"pageMsgs":{}}),
        7 => {
            let mut encrypted = row("501", "12", "99", "webchat", Value::Null);
            encrypted["isE2EE"] = json!(true);
            json!({"msgs":[encrypted],"eeMsgs":[{"opaque":"pending"}]})
        }
        8 => Value::Null,
        9 => {
            json!({"msgs":[row("501","12","99","webchat",json!("prefix")),row("503","12","99","chat.photo",Value::Null)]})
        }
        _ => json!({
            "msgs":[row("501","12","99","webchat",json!("peer-addressed text")),
                row("502","0","12","webchat",json!("own sentinel echo")),
                row("503","12","99","chat.photo",content("photo_object")),
                row("504","12","99","chat.webcontent",content("webcontent_string")),
                row("601","13","99","webchat",json!("direct collision"))],
            "groupMsgs":[row("601","13","77","webchat",json!("group collision"))],
            "pageMsgs":[row("505","14","99","chat.recommended",json!({"action":"unknown.synthetic"}))],"eeMsgs":null
        }),
    }
}
struct Sink {
    store: MessageStore,
    commits: usize,
}
#[tokio::test]
async fn authenticated_preview_source_shapes_and_rejections_preserve_local_state_without_queue_ack()
{
    use crate::protocol::{directory::PreviewField, messaging::FrameReason};
    for mode in 0..=13 {
        let fixture = Fixture::new(Scenario::Preview(mode));
        let session = fixture.session();
        if mode == 0 {
            assert!(
                crate::protocol::directory::parse_previews("11", &preview_data(mode)).is_err(),
                "untrusted generic parsing must retain recipient membership checks"
            );
        }
        let mut sink = Sink {
            store: MessageStore::open(&fixture.profile.paths.data_dir.join("messages.sqlite"))
                .unwrap(),
            commits: 0,
        };
        let cached = crate::domain::Message {
            account_id: "11".into(),
            thread_id: "d:12".into(),
            id: "499".into(),
            sender: "12".into(),
            text: "retained cached body".into(),
            sticker: None,
            media: None,
            sent_at_ms: 100,
        };
        sink.store
            .receive_batch("11", &[(cached.clone(), None)], Some(("511_1", "900")))
            .unwrap();
        sink.store
            .prepare_outgoing("11", "d:12", "unresolved outgoing text", 101)
            .unwrap();
        let observer = rusqlite::Connection::open_with_flags(
            fixture.profile.paths.data_dir.join("messages.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let pending = |connection: &rusqlite::Connection| {
            connection
                .query_row("SELECT body,state,server_id FROM outgoing", [], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .unwrap()
        };
        let old_pending = pending(&observer);
        let (_stop, cancel) = watch::channel(false);
        let report = fixture
            .fetcher()
            .synchronize(&session, &mut sink, cancel)
            .await
            .unwrap();
        assert!(
            report.completed_friends && report.completed_groups,
            "unrelated discovery stages remain usable: {report:?}"
        );
        assert_eq!(sink.store.receive_cursor("11", "511_1").unwrap(), "900");
        assert_eq!(
            pending(&observer),
            old_pending,
            "previews never acknowledge or rewrite an outgoing intent"
        );
        assert!(sink
            .store
            .page("11", "d:12", None, 50)
            .unwrap()
            .contains(&cached));
        assert!(sink.store.page("12", "d:12", None, 50).unwrap().is_empty());
        if mode == 0 {
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(report.completed_previews);
            assert_eq!(report.preview_messages, 7);
            let direct = sink.store.page("11", "d:12", None, 50).unwrap();
            assert_eq!(direct.len(), 5);
            assert_eq!(direct.iter().find(|m| m.id == "502").unwrap().sender, "11");
            for id in ["503", "504"] {
                assert!(direct.iter().find(|m| m.id == id).unwrap().media.is_some());
                assert!(sink
                    .store
                    .message_media("11", "d:12", id)
                    .unwrap()
                    .is_some());
            }
            assert_eq!(
                sink.store.page("11", "d:13", None, 10).unwrap()[0].id,
                "601"
            );
            assert_eq!(
                sink.store.page("11", "g:77", None, 10).unwrap()[0].id,
                "601"
            );
            assert_eq!(
                sink.store.page("11", "d:14", None, 10).unwrap()[0].text,
                "[Tin nhắn chat.recommended chưa được hỗ trợ]"
            );
        } else if (1..=3).contains(&mode) || mode == 7 {
            assert!(report.failures.is_empty(), "{report:?}");
            assert_eq!(report.preview_messages, 0);
            assert_eq!(report.unsupported_encrypted, if mode == 7 { 2 } else { 0 });
            assert_eq!(report.completed_previews, mode != 7);
            assert_eq!(
                sink.store.page("11", "d:12", None, 50).unwrap(),
                vec![cached]
            );
        } else {
            assert_eq!(report.failures.len(), 1, "{report:?}");
            let failure = report.failures[0];
            assert_eq!(failure.stage, DirectoryStage::Previews);
            assert_eq!(failure.category, DirectoryCategory::Schema);
            let parser = failure
                .parser
                .expect("preview rejection must retain a closed boundary diagnostic");
            match mode {
                4 => {
                    assert_eq!(parser.field, PreviewField::Msgs);
                    assert_eq!(parser.reason, FrameReason::MessageRecipient);
                }
                5 => {
                    assert_eq!(parser.field, PreviewField::Msgs);
                    assert_eq!(parser.reason, FrameReason::MessageValidation);
                }
                6 => assert_eq!(parser.field, PreviewField::PageMsgs),
                8 => assert_eq!(parser.field, PreviewField::MessageCollections),
                9 => {
                    assert_eq!(parser.field, PreviewField::Msgs);
                    assert_eq!(parser.reason, FrameReason::MessageContent);
                }
                10 => assert_eq!(parser.field, PreviewField::ResponseJson),
                11 => assert_eq!(parser.field, PreviewField::EncryptedData),
                12 => assert_eq!(parser.field, PreviewField::DecryptedJson),
                13 => assert_eq!(parser.field, PreviewField::DecryptedEnvelope),
                _ => unreachable!(),
            }
            assert!(!report.completed_previews);
            assert_eq!(
                report.preview_messages, 0,
                "a malformed later chunk cannot persist a valid prefix"
            );
            assert_eq!(
                sink.store.page("11", "d:12", None, 50).unwrap(),
                vec![cached]
            );
            assert!(sink.store.page("11", "g:77", None, 10).unwrap().is_empty());
            assert_eq!(
                sink.commits, 3,
                "only friends and the two valid group pages may commit"
            );
        }
        assert!(fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _, _)| matches!(
                path.as_str(),
                "/api/social/friend/getfriends"
                    | "/api/group/getlg/v3"
                    | "/api/preloadconvers/get-last-msgs"
            )));
    }
}
impl DirectorySink for Sink {
    async fn apply(&mut self, batch: DirectoryBatch) -> Result<(), DirectoryFailure> {
        self.store
            .apply_directory_batch(&batch)
            .map_err(|_| failure(DirectoryStage::Storage, DirectoryCategory::Storage))?;
        self.commits += 1;
        Ok(())
    }
}
#[tokio::test]
async fn real_tls_discovery_commits_named_offline_cache_and_recent_messages_without_acknowledging_encrypted_queue(
) {
    let fixture = Fixture::new(Scenario::Normal);
    let session = fixture.session();
    let fetcher = fixture.fetcher();
    let (_stop, cancel) = watch::channel(false);
    let mut sink = Sink {
        store: MessageStore::in_memory().unwrap(),
        commits: 0,
    };
    let report = fetcher
        .synchronize(&session, &mut sink, cancel)
        .await
        .unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        (report.contacts, report.groups, report.preview_messages),
        (1, 2, 2)
    );
    assert!(report.completed_friends && report.completed_groups);
    assert!(!report.completed_previews);
    assert_eq!(report.unsupported_encrypted, 1);
    assert_eq!(
        sink.store
            .directory_page("11", false, "dang", None, 100)
            .unwrap()[0]
            .display_name,
        "Đặng Lan"
    );
    let conversations = sink.store.conversations("11", 100).unwrap();
    assert_eq!(conversations.len(), 2);
    assert!(conversations
        .iter()
        .any(|c| c.title == "Thiết kế" && c.last_text == "Lịch họp"));
    assert!(conversations
        .iter()
        .any(|c| c.title == "Đặng Lan" && c.last_text == "Xin chào"));
    assert_eq!(conversations[0].last_message_at_ms, 1700000000124);
    assert!(sink.store.conversations("22", 100).unwrap().is_empty());
    let repeat = fetcher
        .synchronize(&session, &mut sink, watch::channel(false).1)
        .await
        .unwrap();
    assert_eq!(repeat.preview_messages, 2);
    assert_eq!(sink.store.page("11", "d:12", None, 100).unwrap().len(), 1);
    let requests = fixture.state.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert!(requests
        .iter()
        .all(|(_, _, cookie)| cookie == "zpw_sek=synthetic"));
    assert!(requests.iter().all(|(path, _, _)| !path.contains("ack")));
}
#[tokio::test]
async fn redirects_and_oversize_responses_are_partial_sanitized_failures_not_cache_deletions() {
    for (scenario, category) in [
        (Scenario::UnsafeRedirect, DirectoryCategory::Http),
        (Scenario::Oversize, DirectoryCategory::Resource),
    ] {
        let fixture = Fixture::new(scenario);
        let mut sink = Sink {
            store: MessageStore::in_memory().unwrap(),
            commits: 0,
        };
        let report = fixture
            .fetcher()
            .synchronize(&fixture.session(), &mut sink, watch::channel(false).1)
            .await
            .unwrap();
        assert_eq!(report.failures.len(), 3);
        assert!(report.failures.iter().all(|f| f.category == category));
        assert_eq!(sink.commits, 0);
        for error in report.failures {
            let text = error.to_string();
            assert!(!text.contains("attacker") && !text.contains("synthetic"));
        }
    }
}
#[tokio::test]
async fn cancel_drops_inflight_https_and_never_commits_a_partial_stage() {
    let fixture = Fixture::new(Scenario::Stall);
    let (stop, cancel) = watch::channel(false);
    let mut sink = Sink {
        store: MessageStore::in_memory().unwrap(),
        commits: 0,
    };
    let fetcher = fixture.fetcher();
    let session = fixture.session();
    let error = {
        let flow = fetcher.synchronize(&session, &mut sink, cancel);
        tokio::pin!(flow);
        tokio::select! {result=&mut flow=>panic!("flow finished before cancellation: {}",result.is_ok()),_=async{while !fixture.state.connected.load(Ordering::Acquire){tokio::task::yield_now().await;}}=>{}}
        stop.send_replace(true);
        flow.await.err().unwrap()
    };
    assert_eq!(error.category, DirectoryCategory::Cancelled);
    assert_eq!(sink.commits, 0);
}

#[tokio::test]
async fn large_friend_metadata_uses_its_own_bound_without_enlarging_authentication_budget() {
    let fixture = Fixture::new(Scenario::LargeMetadata);
    let data = fixture
        .fetcher()
        .fetch(
            &fixture.session(),
            wire::friends_request("native-fixture").unwrap(),
            DirectoryStage::Friends,
        )
        .await
        .unwrap();
    assert_eq!(wire::parse_friends(&data).unwrap().len(), 5000);
    let plaintext = serde_json::to_vec(&data).unwrap();
    assert!(plaintext.len() > crate::protocol::qr::MAX_RESPONSE_BYTES);
    assert!(plaintext.len() < MAX_BODY);
    assert_eq!(crate::protocol::qr::MAX_RESPONSE_BYTES, 2 * 1024 * 1024);
}

#[tokio::test]
async fn unsupported_friend_presentation_preserves_cached_identity_and_reports_partial() {
    let fixture = Fixture::new(Scenario::PartialMetadata);
    let mut sink = Sink {
        store: MessageStore::in_memory().unwrap(),
        commits: 0,
    };
    sink.store
        .apply_directory_batch(&DirectoryBatch {
            message_info: Vec::new(),
            message_media: Vec::new(),
            account_id: "11".into(),
            entries: vec![DirectoryRecord {
                remote_id: "13".into(),
                is_group: false,
                display_name: "Retained cached contact".into(),
                avatar_url: None,
                is_friend: Some(true),
                is_blocked: Some(true),
                member_count: None,
            }],
            messages: vec![],
            observed_at_ms: 1,
        })
        .unwrap();
    let report = fixture
        .fetcher()
        .synchronize(&fixture.session(), &mut sink, watch::channel(false).1)
        .await
        .unwrap();
    assert_eq!(report.contacts, 1);
    assert_eq!(report.unsupported_contact_names, 1);
    assert!(!report.completed_friends);
    assert!(report.completed_groups);
    assert_eq!(report.failures[0].stage, DirectoryStage::Friends);
    assert_eq!(report.failures[0].category, DirectoryCategory::Schema);
    let retained = sink.store.directory_record("11", "d:13").unwrap().unwrap();
    assert_eq!(retained.display_name, "Retained cached contact");
    assert_eq!(retained.is_blocked, Some(true));
}
struct BlockedSink {
    entered: watch::Sender<bool>,
}
impl DirectorySink for BlockedSink {
    async fn apply(&mut self, _: DirectoryBatch) -> Result<(), DirectoryFailure> {
        self.entered.send_replace(true);
        std::future::pending().await
    }
}
#[tokio::test]
async fn bounded_sink_backpressure_cancels_without_fetching_later_pages() {
    let fixture = Fixture::new(Scenario::Normal);
    let (entered, mut observed) = watch::channel(false);
    let mut sink = BlockedSink { entered };
    let (stop, cancel) = watch::channel(false);
    let fetcher = fixture.fetcher();
    let session = fixture.session();
    let result = {
        let flow = fetcher.synchronize(&session, &mut sink, cancel);
        tokio::pin!(flow);
        tokio::select! {result=&mut flow=>panic!("blocked sink unexpectedly finished {}",result.is_ok()),_=observed.changed()=>{}}
        stop.send_replace(true);
        flow.await
    };
    assert_eq!(result.err().unwrap().category, DirectoryCategory::Cancelled);
    assert_eq!(fixture.state.requests.lock().unwrap().len(), 1);
}
struct FailedSink;
impl DirectorySink for FailedSink {
    async fn apply(&mut self, _: DirectoryBatch) -> Result<(), DirectoryFailure> {
        Err(failure(DirectoryStage::Storage, DirectoryCategory::Storage))
    }
}
#[tokio::test]
async fn storage_failure_stops_further_requests_and_reports_no_committed_rows() {
    let fixture = Fixture::new(Scenario::Normal);
    let report = fixture
        .fetcher()
        .synchronize(&fixture.session(), &mut FailedSink, watch::channel(false).1)
        .await
        .unwrap();
    assert_eq!(report.contacts, 0);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].category, DirectoryCategory::Storage);
    assert!(!report.completed_groups);
    assert_eq!(fixture.state.requests.lock().unwrap().len(), 1);
}
