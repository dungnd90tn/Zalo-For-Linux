use super::*;
use std::{collections::VecDeque, fs, time::Instant};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "zalo-auth-test-{}",
            crate::domain::local_message_id().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture_bytes(value: &Value, key: &str) -> Vec<u8> {
    let s = value[key].as_str().unwrap();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn saved() -> StoredSession {
    let linking: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/trust/primary_linking_proof.json"
    ))
    .unwrap();
    let list: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/trust/signed_primary_list.json"
    ))
    .unwrap();
    StoredSession {
        client_type: ClientType::PcLinux24,
        account_id: "11".into(),
        display_name: "Synthetic account".into(),
        imei: "native-fixture".into(),
        cookie: "synthetic-cookie".into(),
        refresh_token: None,
        storage_key: "synthetic-storage".into(),
        session_key: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".into(),
        proof: SessionProof::TrustedDevice(StoredTrust {
            master_id: linking["master_id"].as_str().unwrap().into(),
            primary_key: fixture_bytes(&linking, "primary_public_hex"),
            companion_public: fixture_bytes(&linking, "companion_public_hex"),
            companion_private: fixture_bytes(&linking, "companion_fixture_private_hex"),
            linking_secret: fixture_bytes(&linking, "fixture_linking_secret_hex"),
            signed_list: fixture_bytes(&list, "device_list_hex"),
            list_timestamp: 1_700_000_000_000,
            linking_timestamp: 1_700_000_000_000,
        }),
    }
}
fn config() -> bootstrap::ServerConfig {
    bootstrap::ServerConfig::parse(&serde_json::json!({"zpw_service_map_v3":{"chat":["https://chat-wpa.zaloapp.com/"]},"zpw_ws":["wss://ws1.zalo.me/"]})).unwrap()
}
fn repaint() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}
struct Replies {
    values: VecDeque<i64>,
    calls: usize,
}
impl qr::QrTransport for Replies {
    async fn get(&mut self, _: &RequestSpec) -> Result<HttpReply> {
        self.calls += 1;
        let code = self.values.pop_front().unwrap();
        Ok(HttpReply {
            status: 200,
            body: serde_json::to_vec(
                &serde_json::json!({"error_code":code,"data":{"fixture":true}}),
            )
            .unwrap(),
        })
    }
}
fn descriptor(retries: u64) -> QrDescriptor {
    QrDescriptor::parse(&serde_json::json!({"token_id":"synthetic-token","chk_wait_scan":"https://wpa.zaloapp.com/scan?token=synthetic","chk_wait_cfirm":"https://wpa.zaloapp.com/confirm?token=synthetic","qr_timeout":1,"qr_retry":retries}),false).unwrap()
}
#[tokio::test(start_paused = true)]
async fn long_poll_408_is_retried_until_real_scan_result() {
    let mut http = Replies {
        values: VecDeque::from([408, 408, 0]),
        calls: 0,
    };
    let (code, _) = poll(&mut http, &descriptor(3), false).await.unwrap();
    assert_eq!(code, 0);
    assert_eq!(http.calls, 3);
}
#[tokio::test(start_paused = true)]
async fn exhausted_long_poll_408_expires_with_finite_budget() {
    let mut http = Replies {
        values: VecDeque::from([408, 408]),
        calls: 0,
    };
    assert_eq!(
        poll(&mut http, &descriptor(2), false).await.err(),
        Some(LoginFailure::Expired)
    );
    assert_eq!(http.calls, 2);
}

