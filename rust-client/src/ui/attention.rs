//! Scoped presentation of persisted unread state and source-confirmed receipts.
//! Visibility proof is collected during painting and consumed on a later focused frame.
use super::NativeApp;
#[cfg(feature = "messaging")]
use crate::app::live::{LiveCommand, LiveEvent};
#[cfg(feature = "messaging")]
use crate::domain::read_state::{DeliveryState, MessageDelivery, NotificationPreferences};
use eframe::egui;
#[cfg(any(test, feature = "messaging"))]
use std::collections::BTreeSet;
#[cfg(feature = "messaging")]
use std::{collections::BTreeMap, time::Duration};

#[cfg(feature = "messaging")]
const RETRY_DELAY: Duration = Duration::from_millis(100);

#[cfg(any(test, feature = "messaging"))]
const MAX_VISIBLE: usize = 50;

#[cfg(any(test, feature = "messaging"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    pub account: String,
    pub auth: Option<u64>,
    pub thread: String,
    pub generation: u64,
}
#[cfg(any(test, feature = "messaging"))]
#[derive(Default)]
pub(super) struct Presented {
    scope: Option<Scope>,
    previous: BTreeSet<String>,
    painted: BTreeSet<String>,
    acknowledged: BTreeSet<String>,
    eligible: bool,
}
#[cfg(any(test, feature = "messaging"))]
impl Presented {
    fn begin(&mut self, scope: Option<Scope>, eligible: bool) -> Vec<String> {
        if self.scope != scope || !eligible || !self.eligible {
            self.previous.clear();
            self.painted.clear();
            self.acknowledged.clear();
        }
        self.scope = scope;
        self.eligible = eligible;
        self.previous = std::mem::take(&mut self.painted);
        self.previous
            .difference(&self.acknowledged)
            .take(MAX_VISIBLE)
            .cloned()
            .collect()
    }
    pub fn paint(&mut self, id: &str) {
        if self.eligible && self.painted.len() < MAX_VISIBLE {
            self.painted.insert(id.to_owned());
        }
    }
    fn submitted(&mut self, ids: &[String]) {
        self.acknowledged.extend(ids.iter().cloned());
        // The pane is bounded to 50 messages. Retain only its current proof keys.
        self.acknowledged.retain(|id| self.previous.contains(id));
    }
}

#[cfg(feature = "messaging")]
#[derive(PartialEq, Eq)]
struct Snapshot {
    threads: Vec<String>,
    scope: Option<Scope>,
    outgoing: Vec<String>,
}

#[cfg(feature = "messaging")]
#[derive(Default)]
pub(super) struct Attention {
    unread: BTreeMap<String, u64>,
    delivery: BTreeMap<String, MessageDelivery>,
    unread_only: bool,
    pub settings_open: bool,
    presented: Presented,
    tail_visible: bool,
    preferences: Option<NotificationPreferences>,
    preferences_pending: bool,
    preferences_error: bool,
    candidate: Option<(Scope, Vec<String>)>,
    snapshot: Option<Snapshot>,
    request: u64,
    dirty: bool,
    foreground: Option<String>,
    activation: Option<String>,
    account: Option<(String, Option<u64>)>,
}

#[cfg(feature = "messaging")]
impl Attention {
    pub fn scope(&mut self, account: Option<(String, Option<u64>)>) {
        if self.account != account {
            *self = Self {
                account,
                ..Default::default()
            };
        }
    }

    pub(super) fn paint(
        &mut self,
        message: &crate::domain::Message,
        rect: egui::Rect,
        clip: egui::Rect,
        is_tail: bool,
    ) {
        let presented = &mut self.presented;
        let Some(scope) = &presented.scope else {
            return;
        };
        let intersection = rect.intersect(clip);
        if presented.eligible
            && message.account_id == scope.account
            && message.thread_id == scope.thread
            && intersection.width() > 0.0
            && intersection.height() >= rect.height().min(24.0)
        {
            self.tail_visible |= is_tail;
            if !message.is_recalled()
                && message.sender != scope.account
                && !message.id.is_empty()
                && message.id.bytes().all(|byte| byte.is_ascii_digit())
            {
                presented.paint(&message.id);
            }
        }
    }

