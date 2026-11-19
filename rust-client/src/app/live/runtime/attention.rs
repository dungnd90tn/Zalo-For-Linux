//! Local read commands and background receipt/notification policy.
use super::*;
use crate::{domain::read_state::NotificationPreferences, protocol::read_state as receipt_wire};
use std::collections::HashSet;
use tokio::{task::JoinHandle, time::Instant};

pub(super) struct Candidate {
    pub thread: String,
    pub preview: Option<String>,
}

pub(super) struct CandidateSender {
    pub tx: mpsc::Sender<Candidate>,
    preview: std::sync::atomic::AtomicBool,
}
impl CandidateSender {
    pub fn new(tx: mpsc::Sender<Candidate>) -> Self {
        Self {
            tx,
            preview: false.into(),
        }
    }
    pub fn preview(&self, message: &Message) -> Option<String> {
        self.preview
            .load(std::sync::atomic::Ordering::Relaxed)
            .then(|| message.text.chars().take(160).collect())
    }
    fn set_preview(&self, enabled: bool) {
        self.preview
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) struct Attention<'a> {
    desktop: Option<&'a desktop::DesktopNotifications>,
    foreground: Option<String>,
    preferences: Option<NotificationPreferences>,
    privacy_revision: Option<u64>,
    muted: Option<HashSet<String>>,
    online: bool,
    mute_task: Option<JoinHandle<Result<HashSet<String>>>>,
    next_mute: Instant,
    read_task: Option<JoinHandle<Result<()>>>,
    next_read: Instant,
}
impl Drop for Attention<'_> {
    fn drop(&mut self) {
        if let Some(task) = self.mute_task.take() {
            task.abort();
        }
        if let Some(task) = self.read_task.take() {
            task.abort();
        }
    }
}
pub(super) fn permits_normal(events: &Events, thread: &str, confirmed: bool) -> bool {
    Target::from_thread(thread).is_ok()
        && events.privacy.read().is_ok_and(|policy| {
            policy.status != LivePrivacyStatus::Checking
                && (!confirmed || policy.confirmed)
                && !policy.hidden.contains(thread)
        })
}
fn confirmed_revision(privacy: &std::sync::RwLock<privacy::Projection>) -> Option<u64> {
    privacy
        .read()
        .ok()
        .and_then(|p| p.confirmed.then_some(p.revision))
}

pub(super) async fn wait_until(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}

impl<'a> Attention<'a> {
    pub(super) async fn new(
        db: &Database,
        events: &Events,
        desktop: Option<&'a desktop::DesktopNotifications>,
    ) -> Result<Self> {
        let account = events.account.clone();
        let preferences = db
            .call(move |s| s.notification_preferences(&account))
            .await
            .ok();
        let now = Instant::now();
        let mut state = Self {
            desktop,
            foreground: None,
            preferences,
            privacy_revision: None,
            muted: None,
            online: false,
            mute_task: None,
            next_mute: now,
            read_task: None,
            next_read: now,
        };
        state.update_preferences(preferences, events).await?;
        Ok(state)
    }

    async fn update_preferences(
        &mut self,
        preferences: Option<NotificationPreferences>,
        events: &Events,
    ) -> Result<()> {
        if self.preferences != preferences {
            if let Some(desktop) = &self.desktop {
                desktop.clear();
            }
        }
        self.preferences = preferences;
        if let Some(sender) = &events.unread {
            sender.set_preview(preferences.is_some_and(|p| p.enabled && p.preview));
        }
        events
            .emit(LiveEvent::NotificationPreferences {
                account_id: events.account.clone(),
                preferences,
            })
            .await
    }

    pub(super) fn notify(&self, candidate: Candidate, events: &Events) {
        let (Some(desktop), Some(preferences), Some(muted)) =
            (&self.desktop, self.preferences, &self.muted)
        else {
            return;
        };
        if preferences.enabled
            && self.mute_task.is_none()
            && self.foreground.as_deref() != Some(&candidate.thread)
            && !muted.contains(&candidate.thread)
            && permits_normal(events, &candidate.thread, true)
        {
            desktop.notify(
                &candidate.thread,
                candidate.preview.as_deref().filter(|_| preferences.preview),
            );
        }
    }

