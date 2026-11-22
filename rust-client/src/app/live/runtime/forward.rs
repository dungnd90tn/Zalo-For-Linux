//! Ordinary direct text forwarding with per-recipient preflight and dispatch.
use super::*;
use crate::protocol::forward::{self as wire_forward, ForwardAck, ForwardReference};
use std::collections::{HashMap, VecDeque};
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
            || source.remote_id == account
            || (target.kind == ThreadKind::Direct && target.remote_id == account)
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

fn normal_source_revision(
    scope: &std::sync::RwLock<privacy::Projection>,
    thread: &str,
) -> Option<u64> {
    let scope = scope.read().ok()?;
    (scope.confirmed && scope.status == LivePrivacyStatus::Ready && !scope.hidden.contains(thread))
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

struct TtlSnapshot {
    // None means a duplicated destination row whose effective TTL cannot be proven.
    rows: HashMap<String, Option<bool>>,
    checked_at: Instant,
}

impl TtlSnapshot {
    fn enabled(&self, target: &Target) -> Result<bool, WireError> {
        // The source resets its entire TTL map on a verified snapshot.
        self.rows
            .get(&target.thread_id())
            .copied()
            .unwrap_or(Some(false))
            .ok_or(WireError::Invalid)
    }

    fn proof(&self, target: &Target) -> Result<TargetTtlProof, WireError> {
        if self.enabled(target)? {
            return Err(WireError::Unsupported);
        }
        Ok(TargetTtlProof {
            target: target.clone(),
            checked_at: self.checked_at,
        })
    }
}

fn parse_ttl_snapshot(payload: &Value, checked_at: Instant) -> Result<TtlSnapshot, WireError> {
    wire::service_code(payload)?;
    let rows = payload
        .pointer("/data/convers")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > MAX_AUTO_DELETE_THREADS {
        return Err(WireError::Oversize);
    }
    let mut destinations = HashMap::with_capacity(rows.len());
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
        let thread = Target::new(kind, &remote)?.thread_id();
        destinations
            .entry(thread)
            .and_modify(|previous| *previous = None)
            .or_insert(Some(ttl > 0));
    }
    Ok(TtlSnapshot {
        rows: destinations,
        checked_at,
    })
}

#[cfg(test)]
fn ttl_enabled(payload: &Value, target: &Target) -> Result<bool, WireError> {
    parse_ttl_snapshot(payload, Instant::now())?.enabled(target)
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
    Ok(fetch_ttl_snapshot(client, session).await?.proof(target)?)
}

async fn fetch_ttl_snapshot(
    client: &reqwest::Client,
    session: &VerifiedSession,
) -> Result<TtlSnapshot> {
    let mut url = service_url(session, "conversation", TTL_PATH)?;
    let params = session.encrypt_payload(b"{}")?;
    url.query_pairs_mut().append_pair("params", &params);
    let checked_at = Instant::now();
    let response = tokio::time::timeout(
        FORWARD_DEADLINE,
        authenticated_request(client, session, url, None),
    )
    .await??;
    Ok(parse_ttl_snapshot(&response, checked_at)?)
}

pub(super) enum ForwardDispatch {
    NotSent,
    Accepted(wire::SendAck),
    Rejected,
    Unknown,
}

struct GroupProof {
    request: group_roster::Request,
    roster: group_roster::Proof,
}

struct DispatchProof {
    ttl: TargetTtlProof,
    group: Option<GroupProof>,
}

#[derive(Clone, Copy)]
struct Transport<'a> {
    session: &'a Arc<VerifiedSession>,
    client: &'a reqwest::Client,
}

impl GroupProof {
    fn applies(&self, account: &str, target: &Target) -> bool {
        self.request.thread == target.thread_id()
            && self.roster.allows_forward(&self.request, account)
    }
}

struct PreparedDispatch {
    client_id: u64,
    target: Target,
    url: Url,
    body: String,
}

