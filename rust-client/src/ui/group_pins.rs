//! Scoped group pins. Verified snapshots drive the menu and local-history navigation.
use super::{group_members::Scope, live::LiveState, visuals, NativeApp};
use crate::{
    app::live::{
        GroupPinOutcome, GroupPinSnapshot, LiveCommand, LiveConnection, LiveEvent,
        LivePrivacyStatus,
    },
    domain::{Conversation, Message, MessageHit},
};
use eframe::egui::{self, vec2, RichText};
use std::{collections::HashSet, time::Instant};

#[derive(Default)]
pub(super) struct Pins {
    scope: Option<Scope>,
    serial: u64,
    attempted: bool,
    online: bool,
    pending: Option<(u64, Option<String>)>,
    snapshot: Option<GroupPinSnapshot>,
    recalled_during_request: HashSet<String>,
    preview_overflow: bool,
    notice: Option<(&'static str, Instant)>,
    #[cfg(test)]
    fixture: bool,
}

struct Change {
    id: String,
    pinned: bool,
}

fn supports(message: &Message) -> bool {
    message.thread_id.starts_with("g:")
        && !message.text.trim().is_empty()
        && message.quote.is_none()
        && message.reply_reference().is_some()
}

impl Pins {
    pub(super) fn set_scope(&mut self, scope: Option<Scope>) {
        if self.scope != scope {
            *self = Self {
                scope,
                serial: self.serial,
                ..Self::default()
            };
        }
    }

    fn set_online(&mut self, online: bool) {
        if self.online && !online {
            if let Some((_, Some(_))) = self.pending.take() {
                self.snapshot = None;
                self.notice("Chưa rõ kết quả ghim. Cập nhật danh sách để kiểm tra.");
            }
            self.attempted = false;
        }
        self.online = online;
    }

    fn notice(&mut self, text: &'static str) {
        self.notice = Some((text, Instant::now()));
    }

    fn start(&mut self, worker: Option<&crate::app::live::LiveWorker>, change: Option<Change>) {
        let Some(scope) = &self.scope else { return };
        let Some(worker) = worker.filter(|_| self.online && self.pending.is_none()) else {
            return;
        };
        self.serial = self.serial.wrapping_add(1);
        let request_id = self.serial;
        let thread = scope.thread.clone();
        let privacy_revision = scope.privacy_revision;
        let command = if let Some(change) = &change {
            LiveCommand::SetGroupPin {
                request_id,
                thread,
                privacy_revision,
                message_id: change.id.clone(),
                pinned: change.pinned,
            }
        } else {
            LiveCommand::GroupPins {
                request_id,
                thread,
                privacy_revision,
            }
        };
        self.attempted = true;
        if worker.submit(command).is_ok() {
            self.recalled_during_request.clear();
            self.preview_overflow = false;
            let mutation = change.is_some();
            self.pending = Some((request_id, change.map(|change| change.id)));
            self.notice = None;
            if mutation {
                self.notice("Đang cập nhật tin nhắn ghim…");
            }
        } else {
            self.notice("Chưa gửi được yêu cầu ghim. Hãy thử cập nhật danh sách.");
        }
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        let (account, request, thread, revision, outcome, snapshot) = match event {
            LiveEvent::GroupPins {
                account_id,
                request_id,
                thread,
                privacy_revision,
                snapshot,
            } => (
                account_id,
                request_id,
                thread,
                privacy_revision,
                None,
                snapshot,
            ),
            LiveEvent::GroupPinFinished {
                account_id,
                request_id,
                thread,
                privacy_revision,
                message_id,
                outcome,
                snapshot,
            } => {
                if !self
                    .pending
                    .as_ref()
                    .and_then(|(_, change)| change.as_ref())
                    .is_some_and(|id| id == message_id)
                {
                    return;
                }
                (
                    account_id,
                    request_id,
                    thread,
                    privacy_revision,
                    Some(outcome),
                    snapshot,
                )
            }
            _ => return,
        };
        if !self.scope.as_ref().is_some_and(|scope| {
            scope.account == *account
                && scope.thread == *thread
                && scope.privacy_revision == *revision
        }) || !self
            .pending
            .as_ref()
            .is_some_and(|(id, change)| id == request && change.is_some() == outcome.is_some())
        {
            return;
        }
        self.pending = None;
        self.snapshot.clone_from(snapshot);
        if let Some(snapshot) = &mut self.snapshot {
            for entry in &mut snapshot.entries {
                if self.preview_overflow || self.recalled_during_request.contains(&entry.message_id)
                {
                    entry.hit = None;
                }
            }
        }
        self.recalled_during_request.clear();
        self.preview_overflow = false;
        let notice = match outcome {
            Some(GroupPinOutcome::Accepted) if self.snapshot.is_some() => {
                "Đã cập nhật tin nhắn ghim."
            }
            Some(GroupPinOutcome::Accepted | GroupPinOutcome::Unknown) => {
                "Chưa rõ kết quả ghim. Cập nhật danh sách để kiểm tra."
            }
            Some(GroupPinOutcome::Rejected) => "Zalo từ chối thay đổi tin nhắn ghim.",
            Some(GroupPinOutcome::Unavailable) => {
                "Chưa thể thay đổi tin ghim. Kiểm tra kết nối và quyền nhóm."
            }
            None if self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.unresolved.is_empty()) =>
            {
                "Chưa xác nhận được thay đổi trước. Có thể cập nhật lại danh sách."
            }
            None if self.snapshot.is_none() => "Chưa tải được tin nhắn ghim. Hãy thử cập nhật.",
            None => {
                self.notice = None;
                return;
            }
        };
        self.notice(notice);
    }

