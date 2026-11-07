//! Passive live receive of end-to-end encrypted chat messages (stage 4b of `E2EE_LIVE_PLAN.md`).
//!
//! Opt-in: `ZALO_E2EE_RECEIVE` is an absolute report path. Each pushed `551` message from a peer is
//! decrypted with the engine and stored in one SQL transaction, then shown like any received message.
//! This code **never acknowledges anything** (no 544, no 545/1): the service keeps redelivering, and a
//! redelivered ciphertext is recognised and skipped. Own-device messages, non-text messages and a peer whose
//! identity differs from the trusted one are reported and left alone.
use super::e2ee_send_probe::{convert, platform, GENERATION};
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{
        personal_ciphertext_digest, personal_ciphertext_from_wire, prekey_sender,
        stored_peer_identity, Cancellation, Coordinator,
    },
    domain::Message,
    protocol::signal::DevicePlatform,
    store::e2ee::{self, GenerationGate, Receive, VerifiedBinding},
};
use anyhow::Context as _;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use zalo_signal_engine as engine;

const QUEUE: &str = "signal-personal";

pub(super) fn target() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_RECEIVE")?);
    path.is_absolute().then_some(path)
}

/// One engine for the current local epoch, reused across pushes.
pub(super) struct Receiver {
    epoch: u64,
    local: e2ee::LocalNamespace,
    coordinator: Option<Coordinator>,
    gate: Arc<GenerationGate>,
    cancel: Cancellation,
    report: Report,
}

/// Shape only: lengths, flags and field names, never an id, key or content.
fn describe(message: &Value) -> String {
    let text = |name: &str| message.get(name).and_then(Value::as_str);
    let sender = text("uidFrom").unwrap_or("");
    let cipher = text("content")
        .and_then(|c| personal_ciphertext_from_wire(c).ok())
        .map(|c| match c.message_type {
            engine::PersonalMessageType::PreKey => "prekey",
            engine::PersonalMessageType::Whisper => "ordinary",
        })
        .unwrap_or("not a personal cipher");
    let ext = text("e2ee")
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|v| {
            v.as_object().map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        if v.is_number() {
                            format!("{k}=number")
                        } else {
                            k.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
        })
        .unwrap_or_else(|| "none".into());
    format!(
        "message: msgType {} | sender id {} chars{} | platform {} | content {} chars, {cipher} | e2ee ext [{ext}] | fields {}",
        text("msgType").unwrap_or("?"),
        sender.len(),
        if sender.starts_with('0') { ", leading zero" } else { "" },
        message.pointer("/paramsExt/platformType").map(|v| v.to_string()).unwrap_or_else(|| "none".into()),
        text("content").map(str::len).unwrap_or(0),
        message.as_object().map(|m| m.len()).unwrap_or(0)
    )
}

enum Outcome {
    Stored,
    Skipped(String),
}

impl Connection {
    /// Handles the `msgs[]` of one pushed 551 frame. Best effort: every outcome is a report line.
    pub(super) async fn e2ee_receive_push(
        &mut self,
        frame: &DecodedFrame,
        db: &Database,
        events: &Events,
    ) {
        // A report file is optional (`ZALO_E2EE_RECEIVE`); without one the work is the same and nothing is logged.
        let path = self.e2ee_receive_report.clone();
        let Some(messages) = frame
            .payload
            .pointer("/data/msgs")
            .and_then(Value::as_array)
        else {
            return;
        };
        if messages.is_empty() {
            return;
        }
        let mut receiver = match self.e2ee_receive.take() {
            Some(receiver) => receiver,
            None => match Receiver::open(path.as_deref(), db, &self.account).await {
                Ok(receiver) => receiver,
                Err(error) => {
                    if let Some(mut report) = path.as_deref().and_then(Report::open) {
                        report.line(&format!("receive: cannot start: {error}"));
                    }
                    return;
                }
            },
        };
        for message in messages {
            receiver.report.line(&describe(message));
            match receiver.handle(message, db, events).await {
                Ok(Outcome::Stored) => receiver.report.line("message decrypted and stored"),
                Ok(Outcome::Skipped(why)) => {
                    receiver.report.line(&format!("message skipped: {why}"))
                }
                // `{:#}` prints the whole chain: which step failed, then why.
                Err(error) => receiver
                    .report
                    .line(&format!("message not decrypted: {error:#}")),
            }
        }
        self.e2ee_receive = Some(receiver);
    }
}

