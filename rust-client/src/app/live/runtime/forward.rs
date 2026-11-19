//! One-recipient ordinary forward preflight and authenticated HTTP dispatch.
use super::*;
use crate::protocol::forward::{self as wire_forward, ForwardAck, ForwardReference};
use tokio::time::Instant;

const TTL_PATH: &str = "/api/conv/autodelete/getConvers";
const TTL_SNAPSHOT_AGE: Duration = Duration::from_secs(20);
const MAX_AUTO_DELETE_THREADS: usize = 10_000;
const FORWARD_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub(super) struct ForwardRequest {
    pub request_id: u64,
    pub source_thread: String,
    pub source_message_id: String,
    pub target_thread: String,
}

impl ForwardRequest {
    pub fn targets(&self, account: &str) -> Result<(Target, Target), WireError> {
        if self.request_id == 0 {
            return Err(WireError::Invalid);
        }
        wire::numeric_id(account)?;
        wire::numeric_id(&self.source_message_id)?;
        let source = Target::from_thread(&self.source_thread)?;
        let target = Target::from_thread(&self.target_thread)?;
        if source.kind != ThreadKind::Direct
            || target.kind != ThreadKind::Direct
            || target.remote_id == account
            || source.remote_id == account
        {
            return Err(WireError::Unsupported);
        }
        Ok((source, target))
    }
}

pub(super) fn normal_scope_revision(
    scope: &std::sync::RwLock<privacy::Projection>,
    request: &ForwardRequest,
) -> Option<u64> {
    let scope = scope.read().ok()?;
    (scope.confirmed
        && scope.status == LivePrivacyStatus::Ready
        && !scope.hidden.contains(&request.source_thread)
        && !scope.hidden.contains(&request.target_thread))
    .then_some(scope.revision)
}

pub(super) struct TargetTtlProof {
    target: Target,
    checked_at: Instant,
}

impl TargetTtlProof {
    pub(super) fn applies(&self, target: &Target) -> bool {
        self.target == *target && self.checked_at.elapsed() <= TTL_SNAPSHOT_AGE
    }
}

fn ttl_enabled(payload: &Value, target: &Target) -> Result<bool, WireError> {
    wire::service_code(payload)?;
    let rows = payload
        .pointer("/data/convers")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > MAX_AUTO_DELETE_THREADS {
        return Err(WireError::Oversize);
    }
    let mut target_ttl = None;
    for row in rows {
        let remote = wire::id_string(row.get("destId").ok_or(WireError::Invalid)?)?;
        let kind = match row.get("isGroup").ok_or(WireError::Invalid)? {
            Value::Bool(true) => ThreadKind::Group,
            Value::Bool(false) => ThreadKind::Direct,
            value if value.as_u64() == Some(1) => ThreadKind::Group,
            value if value.as_u64() == Some(0) => ThreadKind::Direct,
            _ => return Err(WireError::Invalid),
        };
        let ttl = row
            .get("ttl")
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .ok_or(WireError::Invalid)?;
        if kind == target.kind && remote == target.remote_id && target_ttl.replace(ttl).is_some() {
            return Err(WireError::Invalid);
        }
    }
    // The source resets its entire TTL map on a verified getConvers snapshot.
    Ok(target_ttl.is_some_and(|ttl| ttl > 0))
}

fn service_url(session: &VerifiedSession, service: &str, path: &str) -> Result<Url> {
    let mut url = root_for(session, service)?;
    wire::validate_origin(&url, "https")?;
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type());
    Ok(url)
}

/// Launch as a scoped background task while the actor continues servicing pushes.
pub(super) async fn verify_target_ttl(
    client: &reqwest::Client,
    session: &VerifiedSession,
    target: &Target,
) -> Result<TargetTtlProof> {
    let mut url = service_url(session, "conversation", TTL_PATH)?;
    let params = session.encrypt_payload(b"{}")?;
    url.query_pairs_mut().append_pair("params", &params);
    let checked_at = Instant::now();
    let response = tokio::time::timeout(
        FORWARD_DEADLINE,
        authenticated_request(client, session, url, None),
    )
    .await??;
    if ttl_enabled(&response, target)? {
        return Err(WireError::Unsupported.into());
    }
    Ok(TargetTtlProof {
        target: target.clone(),
        checked_at,
    })
}

pub(super) enum ForwardDispatch {
    NotSent,
    Accepted(wire::SendAck),
    Rejected,
    Unknown,
}

pub(super) struct PreparedDispatch {
    client_id: u64,
    target: Target,
    url: Url,
    body: String,
}

