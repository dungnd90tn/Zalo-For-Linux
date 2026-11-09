//! Explicit clipboard actions for the currently visible, scoped message. No I/O or raw sidecars.
use super::{ModalFocus, NativeApp};
use crate::domain::{MediaDescriptor, Message};
use eframe::egui::{self, vec2};

#[derive(Clone)]
pub(super) struct Scope {
    account: String,
    thread: String,
    generation: u64,
    auth_generation: Option<u64>,
}

#[derive(Clone)]
pub(super) struct Popup {
    scope: Scope,
    message: String,
    position: egui::Pos2,
    just_opened: bool,
    pub(super) focus: Option<ModalFocus>,
}

impl Popup {
    pub(super) fn new(
        scope: Scope,
        message: String,
        position: egui::Pos2,
        focus: Option<ModalFocus>,
    ) -> Self {
        Self { scope, message, position, just_opened: true, focus }
    }
}

struct Action {
    label: &'static str,
    text: String,
}

// Only presentation fields are copied. Never expose raw content, encrypted keys or placeholders.
fn actions(message: &Message) -> Vec<Action> {
    let mut result = Vec::with_capacity(2);
    let (label, parts, link) = match &message.media {
        Some(MediaDescriptor::Photo(photo)) => (
            "Sao chép chú thích",
            vec![photo.title.as_str(), photo.caption.as_str()],
            photo.original_url.as_deref(),
        ),
        Some(MediaDescriptor::File(file)) => (
            "Sao chép tên tệp",
            vec![file.title.as_str()],
            if file.encrypted { None } else { file.url.as_deref() },
        ),
        Some(MediaDescriptor::WebContent(card) | MediaDescriptor::Recommended(card)) => {
            let mut parts = vec![card.title.as_str(), card.description.as_str()];
            for row in &card.rows {
                parts.extend([row.label.as_str(), row.value.as_str()]);
            }
            ("Sao chép nội dung", parts, card.url.as_deref())
        }
        Some(MediaDescriptor::Poll(poll)) => (
            "Sao chép nội dung",
            vec![poll.title.as_str(), poll.question.as_str()],
            None,
        ),
        None if message.sticker.is_none()
            && !(message.text.starts_with("[Tin nhắn ")
                && message.text.ends_with(" chưa được hỗ trợ]")) =>
        {
            ("Sao chép tin nhắn", vec![message.text.as_str()], None)
        }
        None => return result,
    };
    let text = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n");
    if !text.is_empty() && text.len() <= crate::domain::MAX_TEXT_BYTES {
        result.push(Action { label, text });
    }
    if let Some(text) = link.and_then(external_url) {
        result.push(Action { label: "Sao chép liên kết", text });
    }
    result
}

pub(super) fn requested(ui: &egui::Ui, rect: egui::Rect, message: &Message) -> Option<egui::Pos2> {
    if !ui.is_enabled() { return None; }
    let position = ui.input(|input| {
        input.pointer.button_pressed(egui::PointerButton::Secondary)
            .then(|| input.pointer.interact_pos()).flatten()
    })?;
    (rect.intersect(ui.clip_rect()).contains(position) && !actions(message).is_empty())
        .then_some(position)
}

pub(super) fn external_url(raw: &str) -> Option<String> {
    if raw.len() > 8192 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()).then(|| url.to_string())
}

impl NativeApp {
    pub(super) fn message_menu_scope(&self) -> Option<Scope> {
        let thread = self.model.selected.clone()?;
        #[cfg(feature = "messaging")]
        if !self.privacy_visible(&thread) { return None; }
        if !self.demo && !self.is_signed_in() { return None; }
        Some(Scope {
            account: self.account_id().unwrap_or(crate::app::DEMO_ACCOUNT).into(),
            thread,
            generation: self.model.generation,
            auth_generation: self.auth_generation(),
        })
    }

    fn menu_message(&self, popup: &Popup) -> Option<&Message> {
        let current = self.message_menu_scope()?;
        let scope = &popup.scope;
        if current.account != scope.account || current.thread != scope.thread
            || current.generation != scope.generation || current.auth_generation != scope.auth_generation {
            return None;
        }
        self.model.messages.iter().find(|message| {
            message.id == popup.message && message.account_id == scope.account
                && message.thread_id == scope.thread
        })
    }

    pub(super) fn sync_message_menu(&mut self) {
        let invalid = self.message_menu.as_ref().is_some_and(|popup| {
            self.settings_open || self.details || self.account_popup.is_some()
                || self.menu_message(popup).is_none()
        });
        if invalid {
            self.message_menu = None;
            // Never return focus to a newly selected or newly locked conversation.
            self.modal_focus = None;
        }
    }

    pub(super) fn show_message_menu(&mut self, ctx: &egui::Context) {
        self.sync_message_menu();
        let Some(popup) = self.message_menu.clone() else { return; };
        let Some(message) = self.menu_message(&popup) else { return; };
        let actions = actions(message);
        if actions.is_empty() {
            self.message_menu = None;
            return;
        }
        self.latch_modal_input(ctx);
        let width = 244.0_f32.min(ctx.screen_rect().width() - 32.0);
        let mut copied = false;
        let response = egui::Area::new(egui::Id::new("scoped_message_actions"))
            .kind(egui::UiKind::Popup)
            .order(egui::Order::Foreground)
            .movable(false)
            .fixed_pos(popup.position)
            .constrain_to(ctx.screen_rect().shrink(8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    for action in actions {
                        if ui.add_sized(vec2(width, 44.0), egui::Button::new(action.label)).clicked() {
                            ctx.copy_text(action.text);
                            copied = true;
                        }
                    }
                });
            });
        let escape = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if copied || escape || (!popup.just_opened && response.response.clicked_elsewhere()) {
            self.message_menu = None;
        } else if let Some(popup) = &mut self.message_menu {
            popup.just_opened = false;
        }
    }
}
