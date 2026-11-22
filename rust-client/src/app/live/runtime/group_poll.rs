//! Manual poll requests bound to the active group and stored notice.
use super::*;
use crate::{
    domain::poll::{PollDetails, PollVoteSettlement, PollVoteState},
    protocol::group_poll as poll,
};
use tokio::{task::JoinHandle, time::Instant};

const DEADLINE: Duration = Duration::from_secs(25);
const UNAVAILABLE: &str = "Chưa thể xem chi tiết bình chọn trong hội thoại này.";
const FAILED: &str = "Không tải được chi tiết bình chọn. Bạn có thể thử lại.";

#[derive(Clone)]
struct Request {
    id: u64,
    generation: u64,
    message_id: String,
    group: Arc<group_roster::Request>,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Read,
    Vote {
        poll_id: u64,
        option_id: u64,
        option_text: String,
    },
}

enum Response {
    Detail(PollDetails),
    Vote(PollVoteOutcome, Option<PollDetails>),
}

impl Request {
    async fn current_poll(
        &self,
        db: &DatabaseHandle,
        account: &str,
        privacy: &Arc<std::sync::RwLock<privacy::Projection>>,
    ) -> Result<Option<u64>> {
        let request = self.clone();
        let account = account.to_owned();
        let privacy = privacy.clone();
        db.call_with(StorageOperation::LoadHistory, move |store| {
            if !request.group.permits(&privacy) {
                return Ok(None);
            }
            store.poll_detail_target(&account, &request.group.thread, &request.message_id)
        })
        .await
    }

    async fn finish(
        self,
        events: &Events,
        poll_id: Option<u64>,
        result: Result<Response, &'static str>,
    ) -> Result<()> {
        if matches!(self.action, Action::Vote { .. }) {
            let (outcome, detail) = match result {
                Ok(Response::Vote(outcome, detail)) => (outcome, detail),
                _ => (PollVoteOutcome::Unavailable, None),
            };
            return events
                .emit(LiveEvent::PollVoteFinished {
                    account_id: events.account.clone(),
                    request_id: self.id,
                    generation: self.generation,
                    thread: self.group.thread.clone(),
                    message_id: self.message_id,
                    privacy_revision: self.group.privacy_revision,
                    poll_id,
                    outcome,
                    detail,
                })
                .await;
        }
        let (poll_id, detail, notice) = match result {
            Ok(Response::Detail(detail)) => (poll_id, Some(detail), None),
            Err(notice) => (None, None, Some(notice)),
            Ok(Response::Vote(..)) => unreachable!("read-only request returned a vote"),
        };
        events
            .emit(LiveEvent::PollDetail {
                account_id: events.account.clone(),
                request_id: self.id,
                generation: self.generation,
                thread: self.group.thread.clone(),
                message_id: self.message_id,
                privacy_revision: self.group.privacy_revision,
                poll_id,
                detail,
                notice,
            })
            .await
    }

    async fn settle_vote(&self, task: &Task, settlement: PollVoteSettlement) -> Result<()> {
        let account = task.session.account_id().to_owned();
        let thread = self.group.thread.clone();
        let message = self.message_id.clone();
        let id = task.poll_id;
        task.database
            .call_with(StorageOperation::PersistAck, move |store| {
                store.settle_poll_vote(&account, &thread, &message, id, settlement)
            })
            .await
    }
}

struct Task {
    url: Url,
    target: Target,
    poll_id: u64,
    database: DatabaseHandle,
    policy: Arc<std::sync::RwLock<privacy::Projection>>,
    session: Arc<VerifiedSession>,
    client: reqwest::Client,
    vote_client: reqwest::Client,
}

impl Task {
    fn url_for(&self, path: &str, params: Value) -> Result<Url> {
        let mut url = self.url.clone();
        wire::validate_origin(&url, "https")?;
        url.set_path(path);
        let params = self
            .session
            .encrypt_payload(&serde_json::to_vec(&params)?)?;
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", self.session.client_type().api_type())
            .append_pair("params", &params);
        Ok(url)
    }

    async fn read(&self, url: Url) -> Result<(PollDetails, Option<i64>)> {
        let response = authenticated_request(&self.client, &self.session, url, None).await?;
        let detail = poll::detail(&response, &self.target, self.poll_id, now_ms())?;
        Ok((detail, poll::expiry(&response)))
    }

    async fn scope_is_current(&self, request: &Request) -> Result<bool> {
        Ok(request
            .current_poll(&self.database, self.session.account_id(), &self.policy)
            .await?
            == Some(self.poll_id)
            && request.group.permits(&self.policy))
    }

