//! Scoped reply selection and bounded quote presentation. No raw wire metadata or I/O.
use super::{visuals, NativeApp};
use eframe::egui::{self, pos2, vec2, RichText, Sense};

const TRAY_HEIGHT: f32 = 60.0;

fn excerpt(text: &str) -> String {
    text.chars()
        .take(240)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect()
}

pub(super) fn quote_block(ui: &mut egui::Ui, sender: &str, text: &str, width: f32) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::none()
        .fill(colors.accent_soft)
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(1.0));
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                egui::Label::new(
                    RichText::new(sender)
                        .family(egui::FontFamily::Name("Zalo Semibold".into()))
                        .size(visuals::TEXT_SMALL)
                        .color(colors.accent_ink),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(excerpt(text))
                        .size(visuals::TEXT_BODY)
                        .color(colors.text_primary),
                )
                .truncate(),
            );
        });
}

impl NativeApp {
    pub(super) fn reply_sender_label(&self, thread: &str, owner: &str) -> String {
        if self.account_id() == Some(owner) {
            return "Bạn".into();
        }
        #[cfg(feature = "messaging")]
        if let Some(name) = self.live.as_ref().and_then(|state| {
            state.account.as_ref().and_then(|account| {
                state
                    .sender_names
                    .get(&account.id, self.model.generation, thread, owner)
            })
        }) {
            return name.into();
        }
        if thread.strip_prefix("d:") == Some(owner) {
            if let Some(conversation) = self.model.conversations.iter().find(|c| c.id == thread) {
                let name = conversation.title.as_str();
                if !name.trim().is_empty()
                    && name != owner
                    && name != thread
                    && !name.chars().all(|c| c.is_ascii_digit())
                    && name.strip_prefix("Zalo ID ") != Some(owner)
                {
                    return name.into();
                }
            }
        }
        "Người gửi".into()
    }

    pub(super) fn reply_tray_height(&self) -> f32 {
        if self.drafts.active.reply().is_some() {
            TRAY_HEIGHT
        } else {
            0.0
        }
    }

    pub(super) fn reply_tray(&mut self, ui: &mut egui::Ui, composer: egui::Rect) {
        let Some(reply) = self.drafts.active.reply() else {
            return;
        };
        let colors = crate::theme::palette(ui.ctx());
        let rect = egui::Rect::from_min_max(
            composer.min + vec2(8.0, 6.0),
            pos2(composer.right() - 8.0, composer.top() + TRAY_HEIGHT),
        );
        let close_rect = egui::Rect::from_center_size(
            pos2(rect.right() - 24.0, rect.center().y),
            vec2(44.0, 44.0),
        );
        ui.painter().rect_filled(rect, 10.0, colors.accent_soft);
        let heading = format!(
            "Trả lời {}",
            self.reply_sender_label(&reply.thread_id, &reply.sender)
        );
        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
            rect.min + vec2(10.0, 5.0),
            pos2(close_rect.left() - 6.0, rect.bottom() - 4.0),
        )));
        text_ui.set_clip_rect(text_ui.clip_rect().intersect(rect));
        text_ui.spacing_mut().item_spacing.y = 3.0;
        text_ui.add(
            egui::Label::new(
                RichText::new(heading)
                    .family(egui::FontFamily::Name("Zalo Semibold".into()))
                    .size(visuals::TEXT_SMALL)
                    .color(colors.accent_ink),
            )
            .truncate(),
        );
        text_ui.add(
            egui::Label::new(
                RichText::new(excerpt(&reply.text))
                    .size(visuals::TEXT_BODY)
                    .color(colors.text_primary),
            )
            .truncate(),
        );
        let close = ui.interact(close_rect, ui.id().with("cancel_reply"), Sense::click());
        let fill = if close.is_pointer_button_down_on() {
            colors.pressed
        } else if close.hovered() || close.has_focus() {
            colors.hover
        } else {
            colors.accent_soft
        };
        ui.painter().rect_filled(close_rect, 10.0, fill);
        visuals::source_icon(
            ui,
            close_rect.shrink(10.0),
            "fa-close_24",
            colors.text_secondary,
        );
        visuals::focus_outline(ui, &close, close_rect, 10.0);
        if close
            .on_hover_text("Hủy trả lời · giữ nội dung đang soạn")
            .clicked()
        {
            self.drafts.active.clear_reply();
            self.focus_composer = true;
        }
    }
}