impl PreparedDispatch {
    /// Call with the immutable intent returned by the source-validation transaction.
    pub fn new(
        session: &VerifiedSession,
        request: &ForwardRequest,
        client_id: u64,
        text: &str,
        reference: &ForwardReference,
    ) -> Result<Self> {
        let (_, target) = request.targets(session.account_id())?;
        let url = service_url(session, "file", wire_forward::DIRECT_PATH)?;
        let params =
            wire_forward::direct_parameters(&target, client_id, session.imei(), text, reference)?;
        let encrypted = session.encrypt_payload(&serde_json::to_vec(&params)?)?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("params", &encrypted)
            .finish();
        Ok(Self {
            client_id,
            target,
            url,
            body,
        })
    }

    /// Start only after fresh authenticated direct capability verification.
    /// A failure after handing the request to HTTP is not proof of rejection.
    pub async fn send(
        self,
        client: &reqwest::Client,
        session: &VerifiedSession,
        request: &ForwardRequest,
        privacy: &std::sync::RwLock<privacy::Projection>,
        revision: u64,
        ttl: TargetTtlProof,
    ) -> ForwardDispatch {
        if !ttl.applies(&self.target) || normal_scope_revision(privacy, request) != Some(revision) {
            return ForwardDispatch::NotSent;
        }
        let response = tokio::time::timeout(
            FORWARD_DEADLINE,
            authenticated_request(client, session, self.url, Some(self.body)),
        )
        .await;
        match response {
            Ok(Ok(payload)) => {
                match wire_forward::direct_ack(&payload, &self.target, self.client_id) {
                    Ok(ForwardAck::Accepted(ack)) => ForwardDispatch::Accepted(ack),
                    Ok(ForwardAck::Rejected) => ForwardDispatch::Rejected,
                    Err(_) => ForwardDispatch::Unknown,
                }
            }
            _ => ForwardDispatch::Unknown,
        }
    }
}

enum Task {
    Ttl(tokio::task::JoinHandle<Result<TargetTtlProof>>),
    Dispatch {
        id: u64,
        task: tokio::task::JoinHandle<ForwardDispatch>,
    },
}

struct Running {
    request: ForwardRequest,
    revision: u64,
    authenticated_at: Instant,
    task: Task,
}

impl Drop for Running {
    fn drop(&mut self) {
        match &self.task {
            Task::Ttl(task) => task.abort(),
            Task::Dispatch { task, .. } => task.abort(),
        }
        // A durable sending row is recovered as unknown after cancellation.
    }
}

pub(super) enum Phase {
    Ttl(Option<TargetTtlProof>),
    Dispatch { id: u64, result: ForwardDispatch },
}

pub(super) struct Completion {
    request: ForwardRequest,
    revision: u64,
    authenticated_at: Instant,
    phase: Phase,
}

#[derive(Default)]
pub(super) struct Forwarding {
    running: Option<Running>,
}

impl Forwarding {
    pub async fn next(&mut self) -> Completion {
        let Some(job) = &mut self.running else {
            return std::future::pending().await;
        };
        let phase = match &mut job.task {
            Task::Ttl(task) => Phase::Ttl(task.await.ok().and_then(Result::ok)),
            Task::Dispatch { id, task } => Phase::Dispatch {
                id: *id,
                result: task.await.unwrap_or(ForwardDispatch::Unknown),
            },
        };
        let job = self.running.take().expect("awaited forward has an owner");
        Completion {
            request: job.request.clone(),
            revision: job.revision,
            authenticated_at: job.authenticated_at,
            phase,
        }
    }