    async fn apply_vote_state(&self, request: &Request, detail: &mut PollDetails) -> Result<()> {
        let account = self.session.account_id().to_owned();
        let poll_id = self.poll_id;
        let mut state = self
            .database
            .call_with(StorageOperation::LoadHistory, move |store| {
                store.poll_vote_state(&account, poll_id)
            })
            .await?;
        if state == Some(PollVoteState::Pending) && detail.joined == Some(true) {
            request
                .settle_vote(self, PollVoteSettlement::Joined)
                .await?;
            state = Some(PollVoteState::Joined);
        }
        detail.can_vote &= state.is_none();
        detail.vote_pending = state == Some(PollVoteState::Pending);
        Ok(())
    }

    async fn run(&self, request: &Request) -> Result<Response> {
        let read_url = self.url_for(
            poll::PATH,
            poll::parameters(self.poll_id, self.session.imei())?,
        )?;
        let (membership, mut detail, expiry) = tokio::time::timeout(DEADLINE, async {
            let (visibility, membership) = tokio::try_join!(
                verify_group(&self.client, &self.session, &self.target),
                group_roster::verify_members(
                    &self.client,
                    &self.session,
                    &self.policy,
                    &request.group
                ),
            )?;
            if visibility != 0
                || !membership.applies(&request.group, self.session.account_id())
                || !self.scope_is_current(request).await?
            {
                return Err(WireError::Unsupported.into());
            }
            let (detail, expiry) = self.read(read_url.clone()).await?;
            Ok::<_, anyhow::Error>((membership, detail, expiry))
        })
        .await??;
        self.apply_vote_state(request, &mut detail).await?;
        let Action::Vote {
            option_id,
            option_text,
            ..
        } = &request.action
        else {
            return Ok(Response::Detail(detail));
        };
        let outcome = if detail.joined == Some(true) {
            Some(PollVoteOutcome::Joined)
        } else if detail.vote_pending {
            Some(PollVoteOutcome::Unknown)
        } else if !detail.can_vote
            || !detail
                .options
                .iter()
                .any(|option| option.option_id == *option_id && option.text == *option_text)
        {
            Some(PollVoteOutcome::Unavailable)
        } else {
            None
        };
        if let Some(outcome) = outcome {
            return Ok(Response::Vote(outcome, Some(detail)));
        }
        let vote_url = self.url_for(
            poll::VOTE_PATH,
            poll::vote_parameters(self.poll_id, *option_id, self.session.imei())?,
        )?;
        if !membership.applies(&request.group, self.session.account_id())
            || !poll::current(expiry, now_ms())
            || !self.scope_is_current(request).await?
        {
            return Ok(Response::Vote(PollVoteOutcome::Unavailable, None));
        }
        let account = self.session.account_id().to_owned();
        let thread = request.group.thread.clone();
        let message = request.message_id.clone();
        let id = self.poll_id;
        let reserved = self
            .database
            .call_with(StorageOperation::PrepareOutgoing, move |store| {
                store.reserve_poll_vote(&account, &thread, &message, id)
            })
            .await?;
        if !reserved {
            self.apply_vote_state(request, &mut detail).await?;
            return Ok(Response::Vote(
                if detail.vote_pending {
                    PollVoteOutcome::Unknown
                } else {
                    PollVoteOutcome::Unavailable
                },
                Some(detail),
            ));
        }

        if !request.group.permits(&self.policy)
            || !poll::current(expiry, now_ms())
            || !membership.applies(&request.group, self.session.account_id())
            || !matches!(self.scope_is_current(request).await, Ok(true))
        {
            request
                .settle_vote(self, PollVoteSettlement::NotDispatched)
                .await?;
            return Ok(Response::Vote(PollVoteOutcome::Unavailable, None));
        }
        let reply = authenticated_request(&self.vote_client, &self.session, vote_url, None).await;
        // 166 is the PC client's explicit blocked-vote rejection. Every other
        // failure can have lost the reply after the request took effect.
        let rejected = reply.as_ref().err().and_then(wire_error) == Some(&WireError::Service(166));
        if rejected {
            request
                .settle_vote(self, PollVoteSettlement::NotDispatched)
                .await?;
            return Ok(Response::Vote(PollVoteOutcome::Rejected, None));
        }

        // Settle against a fresh authenticated detail, even after a lost response.
        // A temporarily unjoined read cannot distinguish stale data from no vote.
        let mut latest = None;
        if matches!(self.scope_is_current(request).await, Ok(true)) {
            latest = self.read(read_url).await.ok().map(|(detail, _)| detail);
        }
        if let Some(detail) = &mut latest {
            let joined = detail.joined == Some(true);
            if self.apply_vote_state(request, detail).await.is_ok() && joined {
                let selected = detail
                    .options
                    .iter()
                    .any(|option| option.option_id == *option_id && option.voted);
                return Ok(Response::Vote(
                    if selected {
                        PollVoteOutcome::Accepted
                    } else {
                        PollVoteOutcome::Joined
                    },
                    latest,
                ));
            }
            detail.can_vote = false;
            detail.vote_pending = true;
        }
        Ok(Response::Vote(PollVoteOutcome::Unknown, latest))
    }
}

