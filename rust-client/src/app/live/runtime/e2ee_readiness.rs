//! Fresh, account-bound device coverage for a single personal send.
//! Compatibility alone does not enable encryption; the complete 563 snapshot does.
use super::e2ee_send_probe::{platform, GENERATION};
use super::*;
use crate::{
    protocol::signal::{
        self, BundleOptions, DevicePlatform, DeviceSelection, Scope, SignalRequest,
    },
    store::{e2ee, MessageStore},
};
use anyhow::ensure;
use sha2::{Digest, Sha256};

const WAIT: Duration = Duration::from_secs(12);
const READ_BUDGET: Duration = Duration::from_secs(30);
const PAGES: usize = 20;

struct Device {
    uid: String,
    bundle: signal::PublicBundle,
    metadata_known: bool,
    initialized: bool,
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::e2ee_init_peer::tests::{seed_identity, wire_bundle};
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::{io::Write, sync::Arc};

    #[derive(Clone, Copy)]
    pub(crate) enum Mode {
        Normal,
        UnknownIid,
        OmittedWeb,
        RefuseWeb(i64),
        OwnMissing,
        OwnPresent,
        StateDuringInit,
        QueryIncomingFailure,
    }

    async fn frame(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        cmd: u16,
        payload: Value,
    ) {
        socket
            .send(tungstenite::Message::Binary(
                wire::encode_frame(cmd, 0, &json!({"error_code":0,"encrypt":0,"data":payload}))
                    .unwrap(),
            ))
            .await
            .unwrap();
    }

    pub(crate) async fn server(
        listener: tokio::net::TcpListener,
        bundles: Vec<Value>,
        mode: Mode,
    ) -> Vec<(u16, Value)> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":510,"subCmd":0}]}}"#)
            .unwrap();
        socket.send(tungstenite::Message::Binary(wire::encode_frame(1, 0,
            &json!({"error_code":0,"key":STANDARD.encode([7u8;32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())})).unwrap())).await.unwrap();
        let mut seen = Vec::new();
        while let Some(Ok(tungstenite::Message::Binary(bytes))) = socket.next().await {
            let cmd = u16::from_le_bytes([bytes[1], bytes[2]]);
            let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
            seen.push((cmd, body["data"].clone()));
            let data = match cmd {
                563 => {
                    if matches!(mode, Mode::QueryIncomingFailure)
                        && seen.iter().filter(|(command, _)| *command == 563).count() == 2
                    {
                        frame(&mut socket,501,json!({"error_code":0,"data":{"msgs":[{"uidFrom":"22","idTo":"11","msgId":"99003","cliMsgId":"99004","ts":"1700000000000","msgType":"webchat","content":"Tin kiểm thử lỗi lưu trữ"}]}})).await;
                    }
                    let devices:Vec<_> = bundles.iter().filter(|b| !matches!(mode,Mode::OmittedWeb) || b["deviceId"] != 1).map(|bundle| {
                        json!({"di":bundle["deviceId"],"iid":if matches!(mode,Mode::UnknownIid) && bundle["deviceId"]==0 {json!(0)}else{bundle["preKey"]["iKey"]["id"].clone()}})
                    }).collect();
                    json!({"threads":[{"uid":"22","devices":devices}],"uids":[],"waitUids":[],"pendingUids":[]})
                }
                560 => {
                    json!({"compatible":[{"uid":"22","deviceId":0},{"uid":"22","deviceId":1}],"incompatible":[]})
                }
                541 => {
                    let uid = body.pointer("/data/uids/0/uid").unwrap().clone();
                    let device = body
                        .pointer("/data/uids/0/deviceId")
                        .unwrap()
                        .as_i64()
                        .unwrap();
                    if let Mode::RefuseWeb(code) = mode {
                        if uid == "22" && device == 1 {
                            frame(&mut socket,cmd,json!({"error_code":0,"data":{"reqId":body["reqId"],"data":{"err":code,"keys":[]}}})).await;
                            continue;
                        }
                    }
                    let keys: Vec<Value> = if uid == "11" {
                        if matches!(mode, Mode::OwnMissing | Mode::OwnPresent) {
                            vec![bundles[1].clone()]
                        } else {
                            vec![]
                        }
                    } else {
                        bundles
                            .iter()
                            .filter(|bundle| {
                                device == -1 || bundle["deviceId"].as_i64() == Some(device)
                            })
                            .cloned()
                            .collect()
                    };
                    json!({"keys":[{"uid":uid,"keys":keys}]})
                }
                543 => {
                    // An ordinary incoming message must survive even inside the init wait.
                    frame(&mut socket,501,json!({"error_code":0,"data":{"msgs":[{"uidFrom":"22","idTo":"11","msgId":"99001","cliMsgId":"99002","ts":"1700000000000","msgType":"webchat","content":"Tin xen giữa tổng hợp"}]}})).await;
                    if matches!(mode, Mode::StateDuringInit) {
                        frame(&mut socket, 548, json!({"error_code":0,"data":{}})).await;
                        continue;
                    }
                    frame(
                        &mut socket,
                        543,
                        json!({"error_code":0,"data":{"reqId":body["reqId"],"err":0}}),
                    )
                    .await;
                    continue;
                }
                _ => panic!("Unexpected outgoing command {cmd}"),
            };
            frame(
                &mut socket,
                cmd,
                json!({"error_code":0,"data":{"reqId":body["reqId"],"data":data}}),
            )
            .await;
        }
        seen
    }

