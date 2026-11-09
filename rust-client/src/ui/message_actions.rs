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
        result.push(Action { label, text });
    }
    if let Some(text) = link.and_then(external_url) {
        result.push(Action {
            label: "Sao chép liên kết",
            text,
        });
    }
    result
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

impl NativeApp {
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
            let open =
                open || state.open_dialog || state.review_dialog || state.privacy.prompt.is_some();
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
        let Some(message) = self.menu_message(&popup) else {
            return;
        };
        let actions = actions(message);
        if actions.is_empty() {
            self.message_menu = None;
            return;
        }
        self.latch_modal_input(ctx);
        let width = 244.0_f32.min(ctx.screen_rect().width() - 32.0);
        let mut copied = false;
        // egui 0.29 lets disabled background widgets consume the automatic Tab stop.
        // Keep navigation within the visible actions and assign focus after rendering them.
        let tab = ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                Some(false)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                Some(true)
            } else {
                None
            }
        });
        // Shift+Tab may leave an automatic focus request for the next frame, pointing
        // at a disabled background widget. Reconcile it with this popup's scoped action.
        if let Some(id) = popup.action_focus {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
        let focus = popup
            .action_focus
            .or_else(|| ctx.memory(|memory| memory.focused()));
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
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(width);
                for action in actions {
                    let button = ui.add_sized(vec2(width, 44.0), egui::Button::new(action.label));
                    buttons.push(button.id);
                    if button.clicked() {
                        ctx.copy_text(action.text);
                        copied = true;
                    }
                }
            });
        });
        if let Some(forward) = tab {
            let current = buttons.iter().position(|id| Some(*id) == focus);
            let next = match (current, forward) {
                (Some(index), true) => (index + 1) % buttons.len(),
                (Some(index), false) => (index + buttons.len() - 1) % buttons.len(),
                (None, true) => 0,
                (None, false) => buttons.len() - 1,
            };
            ctx.memory_mut(|memory| memory.request_focus(buttons[next]));
            if let Some(popup) = &mut self.message_menu {
                popup.action_focus = Some(buttons[next]);
            }
        }
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if copied || escape || (!popup.just_opened && response.response.clicked_elsewhere()) {
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
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.sync_message_menu();
                    app.update_modal_input(ctx);
                    app.workspace(ctx);
                },
            )
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
                    egui::pos2(300.0, 150.0),
                    app.modal_focus_snapshot(&ctx),
                ));
                let _ = frame(&mut app, &ctx, size, vec![]);
                let _ = frame(&mut app, &ctx, size, vec![]);
                for (backward, label) in [
                    (false, "Sao chép tên tệp"),
                    (false, "Sao chép liên kết"),
                    (true, "Sao chép tên tệp"),
                    (true, "Sao chép liên kết"),
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
                                key: egui::Key::Tab,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers,
                            },
                            egui::Event::Key {
                                key: egui::Key::Tab,
                                physical_key: None,
                                pressed: false,
                                repeat: false,
                                modifiers,
                            },
                        ],
                    );
                    let focused = ctx.memory(|memory| memory.focused()).unwrap();
                    assert!(ctx
                        .read_response(focused)
                        .unwrap()
                        .rect
                        .contains(text_rect(&out, label).center()));
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