struct Active {
    request: Request,
    poll_id: u64,
    authenticated: Instant,
    task: JoinHandle<Result<Response>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) struct Completion {
    active: Active,
    result: Result<Response>,
}

#[derive(Default)]
pub(super) struct Polls {
    active: Option<Active>,
    selected: Option<(String, u64)>,
}

impl Polls {
    pub(super) fn retain(&mut self, authenticated: Option<Instant>, events: &Events) {
        if self.active.as_ref().is_some_and(|active| {
            authenticated != Some(active.authenticated)
                || !active.request.group.permits(&events.privacy)
        }) {
            self.active = None;
        }
    }

    pub(super) async fn command(
        &mut self,
        command: LiveCommand,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        transports: &Transports,
    ) -> Result<Option<LiveCommand>> {
        match &command {
            LiveCommand::Load {
                generation, thread, ..
            } => {
                let selected = (thread.clone(), *generation);
                if self.selected.as_ref() != Some(&selected) {
                    self.active = None;
                }
                self.selected = Some(selected);
            }
            LiveCommand::Open { .. } => {
                self.selected = None;
                self.active = None;
            }
            LiveCommand::Reconnect => self.active = None,
            _ => {}
        }
        let (request_id, generation, thread, message_id, privacy_revision, action) = match command {
            LiveCommand::LoadPollDetail {
                request_id,
                generation,
                thread,
                message_id,
                privacy_revision,
            } => (
                request_id,
                generation,
                thread,
                message_id,
                privacy_revision,
                Action::Read,
            ),
            LiveCommand::VotePoll {
                request_id,
                generation,
                thread,
                message_id,
                privacy_revision,
                poll_id,
                option_id,
                option_text,
            } => (
                request_id,
                generation,
                thread,
                message_id,
                privacy_revision,
                Action::Vote {
                    poll_id,
                    option_id,
                    option_text,
                },
            ),
            command => return Ok(Some(command)),
        };
        let selected = self
            .selected
            .as_ref()
            .is_some_and(|(current, page)| *current == thread && *page == generation);
        let request = Request {
            id: request_id,
            generation,
            message_id,
            group: Arc::new(group_roster::Request {
                thread,
                privacy_revision,
                after: None,
            }),
            action,
        };
        let allowed = request_id > 0 && selected && request.group.permits(&events.privacy);
        let input = (
            authenticated.filter(|_| allowed && self.active.is_none()),
            request.group.target(),
            root_for(session, poll::SERVICE),
        );
        let (Some(authenticated), Ok(target), Ok(url)) = input else {
            request.finish(events, None, Err(UNAVAILABLE)).await?;
            return Ok(None);
        };
        let database = db.handle()?;
        let Ok(Some(poll_id)) = request
            .current_poll(&database, &events.account, &events.privacy)
            .await
        else {
            request.finish(events, None, Err(UNAVAILABLE)).await?;
            return Ok(None);
        };
        if matches!(request.action, Action::Vote {poll_id: expected, ..} if expected != poll_id) {
            request.finish(events, None, Err(UNAVAILABLE)).await?;
            return Ok(None);
        }
        let task = Task {
            url,
            target,
            poll_id,
            database,
            policy: events.privacy.clone(),
            session: session.clone(),
            client: transports.http.clone(),
            vote_client: transports.poll_vote_http.clone(),
        };
        let scoped = request.clone();
        self.active = Some(Active {
            request,
            poll_id,
            authenticated,
            task: tokio::spawn(async move { task.run(&scoped).await }),
        });
        Ok(None)
    }

    pub(super) async fn next(&mut self) -> Completion {
        let Some(active) = self.active.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut active.task)
            .await
            .unwrap_or_else(|error| Err(error.into()));
        let active = self
            .active
            .take()
            .expect("completed poll request has an owner");
        Completion { active, result }
    }

    pub(super) async fn complete(
        completion: Completion,
        authenticated: Option<Instant>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let Completion { active, result } = completion;
        let request = active.request.clone();
        let database = db.handle()?;
        if authenticated != Some(active.authenticated)
            || !request.group.permits(&events.privacy)
            || !matches!(request.current_poll(&database, &events.account, &events.privacy).await, Ok(Some(id)) if id == active.poll_id)
            || !request.group.permits(&events.privacy)
        {
            return request.finish(events, None, Err(UNAVAILABLE)).await;
        }
        let result = match result {
            Ok(response) => Ok(response),
            Err(error) if wire_error(&error) == Some(&WireError::Unsupported) => Err(UNAVAILABLE),
            Err(error) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Synchronization))
                    .await?;
                Err(FAILED)
            }
        };
        request.finish(events, Some(active.poll_id), result).await
    }
}
