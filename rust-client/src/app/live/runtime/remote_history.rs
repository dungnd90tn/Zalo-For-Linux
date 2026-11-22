//! One manually requested group-history page with independent transport scope.
use super::*;
use crate::protocol::remote_history::{self as cloud, Page, RemoteCursor};
use std::collections::VecDeque;
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

#[derive(Clone)]
struct NewestScope {
    thread: String,
    generation: u64,
    revision: u64,
    authenticated: Instant,
}

struct NewestActive {
    scope: NewestScope,
    task: JoinHandle<Result<wire::IncomingBatch>>,
}

impl Drop for NewestActive {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) struct NewestCompletion {
    scope: NewestScope,
    result: Result<wire::IncomingBatch>,
}

pub(super) enum Finished {
    Older(Completion),
    Newest(NewestCompletion),
}

#[derive(Default)]
pub(super) struct History {
    active: Option<Active>,
    continuation: Option<(Scope, RemoteCursor)>,
    selected: Option<(String, u64)>,
    latest_selected: bool,
    newest: Option<NewestActive>,
    attempted_epoch: Option<Instant>,
    attempted: VecDeque<(String, u64)>,
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
                generation,
                thread,
                before,
            } => {
                self.selected = Some((thread.clone(), *generation));
                self.latest_selected = before.is_none();
                if self.newest.as_ref().is_some_and(|active| {
                    before.is_some()
                        || active.scope.generation != *generation
                        || active.scope.thread != *thread
                }) {
                    self.newest = None;
                }
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
                self.latest_selected = false;
                self.newest = None;
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

    pub fn start_newest(
        &mut self,
        authenticated: Option<Instant>,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) {
        if self.attempted_epoch != authenticated {
            self.attempted_epoch = authenticated;
            self.attempted.clear();
            self.newest = None;
        }
        let (Some(authenticated), Some((thread, generation))) =
            (authenticated, self.selected.as_ref())
        else {
            return;
        };
        if self.newest.is_some()
            || !self.latest_selected
            || !thread.starts_with("g:")
            || self
                .attempted
                .iter()
                .any(|(attempted, page)| attempted == thread && page == generation)
        {
            return;
        }
        let Ok(revision) = events.privacy.read().map(|policy| policy.revision) else {
            return;
        };
        let roster = group_roster::Request {
            thread: thread.clone(),
            privacy_revision: revision,
            after: None,
        };
        let (Ok(target), Ok(mut url)) = (roster.target(), root_for(session, cloud::LATEST_SERVICE))
        else {
            return;
        };
        if !roster.permits(&events.privacy) {
            return;
        }
        let params = cloud::RemoteCursor::newest_parameters(&target, session.imei())
            .ok()
            .and_then(|params| serde_json::to_vec(&params).ok())
            .and_then(|params| session.encrypt_payload(&params).ok());
        let Some(params) = params else {
            return;
        };
        url.set_path(RemoteCursor::newest().path());
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", session.client_type().api_type())
            .append_pair("params", &params);
        let scope = NewestScope {
            thread: thread.clone(),
            generation: *generation,
            revision,
            authenticated,
        };
        let policy = events.privacy.clone();
        let session = session.clone();
        let client = client.clone();
        self.newest = Some(NewestActive {
            scope,
            task: tokio::spawn(async move {
                tokio::time::timeout(DEADLINE, async {
                    let (visibility, membership) = tokio::try_join!(
                        verify_group(&client, &session, &target),
                        group_roster::verify_members(&client, &session, &policy, &roster),
                    )?;
                    if visibility != 0
                        || !membership.applies(&roster, session.account_id())
                        || !roster.permits(&policy)
                    {
                        return Err(WireError::Unsupported.into());
                    }
                    let response = authenticated_request(&client, &session, url, None).await?;
                    Ok(cloud::newest_page(&session, &response, &target)?)
                })
                .await?
            }),
        });
    }

    async fn next_newest(slot: &mut Option<NewestActive>) -> NewestCompletion {
        let Some(active) = slot.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut active.task)
            .await
            .unwrap_or_else(|error| Err(error.into()));
        let scope = slot
            .take()
            .expect("completed latest history has an owner")
            .scope
            .clone();
        NewestCompletion { scope, result }
    }

    async fn complete_newest(
        &mut self,
        completion: NewestCompletion,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let NewestCompletion { scope, result } = completion;
        if authenticated != Some(scope.authenticated)
            || !self.latest_selected
            || self.selected.as_ref().is_none_or(|(thread, generation)| {
                *thread != scope.thread || *generation != scope.generation
            })
        {
            return Ok(());
        }
        let roster = group_roster::Request {
            thread: scope.thread.clone(),
            privacy_revision: scope.revision,
            after: None,
        };
        if !roster.permits(&events.privacy) {
            return Ok(());
        }
        if self.attempted.len() == 64 {
            self.attempted.pop_front();
        }
        self.attempted
            .push_back((scope.thread.clone(), scope.generation));
        let batch = match result {
            Ok(batch) if !batch.messages.is_empty() => batch,
            Ok(_) => return Ok(()),
            Err(error) => {
                if wire_error(&error) != Some(&WireError::Unsupported) {
                    let _ = diagnostic(&error, LiveStage::Synchronization);
                }
                return Ok(());
            }
        };
        let account = events.account.clone();
        let policy = events.privacy.clone();
        let saved = db
            .call_with(StorageOperation::LoadHistory, move |store| {
                let privacy = policy.read().map_err(|_| WireError::Unsupported)?;
                if privacy.revision != roster.privacy_revision
                    || !privacy.confirmed
                    || privacy.status != LivePrivacyStatus::Ready
                    || privacy.hidden.contains(&roster.thread)
                {
                    return Err(WireError::Unsupported.into());
                }
                let (inserted, deletions) =
                    store.ingest_newest_group_history(&account, &roster.thread, batch)?;
                if inserted == 0 && deletions.is_empty() {
                    return Ok(None);
                }
                let page = store.page(&account, &roster.thread, None, crate::app::PAGE_SIZE + 1)?;
                let conversations = store.conversations(&account, crate::domain::MAX_PAGE_SIZE)?;
                Ok(Some((deletions, page, conversations)))
            })
            .await;
        match saved {
            Ok(Some((deletions, mut page, conversations))) => {
                for change in deletions {
                    events
                        .emit(delete_for_me::projection(events, change))
                        .await?;
                }
                let has_older = page.len() > crate::app::PAGE_SIZE;
                if has_older {
                    page.remove(0);
                }
                events
                    .emit(LiveEvent::Page {
                        account_id: events.account.clone(),
                        generation: scope.generation,
                        thread: scope.thread,
                        messages: page,
                        has_older,
                    })
                    .await?;
                events
                    .emit(LiveEvent::DirectoryChanged {
                        account_id: events.account.clone(),
                        conversations,
                    })
                    .await?;
            }
            Ok(None) => {}
            Err(error) if wire_error(&error) == Some(&WireError::Unsupported) => {}
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Storage);
            }
        }
        Ok(())
    }

    pub async fn next(&mut self) -> Finished {
        tokio::select! {
            older = Self::next_older(&mut self.active) => Finished::Older(older),
            newest = Self::next_newest(&mut self.newest) => Finished::Newest(newest),
        }
    }

    async fn next_older(slot: &mut Option<Active>) -> Completion {
        let Some(active) = slot.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut active.task)
            .await
            .unwrap_or_else(|error| Err(error.into()));
        let active = slot.take().expect("completed history request has an owner");
        let scope = Scope {
            request: active.scope.request.clone(),
            authenticated: active.scope.authenticated,
        };
        Completion { scope, result }
    }

    pub async fn complete(
        &mut self,
        completion: Finished,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let completion = match completion {
            Finished::Newest(newest) => {
                return self
                    .complete_newest(newest, authenticated, db, events)
                    .await
            }
            Finished::Older(older) => older,
        };
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