#[tokio::test]
async fn saturated_critical_events_wait_and_cancel_without_losing_current_terminal() {
    let (tx, mut rx) = mpsc::channel(1);
    let epoch = AtomicU64::new(1);
    let (change, notice) = watch::channel(1);
    let repaint = repaint();
    tx.send(LoginEvent::Progress {
        generation: 1,
        stage: LoginStage::CreatingQr,
    })
    .await
    .unwrap();
    let send = deliver(
        &tx,
        LoginEvent::Failed {
            generation: 1,
            error: LoginFailure::Trust,
        },
        1,
        &epoch,
        &notice,
        &repaint,
    );
    tokio::pin!(send);
    tokio::select! {biased;_= &mut send=>panic!("terminal event must backpressure"),_=tokio::task::yield_now()=>{}}
    rx.recv().await.unwrap();
    send.await;
    assert!(matches!(
        rx.recv().await,
        Some(LoginEvent::Failed {
            generation: 1,
            error: LoginFailure::Trust
        })
    ));
    tx.send(LoginEvent::Progress {
        generation: 1,
        stage: LoginStage::CreatingQr,
    })
    .await
    .unwrap();
    let stale = deliver(
        &tx,
        LoginEvent::Failed {
            generation: 1,
            error: LoginFailure::Trust,
        },
        1,
        &epoch,
        &notice,
        &repaint,
    );
    tokio::pin!(stale);
    tokio::select! {biased;_= &mut stale=>panic!("terminal event must backpressure"),_=tokio::task::yield_now()=>{}}
    epoch.store(2, Ordering::Release);
    change.send_replace(2);
    stale.await;
    rx.recv().await.unwrap();
    assert!(rx.try_recv().is_err());
}
#[tokio::test]
async fn ready_capacity_is_reserved_before_atomic_commit() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "account").unwrap());
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(LoginEvent::Progress {
        generation: 1,
        stage: LoginStage::CreatingQr,
    })
    .await
    .unwrap();
    let epoch = AtomicU64::new(1);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 1,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    let publish = publish_session(profile.clone(), None, saved(), config(), &context);
    tokio::pin!(publish);
    tokio::select! {biased;_= &mut publish=>panic!("must reserve event capacity first"),_=tokio::task::yield_now()=>{}}
    assert!(!profile
        .paths
        .data_dir
        .join(session::AUTH_STATE_FILE)
        .exists());
    rx.recv().await.unwrap();
    publish.await.unwrap();
    let event = rx.recv().await.unwrap();
    assert!(matches!(event, LoginEvent::Ready { generation: 1, .. }));
    let record = AuthRecord::load(&profile).unwrap().unwrap();
    assert_eq!(record.owner_uid, "11");
    assert!(record.active.is_some());
    assert_eq!(committed.load(Ordering::Acquire), 1);
    assert!(profile.check_account("12").is_err());
    logout_record(&profile).unwrap();
    let record = AuthRecord::load(&profile).unwrap().unwrap();
    assert!(record.active.is_none());
    assert_eq!(record.owner_uid, "11");
    assert!(!record.replay.is_empty());
}
#[tokio::test]
async fn cancelled_before_commit_keeps_previous_record_byte_for_byte() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "account").unwrap());
    let mut old = AuthRecord::empty("11");
    old.active = Some(saved());
    old.write(&profile).unwrap();
    let path = profile.paths.data_dir.join(session::AUTH_STATE_FILE);
    let before = fs::read(&path).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(LoginEvent::Progress {
        generation: 3,
        stage: LoginStage::CreatingQr,
    })
    .await
    .unwrap();
    let epoch = AtomicU64::new(3);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 3,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    let record = AuthRecord::load(&profile).unwrap();
    let publish = publish_session(profile.clone(), record, saved(), config(), &context);
    tokio::pin!(publish);
    tokio::select! {biased;_= &mut publish=>panic!("must reserve event capacity first"),_=tokio::task::yield_now()=>{}}
    epoch.store(4, Ordering::Release);
    rx.recv().await.unwrap();
    assert_eq!(publish.await.err(), Some(LoginFailure::Cancelled));
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(rx.try_recv().is_err());
    assert_eq!(committed.load(Ordering::Acquire), 0);
    assert!(fs::read_dir(&profile.paths.data_dir)
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".prepared-")));
}
#[tokio::test]
async fn storage_failure_does_not_publish_or_bind_new_account() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "account").unwrap());
    fs::create_dir(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let epoch = AtomicU64::new(1);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 1,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    assert!(
        publish_session(profile.clone(), None, saved(), config(), &context)
            .await
            .is_err()
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(committed.load(Ordering::Acquire), 0);
    assert!(!profile.paths.data_dir.join(".account-owner.json").exists());
}
#[test]
fn public_cancel_remains_nonblocking_with_a_full_command_queue() {
    let (controls, _rx) = mpsc::channel(1);
    controls.try_send(Control::Begin(1)).unwrap();
    let (_tx, events) = mpsc::channel(1);
    let (changed, _notice) = watch::channel(1);
    let (cancelled, mut cancellations) = watch::channel(Cancellation {
        generation: 0,
        attempt: 0,
    });
    let controller = LoginController {
        controls,
        epoch: Arc::new(AtomicU64::new(1)),
        changed,
        cancelled,
        attempt: AtomicU64::new(1),
        events,
    };
    let started = Instant::now();
    let generation = controller.cancel();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(generation, 2);
    assert_eq!(cancellations.borrow_and_update().generation, 2);
}
#[test]
fn controller_drop_releases_private_profile_even_without_event_consumer() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "account").unwrap());
    let weak = Arc::downgrade(&profile);
    let controller = LoginController::new(profile.clone(), None, repaint()).unwrap();
    drop(profile);
    drop(controller);
    let deadline = Instant::now() + Duration::from_secs(2);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(weak.upgrade().is_none());
    Profile::at_root(&scratch.0, "account").unwrap();
}
#[test]
fn qr_pixel_and_png_budgets_reject_unbounded_or_wrong_assets() {
    let image = qr_pixels("synthetic-test-content").unwrap();
    assert!(image.width <= 512);
    assert_eq!(
        image.rgba.len(),
        image.width as usize * image.height as usize * 4
    );
    assert!(qr_pixels(&"x".repeat(2049)).is_err());
    assert!(
        check_optional_png(&serde_json::json!({"base64_qr":STANDARD.encode(b"not png")})).is_err()
    );
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1025, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; 1025 * 4])
            .unwrap();
    }
    assert!(check_optional_png(&serde_json::json!({"base64_qr":STANDARD.encode(bytes)})).is_err());
}

