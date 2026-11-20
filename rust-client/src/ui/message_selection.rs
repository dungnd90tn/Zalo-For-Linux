//! Page-scoped local selection. Clipboard text is projected from current rows at copy time.
use super::{message_actions::Scope, visuals, NativeApp};
use crate::domain::Message;
use eframe::egui::{self, pos2, vec2, Rect, RichText, Sense, Stroke};
use std::collections::BTreeSet;

pub(super) const GUTTER: f32 = 48.0;

pub(super) fn text(message: &Message) -> Option<&str> {
    let body = message.text.as_str();
    (!message.is_recalled()
        && !message.reply_blocked
        && message.media.is_none()
        && message.sticker.is_none()
        && !body.trim().is_empty()
        && !(body.starts_with("[Tin nhắn ") && body.ends_with(" chưa được hỗ trợ]"))
        && body.len() <= crate::domain::MAX_TEXT_BYTES)
        .then_some(body)
}

pub(super) struct Selection {
    scope: Scope,
    ids: BTreeSet<String>,
    initial_focus: Option<String>,
    reveal_pending: bool,
}

impl Selection {
    fn eligible(&self, message: &Message) -> bool {
        self.scope.account == message.account_id
            && self.scope.thread == message.thread_id
            && text(message).is_some()
    }

    fn copy_text(&self, messages: &[Message]) -> String {
        let mut selected: Vec<_> = messages
            .iter()
            .filter(|message| self.ids.contains(&message.id) && self.eligible(message))
            .collect();
        selected.sort_unstable_by(|left, right| {
            (left.sent_at_ms, &left.id).cmp(&(right.sent_at_ms, &right.id))
        });
        selected
            .into_iter()
            .filter_map(text)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(super) fn checkbox(&mut self, ui: &egui::Ui, message: &Message, bubble: Rect) {
        if !self.eligible(message) {
            return;
        }
        let rect = Rect::from_center_size(
            pos2(
                ui.max_rect().left() - GUTTER / 2.0,
                bubble.top() + (bubble.height() / 2.0).min(visuals::ICON_BUTTON / 2.0),
            ),
            vec2(visuals::ICON_BUTTON, visuals::ICON_BUTTON),
        );
        let initial_row =
            ui.is_enabled() && self.initial_focus.as_deref() == Some(message.id.as_str());
        let reveal = initial_row && std::mem::take(&mut self.reveal_pending);
        if reveal {
            let target = rect.union(bubble);
            let target = if target.height() > ui.clip_rect().height() {
                rect
            } else {
                target
            };
            if !ui.clip_rect().contains_rect(target) {
                ui.scroll_to_rect(target, None);
            }
        }
        if !ui.is_rect_visible(rect) {
            if initial_row && !reveal {
                self.initial_focus = None;
            }
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        let response = ui.interact(
            rect,
            ui.id().with((
                "selected_message",
                &self.scope.account,
                &self.scope.thread,
                self.scope.generation,
                self.scope.auth_generation,
                &message.id,
            )),
            Sense::click(),
        );
        if initial_row {
            response.request_focus();
            self.initial_focus = None;
        }
        if response.clicked() && !self.ids.remove(&message.id) {
            self.ids.insert(message.id.clone());
        }
        let checked = self.ids.contains(&message.id);
        let outline = if response.is_pointer_button_down_on() {
            colors.pressed
        } else if response.hovered() {
            colors.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 12.0, outline);
        let box_rect = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
        ui.painter().rect(
            box_rect,
            5.0,
            if checked {
                colors.primary
            } else {
                colors.surface
            },
            Stroke::new(
                1.0_f32,
                if checked {
                    colors.primary
                } else {
                    colors.border_strong
                },
            ),
        );
        if checked {
            visuals::icon(
                ui,
                box_rect.shrink(2.0),
                visuals::Icon::Check,
                colors.on_color,
            );
        }
        visuals::focus_outline(ui, &response, rect, 12.0);
        let label = if checked {
            "Bỏ chọn tin nhắn"
        } else {
            "Chọn tin nhắn"
        };
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), checked, label)
        });
        response.on_hover_text(label);
    }
}

impl NativeApp {
    pub(super) fn message_selection_scope(&self) -> Option<Scope> {
        let scope = self.message_menu_scope()?;
        #[cfg(feature = "messaging")]
        if !self.privacy_list_visible(&scope.thread) {
            return None;
        }
        Some(scope)
    }

    pub(super) fn start_message_selection(&mut self, message_id: &str) {
        let Some(scope) = self.message_selection_scope() else {
            return;
        };
        let mut selection = Selection {
            scope,
            ids: BTreeSet::new(),
            initial_focus: Some(message_id.into()),
            reveal_pending: true,
        };
        if self
            .model
            .messages
            .iter()
            .any(|message| message.id == message_id && selection.eligible(message))
        {
            selection.ids.insert(message_id.into());
            self.message_selection = Some(selection);
        }
    }

    fn sync_message_selection(&mut self) {
        let scope = self.message_selection_scope();
        let Some(selection) = &mut self.message_selection else {
            return;
        };
        if scope.as_ref() != Some(&selection.scope) {
            self.message_selection = None;
            return;
        }
        // A recalled row or a row that no longer has eligible text loses selection.
        selection.ids.retain(|id| {
            self.model.messages.iter().any(|message| {
                message.id == *id
                    && selection.scope.account == message.account_id
                    && selection.scope.thread == message.thread_id
                    && text(message).is_some()
            })
        });
    }

    pub(super) fn selection_toolbar(&mut self, ui: &mut egui::Ui) {
        self.sync_message_selection();
        let Some(selection) = &self.message_selection else {
            return;
        };
        let count = selection.ids.len();
        let escape = ui.is_enabled()
            && !self.ime_composing
            && ui.input_mut(|input| {
                !input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
                    && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
        let colors = crate::theme::palette(ui.ctx());
        let mut copy = false;
        let mut cancel = escape;
        egui::Frame::none()
            .fill(colors.surface)
            .inner_margin(egui::Margin::symmetric(16.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Đã chọn {count} tin")).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        copy = visuals::primary_button(ui, count > 0, "Sao chép", vec2(92.0, 44.0))
                            .clicked();
                        cancel |= ui
                            .add_sized(vec2(68.0, 44.0), egui::Button::new("Bỏ chọn"))
                            .clicked();
                    });
                });
            });
        if copy {
            let body = selection.copy_text(&self.model.messages);
            if !body.is_empty() {
                ui.ctx().copy_text(body);
            }
        }
        if copy || cancel {
            self.message_selection = None;
            ui.ctx().request_repaint();
        }
    }
}
