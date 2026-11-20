//! Explicit clipboard actions for the currently visible, scoped message. No I/O or raw sidecars.
use super::{visuals, ModalFocus, NativeApp};
use crate::domain::{MediaDescriptor, Message};
use eframe::egui::{self, pos2, vec2, Align2, FontId, Rect, Sense, Stroke};

const MENU_WIDTH: f32 = 216.0;
const ROW_HEIGHT: f32 = 32.0;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Scope {
    pub(super) account: String,
    pub(super) thread: String,
    pub(super) generation: u64,
    pub(super) auth_generation: Option<u64>,
}

#[derive(Clone)]
pub(super) struct Popup {
    scope: Scope,
    message: String,
    position: egui::Pos2,
    just_opened: bool,
    action_focus: Option<egui::Id>,
    pub(super) focus: Option<ModalFocus>,
}

impl Popup {
    pub(super) fn new(
        scope: Scope,
        message: String,
        position: egui::Pos2,
        focus: Option<ModalFocus>,
    ) -> Self {
        Self {
            scope,
            message,
            position,
            just_opened: true,
            action_focus: None,
            focus,
        }
    }
}

struct Action {
    label: &'static str,
    glyph: &'static str,
    text: String,
}

// Only presentation fields are copied. Never expose raw content, encrypted keys or placeholders.
fn actions(message: &Message) -> Vec<Action> {
    let mut result = Vec::with_capacity(2);
    if message.is_recalled() {
        return result;
    }
    let (label, parts, link) = match &message.media {
        Some(MediaDescriptor::Photo(photo)) => (
            "Sao chép chú thích",
            vec![photo.title.as_str(), photo.caption.as_str()],
            photo.original_url.as_deref(),
        ),
        Some(MediaDescriptor::File(file)) => (
            "Sao chép tên tệp",
            vec![file.title.as_str()],
            if file.encrypted {
                None
            } else {
                file.url.as_deref()
            },
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
    let text = parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() && text.len() <= crate::domain::MAX_TEXT_BYTES {
        result.push(Action {
            label,
            glyph: "fa-icon-outline-copy",
            text,
        });
    }
    if let Some(text) = link.and_then(external_url) {
        result.push(Action {
            label: "Sao chép liên kết",
            glyph: "fa-icon-outline-link",
            text,
        });
    }
    result
}

pub(super) fn action_row(
    ui: &mut egui::Ui,
    label: &str,
    glyph: &str,
    enabled: bool,
    destructive: bool,
) -> egui::Response {
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click())
        })
        .inner;
    let colors = crate::theme::palette(ui.ctx());
    let row = rect.shrink2(vec2(0.0, 1.0));
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(row, 7.0, colors.pressed);
    } else if response.hovered() {
        ui.painter().rect_filled(row, 7.0, colors.hover);
    }
    let (ink, icon) = if !response.enabled() {
        (colors.text_disabled, colors.text_disabled)
    } else if destructive {
        (colors.error, colors.error)
    } else {
        (colors.text_primary, colors.text_tertiary)
    };
    let icon_center = pos2(rect.left() + 20.0, rect.center().y);
    visuals::source_icon(
        ui,
        Rect::from_center_size(icon_center, vec2(17.0, 17.0)),
        glyph,
        icon,
    );
    ui.painter().text(
        pos2(rect.left() + 39.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(visuals::TEXT_BODY),
        ink,
    );
    visuals::focus_outline(ui, &response, row, 7.0);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response
}

#[cfg(feature = "messaging")]
pub(super) fn menu_separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment(
        [pos2(rect.left() + 9.0, y), pos2(rect.right() - 9.0, y)],
        Stroke::new(1.0_f32, crate::theme::palette(ui.ctx()).divider),
    );
}

pub(super) fn requested(ui: &egui::Ui, rect: egui::Rect, message: &Message) -> Option<egui::Pos2> {
    if !ui.is_enabled() {
        return None;
    }
    let position = ui.input(|input| {
        input
            .pointer
            .button_pressed(egui::PointerButton::Secondary)
            .then(|| input.pointer.interact_pos())
            .flatten()
    })?;
    (ui.rect_contains_pointer(rect) && !actions(message).is_empty()).then_some(position)
}

pub(super) fn keyboard_menu(
    ui: &egui::Ui,
    message: &Message,
    rect: egui::Rect,
) -> Option<egui::Pos2> {
    if !ui.is_enabled() || !ui.is_rect_visible(rect) {
        return None;
    }
    let row = ui.interact(
        rect,
        ui.id().with((
            "message_options",
            &message.account_id,
            &message.thread_id,
            &message.id,
        )),
        egui::Sense::focusable_noninteractive(),
    );
    row.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Tùy chọn tin nhắn")
    });
    super::visuals::focus_outline(ui, &row, rect, 12.0);
    (row.has_focus()
        && ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10)
        }))
    .then_some(rect.center())
}

pub(super) fn external_url(raw: &str) -> Option<String> {
    if raw.len() > 8192 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

pub(super) fn popup_tab(ctx: &egui::Context, focus: Option<egui::Id>) -> Option<bool> {
    // egui may try to move focus to a disabled background widget. Restore
    // the popup's action focus before collecting the visible controls.
    if let Some(id) = focus {
        ctx.memory_mut(|memory| memory.request_focus(id));
    }
    ctx.input_mut(|input| {
        if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
            Some(false)
        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
            Some(true)
        } else {
            None
        }
    })
}

