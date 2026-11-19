//! Scoped, one-shot group mutations. The actor owns durable reservation and settlement.
use super::*;
use crate::{
    protocol::{group_actions as action_wire, group_roster as roster},
    store::{GroupActionFailure, GroupActionOutcome},
};
use tokio::{task::JoinHandle, time::Instant};

const PROOF_AGE: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Invite { member: String },
    Leave,
}

#[derive(Clone, Debug)]
pub(super) struct ActionRequest {
    pub thread: String,
    pub privacy_revision: u64,
    pub action: Action,
}

impl ActionRequest {
    fn target(&self, account: &str) -> Result<Target, WireError> {
        wire::numeric_id(account)?;
        if let Action::Invite { member } = &self.action {
            wire::numeric_id(member)?;
            if member == account {
                return Err(WireError::Unsupported);
            }
        }
        self.roster_request().target()
    }

    fn roster_request(&self) -> group_roster::Request {
        group_roster::Request {
            thread: self.thread.clone(),
            privacy_revision: self.privacy_revision,
            after: None,
        }
    }

    fn permits(&self, policy: &std::sync::RwLock<privacy::Projection>) -> bool {
        policy.read().is_ok_and(|scope| {
            scope.confirmed
                && scope.status == LivePrivacyStatus::Ready
                && scope.revision == self.privacy_revision
                && !scope.hidden.contains(&self.thread)
                && match &self.action {
                    Action::Invite { member } => !scope.hidden.contains(&format!("d:{member}")),
                    Action::Leave => true,
                }
        })
    }
}

pub(super) enum Preflight {
    Ready(Proof),
    AlreadyMember,
}

pub(super) struct Proof {
    request: ActionRequest,
    account: String,
    path: &'static str,
    parameters: Value,
    checked_at: Instant,
}

/// The live actor should revalidate the exact known friend on the SQLite
/// worker and reserve the corresponding intent after this preflight returns.
pub(super) async fn preflight(
    client: &reqwest::Client,
    session: &VerifiedSession,
    policy: &std::sync::RwLock<privacy::Projection>,
    request: &ActionRequest,
) -> Result<Preflight> {
    let account = session.account_id();
    let target = request.target(account)?;
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    // Ordinary group mutation must have positive non-E2EE classification.
    // getmg-v2 independently proves current role, membership and settings.
    verify_group(client, session, &target).await?;
    let checked_at = Instant::now();
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    let payload = group_roster::post(
        client,
        session,
        "group",
        roster::ROSTER_PATH,
        roster::roster_parameters(&target)?,
    )
    .await?;
    let group = roster::verify_group(&payload, account, &target)?;
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    let (path, parameters) = match &request.action {
        Action::Invite { member }
            if group.permissions.invite
                && group.contains(member)
                && group.can_view(account, member) =>
        {
            return Ok(Preflight::AlreadyMember);
        }
        Action::Invite { member } if group.contains(member) => {
            // Do not expose members hidden by this group's visibility policy
            // via a mutation check on a name the user already knows.
            return Err(WireError::Unsupported.into());
        }
        Action::Invite { member } if group.permissions.invite => (
            action_wire::INVITE_PATH,
            action_wire::invite_parameters(&target, member, session.imei())?,
        ),
        Action::Leave if group.permissions.leave => (
            action_wire::LEAVE_PATH,
            action_wire::leave_parameters(&target, session.imei())?,
        ),
        _ => return Err(WireError::Unsupported.into()),
    };
    Ok(Preflight::Ready(Proof {
        request: request.clone(),
        account: account.into(),
        path,
        parameters,
        checked_at,
    }))
}

