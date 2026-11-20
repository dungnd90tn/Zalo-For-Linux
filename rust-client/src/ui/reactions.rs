//! Scoped reaction controls and read-only counts from the live worker.
use super::{message_actions::Scope, visuals, InteractionNotice, NativeApp};
use crate::{
    app::{
        live::{
            LiveCommand, LiveConnection, LiveEvent, ReactionAction, ReactionKind, ReactionOutcome,
            ReactionSummary,
        },
        SubmitError,
    },
    domain::Message,
};
use eframe::egui::{self, pos2, vec2, FontId, Rect, Sense, Stroke};
use std::{collections::BTreeMap, time::Duration};

const PRESETS: [(ReactionKind, &str, &[u8]); 6] = [
    (
        ReactionKind::Heart,
        "Thả tim",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-30-column-43.png"),
    ),
    (
        ReactionKind::Like,
        "Thích",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-34-column-43.png"),
    ),
    (
        ReactionKind::Laugh,
        "Cười",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-4-column-42.png"),
    ),
    (
        ReactionKind::Wow,
        "Bất ngờ",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-9-column-43.png"),
    ),
    (
        ReactionKind::Sad,
        "Buồn",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-2-column-43.png"),
    ),
    (
        ReactionKind::Angry,
        "Giận",
        include_bytes!("../../../pc-dist/emoji/emoji-32/row-3-column-43.png"),
    ),
];

pub(super) struct Icons([egui::TextureHandle; PRESETS.len()]);

impl Icons {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        // Six fixed 32px assets embedded by the build. Decode once during window creation.
        Self(PRESETS.map(|(_, label, png)| {
            let pixels = image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .expect("embedded reaction asset must decode")
                .into_rgba8();
            let size = [pixels.width() as usize, pixels.height() as usize];
            ctx.load_texture(
                label,
                egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw()),
                egui::TextureOptions::LINEAR,
            )
        }))
    }
}

struct Snapshot {
    scope: Scope,
    ids: Vec<String>,
    request: u64,
}

#[derive(Default)]
pub(super) struct Reactions {
    snapshot: Option<Snapshot>,
    summaries: BTreeMap<String, Vec<ReactionSummary>>,
    query_id: u64,
    dirty: bool,
    request_id: u64,
    pending: Option<(String, String, u64)>,
    status: Option<InteractionNotice>,
    available: bool,
    #[cfg(test)]
    pub(super) fixture: bool,
}

impl Reactions {
    pub(super) fn reset_view(&mut self) {
        self.snapshot = None;
        self.summaries.clear();
    }

    pub(super) fn worker_closed(&mut self) {
        self.available = false;
        self.reset_view();
        if let Some((thread, _, _)) = self.pending.take() {
            self.status = Some(InteractionNotice::new(
                thread,
                "Chưa rõ Zalo đã nhận cảm xúc.",
            ));
        }
    }

    pub(super) fn observe_outcome(&mut self, event: &LiveEvent) {
        if let LiveEvent::ReactionCapability { available, .. } = event {
            self.available = *available;
            return;
        }
        let LiveEvent::ReactionFinished {
            request_id,
            thread,
            message_id,
            outcome,
            ..
        } = event
        else {
            return;
        };
        if !self
            .pending
            .as_ref()
            .is_some_and(|(expected_thread, expected_message, expected_id)| {
                expected_thread == thread
                    && expected_message == message_id
                    && expected_id == request_id
            })
        {
            return;
        }
        self.pending = None;
        self.status = Some(InteractionNotice::new(
            thread.clone(),
            match outcome {
                ReactionOutcome::Accepted => "Zalo đã nhận cảm xúc.",
                ReactionOutcome::Rejected => "Zalo từ chối thao tác cảm xúc.",
                ReactionOutcome::Unavailable => "Chưa thể dùng cảm xúc cho tin nhắn này.",
                ReactionOutcome::Unknown => "Chưa rõ Zalo đã nhận cảm xúc.",
            },
        ));
        if let Some(snapshot) = &self.snapshot {
            self.dirty |= snapshot.scope.thread == *thread;
        }
    }

