//! Sends one end-to-end encrypted text to a contact (stage 3 of `E2EE_LIVE_PLAN.md`).
//!
//! Opt-in: `ZALO_E2EE_SEND` is an absolute report path and `ZALO_E2EE_SEND_TEXT` is the message (it really
//! goes to the contact). `ZALO_E2EE_PEER` names the contact (name, or `#number` from the report of
//! `ZALO_E2EE_INIT_PEER`). The conversation needs sessions with the contact's devices: run the handshake first.
//!
//! One reviewed pipeline, durable before the network: reserve the draft, plan against one snapshot, the engine
//! encrypts for every device that has a session (the contact's and this account's other devices, so the phone
//! shows the message too), one SQL commit, then the signed HTTP POST of the exact committed bytes to
//! `/api/e2ee/pc/message/sms`. A reply the code cannot read, or no reply, leaves the row `unknown` and the
//! message is never sent again.
use super::e2ee_send_probe::{convert, platform, GENERATION};
use super::signal_transport::Report;
use super::*;
use crate::{
    app::live::signal_crypto::{
        authorize_dispatch, stored_peer_identity, AuthorizedSms, Cancellation, Coordinator,
        Recipient,
    },
    protocol::signal::DevicePlatform,
    store::e2ee::{self, GenerationGate},
};
use std::sync::Arc;
use zalo_signal_engine as engine;

/// What the connection needs to make authenticated HTTP requests (set when the connection opens).
#[derive(Clone)]
pub(super) struct E2eeHttp {
    pub client: reqwest::Client,
    pub session: Arc<VerifiedSession>,
}

/// The report path, the message text and the optional contact selector.
pub(super) fn target() -> Option<(std::path::PathBuf, String, Option<String>)> {
    let path = std::path::PathBuf::from(std::env::var_os("ZALO_E2EE_SEND")?);
    let text = std::env::var("ZALO_E2EE_SEND_TEXT")
        .ok()
        .filter(|t| !t.trim().is_empty())?;
    let selector = std::env::var("ZALO_E2EE_PEER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    path.is_absolute().then_some((path, text, selector))
}
pub(super) fn claim() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    !DONE.swap(true, Ordering::SeqCst)
}

/// A recipient for a device that already has a session here (no bundle needed), or `None`.
fn existing_recipient(
    store: &mut crate::store::MessageStore,
    owner: &str,
    local: &e2ee::LocalNamespace,
    uid: &str,
    device: DevicePlatform,
) -> Result<Option<Recipient>> {
    let Some(active) = store.e2ee_active_peer(owner, local, uid, platform(device))? else {
        return Ok(None);
    };
    let key = e2ee::RecordKey::Session {
        local: local.clone(),
        peer: active.namespace.clone(),
    };
    if store.e2ee_snapshot(owner, &[key])?.records.is_empty() {
        return Ok(None);
    }
    let identity = stored_peer_identity(store, owner, local, &active.namespace)?;
    Ok(Some(Recipient {
        trust: engine::TrustDecision {
            peer: convert(&active.namespace)?,
            registration_id: active.registration_id,
            identity_key: identity,
            session_timestamp: active.session_timestamp,
            basis: engine::TrustBasis::AuthenticatedFirstUse,
        },
        bundle: None,
    }))
}

/// Whether this device already has a session with any device of `peer` (the original's `_getSession`).
pub(super) async fn has_session(db: &Database, owner: &str, peer: &str) -> Result<bool> {
    let (o, u) = (owner.to_owned(), peer.to_owned());
    db.call(move |s| {
        let Some(epoch) = s.e2ee_current_local(&o)? else {
            return Ok(false);
        };
        let local = e2ee::LocalNamespace {
            epoch: epoch.to_string(),
            platform: e2ee::Platform::Pc,
        };
        let mut any = false;
        for device in [
            DevicePlatform::Mobile,
            DevicePlatform::Web,
            DevicePlatform::Pc,
        ] {
            any |= existing_recipient(s, &o, &local, &u, device)?.is_some();
        }
        Ok(any)
    })
    .await
}

/// A committed, authorized send that has not been transmitted.
pub(super) struct Prepared {
    pub client_id: u64,
    pub authorized: AuthorizedSms,
    pub devices: usize,
}