impl PreparedDispatch {
    /// Call with the immutable intent returned by the source-validation transaction.
    fn new(
        session: &VerifiedSession,
        request: &ForwardRequest,
        client_id: u64,
        text: &str,
        reference: &ForwardReference,
    ) -> Result<Self> {
        let (_, target) = request.targets(session.account_id())?;
        let path = match target.kind {
            ThreadKind::Direct => wire_forward::DIRECT_PATH,
            ThreadKind::Group => wire_forward::GROUP_PATH,
        };
        let url = service_url(session, "file", path)?;
        let params = wire_forward::parameters(&target, client_id, session.imei(), text, reference)?;
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

    /// Start only after fresh authenticated capability verification.
    /// A failure after handing the request to HTTP is not proof of rejection.
    async fn send(
        self,
        client: &reqwest::Client,
        session: &VerifiedSession,
        request: &ForwardRequest,
        privacy: &std::sync::RwLock<privacy::Projection>,
        revision: u64,
        proof: DispatchProof,
    ) -> ForwardDispatch {
        if !proof.ttl.applies(&self.target)
            || normal_scope_revision(privacy, request) != Some(revision)
            || self.target.kind == ThreadKind::Group
                && !proof
                    .group
                    .is_some_and(|group| group.applies(session.account_id(), &self.target))
        {
            return ForwardDispatch::NotSent;
        }
        let response = tokio::time::timeout(
            FORWARD_DEADLINE,
            authenticated_request(client, session, self.url, Some(self.body)),
        )
        .await;
        match response {
            Ok(Ok(payload)) => match wire_forward::ack(&payload, &self.target, self.client_id) {
                Ok(ForwardAck::Accepted(ack)) => ForwardDispatch::Accepted(ack),
                Ok(ForwardAck::Rejected) => ForwardDispatch::Rejected,
                Err(_) => ForwardDispatch::Unknown,
            },
            _ => ForwardDispatch::Unknown,
        }
    }
}

enum Task {
    Ttl(tokio::task::JoinHandle<Result<TtlSnapshot>>),
    Group(tokio::task::JoinHandle<Result<GroupProof>>),
    Ready,
    Dispatch {
        id: u64,
        task: tokio::task::JoinHandle<ForwardDispatch>,
    },
}

struct Running {
    request: ForwardRequest,
    task: Task,
}

impl Drop for Running {
    fn drop(&mut self) {
        match &self.task {
            Task::Ttl(task) => task.abort(),
            Task::Group(task) => task.abort(),
            Task::Dispatch { task, .. } => task.abort(),
            Task::Ready => {}
        }
        // A durable sending row is recovered as unknown after cancellation.
    }
}

enum Phase {
    Ttl(Option<TtlSnapshot>),
    Group(Result<GroupProof>),
    Ready,
    Dispatch { id: u64, result: ForwardDispatch },
}

pub(super) struct Completion {
    request: ForwardRequest,
    phase: Phase,
}

struct Work {
    request_id: u64,
    source_thread: String,
    source_message_id: String,
    batch: bool,
    pending: VecDeque<String>,
    results: Vec<ForwardBatchResult>,
    revision: u64,
    authenticated_at: Option<Instant>,
    ttl: Option<TtlSnapshot>,
}

impl Work {
    fn next(&mut self) -> Option<ForwardRequest> {
        Some(ForwardRequest {
            request_id: self.request_id,
            source_thread: self.source_thread.clone(),
            source_message_id: self.source_message_id.clone(),
            target_thread: self.pending.pop_front()?,
        })
    }

    fn record(&mut self, target_thread: String, outcome: ForwardOutcome) {
        self.results.push(ForwardBatchResult {
            target_thread,
            outcome,
        });
    }

    fn skip(&mut self) {
        self.results.extend(
            self.pending
                .drain(..)
                .map(|target_thread| ForwardBatchResult {
                    target_thread,
                    outcome: ForwardOutcome::Unavailable,
                }),
        );
    }

