//! One manually requested group-history page with independent transport scope.
use super::*;
use crate::protocol::remote_history::{self as cloud, Page, RemoteCursor};
use tokio::{task::JoinHandle, time::Instant};

const DEADLINE: Duration = Duration::from_secs(25);

#[derive(Clone)]
struct Request {
    id: u64,
    generation: u64,
    thread: String,
    anchor: crate::domain::Cursor,
    revision: u64,
}

impl Request {
    fn permits(&self, policy: &privacy::Projection) -> bool {
        policy.confirmed
            && policy.status == LivePrivacyStatus::Ready
            && policy.revision == self.revision
            && !policy.hidden.contains(&self.thread)
    }

    async fn finish(self, events: &Events, outcome: RemoteHistoryOutcome) -> Result<()> {
        events
            .emit(LiveEvent::RemoteHistoryFinished {
                account_id: events.account.clone(),
                request_id: self.id,
                generation: self.generation,
                thread: self.thread,
                privacy_revision: self.revision,
                outcome,
            })
            .await
    }
}

struct Scope {
    request: Request,
    authenticated: Instant,
}

impl Scope {
    fn matches(&self, request: &Request, authenticated: Instant) -> bool {
        self.authenticated == authenticated
            && self.request.thread == request.thread
            && self.request.revision == request.revision
    }
}

struct Active {
    scope: Scope,
    task: JoinHandle<Result<Page>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) struct Completion {
    scope: Scope,
    result: Result<Page>,
}

#[derive(Default)]
pub(super) struct History {
    active: Option<Active>,
    continuation: Option<(Scope, RemoteCursor)>,
    selected: Option<(String, u64)>,
}

pub(super) fn epoch(connection: Option<&Connection>) -> Option<Instant> {
    connection
        .filter(|socket| socket.decoder.authenticated())
        .and_then(|socket| socket.authenticated_at)
}

impl History {
    pub async fn command(
        &mut self,
        command: LiveCommand,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<Option<LiveCommand>> {
        match &command {
            LiveCommand::Load {
                generation, thread, ..
            } => {
                self.selected = Some((thread.clone(), *generation));
                let same = |scope: &Scope| {
                    scope.request.generation == *generation && scope.request.thread == *thread
                };
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| !same(&active.scope))
                {
                    self.active = None;
                }
                if self
                    .continuation
                    .as_ref()
                    .is_some_and(|(scope, _)| scope.request.thread != *thread)
                {
                    self.continuation = None;
                }
            }
            // Dispatched before a Load for a newly opened peer.
            LiveCommand::Open { .. } => {
                self.selected = None;
                self.active = None;
                self.continuation = None;
            }
            LiveCommand::Reconnect => self.continuation = None,
            _ => {}
        }
        let LiveCommand::LoadRemoteHistory {
            request_id,
            generation,
            thread,
            anchor,
            privacy_revision,
        } = command
        else {
            return Ok(Some(command));
        };
        let request = Request {
            id: request_id,
            generation,
            thread,
            anchor,
            revision: privacy_revision,
        };
        let allowed = request.id > 0
            && self.selected.as_ref().is_some_and(|(thread, generation)| {
                *thread == request.thread && *generation == request.generation
            })
            && events
                .privacy
                .read()
                .is_ok_and(|policy| request.permits(&policy));
        let input = (
            authenticated.filter(|_| allowed && self.active.is_none()),
            Target::from_thread(&request.thread),
            RemoteCursor::from_anchor(&request.anchor),
            root_for(session, cloud::SERVICE),
        );
        let (Some(authenticated), Ok(target), Ok(initial), Ok(mut url)) = input else {
            request
                .finish(events, RemoteHistoryOutcome::Unavailable)
                .await?;
            return Ok(None);
        };
        if target.kind != ThreadKind::Group {
            request
                .finish(events, RemoteHistoryOutcome::Unavailable)
                .await?;
            return Ok(None);
        }
        let (account, checked) = (events.account.clone(), request.clone());
        if db
            .call_with(StorageOperation::LoadHistory, move |store| {
                store.group_history_anchor(&account, &checked.thread, &checked.anchor)
            })
            .await
            .is_err()
        {
            request
                .finish(events, RemoteHistoryOutcome::Unavailable)
                .await?;
            return Ok(None);
        }
        let cursor = self
            .continuation
            .as_ref()
            .filter(|(scope, cursor)| {
                scope.matches(&request, authenticated) && cursor.id <= initial.id
            })
            .map_or(initial, |(_, cursor)| *cursor);
        wire::validate_origin(&url, "https")?;
        url.set_path(cursor.path());
        let params = session.encrypt_payload(&serde_json::to_vec(
            &cursor.parameters(&target, session.imei())?,
        )?)?;
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", session.client_type().api_type())
            .append_pair("params", &params);
        let policy = events.privacy.clone();
        let roster = group_roster::Request {
            thread: request.thread.clone(),
            privacy_revision: request.revision,
            after: None,
        };
        let scoped = request.clone();
        let session = session.clone();
        let client = client.clone();
        self.active = Some(Active {
            scope: Scope {
                request,
                authenticated,
            },
            task: tokio::spawn(async move {
                tokio::time::timeout(DEADLINE, async {
                    let (visibility, membership) = tokio::try_join!(
                        verify_group(&client, &session, &target),
                        group_roster::verify_members(&client, &session, &policy, &roster),
                    )?;
                    if visibility != 0
                        || !membership.applies(&roster, session.account_id())
                        || !policy.read().is_ok_and(|policy| scoped.permits(&policy))
                    {
                        return Err(WireError::Unsupported.into());
                    }
                    let response = authenticated_request(&client, &session, url, None).await?;
                    Ok(cloud::page(
                        &session,
                        &response,
                        &target,
                        &scoped.anchor,
                        cursor,
                    )?)
                })
                .await?
            }),
        });
        Ok(None)
    }