/// Reserve, plan, encrypt, commit and authorize the dispatch of `text` to contact `peer`. Sends nothing.
pub(super) async fn prepare_authorized(
    db: &Database,
    owner: &str,
    peer: &str,
    text: &str,
    now_ms: i64,
) -> Result<Prepared> {
    let epoch = {
        let owner = owner.to_owned();
        db.call(move |s| s.e2ee_current_local(&owner)).await?
    }
    .ok_or_else(|| anyhow::anyhow!("this client holds no E2EE key yet"))?;
    let local = e2ee::LocalNamespace {
        epoch: epoch.to_string(),
        platform: e2ee::Platform::Pc,
    };
    let gate = Arc::new(GenerationGate::new(GENERATION)?);
    let cancel = Cancellation::default();
    let coordinator = Arc::new(Coordinator::new(
        engine::AccountId::new(owner).map_err(|_| anyhow::anyhow!("invalid account id"))?,
        GENERATION,
        convert(&local)?,
    )?);
    let (o, p, l, t, g, c, co) = (
        owner.to_owned(),
        peer.to_owned(),
        local.clone(),
        text.to_owned(),
        gate.clone(),
        cancel.clone(),
        coordinator.clone(),
    );
    let (client_id, plan, devices) = db
        .call(move |store| {
            let mut recipients = Vec::new();
            for device in [DevicePlatform::Mobile, DevicePlatform::Web, DevicePlatform::Pc] {
                if let Some(recipient) = existing_recipient(store, &o, &l, &p, device)? {
                    recipients.push(recipient);
                }
            }
            anyhow::ensure!(
                !recipients.is_empty(),
                "no session with any device of this contact yet; run the handshake first (ZALO_E2EE_INIT_PEER)"
            );
            // This account's other devices get a copy, so the phone shows the message as well.
            for device in [DevicePlatform::Mobile, DevicePlatform::Web] {
                if let Some(recipient) = existing_recipient(store, &o, &l, &o, device)? {
                    recipients.push(recipient);
                }
            }
            let devices = recipients.len();
            let client_id = store.e2ee_reserve_send(&o, &l, &format!("d:{p}"), &t, now_ms)?;
            match co.plan_personal(store, client_id, &t, recipients, &g, &c) {
                Ok(plan) => Ok((client_id, plan, devices)),
                Err(error) => {
                    let _ = store.e2ee_discard_reserved(&o, client_id);
                    Err(error)
                }
            }
        })
        .await?;
    let mut coordinator = Arc::try_unwrap(coordinator)
        .map_err(|_| anyhow::anyhow!("the coordinator is still shared"))?;
    let prepared = match coordinator.prepare_personal(plan, &gate, &cancel).await {
        Ok(prepared) => prepared,
        Err(error) => {
            let (o, id) = (owner.to_owned(), client_id);
            let _ = db.call(move |s| s.e2ee_discard_reserved(&o, id)).await;
            return Err(error);
        }
    };
    let (o, g2) = (owner.to_owned(), gate.clone());
    let dispatch = db
        .call(move |s| {
            s.e2ee_commit_verified_send(&prepared, &g2)?;
            s.e2ee_begin_dispatch(&o, client_id, GENERATION, &g2)
        })
        .await?;
    Ok(Prepared {
        client_id,
        authorized: authorize_dispatch(dispatch, &gate)?,
        devices,
    })
}