pub(super) async fn dispatch(
    client: &reqwest::Client,
    session: &VerifiedSession,
    policy: &std::sync::RwLock<privacy::Projection>,
    proof: Proof,
) -> GroupActionOutcome {
    use GroupActionOutcome as Outcome;
    if proof.account != session.account_id()
        || proof.checked_at.elapsed() > PROOF_AGE
        || !proof.request.permits(policy)
    {
        return Outcome::NotSent;
    }
    let response = group_roster::post(client, session, "group", proof.path, proof.parameters).await;
    // Actor owns durable settlement even if the UI scope changes as the
    // response returns. The event policy still filters stale projections.
    match (proof.request.action, response) {
        (Action::Leave, Ok(payload)) => match action_wire::leave_ack(&payload) {
            Ok(()) => Outcome::Left,
            Err(_) => Outcome::Unknown,
        },
        (Action::Invite { member }, Ok(payload)) => {
            use action_wire::InviteAck as Ack;
            match action_wire::invite_ack(&payload, &member) {
                Ok(Ack::Accepted) => Outcome::InviteAccepted,
                Ok(Ack::ApprovalPending) => Outcome::ApprovalPending,
                Ok(Ack::InvitationSent) => Outcome::InvitationSent,
                Ok(Ack::AlreadyInvited) => Outcome::AlreadyInvited,
                Ok(Ack::AlreadyMember) => Outcome::AlreadyMember,
                Ok(Ack::Rejected) => Outcome::Rejected,
                Err(_) => Outcome::Unknown,
            }
        }
        (_, Err(error)) if wire_error(&error) == Some(&WireError::Service(166)) => {
            Outcome::Rejected
        }
        // 161/164 remain unverified membership. A failed or cut-off reply
        // cannot authorize eviction or another mutation.
        (_, Err(_)) => Outcome::Unknown,
    }
}

struct Ticket {
    account_id: String,
    request_id: u64,
    thread: String,
    privacy_revision: u64,
}

impl Ticket {
    fn new(events: &Events, request_id: u64, thread: String, privacy_revision: u64) -> Self {
        Self {
            account_id: events.account.clone(),
            request_id,
            thread,
            privacy_revision,
        }
    }

    fn roster_request(&self, after: Option<String>) -> group_roster::Request {
        group_roster::Request {
            thread: self.thread.clone(),
            privacy_revision: self.privacy_revision,
            after,
        }
    }

    fn roster(self, page: Option<GroupMemberPage>) -> LiveEvent {
        LiveEvent::GroupMembers {
            account_id: self.account_id,
            request_id: self.request_id,
            thread: self.thread,
            privacy_revision: self.privacy_revision,
            page,
        }
    }

    fn targets(self, targets: Option<Vec<GroupInviteTarget>>) -> LiveEvent {
        LiveEvent::GroupInviteTargets {
            account_id: self.account_id,
            request_id: self.request_id,
            thread: self.thread,
            privacy_revision: self.privacy_revision,
            targets,
        }
    }

    fn finished(self, member: Option<String>, outcome: GroupActionOutcome) -> LiveEvent {
        LiveEvent::GroupActionFinished {
            account_id: self.account_id,
            request_id: self.request_id,
            thread: self.thread,
            privacy_revision: self.privacy_revision,
            member,
            outcome,
        }
    }
}

type RosterTask = (Ticket, JoinHandle<Option<GroupMemberPage>>);
type MutationTask = (Ticket, Option<String>, JoinHandle<GroupActionOutcome>);

#[derive(Default)]
pub(super) struct Groups {
    roster: Option<RosterTask>,
    mutation: Option<MutationTask>,
}

impl Drop for Groups {
    fn drop(&mut self) {
        self.cancel_roster();
        if let Some((_, _, task)) = self.mutation.take() {
            // After a crash or shutdown, the store recovers pending intents as
            // unknown. A lost response never creates a retry permit.
            task.abort();
        }
    }
}

impl Groups {
    fn cancel_roster(&mut self) {
        if let Some((_, task)) = self.roster.take() {
            task.abort();
        }
    }

