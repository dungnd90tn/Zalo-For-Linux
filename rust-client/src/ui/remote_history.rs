//! Explicit import of older group history. The visible page stays on its local cursor.
use super::{live::LiveState, visuals, NativeApp};
use crate::{
    app::live::{
        LiveCommand, LiveConnection, LiveEvent, LivePrivacyStatus, LiveWorker, RemoteHistoryOutcome,
    },
    domain::{Cursor, Message},
};
use eframe::egui::{self, vec2, RichText};

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    account: String,
    auth: u64,
    thread: String,
    privacy_revision: u64,
}

struct Request {
    id: u64,
    generation: u64,
    anchor: Cursor,
}

#[derive(Default)]
struct View {
    pending: Option<Request>,
    outcome: Option<(u64, RemoteHistoryOutcome)>,
    exhausted: bool,
}

#[derive(Default)]
pub(super) struct RemoteHistory {
    available: bool,
    serial: u64,
    scope: Option<Scope>,
    view: View,
}

impl RemoteHistory {
    fn scope(&mut self, scope: Option<Scope>) {
        if self.scope != scope {
            self.scope = scope;
            self.view = View::default();
        }
    }

    pub(super) fn clear_view(&mut self) {
        self.scope(None);
    }

    pub(super) fn worker_closed(&mut self) {
        self.available = false;
        self.clear_view();
    }

    fn start(&mut self, worker: &LiveWorker, generation: u64, anchor: Cursor) {
        let Some(scope) = &self.scope else { return };
        if self.view.pending.is_some() || self.view.exhausted {
            return;
        }
        self.serial = self.serial.wrapping_add(1).max(1);
        let request_id = self.serial;
        let command = LiveCommand::LoadRemoteHistory {
            request_id,
            generation,
            thread: scope.thread.clone(),
            anchor: anchor.clone(),
            privacy_revision: scope.privacy_revision,
        };
        self.view.outcome = None;
        if worker.submit(command).is_ok() {
            self.view.pending = Some(Request {
                id: request_id,
                generation,
                anchor,
            });
        } else {
            self.view.outcome = Some((generation, RemoteHistoryOutcome::Failed));
        }
    }

    fn finish(
        &mut self,
        event: &LiveEvent,
        current: Option<&Scope>,
        generation: u64,
        anchor: Option<&Cursor>,
    ) -> bool {
        let LiveEvent::RemoteHistoryFinished {
            account_id,
            request_id,
            generation: completed_page,
            thread,
            privacy_revision,
            outcome,
        } = event
        else {
            return false;
        };
        if !self.scope.as_ref().is_some_and(|scope| {
            scope.account == *account_id
                && scope.thread == *thread
                && scope.privacy_revision == *privacy_revision
        }) || !self.view.pending.as_ref().is_some_and(|pending| {
            pending.id == *request_id && pending.generation == *completed_page
        }) {
            return false;
        }
        let pending = self
            .view
            .pending
            .take()
            .expect("confirmed remote history request");
        if self.scope.as_ref() != current
            || pending.generation != generation
            || anchor != Some(&pending.anchor)
        {
            return false;
        }
        self.view.exhausted = matches!(
            outcome,
            RemoteHistoryOutcome::Exhausted
                | RemoteHistoryOutcome::Imported {
                    has_more: false,
                    ..
                }
        );
        self.view.outcome = Some((generation, *outcome));
        matches!(outcome, RemoteHistoryOutcome::Imported { count, .. } if *count > 0)
    }
}

impl NativeApp {
    fn remote_history_scope(&self, state: &LiveState) -> Option<Scope> {
        if !state.remote_history.available
            || self.demo
            || state.privacy.status != LivePrivacyStatus::Ready
        {
            return None;
        }
        let account = state.account.as_ref()?;
        let thread = self.model.selected.as_ref()?;
        let joined = thread
            .strip_prefix("g:")
            .is_some_and(|id| crate::protocol::messaging::numeric_id(id).is_ok())
            && self
                .model
                .conversations
                .iter()
                .any(|group| group.id == *thread && !group.group_left);
        (joined && state.privacy.list_visible(thread)).then(|| Scope {
            account: account.id.clone(),
            auth: state.login.generation(),
            thread: thread.clone(),
            privacy_revision: state.privacy.revision,
        })
    }