#[tokio::test]
async fn real_trust_worker_completes_primary_linking_and_independently_signed_confirmation_list() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/trust/primary_linking_proof.json"
    ))
    .unwrap();
    let worker = TrustWorker::start(
        None,
        Some(TrustKeys {
            public: fixture_bytes(&fixture, "companion_public_hex"),
            private: fixture_bytes(&fixture, "companion_fixture_private_hex"),
            secret: fixture_bytes(&fixture, "fixture_linking_secret_hex"),
        }),
    )
    .await
    .unwrap();
    let timestamp = fixture["timestamp_ms"].as_u64().unwrap();
    let content = worker
        .qr("synthetic-controller-token".into(), timestamp)
        .await
        .unwrap();
    assert!(!content.is_empty());
    assert!(qr_pixels(&content).is_ok());
    let proof = worker
        .proof(
            fixture_bytes(&fixture, "primary_linking_data_hex"),
            fixture_bytes(&fixture, "linking_hmac_hex"),
        )
        .await
        .unwrap();
    assert_eq!(proof.companion_id, 123);
    assert_eq!(proof.link_id, 42);
    assert_eq!(proof.linking_timestamp_ms, timestamp);
    let signature_hex: String = proof
        .device_signature
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut child = Command::new("python3")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/trust/generate_linking_proof.py"),
        )
        .arg("--signed-list")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({"device_signature_hex":signature_hex}))
                .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "independent synthetic primary signer failed"
    );
    let signed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let list = signed_list(
        &signed["devices"],
        &proof,
        &worker.public,
        timestamp,
        timestamp,
        None,
    )
    .unwrap();
    assert_eq!(list.encoded, fixture_bytes(&signed, "device_list_hex"));
    worker
        .verify(list.encoded.clone(), list.primary.clone(), timestamp)
        .await
        .unwrap();
    let exported = worker.keys().await.unwrap();
    assert_eq!(exported.public, worker.public);
    assert!(signed_list(
        &signed["devices"],
        &proof,
        &worker.public,
        timestamp,
        timestamp,
        Some(timestamp)
    )
    .is_err());
    assert!(signed_list(
        &signed["devices"],
        &proof,
        &worker.public,
        timestamp + CLOCK_SKEW_MS + 1,
        timestamp + CLOCK_SKEW_MS + 1,
        None
    )
    .is_err());
    let mut wrong_master = signed["devices"].clone();
    wrong_master["masterId"] = Value::String("different-master".into());
    assert!(signed_list(
        &wrong_master,
        &proof,
        &worker.public,
        timestamp,
        timestamp,
        None
    )
    .is_err());
    // Continue the actual trust result through strict TLS, independent Node AES,
    // bootstrap UID matching, durable ownership/session commit and Ready publication.
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "end-to-end").unwrap());
    let pending = PendingCookies::from_confirmation(
        &serde_json::json!({"uid":"11","dkey":"synthetic-storage","zpw_sek":"synthetic-cookie"}),
    )
    .unwrap();
    let (mut http, server) = bootstrap_tls_fixture("11");
    let info =
        bootstrap_authenticated(&mut http, "native-fixture", &pending, ClientType::PcLinux24)
            .await
            .unwrap();
    server.join().unwrap();
    let mut saved = saved();
    saved.proof.trusted_mut().signed_list = list.encoded.clone();
    saved.proof.trusted_mut().primary_key = list.primary.clone();
    saved.proof.trusted_mut().master_id = list.master.clone();
    saved.proof.trusted_mut().list_timestamp = list.timestamp;
    saved.proof.trusted_mut().companion_public = exported.public;
    saved.proof.trusted_mut().companion_private = exported.private;
    saved.proof.trusted_mut().linking_secret = exported.secret;
    saved.session_key = session::encode_key(&info.session_cipher);
    let (tx, mut rx) = mpsc::channel(1);
    let epoch = AtomicU64::new(1);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 1,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    publish_session(profile.clone(), None, saved, info.config, &context)
        .await
        .unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(LoginEvent::Ready { generation: 1, .. })
    ));
    let mut stored = AuthRecord::load(&profile).unwrap().unwrap();
    assert_eq!(stored.owner_uid, "11");
    assert_eq!(
        stored
            .active
            .as_ref()
            .unwrap()
            .proof
            .trusted_ref()
            .master_id,
        list.master
    );
    validate_saved_context(
        stored.active.as_ref().unwrap().proof.trusted_ref(),
        &stored,
        timestamp,
    )
    .unwrap();
    stored
        .active
        .as_mut()
        .unwrap()
        .proof
        .trusted_mut()
        .list_timestamp += 1;
    assert!(validate_saved_context(
        stored.active.as_ref().unwrap().proof.trusted_ref(),
        &stored,
        timestamp
    )
    .is_err());
    stored
        .active
        .as_mut()
        .unwrap()
        .proof
        .trusted_mut()
        .list_timestamp -= 1;
    stored
        .active
        .as_mut()
        .unwrap()
        .proof
        .trusted_mut()
        .master_id = "unbound-master".into();
    assert!(validate_saved_context(
        stored.active.as_ref().unwrap().proof.trusted_ref(),
        &stored,
        timestamp
    )
    .is_err());
    let (mut http, server) = bootstrap_tls_fixture("12");
    assert_eq!(
        bootstrap_authenticated(&mut http, "native-fixture", &pending, ClientType::PcLinux24)
            .await
            .err(),
        Some(LoginFailure::AccountMismatch)
    );
    server.join().unwrap();
    assert_eq!(AuthRecord::load(&profile).unwrap().unwrap().owner_uid, "11");
    let mut malformed = list.encoded;
    *malformed.last_mut().unwrap() ^= 1;
    assert!(worker
        .verify(malformed, list.primary, timestamp)
        .await
        .is_err());
    assert!(worker.keys().await.is_err());
}

