//! Starts an end-to-end encrypted conversation with a peer: the handshake the original sends before the first
//! message (stage 3 of `E2EE_LIVE_PLAN.md`). Opt-in: `ZALO_E2EE_INIT_PEER` is an absolute report path, and
//! `ZALO_E2EE_PEER` optionally names the contact (as the app shows it; unset means the first usable contact of
//! the account's E2EE list).
//!
//! As captured from the original: 560 for the contact's devices, then one 541 per device with `isReInit`, the
//! engine creates a session with every device while encrypting the init text `{"subDevice":[]}`, the sessions
//! and exact bytes are committed in one SQL transaction, then ONE command 543 (`type 0`, `uidTo` = the contact,
//! one `{deviceId, data}` entry per device) delivers it. The contact's devices answer with a 544 push; chat
//! messages can follow. Devices that already have a session here are left alone, so a repeat sends nothing.
//! The init text is bookkeeping only and never becomes a chat message.
use super::e2ee_init_devices::INIT_TEXT;
use super::e2ee_send_probe::{recipient_for, GENERATION, MAX_TRIES};
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{Cancellation, Coordinator},
    protocol::signal::{self, Scope},
    store::e2ee::{self, GenerationGate},
};
use std::sync::Arc;
use zalo_signal_engine as engine;

/// The report path and the optional contact selector (a name fragment or a numeric UID).
pub(super) fn target() -> Option<(std::path::PathBuf, Option<String>)> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_INIT_PEER")?);
    let selector = std::env::var("ZALO_E2EE_PEER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    path.is_absolute().then_some((path, selector))
}
pub(super) fn claim() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}

impl Connection {
    /// Best effort; the outcome is in the report.
    pub(super) async fn e2ee_init_peer(
        &mut self,
        db: &Database,
        path: &std::path::Path,
        selector: Option<&str>,
    ) {
        let Some(mut report) = Report::open(path) else {
            return;
        };
        match self.init_peer_inner(db, selector, &mut report).await {
            Ok(message) => report.line(&format!("RESULT: {message}")),
            Err(error) => report.line(&format!("RESULT: stopped: {error:#}")),
        }
    }

    async fn init_peer_inner(
        &mut self,
        db: &Database,
        selector: Option<&str>,
        report: &mut Report,
    ) -> Result<&'static str> {
        let owner = self.account.clone();
        let scope = Scope::new(&owner, GENERATION)?;
        let epoch = {
            let owner = owner.clone();
            db.call(move |s| s.e2ee_current_local(&owner)).await?
        }
        .ok_or_else(|| {
            anyhow::anyhow!("this client holds no E2EE key yet (run the key replacement first)")
        })?;
        let local = e2ee::LocalNamespace {
            epoch: epoch.to_string(),
            platform: e2ee::Platform::Pc,
        };
        // Find a contact whose devices the service will hand out (the same walk as the send probe).
        let candidates = self.choose_peers(db, selector, report).await?;
        let (mut found, mut chosen) = (None, None);
        for (index, uid) in candidates.iter().take(MAX_TRIES).enumerate() {
            match self
                .fetch_peer_devices(
                    &scope,
                    uid,
                    9_300_000 + index as u64 * 10,
                    report,
                    index == 0,
                )
                .await
            {
                Ok(bundles) if !bundles.bundles.is_empty() => {
                    found = Some(bundles);
                    chosen = Some(uid.clone());
                    break;
                }
                Ok(_) => report.line(&format!(
                    "contact {}: no device bundles; trying the next",
                    index + 1
                )),
                Err(error) => match error.downcast_ref::<signal::SignalError>() {
                    Some(signal::SignalError::Service(code)) => report.line(&format!(
                        "contact {}: the service answered error {code}; trying the next",
                        index + 1
                    )),
                    _ => return Err(error),
                },
            }
        }
        let (Some(found), Some(peer)) = (found, chosen) else {
            anyhow::bail!(
                "none of the first {} contacts has usable device bundles",
                candidates.len().min(MAX_TRIES)
            );
        };
        report.line(&format!(
            "contact devices: {:?}",
            found
                .bundles
                .iter()
                .map(|b| b.device.wire())
                .collect::<Vec<_>>()
        ));

