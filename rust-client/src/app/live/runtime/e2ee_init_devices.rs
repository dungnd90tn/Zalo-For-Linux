//! Initializes end-to-end sessions with this account's other devices, like the original client does after
//! it registers a new bundle (`INIT_SUBDEVICE`, see `E2EE_LIVE_PLAN.md`). Opt-in: `ZALO_E2EE_INIT_SUBDEVICES`
//! must be an absolute report path, and the key replacement must have been done.
//!
//! For every other device in the account's own bundle list (mobile, web): request its bundle with
//! `isReInit` (541), let the engine create a session while encrypting the init text `{"subDevice":[]}`,
//! commit that session durably, then send it in a 543 init message (`type 0`, `uidTo` = this account). A
//! device that already has a session here is skipped, so a repeated run sends nothing. The init text goes
//! through the same reviewed pipeline as a message (reserve, plan, prepare, one SQL commit, dispatch
//! bookkeeping) but is delivered by command 543 and never shown as a conversation message.
use super::e2ee_send_probe::{recipient_for, BundleQuery, GENERATION};
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{Cancellation, Coordinator},
    protocol::signal::{DevicePlatform, DeviceSelection, Scope},
    store::e2ee::{self, GenerationGate},
};
use std::sync::Arc;
use zalo_signal_engine as engine;

pub(super) const INIT_TEXT: &str = r#"{"subDevice":[]}"#;
const WAIT: Duration = Duration::from_secs(15);

pub(super) fn target() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_INIT_SUBDEVICES")?);
    path.is_absolute().then_some(path)
}
pub(super) fn claim() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}

impl Connection {
    /// Best effort; the outcome is in the report.
    pub(super) async fn e2ee_init_subdevices(&mut self, db: &Database, path: &std::path::Path) {
        let Some(mut report) = Report::open(path) else {
            return;
        };
        match self.init_subdevices_inner(db, &mut report).await {
            Ok(message) => report.line(&format!("RESULT: {message}")),
            Err(error) => report.line(&format!("RESULT: stopped: {error}")),
        }
    }

