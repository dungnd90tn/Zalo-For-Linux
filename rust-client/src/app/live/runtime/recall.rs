//! Scoped online dispatch for one ordinary direct-message recall.
use super::*;
use crate::protocol::recall::{self as wire_recall, RecallReference};
use crate::store::recall::{RecallChange, RecallFailure, RecallOutcome as StoredOutcome};

const RECALL_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub(super) struct RecallRequest {
    pub request_id: u64,
    pub thread: String,
    pub message_id: String,
}

impl RecallRequest {
    pub fn target(&self, account: &str) -> Result<Target, WireError> {
        wire::numeric_id(account)?;
        wire::numeric_id(&self.message_id)?;
        let target = Target::from_thread(&self.thread)?;
        if self.request_id == 0 || target.kind != ThreadKind::Direct || target.remote_id == account
        {
            return Err(WireError::Unsupported);
        }
        Ok(target)
    }
}

pub(super) fn normal_scope_revision(
    privacy: &std::sync::RwLock<privacy::Projection>,
    thread: &str,
) -> Option<u64> {
    let privacy = privacy.read().ok()?;
    (privacy.confirmed
        && privacy.status == LivePrivacyStatus::Ready
        && !privacy.hidden.contains(thread))
    .then_some(privacy.revision)
}

pub(super) enum DispatchOutcome {
    NotSent,
    Accepted,
    Rejected,
    Unknown,
}

pub(super) struct PreparedDispatch {
    thread: String,
    url: Url,
    body: String,
    original: RecallReference,
    max_age_ms: i64,
}

impl PreparedDispatch {
    /// The caller reserves an exact scope-bound intent and verifies ordinary
    /// sending eligibility on the current authenticated socket before dispatch.
    pub fn new(
        session: &VerifiedSession,
        request: &RecallRequest,
        client_id: u64,
        original: RecallReference,
        max_age_ms: i64,
    ) -> Result<Self> {
        let target = request.target(session.account_id())?;
        if request.message_id != original.message_id
            || !original.within_window(now_ms(), max_age_ms)
        {
            return Err(WireError::Unsupported.into());
        }
        let mut url = root_for(session, "chat")?;
        wire::validate_origin(&url, "https")?;
        url.set_path(wire_recall::DIRECT_PATH);
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", session.client_type().api_type());
        let parameters = wire_recall::direct_parameters(&target, client_id, &original)?;
        let encrypted = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("params", &encrypted)
            .finish();
        Ok(Self {
            thread: request.thread.clone(),
            url,
            body,
            original,
            max_age_ms,
        })
    }

    /// Issue the authorized authenticated request once. Any interrupted,
    /// malformed or otherwise unproven response leaves the durable row unknown.
    pub async fn send(
        self,
        client: &reqwest::Client,
        session: &VerifiedSession,
        privacy: &std::sync::RwLock<privacy::Projection>,
        revision: u64,
    ) -> DispatchOutcome {
        if normal_scope_revision(privacy, &self.thread) != Some(revision)
            || !self.original.within_window(now_ms(), self.max_age_ms)
        {
            return DispatchOutcome::NotSent;
        }
        match tokio::time::timeout(
            RECALL_DEADLINE,
            authenticated_request(client, session, self.url, Some(self.body)),
        )
        .await
        {
            Ok(Ok(payload)) if wire_recall::direct_ack(&payload).is_ok() => {
                DispatchOutcome::Accepted
            }
            Ok(Err(error)) if wire_error(&error) == Some(&WireError::Service(211)) => {
                DispatchOutcome::Rejected
            }
            _ => DispatchOutcome::Unknown,
        }
    }
}

pub(super) struct RunningRecall {
    request: RecallRequest,
    client_id: u64,
    task: tokio::task::JoinHandle<DispatchOutcome>,
}

impl Drop for RunningRecall {
    fn drop(&mut self) {
        self.task.abort();
        // A reserved request remains durable, and startup classifies it as unknown.
    }
}

#[derive(Default)]
pub(super) struct Recalls {
    running: Option<RunningRecall>,
}

pub(super) enum Command {
    Handled,
    Continue(LiveCommand),
    PauseTransport,
}

impl Recalls {
    pub async fn next(&mut self) -> (RecallRequest, u64, DispatchOutcome) {
        let Some(job) = &mut self.running else {
            return std::future::pending().await;
        };
        let result = (&mut job.task).await.unwrap_or(DispatchOutcome::Unknown);
        let job = self.running.take().expect("awaited recall has an owner");
        (job.request.clone(), job.client_id, result)
    }