pub(super) fn focus_popup_actions(
    ctx: &egui::Context,
    tab: Option<bool>,
    buttons: &[egui::Id],
    action_focus: &mut Option<egui::Id>,
) {
    let Some(forward) = tab.filter(|_| !buttons.is_empty()) else {
        return;
    };
    let focus = action_focus.or_else(|| ctx.memory(|memory| memory.focused()));
    let current = buttons.iter().position(|id| Some(*id) == focus);
    let next = match (current, forward) {
        (Some(index), true) => (index + 1) % buttons.len(),
        (Some(index), false) => (index + buttons.len() - 1) % buttons.len(),
        (None, true) => 0,
        (None, false) => buttons.len() - 1,
    };
    ctx.memory_mut(|memory| memory.request_focus(buttons[next]));
    *action_focus = Some(buttons[next]);
    ctx.request_repaint();
}

impl NativeApp {
    pub(super) fn message_menu_input(&self, input: &mut egui::RawInput) {
        if self.message_menu.is_none() {
            return;
        }
        // Use the same scoped focus order as Tab. egui records directional focus before
        // widgets consume their events, so this translation has to happen on raw input.
        for event in &mut input.events {
            if let egui::Event::Key { key, modifiers, .. } = event {
                if modifiers.is_none() {
                    match key {
                        egui::Key::ArrowDown => *key = egui::Key::Tab,
                        egui::Key::ArrowUp => {
                            *key = egui::Key::Tab;
                            *modifiers = egui::Modifiers::SHIFT;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(super) fn message_menu_scope(&self) -> Option<Scope> {
        let thread = self.model.selected.clone()?;
        #[cfg(feature = "messaging")]
        if !self.privacy_visible(&thread) {
            return None;
        }
        if !self.demo && !self.is_signed_in() {
            return None;
        }
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
        if current.account != scope.account
            || current.thread != scope.thread
            || current.generation != scope.generation
            || current.auth_generation != scope.auth_generation
        {
            return None;
        }
        self.model.messages.iter().find(|message| {
            message.id == popup.message
                && message.account_id == scope.account
                && message.thread_id == scope.thread
        })
    }

    pub(super) fn sync_message_menu(&mut self) {
        #[cfg(feature = "live-login")]
        let live_modal = self.live.as_ref().is_some_and(|state| {
            let open = state.logout_dialog;
            #[cfg(feature = "messaging")]
            let open = open
                || state.open_dialog
                || state.review_dialog
                || state.privacy.prompt.is_some()
                || state.forwarding.is_open()
                || state.recall.is_open()
                || state.groups.is_open();
            open
        });
        #[cfg(not(feature = "live-login"))]
        let live_modal = false;
        let invalid = self.message_menu.as_ref().is_some_and(|popup| {
            self.settings_open
                || self.details
                || self.account_popup.is_some()
                || live_modal
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
        let Some(popup) = self.message_menu.clone() else {
            return;
        };
        self.latch_modal_input(ctx);
        let Some(message) = self.menu_message(&popup) else {
            return;
        };
        let actions = actions(message);
        let can_select_many = self.message_selection_scope().is_some()
            && super::message_selection::text(message).is_some();
        #[cfg(feature = "messaging")]
        let can_forward = self.can_forward(message);
        #[cfg(feature = "messaging")]
        let mut forward_selected = false;
        #[cfg(feature = "messaging")]
        let can_recall = self.can_recall(message);
        #[cfg(feature = "messaging")]
        let mut recall_selected = false;
        #[cfg(feature = "messaging")]
        let own = self.account_id() == Some(message.sender.as_str());
        let reply = message.reply_reference();
        #[cfg(feature = "messaging")]
        let reaction_scope = self
            .reaction_scope()
            .filter(|_| super::reactions::supports(message));
        #[cfg(feature = "messaging")]
        let mut reaction_selected = None;
        let group_left = self.selected_group_left();
        let can_reply = !self.demo
            && !group_left
            && reply.is_some()
            && self.drafts.active.attachment().is_none()
            && !self.photo_selected();
        let reply_unavailable = if self.demo {
            "Bản demo chưa hỗ trợ trả lời tin nhắn Zalo."
        } else if group_left {
            "Bạn đã rời nhóm. Lịch sử vẫn có thể xem."
        } else if message.media.is_some() || message.sticker.is_some() {
            "Hiện chỉ hỗ trợ trả lời tin nhắn văn bản."
        } else if self.photo_selected() {
            "Bỏ ảnh trước khi trả lời tin nhắn."
        } else if self.drafts.active.attachment().is_some() {
            "Bỏ tệp đính kèm trước khi trả lời tin nhắn văn bản."
        } else if message.reply_blocked {
            "Loại tin nhắn này chưa hỗ trợ trả lời."
        } else {
            "Tin nhắn đã lưu chưa có đủ thông tin gốc để trả lời."
        };
        if actions.is_empty() {
            self.message_menu = None;
            return;
        }
        let mut copied = false;
        let mut reply_selected = false;
        let mut select_many = false;
        let tab = popup_tab(ctx, popup.action_focus);
        let mut buttons = Vec::with_capacity(actions.len());
        let response = egui::Area::new(egui::Id::new((
            "scoped_message_actions",
            &popup.scope.account,
            &popup.scope.thread,
            &popup.message,
            popup.scope.generation,
            popup.scope.auth_generation,
        )))
        .kind(egui::UiKind::Popup)
        .order(egui::Order::Foreground)
        .movable(false)
        // The popup container must not take the first Tab stop ahead of its actions.
        .sense(egui::Sense::hover())
        .fixed_pos(popup.position)
        .constrain_to(ctx.screen_rect().shrink(8.0))
        .show(ctx, |ui| {
            let colors = crate::theme::palette(ctx);
            egui::Frame::popup(ui.style())
                .fill(colors.surface)
                .stroke(Stroke::new(1.0_f32, colors.border))
                .inner_margin(6.0)
                .rounding(12.0)
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    #[cfg(feature = "messaging")]
                    if let Some(scope) = &reaction_scope {
                        reaction_selected =
                            super::reactions::show_menu(ui, self, scope, message, &mut buttons);
                    }
                    let button = action_row(ui, "Trả lời", "fa-outline-reply", can_reply, false);
                    if can_reply {
                        buttons.push(button.id);
                    }
                    reply_selected = button.clicked();
                    button.on_disabled_hover_text(reply_unavailable);
                    for action in actions {
                        let button = action_row(ui, action.label, action.glyph, true, false);
                        buttons.push(button.id);
                        if button.clicked() {
                            ctx.copy_text(action.text);
                            copied = true;
                        }
                    }
                    #[cfg(feature = "messaging")]
                    if !self.demo {
                        let button = action_row(
                            ui,
                            "Chuyển tiếp",
                            "fa-csc-menu-icon-forward",
                            can_forward,
                            false,
                        );
                        if can_forward {
                            buttons.push(button.id);
                        }
                        forward_selected = button.clicked();
                        button.on_disabled_hover_text(
                            "Chỉ hỗ trợ tin văn bản cá nhân đủ dữ liệu gốc khi đang kết nối.",
                        );
                    }
                    if can_select_many {
                        let button = action_row(
                            ui,
                            "Chọn nhiều tin nhắn",
                            "fa-multiselect-stroke-checkbox",
                            true,
                            false,
                        );
                        buttons.push(button.id);
                        select_many = button.clicked();
                    }
                    #[cfg(feature = "messaging")]
                    if !self.demo && own {
                        menu_separator(ui);
                        let button =
                            action_row(ui, "Thu hồi", "fa-outline-recall", can_recall, true);
                        if can_recall {
                            buttons.push(button.id);
                        }
                        recall_selected = button.clicked();
                        button.on_disabled_hover_text(
                            "Chỉ hỗ trợ tin văn bản cá nhân đã gửi khi đang kết nối.",
                        );
                    }
                });
        });
        if let Some(popup) = &mut self.message_menu {
            focus_popup_actions(ctx, tab, &buttons, &mut popup.action_focus);
        }
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if select_many {
            self.message_menu = None;
            self.start_message_selection(&popup.message);
            return;
        }
        #[cfg(feature = "messaging")]
        if let (Some(action), Some(scope)) = (reaction_selected, reaction_scope) {
            self.message_menu = None;
            self.react(&scope, &popup.message, action);
            return;
        }
        if reply_selected {
            if let Some(reply) = reply {
                self.drafts.active.set_reply(reply);
                self.focus_composer = true;
            }
        }
        #[cfg(feature = "messaging")]
        if forward_selected {
            self.message_menu = None;
            self.open_forward(&popup.message);
            return;
        }
        #[cfg(feature = "messaging")]
        if recall_selected {
            self.message_menu = None;
            self.open_recall(&popup.message);
            return;
        }
        if copied
            || reply_selected
            || escape
            || (!popup.just_opened && response.response.clicked_elsewhere())
        {
            self.message_menu = None;
        } else if let Some(popup) = &mut self.message_menu {
            popup.just_opened = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileDescriptor, PhotoDescriptor};

    fn message() -> Message {
        Message {
            reply_blocked: false,
            recalled: false,
            source_client_id: None,
            quote: None,
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "copy-1".into(),
            sender: "22".into(),
            text: "Tiếng Việt 👋\nDòng hai giữ nguyên khoảng trắng  ".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn copies_visible_text_and_metadata_without_placeholders_or_encrypted_links() {
        let mut message = message();
        assert_eq!(actions(&message)[0].text, message.text);
        message.text = crate::domain::LEGACY_STICKER_PLACEHOLDER.into();
        assert!(actions(&message).is_empty());
        message.media = Some(MediaDescriptor::Photo(PhotoDescriptor {
            title: "Ảnh minh họa".into(),
            caption: "Chú thích tiếng Việt".into(),
            original_url: Some("https://f1.zdn.vn/original.jxl?sig=one%2Btwo".into()),
            thumbnail_url: Some("https://f1.zdn.vn/thumbnail.jpg".into()),
            width: None,
            height: None,
        }));
        let photo = actions(&message);
        assert_eq!(photo.len(), 2);
        assert_eq!(photo[0].text, "Ảnh minh họa\nChú thích tiếng Việt");
        assert_eq!(
            photo[1].text,
            "https://f1.zdn.vn/original.jxl?sig=one%2Btwo"
        );
        message.media = Some(MediaDescriptor::File(FileDescriptor {
            title: "Báo cáo.pdf".into(),
            size_bytes: Some(10),
            extension: "pdf".into(),
            url: Some("https://f1.zdn.vn/secret.pdf".into()),
            encrypted: true,
            is_folder: false,
        }));
        let file = actions(&message);
        assert_eq!(file.len(), 1);
        assert_eq!(file[0].text, "Báo cáo.pdf");
    }

    #[test]
    fn clipboard_links_share_the_explicit_browser_url_validation() {
        for invalid in [
            "javascript:alert(1)",
            "file:///tmp/private",
            "https://user:secret@example.com/a",
            "https://example.com/\nsecret",
            " https://example.com/",
        ] {
            assert!(external_url(invalid).is_none(), "{invalid:?}");
        }
        assert_eq!(
            external_url("http://example.com/ảnh?q=one%2Btwo").unwrap(),
            "http://example.com/%E1%BA%A3nh?q=one%2Btwo"
        );
    }

    #[test]
    fn a_foreground_window_cannot_open_a_menu_for_the_message_behind_it() {
        let ctx = egui::Context::default();
        let message = message();
        for clicked in [false, false, true] {
            let events = if clicked {
                vec![
                    egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(50.0, 50.0),
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ]
            } else {
                vec![]
            };
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(200.0, 200.0), egui::Sense::hover());
                        assert!(requested(ui, rect, &message).is_none());
                    });
                    egui::Area::new(egui::Id::new("covering_window"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(egui::pos2(20.0, 20.0))
                        .show(ctx, |ui| {
                            ui.allocate_exact_size(vec2(200.0, 200.0), egui::Sense::click());
                        });
                },
            );
        }
    }

    #[cfg(feature = "messaging")]
    mod native {
        use super::*;
        use crate::ui::{live, privacy};

        fn fixture(ctx: &egui::Context) -> (NativeApp, std::path::PathBuf) {
            let (mut app, root) = live::tests::fixture(ctx);
            app.live.as_mut().unwrap().privacy = privacy::Privacy::fixture_ready();
            let message = message();
            app.model.conversations = vec![crate::domain::Conversation {
                id: message.thread_id.clone(),
                title: "Hội thoại tổng hợp".into(),
                last_message_at_ms: message.sent_at_ms,
                last_text: message.text.clone(),
                last_sender: message.sender.clone(),
                group_left: false,
            }];
            app.model.selected = Some(message.thread_id.clone());
            app.model.messages = vec![message];
            app.drafts.select("d:22").unwrap();
            app.drafts.active.text = "Bản nháp phải được giữ nguyên".into();
            app.focus_composer = true;
            (app, root)
        }

        fn frame(
            app: &mut NativeApp,
            ctx: &egui::Context,
            size: egui::Vec2,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            };
            app.message_menu_input(&mut input);
            ctx.run(input, |ctx| {
                app.sync_message_menu();
                app.update_modal_input(ctx);
                app.workspace(ctx);
            })
        }

        fn text_rect(out: &egui::FullOutput, text: &str) -> egui::Rect {
            out.shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(value) if value.galley.job.text == text => {
                        let rect = egui::Rect::from_min_size(value.pos, value.galley.size());
                        shape.clip_rect.contains_rect(rect).then_some(rect)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing visible text {text:?}"))
        }

        fn pointer(
            position: egui::Pos2,
            button: egui::PointerButton,
            pressed: bool,
        ) -> Vec<egui::Event> {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        }

        fn key(key: egui::Key) -> Vec<egui::Event> {
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                })
                .collect()
        }

        #[test]
        fn keyboard_multiselect_copies_in_time_order_and_preserves_the_draft() {
            let ctx = egui::Context::default();
            let size = vec2(640.0, 480.0);
            let (mut app, root) = fixture(&ctx);
            let draft = app.drafts.active.text.clone();
            let current = app.model.messages[0].clone();
            let mut older = current.clone();
            older.id = "copy-older".into();
            older.text = "Tin nhắn trước".into();
            older.sent_at_ms -= 1000;
            app.model.messages.insert(0, older.clone());
            let _ = frame(&mut app, &ctx, size, vec![]);
            app.message_menu = Some(Popup::new(
                app.message_menu_scope().unwrap(),
                current.id,
                egui::pos2(220.0, 100.0),
                None,
            ));
            for _ in 0..2 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            let menu = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            let focused = ctx.memory(|memory| memory.focused()).unwrap();
            assert!(ctx
                .read_response(focused)
                .unwrap()
                .rect
                .contains(text_rect(&menu, "Chọn nhiều tin nhắn").center()));
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Enter));
            let _ = frame(&mut app, &ctx, size, vec![]);
            let selected = frame(&mut app, &ctx, size, vec![]);
            assert!(text_rect(&selected, "Đã chọn 1 tin").is_positive());
            let focus = ctx.memory(|memory| memory.focused()).unwrap();
            let checked = ctx.read_response(focus).unwrap().rect;
            assert_eq!(checked.size(), vec2(44.0, 44.0));
            let older_y = text_rect(&selected, &older.text).center().y;
            let checkbox_at = egui::pos2(checked.center().x, older_y);
            for pressed in [true, false] {
                let _ = frame(
                    &mut app,
                    &ctx,
                    size,
                    pointer(checkbox_at, egui::PointerButton::Primary, pressed),
                );
            }
            let selected = frame(&mut app, &ctx, size, vec![]);
            assert!(text_rect(&selected, "Đã chọn 2 tin").is_positive());
            let copy_at = text_rect(&selected, "Sao chép").center();
            let _ = frame(
                &mut app,
                &ctx,
                size,
                pointer(copy_at, egui::PointerButton::Primary, true),
            );
            let copied = frame(
                &mut app,
                &ctx,
                size,
                pointer(copy_at, egui::PointerButton::Primary, false),
            );
            assert_eq!(
                copied.platform_output.copied_text,
                format!("{}\n\n{}", older.text, current.text)
            );
            assert!(app.message_selection.is_none());
            assert_eq!(app.drafts.active.text, draft);
            assert!(!app.drafts.active.in_flight());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn multiselect_escape_respects_ime_and_a_new_page_invalidates_selection() {
            let ctx = egui::Context::default();
            let size = vec2(640.0, 480.0);
            let (mut app, root) = fixture(&ctx);
            app.start_message_selection("copy-1");
            let _ = frame(&mut app, &ctx, size, vec![]);
            app.ime_composing = true;
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Escape));
            assert!(app.message_selection.is_some());
            app.ime_composing = false;
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Escape));
            assert!(app.message_selection.is_none());
            app.start_message_selection("copy-1");
            app.model.generation += 1;
            let _ = frame(&mut app, &ctx, size, vec![]);
            assert!(app.message_selection.is_none());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn multiselect_reveals_the_entry_row_once_and_leaves_scrolling_free() {
            let ctx = egui::Context::default();
            let size = vec2(640.0, 480.0);
            let (mut app, root) = fixture(&ctx);
            let first = app.model.messages[0].clone();
            for i in 1..=10 {
                let mut next = first.clone();
                next.id = format!("selection-overflow-{i}");
                next.text = format!("Tin tổng hợp thứ {i}");
                next.sent_at_ms += i * 1000;
                app.model.messages.push(next);
            }
            for _ in 0..3 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            app.start_message_selection(&first.id);
            for _ in 0..2 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            let out = frame(&mut app, &ctx, size, vec![]);
            assert!(text_rect(&out, "Đã chọn 1 tin").is_positive());
            assert!(text_rect(&out, &first.text).is_positive());
            let checked = ctx
                .memory(|memory| memory.focused())
                .and_then(|id| ctx.read_response(id))
                .unwrap();
            assert_eq!(checked.rect.size(), vec2(44.0, 44.0));

            let _ = frame(
                &mut app,
                &ctx,
                size,
                vec![
                    egui::Event::PointerMoved(egui::pos2(320.0, 230.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Page,
                        delta: vec2(0.0, -6.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..10 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            let out = frame(&mut app, &ctx, size, vec![]);
            assert!(text_rect(&out, "Đã chọn 1 tin").is_positive());
            assert!(text_rect(&out, "Tin tổng hợp thứ 10").is_positive());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn compact_sticker_picker_keyboard_dispatches_one_scoped_sticker_and_keeps_draft() {
            use crate::app::live::{LiveCommand, LiveConnection, LiveWorker};
            let ctx = egui::Context::default();
            let size = vec2(640.0, 480.0);
            let (mut app, root) = fixture(&ctx);
            let draft = app.drafts.active.text.clone();
            let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
            {
                let state = app.live.as_mut().unwrap();
                state.connection = LiveConnection::Online;
                state.worker = Some(worker);
                state.sticker_fixture_open = true;
                super::super::super::sticker_picker::fixture_thumbnails(&mut state.stickers, &ctx);
            }
            for _ in 0..3 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            for _ in 0..2 {
                let _ = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            }
            let out = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            assert!(text_rect(&out, "Mặc định").top() >= 8.0);
            let focus = ctx.memory(|memory| memory.focused()).unwrap();
            let rect = ctx.read_response(focus).unwrap().rect;
            assert_eq!(rect.width(), 44.0);
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(rect));
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Enter));
            let sends: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
                .filter(|command| matches!(command, LiveCommand::SendSticker { .. }))
                .collect();
            assert!(matches!(
                sends.as_slice(),
                [LiveCommand::SendSticker { thread, sticker, .. }]
                    if thread == "d:22" && *sticker == crate::domain::StickerDescriptor::builtin(1)
            ));
            assert_eq!(app.drafts.active.text, draft);
            assert!(!app.drafts.active.in_flight());
            assert!(!app.live.as_ref().unwrap().sticker_send.is_open());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn reaction_menu_keyboard_dispatch_uses_its_verified_scope_without_changing_draft_or_counts(
        ) {
            use crate::app::live::{
                LiveCommand, LiveConnection, LiveEvent, LiveWorker, ReactionAction, ReactionKind,
            };
            let ctx = egui::Context::default();
            let size = vec2(640.0, 480.0);
            let (mut app, root) = fixture(&ctx);
            let draft = app.drafts.active.text.clone();
            app.model.messages[0].id = "901".into();
            app.model.messages[0].source_client_id = Some("1901".into());
            let scope = app.reaction_scope().unwrap();
            let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
            {
                let state = app.live.as_mut().unwrap();
                state.connection = LiveConnection::Online;
                state.worker = Some(worker);
                state.reactions.seed(scope.clone(), "901");
                state.reactions.fixture = false;
                state
                    .reactions
                    .observe_outcome(&LiveEvent::ReactionCapability {
                        account_id: "11".into(),
                        available: true,
                    });
            }
            let _ = frame(&mut app, &ctx, size, vec![]);
            app.message_menu = Some(Popup::new(
                scope,
                "901".into(),
                egui::pos2(250.0, 70.0),
                None,
            ));
            for _ in 0..2 {
                let _ = frame(&mut app, &ctx, size, vec![]);
            }
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            let out = frame(&mut app, &ctx, size, key(egui::Key::Enter));
            let sends: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
                .filter(|command| matches!(command, LiveCommand::React { .. }))
                .collect();
            assert!(matches!(
                sends.as_slice(),
                [LiveCommand::React { request_id: 1, thread, message_id, action: ReactionAction::Add(ReactionKind::Heart) }]
                    if thread == "d:22" && message_id == "901"
            ));
            assert!(
                text_rect(&out, "3").is_positive(),
                "displayed count remains the saved projection"
            );
            assert_eq!(app.drafts.active.text, draft);
            assert!(!app.drafts.active.in_flight());
            assert!(app.message_menu.is_none());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn stale_account_or_thread_reply_never_dispatches_or_discards_the_draft() {
            for (account, thread) in [("33", "d:22"), ("11", "g:22")] {
                let ctx = egui::Context::default();
                let (mut app, root) = fixture(&ctx);
                let reply = crate::domain::ReplyReference {
                    account_id: account.into(),
                    thread_id: thread.into(),
                    message_id: "90001".into(),
                    sender: "22".into(),
                    text: "Trích dẫn phải được giữ lại".into(),
                };
                app.drafts.active.set_reply(reply.clone());
                let draft = app.drafts.active.text.clone();
                app.send_local();
                assert_eq!(app.drafts.active.reply(), Some(&reply));
                assert_eq!(app.drafts.active.text, draft);
                assert!(!app.drafts.active.in_flight());
                assert!(app.model.status.contains("không thuộc hội thoại đang mở"));
                drop(app);
                let _ = std::fs::remove_dir_all(root);
            }
        }

        #[test]
        fn unsupported_reply_buttons_explain_missing_references_media_and_ttl() {
            for (case, reason) in [
                (
                    "missing-cli",
                    "Tin nhắn đã lưu chưa có đủ thông tin gốc để trả lời.",
                ),
                ("media", "Hiện chỉ hỗ trợ trả lời tin nhắn văn bản."),
                ("ttl", "Loại tin nhắn này chưa hỗ trợ trả lời."),
            ] {
                let ctx = egui::Context::default();
                let (mut app, root) = fixture(&ctx);
                ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
                let message = &mut app.model.messages[0];
                message.id = "90001".into();
                message.source_client_id = Some("90002".into());
                match case {
                    "missing-cli" => message.source_client_id = None,
                    "media" => {
                        message.media = Some(MediaDescriptor::File(FileDescriptor {
                            title: "Báo cáo.pdf".into(),
                            size_bytes: Some(1024),
                            extension: "pdf".into(),
                            url: None,
                            encrypted: false,
                            is_folder: false,
                        }))
                    }
                    "ttl" => {
                        let payload = serde_json::json!({"error_code":0,"data":{"msgs":[{
                            "uidFrom":"22","idTo":"11","msgId":"90001","cliMsgId":"90002",
                            "ts":"1700000000000","msgType":"webchat","content":"Tin có thời hạn","ttl":60000
                        }]}});
                        *message = crate::protocol::messaging::incoming_messages("11", &payload)
                            .unwrap()
                            .remove(0)
                            .message;
                        assert!(message.reply_blocked);
                    }
                    _ => unreachable!(),
                }
                assert!(message.reply_reference().is_none());
                let draft = app.drafts.active.text.clone();
                let size = vec2(640.0, 480.0);
                let _ = frame(&mut app, &ctx, size, vec![]);
                app.message_menu = Some(Popup::new(
                    app.message_menu_scope().unwrap(),
                    "90001".into(),
                    egui::pos2(100.0, 120.0),
                    None,
                ));
                let _ = frame(&mut app, &ctx, size, vec![]);
                let output = frame(&mut app, &ctx, size, vec![]);
                let reply = text_rect(&output, "Trả lời").center();
                let _ = frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(reply)]);
                let _ = frame(&mut app, &ctx, size, vec![]);
                let output = frame(&mut app, &ctx, size, vec![]);
                assert!(
                    output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.job.text == reason)),
                    "Missing disabled explanation for {case}"
                );
                let _ = frame(
                    &mut app,
                    &ctx,
                    size,
                    pointer(reply, egui::PointerButton::Primary, true),
                );
                let _ = frame(
                    &mut app,
                    &ctx,
                    size,
                    pointer(reply, egui::PointerButton::Primary, false),
                );
                assert!(app.drafts.active.reply().is_none());
                assert_eq!(app.drafts.active.text, draft);
                assert!(!app.drafts.active.in_flight());
                drop(app);
                let _ = std::fs::remove_dir_all(root);
            }
        }

        #[test]
        fn reply_action_and_cancel_preserve_text_with_real_source_reference() {
            for dark in [false, true] {
                for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
                    let ctx = egui::Context::default();
                    let (mut app, root) = fixture(&ctx);
                    crate::theme::apply(
                        &ctx,
                        if dark {
                            crate::theme::ThemeMode::Dark
                        } else {
                            crate::theme::ThemeMode::Light
                        },
                    );
                    app.model.messages[0].id = "90001".into();
                    app.model.messages[0].source_client_id = Some("90002".into());
                    let draft = app.drafts.active.text.clone();
                    let _ = frame(&mut app, &ctx, size, vec![]);
                    app.message_menu = Some(Popup::new(
                        app.message_menu_scope().unwrap(),
                        "90001".into(),
                        egui::pos2(size.x - 20.0, size.y - 20.0),
                        None,
                    ));
                    let _ = frame(&mut app, &ctx, size, vec![]);
                    let output = frame(&mut app, &ctx, size, vec![]);
                    let reply = text_rect(&output, "Trả lời").center();
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(reply, egui::PointerButton::Primary, true),
                    );
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(reply, egui::PointerButton::Primary, false),
                    );
                    assert!(app.message_menu.is_none());
                    assert_eq!(app.drafts.active.reply().unwrap().message_id, "90001");
                    assert_eq!(app.drafts.active.text, draft);
                    let _ = frame(&mut app, &ctx, size, vec![]);
                    let output = frame(&mut app, &ctx, size, vec![]);
                    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    assert!(bounds.contains_rect(text_rect(&output, "Trả lời Hội thoại tổng hợp")));
                    assert!(bounds.contains_rect(text_rect(&output, "Gửi")));
                    let close = crate::theme::catalog::glyph("fa-close_24").unwrap();
                    let cancel = text_rect(&output, &close.text).center();
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(cancel, egui::PointerButton::Primary, true),
                    );
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(cancel, egui::PointerButton::Primary, false),
                    );
                    assert!(app.drafts.active.reply().is_none());
                    assert_eq!(app.drafts.active.text, draft);
                    assert!(!app.drafts.active.in_flight());
                    drop(app);
                    let _ = std::fs::remove_dir_all(root);
                }
            }
        }

        #[test]
        fn secondary_click_copies_only_on_action_and_preserves_draft_in_both_themes_and_sizes() {
            for dark in [false, true] {
                for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
                    let ctx = egui::Context::default();
                    let (mut app, root) = fixture(&ctx);
                    crate::theme::apply(
                        &ctx,
                        if dark {
                            crate::theme::ThemeMode::Dark
                        } else {
                            crate::theme::ThemeMode::Light
                        },
                    );
                    let body = app.model.messages[0].text.clone();
                    let draft = app.drafts.active.text.clone();
                    let _ = frame(&mut app, &ctx, size, vec![]);
                    let out = frame(&mut app, &ctx, size, vec![]);
                    let position = text_rect(&out, &body).center();
                    let out = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(position, egui::PointerButton::Secondary, true),
                    );
                    assert!(out.platform_output.copied_text.is_empty());
                    assert!(app.message_menu.is_some());
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(position, egui::PointerButton::Secondary, false),
                    );
                    let out = frame(&mut app, &ctx, size, vec![]);
                    let action = text_rect(&out, "Sao chép tin nhắn").center();
                    assert!(app.modal_blocked);
                    let _ = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(action, egui::PointerButton::Primary, true),
                    );
                    let out = frame(
                        &mut app,
                        &ctx,
                        size,
                        pointer(action, egui::PointerButton::Primary, false),
                    );
                    assert_eq!(out.platform_output.copied_text, body);
                    assert!(app.message_menu.is_none());
                    assert_eq!(app.drafts.active.text, draft);
                    assert_eq!(app.model.messages.len(), 1);
                    assert!(!app.drafts.active.in_flight());
                    drop(app);
                    let _ = std::fs::remove_dir_all(root);
                }
            }
        }

        #[test]
        fn stale_menu_cannot_copy_after_scope_privacy_or_modal_changes() {
            for change in 0..9 {
                let ctx = egui::Context::default();
                let (mut app, root) = fixture(&ctx);
                app.message_menu = Some(Popup::new(
                    app.message_menu_scope().unwrap(),
                    "copy-1".into(),
                    egui::pos2(300.0, 150.0),
                    None,
                ));
                match change {
                    0 => app.model.selected = Some("g:22".into()),
                    1 => app.model.generation += 1,
                    2 => app.live.as_mut().unwrap().account.as_mut().unwrap().id = "99".into(),
                    3 => app.model.messages.clear(),
                    4 => app.settings_open = true,
                    5 => app.live.as_mut().unwrap().login.signed_out(99),
                    6 => app.live.as_mut().unwrap().privacy = privacy::Privacy::default(),
                    7 => {
                        app.live.as_mut().unwrap().privacy.snapshot(
                            &crate::app::live::LiveEvent::PrivacyChanged {
                                account_id: "11".into(),
                                revision: 2,
                                status: crate::app::live::LivePrivacyStatus::Ready,
                                hidden_threads: vec!["d:22".into()],
                                unlocked_thread: None,
                            },
                        );
                    }
                    _ => app.live.as_mut().unwrap().logout_dialog = true,
                }
                let out = ctx.run(Default::default(), |ctx| app.show_message_menu(ctx));
                assert!(app.message_menu.is_none(), "change {change}");
                assert!(out.platform_output.copied_text.is_empty());
                drop(app);
                let _ = std::fs::remove_dir_all(root);
            }
        }

        #[test]
        fn tab_and_enter_copy_from_the_menu_without_sending_the_draft() {
            let ctx = egui::Context::default();
            let (mut app, root) = fixture(&ctx);
            let size = vec2(640.0, 480.0);
            let _ = frame(&mut app, &ctx, size, vec![]);
            app.message_menu = Some(Popup::new(
                app.message_menu_scope().unwrap(),
                "copy-1".into(),
                egui::pos2(300.0, 150.0),
                app.modal_focus_snapshot(&ctx),
            ));
            let _ = frame(&mut app, &ctx, size, vec![]);
            let _ = frame(&mut app, &ctx, size, vec![]);
            let key = |key| {
                vec![
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ]
            };
            let tab = frame(&mut app, &ctx, size, key(egui::Key::Tab));
            let focused = ctx
                .memory(|memory| memory.focused())
                .expect("Tab must focus a menu action");
            let focused_response = ctx.read_response(focused).unwrap();
            assert!(
                focused_response
                    .rect
                    .contains(text_rect(&tab, "Sao chép tin nhắn").center()),
                "Tab focused {:?}, enabled {}",
                focused_response.rect,
                focused_response.enabled()
            );
            let draft = app.drafts.active.text.clone();
            let out = frame(&mut app, &ctx, size, key(egui::Key::Enter));
            assert_eq!(out.platform_output.copied_text, app.model.messages[0].text);
            assert_eq!(app.drafts.active.text, draft);
            assert!(!app.drafts.active.in_flight());
            assert!(app.message_menu.is_none());
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn keyboard_navigation_wraps_between_name_and_link_in_both_themes() {
            for dark in [false, true] {
                let ctx = egui::Context::default();
                let (mut app, root) = fixture(&ctx);
                crate::theme::apply(
                    &ctx,
                    if dark {
                        crate::theme::ThemeMode::Dark
                    } else {
                        crate::theme::ThemeMode::Light
                    },
                );
                app.model.messages[0].media = Some(MediaDescriptor::File(FileDescriptor {
                    title: "Báo cáo.pdf".into(),
                    size_bytes: Some(10),
                    extension: "pdf".into(),
                    url: Some("https://example.com/report.pdf".into()),
                    encrypted: false,
                    is_folder: false,
                }));
                let size = vec2(640.0, 480.0);
                let _ = frame(&mut app, &ctx, size, vec![]);
                app.message_menu = Some(Popup::new(
                    app.message_menu_scope().unwrap(),
                    "copy-1".into(),
                    egui::pos2(620.0, 460.0),
                    app.modal_focus_snapshot(&ctx),
                ));
                let _ = frame(&mut app, &ctx, size, vec![]);
                let out = frame(&mut app, &ctx, size, vec![]);
                let name = text_rect(&out, "Sao chép tên tệp");
                let link = text_rect(&out, "Sao chép liên kết");
                assert!((name.left() - link.left()).abs() <= 1.0);
                assert!((link.center().y - name.center().y - ROW_HEIGHT).abs() <= 1.0);
                for (key, backward, label) in [
                    (egui::Key::Tab, false, "Sao chép tên tệp"),
                    (egui::Key::ArrowDown, false, "Sao chép liên kết"),
                    (egui::Key::ArrowUp, false, "Sao chép tên tệp"),
                    (egui::Key::Tab, true, "Sao chép liên kết"),
                ] {
                    let modifiers = if backward {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    };
                    let out = frame(
                        &mut app,
                        &ctx,
                        size,
                        vec![
                            egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers,
                            },
                            egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed: false,
                                repeat: false,
                                modifiers,
                            },
                        ],
                    );
                    let focused = ctx.memory(|memory| memory.focused()).unwrap();
                    let row = ctx.read_response(focused).unwrap().rect;
                    assert!(row.contains(text_rect(&out, label).center()));
                    assert!((row.height() - ROW_HEIGHT).abs() <= 1.0);
                    assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size)
                        .shrink(8.0)
                        .contains_rect(row));
                    assert!(out.platform_output.copied_text.is_empty());
                }
                let draft = app.drafts.active.text.clone();
                let out = frame(
                    &mut app,
                    &ctx,
                    size,
                    vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
                assert_eq!(
                    out.platform_output.copied_text,
                    "https://example.com/report.pdf"
                );
                assert_eq!(app.drafts.active.text, draft);
                assert!(!app.drafts.active.in_flight());
                drop(app);
                let _ = std::fs::remove_dir_all(root);
            }
        }

        #[test]
        fn escape_blocks_background_enter_and_popup_stays_inside_compact_window() {
            let ctx = egui::Context::default();
            let (mut app, root) = fixture(&ctx);
            let size = vec2(640.0, 480.0);
            let _ = frame(&mut app, &ctx, size, vec![]);
            let focus = app.modal_focus_snapshot(&ctx);
            app.message_menu = Some(Popup::new(
                app.message_menu_scope().unwrap(),
                "copy-1".into(),
                egui::pos2(638.0, 478.0),
                focus,
            ));
            let _ = frame(&mut app, &ctx, size, vec![]);
            let out = frame(&mut app, &ctx, size, vec![]);
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size)
                .contains_rect(text_rect(&out, "Sao chép tin nhắn")));
            let draft = app.drafts.active.text.clone();
            let key = |key| {
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }]
            };
            let out = frame(&mut app, &ctx, size, key(egui::Key::Enter));
            assert!(out.platform_output.copied_text.is_empty());
            assert_eq!(app.drafts.active.text, draft);
            assert!(!app.drafts.active.in_flight());
            let _ = frame(&mut app, &ctx, size, key(egui::Key::Escape));
            assert!(app.message_menu.is_none());
            let _ = frame(&mut app, &ctx, size, vec![]);
            assert!(!app.modal_blocked);
            assert_eq!(app.drafts.active.text, draft);
            drop(app);
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
