//! One explicit reaction request at a time, with bounded authenticated input.
use super::*;
use crate::protocol::reactions::{self as reaction_wire, ReactionNotice};
use std::collections::BTreeSet;

const SEND_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(super) struct Request {
    request_id: u64,
    thread: String,
    message_id: String,
    action: ReactionAction,
}

pub(super) struct Running {
    request: Request,
    client_id: u64,
    task: tokio::task::JoinHandle<ReactionOutcome>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // The reservation remains durable and is never automatically resent.
        self.task.abort();
    }
}

#[derive(Default)]
pub(super) struct Reactions(Option<Running>);

impl Reactions {
    pub async fn next(&mut self) -> (Request, u64, ReactionOutcome) {
        let Some(running) = &mut self.0 else {
            return std::future::pending().await;
        };
        let outcome = (&mut running.task)
            .await
            .unwrap_or(ReactionOutcome::Unknown);
        let running = self.0.take().expect("awaited reaction has an owner");
        (running.request.clone(), running.client_id, outcome)
    }

    pub async fn command(
        &mut self,
        command: LiveCommand,
        connection: Option<&mut Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<recall::Command> {
        let request = match command {
            LiveCommand::React {
                request_id,
                thread,
                message_id,
                action,
            } => Request {
                request_id,
                thread,
                message_id,
                action,
            },
            LiveCommand::ReactionState {
                generation,
                thread,
                message_ids,
            } => {
                let mut messages = Vec::new();
                if recall::normal_scope_revision(&events.privacy, &thread).is_some() {
                    let account = events.account.clone();
                    let selected = thread.clone();
                    messages = db
                        .call_with(StorageOperation::LoadHistory, move |store| {
                            store.reaction_summaries(&account, &selected, &message_ids)
                        })
                        .await
                        .unwrap_or_default();
                }
                events
                    .emit(LiveEvent::ReactionState {
                        account_id: events.account.clone(),
                        generation,
                        thread,
                        messages,
                    })
                    .await?;
                return Ok(recall::Command::Handled);
            }
            other => return Ok(recall::Command::Continue(other)),
        };
        let (Some(socket), Ok(target), Some(revision), Ok(mut url)) = (
            connection.filter(|socket| socket.decoder.authenticated()),
            Target::from_thread(&request.thread),
            recall::normal_scope_revision(&events.privacy, &request.thread),
            root_for(session, "reaction"),
        ) else {
            finish(events, request, ReactionOutcome::Unavailable).await?;
            return Ok(recall::Command::Handled);
        };
        if self.0.is_some()
            || wire::validate_origin(&url, "https").is_err()
            || (target.kind == ThreadKind::Direct && target.remote_id == events.account)
        {
            finish(events, request, ReactionOutcome::Unavailable).await?;
            return Ok(recall::Command::Handled);
        }
        let ordinary = match target.kind {
            ThreadKind::Direct => socket.verify_direct(&target, db, events).await,
            ThreadKind::Group => {
                verify_group(client, session, &target)
                    .await
                    .and_then(|visibility| {
                        if visibility == 0 {
                            Ok(())
                        } else {
                            Err(WireError::Unsupported.into())
                        }
                    })
            }
        };
        if let Err(error) = ordinary {
            finish(events, request, ReactionOutcome::Unavailable).await?;
            if error.downcast_ref::<StorageContext>().is_some() {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                events.connection(LiveConnection::Offline).await?;
                return Ok(recall::Command::PauseTransport);
            }
            if wire_error(&error) == Some(&WireError::Authentication) {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Authentication))
                    .await?;
                events.connection(LiveConnection::Expired).await?;
                return Ok(recall::Command::PauseTransport);
            }
            return Ok(recall::Command::Handled);
        }
        if recall::normal_scope_revision(&events.privacy, &request.thread) != Some(revision) {
            finish(events, request, ReactionOutcome::Unavailable).await?;
            return Ok(recall::Command::Handled);
        }
        let (account, thread, message, action) = (
            events.account.clone(),
            request.thread.clone(),
            request.message_id.clone(),
            request.action,
        );
        let prepared = db
            .call(move |store| {
                store.reserve_reaction(&account, &thread, &message, action, now_ms())
            })
            .await;
        let (client_id, source) = match prepared {
            Ok(Ok(data)) => data,
            Ok(Err(outcome)) => {
                finish(events, request, outcome).await?;
                return Ok(recall::Command::Handled);
            }
            Err(error) => {
                // If reservation commit was ambiguous, leave the row in place.
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                finish(events, request, ReactionOutcome::Unknown).await?;
                return Ok(recall::Command::Handled);
            }
        };
        let body = (|| -> Result<_> {
            let (path, parameters) = reaction_wire::parameters(
                &target,
                &request.message_id,
                &source,
                client_id,
                session.imei(),
                request.action,
            )?;
            url.set_path(path);
            url.query_pairs_mut()
                .append_pair("zpw_ver", wire::API_VERSION)
                .append_pair("zpw_type", session.client_type().api_type());
            let encrypted = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
            Ok(url::form_urlencoded::Serializer::new(String::new())
                .append_pair("params", &encrypted)
                .finish())
        })();
        let Ok(body) = body else {
            complete(
                db,
                events,
                (request, client_id, ReactionOutcome::Unavailable),
            )
            .await?;
            return Ok(recall::Command::Handled);
        };
        let (session, client, privacy, thread) = (
            session.clone(),
            client.clone(),
            events.privacy.clone(),
            request.thread.clone(),
        );
        self.0 = Some(Running {
            request,
            client_id,
            task: tokio::spawn(async move {
                if recall::normal_scope_revision(&privacy, &thread) != Some(revision) {
                    return ReactionOutcome::Unavailable;
                }
                match tokio::time::timeout(
                    SEND_DEADLINE,
                    authenticated_request(&client, &session, url, Some(body)),
                )
                .await
                {
                    Ok(Ok(_)) => ReactionOutcome::Accepted,
                    Ok(Err(error))
                        if matches!(
                            wire_error(&error),
                            Some(WireError::Authentication | WireError::Service(211 | 804))
                        ) =>
                    {
                        ReactionOutcome::Rejected
                    }
                    _ => ReactionOutcome::Unknown,
                }
            }),
        });
        Ok(recall::Command::Handled)
    }
}

