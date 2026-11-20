//! Scoped board reads and one-shot group message pin mutations.
use super::*;
use crate::protocol::{group_pins as pin_wire, group_roster as roster_wire};
use crate::store::PinSource;
use tokio::{task::JoinHandle, time::Instant};

const DEADLINE: Duration = Duration::from_secs(10);
const PROOF_AGE: Duration = Duration::from_secs(20);

#[derive(Clone)]
enum Action {
    Read,
    Set { message_id: String, pinned: bool },
}

#[derive(Clone)]
struct Request {
    account: String,
    id: u64,
    thread: String,
    revision: u64,
    action: Action,
}

impl Request {
    fn group(&self) -> group_roster::Request {
        group_roster::Request {
            thread: self.thread.clone(),
            privacy_revision: self.revision,
            after: None,
        }
    }

    fn permits(&self, policy: &std::sync::RwLock<privacy::Projection>) -> bool {
        self.group().permits(policy)
    }

    fn event(self, (outcome, snapshot): Completion) -> LiveEvent {
        match self.action {
            Action::Read => LiveEvent::GroupPins {
                account_id: self.account,
                request_id: self.id,
                thread: self.thread,
                privacy_revision: self.revision,
                snapshot,
            },
            Action::Set { message_id, .. } => LiveEvent::GroupPinFinished {
                account_id: self.account,
                request_id: self.id,
                thread: self.thread,
                privacy_revision: self.revision,
                message_id,
                outcome,
                snapshot,
            },
        }
    }
}

type Completion = (GroupPinOutcome, Option<GroupPinSnapshot>);

struct Running {
    request: Request,
    task: JoinHandle<Completion>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub(super) struct Pins {
    read: Option<Running>,
    mutation: Option<Running>,
}

impl Pins {
    pub async fn next(&mut self) -> LiveEvent {
        tokio::select! {
            result = async { (&mut self.mutation.as_mut().expect("guarded board mutation").task).await }, if self.mutation.is_some() => {
                self.mutation.take().expect("completed board mutation")
                    .request.clone().event(result.unwrap_or((GroupPinOutcome::Unknown, None)))
            },
            result = async { (&mut self.read.as_mut().expect("guarded board read").task).await }, if self.read.is_some() => {
                self.read.take().expect("completed board read")
                    .request.clone().event(result.unwrap_or((GroupPinOutcome::Unavailable, None)))
            },
            else => std::future::pending().await
        }
    }

    pub async fn command(
        &mut self,
        command: LiveCommand,
        online: bool,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<Option<LiveCommand>> {
        let (id, thread, revision, action) = match command {
            LiveCommand::GroupPins {
                request_id,
                thread,
                privacy_revision,
            } => (request_id, thread, privacy_revision, Action::Read),
            LiveCommand::SetGroupPin {
                request_id,
                thread,
                privacy_revision,
                message_id,
                pinned,
            } => (
                request_id,
                thread,
                privacy_revision,
                Action::Set { message_id, pinned },
            ),
            command => return Ok(Some(command)),
        };
        let request = Request {
            account: events.account.clone(),
            id,
            thread,
            revision,
            action,
        };
        let reading = matches!(request.action, Action::Read);
        if id == 0
            || !online
            || request.account != session.account_id()
            || !session.config().group_board_v2
            || root_for(session, "group_board").is_err()
            || request.group().target().is_err()
            || !request.permits(&events.privacy)
            || matches!(&request.action, Action::Set { message_id, .. } if wire::numeric_id(message_id).is_err())
            || self
                .mutation
                .as_ref()
                .is_some_and(|active| !reading || active.request.thread == request.thread)
        {
            events
                .emit(request.event((GroupPinOutcome::Unavailable, None)))
                .await?;
            return Ok(None);
        }

        if reading
            || self
                .read
                .as_ref()
                .is_some_and(|active| active.request.thread == request.thread)
        {
            if let Some(read) = self.read.take() {
                read.task.abort();
                events
                    .emit(
                        read.request
                            .clone()
                            .event((GroupPinOutcome::Unavailable, None)),
                    )
                    .await?;
            }
        }

        let ctx = Context {
            db: db.handle()?,
            policy: events.privacy.clone(),
            session: session.clone(),
            client: client.clone(),
        };
        let operation = request.clone();
        let task = tokio::spawn(async move { ctx.execute(&operation).await });
        let running = Some(Running { request, task });
        if reading {
            self.read = running;
        } else {
            self.mutation = running;
        }
        Ok(None)
    }
}

struct Context {
    db: DatabaseHandle,
    policy: Arc<std::sync::RwLock<privacy::Projection>>,
    session: Arc<VerifiedSession>,
    client: reqwest::Client,
}

struct CheckedBoard {
    snapshot: pin_wire::PinSnapshot,
    can_mutate: bool,
    checked_at: Instant,
    ttl: Option<forward::TargetTtlProof>,
}

impl CheckedBoard {
    fn permits_mutation(&self, target: &Target) -> bool {
        self.can_mutate
            && self.checked_at.elapsed() <= PROOF_AGE
            && self.ttl.as_ref().is_some_and(|ttl| ttl.applies(target))
    }
}

impl Context {
    async fn execute(&self, request: &Request) -> Completion {
        let result = self.preflight(request).await;
        let Ok((target, board, projection, source)) = result else {
            return (GroupPinOutcome::Unavailable, None);
        };
        let (Action::Set { pinned, .. }, Some(source)) = (&request.action, source) else {
            return (GroupPinOutcome::Accepted, Some(projection));
        };
        self.change(request, &target, board, projection, source, *pinned)
            .await
    }

