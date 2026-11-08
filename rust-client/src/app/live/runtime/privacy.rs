//! Fail-closed presentation policy; raw receive transactions and cursors are independent.
use super::*;
use crate::protocol::hidden::HiddenSnapshot;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct Projection {
    pub status: LivePrivacyStatus,
    pub revision: u64,
    pub hidden: BTreeSet<String>,
    pub unlocked: Option<String>,
}
impl Default for Projection {
    // Only direct, unmanaged fixture Events use Default. run_with_transports explicitly starts Checking.
    fn default() -> Self {
        Self {
            status: LivePrivacyStatus::Ready,
            revision: 0,
            hidden: BTreeSet::new(),
            unlocked: None,
        }
    }
}
impl Projection {
    pub fn permits(&self, thread: &str) -> bool {
        self.status != LivePrivacyStatus::Checking
            && (!self.hidden.contains(thread) || self.unlocked.as_deref() == Some(thread))
    }
    pub fn event(&self, mut event: LiveEvent) -> Option<LiveEvent> {
        match &mut event {
            LiveEvent::Avatars { avatars, .. } => {
                avatars.retain(|entry| self.permits(&entry.thread_id))
            }
            LiveEvent::Ready { conversations, .. }
            | LiveEvent::DirectoryChanged { conversations, .. } => conversations.retain(|c| {
                self.status != LivePrivacyStatus::Checking && !self.hidden.contains(&c.id)
            }),
            LiveEvent::DirectoryPage { entries, .. } => entries.retain(|e| {
                self.status != LivePrivacyStatus::Checking && !self.hidden.contains(&e.thread_id())
            }),
            LiveEvent::HiddenMatches { candidates, .. } => candidates.retain(|c| {
                self.status != LivePrivacyStatus::Checking && self.hidden.contains(&c.thread_id)
            }),
            LiveEvent::Sent {
                account_id,
                thread,
                request_id,
                ..
            } if !self.permits(thread) => {
                return Some(LiveEvent::SendSettled {
                    account_id: account_id.clone(),
                    thread: thread.clone(),
                    request_id: *request_id,
                    accepted: true,
                })
            }
            LiveEvent::SendFailed {
                account_id,
                thread,
                request_id,
                ..
            } if !self.permits(thread) => {
                return Some(LiveEvent::SendSettled {
                    account_id: account_id.clone(),
                    thread: thread.clone(),
                    request_id: *request_id,
                    accepted: false,
                })
            }
            LiveEvent::Received { message, .. } if !self.permits(&message.thread_id) => {
                return None
            }
            LiveEvent::Opened { conversation, .. } if !self.permits(&conversation.id) => {
                return None
            }
            LiveEvent::Page { thread, .. }
            | LiveEvent::Overview { thread, .. }
            | LiveEvent::OverviewFailed { thread, .. }
            | LiveEvent::SearchResults { thread, .. }
            | LiveEvent::SearchFailed { thread, .. }
            | LiveEvent::MessagesRefreshed { thread, .. }
            | LiveEvent::MessagesRefreshFailed { thread, .. }
            | LiveEvent::SenderNames { thread, .. }
            | LiveEvent::RequestFailed { thread, .. }
                if !self.permits(thread) =>
            {
                return None
            }
            _ => {}
        }
        Some(event)
    }
}
struct Attempt {
    generation: u64,
    revision: u64,
    thread: String,
    pin: HiddenPin,
    token: u64,
}
pub(super) struct Privacy {
    snapshot: Option<HiddenSnapshot>,
    task: Option<tokio::task::JoinHandle<std::result::Result<HiddenSnapshot, DirectoryFailure>>>,
    pending: Option<Attempt>,
    token: u64,
    pub next_check: tokio::time::Instant,
}
impl Drop for Privacy {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
impl Privacy {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            task: None,
            pending: None,
            token: 0,
            next_check: tokio::time::Instant::now(),
        }
    }
    pub fn running(&self) -> bool {
        self.task.is_some()
    }
    pub async fn result(
        &mut self,
    ) -> std::result::Result<
        std::result::Result<HiddenSnapshot, DirectoryFailure>,
        tokio::task::JoinError,
    > {
        self.task.as_mut().expect("guarded privacy request").await
    }
    pub fn start(&mut self, session: Arc<VerifiedSession>, transports: &Transports) {
        if self.task.is_some() {
            return;
        }
        let fetcher = DirectoryFetcher::from_client(transports.http.clone());
        self.task = Some(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(15), fetcher.hidden(&session))
                .await
                .unwrap_or_else(|_| {
                    Err(DirectoryFailure {
                        stage: DirectoryStage::Synchronization,
                        category: DirectoryCategory::Timeout,
                        http_status: None,
                        server_code: None,
                        parser: None,
                    })
                })
        }));
    }
    /// Records the remembered hidden set as the current policy (see `MessageStore::provisional_live_privacy`).
    pub fn provisional(
        events: &Events,
        hidden: Vec<String>,
        status: LivePrivacyStatus,
    ) -> Result<()> {
        let mut state = events
            .privacy
            .write()
            .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?;
        state.status = status;
        state.revision = state.revision.wrapping_add(1);
        state.hidden = hidden.into_iter().collect();
        state.unlocked = None;
        Ok(())
    }
    pub async fn announce(events: &Events) -> Result<()> {
        let state = events
            .privacy
            .read()
            .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
            .clone();
        events
            .emit(LiveEvent::PrivacyChanged {
                account_id: events.account.clone(),
                revision: state.revision,
                status: state.status,
                hidden_threads: state.hidden.into_iter().collect(),
                unlocked_thread: state.unlocked,
            })
            .await
    }
    async fn publish_conversations(db: &Database, events: &Events) -> Result<()> {
        let account = events.account.clone();
        let conversations = db
            .call_with(StorageOperation::Conversations, move |s| {
                s.conversations(&account, crate::domain::MAX_PAGE_SIZE)
            })
            .await?;
        events
            .emit(LiveEvent::Ready {
                account_id: events.account.clone(),
                conversations,
            })
            .await
    }
    pub async fn relock(&mut self, db: &Database, events: &Events) -> Result<()> {
        self.token = self.token.wrapping_add(1);
        self.pending = None;
        let changed = {
            let mut state = events
                .privacy
                .write()
                .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?;
            if state.unlocked.take().is_some() {
                state.revision = state.revision.wrapping_add(1);
                true
            } else {
                false
            }
        };
        if changed {
            let account = events.account.clone();
            db.call(move |s| s.unlock_live_thread(&account, None))
                .await?;
            Self::announce(events).await?;
        }
        Ok(())
    }
    pub async fn completed(
        &mut self,
        result: std::result::Result<
            std::result::Result<HiddenSnapshot, DirectoryFailure>,
            tokio::task::JoinError,
        >,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        self.task.take();
        self.next_check = tokio::time::Instant::now() + Duration::from_secs(30);
        let snapshot = match result {
            Ok(Ok(snapshot)) => snapshot,
            other => {
                if let Ok(Err(error)) = other {
                    record_directory_failure(&error);
                }
                // Keep the interface usable: the last confirmed hidden set stays in force, any unlock ends.
                let account = events.account.clone();
                let remembered = db
                    .call(move |s| s.provisional_live_privacy(&account))
                    .await?;
                Self::provisional(events, remembered, LivePrivacyStatus::Failed)?;
                Self::announce(events).await?;
                if let Some(attempt) = self.pending.take() {
                    self.unlock_result(events, &attempt, HiddenUnlockOutcome::Unavailable)
                        .await?;
                }
                return Ok(());
            }
        };
        let changed = self
            .snapshot
            .as_ref()
            .is_none_or(|old| !old.same_policy(&snapshot));
        let old = events
            .privacy
            .read()
            .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
            .clone();
        let ready = old.status == LivePrivacyStatus::Ready;
        let unlocked = if changed || !ready {
            None
        } else {
            old.unlocked.clone()
        };
        // An unchanged verified snapshot already has the same TEMP policy and lease.
        // Avoid rebuilding up to 20,000 indexed rows on every background poll.
        if changed || !ready {
            let account = events.account.clone();
            let ids = snapshot.threads.clone();
            let lease = unlocked.clone();
            db.call(move |s| s.replace_live_privacy(&account, &ids, lease.as_deref()))
                .await?;
        }
        {
            let mut state = events
                .privacy
                .write()
                .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?;
            if changed || !ready {
                state.revision = state.revision.wrapping_add(1);
            }
            state.status = LivePrivacyStatus::Ready;
            state.hidden = snapshot.threads.iter().cloned().collect();
            state.unlocked = unlocked;
        }
        self.snapshot = Some(snapshot);
        if changed || !ready {
            Self::announce(events).await?;
            Self::publish_conversations(db, events).await?;
        }
        if let Some(attempt) = self.pending.take() {
            let state = events
                .privacy
                .read()
                .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
                .clone();
            if attempt.token != self.token {
                return Ok(());
            }
            if attempt.revision != state.revision || !state.hidden.contains(&attempt.thread) {
                self.unlock_result(events, &attempt, HiddenUnlockOutcome::Unavailable)
                    .await?;
                return Ok(());
            }
            if !self.snapshot.as_ref().is_some_and(|s| s.has_verifier()) {
                self.unlock_result(events, &attempt, HiddenUnlockOutcome::Unavailable)
                    .await?;
                return Ok(());
            }
            if !self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.verifies(&attempt.pin.digest()))
            {
                self.unlock_result(events, &attempt, HiddenUnlockOutcome::IncorrectPin)
                    .await?;
                return Ok(());
            }
            let account = events.account.clone();
            let thread = attempt.thread.clone();
            db.call(move |s| s.unlock_live_thread(&account, Some(&thread)))
                .await?;
            {
                let mut state = events
                    .privacy
                    .write()
                    .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?;
                state.unlocked = Some(attempt.thread.clone());
                state.revision = state.revision.wrapping_add(1);
            }
            Self::announce(events).await?;
            self.unlock_result(events, &attempt, HiddenUnlockOutcome::Unlocked)
                .await?;
            let account = events.account.clone();
            let thread = attempt.thread.clone();
            let conversation = db
                .call_with(StorageOperation::OpenConversation, move |s| {
                    let title = s
                        .directory_record(&account, &thread)?
                        .map(|r| r.display_name)
                        .unwrap_or_else(|| "Cuộc trò chuyện".into());
                    s.open_conversation(&account, &thread, &title)
                })
                .await?;
            events
                .emit(LiveEvent::Opened {
                    account_id: events.account.clone(),
                    generation: attempt.generation,
                    conversation,
                })
                .await?;
        }
        Ok(())
    }
    async fn unlock_result(
        &self,
        events: &Events,
        attempt: &Attempt,
        outcome: HiddenUnlockOutcome,
    ) -> Result<()> {
        let revision = events
            .privacy
            .read()
            .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
            .revision;
        events
            .emit(LiveEvent::HiddenUnlockResult {
                account_id: events.account.clone(),
                generation: attempt.generation,
                revision,
                thread: attempt.thread.clone(),
                outcome,
            })
            .await
    }
    /// Returns true for privacy commands; all others retain the existing messaging flow.
    pub async fn command(
        &mut self,
        command: LiveCommand,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        transports: &Transports,
    ) -> Result<Option<LiveCommand>> {
        match command {
            LiveCommand::RefreshPrivacy => {
                // The remembered policy stays in force while the check runs again, so the interface is not
                // covered meanwhile; the result replaces it when it arrives.
                self.start(session.clone(), transports);
            }
            LiveCommand::RelockHidden => self.relock(db, events).await?,
            LiveCommand::SearchHidden { generation, query } => {
                let account = events.account.clone();
                let search = query.clone();
                let candidates = db
                    .call_with(StorageOperation::DirectoryRead, move |s| {
                        s.hidden_candidates(&account, &search)
                    })
                    .await?;
                let revision = events
                    .privacy
                    .read()
                    .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
                    .revision;
                events
                    .emit(LiveEvent::HiddenMatches {
                        account_id: events.account.clone(),
                        generation,
                        revision,
                        query,
                        candidates,
                    })
                    .await?;
            }
            LiveCommand::UnlockHidden {
                generation,
                revision,
                thread,
                pin,
            } => {
                self.relock(db, events).await?;
                let attempt = Attempt {
                    generation,
                    revision,
                    thread,
                    pin,
                    token: self.token,
                };
                let state = events
                    .privacy
                    .read()
                    .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
                    .clone();
                if state.status != LivePrivacyStatus::Ready
                    || revision != state.revision
                    || !state.hidden.contains(&attempt.thread)
                {
                    self.unlock_result(events, &attempt, HiddenUnlockOutcome::Unavailable)
                        .await?;
                } else {
                    self.pending = Some(attempt);
                    self.start(session.clone(), transports);
                }
            }
            command => {
                if matches!(
                    &command,
                    LiveCommand::Reconnect | LiveCommand::RefreshDirectory { .. }
                ) {
                    self.start(session.clone(), transports);
                }
                let requested = match &command {
                    LiveCommand::Load { thread, .. }
                    | LiveCommand::Overview { thread, .. }
                    | LiveCommand::Search { thread, .. } => Some(thread.clone()),
                    LiveCommand::Open {
                        kind, remote_id, ..
                    } => Some(Target::new(*kind, remote_id)?.thread_id()),
                    _ => None,
                };
                let lease = events
                    .privacy
                    .read()
                    .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
                    .unlocked
                    .clone();
                if matches!(&command, LiveCommand::Directory { .. })
                    || requested.as_deref().is_some_and(|thread| {
                        lease.as_deref().is_some_and(|lease| lease != thread)
                            || self
                                .pending
                                .as_ref()
                                .is_some_and(|pending| pending.thread != thread)
                    })
                {
                    self.relock(db, events).await?;
                }
                return Ok(Some(command));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod avatar_tests {
    use super::*;
    #[test]
    fn late_avatar_metadata_is_filtered_against_current_hidden_lease() {
        let mut state = Projection {
            status: LivePrivacyStatus::Checking,
            revision: 1,
            hidden: ["g:22".into()].into_iter().collect(),
            unlocked: None,
        };
        let event = || LiveEvent::Avatars {
            account_id: "11".into(),
            generation: 7,
            avatars: vec![
                crate::app::live::AvatarIdentity {
                    thread_id: "d:22".into(),
                    url: "https://s120.avatar.talk.zdn.vn/user.png".into(),
                },
                crate::app::live::AvatarIdentity {
                    thread_id: "g:22".into(),
                    url: "https://s120.avatar.talk.zdn.vn/group.png".into(),
                },
            ],
        };
        let count = |state: &Projection| match state.event(event()).unwrap() {
            LiveEvent::Avatars { avatars, .. } => avatars.len(),
            _ => panic!(),
        };
        assert_eq!(count(&state), 0);
        state.status = LivePrivacyStatus::Ready;
        assert_eq!(count(&state), 1);
        state.unlocked = Some("g:22".into());
        assert_eq!(count(&state), 2);
        state.unlocked = None;
        assert_eq!(count(&state), 1);
        // A failed re-check keeps the known policy in force: hidden stays hidden, the rest stays visible.
        state.status = LivePrivacyStatus::Failed;
        assert_eq!(count(&state), 1);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn search_replies_for_locked_threads_never_reach_the_ui() {
        let mut state = Projection {
            status: LivePrivacyStatus::Ready,
            revision: 1,
            hidden: ["d:22".into()].into_iter().collect(),
            unlocked: None,
        };
        let results = |thread: &str| LiveEvent::SearchResults {
            account_id: "11".into(),
            request: 1,
            thread: thread.into(),
            search: Box::default(),
        };
        let failed = |thread: &str| LiveEvent::SearchFailed {
            account_id: "11".into(),
            request: 1,
            thread: thread.into(),
        };
        assert!(state.event(results("d:22")).is_none());
        assert!(state.event(failed("d:22")).is_none());
        assert!(state.event(results("d:33")).is_some());
        state.unlocked = Some("d:22".into());
        assert!(state.event(results("d:22")).is_some());
        state.status = LivePrivacyStatus::Checking;
        assert!(state.event(results("d:33")).is_none());
    }
}