    pub(super) fn remove_preview(&mut self, thread: &str, message: &str) {
        if self
            .scope
            .as_ref()
            .is_some_and(|scope| scope.thread == thread)
        {
            // An outstanding board snapshot may have read the original text before recall committed.
            if self.pending.is_some() && !self.preview_overflow {
                if self.recalled_during_request.len() < crate::domain::MAX_PAGE_SIZE {
                    self.recalled_during_request.insert(message.into());
                } else {
                    self.preview_overflow = true;
                }
            }
            if let Some(snapshot) = &mut self.snapshot {
                for entry in &mut snapshot.entries {
                    if entry.message_id == message {
                        entry.hit = None;
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope, snapshot: GroupPinSnapshot) {
        self.set_scope(Some(scope));
        self.online = true;
        self.attempted = true;
        self.snapshot = Some(snapshot);
        self.fixture = true;
    }

    #[cfg(test)]
    pub(super) fn fixture_entries(&self) -> &[crate::app::live::GroupPinEntry] {
        &self.snapshot.as_ref().unwrap().entries
    }
}

pub(super) struct MenuAction {
    pub label: &'static str,
    pub enabled: bool,
    pub reason: &'static str,
    pub pinned: bool,
}

impl NativeApp {
    pub(super) fn group_pin_scope(&self, state: &LiveState) -> Option<Scope> {
        let account = state.account.as_ref()?;
        let thread = self.model.selected.as_ref()?;
        (!self.demo
            && thread.starts_with("g:")
            && self
                .model
                .conversations
                .iter()
                .any(|item| item.id == *thread && !item.group_left)
            && state.privacy.status == LivePrivacyStatus::Ready
            && state.privacy.list_visible(thread))
        .then(|| Scope {
            account: account.id.clone(),
            auth: Some(state.login.generation()),
            thread: thread.clone(),
            generation: self.model.generation,
            privacy_revision: state.privacy.revision,
        })
    }

    pub(super) fn sync_group_pins(&mut self) {
        let Some(state) = &self.live else { return };
        let scope = self.group_pin_scope(state);
        let available = pins_available(state);
        let details = self.bound_conversation().is_some() && !self.modal_open();
        let Some(state) = &mut self.live else { return };
        let pins = &mut state.group_pins;
        pins.set_scope(scope);
        #[cfg(test)]
        let worker_available = state.worker.is_some() || pins.fixture;
        #[cfg(not(test))]
        let worker_available = state.worker.is_some();
        pins.set_online(
            available && worker_available && state.connection == LiveConnection::Online,
        );
        if (details || self.message_menu.is_some()) && !pins.attempted {
            pins.start(state.worker.as_ref(), None);
        }
    }

    pub(super) fn pin_menu_action(&self, message: &Message) -> Option<MenuAction> {
        let state = self.live.as_ref()?;
        let scope = self.group_pin_scope(state)?;
        if !supports(message)
            || scope.thread != message.thread_id
            || scope.account != message.account_id
        {
            return None;
        }
        let pins = &state.group_pins;
        let snapshot = pins
            .snapshot
            .as_ref()
            .filter(|_| pins.scope.as_ref() == Some(&scope));
        let already_pinned = snapshot.is_some_and(|snapshot| {
            snapshot
                .entries
                .iter()
                .any(|entry| entry.message_id == message.id)
        });
        let reason = if !pins_available(state) {
            "Phiên này chưa được Zalo bật tính năng tin nhắn ghim."
        } else if !pins.online {
            "Kết nối lại để thay đổi tin nhắn ghim."
        } else if pins.pending.is_some() || self.model.pending {
            "Đang kiểm tra trạng thái ghim."
        } else if snapshot.is_none()
            || snapshot.is_some_and(|snapshot| snapshot.unresolved.contains(&message.id))
        {
            "Cập nhật tin nhắn ghim trong thông tin hội thoại."
        } else if !snapshot.is_some_and(|snapshot| snapshot.can_mutate) {
            "Chưa đủ quyền hoặc điều kiện để thay đổi tin nhắn ghim."
        } else {
            ""
        };
        Some(MenuAction {
            label: if already_pinned {
                "Bỏ ghim tin nhắn"
            } else {
                "Ghim tin nhắn"
            },
            enabled: reason.is_empty(),
            reason,
            pinned: !already_pinned,
        })
    }

    pub(super) fn change_group_pin(&mut self, message: &str, pinned: bool) {
        let action = self
            .model
            .messages
            .iter()
            .find(|item| item.id == message)
            .and_then(|item| self.pin_menu_action(item));
        if !action.is_some_and(|action| action.enabled && action.pinned == pinned) {
            return;
        }
        if let Some(state) = &mut self.live {
            state.group_pins.start(
                state.worker.as_ref(),
                Some(Change {
                    id: message.into(),
                    pinned,
                }),
            );
        }
    }

    pub(super) fn group_pin_status(&self) -> Option<(&'static str, Instant)> {
        let state = self.live.as_ref()?;
        let pins = &state.group_pins;
        let notice = pins.notice.filter(|(_, at)| at.elapsed().as_secs() < 10)?;
        (pins.scope == self.group_pin_scope(state)).then_some(notice)
    }

    pub(super) fn group_pins_section(
        &mut self,
        ui: &mut egui::Ui,
        thread: &Conversation,
        compact: bool,
    ) {
        let Some(state) = &self.live else { return };
        let Some(scope) = self
            .group_pin_scope(state)
            .filter(|scope| scope.thread == thread.id)
        else {
            return;
        };
        let pins = &state.group_pins;
        super::information::section_title(ui, "Tin nhắn đã ghim");
        let colors = crate::theme::palette(ui.ctx());
        let mut selected = None::<MessageHit>;
        let available = pins_available(state);
        let refresh = available
            && ui
                .add_enabled(
                    pins.online && pins.pending.is_none(),
                    egui::Button::new("Cập nhật tin ghim").min_size(vec2(140.0, 44.0)),
                )
                .on_disabled_hover_text("Cần kết nối và chờ lượt cập nhật đang chạy.")
                .clicked();
        let ui_status = if !available {
            Some("Phiên này chưa được Zalo bật tính năng tin nhắn ghim.")
        } else if !pins.online && pins.snapshot.is_none() {
            Some("Ngoại tuyến · kết nối lại để tải danh sách tin ghim.")
        } else if !pins.online {
            Some("Ngoại tuyến · chỉ xem danh sách đã tải trong phiên này.")
        } else if pins.pending.is_some() {
            Some("Đang cập nhật tin nhắn ghim…")
        } else if let Some((notice, _)) = pins.notice {
            Some(notice)
        } else if pins
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.can_mutate)
        {
            Some("Chỉ xem · chưa đủ quyền hoặc điều kiện để thay đổi tin ghim.")
        } else {
            None
        };
        if let Some(status) = ui_status {
            ui.label(
                RichText::new(status)
                    .size(visuals::TEXT_SMALL)
                    .color(colors.text_secondary),
            );
        }
        if let Some(snapshot) = pins
            .snapshot
            .as_ref()
            .filter(|_| pins.scope.as_ref() == Some(&scope))
        {
            if snapshot.entries.is_empty() {
                ui.label(
                    RichText::new("Chưa có tin nhắn ghim được hỗ trợ.")
                        .color(colors.text_secondary),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt((
                    "group_pin_entries",
                    &scope.account,
                    &scope.thread,
                    scope.auth,
                ))
                .max_height(152.0)
                .show_rows(ui, 44.0, snapshot.entries.len(), |ui, range| {
                    for entry in &snapshot.entries[range] {
                        let hit = entry.hit.as_ref();
                        let caption = hit.map_or("Tin nhắn chưa có trên máy", |hit| {
                            if hit.snippet.is_empty() {
                                "Tin nhắn đã ghim"
                            } else {
                                &hit.snippet
                            }
                        });
                        let can_open = !self.model.pending
                            && hit.is_some_and(|hit| {
                                self.model.hit_loaded(hit) || state.worker.is_some()
                            });
                        let response = ui
                            .push_id(&entry.message_id, |ui| {
                                super::message_actions::action_row_height(
                                    ui,
                                    caption,
                                    "fa-outline-pin",
                                    can_open,
                                    false,
                                    44.0,
                                )
                            })
                            .inner;
                        if response.clicked() {
                            selected = hit.cloned();
                        }
                        response
                            .on_hover_text("Mở tin nhắn trong lịch sử trên máy")
                            .on_disabled_hover_text(if hit.is_some() {
                                "Lịch sử chưa sẵn sàng để mở tin nhắn."
                            } else {
                                "Tin nhắn đã ghim chưa có trong lịch sử trên máy."
                            });
                    }
                });
        }
        if refresh {
            if let Some(state) = &mut self.live {
                state.group_pins.start(state.worker.as_ref(), None);
            }
        }
        if let Some(hit) = selected {
            self.jump_to_hit(&thread.id, &hit, compact);
        }
    }
}

fn pins_available(state: &LiveState) -> bool {
    #[cfg(test)]
    if state.group_pins.fixture {
        return true;
    }
    state.group_pins_available()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::live::{GroupPinEntry, LiveWorker};

    fn scope(generation: u64) -> Scope {
        Scope {
            account: "11".into(),
            auth: Some(0),
            thread: "g:44".into(),
            generation,
            privacy_revision: 1,
        }
    }

    fn snapshot(pinned: bool, mutable: bool, unresolved: bool) -> GroupPinSnapshot {
        GroupPinSnapshot {
            can_mutate: mutable,
            entries: if pinned {
                vec![GroupPinEntry {
                    message_id: "901".into(),
                    hit: None,
                }]
            } else {
                vec![]
            },
            unresolved: if unresolved {
                vec!["901".into()]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn stale_snapshot_cannot_restore_a_pin_preview_recalled_during_the_read() {
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let mut pins = Pins::default();
        pins.set_scope(Some(scope(4)));
        pins.set_online(true);
        pins.start(Some(&worker), None);
        let Ok(LiveCommand::GroupPins {
            request_id,
            thread,
            privacy_revision,
        }) = commands.try_recv()
        else {
            panic!("expected a scoped read");
        };
        pins.remove_preview(&thread, "901");
        let mut stale = snapshot(true, true, false);
        stale.entries[0].hit = Some(MessageHit {
            message_id: "901".into(),
            sender: "11".into(),
            sent_at_ms: 1000,
            snippet: "Nội dung đã thu hồi".into(),
            highlight: (0, 0),
            jump: None,
        });
        pins.observe(&LiveEvent::GroupPins {
            account_id: "11".into(),
            request_id,
            thread,
            privacy_revision,
            snapshot: Some(stale),
        });
        assert_eq!(pins.snapshot, Some(snapshot(true, true, false)));
    }

    #[test]
    fn refresh_and_mutation_ignore_other_scopes_and_require_authoritative_state() {
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let mut pins = Pins::default();
        pins.set_scope(Some(scope(4)));
        pins.set_online(true);
        pins.start(Some(&worker), None);
        let Ok(LiveCommand::GroupPins {
            request_id,
            thread,
            privacy_revision,
        }) = commands.try_recv()
        else {
            panic!("expected a scoped read");
        };
        assert_eq!((thread.as_str(), privacy_revision), ("g:44", 1));
        let read = |revision| LiveEvent::GroupPins {
            account_id: "11".into(),
            request_id,
            thread: "g:44".into(),
            privacy_revision: revision,
            snapshot: Some(snapshot(false, true, false)),
        };
        pins.observe(&read(2));
        assert!(pins.snapshot.is_none());
        pins.observe(&read(1));
        assert_eq!(pins.snapshot, Some(snapshot(false, true, false)));
        pins.start(
            Some(&worker),
            Some(Change {
                id: "901".into(),
                pinned: true,
            }),
        );
        let Ok(LiveCommand::SetGroupPin {
            request_id,
            thread,
            privacy_revision,
            message_id,
            pinned,
        }) = commands.try_recv()
        else {
            panic!("expected a scoped mutation");
        };
        assert_eq!(
            (
                thread.as_str(),
                privacy_revision,
                message_id.as_str(),
                pinned
            ),
            ("g:44", 1, "901", true)
        );
        assert_eq!(
            pins.snapshot,
            Some(snapshot(false, true, false)),
            "dispatch is not proof of pin"
        );
        let result = |target: &str| LiveEvent::GroupPinFinished {
            account_id: "11".into(),
            request_id,
            thread: "g:44".into(),
            privacy_revision: 1,
            message_id: target.into(),
            outcome: GroupPinOutcome::Unknown,
            snapshot: Some(snapshot(false, true, true)),
        };
        pins.observe(&result("902"));
        assert!(pins.pending.is_some());
        pins.observe(&result("901"));
        assert_eq!(pins.snapshot, Some(snapshot(false, true, true)));
        assert!(pins.notice.unwrap().0.contains("Chưa rõ"));
        pins.set_scope(Some(scope(5)));
        pins.observe(&result("901"));
        assert!(pins.snapshot.is_none());
        assert!(pins.notice.is_none());
    }

    fn visible_group(ctx: &egui::Context) -> (NativeApp, Conversation, std::path::PathBuf) {
        let (mut app, root) = super::super::live::tests::fixture(ctx);
        let group = Conversation {
            id: "g:44".into(),
            title: "Nhóm kiểm thử".into(),
            last_message_at_ms: 1000,
            last_text: String::new(),
            last_sender: "22".into(),
            group_left: false,
        };
        app.model.selected = Some(group.id.clone());
        app.model.conversations.push(group.clone());
        app.model.messages.push(Message {
            mentions: Vec::new(),
            reply_blocked: false,
            expires_at_ms: None,
            recalled: false,
            source_client_id: Some("1901".into()),
            quote: None,
            account_id: "11".into(),
            thread_id: group.id.clone(),
            id: "901".into(),
            sender: "22".into(),
            text: "Lịch họp thứ Sáu".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1000,
        });
        app.drafts.select(&group.id).unwrap();
        app.drafts.active.text = "Bản nháp đang nhập".into();
        (app, group, root)
    }

    #[test]
    fn menu_distinguishes_permission_unsettled_and_verified_pin_states() {
        let ctx = egui::Context::default();
        let (mut app, _, root) = visible_group(&ctx);
        let state = app.live.as_ref().unwrap();
        let scope = app.group_pin_scope(state).unwrap();
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.worker = Some(worker);
        state
            .group_pins
            .fixture(scope, snapshot(true, false, false));
        let action = app.pin_menu_action(&app.model.messages[0]).unwrap();
        assert_eq!(action.label, "Bỏ ghim tin nhắn");
        assert!(!action.enabled);
        assert!(action.reason.contains("quyền"));
        app.live.as_mut().unwrap().group_pins.snapshot = Some(snapshot(true, true, true));
        let action = app.pin_menu_action(&app.model.messages[0]).unwrap();
        assert!(!action.enabled);
        assert!(action.reason.contains("Cập nhật"));
        app.change_group_pin("901", false);
        assert!(commands.try_recv().is_err());
        app.live.as_mut().unwrap().group_pins.snapshot = Some(snapshot(true, true, false));
        app.change_group_pin("901", false);
        app.change_group_pin("901", false);
        assert!(matches!(
            commands.try_recv(),
            Ok(LiveCommand::SetGroupPin { message_id, pinned: false, .. }) if message_id == "901"
        ));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.drafts.active.text, "Bản nháp đang nhập");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_arriving_snapshot_updates_the_open_menu_without_moving_the_other_actions() {
        let ctx = egui::Context::default();
        let (mut app, _, root) = visible_group(&ctx);
        let scope = app.group_pin_scope(app.live.as_ref().unwrap()).unwrap();
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.worker = Some(worker);
        state
            .group_pins
            .fixture(scope, snapshot(false, false, false));
        state.group_pins.snapshot = None;
        state.group_pins.start(state.worker.as_ref(), None);
        let Ok(LiveCommand::GroupPins {
            request_id,
            thread,
            privacy_revision,
        }) = commands.try_recv()
        else {
            panic!("expected a scoped read");
        };
        app.message_menu = Some(super::super::message_actions::Popup::new(
            app.message_menu_scope().unwrap(),
            "901".into(),
            egui::pos2(420.0, 190.0),
            None,
        ));
        let render = |app: &mut NativeApp| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.show_message_menu(ctx),
            )
        };
        let position = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected visible menu label {label:?}"))
        };
        let _ = render(&mut app);
        let waiting = render(&mut app);
        let pin = position(&waiting, "Ghim tin nhắn");
        let selection = position(&waiting, "Chọn nhiều tin nhắn");
        assert!(!app.pin_menu_action(&app.model.messages[0]).unwrap().enabled);
        app.live
            .as_mut()
            .unwrap()
            .group_pins
            .observe(&LiveEvent::GroupPins {
                account_id: "11".into(),
                request_id,
                thread,
                privacy_revision,
                snapshot: Some(snapshot(true, true, false)),
            });
        let ready = render(&mut app);
        assert_eq!(position(&ready, "Bỏ ghim tin nhắn"), pin);
        assert_eq!(position(&ready, "Chọn nhiều tin nhắn"), selection);
        assert!(app.pin_menu_action(&app.model.messages[0]).unwrap().enabled);
        assert!(app.message_menu.is_some());
        assert!(commands.try_recv().is_err());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clicking_a_local_pin_closes_compact_information_and_focuses_exact_message() {
        let ctx = egui::Context::default();
        let (mut app, group, root) = visible_group(&ctx);
        app.conversation_details = Some(("11".into(), group.id.clone()));
        let state = app.live.as_ref().unwrap();
        let scope = app.group_pin_scope(state).unwrap();
        let mut pin = snapshot(true, false, false);
        pin.entries[0].hit = Some(MessageHit {
            message_id: "901".into(),
            sender: "22".into(),
            sent_at_ms: 1000,
            snippet: "Lịch họp thứ Sáu".into(),
            highlight: (0, 0),
            jump: None,
        });
        app.live.as_mut().unwrap().group_pins.fixture(scope, pin);
        let mut render = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(640.0, 480.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.group_pins_section(ui, &group, true));
                },
            )
        };
        let _ = render(vec![]);
        let position = render(vec![])
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Lịch họp thứ Sáu" => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                }
                _ => None,
            })
            .expect("local pin row must be visible");
        for pressed in [true, false] {
            let _ = render(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(app.conversation_details.is_none());
        assert_eq!(app.model.focus().unwrap().message_id, "901");
        assert_eq!(app.drafts.active.text, "Bản nháp đang nhập");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