    pub(super) fn observe_snapshot(&mut self, event: &LiveEvent, current: Option<&Scope>) {
        let Some(snapshot) = self
            .snapshot
            .as_ref()
            .filter(|snapshot| Some(&snapshot.scope) == current)
        else {
            return;
        };
        match event {
            LiveEvent::ReactionState {
                generation,
                thread,
                messages,
                ..
            } if *generation == snapshot.request
                && *thread == snapshot.scope.thread
                && !self.dirty =>
            {
                self.summaries = messages
                    .iter()
                    .filter(|(id, _)| snapshot.ids.contains(id))
                    .cloned()
                    .collect();
            }
            LiveEvent::ReactionsChanged { thread, .. } | LiveEvent::Recalled { thread, .. }
                if *thread == snapshot.scope.thread =>
            {
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn at(&self, scope: &Scope, message_id: &str) -> &[ReactionSummary] {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.scope == *scope)
        {
            self.summaries.get(message_id).map_or(&[], Vec::as_slice)
        } else {
            &[]
        }
    }

    #[cfg(test)]
    pub(super) fn seed(&mut self, scope: Scope, message_id: &str) {
        self.fixture = true;
        self.snapshot = Some(Snapshot {
            scope,
            ids: vec![message_id.into()],
            request: self.query_id,
        });
        self.summaries.insert(
            message_id.into(),
            vec![
                ReactionSummary {
                    kind: ReactionKind::Heart,
                    count: 2,
                    mine: true,
                },
                ReactionSummary {
                    kind: ReactionKind::Like,
                    count: 1,
                    mine: false,
                },
            ],
        );
    }
}

pub(super) fn supports(message: &Message) -> bool {
    message
        .source_client_id
        .as_deref()
        .is_some_and(|client_id| {
            crate::protocol::reactions::reference_supported(&message.id, client_id)
                && message.reply_reference().is_some()
        })
}

impl NativeApp {
    pub(super) fn reaction_scope(&self) -> Option<Scope> {
        let scope = self.message_menu_scope()?;
        (!self.demo && self.privacy_list_visible(&scope.thread)).then_some(scope)
    }

    pub(super) fn sync_reactions(&mut self, ctx: &egui::Context) {
        let scope = self.reaction_scope();
        let Some(state) = &mut self.live else { return };
        let reactions = &mut state.reactions;
        let Some(scope) = scope else {
            reactions.reset_view();
            return;
        };
        if reactions
            .snapshot
            .as_ref()
            .is_some_and(|old| old.scope != scope)
        {
            reactions.reset_view();
        }
        let Some(worker) = &state.worker else { return };
        let ids = self
            .model
            .messages
            .iter()
            .filter(|message| {
                message.account_id == scope.account
                    && message.thread_id == scope.thread
                    && !message.is_recalled()
                    && crate::domain::reply::valid_source_id(&message.id)
            })
            .take(crate::store::MAX_REFRESH_MESSAGES)
            .map(|message| message.id.as_str());
        if !reactions.dirty
            && reactions
                .snapshot
                .as_ref()
                .is_some_and(|old| old.ids.iter().map(String::as_str).eq(ids.clone()))
        {
            return;
        }
        let ids: Vec<String> = ids.map(str::to_owned).collect();
        if ids.is_empty() {
            reactions.reset_view();
            return;
        }
        let generation = reactions.query_id.wrapping_add(1).max(1);
        match worker.submit(LiveCommand::ReactionState {
            generation,
            thread: scope.thread.clone(),
            message_ids: ids.clone(),
        }) {
            Ok(()) => {
                reactions.query_id = generation;
                reactions.snapshot = Some(Snapshot {
                    scope,
                    ids,
                    request: generation,
                });
                reactions.dirty = false;
            }
            Err(SubmitError::Busy) => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(_) => {}
        }
    }

    pub(super) fn reaction_status(&self) -> Option<(&'static str, std::time::Instant)> {
        let thread = self.model.selected.as_deref()?;
        if !self.privacy_list_visible(thread) {
            return None;
        }
        self.live
            .as_ref()?
            .reactions
            .status
            .as_ref()
            .and_then(|notice| notice.for_thread(thread))
    }

    pub(super) fn can_react(&self, scope: &Scope, message: &Message) -> bool {
        if self.model.pending
            || self.selected_group_left()
            || !self.privacy_list_visible(&scope.thread)
            || scope.thread.strip_prefix("d:") == Some(scope.account.as_str())
            || message.account_id != scope.account
            || message.thread_id != scope.thread
            || !supports(message)
        {
            return false;
        }
        self.live.as_ref().is_some_and(|state| {
            #[cfg(test)]
            if state.reactions.fixture {
                return true;
            }
            state.reactions.available
                && state.worker.is_some()
                && state.connection == LiveConnection::Online
                && state.reactions.pending.is_none()
        })
    }

    pub(super) fn react(&mut self, scope: &Scope, message_id: &str, action: ReactionAction) {
        let eligible = self.reaction_scope().as_ref() == Some(scope)
            && self
                .model
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .is_some_and(|message| self.can_react(scope, message));
        if !eligible {
            return;
        }
        if let Some(state) = &mut self.live {
            let reactions = &mut state.reactions;
            let request_id = reactions.request_id.wrapping_add(1).max(1);
            let accepted = state.worker.as_ref().is_some_and(|worker| {
                worker
                    .submit(LiveCommand::React {
                        request_id,
                        thread: scope.thread.clone(),
                        message_id: message_id.into(),
                        action,
                    })
                    .is_ok()
            });
            reactions.status = Some(InteractionNotice::new(
                scope.thread.clone(),
                if accepted {
                    "Đang gửi cảm xúc…"
                } else {
                    "Chưa xếp được lượt gửi cảm xúc. Vui lòng thử lại."
                },
            ));
            if accepted {
                reactions.request_id = request_id;
                reactions.pending = Some((scope.thread.clone(), message_id.into(), request_id));
            }
        }
    }
}

pub(super) fn show_menu(
    ui: &mut egui::Ui,
    app: &NativeApp,
    scope: &Scope,
    message: &Message,
    buttons: &mut Vec<egui::Id>,
) -> Option<ReactionAction> {
    let state = app.live.as_ref()?;
    let active = app.can_react(scope, message);
    let unavailable = if state.reactions.pending.is_some() {
        "Đang chờ Zalo xử lý cảm xúc."
    } else if state.connection != LiveConnection::Online {
        "Cần kết nối Zalo để dùng cảm xúc."
    } else {
        "Phiên hoặc tin nhắn này chưa hỗ trợ cảm xúc."
    };
    let summary = state.reactions.at(scope, &message.id);
    let colors = crate::theme::palette(ui.ctx());
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let cell = ui.available_width() / PRESETS.len() as f32;
        for (index, (kind, label, _)) in PRESETS.iter().enumerate() {
            let selected = summary
                .iter()
                .any(|entry| entry.kind == *kind && entry.mine);
            let (rect, button) = ui
                .add_enabled_ui(active, |ui| {
                    ui.allocate_exact_size(vec2(cell, 36.0), Sense::click())
                })
                .inner;
            let fill = if button.is_pointer_button_down_on() {
                Some(colors.pressed)
            } else if selected {
                Some(colors.accent_soft)
            } else if button.hovered() {
                Some(colors.hover)
            } else {
                None
            };
            if let Some(fill) = fill {
                ui.painter().rect_filled(rect.shrink(2.0), 10.0, fill);
            }
            let tint = if active {
                egui::Color32::WHITE
            } else {
                egui::Color32::WHITE.gamma_multiply(0.45)
            };
            ui.painter().image(
                state.reaction_icons.0[index].id(),
                Rect::from_center_size(rect.center(), vec2(22.0, 22.0)),
                Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)),
                tint,
            );
            visuals::focus_outline(ui, &button, rect.shrink(2.0), 10.0);
            button.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, active, selected, *label)
            });
            if active {
                buttons.push(button.id);
            }
            if button.clicked() {
                action = Some(ReactionAction::Add(*kind));
            }
            button
                .on_hover_text(*label)
                .on_disabled_hover_text(unavailable);
        }
    });
    if summary.iter().any(|entry| entry.mine) {
        let remove = super::message_actions::action_row(
            ui,
            "Gỡ cảm xúc của tôi",
            "fa-close_24",
            active,
            false,
        );
        if active {
            buttons.push(remove.id);
        }
        if remove.clicked() {
            action = Some(ReactionAction::RemoveAll);
        }
        remove.on_disabled_hover_text(unavailable);
    }
    super::message_actions::menu_separator(ui);
    action
}