#[test]
fn repeated_cancel_coalescing_invalidates_committed_attempt_but_preserves_prior_replacement() {
    let scratch = Scratch::new();
    let profile = Profile::at_root(&scratch.0, "account").unwrap();
    let mut record = AuthRecord::empty("11");
    record.active = Some(saved());
    record.write(&profile).unwrap();
    let (controls, _rx) = mpsc::channel(8);
    let (_tx, events) = mpsc::channel(1);
    let (changed, _notice) = watch::channel(1);
    let (cancelled, mut cancellations) = watch::channel(Cancellation {
        generation: 0,
        attempt: 0,
    });
    let controller = LoginController {
        controls,
        epoch: Arc::new(AtomicU64::new(1)),
        changed,
        cancelled,
        attempt: AtomicU64::new(1),
        events,
    };
    controller.cancel();
    controller.cancel();
    let last = AtomicU64::new(1);
    let cancel = *cancellations.borrow_and_update();
    assert_eq!(cancel.generation, 3);
    assert_eq!(cancel.attempt, 1);
    cancel_committed(&profile, &last, cancel).unwrap();
    assert!(AuthRecord::load(&profile)
        .unwrap()
        .unwrap()
        .active
        .is_none());
    let mut record = AuthRecord::load(&profile).unwrap().unwrap();
    record.active = Some(saved());
    record.write(&profile).unwrap();
    let before = fs::read(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap();
    let replacement = controller.begin().unwrap();
    controller.cancel();
    controller.cancel();
    let cancel = *cancellations.borrow_and_update();
    assert_eq!(cancel.attempt, replacement);
    cancel_committed(&profile, &last, cancel).unwrap();
    assert_eq!(
        fs::read(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap(),
        before
    );
    // Invalidating a pending replacement makes its CAS fail even when its late result arrives.
    assert!(controller
        .epoch
        .compare_exchange(
            replacement,
            replacement | COMMITTING,
            Ordering::AcqRel,
            Ordering::Acquire
        )
        .is_err());
    fs::remove_file(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap();
    fs::create_dir(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap();
    assert!(cancel_committed(
        &profile,
        &last,
        Cancellation {
            generation: 9,
            attempt: 1
        }
    )
    .is_err());
}

/// Explicit, bounded development probe: no scan/confirmation endpoint is ever requested.
#[tokio::test]
#[ignore = "requires explicit authorization and public network; never scans or logs QR material"]
async fn public_unauthenticated_startup_probe() {
    assert_eq!(
        std::env::var("ZALO_AUTH_PUBLIC_PROBE").as_deref(),
        Ok("authorized")
    );
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "public-probe").unwrap());
    let weak = Arc::downgrade(&profile);
    let mut controller = LoginController::new(profile.clone(), None, repaint()).unwrap();
    let generation = controller.begin().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            match controller.events.recv().await {
                Some(LoginEvent::Progress {
                    generation: g,
                    stage,
                }) if g == generation => println!("public_probe stage={stage:?}"),
                Some(LoginEvent::Qr {
                    generation: g,
                    pixels,
                }) if g == generation => {
                    assert!(pixels.width <= 1024 && pixels.height <= 1024);
                    assert_eq!(
                        pixels.rgba.len(),
                        pixels.width as usize * pixels.height as usize * 4
                    );
                    println!("public_probe stage=qr_generated");
                    let cancelled = controller.cancel();
                    loop {
                        match controller.events.recv().await {
                            Some(LoginEvent::SignedOut { generation: g }) if g == cancelled => {
                                break
                            }
                            Some(LoginEvent::Failed {
                                generation: g,
                                error,
                            }) if g == cancelled => return Err(error),
                            None => return Err(LoginFailure::Internal),
                            _ => {}
                        }
                    }
                    println!("public_probe result=cancelled_without_scan");
                    return Ok(());
                }
                Some(LoginEvent::Failed {
                    generation: g,
                    error,
                }) if g == generation => return Err(error),
                None => return Err(LoginFailure::Internal),
                _ => {}
            }
        }
    })
    .await;
    drop(controller);
    assert!(!profile
        .paths
        .data_dir
        .join(session::AUTH_STATE_FILE)
        .exists());
    assert!(!profile.paths.data_dir.join(".account-owner.json").exists());
    drop(profile);
    let deadline = Instant::now() + Duration::from_secs(2);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(weak.upgrade().is_none());
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("public_probe result={error:?}"),
        Err(_) => panic!("public_probe result=deadline"),
    }
}

fn bootstrap_tls_fixture(uid: &'static str) -> (HttpAdapter, thread::JoinHandle<()>) {
    bootstrap_tls_fixture_reply(uid, None)
}
fn bootstrap_tls_fixture_reply(
    uid: &'static str,
    response: Option<Value>,
) -> (HttpAdapter, thread::JoinHandle<()>) {
    let (address, server) = bootstrap_tls_server_reply(uid, response);
    (HttpAdapter::local_test_adapter(address), server)
}
fn bootstrap_tls_server_reply(
    uid: &'static str,
    response: Option<Value>,
) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig, ServerConnection, StreamOwned,
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::{Command, Stdio},
    };
    let material = super::super::http::tls_tests::material();
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(material.leaf.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(material.key.clone())),
            )
            .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut tls = StreamOwned::new(ServerConnection::new(Arc::new(config)).unwrap(), socket);
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            tls.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
            assert!(bytes.len() < 16384);
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request
            .lines()
            .any(|line| line.eq_ignore_ascii_case("cookie: zpw_sek=synthetic-cookie")));
        let path = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let query = path.split_once('?').unwrap().1;
        assert!(path.starts_with("/api/login/getLoginInfo?"));
        assert!(request
            .lines()
            .any(|line| line.eq_ignore_ascii_case("user-agent: ZaloPC-linux-24v681")));
        let params: std::collections::BTreeMap<_, _> =
            url::form_urlencoded::parse(query.as_bytes()).collect();
        assert_eq!(params["type"], "24");
        let mut child = Command::new("node")
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/bootstrap/encrypted_login_reply.cjs"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                &serde_json::to_vec(
                    &serde_json::json!({"query":query,"uid":uid,"response":response}),
                )
                .unwrap(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        write!(
            tls,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            output.stdout.len()
        )
        .unwrap();
        tls.write_all(&output.stdout).unwrap();
        tls.flush().unwrap();
    });
    (address, server)
}

#[test]
fn server_confirmation_cannot_prepopulate_companion_proof_query() {
    let mut value = serde_json::json!({"token_id":"synthetic-token","chk_wait_scan":"https://wpa.zaloapp.com/scan","chk_wait_cfirm":"https://wpa.zaloapp.com/confirm?encPK=untrusted","qr_timeout":1,"qr_retry":1});
    assert!(QrDescriptor::parse(&value, false).is_err());
    value["chk_wait_cfirm"] =
        Value::String("https://wpa.zaloapp.com/confirm?ordinary=allowed".into());
    assert!(QrDescriptor::parse(&value, false).is_ok());
}

