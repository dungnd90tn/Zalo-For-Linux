//! One manual read-only poll request bound to the active group and stored notice.
use super::*;
use crate::{domain::poll::PollDetails, protocol::group_poll as poll};
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
        result: Result<(u64, PollDetails), &'static str>,
    ) -> Result<()> {
        let (poll_id, detail, notice) = match result {
            Ok((poll_id, detail)) => (Some(poll_id), Some(detail), None),
            Err(notice) => (None, None, Some(notice)),
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
}

struct Active {
    request: Request,
    poll_id: u64,
    authenticated: Instant,
    task: JoinHandle<Result<PollDetails>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) struct Completion {
    active: Active,
    result: Result<PollDetails>,
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
        client: &reqwest::Client,
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
        let LiveCommand::LoadPollDetail {
            request_id,
            generation,
            thread,
            message_id,
            privacy_revision,
        } = command
        else {
            return Ok(Some(command));
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
        };
        let allowed = request_id > 0 && selected && request.group.permits(&events.privacy);
        let input = (
            authenticated.filter(|_| allowed && self.active.is_none()),
            request.group.target(),
            root_for(session, poll::SERVICE),
        );
        let (Some(authenticated), Ok(target), Ok(mut url)) = input else {
            request.finish(events, Err(UNAVAILABLE)).await?;
            return Ok(None);
        };
        let database = db.handle()?;
        let Ok(Some(poll_id)) = request
            .current_poll(&database, &events.account, &events.privacy)
            .await
        else {
            request.finish(events, Err(UNAVAILABLE)).await?;
            return Ok(None);
        };
        wire::validate_origin(&url, "https")?;
        url.set_path(poll::PATH);
        let params = session.encrypt_payload(&serde_json::to_vec(&poll::parameters(
            poll_id,
            session.imei(),
        )?)?)?;
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", session.client_type().api_type())
            .append_pair("params", &params);
        let policy = events.privacy.clone();
        let scoped = request.clone();
        let session = session.clone();
        let client = client.clone();
        self.active = Some(Active {
            request,
            poll_id,
            authenticated,
            task: tokio::spawn(async move {
                tokio::time::timeout(DEADLINE, async {
                    let (visibility, membership) = tokio::try_join!(
                        verify_group(&client, &session, &target),
                        group_roster::verify_members(&client, &session, &policy, &scoped.group),
                    )?;
                    if visibility != 0
                        || !membership.applies(&scoped.group, session.account_id())
                        || scoped
                            .current_poll(&database, session.account_id(), &policy)
                            .await?
                            != Some(poll_id)
                        || !scoped.group.permits(&policy)
                    {
                        return Err(WireError::Unsupported.into());
                    }
                    let response = authenticated_request(&client, &session, url, None).await?;
                    Ok(poll::detail(&response, &target, poll_id)?)
                })
                .await?
            }),
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
            return request.finish(events, Err(UNAVAILABLE)).await;
        }
        let result = match result {
            Ok(detail) => Ok((active.poll_id, detail)),
            Err(error) if wire_error(&error) == Some(&WireError::Unsupported) => Err(UNAVAILABLE),
            Err(error) => {
                events
                    .diagnostic(diagnostic(&error, LiveStage::Synchronization))
                    .await?;
                Err(FAILED)
            }
        };
        request.finish(events, result).await
    }
}