/// POST a committed, authorized send to the encrypted endpoint and store the service's acknowledgement.
/// A reply the code cannot read, or none, leaves the row `unknown`; the message is never sent again.
pub(super) async fn post_prepared(
    db: &Database,
    http: &E2eeHttp,
    owner: &str,
    prepared: &Prepared,
    report: &mut Report,
) -> Result<(Message, Conversation)> {
    let owner = owner.to_owned();
    let client_id = prepared.client_id;
    let parameters = prepared.authorized.parameters();
    // The encrypted endpoints live on their own service (`chat_e2e`), not on the plain chat service.
    let mut url = root_for(&http.session, "chat_e2e")?;
    url.set_path(parameters.path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", http.session.client_type().api_type());
    let params = http
        .session
        .encrypt_payload(prepared.authorized.exact_json())?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &params)
        .finish();
    let response = tokio::time::timeout(
        Duration::from_secs(20),
        authenticated_request(&http.client, &http.session, url, Some(body)),
    )
    .await;
    let (o, id) = (owner.clone(), client_id);
    let response = match response {
        Ok(Ok(response)) => response,
        other => {
            // Whether it arrived is not known; never resend.
            let _ = db.call(move |s| s.e2ee_send_failed(&o, id, false)).await;
            let cause = match other {
                Ok(Err(error)) => format!("{error:#}"),
                _ => "timed out".to_owned(),
            };
            anyhow::bail!(
                "no readable reply from the service ({cause}); the message may or may not have been delivered and is not resent"
            );
        }
    };
    report.line("## reply to the message post");
    for line in super::e2ee_probe::shape(&response) {
        report.line(&line);
    }
    let ack = match wire::e2ee_send_ack(&response) {
        Ok(ack) => ack,
        Err(_) => {
            let _ = db.call(move |s| s.e2ee_send_failed(&o, id, false)).await;
            // Field names and kinds of `data` only (names are the service's, never message content).
            let fields = response
                .get("data")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .take(12)
                        .map(|(k, v)| {
                            let kind = match v {
                                Value::Null => "null",
                                Value::Bool(_) => "bool",
                                Value::Number(_) => "number",
                                Value::String(_) => "string",
                                Value::Array(_) => "array",
                                Value::Object(_) => "object",
                            };
                            format!("{}:{kind}", k.chars().take(24).collect::<String>())
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| "no data object".into());
            anyhow::bail!(
                "the reply has an unexpected shape (data: {fields}); the message is not resent"
            );
        }
    };
    let (o, server, ts) = (owner, ack.server_id.clone(), ack.timestamp_ms);
    db.call(move |s| {
        s.e2ee_record_ack(&o, id, &server)?;
        s.e2ee_commit_echo(&o, id, &server, ts)
    })
    .await
}

impl Connection {
    /// Best effort; the outcome is in the report.
    pub(super) async fn e2ee_send_text(
        &mut self,
        db: &Database,
        events: &Events,
        path: &std::path::Path,
        text: &str,
        selector: Option<&str>,
    ) {
        let Some(mut report) = Report::open(path) else {
            return;
        };
        match self
            .send_text_inner(db, events, text, selector, &mut report)
            .await
        {
            Ok(message) => report.line(&format!("RESULT: {message}")),
            Err(error) => report.line(&format!("RESULT: stopped: {error:#}")),
        }
    }

    async fn send_text_inner(
        &mut self,
        db: &Database,
        events: &Events,
        text: &str,
        selector: Option<&str>,
        report: &mut Report,
    ) -> Result<&'static str> {
        let http = self
            .e2ee_http
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no HTTP context on this connection"))?;
        let owner = self.account.clone();
        // The first contact (in the list's order) that already has a session wins when no name is given.
        let candidates = self.choose_peers(db, selector, report).await?;
        let mut chosen = None;
        for uid in candidates.iter().take(super::e2ee_send_probe::MAX_TRIES) {
            let (o, u) = (owner.clone(), uid.clone());
            let has_session = db
                .call(move |s| {
                    let epoch = s
                        .e2ee_current_local(&o)?
                        .ok_or_else(|| anyhow::anyhow!("no local key"))?;
                    let local = e2ee::LocalNamespace {
                        epoch: epoch.to_string(),
                        platform: e2ee::Platform::Pc,
                    };
                    let mut any = false;
                    for device in [
                        DevicePlatform::Mobile,
                        DevicePlatform::Web,
                        DevicePlatform::Pc,
                    ] {
                        any |= existing_recipient(s, &o, &local, &u, device)?.is_some();
                    }
                    Ok(any)
                })
                .await?;
            if has_session {
                chosen = Some(uid.clone());
                break;
            }
        }
        let peer = chosen.ok_or_else(|| {
            anyhow::anyhow!("no contact with an existing session; run the handshake first (ZALO_E2EE_INIT_PEER)")
        })?;
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let prepared = prepare_authorized(db, &owner, &peer, text, now_ms).await?;
        report.line(&format!(
            "encrypted for {} device(s) and committed; posting to the service",
            prepared.devices
        ));
        let (message, conversation) = post_prepared(db, &http, &owner, &prepared, report).await?;
        events
            .emit(LiveEvent::Received {
                account_id: owner,
                message,
                conversation,
            })
            .await?;
        (events.repaint)();
        Ok("the message was sent and stored")
    }
}