#[tokio::test]
async fn phone_approved_actual_tls_flow_commits_exact_assurance_without_loading_trust_module() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "classic").unwrap());
    let (http, server) = phone_tls_fixture(false);
    let (tx, mut rx) = mpsc::channel(EVENT_CAPACITY);
    let epoch = AtomicU64::new(1);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 1,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    tokio::time::timeout(
        Duration::from_secs(10),
        authenticate_with_http(
            profile.clone(),
            Some(PathBuf::from("/nonexistent/trust-wasm")),
            false,
            &context,
            http,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    server.join().unwrap();
    let mut ready = None;
    let mut qr = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            LoginEvent::Ready { session, .. } => ready = Some(session),
            LoginEvent::Qr { pixels, .. } => {
                qr = true;
                assert_eq!(
                    pixels.rgba.len(),
                    pixels.width as usize * pixels.height as usize * 4
                );
            }
            _ => {}
        }
    }
    let ready = ready.unwrap();
    assert!(qr);
    assert_eq!(ready.account_id(), "11");
    assert_eq!(ready.assurance(), session::AuthAssurance::PhoneApproved);
    assert!(ready.config().services.contains_key("chat"));
    assert!(ready.config().services.contains_key("group"));
    assert_eq!(ready.config().services.len(), 3);
    assert_eq!(
        ready.config().services["profile"][0].host_str(),
        Some("profile-wpa.zaloapp.com")
    );
    let record = AuthRecord::load(&profile).unwrap().unwrap();
    assert!(record.replay.is_empty());
    assert!(matches!(
        record.active.as_ref().unwrap().proof,
        SessionProof::PhoneApproved
    ));
    let value: Value = serde_json::from_slice(
        &fs::read(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(value["active"]["proof"]["assurance"], "PhoneApproved");
    assert!(value["active"]["proof"].get("trust").is_none());
    assert!(profile.check_account("12").is_err());
    drop(ready);
    let (http, server) = bootstrap_tls_fixture("11");
    let epoch = AtomicU64::new(2);
    let context = Attempt {
        generation: 2,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    authenticate_with_http(
        profile.clone(),
        Some(PathBuf::from("/nonexistent/trust-wasm")),
        true,
        &context,
        http,
    )
    .await
    .unwrap();
    server.join().unwrap();
    let mut resumed = false;
    while let Ok(event) = rx.try_recv() {
        if let LoginEvent::Ready { session, .. } = event {
            assert_eq!(session.assurance(), session::AuthAssurance::PhoneApproved);
            resumed = true;
        }
    }
    assert!(resumed);
    assert!(matches!(
        AuthRecord::load(&profile)
            .unwrap()
            .unwrap()
            .active
            .unwrap()
            .proof,
        SessionProof::PhoneApproved
    ));
}
#[tokio::test]
async fn advertised_trusted_mode_never_downgrades_after_missing_primary_proof() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "trusted").unwrap());
    let (http, server) = phone_tls_fixture(true);
    let (tx, mut rx) = mpsc::channel(EVENT_CAPACITY);
    let epoch = AtomicU64::new(1);
    let committed = AtomicU64::new(0);
    let repaint = repaint();
    let context = Attempt {
        generation: 1,
        tx: &tx,
        epoch: &epoch,
        last_commit: &committed,
        repaint: &repaint,
    };
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        authenticate_with_http(profile.clone(), None, false, &context, http),
    )
    .await
    .unwrap();
    assert_eq!(result.err(), Some(LoginFailure::Trust));
    server.join().unwrap();
    assert!(!profile
        .paths
        .data_dir
        .join(session::AUTH_STATE_FILE)
        .exists());
    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(event, LoginEvent::Ready { .. }));
    }
    assert!(reject_unexpected_linking(
        &serde_json::json!({"link_data":{"enc_link_data":"nonempty","enc_link_hmac":""}})
    )
    .is_err());
}
fn phone_tls_fixture(trusted: bool) -> (HttpAdapter, thread::JoinHandle<()>) {
    let (address, server) = phone_tls_server(trusted);
    (HttpAdapter::local_test_adapter(address), server)
}
fn phone_tls_server(trusted: bool) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    phone_tls_server_mode(trusted, None, false)
}
fn phone_tls_server_mode(
    trusted: bool,
    initial_rejection: Option<i64>,
    hold_qr: bool,
) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig, ServerConnection, StreamOwned,
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::{Command, Stdio},
    };
    let material = super::super::http::tls_tests::material();
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(material.leaf.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(material.key.clone())),
            )
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut png = Vec::new();
        if hold_qr {
            let pixels = qr_pixels("zalo-native-synthetic-startup-fixture").unwrap();
            let mut encoder = png::Encoder::new(&mut png, pixels.width, pixels.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels.rgba)
                .unwrap();
        } else {
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[0, 255, 255, 0])
                .unwrap();
        }
        let mut routes = if trusted {
            vec![
                "/api/login/getServerInfo",
                "/api/login/reqqr",
                "/fixture-scan",
            ]
        } else {
            vec![
                "/api/login/getServerInfo",
                "/api/login/reqqr",
                "/fixture-scan",
                "/fixture-confirm",
                "/api/login/getLoginInfo",
            ]
        };
        if hold_qr {
            routes.truncate(2);
        }
        if initial_rejection.is_some() {
            routes.insert(
                0,
                if initial_rejection == Some(-503) {
                    "/api/login/getServerInfo"
                } else {
                    "/api/login/getLoginInfo"
                },
            );
        }
        for (index, route) in routes.into_iter().enumerate() {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config.clone()).unwrap(), socket);
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                tls.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
                assert!(bytes.len() < 16384);
            }
            let request = String::from_utf8(bytes).unwrap();
            let path = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            assert_eq!(path.split('?').next().unwrap(), route);
            assert!(request
                .lines()
                .any(|line| line.eq_ignore_ascii_case("user-agent: ZaloPC-linux-24v681")));
            if route.starts_with("/api/login/") {
                let parsed = Url::parse(&format!("https://wpa.zaloapp.com{path}")).unwrap();
                assert_eq!(
                    parsed
                        .query_pairs()
                        .find(|(key, _)| key == "type")
                        .unwrap()
                        .1,
                    "24"
                );
            }
            let data = match route {
                "/api/login/getServerInfo" => {
                    serde_json::json!({"server_time":now_ms().unwrap()/1000,"setttings":{"authen":{"encryption":{"enable":1},"trusted_device":{"self_g":u8::from(trusted),"qr_format_trust_device":u8::from(trusted)}}}})
                }
                "/api/login/reqqr" => {
                    serde_json::json!({"token_id":"synthetic-token","base64_qr":STANDARD.encode(&png),"chk_wait_scan":"https://wpa.zaloapp.com/fixture-scan?token=synthetic","chk_wait_cfirm":"https://wpa.zaloapp.com/fixture-confirm?token=synthetic","qr_timeout":if hold_qr{180}else{1},"qr_retry":1})
                }
                "/fixture-scan" => serde_json::json!({"avatar":"","display_name":"Synthetic"}),
                "/fixture-confirm" => {
                    assert!(!path.contains("encPK"));
                    serde_json::json!({"uid":"11","dkey":"synthetic-storage","zpw_sek":"synthetic-cookie","profile":{"displayName":"Synthetic"}})
                }
                _ => Value::Null,
            };
            let response = if index == 0 && initial_rejection == Some(-503) {
                b"temporary fixture failure".to_vec()
            } else if route == "/api/login/reqqr" || route == "/api/login/getLoginInfo" {
                if route.ends_with("getLoginInfo") {
                    assert!(request
                        .lines()
                        .any(|line| line.eq_ignore_ascii_case("cookie: zpw_sek=synthetic-cookie")));
                }
                let mut child = Command::new("node")
                    .arg(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("tests/fixtures/bootstrap/encrypted_login_reply.cjs"),
                    )
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                let mut input =
                    serde_json::json!({"query":path.split_once('?').unwrap().1,"uid":"11"});
                if let (0, Some(code)) = (index, initial_rejection) {
                    input["response"] = serde_json::json!({"error_code":code,"data":null});
                } else if route.ends_with("reqqr") {
                    input["response"] = serde_json::json!({"error_code":0,"data":data});
                }
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(&serde_json::to_vec(&input).unwrap())
                    .unwrap();
                let output = child.wait_with_output().unwrap();
                assert!(output.status.success());
                output.stdout
            } else {
                serde_json::to_vec(&serde_json::json!({"error_code":0,"data":data})).unwrap()
            };
            write!(
                tls,
                "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                if index == 0 && initial_rejection == Some(-503) {
                    "503 Service Unavailable"
                } else {
                    "200 OK"
                },
                response.len()
            )
            .unwrap();
            tls.write_all(&response).unwrap();
            tls.flush().unwrap();
        }
        if hold_qr {
            // Keep a real local long-poll pending until GUI cancellation closes
            // it. No confirmation/Ready can escape into messaging transports.
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(60)))
                .unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), socket);
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") && header.len() < 16384 {
                let mut byte = [0];
                if tls.read_exact(&mut byte).is_err() {
                    return;
                }
                header.push(byte[0]);
            }
            let _ = tls.read(&mut [0]);
        }
    });
    (address, server)
}