    pub async fn next(&mut self) -> Completion {
        let Some(active) = self.active.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut active.task)
            .await
            .unwrap_or_else(|error| Err(error.into()));
        let active = self
            .active
            .take()
            .expect("completed history request has an owner");
        let scope = Scope {
            request: active.scope.request.clone(),
            authenticated: active.scope.authenticated,
        };
        Completion { scope, result }
    }

    pub async fn complete(
        &mut self,
        completion: Completion,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let Completion { scope, result } = completion;
        let request = scope.request.clone();
        if authenticated != Some(scope.authenticated)
            || !events
                .privacy
                .read()
                .is_ok_and(|policy| request.permits(&policy))
        {
            return request
                .finish(events, RemoteHistoryOutcome::Unavailable)
                .await;
        }
        let Page { batch, next } = match result {
            Ok(page) => page,
            Err(error) => {
                if wire_error(&error) == Some(&WireError::Unsupported) {
                    return request
                        .finish(events, RemoteHistoryOutcome::Unavailable)
                        .await;
                }
                events
                    .diagnostic(diagnostic(&error, LiveStage::Synchronization))
                    .await?;
                return request.finish(events, RemoteHistoryOutcome::Failed).await;
            }
        };
        let (account, checked, privacy) = (
            events.account.clone(),
            request.clone(),
            events.privacy.clone(),
        );
        let saved = db
            .call_with(StorageOperation::LoadHistory, move |store| {
                let policy = privacy.read().map_err(|_| WireError::Unsupported)?;
                if !checked.permits(&policy) {
                    return Err(WireError::Unsupported.into());
                }
                store.ingest_group_history(&account, &checked.thread, &checked.anchor, batch)
            })
            .await;
        let outcome = match saved {
            Ok((count, deletions)) => {
                for change in deletions {
                    events
                        .emit(delete_for_me::projection(events, change))
                        .await?;
                }
                self.continuation = next.map(|next| (scope, next));
                if count == 0 && next.is_none() {
                    RemoteHistoryOutcome::Exhausted
                } else {
                    RemoteHistoryOutcome::Imported {
                        count,
                        has_more: next.is_some(),
                    }
                }
            }
            Err(error) if wire_error(&error) == Some(&WireError::Unsupported) => {
                RemoteHistoryOutcome::Unavailable
            }
            Err(error) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Storage))
                    .await?;
                RemoteHistoryOutcome::Failed
            }
        };
        request.finish(events, outcome).await
    }
}