    #[cfg(test)]
    pub fn fixture(&mut self, model: &crate::app::Model, auth: u64, scene: &str) {
        let scope = Scope {
            account: "11".into(),
            auth: Some(auth),
            thread: "d:22".into(),
            generation: model.generation,
        };
        self.scope(Some((scope.account.clone(), scope.auth)));
        self.unread.insert("d:33".into(), 3);
        self.unread_only = scene == "filter";
        self.settings_open = scene == "settings";
        self.preferences = Some(NotificationPreferences::default());
        self.snapshot = Some(Snapshot {
            threads: vec!["d:22".into(), "d:33".into()],
            scope: Some(scope),
            outgoing: vec!["902".into()],
        });
        self.delivery.insert(
            "902".into(),
            MessageDelivery {
                message_id: "902".into(),
                state: DeliveryState::Seen,
                seen_count: 1,
            },
        );
    }

    pub fn observe(&mut self, event: &LiveEvent, model: &crate::app::Model) {
        match event {
            LiveEvent::NotificationOpen { thread, .. } => self.activation = Some(thread.clone()),
            LiveEvent::ReadStateChanged { .. } | LiveEvent::Recalled { .. } => self.dirty = true,
            LiveEvent::ReadState {
                generation,
                unread,
                thread,
                delivery,
                ..
            } if *generation == self.request => {
                self.unread = unread
                    .iter()
                    .map(|row| (row.thread_id.clone(), row.unread))
                    .collect();
                self.delivery.clear();
                if let Some(scope) = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.scope.as_ref())
                {
                    if thread.as_deref() == Some(scope.thread.as_str())
                        && model.selected.as_deref() == Some(scope.thread.as_str())
                        && model.generation == scope.generation
                    {
                        self.delivery.extend(
                            delivery
                                .iter()
                                .map(|row| (row.message_id.clone(), row.clone())),
                        );
                    }
                }
            }
            LiveEvent::NotificationPreferences { preferences, .. } => {
                if let Some(preferences) = preferences {
                    self.preferences = Some(*preferences);
                }
                self.preferences_pending = false;
                self.preferences_error = preferences.is_none();
            }
            _ => {}
        }
    }
}
impl NativeApp {
    pub(super) fn unread_count(&self, thread: &str) -> u64 {
        #[cfg(feature = "messaging")]
        if let Some(state) = &self.live {
            return state.attention.unread.get(thread).copied().unwrap_or(0);
        }
        let _ = thread;
        0
    }
    pub(super) fn unread_filter(&self) -> bool {
        #[cfg(feature = "messaging")]
        if let Some(state) = &self.live {
            return state.attention.unread_only;
        }
        false
    }
    pub(super) fn set_unread_filter(&mut self, unread: bool) {
        #[cfg(feature = "messaging")]
        if let Some(state) = &mut self.live {
            state.attention.unread_only = unread;
        }
        let _ = unread;
    }
}
#[cfg(feature = "messaging")]
impl NativeApp {
    fn attention_scope(&self) -> Option<Scope> {
        let state = self.live.as_ref()?;
        let account = state.account.as_ref()?;
        let thread = self.model.selected.as_ref()?;
        state.privacy.list_visible(thread).then(|| Scope {
            account: account.id.clone(),
            auth: self.auth_generation(),
            thread: thread.clone(),
            generation: self.model.generation,
        })
    }

    fn read_eligible(&self, ctx: &egui::Context) -> bool {
        !self.demo
            && !self.selected_group_left()
            && !self.modal_open()
            && !self.modal_blocked
            && !self.directory_visible()
            && !(ctx.screen_rect().width() < super::COMPACT_BREAKPOINT && self.compact_list)
            && !self.model.pending
            && ctx.input(|input| input.focused && input.viewport().minimized != Some(true))
    }