        let gate = Arc::new(GenerationGate::new(GENERATION)?);
        let cancel = Cancellation::default();
        let coordinator = Arc::new(Coordinator::new(
            engine::AccountId::new(&owner).map_err(|_| anyhow::anyhow!("invalid account id"))?,
            GENERATION,
            super::e2ee_send_probe::convert(&local)?,
        )?);
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let thread = format!("d:{peer}");
        let (o, l, g, c, co, t, p) = (
            owner.clone(),
            local.clone(),
            gate.clone(),
            cancel.clone(),
            coordinator.clone(),
            thread.clone(),
            peer.clone(),
        );
        let planned = db
            .call(move |store| {
                let mut recipients = Vec::new();
                let mut skipped = 0usize;
                for bundle in &found.bundles {
                    if let Some(active) = store.e2ee_active_peer(
                        &o,
                        &l,
                        &p,
                        super::e2ee_send_probe::platform(bundle.device),
                    )? {
                        let key = e2ee::RecordKey::Session {
                            local: l.clone(),
                            peer: active.namespace,
                        };
                        if !store.e2ee_snapshot(&o, &[key])?.records.is_empty() {
                            skipped += 1;
                            continue;
                        }
                    }
                    match recipient_for(store, &o, &l, &p, bundle, now_ms)? {
                        Ok(recipient) => recipients.push(recipient),
                        Err(note) => anyhow::bail!(note),
                    }
                }
                if recipients.is_empty() {
                    return Ok((skipped, None));
                }
                let client_id = store.e2ee_reserve_send(&o, &l, &t, INIT_TEXT, now_ms)?;
                match co.plan_personal(store, client_id, INIT_TEXT, recipients, &g, &c) {
                    Ok(plan) => Ok((skipped, Some((client_id, plan)))),
                    Err(error) => {
                        let _ = store.e2ee_discard_reserved(&o, client_id);
                        Err(error)
                    }
                }
            })
            .await?;
        let (skipped, Some((client_id, plan))) = planned else {
            return Ok(
                "a session already exists with every device of this contact; nothing to send",
            );
        };
        if skipped > 0 {
            report.line(&format!(
                "{skipped} device(s) already had a session and are left alone"
            ));
        }
        let mut coordinator = Arc::try_unwrap(coordinator)
            .map_err(|_| anyhow::anyhow!("the coordinator is still shared"))?;
        let prepared = match coordinator.prepare_personal(plan, &gate, &cancel).await {
            Ok(prepared) => prepared,
            Err(error) => {
                let (o, id) = (owner.clone(), client_id);
                let _ = db.call(move |s| s.e2ee_discard_reserved(&o, id)).await;
                return Err(error);
            }
        };
        // One entry per device, exactly as the SMS records carry them (the contact's devices only).
        let records: Vec<Value> = serde_json::from_slice::<Value>(prepared.exact_sms())
            .ok()
            .and_then(|v| serde_json::from_str(v["listParams"].as_str()?).ok())
            .ok_or_else(|| anyhow::anyhow!("the prepared message has no ciphertext"))?;
        let data: Vec<Value> = records
            .iter()
            .filter(|r| r["toId"].as_str() == Some(peer.as_str()))
            .map(|r| json!({"deviceId": r["platform"], "data": r["message"]}))
            .collect();
        anyhow::ensure!(!data.is_empty(), "no ciphertext for the contact's devices");
        // Durable first: the new sessions and the exact bytes are committed before anything is sent.
        let (o, g2) = (owner.clone(), gate.clone());
        db.call(move |s| {
            s.e2ee_commit_verified_send(&prepared, &g2)?;
            s.e2ee_begin_dispatch(&o, client_id, GENERATION, &g2)?;
            Ok(())
        })
        .await?;
        report.line(&format!(
            "sessions created and committed for {} device(s); sending the handshake",
            data.len()
        ));
        let request_id = self.request();
        let body = json!({"reqId": request_id, "data": {"type": 0, "uidTo": peer, "data": data}});
        let outcome = self.send_init(&request_id, body, report).await;
        let (o, id) = (owner.clone(), client_id);
        match outcome {
            Ok(()) => {
                let server_id = format!("init-peer-{now_ms}");
                db.call(move |s| {
                    s.e2ee_record_ack(&o, id, &server_id)?;
                    s.e2ee_retire_init(&o, id)
                })
                .await?;
                report.line("handshake accepted by the service");
                Ok("the handshake with the contact was sent; the contact's devices answer with a session acknowledgement")
            }
            Err(error) => {
                // Whether it arrived is not known; never resend.
                let _ = db.call(move |s| s.e2ee_send_failed(&o, id, false)).await;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use futures_util::{SinkExt, StreamExt};
    use std::io::Write;

    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    /// A genuine engine-created public bundle for the contact's device, in the service's JSON shape.
    async fn wire_bundle(platform: engine::SignalPlatform, device: u8) -> Value {
        let id = engine::AccountId::new("22").unwrap();
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
    fn seed_identity(db_path: &std::path::Path) {
        use crate::store::e2ee::VerifiedBinding;
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/e2ee/session.json"))
                .unwrap();
        let public: [u8; 33] = hex(fixture["alice_identity_public"].as_str().unwrap())
            .try_into()
            .unwrap();
        let private = hex(fixture["alice_identity_private"].as_str().unwrap());
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
    /// The service: lists contact 22 as E2EE, says it has one mobile device, hands out its bundle only for
    /// `isReInit` (like the live service) and accepts 543 with the real reply shape.
    async fn serve(
        listener: tokio::net::TcpListener,
        bundle: Value,
        expected: usize,
        inactive_second_device: bool,
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
            let envelope = match cmd {
                563 => {
                    json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"threads":[{"uid":"22"}],"waitUids":[],"uids":[],"pendingUids":[]}}}})
                }
                560 => {
                    json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"err":0,"compatible":if inactive_second_device {json!([{"deviceId":0,"uid":body["data"]["uids"][0]},{"deviceId":1,"uid":body["data"]["uids"][0]}])} else {json!([{"deviceId":0,"uid":body["data"]["uids"][0]}])},"incompatible":[]}}}})
                }
                541 => {
                    // Device 1 is inactive for a long time (the service's -7); device 0 is fine.
                    let data = if body["data"]["uids"][0]["deviceId"] == 1 {
                        json!({"err":-7,"errData":[{"deviceId":1,"err":-7,"uid":"22"}],"keys":[]})
                    } else if body["data"]["isReInit"] == 1 {
                        json!({"err":0,"errData":[],"keys":[{"uid":"22","err":0,"keys":[bundle.clone()]}]})
                    } else {
                        json!({"err":-11,"errData":[],"keys":[]})
                    };
                    json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":data}}})
                }
                // 543 answers `{data:{err,reqId,ts}}` with no inner `data`.
                _ => {
                    json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"err":0,"reqId":body["reqId"],"ts":1700000000000u64}}})
                }
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

    /// One of the contact's devices is refused with -7 (inactive); the handshake goes to the other device only.
    #[tokio::test]
    async fn an_inactive_device_is_skipped_and_the_others_still_get_the_handshake() {
        let bundle = wire_bundle(engine::SignalPlatform::Mobile, 0).await;
        let scratch =
            std::env::temp_dir().join(format!("zalo-initpeer-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&db_path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        let report = scratch.join("report.txt");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, bundle, 5, true));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.e2ee_init_peer(&db, &report, None).await;
        let seen = server.await.unwrap();
        assert_eq!(
            seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [563, 560, 541, 541, 543],
            "{seen:?}"
        );
        let handshake = &seen[4].1;
        assert_eq!(handshake["data"].as_array().unwrap().len(), 1);
        assert_eq!(handshake["data"][0]["deviceId"], 0);
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(
            text.contains("device 1: the service answered error -7; skipping this device"),
            "{text}"
        );
        assert!(text.contains("handshake accepted by the service"), "{text}");
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[tokio::test]
    async fn starting_a_conversation_sends_one_543_handshake_and_a_repeat_sends_none() {
        let bundle = wire_bundle(engine::SignalPlatform::Mobile, 0).await;
        let scratch =
            std::env::temp_dir().join(format!("zalo-initpeer-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&db_path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        let report = scratch.join("report.txt");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, bundle.clone(), 4, false));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.e2ee_init_peer(&db, &report, None).await;
        let seen = server.await.unwrap();
        assert_eq!(
            seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [563, 560, 541, 543],
            "{seen:?}"
        );
        // 541 per device with isReInit, as the original asks.
        assert_eq!(
            (
                seen[2].1["isReInit"].clone(),
                seen[2].1["uids"][0]["deviceId"].clone()
            ),
            (json!(1), json!(0))
        );
        let handshake = &seen[3].1;
        assert_eq!(
            (handshake["type"].clone(), handshake["uidTo"].clone()),
            (json!(0), json!("22"))
        );
        assert_eq!(handshake["data"].as_array().unwrap().len(), 1);
        assert_eq!(handshake["data"][0]["deviceId"], 0);
        // A fresh session opens with a type-3 prekey message.
        assert_eq!(
            STANDARD
                .decode(handshake["data"][0]["data"].as_str().unwrap())
                .unwrap()[0],
            3
        );
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(text.contains("handshake accepted by the service"), "{text}");
        let c = rusqlite::Connection::open(&db_path).unwrap();
        let states: Vec<String> = c
            .prepare("SELECT state FROM e2ee_outbox")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(states, ["committed"]);
        assert_eq!(
            c.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );

        // Repeat: the session exists, so only the lookups happen and no 543 goes out.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, bundle, 3, false));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        connection.e2ee_init_peer(&db, &report, None).await;
        let seen = server.await.unwrap();
        assert!(seen.iter().all(|(c, _)| *c != 543), "{seen:?}");
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(
            text.contains("a session already exists with every device"),
            "{text}"
        );

        // With the session in place a chat text can be prepared: encrypted for the contact's device,
        // committed, and authorized for dispatch to the E2EE sms endpoint, still without sending anything.
        let sent = super::super::e2ee_send::prepare_authorized(
            &db,
            "11",
            "22",
            "xin chào 😀",
            1_700_000_000_000,
        )
        .await
        .unwrap();
        assert_eq!(sent.devices, 1);
        assert_eq!(
            sent.authorized.parameters().path,
            "/api/e2ee/pc/message/sms"
        );
        let body: Value = serde_json::from_slice(sent.authorized.exact_json()).unwrap();
        assert_eq!(
            (body["destType"].clone(), body["threadId"].clone()),
            (json!(3), json!("22"))
        );
        let records: Vec<Value> =
            serde_json::from_str(body["listParams"].as_str().unwrap()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            (records[0]["toId"].clone(), records[0]["platform"].clone()),
            (json!("22"), json!(0))
        );
        // The committed row is `sending`: it can be acknowledged or marked unknown, never re-encrypted.
        let state: String = rusqlite::Connection::open(&db_path)
            .unwrap()
            .query_row(
                "SELECT state FROM e2ee_outbox WHERE body='xin chào 😀'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "sending");
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