#[test]
fn assurance_record_rejects_fake_or_missing_trust_payloads() {
    assert!(serde_json::from_value::<SessionProof>(
        serde_json::json!({"assurance":"PhoneApproved","trust":{"master_id":"fake"}})
    )
    .is_err());
    assert!(serde_json::from_value::<SessionProof>(
        serde_json::json!({"assurance":"TrustedDevice"})
    )
    .is_err());
    let mut value = serde_json::json!({"token_id":"synthetic","chk_wait_scan":"https://wpa.zaloapp.com/s","chk_wait_cfirm":"https://wpa.zaloapp.com/c","qr_timeout":180,"qr_retry":3});
    assert_eq!(
        QrDescriptor::parse(&value, false)
            .unwrap()
            .request(false)
            .timeout_ms,
        180000
    );
    value["qr_timeout"] = Value::from(181);
    assert!(QrDescriptor::parse(&value, false).is_err());
}

#[test]
fn polling_policy_is_exact_and_does_not_expand_authentication_origins() {
    assert!(qr::validate_poll_url("https://qr-id.zaloapp.com/fixture?opaque=synthetic").is_ok());
    assert!(qr::validate_login_url("https://qr-id.zaloapp.com/").is_err());
    for text in [
        "https://qr-id.zaloapp.com.attacker.invalid/",
        "https://attacker.zaloapp.com/",
        "https://localhost/",
        "https://127.0.0.1/",
        "https://[::1]/",
        "http://qr-id.zaloapp.com/",
        "https://user@qr-id.zaloapp.com/",
        "https://qr-id.zaloapp.com:444/",
        "https://qr-id.zaloapp.com/#fragment",
    ] {
        assert!(qr::validate_poll_url(text).is_err());
    }
}

#[tokio::test]
async fn bootstrap_failures_preserve_sanitized_stage_and_code_without_false_server_rejection() {
    use bootstrap::{BootstrapError as E, BootstrapFailure as F, BootstrapStage as S};
    let pending = PendingCookies::from_confirmation(
        &serde_json::json!({"uid":"11","dkey":"synthetic-storage","zpw_sek":"synthetic-cookie"}),
    )
    .unwrap();
    let unsafe_config = serde_json::json!({"error_code":0,"data":{"uid":"11","dkey":"synthetic-storage","zpw_enk":"AAECAwQFBgcICQoLDA0ODw==","zpw_service_map_v3":{"chat":["http://chat-wpa.zaloapp.com/private-fixture-token"]}}});
    let (mut http, server) = bootstrap_tls_fixture_reply("11", Some(unsafe_config));
    let failure =
        bootstrap_authenticated(&mut http, "native-fixture", &pending, ClientType::PcLinux24)
            .await
            .err()
            .unwrap();
    server.join().unwrap();
    assert_eq!(
        failure,
        LoginFailure::Bootstrap(F {
            stage: S::ServiceConfiguration,
            category: E::UnsafeEndpoint
        })
    );
    let text = failure.to_string();
    assert!(text.contains("bootstrap/service-config/unsafe-endpoint"));
    assert!(!text.contains("Máy chủ từ chối"));
    assert!(!text.contains("zaloapp"));
    assert!(!text.contains("private-fixture-token"));
    let (mut http, server) = bootstrap_tls_fixture_reply(
        "11",
        Some(serde_json::json!({"error_code":600,"error_message":"secret-fixture-cookie-and-url"})),
    );
    let failure =
        bootstrap_authenticated(&mut http, "native-fixture", &pending, ClientType::PcLinux24)
            .await
            .err()
            .unwrap();
    server.join().unwrap();
    assert_eq!(
        failure,
        LoginFailure::Bootstrap(F {
            stage: S::InnerEnvelope,
            category: E::AuthenticationRejected(600)
        })
    );
    assert!(failure
        .to_string()
        .contains("authentication-rejected code=600"));
    assert!(!format!("{failure:?} {failure}").contains("secret-fixture"));
    let (mut http, server) = bootstrap_tls_fixture_reply(
        "11",
        Some(
            serde_json::json!({"error_code":0,"data":{"uid":"11","dkey":"synthetic-storage","zpw_enk":"invalid-fixture-secret"}}),
        ),
    );
    let failure =
        bootstrap_authenticated(&mut http, "native-fixture", &pending, ClientType::PcLinux24)
            .await
            .err()
            .unwrap();
    server.join().unwrap();
    assert_eq!(
        failure,
        LoginFailure::Bootstrap(F {
            stage: S::SessionKey,
            category: E::Malformed
        })
    );
    assert!(!failure.to_string().contains("invalid-fixture"));
}