    pub(super) fn begin_attention_frame(&mut self, ctx: &egui::Context) {
        if !self.modal_open() && !self.modal_blocked {
            let activation = self
                .live
                .as_mut()
                .and_then(|state| state.attention.activation.take());
            if let Some(thread) = activation.filter(|thread| self.privacy_list_visible(thread)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                if self.load_latest(thread) {
                    self.compact_list = false;
                    if let Some(state) = &mut self.live {
                        state.directory.visible = false;
                    }
                }
            }
        }
        let scope = self.attention_scope();
        let eligible = scope.is_some() && self.read_eligible(ctx);
        let account = self
            .account_id()
            .map(|id| (id.to_owned(), self.auth_generation()));
        if let Some(state) = &mut self.live {
            let attention = &mut state.attention;
            attention.scope(account);
            attention.tail_visible = false;
            let ids = attention.presented.begin(scope.clone(), eligible);
            attention.candidate = scope.filter(|_| !ids.is_empty()).map(|scope| (scope, ids));
        }
    }

    pub(super) fn finish_attention_frame(&mut self, ctx: &egui::Context) {
        let scope = self.attention_scope();
        let eligible = scope.is_some() && self.read_eligible(ctx);
        let Some(state) = &mut self.live else { return };
        let Some(worker) = &state.worker else { return };
        let attention = &mut state.attention;
        let foreground = scope
            .as_ref()
            .filter(|_| {
                eligible
                    && self.latest_page
                    && attention.tail_visible
                    && attention.presented.scope == scope
            })
            .map(|scope| scope.thread.clone());

        // Recheck after input handling. Selecting a different thread, paging or
        // opening a dialog during this frame invalidates its earlier candidate.
        if !eligible || attention.presented.scope != scope {
            let _ = attention.presented.begin(scope.clone(), false);
            attention.candidate = None;
        }
        let mut retry = false;
        if attention.foreground != foreground {
            match worker.submit(LiveCommand::NotificationForeground {
                thread: foreground.clone(),
            }) {
                Ok(()) => attention.foreground = foreground,
                Err(crate::app::SubmitError::Busy) => retry = true,
                Err(_) => {}
            }
        }
        if let Some((candidate, ids)) = attention.candidate.take() {
            match worker.submit(LiveCommand::ReadVisible {
                generation: candidate.generation,
                thread: candidate.thread,
                message_ids: ids.clone(),
            }) {
                Ok(()) => attention.presented.submitted(&ids),
                Err(crate::app::SubmitError::Busy) => retry = true,
                Err(_) => {}
            }
        }
        if eligible
            && !attention
                .presented
                .painted
                .is_subset(&attention.presented.acknowledged)
        {
            // Wait until the subsequent frame before submitting any new IDs.
            ctx.request_repaint_after(RETRY_DELAY);
        }
        let threads = self
            .model
            .conversations
            .iter()
            .filter(|thread| state.privacy.list_visible(&thread.id))
            .map(|thread| thread.id.as_str())
            .take(200);
        let outgoing = self
            .model
            .messages
            .iter()
            .filter(|message| {
                scope.as_ref().is_some_and(|scope| {
                    message.account_id == scope.account
                        && message.sender == scope.account
                        && message.thread_id == scope.thread
                })
            })
            .map(|message| message.id.as_str())
            .take(MAX_VISIBLE);
        let current = attention.snapshot.as_ref().is_some_and(|old| {
            old.scope == scope
                && old.threads.iter().map(String::as_str).eq(threads.clone())
                && old.outgoing.iter().map(String::as_str).eq(outgoing.clone())
        });
        if attention.dirty || !current {
            let snapshot = Snapshot {
                threads: threads.map(str::to_owned).collect(),
                outgoing: outgoing.map(str::to_owned).collect(),
                scope: scope.clone(),
            };
            let generation = attention.request.wrapping_add(1);
            let thread = snapshot.scope.as_ref().map(|scope| scope.thread.clone());
            match worker.submit(LiveCommand::ReadState {
                generation,
                threads: snapshot.threads.clone(),
                thread,
                message_ids: snapshot.outgoing.clone(),
            }) {
                Ok(()) => {
                    if attention
                        .snapshot
                        .as_ref()
                        .and_then(|old| old.scope.as_ref())
                        != snapshot.scope.as_ref()
                    {
                        attention.delivery.clear();
                    }
                    attention.snapshot = Some(snapshot);
                    attention.request = generation;
                    attention.dirty = false;
                }
                Err(crate::app::SubmitError::Busy) => retry = true,
                Err(_) => {}
            }
        }
        if retry {
            ctx.request_repaint_after(RETRY_DELAY);
        }
    }