    pub(super) async fn command(
        &mut self,
        command: LiveCommand,
        db: &Database,
        events: &Events,
    ) -> Result<Option<LiveCommand>> {
        match command {
            LiveCommand::ReadVisible {
                thread,
                message_ids,
                ..
            } => {
                if permits_normal(events, &thread, false)
                    && !message_ids.is_empty()
                    && message_ids.len() <= 50
                {
                    let account = events.account.clone();
                    let id = thread.clone();
                    if db
                        .call(move |s| s.mark_visible_read(&account, &id, &message_ids))
                        .await
                        .is_ok()
                    {
                        if let Some(desktop) = &self.desktop {
                            desktop.close_thread(&thread);
                        }
                        if self.next_read > Instant::now() + Duration::from_secs(1) {
                            self.next_read = Instant::now() + Duration::from_millis(150);
                        }
                        events
                            .emit(LiveEvent::ReadStateChanged {
                                account_id: events.account.clone(),
                            })
                            .await?;
                    }
                }
            }
            LiveCommand::ReadState {
                generation,
                threads,
                thread,
                message_ids,
            } => {
                let threads: Vec<_> = threads
                    .into_iter()
                    .take(crate::domain::MAX_PAGE_SIZE)
                    .filter(|id| permits_normal(events, id, false))
                    .collect();
                let thread = thread.filter(|id| permits_normal(events, id, false));
                let account = events.account.clone();
                let requested = thread.clone();
                let (unread, delivery) = db
                    .call(move |s| {
                        let unread = s.read_state_snapshot(&account, &threads)?;
                        let delivery = match requested {
                            Some(thread) => s.message_delivery(&account, &thread, &message_ids)?,
                            None => Vec::new(),
                        };
                        Ok((unread, delivery))
                    })
                    .await
                    .unwrap_or_default();
                events
                    .emit(LiveEvent::ReadState {
                        account_id: events.account.clone(),
                        generation,
                        unread,
                        thread,
                        delivery,
                    })
                    .await?;
            }
            LiveCommand::NotificationForeground { thread } => {
                self.foreground = thread.filter(|id| permits_normal(events, id, false));
                if let (Some(desktop), Some(thread)) = (&self.desktop, &self.foreground) {
                    desktop.close_thread(thread);
                }
            }
            LiveCommand::SetNotificationPreferences { preferences } => {
                let account = events.account.clone();
                let saved = db
                    .call(move |s| s.set_notification_preferences(&account, preferences))
                    .await
                    .ok();
                if saved.is_some() {
                    self.update_preferences(Some(preferences), events).await?;
                } else {
                    // Preserve the last durable policy; report the failed write to settings.
                    events
                        .emit(LiveEvent::NotificationPreferences {
                            account_id: events.account.clone(),
                            preferences: None,
                        })
                        .await?;
                }
            }
            other => return Ok(Some(other)),
        }
        Ok(None)
    }

    pub(super) fn reset_transport(&mut self) {
        self.online = false;
        self.muted = None;
        self.next_mute = Instant::now();
        if let Some(task) = self.mute_task.take() {
            task.abort();
        }
        if let Some(task) = self.read_task.take() {
            task.abort();
        }
        if let Some(desktop) = &self.desktop {
            desktop.clear();
        }
    }

    pub(super) async fn wait_activation(&self) {
        if let Some(desktop) = &self.desktop {
            desktop.wait_activation().await;
        } else {
            std::future::pending::<()>().await;
        }
    }

    pub(super) fn next_wake(
        &self,
        connection: Option<&Connection>,
        events: &Events,
        session: &VerifiedSession,
    ) -> Option<Instant> {
        let online = connection.is_some_and(|c| c.decoder.authenticated());
        let revision = confirmed_revision(&events.privacy);
        if online != self.online || revision != self.privacy_revision {
            return Some(Instant::now());
        }
        if self.read_task.as_ref().is_some_and(JoinHandle::is_finished)
            || self.mute_task.as_ref().is_some_and(JoinHandle::is_finished)
        {
            return Some(Instant::now());
        }
        if self.read_task.is_some() || self.mute_task.is_some() {
            return Some(Instant::now() + Duration::from_millis(150));
        }
        if !online {
            return None;
        }
        let mute = (self.desktop.is_some() && self.preferences.is_some_and(|p| p.enabled))
            .then_some(self.next_mute);
        let read = (session.config().seen_v2 && revision.is_some()).then_some(self.next_read);
        mute.into_iter().chain(read).min()
    }