pub(super) fn paint_badge(
    ui: &mut egui::Ui,
    state: &super::live::LiveState,
    scope: &Scope,
    message: &Message,
    bubble: Rect,
) {
    if message.is_recalled()
        || message.account_id != scope.account
        || message.thread_id != scope.thread
    {
        return;
    }
    let entries = state.reactions.at(scope, &message.id);
    if entries.is_empty() {
        return;
    }
    let colors = crate::theme::palette(ui.ctx());
    let own = message.sender == scope.account;
    let mine = entries.iter().any(|entry| entry.mine);
    let total: u64 = entries.iter().map(|entry| u64::from(entry.count)).sum();
    let count = ui.painter().layout_no_wrap(
        total.to_string(),
        FontId::proportional(visuals::TEXT_SMALL),
        if mine {
            colors.accent_ink
        } else {
            colors.text_secondary
        },
    );
    let width = 18.0 * entries.len().min(PRESETS.len()) as f32 + count.size().x + 19.0;
    let (space, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let x = if own {
        bubble.right() - width
    } else {
        bubble.left()
    };
    let rect = Rect::from_min_size(
        pos2(
            x.clamp(space.left(), (space.right() - width).max(space.left())),
            space.top() + 2.0,
        ),
        vec2(width, 24.0),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.rect_filled(
        rect,
        12.0,
        if mine {
            colors.accent_soft
        } else {
            colors.surface
        },
    );
    p.rect_stroke(
        rect,
        12.0,
        Stroke::new(
            1.0_f32,
            if mine {
                colors.accent_ink
            } else {
                colors.border_strong
            },
        ),
    );
    let mut offset = rect.left() + 8.0;
    let mut details = Vec::new();
    for (index, (kind, label, _)) in PRESETS.iter().enumerate() {
        if let Some(entry) = entries.iter().find(|entry| entry.kind == *kind) {
            p.image(
                state.reaction_icons.0[index].id(),
                Rect::from_center_size(pos2(offset + 8.0, rect.center().y), vec2(16.0, 16.0)),
                Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            offset += 18.0;
            details.push(format!(
                "{label}: {}{}",
                entry.count,
                if entry.mine { " (có bạn)" } else { "" }
            ));
        }
    }
    let text_color = if mine {
        colors.accent_ink
    } else {
        colors.text_secondary
    };
    p.galley(
        pos2(offset + 2.0, rect.center().y - count.size().y / 2.0),
        count,
        text_color,
    );
    let response = ui.interact(
        rect,
        ui.id().with(("reaction_badge", &message.id)),
        Sense::hover(),
    );
    let label = details.join("\n");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    response.on_hover_text(label);
}