    fn remote_history_anchor(&self, scope: &Scope) -> Option<Cursor> {
        self.model
            .messages
            .first()
            .filter(|message| {
                message.account_id == scope.account
                    && message.thread_id == scope.thread
                    && message.sent_at_ms > 0
                    && crate::protocol::messaging::numeric_id(&message.id).is_ok()
            })
            .map(Message::cursor)
    }

    pub(super) fn sync_remote_history(&mut self) {
        let scope = self
            .live
            .as_ref()
            .and_then(|state| self.remote_history_scope(state));
        if let Some(state) = &mut self.live {
            state.remote_history.scope(scope);
        }
    }

    pub(super) fn apply_remote_history_event(
        &mut self,
        state: &mut LiveState,
        event: &LiveEvent,
    ) -> bool {
        match event {
            LiveEvent::RemoteHistoryCapability { available, .. } => {
                state.remote_history.available = *available;
                if !available {
                    state.remote_history.clear_view();
                }
                true
            }
            LiveEvent::RemoteHistoryFinished { .. } => {
                let scope = self.remote_history_scope(state);
                let anchor = scope
                    .as_ref()
                    .and_then(|scope| self.remote_history_anchor(scope));
                if state.remote_history.finish(
                    event,
                    scope.as_ref(),
                    self.model.generation,
                    anchor.as_ref(),
                ) {
                    self.model.has_older = true;
                    self.model.overview = Default::default();
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn remote_history_visible(&self) -> bool {
        let Some(state) = &self.live else {
            return false;
        };
        let Some(scope) = self.remote_history_scope(state) else {
            return false;
        };
        self.remote_history_anchor(&scope).is_some()
            && (!self.model.has_older
                || state
                    .remote_history
                    .view
                    .outcome
                    .is_some_and(|(page, _)| page == self.model.generation))
    }

    pub(super) fn remote_history_controls(&mut self, ui: &mut egui::Ui) {
        let Some(state) = &self.live else { return };
        let Some(scope) = self.remote_history_scope(state) else {
            return;
        };
        let Some(anchor) = self.remote_history_anchor(&scope) else {
            return;
        };
        let history = &state.remote_history;
        let view = &history.view;
        let online = state.connection == LiveConnection::Online && state.worker.is_some();
        let waiting = view.pending.is_some();
        let exhausted = view.exhausted;
        let outcome = view
            .outcome
            .filter(|(page, _)| *page == self.model.generation)
            .map(|(_, outcome)| outcome);
        let mut clicked = false;
        if !self.model.has_older && !exhausted {
            let label = if waiting {
                "Đang tải tin cũ…"
            } else if matches!(
                outcome,
                Some(RemoteHistoryOutcome::Imported {
                    count: 0,
                    has_more: true
                })
            ) {
                "Tải tiếp từ Zalo"
            } else {
                "Tải tin cũ từ Zalo"
            };
            let ready = online && !waiting && !self.model.pending && ui.is_enabled();
            let response =
                ui.add_enabled(ready, egui::Button::new(label).min_size(vec2(140.0, 44.0)));
            visuals::focus_outline(ui, &response, response.rect, 12.0);
            clicked = response.clicked();
            response.on_disabled_hover_text(if !online {
                "Kết nối lại để tải lịch sử từ Zalo."
            } else {
                "Chờ lượt tải lịch sử hiện tại."
            });
        }
        let status = match outcome {
            Some(RemoteHistoryOutcome::Imported { count, .. }) if count > 0 => Some(format!(
                "Đã lưu thêm {count} tin. Chọn Xem trang trước để đọc."
            )),
            Some(RemoteHistoryOutcome::Imported { has_more: true, .. }) => {
                Some("Trang này chưa bổ sung tin. Có thể tải tiếp.".into())
            }
            Some(RemoteHistoryOutcome::Unavailable) => {
                Some("Chưa đủ điều kiện tải lịch sử từ Zalo.".into())
            }
            Some(RemoteHistoryOutcome::Failed) => {
                Some("Tải tin cũ chưa thành công. Có thể thử lại.".into())
            }
            _ if exhausted => Some("Zalo không trả thêm tin cũ.".into()),
            _ if !online && !self.model.has_older => {
                Some("Ngoại tuyến · có thể đọc lịch sử đã lưu.".into())
            }
            _ => None,
        };
        if let Some(status) = status {
            let colors = crate::theme::palette(ui.ctx());
            ui.add(
                egui::Label::new(
                    RichText::new(&status)
                        .size(visuals::TEXT_SMALL)
                        .color(colors.text_secondary),
                )
                .wrap(),
            )
            .on_hover_text(status);
        }
        if clicked {
            if let Some(state) = &mut self.live {
                state.remote_history.scope(Some(scope));
                if let Some(worker) = &state.worker {
                    state
                        .remote_history
                        .start(worker, self.model.generation, anchor);
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn fixture_remote_history(&mut self, saved: bool) {
        self.live.as_mut().unwrap().remote_history.available = true;
        self.sync_remote_history();
        if saved {
            self.model.has_older = true;
            self.live.as_mut().unwrap().remote_history.view.outcome = Some((
                self.model.generation,
                RemoteHistoryOutcome::Imported {
                    count: 3,
                    has_more: true,
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Conversation;

    fn fixture(ctx: &egui::Context) -> (NativeApp, std::path::PathBuf) {
        let (mut app, root) = super::super::live::tests::fixture(ctx);
        app.model.generation = 8;
        app.model.selected = Some("g:44".into());
        app.model.conversations.push(Conversation {
            id: "g:44".into(),
            title: "Nhóm kiểm thử".into(),
            last_message_at_ms: 100,
            last_text: "Tin đã lưu".into(),
            last_sender: "22".into(),
            group_left: false,
        });
        app.model.messages.push(Message {
            account_id: "11".into(),
            thread_id: "g:44".into(),
            id: "901".into(),
            sender: "22".into(),
            source_client_id: Some("1901".into()),
            quote: None,
            mentions: Vec::new(),
            reply_blocked: false,
            expires_at_ms: None,
            recalled: false,
            sticker: None,
            text: "Tin đã lưu".into(),
            sent_at_ms: 100,
            media: None,
        });
        app.drafts.select("g:44").unwrap();
        app.drafts.active.text = "Nháp đang nhập".into();
        app.fixture_remote_history(false);
        (app, root)
    }

    #[test]
    fn committed_history_only_enables_local_paging_after_matching_the_current_anchor() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let mut state = app.live.take().unwrap();
        let original = app.model.messages[0].cursor();
        state
            .remote_history
            .start(&worker, app.model.generation, original.clone());
        let Ok(LiveCommand::LoadRemoteHistory {
            request_id,
            generation,
            thread,
            anchor,
            privacy_revision,
        }) = commands.try_recv()
        else {
            panic!("expected the scoped manual import");
        };
        assert_eq!(
            (generation, thread.as_str(), anchor),
            (8, "g:44", original.clone())
        );
        let settled = |account: &str, request_id| LiveEvent::RemoteHistoryFinished {
            account_id: account.into(),
            request_id,
            generation,
            thread: thread.clone(),
            privacy_revision,
            outcome: RemoteHistoryOutcome::Imported {
                count: 3,
                has_more: true,
            },
        };
        app.apply_live_event(&mut state, settled("12", request_id), &ctx);
        app.apply_live_event(&mut state, settled("11", request_id + 1), &ctx);
        assert!(!app.model.has_older);
        app.apply_live_event(&mut state, settled("11", request_id), &ctx);
        assert!(app.model.has_older);
        assert_eq!(app.model.generation, 8);
        assert_eq!(app.model.messages.len(), 1);
        assert_eq!(app.model.messages[0].cursor(), original);
        assert_eq!(app.drafts.active.text, "Nháp đang nhập");

        app.model.has_older = false;
        state
            .remote_history
            .start(&worker, app.model.generation, original);
        assert!(commands.try_recv().is_ok());
        app.model.messages[0].id = "902".into();
        app.apply_live_event(&mut state, settled("11", request_id + 1), &ctx);
        assert!(!app.model.has_older);
        assert!(state.remote_history.view.pending.is_none());
        app.live = Some(state);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remote_action_requires_joined_group_capability_and_local_anchor() {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        assert!(app.remote_history_visible());
        app.model.conversations[0].group_left = true;
        assert!(!app.remote_history_visible());
        app.model.conversations[0].group_left = false;
        app.model.selected = Some("d:44".into());
        assert!(!app.remote_history_visible());
        app.model.selected = Some("g:44".into());
        app.model.messages.clear();
        assert!(!app.remote_history_visible());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