    async fn preflight(
        &self,
        request: &Request,
    ) -> Result<(Target, CheckedBoard, GroupPinSnapshot, Option<PinSource>)> {
        let target = request.group().target()?;
        if !request.permits(&self.policy) {
            return Err(WireError::Unsupported.into());
        }
        let (account, thread, action) = (
            request.account.clone(),
            request.thread.clone(),
            request.action.clone(),
        );
        let source = self
            .db
            .call(move |store| match action {
                Action::Read => {
                    store.validate_group_scope(&account, &thread)?;
                    Ok(None)
                }
                Action::Set { message_id, .. } => store
                    .group_pin_source(&account, &thread, &message_id)
                    .map(Some),
            })
            .await?;

        let permissions = async {
            let payload = group_roster::post(
                &self.client,
                &self.session,
                "group",
                roster_wire::ROSTER_PATH,
                roster_wire::roster_parameters(&target)?,
            )
            .await?;
            Ok::<_, anyhow::Error>(pin_wire::parse_permission(
                &payload,
                &request.account,
                &target,
            )?)
        };
        let checked_at = Instant::now();
        let (ordinary, permissions, ttl) = tokio::join!(
            verify_group(&self.client, &self.session, &target),
            permissions,
            forward::verify_target_ttl(&self.client, &self.session, &target),
        );
        ordinary?;
        let can_mutate = permissions?;
        let snapshot = self.read_board(request, &target).await?;
        let board = CheckedBoard {
            snapshot,
            can_mutate,
            checked_at,
            ttl: ttl.ok(),
        };
        let projection = self
            .project(request, &board.snapshot, board.permits_mutation(&target))
            .await?;
        Ok((target, board, projection, source))
    }

    async fn get(&self, path: &'static str, parameters: Value) -> Result<Value> {
        let mut url = root_for(&self.session, "group_board")?;
        wire::validate_origin(&url, "https")?;
        url.set_path(path);
        let encrypted = self
            .session
            .encrypt_payload(&serde_json::to_vec(&parameters)?)?;
        url.query_pairs_mut()
            .append_pair("zpw_ver", wire::API_VERSION)
            .append_pair("zpw_type", self.session.client_type().api_type())
            .append_pair("params", &encrypted);
        tokio::time::timeout(
            DEADLINE,
            authenticated_request(&self.client, &self.session, url, None),
        )
        .await?
    }

    async fn read_board(
        &self,
        request: &Request,
        target: &Target,
    ) -> Result<pin_wire::PinSnapshot> {
        if !request.permits(&self.policy) {
            return Err(WireError::Unsupported.into());
        }
        let payload = self
            .get(
                pin_wire::LIST_PATH,
                pin_wire::list_parameters(target, self.session.imei())?,
            )
            .await?;
        Ok(pin_wire::parse_snapshot(&payload, target)?)
    }