    pub(super) async fn next(&mut self) -> LiveEvent {
        tokio::select! {
            result = async { (&mut self.roster.as_mut().expect("guarded roster").1).await }, if self.roster.is_some() => {
                let (ticket, _) = self.roster.take().expect("completed roster");
                ticket.roster(result.ok().flatten())
            },
            result = async { (&mut self.mutation.as_mut().expect("guarded mutation").2).await }, if self.mutation.is_some() => {
                let (ticket, member, _) = self.mutation.take().expect("completed mutation");
                ticket.finished(member, result.unwrap_or(GroupActionOutcome::Unknown))
            },
            else => std::future::pending().await
        }
    }

    pub(super) async fn command(
        &mut self,
        command: LiveCommand,
        online: bool,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<Option<LiveCommand>> {
        match command {
            LiveCommand::GroupMembers {
                request_id,
                thread,
                privacy_revision,
                after,
            } => {
                let ticket = Ticket::new(events, request_id, thread, privacy_revision);
                let request = ticket.roster_request(after);
                self.cancel_roster();
                if !online || request_id == 0 || !request.permits(&events.privacy) {
                    events.emit(ticket.roster(None)).await?;
                    return Ok(None);
                }
                let (client, session, policy, db) = (
                    client.clone(),
                    session.clone(),
                    events.privacy.clone(),
                    db.handle()?,
                );
                let task = tokio::spawn(async move {
                    fetch_roster(&client, &session, &policy, &db, &request)
                        .await
                        .ok()
                });
                self.roster = Some((ticket, task));
            }
            LiveCommand::CancelGroupMembers { request_id } => {
                if self
                    .roster
                    .as_ref()
                    .is_some_and(|(ticket, _)| ticket.request_id == request_id)
                {
                    self.cancel_roster();
                }
            }
            LiveCommand::GroupInviteTargets {
                request_id,
                thread,
                privacy_revision,
                query,
            } => {
                let ticket = Ticket::new(events, request_id, thread, privacy_revision);
                let request = ticket.roster_request(None);
                let mut targets = None;
                if online
                    && request_id != 0
                    && request.permits(&events.privacy)
                    && query.len() <= 512
                    && query.chars().count() <= 128
                    && !query.chars().any(char::is_control)
                {
                    let (account, thread) = (ticket.account_id.clone(), ticket.thread.clone());
                    targets = db
                        .call(move |store| store.group_invite_targets(&account, &thread, &query))
                        .await
                        .ok();
                }
                events.emit(ticket.targets(targets)).await?;
            }
            LiveCommand::GroupInvite {
                request_id,
                thread,
                privacy_revision,
                member,
            } => {
                let ticket = Ticket::new(events, request_id, thread, privacy_revision);
                self.start_mutation(ticket, Some(member), online, db, events, session, client)
                    .await?;
            }
            LiveCommand::GroupLeave {
                request_id,
                thread,
                privacy_revision,
            } => {
                let ticket = Ticket::new(events, request_id, thread, privacy_revision);
                self.start_mutation(ticket, None, online, db, events, session, client)
                    .await?;
            }
            other => return Ok(Some(other)),
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_mutation(
        &mut self,
        ticket: Ticket,
        member: Option<String>,
        online: bool,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let request = ActionRequest {
            thread: ticket.thread.clone(),
            privacy_revision: ticket.privacy_revision,
            action: member
                .clone()
                .map_or(Action::Leave, |member| Action::Invite { member }),
        };
        if self
            .mutation
            .as_ref()
            .is_some_and(|(active, _, _)| active.request_id == ticket.request_id)
        {
            return Ok(());
        }
        if !online
            || self.mutation.is_some()
            || ticket.request_id == 0
            || request.target(&events.account).is_err()
            || !request.permits(&events.privacy)
        {
            return events
                .emit(ticket.finished(member, GroupActionOutcome::NotSent))
                .await;
        }
        let (client, session, policy, db) = (
            client.clone(),
            session.clone(),
            events.privacy.clone(),
            db.handle()?,
        );
        let task =
            tokio::spawn(
                async move { run_mutation(&client, &session, &policy, &db, request).await },
            );
        self.mutation = Some((ticket, member, task));
        Ok(())
    }
}

async fn fetch_roster(
    client: &reqwest::Client,
    session: &VerifiedSession,
    policy: &std::sync::RwLock<privacy::Projection>,
    db: &DatabaseHandle,
    request: &group_roster::Request,
) -> Result<GroupMemberPage> {
    let (account, thread) = (session.account_id().to_owned(), request.thread.clone());
    let (guard_account, guard_thread) = (account.clone(), thread.clone());
    db.call(move |store| store.validate_group_scope(&guard_account, &guard_thread))
        .await?;
    let page = group_roster::load_page(client, session, policy, request).await?;
    let confirmed_members: Vec<_> = page
        .members
        .iter()
        .filter(|member| member.id != account)
        .map(|member| member.id.clone())
        .collect();
    db.call(move |store| {
        store.validate_group_scope(&account, &thread)?;
        for member in confirmed_members {
            store.confirm_group_member(&account, &thread, &member)?;
        }
        Ok(())
    })
    .await?;
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    Ok(page)
}

async fn run_mutation(
    client: &reqwest::Client,
    session: &VerifiedSession,
    policy: &std::sync::RwLock<privacy::Projection>,
    db: &DatabaseHandle,
    request: ActionRequest,
) -> GroupActionOutcome {
    use GroupActionOutcome as Outcome;
    let (account, thread, action) = (
        session.account_id().to_owned(),
        request.thread.clone(),
        request.action.clone(),
    );
    let reserved = db
        .call(move |store| {
            let result = match &action {
                Action::Invite { member } => {
                    store.prepare_group_invite(&account, &thread, member, now_ms())
                }
                Action::Leave => store.prepare_group_leave(&account, &thread, now_ms()),
            };
            match result {
                Ok(intent) => Ok(Ok(intent.id)),
                Err(error) => {
                    let outcome = match error.downcast_ref::<GroupActionFailure>() {
                        Some(GroupActionFailure::Unsettled) => {
                            let member = match &action {
                                Action::Invite { member } => Some(member.as_str()),
                                Action::Leave => None,
                            };
                            store
                                .group_pending_outcome(&account, &thread, member)?
                                .unwrap_or(Outcome::Unknown)
                        }
                        Some(GroupActionFailure::AlreadyLeft) if action == Action::Leave => {
                            Outcome::Left
                        }
                        _ => Outcome::NotSent,
                    };
                    Ok(Err(outcome))
                }
            }
        })
        .await;
    let id = match reserved {
        Ok(Ok(id)) => id,
        Ok(Err(outcome)) => return outcome,
        Err(_) => return Outcome::Unknown,
    };

    let result = match preflight(client, session, policy, &request).await {
        Err(_) => Outcome::NotSent,
        Ok(Preflight::AlreadyMember) => Outcome::AlreadyMember,
        Ok(Preflight::Ready(proof)) => {
            let account = session.account_id().to_owned();
            match db
                .call(move |store| store.group_action_outcome(&account, id))
                .await
            {
                Ok(Outcome::Pending) => dispatch(client, session, policy, proof).await,
                Ok(outcome) => outcome,
                Err(_) => Outcome::Unknown,
            }
        }
    };
    let account = session.account_id().to_owned();
    db.call(move |store| store.finish_group_action(&account, id, result))
        .await
        .unwrap_or(Outcome::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_mutation_scope_rejects_self_stale_or_hidden_subjects() {
        let request = ActionRequest {
            thread: "g:77".into(),
            privacy_revision: 4,
            action: Action::Invite {
                member: "22".into(),
            },
        };
        let privacy = std::sync::RwLock::new(privacy::Projection {
            confirmed: true,
            revision: 4,
            ..Default::default()
        });
        assert!(request.target("22").is_err());
        assert!(request.target("11").is_ok());
        assert!(request.permits(&privacy));
        privacy.write().unwrap().hidden.insert("d:22".into());
        assert!(!request.permits(&privacy));
        let request = ActionRequest {
            action: Action::Leave,
            ..request
        };
        assert!(request.permits(&privacy));
        privacy.write().unwrap().revision += 1;
        assert!(!request.permits(&privacy));
    }
}