    pub(super) fn delivery_label(&self, message: &crate::domain::Message) -> Option<&'static str> {
        let attention = &self.live.as_ref()?.attention;
        let scope = attention.snapshot.as_ref()?.scope.as_ref()?;
        if message.account_id != scope.account
            || message.thread_id != scope.thread
            || self.model.generation != scope.generation
            || !self.privacy_list_visible(&scope.thread)
        {
            return None;
        }
        let delivery = attention.delivery.get(&message.id)?;
        Some(match delivery.state {
            DeliveryState::Accepted => "Zalo đã nhận",
            DeliveryState::Delivered => "Đã chuyển",
            DeliveryState::Seen if message.thread_id.starts_with("g:") => "Có người đã xem",
            DeliveryState::Seen => "Đã xem",
        })
    }

    pub(super) fn show_notification_settings(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.live else { return };
        let attention = &mut state.attention;
        if !attention.settings_open {
            return;
        }
        let close = super::dialog::show(
            ctx,
            "notification_settings",
            "Thông báo",
            420.0,
            true,
            |ui| {
                super::dialog::body(ui, |ui| {
                    let Some(mut prefs) = attention.preferences else {
                        ui.label(if attention.preferences_error {
                            "Không tải được cài đặt thông báo."
                        } else {
                            "Đang tải cài đặt thông báo…"
                        });
                        return;
                    };
                    let enabled = !attention.preferences_pending && state.worker.is_some();
                    let changed = ui
                        .add_enabled(
                            enabled,
                            egui::Checkbox::new(&mut prefs.enabled, "Thông báo trên máy tính"),
                        )
                        .changed();
                    let preview_changed = ui
                        .add_enabled(
                            enabled && prefs.enabled,
                            egui::Checkbox::new(&mut prefs.preview, "Xem trước nội dung tin nhắn"),
                        )
                        .changed();
                    if changed || preview_changed {
                        let command =
                            LiveCommand::SetNotificationPreferences { preferences: prefs };
                        attention.preferences_pending = state
                            .worker
                            .as_ref()
                            .is_some_and(|worker| worker.submit(command).is_ok());
                        attention.preferences_error = !attention.preferences_pending;
                    }
                    if attention.preferences_error {
                        ui.colored_label(
                            crate::theme::palette(ctx).error,
                            "Chưa lưu được cài đặt. Hãy thử lại.",
                        );
                    } else {
                        ui.label("Khi tắt xem trước, thông báo chỉ báo có tin nhắn mới.");
                    }
                });
            },
        );
        if close.closed {
            attention.settings_open = false;
        }
    }
}

pub(super) fn badge(ui: &egui::Ui, row: egui::Rect, count: u64) {
    if count == 0 {
        return;
    }
    let colors = crate::theme::palette(ui.ctx());
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let font = egui::FontId::proportional(super::visuals::TEXT_SMALL);
    let galley = ui.painter().layout_no_wrap(text, font, colors.on_color);
    let rect = egui::Rect::from_center_size(
        row.right_bottom() - egui::vec2(34.0, 22.0),
        egui::vec2((galley.size().x + 12.0).max(22.0), 22.0),
    );
    ui.painter().rect_filled(rect, 11.0, colors.primary);
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, colors.on_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(thread: &str) -> Option<Scope> {
        Some(Scope {
            account: "11".into(),
            auth: Some(1),
            thread: thread.into(),
            generation: 1,
        })
    }
    #[test]
    fn read_requires_previous_painted_frame_and_current_focus_scope() {
        let mut proof = Presented::default();
        assert!(proof.begin(scope("d:22"), true).is_empty());
        proof.paint("900");
        let ids = proof.begin(scope("d:22"), true);
        assert_eq!(ids, vec!["900"]);
        proof.submitted(&ids);
        proof.paint("900");
        assert!(proof.begin(scope("d:22"), true).is_empty());
        proof.paint("901");
        assert!(proof.begin(scope("d:33"), true).is_empty());
        proof.paint("902");
        assert!(proof.begin(scope("d:33"), false).is_empty());
        proof.paint("903");
        assert!(proof.begin(scope("d:33"), true).is_empty());
    }
}
