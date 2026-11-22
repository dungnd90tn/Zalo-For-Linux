//! A scoped delete is either local to this device or an explicit account action.
use super::*;
use crate::app::live::{DeleteForMeMode, DeleteForMeOutcome};
use crate::protocol::delete_for_me as wire_delete;
use crate::store::delete_for_me::{DeleteForMeChange, DeleteForMeFailure};

const DEADLINE: Duration = Duration::from_secs(10);
const GROUP_PROOF_AGE: Duration = Duration::from_secs(20);

pub(super) struct Request {
    request_id: u64,
    thread: String,
    message_id: String,
    mode: DeleteForMeMode,
}

enum Dispatch {
    NotSent,
    Accepted,
    Rejected,
    Unknown,
}

struct Job {
    request: Request,
    client_id: u64,
    task: tokio::task::JoinHandle<Dispatch>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.task.abort();
        // Startup repairs a durable acknowledgement or conservatively retains
        // an unresolved reservation. It never submits a second DELETE.
    }
}

#[derive(Default)]
pub(super) struct Deletes {
    running: Option<Job>,
}

impl Deletes {
    pub(super) async fn next(&mut self) -> Completion {
        let Some(job) = self.running.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut job.task).await.unwrap_or(Dispatch::Unknown);
        Completion {
            job: self.running.take().expect("awaited delete has an owner"),
            result,
        }
    }

    pub(super) async fn command(
        &mut self,
        command: LiveCommand,
        socket: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<recall::Command> {
        let LiveCommand::DeleteForMe {
            request_id,
            thread,
            message_id,
            mode,
        } = command
        else {
            return Ok(recall::Command::Continue(command));
        };
        let request = Request {
            request_id,
            thread,
            message_id,
            mode,
        };
        match self
            .start(request, socket, db, events, session, client)
            .await
        {
            Ok(()) => Ok(recall::Command::Handled),
            Err(error) if error.downcast_ref::<StorageContext>().is_some() => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                events.connection(LiveConnection::Offline).await?;
                Ok(recall::Command::PauseTransport)
            }
            Err(error) if wire_error(&error) == Some(&WireError::Authentication) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Authentication))
                    .await?;
                events.connection(LiveConnection::Expired).await?;
                Ok(recall::Command::PauseTransport)
            }
            Err(error) => Err(error),
        }
    }

    async fn start(
        &mut self,
        request: Request,
        socket: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let (Ok(target), Some(revision)) = (
            Target::from_thread(&request.thread),
            recall::normal_scope_revision(&events.privacy, &request.thread),
        ) else {
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        };
        if request.request_id == 0
            || wire::numeric_id(&request.message_id).is_err()
            || (target.kind == ThreadKind::Direct && target.remote_id == events.account)
            || session.account_id() != events.account
            || self.running.is_some()
        {
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        }

        let (account, thread, message_id) = (
            events.account.clone(),
            request.thread.clone(),
            request.message_id.clone(),
        );
        let sync = session.config().delete_for_me_sync;
        if request.mode == DeleteForMeMode::Device {
            if sync {
                return finish(events, request, DeleteForMeOutcome::Unavailable).await;
            }
            let privacy = events.privacy.clone();
            let result = db
                .call(move |store| {
                    if recall::normal_scope_revision(&privacy, &thread) != Some(revision) {
                        return Err(DeleteForMeFailure::Unavailable.into());
                    }
                    store.delete_for_me_on_device(&account, &thread, &message_id)
                })
                .await;
            return publish(events, request, result).await;
        }

        let Some(socket) = socket.filter(|socket| socket.decoder.authenticated() && sync) else {
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        };
        let (service, path) = match target.kind {
            ThreadKind::Direct => ("chat", wire_delete::DIRECT_PATH),
            ThreadKind::Group => ("group", wire_delete::GROUP_PATH),
        };
        let Ok(mut url) = root_for(session, service) else {
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        };
        wire::validate_origin(&url, "https")?;
        let checked_at = tokio::time::Instant::now();
        let proof = match target.kind {
            ThreadKind::Direct => socket.verify_direct(&target, db, events).await,
            ThreadKind::Group => {
                let group = group_roster::Request {
                    thread: request.thread.clone(),
                    privacy_revision: revision,
                    after: None,
                };
                tokio::try_join!(
                    verify_group(client, session, &target),
                    group_roster::verify_members(client, session, &events.privacy, &group)
                )
                .and_then(|(visibility, roster)| {
                    if visibility != 0 || !roster.applies(&group, &events.account) {
                        return Err(WireError::Unsupported.into());
                    }
                    Ok(())
                })
            }
        };
        if let Err(error) = proof {
            finish(events, request, DeleteForMeOutcome::Unavailable).await?;
            return if error.downcast_ref::<StorageContext>().is_some()
                || wire_error(&error) == Some(&WireError::Authentication)
            {
                Err(error)
            } else {
                Ok(())
            };
        }
        if !socket.decoder.authenticated()
            || recall::normal_scope_revision(&events.privacy, &request.thread) != Some(revision)
        {
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        }

        let prepared = match db
            .call(move |store| {
                store.prepare_delete_for_me(&account, &thread, &message_id, now_ms())
            })
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => return finish(events, request, store_outcome(&error)).await,
        };
        let client_id = prepared.client_id;
        let build = || -> Result<String> {
            let parameters = wire_delete::parameters(
                &events.account,
                &target,
                client_id,
                &request.message_id,
                &prepared.sender_id,
                &prepared.source_client_id,
                session.imei(),
            )?;
            let encrypted = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
            Ok(url::form_urlencoded::Serializer::new(String::new())
                .append_pair("params", &encrypted)
                .finish())
        };
        let Ok(body) = build() else {
            let account = events.account.clone();
            let _ = db
                .call(move |store| store.fail_delete_for_me(&account, client_id, false))
                .await;
            return finish(events, request, DeleteForMeOutcome::Unavailable).await;
        };
        url.set_path(path);
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", session.client_type().api_type());
        let (session, client, privacy, account, thread) = (
            session.clone(),
            client.clone(),
            events.privacy.clone(),
            events.account.clone(),
            request.thread.clone(),
        );
        let db = db.handle()?;
        let task = tokio::spawn(async move {
            if !db
                .call(move |store| store.delete_for_me_dispatch_pending(&account, client_id))
                .await
                .unwrap_or(false)
                || recall::normal_scope_revision(&privacy, &thread) != Some(revision)
                || (target.kind == ThreadKind::Group && checked_at.elapsed() > GROUP_PROOF_AGE)
            {
                return Dispatch::NotSent;
            }
            match tokio::time::timeout(
                DEADLINE,
                authenticated_request(&client, &session, url, Some(body)),
            )
            .await
            {
                Ok(Ok(payload)) if wire_delete::ack(&payload, &target, client_id).is_ok() => {
                    Dispatch::Accepted
                }
                Ok(Err(error)) if wire_error(&error) == Some(&WireError::Service(211)) => {
                    Dispatch::Rejected
                }
                _ => Dispatch::Unknown,
            }
        });
        self.running = Some(Job {
            request,
            client_id,
            task,
        });
        Ok(())
    }
}