    pub async fn command(
        &mut self,
        command: LiveCommand,
        connection: Option<&Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<Option<LiveCommand>> {
        match command {
            LiveCommand::ForwardTargets {
                generation,
                source_thread,
                source_message_id,
                query,
            } => {
                let permitted = events.privacy.read().is_ok_and(|scope| {
                    scope.confirmed
                        && scope.status == LivePrivacyStatus::Ready
                        && !scope.hidden.contains(&source_thread)
                });
                let result = if permitted {
                    let (account, source, message, query) = (
                        events.account.clone(),
                        source_thread.clone(),
                        source_message_id.clone(),
                        query.clone(),
                    );
                    db.call(move |store| store.forward_targets(&account, &source, &message, &query))
                        .await
                } else {
                    Err(WireError::Unsupported.into())
                };
                let failed = result.is_err();
                events
                    .emit(LiveEvent::ForwardTargets {
                        account_id: events.account.clone(),
                        generation,
                        source_thread,
                        source_message_id,
                        query,
                        entries: result.unwrap_or_default(),
                        failed,
                    })
                    .await?;
            }
            LiveCommand::Forward {
                request_id,
                source_thread,
                source_message_id,
                target_thread,
            } => {
                let request = ForwardRequest {
                    request_id,
                    source_thread,
                    source_message_id,
                    target_thread,
                };
                let Some((revision, authenticated_at, target)) = ({
                    let target = request.targets(&events.account).ok().map(|(_, t)| t);
                    let at = connection
                        .filter(|socket| socket.decoder.authenticated())
                        .and_then(|socket| socket.authenticated_at);
                    let revision = normal_scope_revision(&events.privacy, &request);
                    revision.zip(at).zip(target).map(|((r, at), t)| (r, at, t))
                })
                .filter(|_| self.running.is_none() && root_for(session, "file").is_ok()) else {
                    finish(events, request, ForwardOutcome::Unavailable).await?;
                    return Ok(None);
                };
                let (session, client) = (session.clone(), client.clone());
                self.running = Some(Running {
                    request,
                    revision,
                    authenticated_at,
                    task: Task::Ttl(tokio::spawn(async move {
                        verify_target_ttl(&client, &session, &target).await
                    })),
                });
            }
            other => return Ok(Some(other)),
        }
        Ok(None)
    }

    pub async fn complete(
        &mut self,
        completion: Completion,
        connection: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<CommandOutcome> {
        let Completion {
            request,
            revision,
            authenticated_at,
            phase,
        } = completion;
        match phase {
            Phase::Ttl(Some(ttl)) => {
                let (_, target) = request.targets(&events.account)?;
                let socket = connection.filter(|socket| {
                    socket.decoder.authenticated()
                        && socket.authenticated_at == Some(authenticated_at)
                });
                let Some(socket) = socket.filter(|_| {
                    ttl.applies(&target)
                        && normal_scope_revision(&events.privacy, &request) == Some(revision)
                }) else {
                    finish(events, request, ForwardOutcome::Unavailable).await?;
                    return Ok(CommandOutcome::Continue);
                };
                if let Err(error) = socket.verify_direct(&target, db, events).await {
                    finish(events, request, ForwardOutcome::Unavailable).await?;
                    if error.downcast_ref::<StorageContext>().is_some() {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Storage))
                            .await?;
                        events.connection(LiveConnection::Offline).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                    if wire_error(&error) == Some(&WireError::Authentication) {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Authentication))
                            .await?;
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                    return Ok(CommandOutcome::Continue);
                }
                if !ttl.applies(&target)
                    || normal_scope_revision(&events.privacy, &request) != Some(revision)
                {
                    finish(events, request, ForwardOutcome::Unavailable).await?;
                    return Ok(CommandOutcome::Continue);
                }
                let (account, source, message, target) = (
                    events.account.clone(),
                    request.source_thread.clone(),
                    request.source_message_id.clone(),
                    request.target_thread.clone(),
                );
                let prepared = db
                    .call(move |store| {
                        store.prepare_forward(&account, &source, &message, &target, now_ms())
                    })
                    .await;
                let prepared = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        let outcome = if error
                            .downcast_ref::<crate::store::forward::ForwardFailure>()
                            == Some(&crate::store::forward::ForwardFailure::Unresolved)
                        {
                            ForwardOutcome::Unknown
                        } else {
                            ForwardOutcome::Unavailable
                        };
                        finish(events, request, outcome).await?;
                        return Ok(CommandOutcome::Continue);
                    }
                };
                let id = prepared.client_id;
                let dispatch = PreparedDispatch::new(
                    session,
                    &request,
                    id,
                    &prepared.text,
                    &prepared.reference,
                );
                let Ok(dispatch) = dispatch else {
                    settle(db, events, request, id, ForwardDispatch::NotSent).await?;
                    return Ok(CommandOutcome::Continue);
                };
                let (session, client, scope) =
                    (session.clone(), client.clone(), events.privacy.clone());
                let submitted = request.clone();
                self.running = Some(Running {
                    request,
                    revision,
                    authenticated_at,
                    task: Task::Dispatch {
                        id,
                        task: tokio::spawn(async move {
                            dispatch
                                .send(&client, &session, &submitted, &scope, revision, ttl)
                                .await
                        }),
                    },
                });
            }
            Phase::Ttl(None) => finish(events, request, ForwardOutcome::Unavailable).await?,
            Phase::Dispatch { id, result } => settle(db, events, request, id, result).await?,
        }
        Ok(CommandOutcome::Continue)
    }
}