    // Only the small storage reads and task handoffs are awaited here. Network
    // requests run on independent, abort-on-drop tasks.
    pub(super) async fn poll(
        &mut self,
        connection: Option<&Connection>,
        db: &Database,
        events: &Events,
        session: &Arc<VerifiedSession>,
        client: &reqwest::Client,
    ) -> Result<()> {
        let revision = confirmed_revision(&events.privacy);
        if self.privacy_revision != revision {
            self.privacy_revision = revision;
            if let Some(desktop) = &self.desktop {
                desktop.clear();
            }
            if let Some(task) = self.read_task.take() {
                task.abort();
            }
        }
        let online = connection.is_some_and(|c| c.decoder.authenticated());
        let now = Instant::now();
        if self.online && !online {
            self.reset_transport();
        }
        self.online = online;
        if let Some(task) = self.mute_task.take_if(|task| task.is_finished()) {
            let next_muted = task.await.ok().and_then(Result::ok);
            if next_muted != self.muted {
                if let Some(desktop) = &self.desktop {
                    desktop.clear();
                }
            }
            self.muted = next_muted;
            self.next_mute = now + Duration::from_secs(30);
        }
        if self.desktop.is_some()
            && self.preferences.is_some_and(|p| p.enabled)
            && self.mute_task.is_none()
            && online
            && now >= self.next_mute
        {
            let (session, client) = (session.clone(), client.clone());
            self.mute_task = Some(tokio::spawn(
                async move { fetch_mutes(&client, &session).await },
            ));
        }
        if let Some(desktop) = &self.desktop {
            if let Some(thread) = desktop.take_activated() {
                if permits_normal(events, &thread, true)
                    && self.preferences.is_some_and(|p| p.enabled)
                    && self.mute_task.is_none()
                    && self
                        .muted
                        .as_ref()
                        .is_some_and(|muted| !muted.contains(&thread))
                {
                    let account = events.account.clone();
                    events
                        .emit(LiveEvent::NotificationOpen {
                            account_id: account,
                            thread,
                        })
                        .await?;
                }
            }
        }
        if let Some(task) = self.read_task.take_if(|task| task.is_finished()) {
            self.next_read = now
                + if task.await.is_ok_and(|result| result.is_ok()) {
                    Duration::from_millis(150)
                } else {
                    Duration::from_secs(15)
                };
        }
        if !online {
            if let Some(task) = self.read_task.take() {
                task.abort();
            }
        } else if session.config().seen_v2
            && revision.is_some()
            && self.read_task.is_none()
            && now >= self.next_read
        {
            self.next_read = now + Duration::from_secs(5);
            let account = events.account.clone();
            let pending = db
                .call(move |s| s.pending_read_threads(&account, 1))
                .await
                .unwrap_or_default();
            if let Some(thread) = pending
                .into_iter()
                .find(|thread| permits_normal(events, thread, true))
            {
                let account = events.account.clone();
                let requested = thread.clone();
                let references = db
                    .call(move |s| s.pending_read_receipts(&account, &requested, 50))
                    .await
                    .unwrap_or_default();
                if !references.is_empty() {
                    let (session, client, db) = (session.clone(), client.clone(), db.handle()?);
                    let privacy = events.privacy.clone();
                    self.read_task = Some(tokio::spawn(async move {
                        if !privacy.read().is_ok_and(|p| {
                            p.confirmed
                                && Some(p.revision) == revision
                                && !p.hidden.contains(&thread)
                        }) {
                            return Ok(());
                        }
                        send_receipts(&client, &session, &thread, &references).await?;
                        let ids: Vec<_> = references.into_iter().map(|r| r.message_id).collect();
                        let account = session.account_id().to_owned();
                        db.call(move |s| s.confirm_read_receipts(&account, &thread, &ids))
                            .await
                    }));
                }
            }
        }
        Ok(())
    }
}

pub(super) async fn apply_status(db: &Database, events: &Events, payload: &Value) -> Result<()> {
    let receipt_wire::StatusChanges {
        deliveries,
        watermarks,
    } = receipt_wire::statuses(&events.account, payload);
    if deliveries.is_empty() && watermarks.is_empty() {
        return Ok(());
    }
    let account = events.account.clone();
    let result = db
        .call(move |s| {
            if !watermarks.is_empty() {
                s.apply_server_read(&account, &watermarks)?;
            }
            if !deliveries.is_empty() {
                s.apply_receipt_updates(&account, &deliveries, now_ms())?;
            }
            Ok(())
        })
        .await;
    // Auxiliary statuses are not a socket health or authentication signal.
    if result.is_ok() {
        events
            .emit(LiveEvent::ReadStateChanged {
                account_id: events.account.clone(),
            })
            .await?;
    }
    Ok(())
}

async fn fetch_mutes(
    client: &reqwest::Client,
    session: &VerifiedSession,
) -> Result<HashSet<String>> {
    let mut url = root_for(session, "profile")?;
    wire::validate_origin(&url, "https")?;
    url.set_path("/api/social/profile/getmute");
    let params = session.encrypt_payload(&serde_json::to_vec(&json!({"imei": session.imei()}))?)?;
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type())
        .append_pair("params", &params);
    let data = tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, None),
    )
    .await??;
    Ok(receipt_wire::muted_threads(&data)?)
}

async fn send_receipts(
    client: &reqwest::Client,
    session: &VerifiedSession,
    thread: &str,
    references: &[crate::domain::read_state::ReceiptReference],
) -> Result<()> {
    let target = Target::from_thread(thread)?;
    let (service, path) = match target.kind {
        ThreadKind::Direct => ("chat", "/api/message/seenv2"),
        ThreadKind::Group => ("group", "/api/group/seenv2"),
    };
    let mut url = root_for(session, service)?;
    wire::validate_origin(&url, "https")?;
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type());
    let parameters = receipt_wire::seen_v2_parameters(&target, references, session.imei())?;
    let encoded = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &encoded)
        .finish();
    tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, Some(body)),
    )
    .await??;
    Ok(())
}