#[tokio::test]
async fn actual_controller_startup_auto_qr_then_verified_session_without_manual_resume() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "controller-linux24").unwrap());
    let (address, server) = phone_tls_server(false);
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    let generation = controller.startup().unwrap();
    let mut saw_qr = false;
    let session = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match controller.events.recv().await.unwrap() {
                LoginEvent::Qr {
                    generation: g,
                    pixels,
                } if g == generation => {
                    saw_qr = true;
                    assert_eq!(pixels.rgba.len(), 16);
                }
                LoginEvent::Ready {
                    generation: g,
                    session,
                } if g == generation => break session,
                LoginEvent::Failed { error, .. } => panic!("synthetic controller failed: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    server.join().unwrap();
    assert!(saw_qr);
    assert_eq!(session.client_type(), ClientType::PcLinux24);
    let path = profile.paths.data_dir.join(session::AUTH_STATE_FILE);
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["active"]["client_type"], 24);
    drop(session);
    drop(controller);
    let (address, server) = bootstrap_tls_server_reply("11", None);
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    let generation = controller.startup().unwrap();
    let session = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match controller.events.recv().await.unwrap() {
                LoginEvent::Ready {
                    generation: g,
                    session,
                } if g == generation => break session,
                LoginEvent::Failed { error, .. } => {
                    panic!("synthetic desktop resume failed: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(session.client_type(), ClientType::PcLinux24);
    assert_eq!(session.account_id(), "11");
    assert_eq!(
        AuthRecord::load(&profile)
            .unwrap()
            .unwrap()
            .active
            .unwrap()
            .client_type,
        ClientType::PcLinux24
    );
    let generation = controller.logout().unwrap();
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {if matches!(controller.events.recv().await,Some(LoginEvent::SignedOut{generation:g}) if g==generation){break;}}
    }).await.unwrap();
    assert!(AuthRecord::load(&profile)
        .unwrap()
        .unwrap()
        .active
        .is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), controller.events.recv())
            .await
            .is_err(),
        "explicit logout must not restart login"
    );
}
#[tokio::test]
async fn legacy_missing_client_type_requires_approval_without_network_or_any_disk_mutation() {
    use std::net::TcpListener;
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "legacy30").unwrap());
    let mut record = AuthRecord::empty("11");
    let mut active = saved();
    active.proof = SessionProof::PhoneApproved;
    record.active = Some(active);
    record
        .replay
        .insert("synthetic-master".into(), 1700000000000);
    let mut value = serde_json::to_value(&record).unwrap();
    value["version"] = Value::from(1);
    value["active"]
        .as_object_mut()
        .unwrap()
        .remove("client_type");
    let path = profile.paths.data_dir.join(session::AUTH_STATE_FILE);
    crate::profile::atomic_write_secret(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let db_path = profile.paths.data_dir.join("messages.sqlite");
    {
        let mut store = crate::store::MessageStore::open(&db_path).unwrap();
        store
            .insert_batch(&[crate::domain::Message {
                account_id: "11".into(),
                thread_id: "d:12".into(),
                id: "kept".into(),
                sender: "12".into(),
                text: "Preserved history".into(),
                sticker: None,
                media: None,
                sent_at_ms: 1,
            }])
            .unwrap();
    }
    let history_before = fs::read(&db_path).unwrap();
    let loaded = AuthRecord::load(&profile).unwrap().unwrap();
    assert_eq!(
        loaded.active.as_ref().unwrap().client_type,
        ClientType::Legacy30
    );
    assert!(VerifiedSession::from_committed(
        loaded.active.as_ref().unwrap(),
        config(),
        profile.clone()
    )
    .is_err());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(listener.local_addr().unwrap()),
    )
    .unwrap();
    let generation = controller.resume().unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(LoginEvent::Failed {
                generation: g,
                error,
            }) = controller.events.recv().await
            {
                if g == generation {
                    break error;
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(failure, LoginFailure::FreshApprovalRequired);
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&db_path).unwrap(), history_before);
    assert_eq!(
        AuthRecord::load(&profile).unwrap().unwrap().replay,
        record.replay
    );
    assert!(profile.check_account("12").is_err());
    assert!(failure.to_string().contains("Lịch sử đã lưu vẫn được giữ"));
}
#[test]
fn new_record_requires_explicit_supported_type_but_v1_defaults_only_to_legacy() {
    let scratch = Scratch::new();
    let profile = Profile::at_root(&scratch.0, "record-types").unwrap();
    let mut record = AuthRecord::empty("11");
    record.active = Some(saved());
    let path = profile.paths.data_dir.join(session::AUTH_STATE_FILE);
    let mut value = serde_json::to_value(&record).unwrap();
    value["active"]
        .as_object_mut()
        .unwrap()
        .remove("client_type");
    crate::profile::atomic_write_secret(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(AuthRecord::load(&profile).is_err());
    value["active"]["client_type"] = Value::from(25);
    crate::profile::atomic_write_secret(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(AuthRecord::load(&profile).is_err());
    value["active"]["client_type"] = Value::from(24);
    crate::profile::atomic_write_secret(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        AuthRecord::load(&profile)
            .unwrap()
            .unwrap()
            .active
            .unwrap()
            .client_type,
        ClientType::PcLinux24
    );
}

fn write_phone_approved_session(profile: &Profile) -> Vec<u8> {
    let mut record = AuthRecord::empty("11");
    let mut active = saved();
    active.proof = SessionProof::PhoneApproved;
    record.active = Some(active);
    let encoded = record.encode().unwrap();
    crate::profile::atomic_write_secret(
        &profile.paths.data_dir.join(session::AUTH_STATE_FILE),
        &encoded,
    )
    .unwrap();
    encoded
}

#[tokio::test]
async fn startup_verified_authentication_expiry_automatically_opens_fresh_qr() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "startup-expired").unwrap());
    write_phone_approved_session(&profile);
    let (address, server) = phone_tls_server_mode(false, Some(600), false);
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    let generation = controller.startup().unwrap();
    let mut qr = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match controller.events.recv().await.unwrap() {
                LoginEvent::Qr { generation: g, .. } if g == generation => qr = true,
                LoginEvent::Ready {
                    generation: g,
                    session,
                } if g == generation => {
                    assert!(qr, "verified expiry must enter QR before fresh Ready");
                    assert_eq!(session.account_id(), "11");
                    break;
                }
                LoginEvent::Failed { error, .. } => panic!("expired startup failed {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn startup_service_failure_preserves_saved_session_and_never_enters_qr() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "startup-retry").unwrap());
    let before = write_phone_approved_session(&profile);
    let (address, server) = bootstrap_tls_server_reply(
        "11",
        Some(serde_json::json!({"error_code":18060,"data":null})),
    );
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    let generation = controller.startup().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match controller.events.recv().await.unwrap() {
                LoginEvent::Progress {
                    stage: LoginStage::CreatingQr,
                    ..
                }
                | LoginEvent::Qr { .. } => panic!("non-auth failure must not force QR"),
                LoginEvent::Failed {
                    generation: g,
                    error,
                } if g == generation => {
                    assert!(matches!(
                        error,
                        LoginFailure::Bootstrap(bootstrap::BootstrapFailure {
                            category: bootstrap::BootstrapError::EncryptionRejected(18060),
                            ..
                        })
                    ));
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(
        fs::read(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap(),
        before
    );
}

#[test]
fn startup_fallback_policy_does_not_expire_sessions_on_network_http_or_unknown_service_errors() {
    use bootstrap::{BootstrapError as E, BootstrapFailure as F, BootstrapStage as S};
    for error in [
        LoginFailure::Network,
        LoginFailure::Expired,
        LoginFailure::Rejected,
        LoginFailure::Storage,
        LoginFailure::Malformed,
        LoginFailure::Cancelled,
        LoginFailure::Bootstrap(F {
            stage: S::Http,
            category: E::Http(401),
        }),
        LoginFailure::Bootstrap(F {
            stage: S::InnerEnvelope,
            category: E::ServiceRejected(500),
        }),
        LoginFailure::Bootstrap(F {
            stage: S::ResponseDecryption,
            category: E::EncryptionRejected(18060),
        }),
    ] {
        assert!(
            !requires_fresh_login(error),
            "ambiguous failure preserves credentials: {error}"
        );
    }
    assert!(requires_fresh_login(LoginFailure::FreshApprovalRequired));
    assert!(requires_fresh_login(LoginFailure::Bootstrap(F {
        stage: S::InnerEnvelope,
        category: E::AuthenticationRejected(102)
    })));
}

#[cfg(feature = "messaging")]
pub(super) fn startup_fixture(
    profile: Arc<Profile>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    mode: &str,
) -> Result<LoginController> {
    let (address, _server) = match mode {
        "qr" => phone_tls_server_mode(false, None, true),
        "retry" => {
            write_phone_approved_session(&profile);
            bootstrap_tls_server_reply(
                "11",
                Some(serde_json::json!({"error_code":18060,"data":null})),
            )
        }
        _ => anyhow::bail!("Unknown synthetic startup fixture mode"),
    };
    LoginController::with_transport(profile, None, repaint, AuthTransport::Local(address))
}

#[tokio::test]
async fn startup_without_saved_session_retries_transient_failure_then_opens_qr_without_mode_choice()
{
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "startup-no-session-retry").unwrap());
    let (address, server) = phone_tls_server_mode(false, Some(-503), false);
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    let generation = controller.startup().unwrap();
    tokio::time::timeout(Duration::from_secs(5),async{loop{if matches!(controller.events.recv().await,Some(LoginEvent::Failed{generation:g,..})if g==generation){break;}}}).await.unwrap();
    assert!(AuthRecord::load(&profile).unwrap().is_none());
    let generation = controller.startup().unwrap();
    let mut saw_qr = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match controller.events.recv().await.unwrap() {
                LoginEvent::Qr { generation: g, .. } if g == generation => saw_qr = true,
                LoginEvent::Ready { generation: g, .. } if g == generation => {
                    assert!(saw_qr);
                    break;
                }
                LoginEvent::Failed { error, .. } => panic!("startup retry failed {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn startup_connection_failure_and_retry_never_force_qr_or_mutate_saved_session() {
    let scratch = Scratch::new();
    let profile = Arc::new(Profile::at_root(&scratch.0, "startup-network-retry").unwrap());
    let before = write_phone_approved_session(&profile);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut controller = LoginController::with_transport(
        profile.clone(),
        None,
        repaint(),
        AuthTransport::Local(address),
    )
    .unwrap();
    for _ in 0..2 {
        let generation = controller.startup().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match controller.events.recv().await.unwrap() {
                    LoginEvent::Qr { .. }
                    | LoginEvent::Progress {
                        stage: LoginStage::CreatingQr,
                        ..
                    } => panic!("network failure cannot force QR"),
                    LoginEvent::Failed {
                        generation: g,
                        error,
                    } if g == generation => {
                        assert_eq!(error, LoginFailure::Network);
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            fs::read(profile.paths.data_dir.join(session::AUTH_STATE_FILE)).unwrap(),
            before
        );
    }
}
