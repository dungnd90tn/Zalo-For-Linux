//! Refresh late durable media metadata without replacing a page or its drafts.
use crate::{
    app::{
        live::{LiveCommand, LiveEvent, LiveWorker},
        Model, SubmitError,
    },
    domain::{validate_id, MediaKind, Message},
    store::MAX_REFRESH_MESSAGES,
};
use eframe::egui;
use std::time::{Duration, Instant};
const COALESCE: Duration = Duration::from_millis(180);
const BUSY_RETRY: Duration = Duration::from_millis(100);
const READ_RETRY: Duration = Duration::from_secs(2);
#[derive(Clone, PartialEq, Eq)]
struct Scope {
    account: String,
    thread: String,
    page_generation: u64,
    ids: Vec<String>,
}
impl Scope {
    fn capture(account: &str, model: &Model) -> Self {
        let thread = model.selected.as_deref().unwrap_or_default();
        let mut ids = Vec::new();
        for message in model.messages.iter().rev().filter(|m| {
            m.account_id == account
                && m.thread_id == thread
                && m.media.is_none()
                && m.sticker.is_none()
                && is_media_placeholder(&m.text)
        }) {
            if validate_id(&message.id).is_ok() && !ids.contains(&message.id) {
                ids.push(message.id.clone());
            }
            if ids.len() == MAX_REFRESH_MESSAGES {
                break;
            }
        }
        ids.sort_unstable();
        Self {
            account: account.into(),
            thread: thread.into(),
            page_generation: model.generation,
            ids,
        }
    }
}
fn is_media_placeholder(text: &str) -> bool {
    text == MediaKind::Photo.legacy_placeholder()
        || text == MediaKind::WebContent.legacy_placeholder()
}
struct Flight {
    epoch: u64,
    scope: Scope,
}
#[derive(Default)]
pub(super) struct Hydration {
    scope: Option<Scope>,
    epoch: u64,
    in_flight: Option<Flight>,
    due: Option<Instant>,
}
impl Hydration {
    pub fn sync_scope(&mut self, account: &str, model: &Model, ctx: &egui::Context) {
        let scope = Scope::capture(account, model);
        if self.scope.as_ref() != Some(&scope) {
            self.epoch = self.epoch.wrapping_add(1);
            self.due = (!scope.ids.is_empty()).then_some(Instant::now());
            self.scope = Some(scope);
            if self.due.is_some() {
                ctx.request_repaint();
            }
        }
    }
    pub fn invalidate(&mut self, account: &str, model: &Model, ctx: &egui::Context) {
        self.sync_scope(account, model, ctx);
        self.epoch = self.epoch.wrapping_add(1);
        if self
            .scope
            .as_ref()
            .is_some_and(|scope| !scope.ids.is_empty())
        {
            let due = Instant::now() + COALESCE;
            self.due = Some(self.due.map_or(due, |old| old.min(due)));
            ctx.request_repaint_after(COALESCE);
        }
    }
    pub fn worker_closed(&mut self) {
        self.in_flight = None;
        self.epoch = self.epoch.wrapping_add(1);
        self.due = self
            .scope
            .as_ref()
            .filter(|scope| !scope.ids.is_empty())
            .map(|_| Instant::now() + READ_RETRY);
    }
    pub fn poll(&mut self, worker: Option<&LiveWorker>, ctx: &egui::Context) {
        if let Some(worker) = worker {
            self.submit(ctx, |command| worker.submit(command));
        }
    }
    fn submit(
        &mut self,
        ctx: &egui::Context,
        submit: impl FnOnce(LiveCommand) -> Result<(), SubmitError>,
    ) {
        if self.in_flight.is_some() {
            return;
        }
        let Some(due) = self.due else {
            return;
        };
        if due > Instant::now() {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
            return;
        }
        let Some(scope) = self
            .scope
            .as_ref()
            .filter(|scope| !scope.ids.is_empty())
            .cloned()
        else {
            self.due = None;
            return;
        };
        match submit(LiveCommand::RefreshMessages {
            generation: self.epoch,
            thread: scope.thread.clone(),
            message_ids: scope.ids.clone(),
        }) {
            Ok(()) => {
                self.in_flight = Some(Flight {
                    epoch: self.epoch,
                    scope,
                });
                self.due = None;
            }
            Err(_) => {
                self.due = Some(Instant::now() + BUSY_RETRY);
                ctx.request_repaint_after(BUSY_RETRY);
            }
        }
    }
    pub fn apply(
        &mut self,
        event: &LiveEvent,
        account: &str,
        model: &mut Model,
        ctx: &egui::Context,
    ) -> bool {
        let (event_account, epoch, thread, messages) = match event {
            LiveEvent::MessagesRefreshed {
                account_id,
                generation,
                thread,
                messages,
            } => (account_id, *generation, thread, Some(messages)),
            LiveEvent::MessagesRefreshFailed {
                account_id,
                generation,
                thread,
            } => (account_id, *generation, thread, None),
            _ => return false,
        };
        if event_account != account {
            return false;
        }
        self.sync_scope(account, model, ctx);
        let Some(flight) = self.in_flight.as_ref() else {
            return false;
        };
        if flight.epoch != epoch
            || flight.scope.account != *event_account
            || flight.scope.thread != *thread
        {
            return false;
        }
        let flight = self.in_flight.take().unwrap();
        if epoch != self.epoch || self.scope.as_ref() != Some(&flight.scope) {
            return false;
        }
        let Some(messages) = messages else {
            // Failed reads do not mark this dirty revision as successfully read.
            self.epoch = self.epoch.wrapping_add(1);
            self.due = Some(Instant::now() + READ_RETRY);
            ctx.request_repaint_after(READ_RETRY);
            return false;
        };
        if messages.len() > MAX_REFRESH_MESSAGES {
            self.epoch = self.epoch.wrapping_add(1);
            self.due = Some(Instant::now() + READ_RETRY);
            ctx.request_repaint_after(READ_RETRY);
            return false;
        }
        let mut changed = false;
        for snapshot in messages {
            if snapshot.account_id != account
                || snapshot.thread_id != *thread
                || flight.scope.ids.binary_search(&snapshot.id).is_err()
            {
                continue;
            }
            if let Some(existing) = model.messages.iter_mut().find(|m| m.id == snapshot.id) {
                if same_identity(existing, snapshot)
                    && existing.media.is_none()
                    && snapshot.media.is_some()
                {
                    existing.media = snapshot.media.clone();
                    changed = true;
                }
            }
        }
        // Rows omitted by the read remain visible, and negative results are
        // remembered until another committed directory update or page change.
        self.scope = Some(Scope::capture(account, model));
        self.due = None;
        if changed {
            ctx.request_repaint();
        }
        changed
    }
}
fn same_identity(a: &Message, b: &Message) -> bool {
    a.account_id == b.account_id
        && a.thread_id == b.thread_id
        && a.id == b.id
        && a.sender == b.sender
        && a.text == b.text
        && a.sent_at_ms == b.sent_at_ms
        && a.sticker == b.sticker
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::DraftBook,
        domain::{MediaDescriptor, MediaMessageContent, PhotoDescriptor},
        store::MessageStore,
    };
    fn message(id: &str) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: id.into(),
            sender: "22".into(),
            text: MediaKind::Photo.legacy_placeholder().into(),
            sent_at_ms: 100,
            sticker: None,
            media: None,
        }
    }
    fn descriptor() -> MediaDescriptor {
        MediaDescriptor::Photo(PhotoDescriptor {
            title: "Ảnh đã tải metadata".into(),
            caption: String::new(),
            thumbnail_url: Some("https://f12-zpcloud.zdn.vn/synthetic.jpg".into()),
            original_url: None,
            width: Some(640),
            height: Some(480),
        })
    }
    fn model() -> Model {
        Model {
            generation: 7,
            selected: Some("d:22".into()),
            messages: vec![message("old-1"), message("old-2")],
            has_older: true,
            ..Default::default()
        }
    }
    fn request(hydration: &mut Hydration, ctx: &egui::Context) -> (u64, String, Vec<String>) {
        hydration.due = Some(Instant::now());
        let mut command = None;
        hydration.submit(ctx, |value| {
            command = Some(value);
            Ok(())
        });
        match command.unwrap() {
            LiveCommand::RefreshMessages {
                generation,
                thread,
                message_ids,
            } => (generation, thread, message_ids),
            _ => panic!("wrong command"),
        }
    }
    fn event(epoch: u64, messages: Vec<Message>) -> LiveEvent {
        LiveEvent::MessagesRefreshed {
            account_id: "11".into(),
            generation: epoch,
            thread: "d:22".into(),
            messages,
        }
    }
    #[test]
    fn late_committed_enrichment_updates_open_older_page_without_reselect_or_draft_loss() {
        let ctx = egui::Context::default();
        let mut store = MessageStore::in_memory().unwrap();
        let mut model = model();
        let mut drafts = DraftBook::default();
        drafts.select("d:22").unwrap();
        drafts.active.text = "Bản nháp đang soạn".into();
        store.insert_batch(&model.messages).unwrap();
        let mut latest = message("outside-current-range");
        latest.sent_at_ms = 1000;
        store.insert_batch(&[latest]).unwrap();
        let mut hydration = Hydration::default();
        hydration.sync_scope("11", &model, &ctx);
        let (first, _, ids) = request(&mut hydration, &ctx);
        assert!(!hydration.apply(
            &event(first, store.refresh_messages("11", "d:22", &ids).unwrap()),
            "11",
            &mut model,
            &ctx
        ));
        let content=MediaMessageContent::new(MediaKind::Photo,&serde_json::json!({"title":"Ảnh nhận","thumb":"https://f12-zpcloud.zdn.vn/synthetic.jpg"})).unwrap();
        let mut enriched = message("old-1");
        enriched.media = Some(content.descriptor().clone());
        store
            .receive_batch_with_media("11", &[(enriched.clone(), None, None, Some(content))], None)
            .unwrap();
        hydration.invalidate("11", &model, &ctx);
        let (second, _, ids) = request(&mut hydration, &ctx);
        assert_ne!(first, second);
        assert!(hydration.apply(
            &event(second, store.refresh_messages("11", "d:22", &ids).unwrap()),
            "11",
            &mut model,
            &ctx
        ));
        assert_eq!(model.messages[0], enriched);
        assert_eq!(model.messages[1], message("old-2"));
        assert_eq!(model.messages.len(), 2);
        assert_eq!(model.generation, 7);
        assert_eq!(model.selected.as_deref(), Some("d:22"));
        assert!(model.has_older);
        assert!(!model.pending);
        assert_eq!(drafts.active.text, "Bản nháp đang soạn");
        assert!(hydration.due.is_none());
    }
    #[test]
    fn directory_revision_during_pending_read_discards_old_result_and_coalesces_one_followup() {
        let ctx = egui::Context::default();
        let mut model = model();
        let mut hydration = Hydration::default();
        hydration.sync_scope("11", &model, &ctx);
        let (old, _, _) = request(&mut hydration, &ctx);
        hydration.invalidate("11", &model, &ctx);
        hydration.invalidate("11", &model, &ctx);
        hydration.submit(&ctx, |_| panic!("must preserve one physical request"));
        let mut enriched = message("old-1");
        enriched.media = Some(descriptor());
        assert!(!hydration.apply(&event(old, vec![enriched.clone()]), "11", &mut model, &ctx));
        assert!(model.messages[0].media.is_none());
        let (new, _, _) = request(&mut hydration, &ctx);
        assert_ne!(old, new);
        assert!(!hydration.apply(&event(old, vec![enriched.clone()]), "11", &mut model, &ctx));
        assert!(hydration.in_flight.is_some());
        assert!(hydration.apply(&event(new, vec![enriched]), "11", &mut model, &ctx));
        assert!(hydration.in_flight.is_none());
    }
    #[test]
    fn wrong_account_thread_epoch_page_and_identity_cannot_mutate_the_pane() {
        let ctx = egui::Context::default();
        let mut model = model();
        let before = model.messages.clone();
        let mut hydration = Hydration::default();
        hydration.sync_scope("11", &model, &ctx);
        let (epoch, _, _) = request(&mut hydration, &ctx);
        let mut enriched = message("old-1");
        enriched.media = Some(descriptor());
        let foreign = LiveEvent::MessagesRefreshed {
            account_id: "33".into(),
            generation: epoch,
            thread: "d:22".into(),
            messages: vec![enriched.clone()],
        };
        assert!(!hydration.apply(&foreign, "11", &mut model, &ctx));
        let other = LiveEvent::MessagesRefreshed {
            account_id: "11".into(),
            generation: epoch,
            thread: "g:22".into(),
            messages: vec![enriched.clone()],
        };
        assert!(!hydration.apply(&other, "11", &mut model, &ctx));
        assert!(hydration.in_flight.is_some());
        model.generation += 1;
        assert!(!hydration.apply(
            &event(epoch, vec![enriched.clone()]),
            "11",
            &mut model,
            &ctx
        ));
        assert_eq!(model.messages, before);
        let (epoch, _, _) = request(&mut hydration, &ctx);
        let mut wrong = enriched.clone();
        wrong.sender = "44".into();
        assert!(!hydration.apply(&event(epoch, vec![wrong]), "11", &mut model, &ctx));
        assert_eq!(model.messages, before);
        hydration.invalidate("11", &model, &ctx);
        let (epoch, _, _) = request(&mut hydration, &ctx);
        let mut wrong = enriched.clone();
        wrong.text = "different body".into();
        assert!(!hydration.apply(&event(epoch, vec![wrong]), "11", &mut model, &ctx));
        assert_eq!(model.messages, before);
        hydration.invalidate("11", &model, &ctx);
        let (epoch, _, _) = request(&mut hydration, &ctx);
        let mut wrong = enriched;
        wrong.sent_at_ms += 1;
        assert!(!hydration.apply(&event(epoch, vec![wrong]), "11", &mut model, &ctx));
        assert_eq!(model.messages, before);
    }
    #[test]
    fn read_failure_backpressure_and_worker_restart_keep_revision_retryable_and_bound_ids() {
        let ctx = egui::Context::default();
        let mut model = model();
        model.messages = (0..80).map(|id| message(&id.to_string())).collect();
        let mut hydration = Hydration::default();
        hydration.sync_scope("11", &model, &ctx);
        assert_eq!(
            hydration.scope.as_ref().unwrap().ids.len(),
            MAX_REFRESH_MESSAGES
        );
        hydration.submit(&ctx, |_| Err(SubmitError::Busy));
        assert!(hydration.due.is_some() && hydration.in_flight.is_none());
        let (epoch, _, _) = request(&mut hydration, &ctx);
        let failed = LiveEvent::MessagesRefreshFailed {
            account_id: "11".into(),
            generation: epoch,
            thread: "d:22".into(),
        };
        assert!(!hydration.apply(&failed, "11", &mut model, &ctx));
        assert!(hydration.due.is_some() && hydration.in_flight.is_none());
        let (next, _, _) = request(&mut hydration, &ctx);
        assert_ne!(epoch, next);
        hydration.worker_closed();
        assert!(hydration.in_flight.is_none() && hydration.due.is_some());
        let (restarted, _, _) = request(&mut hydration, &ctx);
        assert_ne!(next, restarted);
        assert!(!hydration.apply(&event(next, vec![]), "11", &mut model, &ctx));
        assert!(hydration.in_flight.is_some());
        let before = model.messages.clone();
        assert!(!hydration.apply(&event(restarted, vec![]), "11", &mut model, &ctx));
        assert_eq!(model.messages, before);
        hydration.submit(&ctx, |_| {
            panic!("negative snapshot must not refetch every frame")
        });
    }
    #[test]
    fn same_generation_page_or_thread_switch_does_not_accept_old_snapshots() {
        let ctx = egui::Context::default();
        let mut model = model();
        let mut hydration = Hydration::default();
        hydration.sync_scope("11", &model, &ctx);
        let (epoch, _, _) = request(&mut hydration, &ctx);
        model.messages = vec![message("other-page")];
        let before = model.messages.clone();
        let mut enriched = message("old-1");
        enriched.media = Some(descriptor());
        assert!(!hydration.apply(
            &event(epoch, vec![enriched.clone()]),
            "11",
            &mut model,
            &ctx
        ));
        assert_eq!(model.messages, before);
        let (next, _, ids) = request(&mut hydration, &ctx);
        assert_eq!(ids, vec!["other-page"]);
        model.selected = Some("g:22".into());
        assert!(!hydration.apply(&event(next, vec![enriched]), "11", &mut model, &ctx));
        assert_eq!(model.messages, before);
        assert!(hydration.due.is_none());
    }
}