impl Receiver {
    async fn open(path: Option<&std::path::Path>, db: &Database, account: &str) -> Result<Self> {
        let mut report = match path {
            Some(path) => Report::open(path).ok_or_else(|| anyhow::anyhow!("report file"))?,
            None => Report::discard(),
        };
        let owner = account.to_owned();
        let epoch = db
            .call(move |s| s.e2ee_current_local(&owner))
            .await?
            .ok_or_else(|| anyhow::anyhow!("this client holds no E2EE key yet"))?;
        let local = e2ee::LocalNamespace {
            epoch: epoch.to_string(),
            platform: e2ee::Platform::Pc,
        };
        let coordinator = Coordinator::new(
            engine::AccountId::new(account).map_err(|_| anyhow::anyhow!("invalid account id"))?,
            GENERATION,
            convert(&local)?,
        )?;
        report.line(&format!(
            "receive: listening on local epoch {epoch}; nothing is acknowledged"
        ));
        Ok(Self {
            epoch,
            local,
            coordinator: Some(coordinator),
            gate: Arc::new(GenerationGate::new(GENERATION)?),
            cancel: Cancellation::default(),
            report,
        })
    }

    async fn handle(&mut self, message: &Value, db: &Database, events: &Events) -> Result<Outcome> {
        let owner = events.account.clone();
        let text = |name: &str| message.get(name).and_then(Value::as_str);
        // `uidFrom: "0"` marks a message this account sent from another of its own devices (the phone):
        // the sender is then this account, and the conversation is the peer in `idTo`.
        let from = text("uidFrom").ok_or_else(|| anyhow::anyhow!("no valid sender"))?;
        let digits = |value: &str| {
            !value.is_empty()
                && value.len() <= 24
                && !value.starts_with('0')
                && value.bytes().all(|b| b.is_ascii_digit())
        };
        let own_device = from == "0";
        let uid = if own_device {
            owner.clone()
        } else {
            from.to_owned()
        };
        let conversation = if own_device {
            text("idTo")
                .ok_or_else(|| anyhow::anyhow!("no recipient"))?
                .to_owned()
        } else {
            uid.clone()
        };
        anyhow::ensure!(
            digits(&uid) && digits(&conversation),
            "no valid sender or conversation"
        );
        let thread = format!("d:{conversation}");
        let Some(ext) = text("e2ee") else {
            return Ok(Outcome::Skipped(
                "not an end-to-end encrypted message".into(),
            ));
        };
        if text("msgType") != Some("webchat") {
            return Ok(Outcome::Skipped(
                "not a text message (not handled yet)".into(),
            ));
        }
        // `iid` in the extension is the registration id of the *recipient's* key (this device), as this
        // client also writes it; it is not the sender's. It tells which of our keys the message targets.
        let target_iid = serde_json::from_str::<Value>(ext)
            .ok()
            .and_then(|v| v["iid"].as_u64())
            .and_then(|v| u16::try_from(v).ok());
        let device = DevicePlatform::parse(
            message
                .pointer("/paramsExt/platformType")
                .ok_or_else(|| anyhow::anyhow!("no sender device"))?,
        )?;
        let cipher = personal_ciphertext_from_wire(text("content").unwrap_or(""))?;
        let digest = personal_ciphertext_digest(&cipher);
        let server_id = text("msgId")
            .ok_or_else(|| anyhow::anyhow!("no message id"))?
            .to_owned();
        let sent_ms: u64 = text("ts").and_then(|v| v.parse().ok()).unwrap_or(0);
        let cursor: u64 = text("actionId")
            .or_else(|| text("msgId"))
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
            .unwrap_or(1);
        let seen = {
            let owner = owner.clone();
            db.call(move |s| s.e2ee_ciphertext_seen(&owner, &digest))
                .await?
        };
        if seen {
            return Ok(Outcome::Skipped("already stored (redelivery)".into()));
        }
        // The local epoch may have been rotated meanwhile; rebuild the engine for the new one.
        let (current, local_registration) = {
            let owner = owner.clone();
            let epoch = self.epoch;
            db.call(move |s| {
                Ok((
                    s.e2ee_current_local(&owner)?,
                    s.e2ee_local_registration(&owner, epoch)?,
                ))
            })
            .await?
        };
        if current != Some(self.epoch) {
            anyhow::bail!(
                "the local key changed while running; restart to listen with the new key"
            );
        }
        if let (Some(target), Some(ours)) = (target_iid, local_registration) {
            if target != ours {
                return Ok(Outcome::Skipped(
                    "encrypted for a different key of this device (the sender used an older registration)".into(),
                ));
            }
        }

        let coordinator = Arc::new(
            self.coordinator
                .take()
                .ok_or_else(|| anyhow::anyhow!("engine busy"))?,
        );
        let planned = {
            let (o, l, g, c, co, sid, u, th) = (
                owner.clone(),
                self.local.clone(),
                self.gate.clone(),
                self.cancel.clone(),
                coordinator.clone(),
                server_id.clone(),
                uid.clone(),
                thread.clone(),
            );
            let cipher_for_plan = engine::PersonalCiphertext {
                message_type: match cipher.message_type {
                    engine::PersonalMessageType::PreKey => engine::PersonalMessageType::PreKey,
                    engine::PersonalMessageType::Whisper => engine::PersonalMessageType::Whisper,
                },
                body: cipher.body.clone(),
            };
            let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
            db.call(move |store| {
                let header = matches!(cipher_for_plan.message_type, engine::PersonalMessageType::PreKey)
                    .then(|| prekey_sender(&cipher_for_plan.body))
                    .transpose()?;
                let (namespace, registration, identity, session_ts) =
                    match store
                    .e2ee_active_peer(&o, &l, &u, platform(device))
                    .context("reading the sender's trusted binding")?
                {
                        Some(active) => {
                            let identity = match header {
                                Some((_, identity)) => identity,
                                None => stored_peer_identity(store, &o, &l, &active.namespace)
                                    .context("reading the sender's stored identity")?,
                            };
                            // A prekey header carries the sender's registration id; an ordinary message
                            // relies on the stored binding.
                            let sender_registration =
                                header.map(|(registration, _)| registration).unwrap_or(active.registration_id);
                            anyhow::ensure!(
                                active.registration_id == sender_registration
                                    && active.fingerprint == Sha256::digest(identity).to_vec(),
                                "this device presents a different identity than the trusted one (identity change is not handled yet)"
                            );
                            (active.namespace, active.registration_id, identity, active.session_timestamp)
                        }
                        None => {
                            let (registration, identity) = header.ok_or_else(|| {
                                anyhow::anyhow!("no session with this device and the message does not start one")
                            })?;
                            let namespace = store
                                .e2ee_register_peer(
                                &o,
                                &l,
                                &u,
                                None,
                                &VerifiedBinding {
                                    platform: platform(device),
                                    registration_id: registration,
                                    identity_key: identity,
                                    session_timestamp: now_ms,
                                },
                                )
                                .context("recording the sender")?;
                            (namespace, registration, identity, now_ms)
                        }
                    };
                let trust = engine::TrustDecision {
                    peer: convert(&namespace).context("building the trust decision")?,
                    registration_id: registration,
                    identity_key: identity,
                    session_timestamp: session_ts,
                    basis: engine::TrustBasis::AuthenticatedFirstUse,
                };
                co.plan_receive(store, trust, cipher_for_plan, sent_ms, &th, &sid, QUEUE, &g, &c)
                    .context("planning the decryption")
            })
            .await
        };
        let plan = match planned {
            Ok(plan) => plan,
            Err(error) => {
                self.coordinator = Arc::try_unwrap(coordinator).ok();
                return Err(error);
            }
        };
        let mut coordinator = Arc::try_unwrap(coordinator)
            .map_err(|_| anyhow::anyhow!("the coordinator is still shared"))?;
        let verified = coordinator
            .prepare_receive(plan, &self.gate, &self.cancel)
            .await;
        self.coordinator = Some(coordinator);
        let verified = verified?;
        let plaintext = String::from_utf8(verified.plaintext().to_vec())
            .map_err(|_| anyhow::anyhow!("the decrypted message is not text"))?;
        let stored = Message {
            account_id: owner.clone(),
            thread_id: thread,
            id: server_id,
            sender: uid,
            text: plaintext,
            sticker: None,
            media: None,
            sent_at_ms: i64::try_from(sent_ms).unwrap_or(0),
        };
        let gate = self.gate.clone();
        let published = db
            .call(move |store| {
                let receipt = store.e2ee_commit_receive(
                    Receive {
                        delta: verified.delta(),
                        messages: std::slice::from_ref(&stored),
                        queue: verified.queue(),
                        cursor,
                        context: verified.context().clone(),
                        ciphertext_digest: *verified.ciphertext_digest(),
                    },
                    &gate,
                )?;
                Ok(receipt.published().to_vec())
            })
            .await?;
        for (message, conversation) in published {
            events
                .emit(LiveEvent::Received {
                    account_id: owner.clone(),
                    message,
                    conversation,
                })
                .await?;
        }
        (events.repaint)();
        Ok(Outcome::Stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::live::signal_crypto::Recipient;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use futures_util::{SinkExt, StreamExt};
    use std::io::Write;

    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    fn local(platform: engine::SignalPlatform) -> engine::LocalNamespace {
        engine::LocalNamespace {
            epoch: engine::RegistrationEpoch::new(1).unwrap(),
            platform,
        }
    }
    /// This client ("11", a PC): a real engine identity installed as the active local epoch of `db_path`.
    async fn install_receiver(db_path: &std::path::Path) -> engine::PublicBundle {
        let account = engine::AccountId::new("11").unwrap();
        let worker =
            engine::Engine::new(account.clone(), 1, engine::EngineOptions::default()).unwrap();
        let result = worker
            .submit(engine::Request {
                account: account.clone(),
                generation: 1,
                operation_id: 1,
                expected_revision: 0,
                operation: engine::Operation::CreateBundle {
                    local: local(engine::SignalPlatform::Pc),
                    prekey_count: 3,
                },
                snapshot: engine::Snapshot::new(account, 0, vec![]).unwrap(),
            })
            .unwrap()
            .await
            .unwrap();
        let (_, _, _, _, output, delta) = result.into_parts();
        let engine::Output::Bundle(bundle) = output else {
            panic!("bundle")
        };
        let mut store = crate::store::MessageStore::open(db_path).unwrap();
        store
            .e2ee_register_local(
                "11",
                None,
                &VerifiedBinding {
                    platform: e2ee::Platform::Pc,
                    registration_id: bundle.registration_id,
                    identity_key: bundle.identity_key,
                    session_timestamp: 100,
                },
            )
            .unwrap();
        let connection = rusqlite::Connection::open(db_path).unwrap();
        for mutation in delta.into_mutations() {
            let engine::Mutation::Put(record) = mutation else {
                panic!("delete")
            };
            let key: e2ee::RecordKey = convert(record.key()).unwrap();
            connection
                .execute(
                    "INSERT INTO e2ee_records(account,key,value) VALUES('11',?1,?2)",
                    rusqlite::params![serde_json::to_string(&key).unwrap(), record.value()],
                )
                .unwrap();
        }
        bundle
    }
    /// A peer "101" (a phone) encrypts `texts` in order for the receiver, with the real send pipeline.
    async fn peer_sends(
        path: &std::path::Path,
        sender: &str,
        texts: &[&str],
        bundle: &engine::PublicBundle,
    ) -> Vec<String> {
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/e2ee/session.json"))
                .unwrap();
        let public: [u8; 33] = hex(fixture["alice_identity_public"].as_str().unwrap())
            .try_into()
            .unwrap();
        let private = hex(fixture["alice_identity_private"].as_str().unwrap());
        let mut store = crate::store::MessageStore::open(path).unwrap();
        let local_ns = store
            .e2ee_register_local(
                sender,
                None,
                &VerifiedBinding {
                    platform: e2ee::Platform::Mobile,
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
        let key = serde_json::to_string(&e2ee::RecordKey::LocalIdentity {
            local: local_ns.clone(),
        })
        .unwrap();
        rusqlite::Connection::open(path)
            .unwrap()
            .execute(
                "INSERT INTO e2ee_records(account,key,value) VALUES(?3,?1,?2)",
                rusqlite::params![key, value, sender],
            )
            .unwrap();
        let peer = store
            .e2ee_register_peer(
                sender,
                &local_ns,
                "11",
                None,
                &VerifiedBinding {
                    platform: e2ee::Platform::Pc,
                    registration_id: bundle.registration_id,
                    identity_key: bundle.identity_key,
                    session_timestamp: 100,
                },
            )
            .unwrap();
        let gate = e2ee::GenerationGate::new(1).unwrap();
        let cancel = Cancellation::default();
        let mut coordinator = Coordinator::new(
            engine::AccountId::new(sender).unwrap(),
            1,
            convert(&local_ns).unwrap(),
        )
        .unwrap();
        let mut wires = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let recipient = Recipient {
                trust: engine::TrustDecision {
                    peer: convert(&peer).unwrap(),
                    registration_id: bundle.registration_id,
                    identity_key: bundle.identity_key,
                    session_timestamp: 100,
                    basis: engine::TrustBasis::AuthenticatedFirstUse,
                },
                // The first message starts the session from the bundle; later ones use it.
                bundle: (index == 0).then(|| engine::PeerBundle {
                    registration_id: bundle.registration_id,
                    identity_key: bundle.identity_key,
                    signed_prekey_id: bundle.signed_prekey_id,
                    signed_prekey: bundle.signed_prekey,
                    signature: bundle.signature,
                    prekey: bundle.prekeys.first().copied(),
                }),
            };
            let id = store
                .e2ee_reserve_send(sender, &local_ns, "d:11", text, 1000 + index as i64)
                .unwrap();
            let plan = coordinator
                .plan_personal(&mut store, id, text, vec![recipient], &gate, &cancel)
                .unwrap();
            let prepared = coordinator
                .prepare_personal(plan, &gate, &cancel)
                .await
                .unwrap();
            store.e2ee_commit_verified_send(&prepared, &gate).unwrap();
            let outgoing = store.e2ee_outgoing(sender, id).unwrap();
            let payload: Value = serde_json::from_slice(&outgoing.ciphertext.unwrap()).unwrap();
            let records: Vec<Value> =
                serde_json::from_str(payload["listParams"].as_str().unwrap()).unwrap();
            wires.push(records[0]["message"].as_str().unwrap().to_owned());
            // Make the next reservation possible (the draft guard waits for an echo that never comes).
            let _ = store.e2ee_send_failed(sender, id, true);
        }
        wires
    }
    fn push_frame(msgs: Vec<Value>) -> Vec<u8> {
        wire::encode_frame(
            551,
            0,
            &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"msgs":msgs,"eesession":[],"delivereds":[]}}}),
        )
        .unwrap()
    }
    /// `iid` is the registration id of the key the message was encrypted for (the recipient's).
    fn msg(id: u64, content: &str, from: &str, kind: &str, iid: u16) -> Value {
        json!({
            "uidFrom": from, "idTo": "11", "msgId": id.to_string(), "actionId": (id + 1).to_string(),
            "ts": "1700000000123", "msgType": kind, "content": content,
            "e2ee": format!("{{\"version\":1,\"compat_version\":0,\"iid\":{iid}}}"),
            "paramsExt": {"platformType": 0}
        })
    }

    #[tokio::test]
    async fn pushed_messages_are_decrypted_and_shown_without_any_acknowledgement_and_redeliveries_are_skipped(
    ) {
        let scratch =
            std::env::temp_dir().join(format!("zalo-recv-live-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        let bundle = install_receiver(&db_path).await;
        let iid = bundle.registration_id;
        let sender_db = scratch.join("sender.sqlite");
        let wires = peer_sends(&sender_db, "101", &["xin chào 😀", "tin thứ hai"], &bundle).await;
        // Until the receiver answers, the sender keeps writing prekey messages (type 3) for the same session, so
        // the second message exercises a second decrypt that reuses the recorded sender and the session.
        assert_eq!(STANDARD.decode(&wires[1]).unwrap()[0], 3);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first, second) = (wires[0].clone(), wires[1].clone());
        let server = tokio::spawn(async move {
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
            // One push with the first message, one with the second as a phone would write it, one redelivery,
            // one repeat claimed by another sender id, one sticker, one with a different sender claiming the same ciphertext.
            for frame in [
                push_frame(vec![msg(5001, &first, "101", "webchat", iid)]),
                push_frame(vec![msg(5003, &second, "101", "webchat", iid)]),
                push_frame(vec![msg(5001, &first, "101", "webchat", iid)]),
                push_frame(vec![msg(5005, &first, "11", "webchat", iid)]),
                push_frame(vec![msg(5007, "", "101", "chat.sticker", iid)]),
            ] {
                socket
                    .send(tungstenite::Message::Binary(frame))
                    .await
                    .unwrap();
            }
            // Anything the client sends back would be an acknowledgement; there must be none.
            tokio::time::timeout(Duration::from_millis(500), socket.next())
                .await
                .is_ok_and(|m| matches!(m, Some(Ok(tungstenite::Message::Binary(_)))))
        });

        let db = Database::open(profile, "11".into()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(32);
        let events = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        let report = scratch.join("report.txt");
        connection.e2ee_receive_report = Some(report.clone());
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        for _ in 0..5 {
            let frame = loop {
                if let Some(frame) = connection.next().await.unwrap() {
                    break frame;
                }
            };
            assert_eq!(frame.cmd, 551);
            connection.process(frame, &db, &events).await.unwrap();
        }
        let sent_something = server.await.unwrap();
        assert!(!sent_something, "nothing may be acknowledged");

        // Exactly two messages were shown, in order, with their real text.
        let mut shown = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let LiveEvent::Received {
                message,
                conversation,
                ..
            } = event
            {
                assert_eq!(conversation.id, "d:101");
                shown.push(message.text);
            }
        }
        assert_eq!(shown, ["xin chào 😀", "tin thứ hai"]);
        let text = std::fs::read_to_string(&report).unwrap();
        assert_eq!(
            text.matches("message decrypted and stored").count(),
            2,
            "{text}"
        );
        assert!(text.contains("already stored (redelivery)"), "{text}");
        // Both repeated ciphertexts (even under another sender id) are recognised and skipped.
        assert_eq!(
            text.matches("already stored (redelivery)").count(),
            2,
            "{text}"
        );
        assert!(text.contains("not a text message"), "{text}");
        std::fs::remove_dir_all(scratch).unwrap();
    }

    /// A message this account sent from its own phone arrives with `uidFrom "0"` and the conversation in
    /// `idTo`; it is decrypted with the session to the phone and stored in that conversation as this account's.
    #[tokio::test]
    async fn a_message_sent_from_the_accounts_own_phone_is_stored_in_the_peer_conversation() {
        let scratch =
            std::env::temp_dir().join(format!("zalo-recv-own-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        let bundle = install_receiver(&db_path).await;
        let iid = bundle.registration_id;
        // The phone is another device of account "11" (its own store); the conversation is "d:11" here.
        let wires = peer_sends(
            &scratch.join("phone.sqlite"),
            "11",
            &["gửi từ điện thoại"],
            &bundle,
        )
        .await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let wire_text = wires[0].clone();
        let server = tokio::spawn(async move {
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
            let mut own = msg(6001, &wire_text, "0", "webchat", iid);
            own["idTo"] = json!("11");
            socket
                .send(tungstenite::Message::Binary(push_frame(vec![own])))
                .await
                .unwrap();
            let _ = tokio::time::timeout(Duration::from_millis(300), socket.next()).await;
        });
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let events = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        let report = scratch.join("report.txt");
        connection.e2ee_receive_report = Some(report.clone());
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        let frame = loop {
            if let Some(frame) = connection.next().await.unwrap() {
                break frame;
            }
        };
        connection.process(frame, &db, &events).await.unwrap();
        server.await.unwrap();
        let Some(LiveEvent::Received {
            message,
            conversation,
            ..
        }) = rx.try_recv().ok()
        else {
            panic!(
                "expected the message to be shown: {}",
                std::fs::read_to_string(&report).unwrap_or_default()
            )
        };
        assert_eq!(
            (
                message.thread_id.as_str(),
                message.sender.as_str(),
                message.text.as_str()
            ),
            ("d:11", "11", "gửi từ điện thoại")
        );
        assert_eq!(conversation.id, "d:11");
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(text.contains("message decrypted and stored"), "{text}");
        assert!(text.contains("sender id 1 chars"), "{text}");
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