    pub(super) async fn check(
        db: &Database,
        bundles: &[Value],
        mode: Mode,
        sequence: u64,
    ) -> (Result<Readiness>, Vec<(u16, Value)>, Vec<LiveEvent>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(server(listener, bundles.to_vec(), mode));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        let (tx, mut rx) = mpsc::channel(32);
        let events = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let result = connection.send_readiness(db, &events, "22", sequence).await;
        drop(connection);
        let seen = server.await.unwrap();
        let mut received = Vec::new();
        while let Ok(event) = rx.try_recv() {
            received.push(event);
        }
        (result, seen, received)
    }

    fn state(path: &std::path::Path) -> (Vec<(String, Vec<u8>)>, i64) {
        let conn = rusqlite::Connection::open(path).unwrap();
        let records = conn
            .prepare("SELECT key,value FROM e2ee_records WHERE account='11' ORDER BY key")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let outbox = conn
            .query_row("SELECT count(*) FROM e2ee_outbox", [], |row| row.get(0))
            .unwrap();
        (records, outbox)
    }

    pub(crate) fn parsed_bundle(
        uid: &str,
        device: DevicePlatform,
        value: Value,
    ) -> signal::PublicBundle {
        let scope = Scope::new("11", GENERATION).unwrap();
        let request = signal::get_bundle(
            &scope,
            98,
            uid,
            DeviceSelection::One(device),
            BundleOptions {
                reinit: false,
                notify: false,
                auto_upgrade: false,
            },
        )
        .unwrap();
        let reply = json!({"error_code":0,"data":{"reqId":request.request_id(),"data":{"keys":[{"uid":uid,"keys":[value]}]}}});
        signal::bundles_reply(&request, &scope, &reply, uid, DeviceSelection::One(device))
            .unwrap()
            .bundles
            .into_iter()
            .next()
            .unwrap()
    }