pub(super) struct Completion {
    job: Job,
    result: Dispatch,
}

pub(super) async fn complete(db: &Database, events: &Events, completed: Completion) -> Result<()> {
    let Completion { job, result } = completed;
    let request = Request {
        request_id: job.request.request_id,
        thread: job.request.thread.clone(),
        message_id: job.request.message_id.clone(),
        mode: job.request.mode,
    };
    let account = events.account.clone();
    let client_id = job.client_id;
    if matches!(result, Dispatch::Accepted) {
        let storage = db
            .call(move |store| {
                store.record_delete_for_me_ack(&account, client_id)?;
                store.accept_delete_for_me_ack(&account, client_id)
            })
            .await;
        // A proved receipt with a failed local write still cannot be retried.
        return match storage {
            Ok(change) => publish(events, request, Ok(change)).await,
            Err(_) => finish(events, request, DeleteForMeOutcome::Unknown).await,
        };
    }
    let unknown = matches!(result, Dispatch::Unknown);
    let saved = db
        .call(move |store| store.fail_delete_for_me(&account, client_id, unknown))
        .await;
    let outcome = if saved.is_err() {
        DeleteForMeOutcome::Unknown
    } else {
        match result {
            Dispatch::NotSent => DeleteForMeOutcome::Unavailable,
            Dispatch::Rejected => DeleteForMeOutcome::Rejected,
            Dispatch::Unknown | Dispatch::Accepted => DeleteForMeOutcome::Unknown,
        }
    };
    finish(events, request, outcome).await
}

fn store_outcome(error: &anyhow::Error) -> DeleteForMeOutcome {
    if matches!(
        error.downcast_ref::<DeleteForMeFailure>(),
        Some(DeleteForMeFailure::Unresolved)
    ) {
        DeleteForMeOutcome::Unknown
    } else {
        DeleteForMeOutcome::Unavailable
    }
}

async fn publish(
    events: &Events,
    request: Request,
    result: Result<DeleteForMeChange>,
) -> Result<()> {
    let outcome = match result {
        Ok(change) => {
            // This deletion is committed. A revision change elsewhere need
            // not suppress updates for a still-visible conversation.
            if recall::normal_scope_revision(&events.privacy, &request.thread).is_some() {
                events
                    .emit(LiveEvent::DeletedForMe {
                        account_id: events.account.clone(),
                        thread: change.thread_id,
                        message_id: change.message_id,
                        sender_id: change.sender_id,
                        conversation: change.conversation,
                    })
                    .await?;
            }
            DeleteForMeOutcome::Accepted
        }
        Err(error) => store_outcome(&error),
    };
    finish(events, request, outcome).await
}

async fn finish(events: &Events, request: Request, outcome: DeleteForMeOutcome) -> Result<()> {
    events
        .emit(LiveEvent::DeleteForMeFinished {
            account_id: events.account.clone(),
            request_id: request.request_id,
            thread: request.thread,
            message_id: request.message_id,
            outcome,
        })
        .await
}