    pub async fn command(
        &mut self,
        command: LiveCommand,
        connection: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<Command> {
        let LiveCommand::Recall {
            request_id,
            thread,
            message_id,
        } = command
        else {
            return Ok(Command::Continue(command));
        };
        let request = RecallRequest {
            request_id,
            thread,
            message_id,
        };
        match self
            .start(request, connection, db, events, session, client)
            .await
        {
            Ok(()) => Ok(Command::Handled),
            Err(error) if error.downcast_ref::<StorageContext>().is_some() => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                events.connection(LiveConnection::Offline).await?;
                Ok(Command::PauseTransport)
            }
            Err(error) if wire_error(&error) == Some(&WireError::Authentication) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Authentication))
                    .await?;
                events.connection(LiveConnection::Expired).await?;
                Ok(Command::PauseTransport)
            }
            Err(error) => Err(error),
        }
    }

    async fn start(
        &mut self,
        request: RecallRequest,
        connection: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let (Some(socket), Ok(target), Some(revision), Some(max_age)) = (
            connection.filter(|socket| socket.decoder.authenticated()),
            request.target(&events.account),
            normal_scope_revision(&events.privacy, &request.thread),
            session.config().recall_max_age_ms,
        ) else {
            return finish(events, request, RecallOutcome::Unavailable).await;
        };
        if self.running.is_some() || root_for(session, "chat").is_err() {
            return finish(events, request, RecallOutcome::Unavailable).await;
        }
        // The actor is the sole WSS reader. This bounded eligibility check can
        // receive and commit queue pushes before it reserves a recall intent.
        if let Err(error) = socket.verify_direct(&target, db, events).await {
            finish(events, request, RecallOutcome::Unavailable).await?;
            if error.downcast_ref::<StorageContext>().is_some()
                || wire_error(&error) == Some(&WireError::Authentication)
            {
                return Err(error);
            }
            return Ok(());
        }
        if normal_scope_revision(&events.privacy, &request.thread) != Some(revision) {
            return finish(events, request, RecallOutcome::Unavailable).await;
        }
        let (account, thread, message) = (
            events.account.clone(),
            request.thread.clone(),
            request.message_id.clone(),
        );
        let prepared = db
            .call(move |store| store.prepare_recall(&account, &thread, &message, now_ms(), max_age))
            .await;
        let (client_id, original) = match prepared {
            Ok(value) => (value.client_id, value.reference),
            Err(error) => {
                let outcome = match error.downcast_ref::<RecallFailure>() {
                    Some(RecallFailure::Unresolved) => RecallOutcome::Unknown,
                    _ => RecallOutcome::Unavailable,
                };
                return finish(events, request, outcome).await;
            }
        };
        let Ok(dispatch) = PreparedDispatch::new(session, &request, client_id, original, max_age)
        else {
            return complete(db, events, (request, client_id, DispatchOutcome::NotSent)).await;
        };
        let (session, client, privacy) = (session.clone(), client.clone(), events.privacy.clone());
        let db = db.handle()?;
        let account = events.account.clone();
        self.running = Some(RunningRecall {
            request,
            client_id,
            task: tokio::spawn(async move {
                // A preview or socket notice can settle the intent after it is
                // reserved. Recheck on the storage actor before any HTTP write.
                if !db
                    .call(move |store| store.recall_outcome(&account, client_id))
                    .await
                    .is_ok_and(|outcome| outcome == StoredOutcome::Pending)
                {
                    return DispatchOutcome::NotSent;
                }
                dispatch.send(&client, &session, &privacy, revision).await
            }),
        });
        Ok(())
    }
}

pub(super) async fn complete(
    db: &Database,
    events: &Events,
    (request, client_id, result): (RecallRequest, u64, DispatchOutcome),
) -> Result<()> {
    let account = events.account.clone();
    let unavailable = matches!(result, DispatchOutcome::NotSent);
    let settled = db
        .call(move |store| {
            if matches!(result, DispatchOutcome::Accepted) {
                store.record_recall_ack(&account, client_id)?;
                let change = store.accept_recall_ack(&account, client_id)?;
                return Ok((StoredOutcome::Recalled, Some(change)));
            }
            store.fail_recall(
                &account,
                client_id,
                matches!(result, DispatchOutcome::Unknown),
            )?;
            Ok((store.recall_outcome(&account, client_id)?, None))
        })
        .await;
    let outcome = match settled {
        Ok((StoredOutcome::Recalled, change)) => {
            if let Some(change) = change {
                events.emit(projection(events, change)).await?;
            }
            RecallOutcome::Accepted
        }
        Ok((StoredOutcome::Rejected, _)) if unavailable => RecallOutcome::Unavailable,
        Ok((StoredOutcome::Rejected, _)) => RecallOutcome::Rejected,
        _ => RecallOutcome::Unknown,
    };
    finish(events, request, outcome).await
}

pub(super) fn projection(events: &Events, change: RecallChange) -> LiveEvent {
    LiveEvent::Recalled {
        account_id: events.account.clone(),
        thread: change.thread_id,
        message_id: change.message_id,
        actor_id: change.actor_id,
        message: change.message,
        conversation: change.conversation,
    }
}

async fn finish(events: &Events, request: RecallRequest, outcome: RecallOutcome) -> Result<()> {
    events
        .emit(LiveEvent::RecallFinished {
            account_id: events.account.clone(),
            request_id: request.request_id,
            thread: request.thread,
            message_id: request.message_id,
            outcome,
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_confirmed_visible_direct_chat_can_supply_recall_scope() {
        let mut request = RecallRequest {
            request_id: 2,
            thread: "d:22".into(),
            message_id: "777".into(),
        };
        let scope = std::sync::RwLock::new(privacy::Projection {
            confirmed: true,
            revision: 3,
            ..Default::default()
        });
        assert_eq!(normal_scope_revision(&scope, &request.thread), Some(3));
        assert!(request.target("22").is_err());
        assert!(request.target("11").is_ok());
        {
            let mut policy = scope.write().unwrap();
            policy.hidden.insert(request.thread.clone());
            policy.unlocked = Some(request.thread.clone());
        }
        assert_eq!(normal_scope_revision(&scope, &request.thread), None);
        request.thread = "g:22".into();
        assert!(request.target("11").is_err());
    }
}