    async fn init_subdevices_inner(
        &mut self,
        db: &Database,
        report: &mut Report,
    ) -> Result<&'static str> {
        let owner = self.account.clone();
        let scope = Scope::new(&owner, GENERATION)?;
        let current = {
            let owner = owner.clone();
            db.call(move |s| s.e2ee_current_local(&owner)).await?
        };
        let epoch = current.ok_or_else(|| {
            anyhow::anyhow!("this client holds no E2EE key yet (run the key replacement first)")
        })?;
        let local = e2ee::LocalNamespace {
            epoch: epoch.to_string(),
            platform: e2ee::Platform::Pc,
        };
        let all = self
            .fetch_bundles_for_init(&scope, &owner, 9_100_001, report)
            .await?;
        let devices: Vec<DevicePlatform> = all
            .bundles
            .iter()
            .map(|b| b.device)
            .filter(|device| *device != DevicePlatform::Pc)
            .collect();
        report.line(&format!(
            "other devices of this account: {:?}",
            devices.iter().map(|d| d.wire()).collect::<Vec<_>>()
        ));
        if devices.is_empty() {
            return Ok("this account has no other device to initialize");
        }
        let (mut initialized, mut skipped) = (0, 0);
        for (index, device) in devices.into_iter().enumerate() {
            let found = self
                .fetch_selected_for_init(&scope, &owner, 9_100_100 + index as u64, device, report)
                .await?;
            let Some(bundle) = found.bundles.into_iter().find(|b| b.device == device) else {
                report.line(&format!(
                    "device {}: the service returned no bundle; skipped",
                    device.wire()
                ));
                continue;
            };
            let gate = Arc::new(GenerationGate::new(GENERATION)?);
            let cancel = Cancellation::default();
            let mut coordinator = Coordinator::new(
                engine::AccountId::new(&owner)
                    .map_err(|_| anyhow::anyhow!("invalid account id"))?,
                GENERATION,
                super::e2ee_send_probe::convert(&local)?,
            )?;
            let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
            let (o, l, g, c, thread) = (
                owner.clone(),
                local.clone(),
                gate.clone(),
                cancel.clone(),
                format!("d:{owner}"),
            );
            // Plan needs the committed store; the Coordinator is lent to the storage thread and back.
            let coordinator_shared = Arc::new(coordinator);
            let co = coordinator_shared.clone();
            let planned = db
                .call(move |store| {
                    if let Some(active) = store.e2ee_active_peer(
                        &o,
                        &l,
                        &o,
                        super::e2ee_send_probe::platform(device),
                    )? {
                        let key = e2ee::RecordKey::Session {
                            local: l.clone(),
                            peer: active.namespace,
                        };
                        if !store.e2ee_snapshot(&o, &[key])?.records.is_empty() {
                            return Ok(None);
                        }
                    }
                    let recipient = match recipient_for(store, &o, &l, &o, &bundle, now_ms)? {
                        Ok(recipient) => recipient,
                        Err(note) => anyhow::bail!(note),
                    };
                    let client_id = store.e2ee_reserve_send(&o, &l, &thread, INIT_TEXT, now_ms)?;
                    match co.plan_personal(store, client_id, INIT_TEXT, vec![recipient], &g, &c) {
                        Ok(plan) => Ok(Some((client_id, plan))),
                        Err(error) => {
                            let _ = store.e2ee_discard_reserved(&o, client_id);
                            Err(error)
                        }
                    }
                })
                .await?;
            let Some((client_id, plan)) = planned else {
                report.line(&format!(
                    "device {}: a session already exists; skipped",
                    device.wire()
                ));
                skipped += 1;
                continue;
            };
            coordinator = Arc::try_unwrap(coordinator_shared)
                .map_err(|_| anyhow::anyhow!("the coordinator is still shared"))?;
            let prepared = match coordinator.prepare_personal(plan, &gate, &cancel).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    let (o, id) = (owner.clone(), client_id);
                    let _ = db.call(move |s| s.e2ee_discard_reserved(&o, id)).await;
                    return Err(error);
                }
            };
            // The ciphertext for the one device, as the SMS record carries it.
            let cipher = serde_json::from_slice::<Value>(prepared.exact_sms())
                .ok()
                .and_then(|v| serde_json::from_str::<Vec<Value>>(v["listParams"].as_str()?).ok())
                .and_then(|records| records.first()?["message"].as_str().map(str::to_owned))
                .ok_or_else(|| anyhow::anyhow!("the prepared message has no ciphertext"))?;
            // Durable first: the new session and the exact bytes are committed before anything is sent.
            let (o, g2) = (owner.clone(), gate.clone());
            db.call(move |s| {
                s.e2ee_commit_verified_send(&prepared, &g2)?;
                s.e2ee_begin_dispatch(&o, client_id, GENERATION, &g2)?;
                Ok(())
            })
            .await?;
            report.line(&format!(
                "device {}: session created and committed; sending the init message",
                device.wire()
            ));
            let request_id = self.request();
            let body = json!({"reqId":request_id,"data":{"type":0,"uidTo":owner,"data":[{"deviceId":device.wire(),"data":cipher}]}});
            let outcome = self.send_init(&request_id, body, report).await;
            let (o, id) = (owner.clone(), client_id);
            match outcome {
                Ok(()) => {
                    let server_id = format!("init-{}-{now_ms}", device.wire());
                    db.call(move |s| {
                        s.e2ee_record_ack(&o, id, &server_id)?;
                        s.e2ee_retire_init(&o, id)
                    })
                    .await?;
                    report.line(&format!("device {}: init message accepted", device.wire()));
                    initialized += 1;
                }
                Err(error) => {
                    // Whether it arrived is not known; never resend.
                    let _ = db.call(move |s| s.e2ee_send_failed(&o, id, false)).await;
                    return Err(error);
                }
            }
        }
        report.line(&format!(
            "initialized {initialized} device(s), {skipped} already had a session"
        ));
        Ok("session initialization with the account's other devices finished")
    }

    pub(super) async fn send_init(
        &mut self,
        request_id: &str,
        body: Value,
        report: &mut Report,
    ) -> Result<()> {
        self.send(543, 0, body).await?;
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for the init reply"))??;
            let Some(frame) = frame.filter(|f| {
                f.cmd == 543
                    && f.payload.pointer("/data/reqId").and_then(Value::as_str) == Some(request_id)
            }) else {
                continue;
            };
            report.line("## reply command 543");
            for line in super::e2ee_probe::shape(&frame.payload) {
                report.line(&line);
            }
            // The real 543 reply carries `err` directly under `data` (`{data:{err,reqId,ts}}`); other commands
            // nest it one level deeper. Either place counts, and any non-zero value is a refusal.
            let err = frame
                .payload
                .pointer("/data/err")
                .or_else(|| frame.payload.pointer("/data/data/err"));
            return match err {
                None | Some(Value::Null) => Ok(()),
                Some(err) if err.as_i64() == Some(0) => Ok(()),
                Some(err) => anyhow::bail!("the service refused the init message (err {err})"),
            };
        }
    }
    async fn fetch_bundles_for_init(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        report: &mut Report,
    ) -> Result<crate::protocol::signal::FetchedBundles> {
        self.fetch_selected(
            scope,
            uid,
            sequence,
            BundleQuery {
                selection: DeviceSelection::All,
                reinit: false,
                verbose: true,
            },
            report,
        )
        .await
    }
    async fn fetch_selected_for_init(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        device: DevicePlatform,
        report: &mut Report,
    ) -> Result<crate::protocol::signal::FetchedBundles> {
        self.fetch_selected(
            scope,
            uid,
            sequence,
            BundleQuery {
                selection: DeviceSelection::One(device),
                reinit: true,
                verbose: false,
            },
            report,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use futures_util::{SinkExt, StreamExt};
    use std::io::Write;

    fn bytes(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    async fn wire_bundle(platform: engine::SignalPlatform, device: u8) -> Value {
        let id = engine::AccountId::new("11").unwrap();
        let worker = engine::Engine::new(id.clone(), 1, engine::EngineOptions::default()).unwrap();
        let result = worker
            .submit(engine::Request {
                account: id.clone(),
                generation: 1,
                operation_id: 1,
                expected_revision: 0,
                operation: engine::Operation::CreateBundle {
                    local: engine::LocalNamespace {
                        epoch: engine::RegistrationEpoch::new(1).unwrap(),
                        platform,
                    },
                    prekey_count: 2,
                },
                snapshot: engine::Snapshot::new(id, 0, vec![]).unwrap(),
            })
            .unwrap()
            .await
            .unwrap();
        let (_, _, _, _, output, _) = result.into_parts();
        let engine::Output::Bundle(b) = output else {
            panic!("bundle")
        };
        json!({
            "deviceId": device,
            "preKey": {
                "iKey": {"id": b.registration_id, "key": STANDARD.encode(b.identity_key)},
                "signature": STANDARD.encode(b.signature),
                "spKey": {"id": b.signed_prekey_id, "key": STANDARD.encode(b.signed_prekey)},
            },
            "opKeys": b.prekeys.iter().map(|(id, key)| json!({"id": id, "key": STANDARD.encode(key)})).collect::<Vec<_>>(),
        })
    }

    /// The account's other devices are mobile (0) and web (1). The fake service lists both, serves each
    /// by `deviceId`, and accepts 543. Returns what it saw: `(cmd, body.data)` in order.
    async fn serve(
        listener: tokio::net::TcpListener,
        mobile: Value,
        web: Value,
        expected: usize,
        refuse_543: i64,
    ) -> Vec<(u16, Value)> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}}"#)
            .unwrap();
        let auth = wire::encode_frame(
            1,
            0,
            &json!({"error_code":0,"key":STANDARD.encode([7u8; 32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())}),
        )
        .unwrap();
        socket
            .send(tungstenite::Message::Binary(auth))
            .await
            .unwrap();
        let mut seen = Vec::new();
        while seen.len() < expected {
            let Some(Ok(tungstenite::Message::Binary(bytes))) = socket.next().await else {
                break;
            };
            let cmd = u16::from(bytes[1]) | (u16::from(bytes[2]) << 8);
            let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
            seen.push((cmd, body["data"].clone()));
            let data = match cmd {
                541 => {
                    let keys = match body["data"]["uids"][0]["deviceId"].as_i64().unwrap() {
                        -1 => vec![mobile.clone(), web.clone()],
                        0 => vec![mobile.clone()],
                        _ => vec![web.clone()],
                    };
                    json!({"err":0,"errData":[],"keys":[{"uid":"11","err":0,"keys":keys}]})
                }
                _ => json!({"err": 0}),
            };
            // Like the live service, 543 answers `{data:{err,reqId,ts}}` with no inner `data`.
            let envelope = if cmd == 543 {
                json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"err":refuse_543,"reqId":body["reqId"],"ts":1700000000000u64}}})
            } else {
                json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":data}}})
            };
            let frame = wire::encode_frame(cmd, 0, &envelope).unwrap();
            socket
                .send(tungstenite::Message::Binary(frame))
                .await
                .unwrap();
        }
        let _ = tokio::time::timeout(Duration::from_millis(300), socket.next()).await;
        seen
    }

    /// A refused 543 (err under `data`, where the live service puts it) must stop the run and must not
    /// be read as success; the row is left unknown and nothing is resent.
    #[tokio::test]
    async fn a_refused_543_is_an_error_and_is_never_resent() {
        let mobile = wire_bundle(engine::SignalPlatform::Mobile, 0).await;
        let web = wire_bundle(engine::SignalPlatform::Web, 1).await;
        let scratch =
            std::env::temp_dir().join(format!("zalo-initdev-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&db_path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        let report = scratch.join("report.txt");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, mobile, web, 3, -9));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.e2ee_init_subdevices(&db, &report).await;
        let seen = server.await.unwrap();
        assert_eq!(
            seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [541, 541, 543]
        );
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(
            text.contains("RESULT: stopped: the service refused the init message (err -9)"),
            "{text}"
        );
        assert!(!text.contains("init message accepted"), "{text}");
        let c = rusqlite::Connection::open(&db_path).unwrap();
        let states: Vec<String> = c
            .prepare("SELECT state FROM e2ee_outbox")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(states, ["unknown"]);
        std::fs::remove_dir_all(scratch).unwrap();
    }
    fn seed_identity(db_path: &std::path::Path) {
        use crate::store::e2ee::VerifiedBinding;
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/e2ee/session.json"))
                .unwrap();
        let public: [u8; 33] = bytes(fixture["alice_identity_public"].as_str().unwrap())
            .try_into()
            .unwrap();
        let private = bytes(fixture["alice_identity_private"].as_str().unwrap());
        let mut store = crate::store::MessageStore::open(db_path).unwrap();
        let namespace = store
            .e2ee_register_local(
                "11",
                None,
                &VerifiedBinding {
                    platform: e2ee::Platform::Pc,
                    registration_id: 1001,
                    identity_key: public,
                    session_timestamp: 100,
                },
            )
            .unwrap();
        let value = serde_json::to_vec(
            &json!({"publicKey":public.as_slice(),"privateKey":private,"registrationId":1001}),
        )
        .unwrap();
        let key =
            serde_json::to_string(&e2ee::RecordKey::LocalIdentity { local: namespace }).unwrap();
        rusqlite::Connection::open(db_path)
            .unwrap()
            .execute(
                "INSERT INTO e2ee_records(account,key,value) VALUES('11',?1,?2)",
                rusqlite::params![key, value],
            )
            .unwrap();
    }
    #[tokio::test]
    async fn initializes_each_other_device_once_with_a_543_init_and_a_repeat_run_sends_nothing() {
        let mobile = wire_bundle(engine::SignalPlatform::Mobile, 0).await;
        let web = wire_bundle(engine::SignalPlatform::Web, 1).await;
        let scratch =
            std::env::temp_dir().join(format!("zalo-initdev-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        {
            use crate::store::e2ee::VerifiedBinding;
            let fixture: Value =
                serde_json::from_str(include_str!("../../../../tests/fixtures/e2ee/session.json"))
                    .unwrap();
            let public: [u8; 33] = bytes(fixture["alice_identity_public"].as_str().unwrap())
                .try_into()
                .unwrap();
            let private = bytes(fixture["alice_identity_private"].as_str().unwrap());
            let mut store = crate::store::MessageStore::open(&db_path).unwrap();
            let namespace = store
                .e2ee_register_local(
                    "11",
                    None,
                    &VerifiedBinding {
                        platform: e2ee::Platform::Pc,
                        registration_id: 1001,
                        identity_key: public,
                        session_timestamp: 100,
                    },
                )
                .unwrap();
            let value = serde_json::to_vec(
                &json!({"publicKey":public.as_slice(),"privateKey":private,"registrationId":1001}),
            )
            .unwrap();
            let key = serde_json::to_string(&e2ee::RecordKey::LocalIdentity { local: namespace })
                .unwrap();
            rusqlite::Connection::open(&db_path)
                .unwrap()
                .execute(
                    "INSERT INTO e2ee_records(account,key,value) VALUES('11',?1,?2)",
                    rusqlite::params![key, value],
                )
                .unwrap();
        }
        let db = Database::open(profile, "11".into()).await.unwrap();
        let report = scratch.join("report.txt");

        // Run 1: list (all), then per device: bundle (isReInit) and 543, for both devices.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, mobile.clone(), web.clone(), 5, 0));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.e2ee_init_subdevices(&db, &report).await;
        let seen = server.await.unwrap();
        assert_eq!(
            seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [541, 541, 543, 541, 543],
            "{seen:?}"
        );
        assert_eq!(seen[1].1["isReInit"], 1);
        for (cmd, data) in seen.iter().filter(|(c, _)| *c == 543) {
            assert_eq!(*cmd, 543);
            assert_eq!(data["type"], 0);
            assert_eq!(data["uidTo"], "11");
            let record = &data["data"][0];
            assert!(record["deviceId"] == 0 || record["deviceId"] == 1);
            // A fresh session starts with a type-3 prekey message (the first decoded byte).
            assert_eq!(
                STANDARD.decode(record["data"].as_str().unwrap()).unwrap()[0],
                3
            );
        }
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(
            text.contains("initialized 2 device(s), 0 already had a session"),
            "{text}"
        );
        assert!(text.contains("RESULT: session initialization"), "{text}");
        // The init text is bookkeeping only: nothing is left dispatchable and no message was created.
        let c = rusqlite::Connection::open(&db_path).unwrap();
        let states: Vec<String> = c
            .prepare("SELECT state FROM e2ee_outbox")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(states, ["committed", "committed"]);
        assert_eq!(
            c.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );

        // Run 2: sessions exist, so only the device list is read and no 543 is sent.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, mobile, web, 1, 0));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        // The per-device bundle request still happens before the skip decision; only 543 must not.
        connection.e2ee_init_subdevices(&db, &report).await;
        let seen = server.await.unwrap();
        assert!(seen.iter().all(|(c, _)| *c != 543), "{seen:?}");
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