pub(super) async fn emit_changed(events: &Events, threads: BTreeSet<String>) -> Result<()> {
    for thread in threads {
        events
            .emit(LiveEvent::ReactionsChanged {
                account_id: events.account.clone(),
                thread,
            })
            .await?;
    }
    Ok(())
}

pub(super) async fn receive(
    db: &Database,
    events: &Events,
    notices: Vec<ReactionNotice>,
) -> Result<()> {
    if !notices.is_empty() {
        let account = events.account.clone();
        let changed = db
            .call_with(StorageOperation::ReceiveBatch, move |store| {
                store.apply_reaction_notices(&account, &notices)
            })
            .await?;
        emit_changed(events, changed).await?;
    }
    Ok(())
}

pub(super) async fn complete(
    db: &Database,
    events: &Events,
    (request, client_id, outcome): (Request, u64, ReactionOutcome),
) -> Result<()> {
    let account = events.account.clone();
    let outcome = db
        .call_with(StorageOperation::PersistAck, move |store| {
            store.settle_reaction(&account, client_id, outcome)
        })
        .await
        .unwrap_or_else(|error| {
            record_storage_failure(&error, StorageOperation::PersistAck);
            ReactionOutcome::Unknown
        });
    if outcome == ReactionOutcome::Accepted {
        emit_changed(events, BTreeSet::from([request.thread.clone()])).await?;
    }
    finish(events, request, outcome).await
}

async fn finish(events: &Events, request: Request, outcome: ReactionOutcome) -> Result<()> {
    events
        .emit(LiveEvent::ReactionFinished {
            account_id: events.account.clone(),
            request_id: request.request_id,
            thread: request.thread,
            message_id: request.message_id,
            outcome,
        })
        .await
}
