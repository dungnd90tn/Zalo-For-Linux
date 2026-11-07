//! Dry run of an end-to-end encrypted send (stage 3 of `E2EE_LIVE_PLAN.md`). Nothing is sent.
//!
//! Enabled only when `ZALO_E2EE_SEND_PROBE` is an absolute report path. The peer is chosen from this
//! account's own E2EE list: `ZALO_E2EE_PEER` may be part of the contact's name (as shown in the app,
//! alias included), or a numeric UID; unset means the first E2EE contact. Once per process it fetches the real bundles of that peer and of this account's
//! other devices (command 541, read-only), records the peers' identities on first use, and lets the
//! real engine encrypt a short probe text for every device. That proves the engine accepts the live
//! service's bundles (signatures, key derivation). The encrypted result is discarded: it is not
//! committed, not stored as a message and never dispatched; the reserved draft row is removed.
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{Cancellation, Coordinator, Recipient},
    protocol::signal::{self, BundleOptions, DevicePlatform, DeviceSelection, Scope},
    store::e2ee::{self, GenerationGate, VerifiedBinding},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use zalo_signal_engine as engine;

pub(super) const GENERATION: u64 = 1;
const PROBE_TEXT: &str = "e2ee probe";
const WAIT: Duration = Duration::from_secs(12);
const MAX_TRIES: usize = 10;

/// The report path and the optional peer selector (a name fragment or a numeric UID).
pub(super) fn target() -> Option<(std::path::PathBuf, Option<String>)> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_SEND_PROBE")?);
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

pub(super) fn platform(device: DevicePlatform) -> e2ee::Platform {
    match device {
        DevicePlatform::Mobile => e2ee::Platform::Mobile,
        DevicePlatform::Web => e2ee::Platform::Web,
        DevicePlatform::Pc => e2ee::Platform::Pc,
    }
}
pub(super) fn convert<A: serde::Serialize, B: serde::de::DeserializeOwned>(value: &A) -> Result<B> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

/// What to ask a bundle request for.
#[derive(Clone, Copy)]
pub(super) struct BundleQuery {
    pub selection: DeviceSelection,
    /// Ask with `isReInit` (re-establishing a session), as the original does for its own devices.
    pub reinit: bool,
    /// Write the reply's shape to the report even when the service refused.
    pub verbose: bool,
}
/// A recipient for one fetched device bundle. The identity is recorded on first use; a device already
/// trusted with a *different* identity is refused (the note says so) rather than replaced.
pub(super) fn recipient_for(
    store: &mut crate::store::MessageStore,
    owner: &str,
    local: &e2ee::LocalNamespace,
    uid: &str,
    b: &signal::PublicBundle,
    now_ms: i64,
) -> Result<std::result::Result<Recipient, String>> {
    let identity = *b.identity.bytes();
    let fingerprint = Sha256::digest(identity).to_vec();
    let (namespace, session_ts) = match store.e2ee_active_peer(owner, local, uid, platform(b.device))? {
        Some(active)
            if active.registration_id == b.registration_id && active.fingerprint == fingerprint =>
        {
            (active.namespace, active.session_timestamp)
        }
        Some(_) => {
            return Ok(Err(format!(
                "device {} of {} presents a different identity than the trusted one; skipped (identity-change handling is not built yet)",
                b.device.wire(),
                if uid == owner { "own account" } else { "peer" }
            )))
        }
        None => {
            let namespace = store.e2ee_register_peer(
                owner,
                local,
                uid,
                None,
                &VerifiedBinding {
                    platform: platform(b.device),
                    registration_id: b.registration_id,
                    identity_key: identity,
                    session_timestamp: now_ms as u64,
                },
            )?;
            (namespace, now_ms as u64)
        }
    };
    Ok(Ok(Recipient {
        trust: engine::TrustDecision {
            peer: convert(&namespace)?,
            registration_id: b.registration_id,
            identity_key: identity,
            session_timestamp: session_ts,
            basis: engine::TrustBasis::AuthenticatedFirstUse,
        },
        bundle: Some(engine::PeerBundle {
            registration_id: b.registration_id,
            identity_key: identity,
            signed_prekey_id: b.signed_prekey_id,
            signed_prekey: *b.signed_prekey.bytes(),
            signature: *b.signature.bytes(),
            prekey: b.one_time_prekeys.first().map(|k| (k.id, *k.key.bytes())),
        }),
    }))
}