    async fn project(
        &self,
        request: &Request,
        snapshot: &pin_wire::PinSnapshot,
        can_mutate: bool,
    ) -> Result<GroupPinSnapshot> {
        if !request.permits(&self.policy) {
            return Err(WireError::Unsupported.into());
        }
        let (account, thread, snapshot) = (
            request.account.clone(),
            request.thread.clone(),
            snapshot.clone(),
        );
        self.db
            .call(move |store| store.project_group_pins(&account, &thread, &snapshot, can_mutate))
            .await
    }

    async fn change(
        &self,
        request: &Request,
        target: &Target,
        board: CheckedBoard,
        projection: GroupPinSnapshot,
        source: PinSource,
        pinned: bool,
    ) -> Completion {
        use GroupPinOutcome::*;
        let current = board
            .snapshot
            .items
            .iter()
            .find(|entry| entry.reference.message_id == source.reference.message_id);
        if current.is_some_and(|entry| entry.reference != source.reference) {
            return (Unavailable, Some(projection));
        }
        if projection.unresolved.contains(&source.reference.message_id) {
            return (Unknown, Some(projection));
        }
        if current.is_some() == pinned {
            return (Accepted, Some(projection));
        }
        if !projection.can_mutate || (pinned && board.snapshot.total >= board.snapshot.limit) {
            return (Unavailable, Some(projection));
        }
        let parameters = if pinned {
            let name = if source.reference.sender_id == self.session.account_id() {
                self.session.display_name()
            } else {
                &source.sender_name
            };
            let name = if name.len() <= 256 && !name.chars().any(char::is_control) {
                name
            } else {
                ""
            };
            pin_wire::pin_parameters(
                target,
                &source.reference,
                &source.text,
                name,
                self.session.imei(),
            )
        } else {
            pin_wire::unpin_parameters(
                target,
                &board.snapshot,
                &current.expect("verified pinned topic").topic_id,
                self.session.imei(),
            )
        };
        let Ok(parameters) = parameters else {
            return (Unavailable, Some(projection));
        };

        let (account, thread, reference) = (
            request.account.clone(),
            request.thread.clone(),
            source.reference.clone(),
        );
        let baseline = board.snapshot.version;
        let reserved = self
            .db
            .call(move |store| {
                store.reserve_group_pin(&account, &thread, &source, pinned, baseline)
            })
            .await;
        if !matches!(reserved, Ok(true)) {
            return (Unavailable, Some(projection));
        }
        if !request.permits(&self.policy) || !board.permits_mutation(target) {
            let outcome = if self.release(request, &reference, pinned).await.is_ok() {
                Unavailable
            } else {
                Unknown
            };
            return (outcome, None);
        }

        let response = if pinned {
            group_roster::post(
                &self.client,
                &self.session,
                "group_board",
                pin_wire::PIN_PATH,
                parameters,
            )
            .await
        } else {
            self.get(pin_wire::UNPIN_PATH, parameters).await
        };
        let explicit_rejection = response.as_ref().err().is_some_and(|error| {
            matches!(
                wire_error(error),
                Some(WireError::Service(161 | 164 | 166 | 183 | 258))
            )
        });
        let rejected =
            explicit_rejection && self.release(request, &reference, pinned).await.is_ok();
        if let Ok(after) = self.read_board(request, target).await {
            let actual = after
                .items
                .iter()
                .find(|item| item.reference.message_id == reference.message_id);
            let confirmed = after.version > baseline
                && match actual {
                    Some(item) => pinned && item.reference == reference,
                    None => !pinned,
                };
            // A stale lower-version list cannot reconcile a newer attempt.
            if after.version > baseline || after == board.snapshot {
                if let Ok(projection) = self
                    .project(request, &after, board.permits_mutation(target))
                    .await
                {
                    return (
                        if confirmed {
                            Accepted
                        } else if rejected {
                            Rejected
                        } else {
                            Unknown
                        },
                        Some(projection),
                    );
                }
            }
        }
        (if rejected { Rejected } else { Unknown }, None)
    }

    async fn release(
        &self,
        request: &Request,
        reference: &pin_wire::PinReference,
        pinned: bool,
    ) -> Result<()> {
        let (account, thread, reference) = (
            request.account.clone(),
            request.thread.clone(),
            reference.clone(),
        );
        self.db
            .call(move |store| store.release_group_pin(&account, &thread, &reference, pinned))
            .await
    }
}