async fn settle(
    db: &Database,
    events: &Events,
    request: ForwardRequest,
    id: u64,
    dispatch: ForwardDispatch,
) -> Result<()> {
    use crate::store::forward::ForwardOutcome as Stored;
    let account = events.account.clone();
    let (outcome, projection) = db
        .call(move |store| {
            let accepted = matches!(dispatch, ForwardDispatch::Accepted(_));
            let not_sent = matches!(dispatch, ForwardDispatch::NotSent);
            match dispatch {
                ForwardDispatch::Accepted(ack) => {
                    // Record remote proof before attempting to construct local history.
                    let _ =
                        store.record_forward_ack(&account, id, &ack.server_id, ack.timestamp_ms);
                }
                ForwardDispatch::Rejected | ForwardDispatch::NotSent => {
                    store.fail_forward(&account, id, false)?;
                }
                ForwardDispatch::Unknown => store.fail_forward(&account, id, true)?,
            }
            let result = match store.forward_outcome(&account, id)? {
                Stored::Acknowledged | Stored::Accepted => (
                    ForwardOutcome::Accepted,
                    store.accept_forward_ack(&account, id).ok(),
                ),
                Stored::Rejected if not_sent => (ForwardOutcome::Unavailable, None),
                Stored::Rejected => (ForwardOutcome::Rejected, None),
                Stored::Pending | Stored::Unknown => {
                    if accepted {
                        store.fail_forward(&account, id, true)?;
                    }
                    (ForwardOutcome::Unknown, None)
                }
            };
            Ok(result)
        })
        .await
        .unwrap_or((ForwardOutcome::Unknown, None));
    if let Some((message, conversation)) = projection {
        events
            .emit(LiveEvent::Received {
                account_id: events.account.clone(),
                message,
                conversation,
            })
            .await?;
    }
    finish(events, request, outcome).await
}

async fn finish(events: &Events, request: ForwardRequest, outcome: ForwardOutcome) -> Result<()> {
    events
        .emit(LiveEvent::ForwardFinished {
            account_id: events.account.clone(),
            request_id: request.request_id,
            source_thread: request.source_thread,
            source_message_id: request.source_message_id,
            target_thread: request.target_thread,
            outcome,
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_policy_rejects_positive_target_and_does_not_guess_missing_snapshot() {
        let direct = Target::new(ThreadKind::Direct, "33").unwrap();
        let snapshot = |convers| json!({"error_code":0,"data":{"convers":convers}});
        for (rows, expected) in [
            (json!([]), false),
            (json!([{"destId":"33","isGroup":false,"ttl":0}]), false),
            (json!([{"destId":"33","isGroup":true,"ttl":3600}]), false),
            (json!([{"destId":"33","isGroup":false,"ttl":3600}]), true),
            (json!([{"destId":33,"isGroup":0,"ttl":"3600"}]), true),
        ] {
            assert_eq!(ttl_enabled(&snapshot(rows), &direct), Ok(expected));
        }
        for payload in [
            json!({"error_code":0,"data":{}}),
            snapshot(Value::Null),
            snapshot(json!([{"destId":"33","isGroup":false,"ttl":-1}])),
            snapshot(json!([{"destId":"33","isGroup":false,"ttl":"bad"}])),
            snapshot(json!([
                {"destId":"33","isGroup":false,"ttl":3600},
                {"destId":"33","isGroup":false,"ttl":0}
            ])),
        ] {
            assert_eq!(ttl_enabled(&payload, &direct), Err(WireError::Invalid));
        }
    }

    #[test]
    fn forwarding_scope_excludes_both_hidden_threads_even_if_unlocked() {
        let request = ForwardRequest {
            request_id: 5,
            source_thread: "d:22".into(),
            source_message_id: "7001".into(),
            target_thread: "d:33".into(),
        };
        let scope = std::sync::RwLock::new(privacy::Projection {
            revision: 7,
            confirmed: true,
            ..Default::default()
        });
        assert!(request.targets("11").is_ok());
        assert!(request.targets("33").is_err());
        assert_eq!(normal_scope_revision(&scope, &request), Some(7));
        for hidden in [&request.source_thread, &request.target_thread] {
            let mut current = scope.write().unwrap();
            current.hidden.clear();
            current.hidden.insert(hidden.clone());
            current.unlocked = Some(hidden.clone());
            drop(current);
            assert_eq!(normal_scope_revision(&scope, &request), None);
        }
    }
}