    async fn session_bytes(db: &Database, device: e2ee::Platform) -> Vec<u8> {
        db.call(move |store| {
            let local = e2ee::LocalNamespace {
                epoch: store.e2ee_current_local("11")?.unwrap().to_string(),
                platform: e2ee::Platform::Pc,
            };
            let peer = store
                .e2ee_active_peer("11", &local, "22", device)?
                .unwrap()
                .namespace;
            Ok(store
                .e2ee_snapshot("11", &[e2ee::RecordKey::Session { local, peer }])?
                .records
                .into_iter()
                .next()
                .unwrap()
                .value)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn readiness_initializes_missing_devices_and_checks_cached_devices_before_planning() {
        let bundles = vec![
            wire_bundle(engine_platform(0), 0).await,
            wire_bundle(engine_platform(1), 1).await,
        ];
        let scratch =
            std::env::temp_dir().join(format!("zalo-readiness-{:x}", rand::random::<u64>()));
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (ready, seen, events) = check(&db, &bundles[..1], Mode::Normal, 20_000_000).await;
        let ready = ready.unwrap();
        assert_eq!(seen.iter().filter(|(cmd, _)| *cmd == 543).count(), 1);
        assert_eq!(
            seen.iter().find(|(cmd, _)| *cmd == 543).unwrap().1["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, LiveEvent::Received { .. }))
                .count(),
            1
        );
        let first = super::super::e2ee_send::prepare_checked(
            &db,
            "11",
            "22",
            "Nội dung tổng hợp",
            1700000000100,
            Arc::new(ready),
        )
        .await
        .unwrap();
        assert_eq!(first.devices, 1);
        let mobile_before = session_bytes(&db, e2ee::Platform::Mobile).await;
        let (ready, seen, _) = check(&db, &bundles, Mode::Normal, 20_000_128).await;
        assert!(
            session_bytes(&db, e2ee::Platform::Mobile).await == mobile_before,
            "Initializing a missing web device must not rewrite the good mobile session"
        );
        let init = &seen
            .iter()
            .find(|(cmd, _)| *cmd == 543)
            .expect("Only the missing web device needs initialization")
            .1["data"];
        assert_eq!(init.as_array().unwrap().len(), 1);
        assert_eq!(init[0]["deviceId"], 1);
        let prepared = super::super::e2ee_send::prepare_checked(
            &db,
            "11",
            "22",
            "Second synthetic body",
            1700000000101,
            Arc::new(ready.unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(prepared.devices, 2);
        for (index, mode) in [
            Mode::UnknownIid,
            Mode::OmittedWeb,
            Mode::RefuseWeb(-8),
            Mode::OwnMissing,
        ]
        .into_iter()
        .enumerate()
        {
            let before = state(&path);
            let (result, seen, _) =
                check(&db, &bundles, mode, 20_001_000 + index as u64 * 128).await;
            assert!(result.is_err());
            assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
            assert!(
                state(&path) == before,
                "Rejected readiness must not mutate keys or the outbox"
            );
        }
        let mut replacement = bundles.clone();
        replacement[0] = wire_bundle(engine_platform(0), 0).await;
        let before = state(&path);
        let (changed, seen, _) = check(&db, &replacement, Mode::Normal, 20_001_640).await;
        assert!(changed.is_err());
        assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
        assert!(
            state(&path) == before,
            "Changed identities must never silently replace trusted records"
        );
        let web_before = session_bytes(&db, e2ee::Platform::Web).await;
        let (inactive, seen, _) = check(&db, &bundles, Mode::RefuseWeb(-7), 20_001_800).await;
        assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
        let inactive = super::super::e2ee_send::prepare_checked(
            &db,
            "11",
            "22",
            "Inactive device synthetic body",
            1700000000150,
            Arc::new(inactive.unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(inactive.devices, 1);
        assert!(
            session_bytes(&db, e2ee::Platform::Web).await == web_before,
            "Inactive device sessions are retained"
        );
        let (ready, seen, _) = check(&db, &bundles, Mode::Normal, 20_002_000).await;
        assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
        let mut ready = ready.unwrap();
        let body: Value = serde_json::from_slice(prepared.authorized.exact_json()).unwrap();
        let mut records: Vec<Value> =
            serde_json::from_str(body["listParams"].as_str().unwrap()).unwrap();
        let mut ext: Value = serde_json::from_str(records[0]["e2eeExt"].as_str().unwrap()).unwrap();
        ext["iid"] = json!(0);
        records[0]["e2eeExt"] = json!(serde_json::to_string(&ext).unwrap());
        let mut wrong = body;
        wrong["listParams"] = json!(serde_json::to_string(&records).unwrap());
        assert!(ready
            .check_prepared(&serde_json::to_vec(&wrong).unwrap())
            .is_err());
        ready.owner = "33".into();
        let before = state(&path);
        assert!(super::super::e2ee_send::prepare_checked(
            &db,
            "11",
            "22",
            "Must remain unreserved",
            1700000000200,
            Arc::new(ready)
        )
        .await
        .is_err());
        assert!(state(&path) == before);
        // Initialize an own web session through the same reviewed 543 pipeline,
        // then require the actual checked send to include its copy after peers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(server(listener, bundles.clone(), Mode::Normal));
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        connection.next().await.unwrap();
        let (tx, _rx) = mpsc::channel(32);
        let events = Events {
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let local = e2ee::LocalNamespace {
            epoch: "1".into(),
            platform: e2ee::Platform::Pc,
        };
        connection
            .handshake_checked_devices(
                &db,
                &events,
                &local,
                "11",
                signal::FetchedBundles {
                    peer: "11".into(),
                    bundles: vec![parsed_bundle("11", DevicePlatform::Web, bundles[1].clone())],
                },
                tokio::time::Instant::now() + READ_BUDGET,
            )
            .await
            .unwrap();
        drop(connection);
        server.await.unwrap();
        let (ready, seen, _) = check(&db, &bundles, Mode::OwnPresent, 20_003_000).await;
        assert!(seen.iter().all(|(cmd, _)| *cmd != 543));
        let proof = Arc::new(ready.unwrap());
        let copied = super::super::e2ee_send::prepare_checked(
            &db,
            "11",
            "22",
            "Include own web copy",
            1700000000300,
            proof.clone(),
        )
        .await
        .unwrap();
        assert_eq!(copied.devices, 3);
        let body: Value = serde_json::from_slice(copied.authorized.exact_json()).unwrap();
        let records: Vec<Value> =
            serde_json::from_str(body["listParams"].as_str().unwrap()).unwrap();
        // The engine orders complete namespaces, so own account 11 precedes peer 22.
        // Authorization checks routed coverage without changing these exact bytes.
        assert_eq!(records[0]["toId"], json!("11"));
        let addresses = records
            .iter()
            .map(|record| {
                (
                    record["toId"].as_str().unwrap(),
                    record["platform"].as_u64().unwrap(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            addresses,
            [("22", 0), ("22", 1), ("11", 1)].into_iter().collect()
        );
        let mut duplicate = body.clone();
        let mut duplicated = records.clone();
        duplicated[1] = duplicated[0].clone();
        duplicate["listParams"] = json!(serde_json::to_string(&duplicated).unwrap());
        // Duplicating one route keeps cardinality but loses another required recipient.
        // The same checked snapshot must refuse it before any durable cipher commit.
        assert!(proof
            .check_prepared(&serde_json::to_vec(&duplicate).unwrap())
            .is_err());
        let mut missing = body;
        missing["listParams"] = json!(serde_json::to_string(&records[..2]).unwrap());
        assert!(proof
            .check_prepared(&serde_json::to_vec(&missing).unwrap())
            .is_err());
        drop(db);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn readiness_state_event_during_init_never_authorizes_a_chat_send() {
        let bundles = vec![
            wire_bundle(engine_platform(0), 0).await,
            wire_bundle(engine_platform(1), 1).await,
        ];
        let scratch =
            std::env::temp_dir().join(format!("zalo-readiness-state-{:x}", rand::random::<u64>()));
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let path = profile.paths.data_dir.join("messages.sqlite");
        seed_identity(&path);
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (result, seen, events) = check(&db, &bundles, Mode::StateDuringInit, 20_003_000).await;
        assert!(result.is_err());
        assert_eq!(seen.iter().filter(|(cmd, _)| *cmd == 543).count(), 1);
        assert!(events
            .iter()
            .any(|event| matches!(event, LiveEvent::Received { .. })));
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM e2ee_outbox WHERE state='unknown'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM e2ee_outbox", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(db);
        drop(conn);
        let _ = std::fs::remove_dir_all(scratch);
    }

    fn engine_platform(device: u8) -> zalo_signal_engine::SignalPlatform {
        if device == 0 {
            zalo_signal_engine::SignalPlatform::Mobile
        } else {
            zalo_signal_engine::SignalPlatform::Web
        }
    }
}

/// Created only after fresh scoped metadata and identity checks. Rechecked in the
/// same database job that plans the encryption, before reserving the chat outbox.
pub(super) struct Readiness {
    owner: String,
    peer: String,
    pub(super) local: e2ee::LocalNamespace,
    devices: Vec<Device>,
    inactive: Vec<DevicePlatform>,
    own_pc: Option<signal::PublicBundle>,
}

impl Readiness {
    pub(super) fn recipients(
        &self,
        store: &mut MessageStore,
        owner: &str,
        peer: &str,
    ) -> Result<Vec<crate::app::live::signal_crypto::Recipient>> {
        ensure!(
            owner == self.owner && peer == self.peer,
            "Encrypted readiness scope changed"
        );
        ensure!(
            store
                .e2ee_current_local(owner)?
                .map(|epoch| epoch.to_string())
                .as_deref()
                == Some(self.local.epoch.as_str()),
            "Encrypted readiness epoch changed"
        );
        let missing = self.missing(store)?;
        ensure!(
            missing.is_empty(),
            "An expected encrypted device has no session"
        );
        self.devices
            .iter()
            .map(|device| {
                let recipient = super::e2ee_send::existing_recipient(
                    store,
                    owner,
                    &self.local,
                    &device.uid,
                    device.bundle.device,
                )?
                .ok_or_else(|| anyhow::anyhow!("An expected encrypted device has no session"))?;
                ensure!(
                    recipient.trust.registration_id == device.bundle.registration_id
                        && recipient.trust.identity_key == *device.bundle.identity.bytes(),
                    "The stored encrypted identity differs from the verified device"
                );
                Ok(recipient)
            })
            .collect()
    }

    fn missing(&self, store: &mut MessageStore) -> Result<Vec<DevicePlatform>> {
        ensure!(
            !store.e2ee_peer_init_pending(&self.owner, &self.local, &self.peer)?
                && !store.e2ee_peer_init_pending(&self.owner, &self.local, &self.owner)?,
            "An encrypted initialization is not durably confirmed"
        );
        if let Some(pc) = &self.own_pc {
            store.e2ee_validate_local_identity(
                &self.owner,
                &self.local,
                pc.registration_id,
                pc.identity.bytes(),
            )?;
        }
        let mut missing = Vec::new();
        for device in &self.devices {
            let active = store.e2ee_active_peer(
                &self.owner,
                &self.local,
                &device.uid,
                platform(device.bundle.device),
            )?;
            let has_session = if let Some(active) = active {
                ensure!(active.registration_id == device.bundle.registration_id
                    && active.fingerprint == Sha256::digest(device.bundle.identity.bytes()).as_slice(),
                    "An encrypted device identity or registration changed; confirmation is required");
                !store
                    .e2ee_snapshot(
                        &self.owner,
                        &[e2ee::RecordKey::Session {
                            local: self.local.clone(),
                            peer: active.namespace,
                        }],
                    )?
                    .records
                    .is_empty()
            } else {
                false
            };
            ensure!(
                !has_session || device.metadata_known || device.initialized,
                "An encrypted device has stale metadata and needs session renewal"
            );
            if !has_session {
                ensure!(
                    device.uid != self.owner,
                    "An own device needs encrypted session initialization"
                );
                missing.push(device.bundle.device);
            }
        }
        // An omitted cached device is not proof of removal. Keep its session and
        // require explicit renewal/inactivity handling rather than silently omitting a copy.
        for uid in [&self.peer, &self.owner] {
            for device in [
                DevicePlatform::Mobile,
                DevicePlatform::Web,
                DevicePlatform::Pc,
            ] {
                if uid == &self.owner && device == DevicePlatform::Pc {
                    continue;
                }
                if self
                    .devices
                    .iter()
                    .any(|entry| &entry.uid == uid && entry.bundle.device == device)
                    || (uid == &self.peer && self.inactive.contains(&device))
                {
                    continue;
                }
                if let Some(active) =
                    store.e2ee_active_peer(&self.owner, &self.local, uid, platform(device))?
                {
                    ensure!(
                        store
                            .e2ee_snapshot(
                                &self.owner,
                                &[e2ee::RecordKey::Session {
                                    local: self.local.clone(),
                                    peer: active.namespace,
                                }]
                            )?
                            .records
                            .is_empty(),
                        "A cached encrypted device is absent from fresh metadata"
                    );
                }
            }
        }
        Ok(missing)
    }

    /// Check actual engine remote registration IDs before committing its prepared
    /// ratchet delta. A trusted binding alone cannot prove the opaque session IID.
    pub(super) fn check_prepared(&self, bytes: &[u8]) -> Result<()> {
        let outer: Value = serde_json::from_slice(bytes)?;
        ensure!(
            outer["threadId"].as_str() == Some(self.peer.as_str()),
            "Encrypted payload thread changed"
        );
        let records: Vec<Value> = serde_json::from_str(
            outer["listParams"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing encrypted device records"))?,
        )?;
        ensure!(
            records.len() == self.devices.len(),
            "Encrypted device coverage changed"
        );
        let mut seen = std::collections::BTreeSet::new();
        for record in &records {
            let uid = record["toId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing encrypted recipient"))?;
            let platform = record["platform"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing encrypted device platform"))?;
            ensure!(
                seen.insert((uid, platform)),
                "Duplicate encrypted recipient"
            );
            let device = self
                .devices
                .iter()
                .find(|device| {
                    device.uid == uid && u64::from(device.bundle.device.wire()) == platform
                })
                .ok_or_else(|| anyhow::anyhow!("Unexpected encrypted device address"))?;
            let ext: Value = serde_json::from_str(
                record["e2eeExt"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing encrypted device registration"))?,
            )?;
            ensure!(
                ext["iid"].as_u64() == Some(u64::from(device.bundle.registration_id)),
                "The encrypted session registration differs from the verified device"
            );
        }
        Ok(())
    }
}

impl Connection {
    pub(super) async fn send_checked_init(
        &mut self,
        request_id: &str,
        body: Value,
        db: &Database,
        events: &Events,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Encrypted readiness expired before initialization"
        );
        tokio::time::timeout_at(deadline, self.send(543, 0, body))
            .await
            .map_err(|_| anyhow::anyhow!("Encrypted initialization timed out"))??;
        let deadline = deadline.min(tokio::time::Instant::now() + WAIT);
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .map_err(|_| anyhow::anyhow!("Encrypted initialization timed out"))??;
            let Some(frame) = frame else { continue };
            if frame.cmd == 543
                && frame.payload.pointer("/data/reqId").and_then(Value::as_str) == Some(request_id)
            {
                wire::service_code(&frame.payload)?;
                let error = frame
                    .payload
                    .pointer("/data/err")
                    .or_else(|| frame.payload.pointer("/data/data/err"));
                ensure!(
                    matches!(error, None | Some(Value::Null))
                        || error.and_then(Value::as_i64) == Some(0),
                    "The service refused encrypted initialization"
                );
                return Ok(());
            }
            ensure!(
                ![1, 543, 544, 548, 561, 580, 581, 582].contains(&frame.cmd),
                "Encrypted device state changed during initialization"
            );
            Box::pin(self.process(frame, db, events)).await?;
        }
    }

    async fn readiness_reply(
        &mut self,
        request: &SignalRequest,
        scope: &Scope,
        db: &Database,
        events: &Events,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        let command = request.command() as u16;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Encrypted device verification budget expired"
        );
        tokio::time::timeout_at(deadline, self.send(command, 0, request.wire_data(scope)?))
            .await
            .map_err(|_| anyhow::anyhow!("Encrypted device verification timed out"))??;
        let deadline = deadline.min(tokio::time::Instant::now() + WAIT);
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .map_err(|_| anyhow::anyhow!("Encrypted device verification timed out"))??;
            let Some(frame) = frame else { continue };
            if frame.cmd == command
                && frame.payload.pointer("/data/reqId").and_then(Value::as_str)
                    == Some(request.request_id())
            {
                return Ok(frame.payload);
            }
            ensure!(
                ![1, 543, 544, 548, 561, 580, 581, 582].contains(&frame.cmd),
                "Encrypted device state changed during verification"
            );
            // Queries must not discard incoming chat, queue progress or PONGs.
            Box::pin(self.process(frame, db, events)).await?;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn readiness_bundle(
        &mut self,
        scope: &Scope,
        uid: &str,
        selection: DeviceSelection,
        sequence: u64,
        reinit: bool,
        db: &Database,
        events: &Events,
        deadline: tokio::time::Instant,
    ) -> Result<signal::FetchedBundles> {
        let request = signal::get_bundle(
            scope,
            sequence,
            uid,
            selection,
            BundleOptions {
                reinit,
                notify: false,
                auto_upgrade: false,
            },
        )?;
        let reply = self
            .readiness_reply(&request, scope, db, events, deadline)
            .await?;
        Ok(signal::bundles_reply(
            &request, scope, &reply, uid, selection,
        )?)
    }

    pub(super) async fn send_readiness(
        &mut self,
        db: &Database,
        events: &Events,
        peer: &str,
        sequence: u64,
    ) -> Result<Readiness> {
        ensure!(
            self.account == events.account,
            "Encrypted readiness account changed"
        );
        ensure!(
            self.queues.is_empty(),
            "Offline history is still being synchronized"
        );
        let owner = self.account.clone();
        ensure!(
            peer != owner,
            "Encrypted note-to-self readiness is not supported"
        );
        let o = owner.clone();
        let p = peer.to_owned();
        let local = db
            .call(move |store| {
                let epoch = store
                    .e2ee_current_local(&o)?
                    .ok_or_else(|| anyhow::anyhow!("This client holds no encrypted identity"))?;
                let local = e2ee::LocalNamespace {
                    epoch: epoch.to_string(),
                    platform: e2ee::Platform::Pc,
                };
                ensure!(
                    !store.e2ee_peer_init_pending(&o, &local, &p)?
                        && !store.e2ee_peer_init_pending(&o, &local, &o)?,
                    "An encrypted initialization is not durably confirmed"
                );
                Ok(local)
            })
            .await?;
        let scope = Scope::new(&owner, GENERATION)?;
        let deadline = tokio::time::Instant::now() + READ_BUDGET;
        let mut sequence = sequence;
        let mut after = None;
        let mut seen = Vec::new();
        let mut metadata = None;
        let mut complete = false;
        for _ in 0..PAGES {
            let request = signal::metadata(&scope, sequence, after.as_ref())?;
            sequence += 1;
            let reply = self
                .readiness_reply(&request, &scope, db, events, deadline)
                .await?;
            let page = signal::target_device_page(&request, &scope, &reply, peer)?;
            ensure!(
                !page.uncertain,
                "Encrypted device initialization is still pending"
            );
            if let Some(devices) = page.devices {
                ensure!(metadata.is_none(), "Duplicate encrypted target metadata");
                metadata = Some(devices);
            }
            match page.next {
                None => {
                    complete = true;
                    break;
                }
                Some(cursor) => {
                    ensure!(
                        !seen.contains(&cursor),
                        "Encrypted device pagination repeated"
                    );
                    seen.push(cursor.clone());
                    after = Some(cursor);
                }
            }
        }
        ensure!(complete, "Encrypted device metadata is incomplete");
        let mut metadata = metadata
            .filter(|devices: &Vec<signal::ReadinessDevice>| !devices.is_empty())
            .ok_or_else(|| anyhow::anyhow!("No enabled encrypted device metadata"))?;
        metadata.sort_by_key(|device| device.device.wire());
        let request = signal::compatible(&scope, sequence, peer)?;
        sequence += 1;
        let reply = self
            .readiness_reply(&request, &scope, db, events, deadline)
            .await?;
        let compatible = signal::compatible_reply(&request, &scope, &reply, peer)?;
        ensure!(
            metadata
                .iter()
                .all(|device| compatible.contains(&device.device)),
            "An enabled encrypted device is not compatible"
        );
        let mut devices = Vec::new();
        let mut inactive = Vec::new();
        for expected in metadata {
            let result = self
                .readiness_bundle(
                    &scope,
                    peer,
                    DeviceSelection::One(expected.device),
                    sequence,
                    false,
                    db,
                    events,
                    deadline,
                )
                .await;
            sequence += 1;
            let found = match result {
                Err(error)
                    if matches!(
                        error.downcast_ref::<signal::SignalError>(),
                        Some(signal::SignalError::Service(-7))
                    ) =>
                {
                    inactive.push(expected.device);
                    continue;
                }
                other => other?,
            };
            ensure!(
                found.bundles.len() == 1,
                "An enabled encrypted device returned no bundle"
            );
            let bundle = found
                .bundles
                .into_iter()
                .next()
                .expect("one checked bundle");
            ensure!(
                bundle.registration_id > 0,
                "Encrypted device registration is unknown"
            );
            ensure!(
                expected.iid.is_none_or(|iid| iid == bundle.registration_id),
                "Encrypted device metadata and bundle registration differ"
            );
            devices.push(Device {
                uid: peer.into(),
                bundle,
                metadata_known: expected.iid.is_some(),
                initialized: false,
            });
        }
        ensure!(!devices.is_empty(), "No active encrypted peer device");
        let own = self
            .readiness_bundle(
                &scope,
                &owner,
                DeviceSelection::All,
                sequence,
                false,
                db,
                events,
                deadline,
            )
            .await?;
        sequence += 1;
        let mut own_pc = None;
        for bundle in own.bundles {
            ensure!(
                bundle.registration_id > 0,
                "Own encrypted device registration is unknown"
            );
            if bundle.device == DevicePlatform::Pc {
                own_pc = Some(bundle);
                continue;
            }
            devices.push(Device {
                uid: owner.clone(),
                bundle,
                metadata_known: true,
                initialized: false,
            });
        }
        let ready = Readiness {
            owner: owner.clone(),
            peer: peer.into(),
            local,
            devices,
            inactive,
            own_pc,
        };
        let ready = std::sync::Arc::new(ready);
        let checked = ready.clone();
        let missing = db.call(move |store| checked.missing(store)).await?;
        let mut ready = std::sync::Arc::try_unwrap(ready)
            .map_err(|_| anyhow::anyhow!("Encrypted readiness is still shared"))?;
        if !missing.is_empty() {
            let mut bundles = Vec::new();
            for device in &missing {
                let found = self
                    .readiness_bundle(
                        &scope,
                        peer,
                        DeviceSelection::One(*device),
                        sequence,
                        true,
                        db,
                        events,
                        deadline,
                    )
                    .await?;
                sequence += 1;
                ensure!(
                    found.bundles.len() == 1,
                    "A missing encrypted device returned no bundle"
                );
                let bundle = found
                    .bundles
                    .into_iter()
                    .next()
                    .expect("one checked bundle");
                let original = ready
                    .devices
                    .iter()
                    .find(|entry| entry.uid == peer && entry.bundle.device == *device)
                    .expect("missing device belongs to verified set");
                ensure!(
                    bundle.registration_id == original.bundle.registration_id
                        && bundle.identity.bytes() == original.bundle.identity.bytes(),
                    "An encrypted identity changed before initialization"
                );
                bundles.push(bundle);
            }
            self.handshake_checked_devices(
                db,
                events,
                &ready.local,
                peer,
                signal::FetchedBundles {
                    peer: peer.into(),
                    bundles,
                },
                deadline,
            )
            .await?;
            for device in &mut ready.devices {
                if device.uid == peer && missing.contains(&device.bundle.device) {
                    device.initialized = true;
                }
            }
        }
        let ready = std::sync::Arc::new(ready);
        let checked = ready.clone();
        let o = owner.clone();
        let p = peer.to_owned();
        db.call(move |store| checked.recipients(store, &o, &p).map(|_| ()))
            .await?;
        std::sync::Arc::try_unwrap(ready)
            .map_err(|_| anyhow::anyhow!("Encrypted readiness is still shared"))
    }
}

#[cfg(test)]
#[path = "e2ee_readiness/engine_tests.rs"]
mod engine_tests;
