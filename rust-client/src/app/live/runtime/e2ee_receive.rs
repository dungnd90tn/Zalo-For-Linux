//! Durable receive of personal end-to-end encrypted text, from push or catch-up.
//!
//! An optional report can be written to an absolute `ZALO_E2EE_RECEIVE` path. Each supported
//! message is decrypted and stored in one SQL transaction, then shown in the conversation.
//! This code **never acknowledges anything** (no 544, no 545/1): the service keeps redelivering, and a
//! redelivered ciphertext is recognised and skipped. Unsupported content and a peer whose identity differs
//! from the trusted one remain outside the durable frontier.
use super::e2ee_send_probe::{convert, platform, GENERATION};
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{
        personal_ciphertext_digest, personal_ciphertext_from_wire, prekey_sender,
        stored_peer_identity, Cancellation, Coordinator,
    },
    diagnostics::{self, Kind, Reason, Stage},
    domain::Message,
    protocol::signal::{Command, DevicePlatform, SessionEvent},
    store::e2ee::{self, GenerationGate, Receive, VerifiedBinding},
};
use anyhow::Context as _;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;
use zalo_signal_engine as engine;

const QUEUE: &str = "signal-personal";
const SESSION_RECEIPTS: &str = "signal-personal-session";

struct PersonalInput {
    kind: e2ee::TargetKind,
    uid: String,
    thread: String,
    server_id: String,
    device: DevicePlatform,
    cipher: engine::PersonalCiphertext,
    target_iid: Option<u16>,
    sent_ms: u64,
    cursor: u64,
}

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
        if sender.starts_with('0') {
            ", leading zero"
        } else {
            ""
        },
        message
            .pointer("/paramsExt/platformType")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".into()),
        text("content").map(str::len).unwrap_or(0),
        message.as_object().map(|m| m.len()).unwrap_or(0)
    )
}

enum Outcome {
    Stored,
    Duplicate,
    Skipped(Reason, &'static str),
}

#[derive(Debug)]
enum ReceiveError {
    KeyUnavailable,
    SessionMissing,
    IdentityChanged,
}

impl std::fmt::Display for ReceiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::KeyUnavailable => "this client holds no E2EE key yet",
            Self::SessionMissing => {
                "no session with this device and the message does not start one"
            }
            Self::IdentityChanged => {
                "this device presents a different identity than the trusted one"
            }
        })
    }
}

impl std::error::Error for ReceiveError {}

fn failure_reason(error: &anyhow::Error) -> Reason {
    match error.downcast_ref::<ReceiveError>() {
        Some(ReceiveError::KeyUnavailable) => Reason::EncryptedKeyUnavailable,
        Some(ReceiveError::SessionMissing) => Reason::EncryptedSessionMissing,
        Some(ReceiveError::IdentityChanged) => Reason::EncryptedIdentityChanged,
        None => Reason::EncryptedReceiveFailed,
    }
}

impl Connection {
    /// Applies authenticated pairwise session events without publishing or acknowledging them.
    pub(super) async fn e2ee_receive_session_events(
        &mut self,
        records: &[SessionEvent],
        db: &Database,
        events: &Events,
    ) -> bool {
        if records.is_empty() {
            return true;
        }
        let trace = |command, reason| {
            diagnostics::record(
                diagnostics::Event::new(Kind::Debug, Stage::Receive, reason)
                    .frame(command, 0, None),
            );
        };
        let mut receiver = match self.e2ee_receive.take() {
            Some(receiver) => receiver,
            None => {
                match Receiver::open(self.e2ee_receive_report.as_deref(), db, &self.account).await {
                    Ok(receiver) => receiver,
                    Err(error) => {
                        trace(records[0].source_command, failure_reason(&error));
                        return false;
                    }
                }
            }
        };
        let mut failures = HashSet::new();
        for event in records {
            let reason = match receiver.handle_session(event, db, events).await {
                Ok(Outcome::Stored | Outcome::Duplicate) => continue,
                Ok(Outcome::Skipped(reason, _)) => reason,
                Err(error) => failure_reason(&error),
            };
            if failures.insert(reason) {
                trace(event.source_command, reason);
            }
        }
        self.e2ee_receive = Some(receiver);
        failures.is_empty()
    }