    async fn finish(mut self, events: &Events) -> Result<()> {
        let event = if self.batch {
            LiveEvent::ForwardBatchFinished {
                account_id: events.account.clone(),
                request_id: self.request_id,
                source_thread: self.source_thread,
                source_message_id: self.source_message_id,
                results: self.results,
            }
        } else {
            let result = self.results.pop().expect("single forward has one result");
            LiveEvent::ForwardFinished {
                account_id: events.account.clone(),
                request_id: self.request_id,
                source_thread: self.source_thread,
                source_message_id: self.source_message_id,
                target_thread: result.target_thread,
                outcome: result.outcome,
            }
        };
        events.emit(event).await
    }
}

#[derive(Default)]
pub(super) struct Forwarding {
    work: Option<Work>,
    running: Option<Running>,
}

enum Attempt {
    Dispatched,
    Finished(ForwardOutcome),
    Pause(anyhow::Error),
}

fn authenticated_at(connection: Option<&Connection>) -> Option<Instant> {
    connection
        .filter(|socket| socket.decoder.authenticated())
        .and_then(|socket| socket.authenticated_at)
}

impl Forwarding {
    pub async fn next(&mut self) -> Completion {
        let Some(job) = &mut self.running else {
            return std::future::pending().await;
        };
        let phase = match &mut job.task {
            Task::Ttl(task) => Phase::Ttl(task.await.ok().and_then(Result::ok)),
            Task::Group(task) => Phase::Group(task.await.unwrap_or_else(|error| Err(error.into()))),
            Task::Ready => {
                // Give queued local work a turn before another synchronous socket proof.
                tokio::task::yield_now().await;
                Phase::Ready
            }
            Task::Dispatch { id, task } => Phase::Dispatch {
                id: *id,
                result: task.await.unwrap_or(ForwardDispatch::Unknown),
            },
        };
        let job = self.running.take().expect("awaited forward has an owner");
        Completion {
            request: job.request.clone(),
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
        let (request_id, source_thread, source_message_id, target_threads, batch) = match command {
            LiveCommand::ForwardTargets {
                generation,
                source_thread,
                source_message_id,
                query,
            } => {
                let permitted = normal_source_revision(&events.privacy, &source_thread).is_some();
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
                return Ok(None);
            }
            LiveCommand::Forward {
                request_id,
                source_thread,
                source_message_id,
                target_thread,
            } => (
                request_id,
                source_thread,
                source_message_id,
                vec![target_thread],
                false,
            ),
            LiveCommand::ForwardBatch {
                request_id,
                source_thread,
                source_message_id,
                target_threads,
            } => (
                request_id,
                source_thread,
                source_message_id,
                target_threads,
                true,
            ),
            other => return Ok(Some(other)),
        };
        let revision = normal_source_revision(&events.privacy, &source_thread);
        let valid = request_id != 0
            && Target::from_thread(&source_thread).is_ok_and(|target| {
                target.kind == ThreadKind::Direct && target.remote_id != events.account
            })
            && wire::numeric_id(&source_message_id).is_ok()
            && valid_forward_targets(&target_threads, &events.account);
        let at = authenticated_at(connection);
        let mut work = Work {
            request_id,
            source_thread,
            source_message_id,
            batch,
            pending: target_threads.into_iter().take(MAX_FORWARD_BATCH).collect(),
            results: Vec::new(),
            revision: revision.unwrap_or_default(),
            authenticated_at: at,
            ttl: None,
        };
        if !valid
            || revision.is_none()
            || at.is_none()
            || self.work.is_some()
            || root_for(session, "file").is_err()
        {
            work.skip();
            work.finish(events).await?;
        } else {
            self.work = Some(work);
            self.advance(at, events, session, client).await?;
        }
        Ok(None)
    }

    async fn advance(
        &mut self,
        at: Option<Instant>,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let work = self.work.as_mut().expect("forward has an active request");
        if at != work.authenticated_at
            || normal_source_revision(&events.privacy, &work.source_thread) != Some(work.revision)
        {
            work.skip();
        }
        while let Some(request) = work.next() {
            if normal_scope_revision(&events.privacy, &request) != Some(work.revision) {
                work.record(request.target_thread, ForwardOutcome::Unavailable);
                continue;
            }
            let task = if work
                .ttl
                .as_ref()
                .is_some_and(|ttl| ttl.checked_at.elapsed() <= TTL_SNAPSHOT_AGE)
            {
                Task::Ready
            } else {
                let (session, client) = (session.clone(), client.clone());
                Task::Ttl(tokio::spawn(async move {
                    fetch_ttl_snapshot(&client, &session).await
                }))
            };
            self.running = Some(Running { request, task });
            return Ok(());
        }
        self.work
            .take()
            .expect("drained forward has an owner")
            .finish(events)
            .await
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
        let Completion { request, phase } = completion;
        let at = authenticated_at(connection.as_deref());
        let transport = Transport { session, client };
        let attempt = match phase {
            Phase::Ttl(None) => {
                // A failed full snapshot cannot certify any remaining peer.
                let mut work = self.work.take().expect("TTL belongs to a forward");
                work.record(request.target_thread, ForwardOutcome::Unavailable);
                work.skip();
                work.finish(events).await?;
                return Ok(CommandOutcome::Continue);
            }
            Phase::Ttl(Some(ttl)) => {
                self.work.as_mut().expect("TTL belongs to a forward").ttl = Some(ttl);
                self.launch(&request, connection, db, events, transport, None)
                    .await?
            }
            Phase::Ready => {
                self.launch(&request, connection, db, events, transport, None)
                    .await?
            }
            Phase::Group(Ok(proof)) => {
                self.launch(&request, connection, db, events, transport, Some(proof))
                    .await?
            }
            Phase::Group(Err(error)) if wire_error(&error) == Some(&WireError::Authentication) => {
                Attempt::Pause(error)
            }
            Phase::Group(Err(_)) => Attempt::Finished(ForwardOutcome::Unavailable),
            Phase::Dispatch { id, result } => {
                Attempt::Finished(settle(db, events, id, result).await?)
            }
        };
        match attempt {
            Attempt::Dispatched => {}
            Attempt::Finished(outcome) => {
                self.work
                    .as_mut()
                    .expect("active forward")
                    .record(request.target_thread, outcome);
                self.advance(at, events, session, client).await?;
            }
            Attempt::Pause(error) => {
                let mut work = self.work.take().expect("active forward");
                work.record(request.target_thread, ForwardOutcome::Unavailable);
                work.skip();
                work.finish(events).await?;
                let (stage, connection_state) = if error.downcast_ref::<StorageContext>().is_some()
                {
                    (LiveStage::Storage, LiveConnection::Offline)
                } else {
                    (LiveStage::Authentication, LiveConnection::Expired)
                };
                events.diagnostic(diagnostic(&error, stage)).await?;
                events.connection(connection_state).await?;
                return Ok(CommandOutcome::PauseTransport);
            }
        }
        Ok(CommandOutcome::Continue)
    }

    async fn launch(
        &mut self,
        request: &ForwardRequest,
        connection: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        transport: Transport<'_>,
        group: Option<GroupProof>,
    ) -> Result<Attempt> {
        let Transport { session, client } = transport;
        let work = self.work.as_ref().expect("preflight belongs to a forward");
        let revision = work.revision;
        let (_, target) = request.targets(&events.account)?;
        let ttl = work
            .ttl
            .as_ref()
            .and_then(|snapshot| snapshot.proof(&target).ok());
        let at = authenticated_at(connection.as_deref());
        let (Some(ttl), Some(socket)) = (ttl, connection) else {
            return Ok(Attempt::Finished(ForwardOutcome::Unavailable));
        };
        let permits_dispatch = || {
            ttl.applies(&target)
                && normal_scope_revision(&events.privacy, request) == Some(revision)
        };
        if at != work.authenticated_at || !permits_dispatch() {
            return Ok(Attempt::Finished(ForwardOutcome::Unavailable));
        }
        match target.kind {
            ThreadKind::Direct => {
                if let Err(error) = socket.verify_direct(&target, db, events).await {
                    return Ok(
                        if error.downcast_ref::<StorageContext>().is_some()
                            || wire_error(&error) == Some(&WireError::Authentication)
                        {
                            Attempt::Pause(error)
                        } else {
                            Attempt::Finished(ForwardOutcome::Unavailable)
                        },
                    );
                }
            }
            ThreadKind::Group => match &group {
                None => {
                    let (client, session, scope) =
                        (client.clone(), session.clone(), events.privacy.clone());
                    let roster_request = group_roster::Request {
                        thread: request.target_thread.clone(),
                        privacy_revision: revision,
                        after: None,
                    };
                    self.running = Some(Running {
                        request: request.clone(),
                        task: Task::Group(tokio::spawn(async move {
                            let (visibility, roster) = tokio::try_join!(
                                verify_group(&client, &session, &target),
                                group_roster::verify_members(
                                    &client,
                                    &session,
                                    &scope,
                                    &roster_request
                                ),
                            )?;
                            if visibility != 0
                                || !roster.allows_forward(&roster_request, session.account_id())
                            {
                                return Err(WireError::Unsupported.into());
                            }
                            Ok(GroupProof {
                                request: roster_request,
                                roster,
                            })
                        })),
                    });
                    return Ok(Attempt::Dispatched);
                }
                Some(proof) if !proof.applies(&events.account, &target) => {
                    return Ok(Attempt::Finished(ForwardOutcome::Unavailable));
                }
                Some(_) => {}
            },
        }
        if authenticated_at(Some(socket)) != at || !permits_dispatch() {
            return Ok(Attempt::Finished(ForwardOutcome::Unavailable));
        }
        let (account, source, message, destination) = (
            events.account.clone(),
            request.source_thread.clone(),
            request.source_message_id.clone(),
            request.target_thread.clone(),
        );
        let prepared = db
            .call(move |store| {
                store.prepare_forward(&account, &source, &message, &destination, now_ms())
            })
            .await;
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                use crate::store::forward::ForwardFailure;
                let outcome = if error.downcast_ref::<ForwardFailure>()
                    == Some(&ForwardFailure::Unresolved)
                {
                    ForwardOutcome::Unknown
                } else {
                    ForwardOutcome::Unavailable
                };
                return Ok(Attempt::Finished(outcome));
            }
        };
        let id = prepared.client_id;
        let Ok(dispatch) =
            PreparedDispatch::new(session, request, id, &prepared.text, &prepared.reference)
        else {
            return Ok(Attempt::Finished(
                settle(db, events, id, ForwardDispatch::NotSent).await?,
            ));
        };
        let (session, client, scope, submitted) = (
            session.clone(),
            client.clone(),
            events.privacy.clone(),
            request.clone(),
        );
        let db = db.handle()?;
        self.running = Some(Running {
            request: request.clone(),
            task: Task::Dispatch {
                id,
                task: tokio::spawn(async move {
                    if group.is_some() {
                        let account = session.account_id().to_owned();
                        let destination = submitted.target_thread.clone();
                        if db
                            .call(move |store| {
                                store.forward_destination_valid(&account, &destination)
                            })
                            .await
                            .is_err()
                        {
                            return ForwardDispatch::NotSent;
                        }
                    }
                    dispatch
                        .send(
                            &client,
                            &session,
                            &submitted,
                            &scope,
                            revision,
                            DispatchProof { ttl, group },
                        )
                        .await
                }),
            },
        });
        Ok(Attempt::Dispatched)
    }
}

async fn settle(
    db: &Database,
    events: &Events,
    id: u64,
    dispatch: ForwardDispatch,
) -> Result<ForwardOutcome> {
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
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ready_recipient_yields_to_queued_control() {
        let mut forwarding = Forwarding {
            running: Some(Running {
                request: ForwardRequest {
                    request_id: 1,
                    source_thread: "d:22".into(),
                    source_message_id: "7001".into(),
                    target_thread: "d:33".into(),
                },
                task: Task::Ready,
            }),
            ..Default::default()
        };

        tokio::select! {
            biased;
            _ = forwarding.next() => panic!("the next recipient ran ahead of queued control"),
            _ = std::future::ready(()) => {}
        }
        assert!(matches!(forwarding.next().await.phase, Phase::Ready));
    }

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