impl Connection {
    /// Best effort; the outcome is in the report.
    pub(super) async fn e2ee_send_probe(
        &mut self,
        db: &Database,
        path: &std::path::Path,
        selector: Option<&str>,
    ) {
        let Some(mut report) = Report::open(path) else {
            return;
        };
        match self.send_probe_inner(db, selector, &mut report).await {
            Ok(()) => report
                .line("RESULT: the engine encrypted the probe for every device; nothing was sent"),
            Err(error) => report.line(&format!("RESULT: stopped: {error}")),
        }
    }
    async fn fetch_bundles(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        report: &mut Report,
        verbose: bool,
    ) -> Result<signal::FetchedBundles> {
        self.fetch_selected(
            scope,
            uid,
            sequence,
            BundleQuery {
                selection: DeviceSelection::All,
                reinit: false,
                verbose,
            },
            report,
        )
        .await
    }
    pub(super) async fn fetch_selected(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        query: BundleQuery,
        report: &mut Report,
    ) -> Result<signal::FetchedBundles> {
        let BundleQuery {
            selection,
            reinit,
            verbose,
        } = query;
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
        self.send(541, 0, request.wire_data(scope)?).await?;
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for the bundle list"))??;
            let Some(frame) = frame.filter(|f| {
                f.cmd == 541
                    && f.payload.pointer("/data/reqId").and_then(Value::as_str)
                        == Some(request.request_id())
            }) else {
                continue;
            };
            // Shapes for the first reply of each kind, and for any that succeed.
            let ok = frame
                .payload
                .pointer("/data/data/err")
                .is_none_or(|err| err.as_i64() == Some(0));
            if verbose || ok {
                report.line(&format!(
                    "## bundle reply for a {}",
                    if uid == scope.account() {
                        "own account"
                    } else {
                        "peer"
                    }
                ));
                for line in super::e2ee_probe::shape(&frame.payload) {
                    report.line(&line);
                }
            }
            return Ok(signal::bundles_reply(
                &request,
                scope,
                &frame.payload,
                uid,
                selection,
            )?);
        }
    }

    /// The devices a contact can be reached on (command 560), as the original asks before it builds a
    /// session. An empty or unreadable answer falls back to every device type.
    async fn compatible_devices(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        report: &mut Report,
        verbose: bool,
    ) -> Result<Vec<DevicePlatform>> {
        let request = signal::compatible(scope, sequence, uid)?;
        self.send(560, 0, request.wire_data(scope)?).await?;
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.next())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for the compatibility check"))??;
            let Some(frame) = frame.filter(|f| {
                f.cmd == 560
                    && f.payload.pointer("/data/reqId").and_then(Value::as_str)
                        == Some(request.request_id())
            }) else {
                continue;
            };
            if verbose {
                report.line("## compatibility reply (560)");
                for line in super::e2ee_probe::shape(&frame.payload) {
                    report.line(&line);
                }
            }
            let mut devices: Vec<DevicePlatform> = frame
                .payload
                .pointer("/data/data/compatible")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|entry| entry["uid"].as_str() == Some(uid))
                .filter_map(|entry| DevicePlatform::parse(&entry["deviceId"]).ok())
                .collect();
            devices.sort_by_key(|device| device.wire());
            devices.dedup();
            if devices.is_empty() {
                devices = vec![
                    DevicePlatform::Mobile,
                    DevicePlatform::Web,
                    DevicePlatform::Pc,
                ];
            }
            return Ok(devices);
        }
    }
    /// A contact's bundles the way the original gets them (live capture): 560 for the device list, then one
    /// 541 per device with `isReInit` set. Note `isReInit` is not read-only: it asks the service to begin
    /// a fresh session with that device. The first service error ends the attempt.
    async fn fetch_peer_devices(
        &mut self,
        scope: &Scope,
        uid: &str,
        sequence: u64,
        report: &mut Report,
        verbose: bool,
    ) -> Result<signal::FetchedBundles> {
        let devices = self
            .compatible_devices(scope, uid, sequence, report, verbose)
            .await?;
        let mut bundles = Vec::new();
        for (n, device) in devices.into_iter().enumerate() {
            let found = self
                .fetch_selected(
                    scope,
                    uid,
                    sequence + 1 + n as u64,
                    BundleQuery {
                        selection: DeviceSelection::One(device),
                        reinit: true,
                        verbose,
                    },
                    report,
                )
                .await?;
            bundles.extend(found.bundles);
        }
        Ok(signal::FetchedBundles {
            peer: uid.to_owned(),
            bundles,
        })
    }
    /// Read-only: for the first few contacts, what the service answers to the compatibility check (560)
    /// and to a bundle request per device type. Only error codes and shapes are written.
    async fn diagnose(&mut self, scope: &Scope, candidates: &[String], report: &mut Report) {
        report.line(
            "## diagnosis: compatibility check and per-device bundle requests (first contacts)",
        );
        for (index, uid) in candidates.iter().take(3).enumerate() {
            let request = match signal::compatible(scope, 9_000_300 + index as u64, uid) {
                Ok(request) => request,
                Err(_) => continue,
            };
            let Ok(wire) = request.wire_data(scope) else {
                continue;
            };
            if self.send(560, 0, wire).await.is_err() {
                return;
            }
            let deadline = tokio::time::Instant::now() + WAIT;
            loop {
                let Ok(frame) = tokio::time::timeout_at(deadline, self.next()).await else {
                    report.line(&format!("contact {}: no answer to 560", index + 1));
                    break;
                };
                let Ok(Some(frame)) = frame else {
                    if frame.is_err() {
                        return;
                    }
                    continue;
                };
                if frame.cmd == 560
                    && frame.payload.pointer("/data/reqId").and_then(Value::as_str)
                        == Some(request.request_id())
                {
                    report.line(&format!("contact {}: command 560 reply", index + 1));
                    for line in super::e2ee_probe::shape(&frame.payload) {
                        report.line(&line);
                    }
                    break;
                }
            }
            for (n, device) in [
                DevicePlatform::Mobile,
                DevicePlatform::Web,
                DevicePlatform::Pc,
            ]
            .into_iter()
            .enumerate()
            {
                let outcome = self
                    .fetch_selected(
                        scope,
                        uid,
                        9_000_400 + (index * 10 + n) as u64,
                        BundleQuery {
                            selection: DeviceSelection::One(device),
                            reinit: true,
                            verbose: false,
                        },
                        report,
                    )
                    .await;
                let text = match outcome {
                    Ok(found) => format!("{} bundle(s)", found.bundles.len()),
                    Err(error) => match error.downcast_ref::<signal::SignalError>() {
                        Some(signal::SignalError::Service(code)) => format!("error {code}"),
                        _ => format!("failed: {error}"),
                    },
                };
                report.line(&format!(
                    "contact {} device {}: {text}",
                    index + 1,
                    device.wire()
                ));
            }
        }
    }
    /// Candidate peers in the order to try: a UID if given as digits; a name fragment matched among
    /// this account's E2EE contacts; otherwise every E2EE contact in list order. Never prints names or
    /// UIDs, only how it chose.
    async fn choose_peers(
        &mut self,
        db: &Database,
        selector: Option<&str>,
        report: &mut Report,
    ) -> Result<Vec<String>> {
        let digits = |text: &str| {
            !text.is_empty()
                && text.len() <= 24
                && !text.starts_with('0')
                && text.bytes().all(|b| b.is_ascii_digit())
        };
        if let Some(text) = selector.filter(|text| digits(text)) {
            report.line("peer: given as a numeric id");
            return Ok(vec![text.to_owned()]);
        }
        let candidates = self.encrypted_peers().await?;
        anyhow::ensure!(
            !candidates.is_empty(),
            "this account has no end-to-end encrypted contact in its E2EE list"
        );
        report.line(&format!(
            "the account's E2EE list has {} contact(s)",
            candidates.len()
        ));
        let Some(name) = selector else {
            report.line("peer: no name given; trying the E2EE contacts in list order");
            return Ok(candidates);
        };
        let (owner, query, among) = (self.account.clone(), name.to_owned(), candidates);
        let matches = db
            .call(move |s| s.direct_uids_matching(&owner, &query, &among))
            .await?;
        match matches.as_slice() {
            [one] => {
                report.line("peer: matched by name");
                Ok(vec![one.clone()])
            }
            [] => anyhow::bail!("no E2EE contact has a name containing that text"),
            many => anyhow::bail!(
                "{} E2EE contacts match that name; use a longer part of it",
                many.len()
            ),
        }
    }
    async fn send_probe_inner(
        &mut self,
        db: &Database,
        selector: Option<&str>,
        report: &mut Report,
    ) -> Result<()> {
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
        let candidates = self.choose_peers(db, selector, report).await?;
        report.line(&format!("start: local epoch {epoch}"));
        // A contact in the E2EE list may not be usable yet (the service answers with an error code and no
        // keys). Try the next one; only network problems end the probe.
        let (mut peer_bundles, mut chosen) = (None, None);
        for (index, uid) in candidates.iter().take(MAX_TRIES).enumerate() {
            match self
                .fetch_peer_devices(
                    &scope,
                    uid,
                    9_000_100 + index as u64 * 10,
                    report,
                    index == 0,
                )
                .await
            {
                Ok(found) if !found.bundles.is_empty() => {
                    peer_bundles = Some(found);
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
        if peer_bundles.is_none() {
            self.diagnose(&scope, &candidates, report).await;
        }
        let (peer_bundles, peer) = match (peer_bundles, chosen) {
            (Some(bundles), Some(uid)) => (bundles, uid),
            _ => anyhow::bail!(
                "none of the first {} E2EE contacts has usable bundles on the service",
                candidates.len().min(MAX_TRIES)
            ),
        };
        let peer = peer.as_str();
        let own_bundles = self
            .fetch_bundles(&scope, &owner, 9_000_002, report, true)
            .await?;
        report.line(&format!(
            "bundles: peer devices={:?}, own other devices={:?}",
            peer_bundles
                .bundles
                .iter()
                .map(|b| b.device.wire())
                .collect::<Vec<_>>(),
            own_bundles
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
            convert(&local)?,
        )?);
        let thread = format!("d:{peer}");
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let (g, c, co, o, l, t) = (
            gate.clone(),
            cancel.clone(),
            coordinator.clone(),
            owner.clone(),
            local.clone(),
            thread.clone(),
        );
        let (client_id, notes, plan) = db
            .call(move |store| {
                let mut recipients = Vec::new();
                let mut notes = Vec::new();
                for fetched in [&peer_bundles, &own_bundles] {
                    for b in &fetched.bundles {
                        // The PC bundle of this account is this client itself.
                        if fetched.peer == o && b.device == DevicePlatform::Pc {
                            continue;
                        }
                        match recipient_for(store, &o, &l, &fetched.peer, b, now_ms)? {
                            Ok(recipient) => recipients.push(recipient),
                            Err(note) => notes.push(note),
                        }
                    }
                }
                anyhow::ensure!(
                    recipients
                        .iter()
                        .any(|r| r.trust.peer.uid.as_str() == t.strip_prefix("d:").unwrap_or("")),
                    "the peer has no usable device bundle"
                );
                let client_id = store.e2ee_reserve_send(&o, &l, &t, PROBE_TEXT, now_ms)?;
                match co.plan_personal(store, client_id, PROBE_TEXT, recipients, &g, &c) {
                    Ok(plan) => Ok((client_id, notes, plan)),
                    Err(error) => {
                        let _ = store.e2ee_discard_reserved(&o, client_id);
                        Err(error)
                    }
                }
            })
            .await?;
        for note in &notes {
            report.line(note);
        }
        report.line("recipients planned; encrypting with the engine (not committed, not sent)");
        // The planning closure has finished and dropped its clone, so this is the only owner again.
        let mut coordinator = Arc::try_unwrap(coordinator)
            .map_err(|_| anyhow::anyhow!("the coordinator is still shared"))?;
        let prepared = coordinator.prepare_personal(plan, &gate, &cancel).await;
        // The draft row was only a reservation for this dry run.
        let (o, id) = (owner.clone(), client_id);
        let _ = db.call(move |s| s.e2ee_discard_reserved(&o, id)).await;
        let prepared = prepared?;
        report.line(&format!(
            "encrypted: {} bytes of SMS parameters for {} peer device(s)",
            prepared.exact_sms().len(),
            prepared.peers().len()
        ));
        Ok(())
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
    /// A genuine engine-created public bundle for one device, as JSON in the service's shape.
    async fn wire_bundle(account: &str, platform: engine::SignalPlatform, device: u8) -> Value {
        let id = engine::AccountId::new(account).unwrap();
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

    /// Real engine and SQLite; the service is a local socket answering 563 (E2EE list), and 541 for a
    /// peer and for this account's web device. `expected` is how many requests the probe should make.
    async fn scenario(
        selector: Option<&'static str>,
        expected: usize,
        named: bool,
        all_dead: bool,
    ) -> (Vec<u16>, String) {
        let peer_bundle = wire_bundle("22", engine::SignalPlatform::Mobile, 0).await;
        let own_web = wire_bundle("11", engine::SignalPlatform::Web, 1).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
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
            let mut seen = Vec::new();
            while seen.len() < expected {
                let Some(Ok(tungstenite::Message::Binary(bytes))) = socket.next().await else {
                    break;
                };
                let cmd = u16::from(bytes[1]) | (u16::from(bytes[2]) << 8);
                let body: Value = serde_json::from_slice(&bytes[4..]).unwrap();
                seen.push(cmd);
                if cmd == 563 {
                    let frame = wire::encode_frame(
                        563,
                        0,
                        &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"threads":[{"uid":"21"},{"uid":"22"}],"waitUids":[],"uids":[],"pendingUids":[]}}}}),
                    )
                    .unwrap();
                    socket
                        .send(tungstenite::Message::Binary(frame))
                        .await
                        .unwrap();
                    continue;
                }
                if cmd == 560 {
                    let frame = wire::encode_frame(
                        560,
                        0,
                        // Like the live service: the contact is reachable on its mobile device.
                        &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"err":0,"compatible":[{"deviceId":0,"uid":body["data"]["uids"][0]}],"incompatible":[]}}}}),
                    )
                    .unwrap();
                    socket
                        .send(tungstenite::Message::Binary(frame))
                        .await
                        .unwrap();
                    continue;
                }
                let uid = body["data"]["uids"][0]["uid"].as_str().unwrap().to_owned();
                // The live service answered -11 to peer requests without isReInit and err 0 with it.
                let reinit = body["data"]["isReInit"] == 1;
                if uid == "21" || (all_dead && uid != "11") || (uid != "11" && !reinit) {
                    // A listed contact that has no usable keys on the service (observed: err -11).
                    let frame = wire::encode_frame(
                        541,
                        0,
                        &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"err":-11,"errData":[],"keys":[]}}}}),
                    )
                    .unwrap();
                    socket
                        .send(tungstenite::Message::Binary(frame))
                        .await
                        .unwrap();
                    continue;
                }
                let bundle = if uid == "22" {
                    peer_bundle.clone()
                } else {
                    own_web.clone()
                };
                let frame = wire::encode_frame(
                    541,
                    0,
                    &json!({"error_code":0,"encrypt":0,"data":{"error_code":0,"data":{"reqId":body["reqId"],"data":{"err":0,"errData":[],"keys":[{"uid":uid,"err":0,"keys":[bundle]}]}}}}),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(frame))
                    .await
                    .unwrap();
            }
            // Anything beyond the two bundle reads would be a send; give it a moment to show up.
            let extra = tokio::time::timeout(Duration::from_millis(300), socket.next()).await;
            (
                seen,
                extra.is_ok_and(|m| matches!(m, Some(Ok(tungstenite::Message::Binary(_))))),
            )
        });

        // Seed an active local identity exactly as the key replacement leaves it.
        let scratch =
            std::env::temp_dir().join(format!("zalo-sendprobe-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        {
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
        if named {
            rusqlite::Connection::open(&db_path)
                .unwrap()
                .execute(
                    "INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,search_name,observed_at_ms) VALUES('11','d:22','22',0,'Bạn thân','ban than',1)",
                    [],
                )
                .unwrap();
        }
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        assert_eq!(connection.next().await.unwrap().unwrap().cmd, 1);
        let report = scratch.join("report.txt");
        connection.e2ee_send_probe(&db, &report, selector).await;
        let (seen, extra_frame) = server.await.unwrap();
        assert!(!extra_frame, "the dry run must not send anything else");
        let text = std::fs::read_to_string(&report).unwrap();
        // No draft is left, and both peers' identities were recorded on first use.
        let leftover: i64 =
            rusqlite::Connection::open(scratch.join("data").join("messages.sqlite"))
                .or_else(|_| rusqlite::Connection::open(&db_path))
                .unwrap()
                .query_row("SELECT count(*) FROM e2ee_outbox", [], |r| r.get(0))
                .unwrap();
        assert_eq!(leftover, 0);
        std::fs::remove_dir_all(scratch).unwrap();
        (seen, text)
    }
    fn assert_encrypted_for_both(text: &str) {
        assert!(
            text.contains("peer devices=[0], own other devices=[1]"),
            "{text}"
        );
        assert!(
            text.contains("encrypted:") && text.contains("RESULT: the engine encrypted the probe"),
            "{text}"
        );
        assert!(text.contains("for 2 peer device(s)"), "{text}");
    }
    #[tokio::test]
    async fn dry_run_with_a_uid_encrypts_for_peer_and_own_devices_without_sending_or_leaving_a_draft(
    ) {
        let (seen, text) = scenario(Some("22"), 3, false, false).await;
        assert_eq!(seen, [560, 541, 541]);
        assert!(text.contains("peer: given as a numeric id"), "{text}");
        assert_encrypted_for_both(&text);
    }
    /// No UID needed: with nothing given it walks the account's E2EE list. The first contact has no
    /// keys on the service (observed on the live account: err -11), so the probe moves on.
    #[tokio::test]
    async fn dry_run_without_a_peer_skips_contacts_without_keys_and_uses_the_next() {
        let (seen, text) = scenario(None, 6, false, false).await;
        assert_eq!(seen, [563, 560, 541, 560, 541, 541]);
        assert!(
            text.contains("trying the E2EE contacts in list order"),
            "{text}"
        );
        assert!(
            text.contains("contact 1: the service answered error -11; trying the next"),
            "{text}"
        );
        assert_encrypted_for_both(&text);
    }
    /// A name as the user knows it (alias, no accents or case) selects the contact.
    #[tokio::test]
    async fn dry_run_selects_the_peer_by_the_name_shown_in_the_app() {
        let (seen, text) = scenario(Some("Ban Than"), 4, true, false).await;
        assert_eq!(seen, [563, 560, 541, 541]);
        assert!(text.contains("peer: matched by name"), "{text}");
        assert!(
            !text.contains("Bạn") && !text.contains("22\n"),
            "no names or ids in the report: {text}"
        );
        assert_encrypted_for_both(&text);
    }
    /// Every listed contact answers -11 (the live account's case): the probe adds a read-only diagnosis
    /// of 560 and per-device bundle requests, then stops without encrypting anything.
    #[tokio::test]
    async fn when_every_contact_is_refused_it_diagnoses_read_only_and_stops() {
        let (seen, text) = scenario(None, 13, false, true).await;
        assert_eq!(seen[..5], [563, 560, 541, 560, 541]);
        assert_eq!(seen[5..].iter().filter(|c| **c == 560).count(), 2);
        assert_eq!(seen[5..].iter().filter(|c| **c == 541).count(), 6);
        assert!(text.contains("## diagnosis"), "{text}");
        assert!(text.contains("contact 1: command 560 reply"), "{text}");
        assert!(text.contains("contact 1 device 0: error -11"), "{text}");
        assert!(text.contains("contact 2 device 2: error -11"), "{text}");
        assert!(
            text.contains("RESULT: stopped: none of the first 2 E2EE contacts"),
            "{text}"
        );
        assert!(!text.contains("encrypted:"), "{text}");
    }
    #[tokio::test]
    async fn an_unknown_name_stops_before_any_bundle_request() {
        let (_seen, text) = scenario(Some("Không có ai"), 1, true, false).await;
        assert!(
            text.contains("no E2EE contact has a name containing that text"),
            "{text}"
        );
        assert!(!text.contains("encrypted:"), "{text}");
    }
}