    /// Handles the `msgs[]` of one pushed 551 frame. Best effort: every outcome is a report line.
    pub(super) async fn e2ee_receive_push(
        &mut self,
        frame: &DecodedFrame,
        db: &Database,
        events: &Events,
    ) {
        let Some(messages) = frame
            .payload
            .pointer("/data/msgs")
            .and_then(Value::as_array)
        else {
            return;
        };
        self.e2ee_receive_batch(messages, db, events, frame.cmd)
            .await;
    }

    /// A complete batch is a sequence of committed messages or exact committed replays.
    /// The caller owns the offline page cursor and may advance it only for a complete batch.
    pub(super) async fn e2ee_receive_batch(
        &mut self,
        messages: &[Value],
        db: &Database,
        events: &Events,
        command: u16,
    ) -> bool {
        if messages.is_empty() {
            return true;
        }
        let trace = |reason| {
            diagnostics::record(
                diagnostics::Event::new(Kind::Debug, Stage::Receive, reason)
                    .frame(command, 0, None),
            );
        };
        // A report file is optional (`ZALO_E2EE_RECEIVE`).
        let path = self.e2ee_receive_report.clone();
        let mut receiver = match self.e2ee_receive.take() {
            Some(receiver) => receiver,
            None => match Receiver::open(path.as_deref(), db, &self.account).await {
                Ok(receiver) => receiver,
                Err(error) => {
                    trace(failure_reason(&error));
                    if let Some(mut report) = path.as_deref().and_then(Report::open) {
                        report.line(&format!("receive: cannot start: {error}"));
                    }
                    return false;
                }
            },
        };
        let mut failures = HashSet::new();
        for message in messages {
            receiver.report.line(&describe(message));
            match receiver.handle(message, db, events).await {
                Ok(Outcome::Stored) => receiver.report.line("message decrypted and stored"),
                Ok(Outcome::Duplicate) => receiver.report.line("already stored (redelivery)"),
                Ok(Outcome::Skipped(reason, why)) => {
                    if failures.insert(reason) {
                        trace(reason);
                    }
                    receiver.report.line(&format!("message skipped: {why}"));
                }
                // `{:#}` prints the whole chain: which step failed, then why.
                Err(error) => {
                    let reason = failure_reason(&error);
                    if failures.insert(reason) {
                        trace(reason);
                    }
                    receiver
                        .report
                        .line(&format!("message not decrypted: {error:#}"));
                }
            }
        }
        self.e2ee_receive = Some(receiver);
        failures.is_empty()
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
            .ok_or(ReceiveError::KeyUnavailable)?;
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
        let from = message
            .get("uidFrom")
            .ok_or_else(|| anyhow::anyhow!("no valid sender"))?;
        let own_device = from.as_str() == Some("0") || from.as_u64() == Some(0);
        let uid = if own_device {
            owner.clone()
        } else {
            wire::id_string(from).context("invalid encrypted sender")?
        };
        let raw_to = message
            .get("idTo")
            .ok_or_else(|| anyhow::anyhow!("no recipient"))?;
        // An authenticated inbound personal row may use 0 for the current
        // account. A row from another own device must name its peer explicitly.
        let to_self = raw_to.as_str() == Some("0") || raw_to.as_u64() == Some(0);
        let conversation = if own_device {
            wire::id_string(raw_to).context("invalid own-device destination")?
        } else {
            if !to_self {
                let recipient = wire::id_string(raw_to).context("invalid inbound recipient")?;
                anyhow::ensure!(recipient == owner, "message targets a different account");
            }
            uid.clone()
        };
        let thread = format!("d:{conversation}");
        let Some(ext) = text("e2ee") else {
            return Ok(Outcome::Skipped(
                Reason::EncryptedContentUnsupported,
                "not an end-to-end encrypted message",
            ));
        };
        if text("msgType") != Some("webchat") {
            return Ok(Outcome::Skipped(
                Reason::EncryptedContentUnsupported,
                "not a text message (not handled yet)",
            ));
        }
        if wire::message_ttl(message.get("ttl")) != Some(0) {
            return Ok(Outcome::Skipped(
                Reason::EncryptedContentUnsupported,
                "encrypted disappearing message (not handled yet)",
            ));
        }
        // `iid` in the extension is the registration id of the *recipient's* key (this device), as this
        // client also writes it; it is not the sender's. It tells which of our keys the message targets.
        anyhow::ensure!(ext.len() <= 1024, "invalid E2EE extension");
        let extension: Value = serde_json::from_str(ext).context("invalid E2EE extension")?;
        let extension = extension
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("invalid E2EE extension"))?;
        let target_iid = extension
            .get("iid")
            .map(|iid| {
                iid.as_u64()
                    .and_then(|value| u16::try_from(value).ok())
                    .filter(|id| *id <= 16383)
                    .ok_or_else(|| anyhow::anyhow!("invalid recipient registration"))
            })
            .transpose()?;
        let device = DevicePlatform::parse(
            message
                .pointer("/paramsExt/platformType")
                .ok_or_else(|| anyhow::anyhow!("no sender device"))?,
        )?;
        let cipher = personal_ciphertext_from_wire(text("content").unwrap_or(""))?;
        let server_id = wire::id_string(
            message
                .get("msgId")
                .ok_or_else(|| anyhow::anyhow!("no message id"))?,
        )
        .context("invalid encrypted message id")?;
        let positive = |value: &Value| {
            value
                .as_u64()
                .or_else(|| value.as_str()?.parse().ok())
                .filter(|value| *value > 0 && *value <= 9_007_199_254_740_991)
        };
        let sent_ms = message
            .get("ts")
            .and_then(positive)
            .ok_or_else(|| anyhow::anyhow!("invalid message timestamp"))?;
        let cursor = message
            .get("actionId")
            .or_else(|| message.get("msgId"))
            .and_then(positive)
            .unwrap_or(1);
        self.decrypt(
            PersonalInput {
                kind: e2ee::TargetKind::Personal,
                uid,
                thread,
                server_id,
                device,
                cipher,
                target_iid,
                sent_ms,
                cursor,
            },
            db,
            events,
        )
        .await
    }

    async fn handle_session(
        &mut self,
        event: &SessionEvent,
        db: &Database,
        events: &Events,
    ) -> Result<Outcome> {
        let payload = event.payload();
        if !(event.command == Command::Init
            || (event.command == Command::InitAck && payload["type"].as_u64() == Some(1)))
        {
            return Ok(Outcome::Skipped(
                Reason::EncryptedContentUnsupported,
                "unsupported personal session event",
            ));
        }
        anyhow::ensure!(
            payload["uidTo"].as_str() == Some(events.account.as_str())
                && payload["uidFrom"].as_str() == Some(event.sender.as_str())
                && DevicePlatform::parse(&payload["deviceId"])? == event.device,
            "Session event account or device scope does not match"
        );
        let cipher = personal_ciphertext_from_wire(payload["data"].as_str().unwrap_or(""))?;
        if !matches!(
            (event.command, &cipher.message_type),
            (Command::Init, engine::PersonalMessageType::PreKey)
                | (Command::InitAck, engine::PersonalMessageType::Whisper)
        ) {
            return Ok(Outcome::Skipped(
                Reason::EncryptedContentUnsupported,
                "unsupported personal session ciphertext",
            ));
        }
        self.decrypt(
            PersonalInput {
                kind: e2ee::TargetKind::PersonalSession,
                uid: event.sender.clone(),
                thread: format!("d:{}", event.sender),
                // A queued/live copy of the same init shares a durable identity.
                server_id: format!(
                    "{}:{}:{}",
                    self.epoch, event.command as u16, event.server_id
                ),
                device: event.device,
                cipher,
                target_iid: None,
                sent_ms: event.timestamp,
                // This is an internal receipt marker. The queue owns its page frontier.
                cursor: 1,
            },
            db,
            events,
        )
        .await
    }

    async fn decrypt(
        &mut self,
        input: PersonalInput,
        db: &Database,
        events: &Events,
    ) -> Result<Outcome> {
        let PersonalInput {
            kind,
            uid,
            thread,
            server_id,
            device,
            cipher,
            target_iid,
            sent_ms,
            cursor,
        } = input;
        let owner = events.account.clone();
        let session = kind == e2ee::TargetKind::PersonalSession;
        let queue = if session { SESSION_RECEIPTS } else { QUEUE };
        let digest = personal_ciphertext_digest(&cipher);
        let header = matches!(cipher.message_type, engine::PersonalMessageType::PreKey)
            .then(|| prekey_sender(&cipher.body))
            .transpose()?;
        let seen = {
            let owner = owner.clone();
            let (thread, server_id, uid) = (thread.clone(), server_id.clone(), uid.clone());
            db.call(move |s| {
                Ok(s.e2ee_lookup_committed_input(
                    &owner,
                    e2ee::InputLocator {
                        kind,
                        thread: &thread,
                        server_id: &server_id,
                        sender_uid: &uid,
                        sender_platform: platform(device),
                        registration_id: header.map(|(registration, _)| registration),
                    },
                    &digest,
                )?
                .is_some())
            })
            .await?
        };
        if seen {
            return Ok(Outcome::Duplicate);
        }
        // The local epoch may have been rotated meanwhile; rebuild the engine for the new one.
        let (current, local_registration) = {
            let owner = owner.clone();
            let epoch = self.epoch;
            db.call(move |s| {
                if session {
                    let registered_at = s.e2ee_local_session_timestamp(&owner, epoch)?;
                    anyhow::ensure!(
                        sent_ms >= registered_at && sent_ms > 0,
                        "Session event predates this key"
                    );
                }
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
                    Reason::EncryptedKeyMismatch,
                    "encrypted for a different key of this device (the sender used an older registration)",
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
            let now_ms = if session {
                sent_ms
            } else {
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64
            };
            db.call(move |store| {
                let (namespace, registration, identity, session_ts, first_use) = match store
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
                        let sender_registration = header
                            .map(|(registration, _)| registration)
                            .unwrap_or(active.registration_id);
                        anyhow::ensure!(
                            active.registration_id == sender_registration
                                && active.fingerprint == Sha256::digest(identity).to_vec(),
                            ReceiveError::IdentityChanged
                        );
                        (
                            active.namespace,
                            active.registration_id,
                            identity,
                            active.session_timestamp,
                            false,
                        )
                    }
                    None => {
                        let (registration, identity) =
                            header.ok_or(ReceiveError::SessionMissing)?;
                        let (namespace, session_ts) = store
                            .e2ee_reserve_first_use_peer(
                                &o,
                                &l,
                                &u,
                                &VerifiedBinding {
                                    platform: platform(device),
                                    registration_id: registration,
                                    identity_key: identity,
                                    session_timestamp: now_ms,
                                },
                            )
                            .context("reserving the sender's untrusted binding")?;
                        (namespace, registration, identity, session_ts, true)
                    }
                };
                let trust = engine::TrustDecision {
                    peer: convert(&namespace).context("building the trust decision")?,
                    registration_id: registration,
                    identity_key: identity,
                    session_timestamp: session_ts,
                    basis: engine::TrustBasis::AuthenticatedFirstUse,
                };
                let plan = if session {
                    co.plan_receive_session(
                        store,
                        trust,
                        cipher_for_plan,
                        sent_ms,
                        &th,
                        &sid,
                        queue,
                        &g,
                        &c,
                        first_use,
                    )
                } else if first_use {
                    co.plan_receive_first_use(
                        store,
                        trust,
                        cipher_for_plan,
                        sent_ms,
                        &th,
                        &sid,
                        queue,
                        &g,
                        &c,
                    )
                } else {
                    co.plan_receive(
                        store,
                        trust,
                        cipher_for_plan,
                        sent_ms,
                        &th,
                        &sid,
                        queue,
                        &g,
                        &c,
                    )
                };
                plan.context("planning the decryption")
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
        let stored = if session {
            validate_session_plaintext(verified.plaintext())?;
            None
        } else {
            let plaintext = String::from_utf8(verified.plaintext().to_vec())
                .map_err(|_| anyhow::anyhow!("the decrypted message is not text"))?;
            Some(Message {
                mentions: Vec::new(),
                reply_blocked: false,
                recalled: false,
                source_client_id: None,
                quote: None,
                account_id: owner.clone(),
                thread_id: thread,
                id: server_id,
                sender: uid,
                text: plaintext,
                sticker: None,
                media: None,
                sent_at_ms: i64::try_from(sent_ms).unwrap_or(0),
                expires_at_ms: None,
            })
        };
        let gate = self.gate.clone();
        let published = db
            .call(move |store| {
                verified.check_preparation()?;
                let receipt = store.e2ee_commit_receive(
                    Receive {
                        delta: verified.delta(),
                        messages: stored.as_slice(),
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
        if !session {
            (events.repaint)();
        }
        Ok(Outcome::Stored)
    }
}

fn validate_session_plaintext(bytes: &[u8]) -> Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Init {
        #[serde(rename = "subDevice")]
        devices: Vec<u8>,
    }
    anyhow::ensure!(bytes.len() <= 128, "Invalid session plaintext length");
    let init: Init = serde_json::from_slice(bytes).context("Invalid session plaintext")?;
    let mut seen = [false; 3];
    for device in init.devices {
        let flag = seen
            .get_mut(usize::from(device))
            .context("Invalid session platform")?;
        anyhow::ensure!(!*flag, "Duplicate session platform");
        *flag = true;
    }
    Ok(())
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
            unread: None,
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
        // Redelivery is durable only for the original authenticated scope.
        assert_eq!(
            text.matches("already stored (redelivery)").count(),
            1,
            "{text}"
        );
        assert!(
            text.contains("different authenticated input context"),
            "{text}"
        );
        assert!(text.contains("not a text message"), "{text}");
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[tokio::test]
    async fn failed_first_use_does_not_activate_peer_or_advance_the_cursor() {
        let scratch =
            std::env::temp_dir().join(format!("zalo-recv-first-use-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        let db = Database::open(profile.clone(), "11".into()).await.unwrap();
        assert!(Receiver::open(None, &db, "11").await.is_err());
        drop(db);
        let bundle = install_receiver(&db_path).await;
        let wires = peer_sends(&scratch.join("peer.sqlite"), "101", &["Tin thử"], &bundle).await;
        let mut row = msg(7001, &wires[0], "101", "webchat", bundle.registration_id);
        row["idTo"] = json!("0");
        let db = Database::open(profile, "11".into()).await.unwrap();
        let mut receiver = Receiver::open(None, &db, "11").await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let events = Events {
            unread: None,
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let observer = rusqlite::Connection::open(&db_path).unwrap();
        observer.execute_batch("CREATE TRIGGER block_chat BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'synthetic commit rejection'); END;").unwrap();
        assert!(receiver.handle(&row, &db, &events).await.is_err());
        assert!(rx.try_recv().is_err());
        let local = receiver.local.clone();
        db.call(move |store| {
            assert!(store
                .e2ee_active_peer("11", &local, "101", e2ee::Platform::Mobile)?
                .is_none());
            assert_eq!(store.receive_cursor("11", QUEUE)?, "1");
            Ok(())
        })
        .await
        .unwrap();

        observer.execute_batch("DROP TRIGGER block_chat").unwrap();
        assert!(matches!(
            receiver.handle(&row, &db, &events).await.unwrap(),
            Outcome::Stored
        ));
        assert!(
            matches!(rx.try_recv(), Ok(LiveEvent::Received { message, .. }) if message.id == "7001")
        );
        assert!(matches!(
            receiver.handle(&row, &db, &events).await.unwrap(),
            Outcome::Duplicate
        ));
        row["idTo"] = json!(0);
        assert!(matches!(
            receiver.handle(&row, &db, &events).await.unwrap(),
            Outcome::Duplicate
        ));
        row["uidFrom"] = json!("0");
        assert!(receiver.handle(&row, &db, &events).await.is_err());
        row["uidFrom"] = json!("101");
        let mut wrong_recipient = row.clone();
        wrong_recipient["idTo"] = json!("33");
        assert!(receiver
            .handle(&wrong_recipient, &db, &events)
            .await
            .is_err());
        assert!(rx.try_recv().is_err());
        drop(observer);
        drop(db);
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[tokio::test]
    async fn authenticated_personal_catchup_commits_only_the_durable_prefix_without_ack() {
        let scratch =
            std::env::temp_dir().join(format!("zalo-recv-catchup-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        let bundle = install_receiver(&db_path).await;
        let wires = peer_sends(
            &scratch.join("peer.sqlite"),
            "101",
            &["Tin trước", "Tin sau"],
            &bundle,
        )
        .await;
        let iid = bundle.registration_id;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":513,"subCmd":0}]}}"#)
                .unwrap();
            socket.send(tungstenite::Message::Binary(wire::encode_frame(1, 0, &json!({
                "error_code":0,"key":STANDARD.encode([7u8;32]),"encrypt":1,"data":STANDARD.encode(z.finish().unwrap())
            })).unwrap())).await.unwrap();

            for (first, requested, next, rows) in [
                (
                    true,
                    1,
                    8002,
                    vec![msg(8001, &wires[0], "101", "webchat", iid)],
                ),
                (
                    false,
                    8002,
                    8006,
                    vec![
                        msg(8003, &wires[1], "101", "webchat", iid),
                        msg(8005, "", "101", "chat.sticker", iid),
                    ],
                ),
            ] {
                let Some(Ok(tungstenite::Message::Binary(request))) = socket.next().await else {
                    panic!("expected encrypted catch-up request");
                };
                assert_eq!(&request[..4], &[1, 1, 2, 0]);
                let request: Value = serde_json::from_slice(&request[4..]).unwrap();
                assert_eq!(request["first"], first);
                assert_eq!(request["lastId"], requested);
                let response = wire::encode_frame(
                    513,
                    0,
                    &json!({
                        "error_code":0,"encrypt":0,"data": {"error_code":0,"data": {
                            "msgs":rows,"more":first,"lastActionId":next,"reqId":request["reqId"]
                        }}
                    }),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(response))
                    .await
                    .unwrap();
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(300), socket.next())
                    .await
                    .is_err()
            );
        });
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let events = Events {
            unread: None,
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        for _ in 0..3 {
            let frame = connection.next().await.unwrap().unwrap();
            connection.process(frame, &db, &events).await.unwrap();
        }
        service.await.unwrap();
        let mut ids = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let LiveEvent::Received { message, .. } = event {
                ids.push(message.id);
            }
        }
        assert_eq!(ids, ["8001", "8003"]);
        db.call(move |store| {
            let epoch = store.e2ee_current_local("11")?.unwrap();
            assert_eq!(
                store.receive_cursor("11", &format!("signal-pc-513_0-{epoch}"))?,
                "8002"
            );
            Ok(())
        })
        .await
        .unwrap();
        drop(db);
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[tokio::test]
    async fn desktop_session_catchup_unlocks_ordinary_incoming_message_without_ack() {
        let scratch =
            std::env::temp_dir().join(format!("zalo-recv-session-{:x}", rand::random::<u64>()));
        std::fs::create_dir(&scratch).unwrap();
        let profile = Arc::new(crate::profile::Profile::at_root(&scratch, "fixture").unwrap());
        let db_path = profile.paths.data_dir.join("messages.sqlite");
        let bundle = install_receiver(&db_path).await;
        let wires = peer_sends(
            &scratch.join("peer.sqlite"),
            "101",
            &[super::super::e2ee_init_devices::INIT_TEXT, "Tin đến từ bạn"],
            &bundle,
        )
        .await;
        let ordinary = crate::app::live::signal_crypto::ordinary_fixture_from_prekey(&wires[1]);
        let session = json!({
            "cmd":543,"msgId":"8010",
            "sessionData":{
                "uidFrom":"101","uidTo":"11","deviceId":0,"reqServerTs":1700000000000u64,
                "data":wires[0]
            }
        });
        let personal = msg(8011, &ordinary, "101", "webchat", bundle.registration_id);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(br#"{"error_code":0,"data":{"qCmds":[{"cmd":513,"subCmd":0},{"cmd":516,"subCmd":0}]}}"#)
                .unwrap();
            let auth = wire::encode_frame(
                1,
                0,
                &json!({
                    "error_code":0,"key":STANDARD.encode([7u8;32]),"encrypt":1,
                    "data":STANDARD.encode(z.finish().unwrap())
                }),
            )
            .unwrap();
            socket
                .send(tungstenite::Message::Binary(auth))
                .await
                .unwrap();
            for (command, mut page) in [
                (
                    516u16,
                    json!({"eesession":[session],"lastActionId":"8010","more":false}),
                ),
                (
                    513,
                    json!({"msgs":[personal],"lastActionId":"8012","more":false}),
                ),
            ] {
                let Some(Ok(tungstenite::Message::Binary(request))) = socket.next().await else {
                    panic!("expected encrypted catch-up request");
                };
                assert_eq!(&request[1..3], &command.to_le_bytes());
                let request: Value = serde_json::from_slice(&request[4..]).unwrap();
                page["reqId"] = request["reqId"].clone();
                let reply = wire::encode_frame(
                    command,
                    0,
                    &json!({
                        "error_code":0,"encrypt":0,"data":{"error_code":0,"data":page}
                    }),
                )
                .unwrap();
                socket
                    .send(tungstenite::Message::Binary(reply))
                    .await
                    .unwrap();
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(300), socket.next())
                    .await
                    .is_err()
            );
        });
        let db = Database::open(profile, "11".into()).await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let events = Events {
            unread: None,
            account: "11".into(),
            tx,
            repaint: Arc::new(|| {}),
            privacy: Default::default(),
        };
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        let mut connection = Connection::connected(socket, "11");
        for _ in 0..3 {
            let frame = tokio::time::timeout(Duration::from_secs(5), connection.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            connection.process(frame, &db, &events).await.unwrap();
        }
        service.await.unwrap();
        let received: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|event| match event {
                LiveEvent::Received { message, .. } => Some(message),
                _ => None,
            })
            .collect();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].thread_id, "d:101");
        assert_eq!(received[0].sender, "101");
        assert_eq!(received[0].text, "Tin đến từ bạn");
        db.call(|store| {
            let epoch = store.e2ee_current_local("11")?.unwrap();
            assert_eq!(
                store.receive_cursor("11", &format!("signal-pc-513_0-{epoch}"))?,
                "8012"
            );
            Ok(())
        })
        .await
        .unwrap();
        drop(db);
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
            unread: None,
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
