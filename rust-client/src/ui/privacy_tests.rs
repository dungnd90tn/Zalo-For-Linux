//! Independent hidden-conversation presentation and stale-capability regressions.
use super::*;
use crate::app::live::{
    HiddenCandidate, HiddenUnlockOutcome, LiveCommand, LiveEvent, LivePrivacyStatus, LiveWorker,
};

const PRIVATE_TITLE: &str = "Nguyễn Thị Ánh · hội thoại ẩn";
const PRIVATE_BODY: &str = "Nội dung riêng chỉ xuất hiện sau PIN đúng";
const DRAFT: &str = "Bản nháp riêng được giữ lại";

fn conversation(id: &str, title: &str) -> Conversation {
    Conversation {
        id: id.into(),
        title: title.into(),
        last_message_at_ms: 1_700_000_000_000,
        last_text: PRIVATE_BODY.into(),
        last_sender: "22".into(),
    }
}
fn message() -> Message {
    Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: "private-message".into(),
        sender: "22".into(),
        text: PRIVATE_BODY.into(),
        sticker: None,
        media: None,
        sent_at_ms: 1_700_000_000_000,
    }
}
fn apply(app: &mut NativeApp, ctx: &egui::Context, event: LiveEvent) {
    let mut state = app.live.take().unwrap();
    app.apply_live_event(&mut state, event, ctx);
    app.live = Some(state);
}
fn policy(
    app: &mut NativeApp,
    ctx: &egui::Context,
    revision: u64,
    status: LivePrivacyStatus,
    unlocked: Option<&str>,
) {
    apply(
        app,
        ctx,
        LiveEvent::PrivacyChanged {
            account_id: "11".into(),
            revision,
            status,
            hidden_threads: vec!["d:22".into()],
            unlocked_thread: unlocked.map(str::to_owned),
        },
    );
}
fn candidate() -> HiddenCandidate {
    HiddenCandidate {
        thread_id: "d:22".into(),
        title: PRIVATE_TITLE.into(),
    }
}
fn prompt(app: &mut NativeApp) {
    let state = app.live.as_mut().unwrap();
    state
        .privacy
        .begin_prompt("11".into(), state.login.generation(), candidate());
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
            ctx.input(|input| update_ime_state(&mut app.ime_composing, &input.events));
            app.update_modal_input(ctx);
            app.workspace(ctx);
        },
    )
}

#[test]
fn hidden_name_search_is_minimal_and_its_pointer_action_requires_the_pin_dialog() {
    for (mode, size) in [
        (crate::theme::ThemeMode::Light, vec2(1200.0, 780.0)),
        (crate::theme::ThemeMode::Dark, vec2(640.0, 480.0)),
    ] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        app.theme.set_mode(mode);
        app.theme.poll(&ctx);
        app.compact_list = size.x < COMPACT_BREAKPOINT;
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.live.as_mut().unwrap().worker = Some(worker);
        policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
        let output = settle(&mut app, &ctx, size);
        tap(
            &mut app,
            &ctx,
            size,
            text_rect(&output, "Tìm kiếm").center(),
        );
        frame(&mut app, &ctx, size, vec![egui::Event::Text("Ánh".into())]);
        app.poll_live(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.poll_live(&ctx);
        let LiveCommand::SearchHidden { generation, query } = commands.try_recv().unwrap() else {
            panic!("Typing a nonempty name must request only the bounded hidden-name query");
        };
        assert_eq!(query, "Ánh");
        apply(
            &mut app,
            &ctx,
            LiveEvent::HiddenMatches {
                account_id: "11".into(),
                generation: generation.wrapping_add(1),
                revision: 2,
                query: query.clone(),
                candidates: vec![candidate()],
            },
        );
        assert!(app.live.as_ref().unwrap().privacy.candidates.is_empty());
        apply(
            &mut app,
            &ctx,
            LiveEvent::HiddenMatches {
                account_id: "11".into(),
                generation,
                revision: 2,
                query,
                candidates: vec![candidate()],
            },
        );
        let output = settle(&mut app, &ctx, size);
        let name = text_rect(&output, PRIVATE_TITLE);
        assert!(texts(&output)
            .iter()
            .all(|text| !text.contains(PRIVATE_BODY)));
        text_rect(&output, "0 hội thoại");
        let lock = crate::theme::catalog::glyph("fa-icon-outline-lock").unwrap();
        assert!(texts(&output).contains(&lock.text.as_str()));
        tap(&mut app, &ctx, size, name.center());
        let output = settle(&mut app, &ctx, size);
        text_rect(&output, "Mở hội thoại ẩn");
        assert!(app
            .live
            .as_ref()
            .unwrap()
            .privacy
            .prompt
            .as_ref()
            .unwrap()
            .pin
            .is_empty());
        assert!(app.model.selected.is_none() && app.model.messages.is_empty());
        assert!(
            commands.try_recv().is_err(),
            "A locked-name click must not load or open history"
        );
        cleanup(app, root);
    }
}

#[test]
fn opening_a_known_hidden_id_routes_to_pin_without_a_load_or_stuck_spinner() {
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.live.as_mut().unwrap().worker = Some(worker);
        app.compact_list = size.x < COMPACT_BREAKPOINT;
        policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
        let output = settle(&mut app, &ctx, size);
        let plus = crate::theme::catalog::glyph("fa-outline-plus").unwrap();
        tap(
            &mut app,
            &ctx,
            size,
            text_rect(&output, &plus.text).center(),
        );
        let output = settle(&mut app, &ctx, size);
        tap(
            &mut app,
            &ctx,
            size,
            text_rect(&output, "ID người nhận").center(),
        );
        frame(&mut app, &ctx, size, vec![egui::Event::Text("22".into())]);
        let output = settle(&mut app, &ctx, size);
        tap(
            &mut app,
            &ctx,
            size,
            text_rect(&output, "Mở hội thoại").center(),
        );
        let output = settle(&mut app, &ctx, size);
        text_rect(&output, "Mở hội thoại ẩn");
        let state = app.live.as_ref().unwrap();
        assert!(!state.open_dialog);
        assert_eq!(state.privacy.prompt.as_ref().unwrap().thread, "d:22");
        assert!(
            !app.model.pending && app.model.selected.is_none() && app.model.messages.is_empty()
        );
        assert!(
            commands.try_recv().is_err(),
            "Knowing a hidden ID must not bypass PIN authorization"
        );
        cleanup(app, root);
    }
}

#[test]
fn pin_enter_cannot_verify_during_composition_or_a_repeated_key() {
    let ctx = egui::Context::default();
    let (mut app, root) = live::tests::fixture(&ctx);
    let size = vec2(640.0, 480.0);
    let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
    app.live.as_mut().unwrap().worker = Some(worker);
    policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
    prompt(&mut app);
    settle(&mut app, &ctx, size);
    let editor = ctx.memory(|memory| memory.focused()).unwrap();
    frame(&mut app, &ctx, size, vec![egui::Event::Text("12".into())]);
    // egui derives `repeat` from held keys; establish an actual held key first.
    frame(
        &mut app,
        &ctx,
        size,
        vec![enter(egui::Modifiers::NONE).remove(0)],
    );
    ctx.memory_mut(|memory| memory.request_focus(editor));
    frame(&mut app, &ctx, size, vec![egui::Event::Text("34".into())]);
    let mut repeated = enter(egui::Modifiers::NONE);
    if let egui::Event::Key { repeat, .. } = &mut repeated[0] {
        *repeat = true;
    }
    frame(&mut app, &ctx, size, repeated);
    let repeated_action = commands.try_recv();
    assert!(
        repeated_action.is_err(),
        "Repeated Enter dispatched {repeated_action:?}"
    );
    ctx.memory_mut(|memory| memory.request_focus(editor));
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("1".into()))],
    );
    frame(&mut app, &ctx, size, enter(egui::Modifiers::NONE));
    assert!(commands.try_recv().is_err());
    assert!(app
        .live
        .as_ref()
        .unwrap()
        .privacy
        .prompt
        .as_ref()
        .unwrap()
        .pending
        .is_none());
    frame(
        &mut app,
        &ctx,
        size,
        vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
    );
    let output = settle(&mut app, &ctx, size);
    tap(&mut app, &ctx, size, text_rect(&output, "Hủy").center());
    assert!(app.live.as_ref().unwrap().privacy.prompt.is_none());
    cleanup(app, root);
}

#[test]
fn unknown_hidden_names_use_a_neutral_label_in_the_row_and_pin_dialog() {
    for title in ["d:22", "   "] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let size = vec2(640.0, 480.0);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.live.as_mut().unwrap().worker = Some(worker);
        policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
        app.compact_list = true;
        app.search = "22".into();
        app.poll_live(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.poll_live(&ctx);
        let LiveCommand::SearchHidden { generation, query } = commands.try_recv().unwrap() else {
            panic!("Expected a bounded hidden-name lookup");
        };
        apply(
            &mut app,
            &ctx,
            LiveEvent::HiddenMatches {
                account_id: "11".into(),
                generation,
                revision: 2,
                query,
                candidates: vec![HiddenCandidate {
                    thread_id: "d:22".into(),
                    title: title.into(),
                }],
            },
        );
        let output = settle(&mut app, &ctx, size);
        assert!(texts(&output).iter().all(|text| !text.contains("d:22")));
        tap(
            &mut app,
            &ctx,
            size,
            text_rect(&output, "Hội thoại ẩn").center(),
        );
        let output = settle(&mut app, &ctx, size);
        text_rect(&output, "Mở hội thoại ẩn");
        text_rect(&output, "Hội thoại ẩn");
        assert!(texts(&output).iter().all(|text| !text.contains("d:22")));
        cleanup(app, root);
    }
}
fn settle(app: &mut NativeApp, ctx: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    for _ in 0..3 {
        frame(app, ctx, size, vec![]);
    }
    frame(app, ctx, size, vec![])
}
fn texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect()
}
fn text_rect(output: &egui::FullOutput, value: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == value => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let geometry: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == value => {
                        Some((text.pos, text.galley.size(), shape.clip_rect))
                    }
                    _ => None,
                })
                .collect();
            panic!(
                "Missing fully visible label {value:?}; geometry={geometry:?}; {:?}",
                texts(output)
            )
        })
}
fn tap(app: &mut NativeApp, ctx: &egui::Context, size: egui::Vec2, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn enter(modifiers: egui::Modifiers) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}
fn cleanup(app: NativeApp, root: std::path::PathBuf) {
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_and_failed_policy_clear_cached_content_before_late_event_consumers() {
    for status in [LivePrivacyStatus::Checking, LivePrivacyStatus::Failed] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.model.conversations = vec![conversation("d:22", PRIVATE_TITLE)];
        app.model.messages = vec![message()];
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = DRAFT.into();
        let key = crate::app::media::MediaKey {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: "photo".into(),
            url: "https://photo-talk.zadn.vn/synthetic.jpg".into(),
        };
        let seeded = ctx.run(egui::RawInput::default(), |ctx| {
            app.live
                .as_mut()
                .unwrap()
                .media
                .seed(ctx, key.clone(), 0, 2, 2, vec![255; 16]);
        });
        let texture_ids: Vec<_> = seeded
            .textures_delta
            .set
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| *id != egui::TextureId::Managed(0))
            .collect();
        assert!(!texture_ids.is_empty());
        policy(&mut app, &ctx, 2, status, None);
        assert!(app.model.selected.is_none());
        assert!(app.model.messages.is_empty() && app.model.conversations.is_empty());
        let generation = app.model.generation;
        for event in [
            LiveEvent::Ready {
                account_id: "11".into(),
                conversations: vec![conversation("d:22", PRIVATE_TITLE)],
            },
            LiveEvent::Received {
                account_id: "11".into(),
                message: message(),
                conversation: conversation("d:22", PRIVATE_TITLE),
            },
            LiveEvent::Page {
                account_id: "11".into(),
                generation,
                thread: "d:22".into(),
                messages: vec![message()],
                has_older: false,
            },
            LiveEvent::SenderNames {
                account_id: "11".into(),
                generation,
                thread: "d:22".into(),
                names: vec![crate::store::directory::SenderName {
                    sender_id: "22".into(),
                    display_name: PRIVATE_TITLE.into(),
                }],
            },
            LiveEvent::MessagesRefreshed {
                account_id: "11".into(),
                generation,
                thread: "d:22".into(),
                messages: vec![message()],
            },
        ] {
            apply(&mut app, &ctx, event);
        }
        assert!(app.model.messages.is_empty() && app.model.conversations.is_empty());
        let first = frame(&mut app, &ctx, vec2(640.0, 480.0), vec![]);
        for id in texture_ids {
            assert!(
                first.textures_delta.free.contains(&id),
                "private GPU texture must be retired"
            );
        }
        for size in [vec2(640.0, 480.0), vec2(1200.0, 780.0)] {
            let output = settle(&mut app, &ctx, size);
            let status_label = match status {
                LivePrivacyStatus::Failed => "Chưa xác minh được trạng thái riêng tư của hội thoại",
                _ => "Đang xác minh trạng thái riêng tư của hội thoại",
            };
            for label in [status_label, "Nội dung được giữ kín đến khi kiểm tra xong. Bấm avatar để xem trạng thái hoặc thử lại."] {
                assert_eq!(texts(&output).iter().filter(|text| **text == label).count(), 1);
                assert!(egui::Rect::from_min_max(egui::pos2(64.0, 0.0), size.to_pos2())
                    .contains_rect(text_rect(&output, label)));
            }
            assert!(texts(&output)
                .iter()
                .all(|text| !text.contains(PRIVATE_TITLE)
                    && !text.contains(PRIVATE_BODY)
                    && !text.contains(DRAFT)));
        }
        app.drafts.select("d:22").unwrap();
        assert_eq!(app.drafts.active.text, DRAFT);
        cleanup(app, root);
    }
}

#[test]
fn pin_dialog_is_masked_bounded_and_cancel_invalidates_a_pending_unlock() {
    for size in [vec2(640.0, 480.0), vec2(1200.0, 780.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.live.as_mut().unwrap().worker = Some(worker);
        policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
        prompt(&mut app);
        let output = settle(&mut app, &ctx, size);
        for label in ["Mở hội thoại ẩn", PRIVATE_TITLE, "Hủy", "Mở hội thoại"] {
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size)
                .contains_rect(text_rect(&output, label)));
        }
        frame(&mut app, &ctx, size, vec![egui::Event::Text("1234".into())]);
        let output = settle(&mut app, &ctx, size);
        assert!(texts(&output).iter().all(|text| !text.contains("1234")));
        let ctrl = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        frame(&mut app, &ctx, size, enter(ctrl));
        assert!(
            commands.try_recv().is_err(),
            "Ctrl+Enter behind a PIN modal cannot dispatch a send/unlock"
        );
        frame(&mut app, &ctx, size, enter(egui::Modifiers::NONE));
        let request = commands.try_recv().unwrap();
        let generation = match request {
            LiveCommand::UnlockHidden {
                generation,
                revision,
                thread,
                pin,
            } => {
                assert_eq!((revision, thread.as_str()), (2, "d:22"));
                assert!(!format!("{pin:?}").contains("1234"));
                generation
            }
            other => panic!("Unexpected PIN action: {other:?}"),
        };
        assert!(app
            .live
            .as_ref()
            .unwrap()
            .privacy
            .prompt
            .as_ref()
            .unwrap()
            .pin
            .is_empty());
        let output = settle(&mut app, &ctx, size);
        let cancel = text_rect(&output, "Hủy").center();
        tap(&mut app, &ctx, size, cancel);
        assert!(matches!(
            commands.try_recv().unwrap(),
            LiveCommand::RelockHidden
        ));
        assert!(app.live.as_ref().unwrap().privacy.prompt.is_none());
        policy(&mut app, &ctx, 3, LivePrivacyStatus::Ready, Some("d:22"));
        apply(
            &mut app,
            &ctx,
            LiveEvent::HiddenUnlockResult {
                account_id: "11".into(),
                generation,
                revision: 3,
                thread: "d:22".into(),
                outcome: HiddenUnlockOutcome::Unlocked,
            },
        );
        assert!(!app.privacy_visible("d:22") && app.model.selected.is_none());
        cleanup(app, root);
    }
}

#[test]
fn wrong_pin_and_stale_unlock_results_never_authorize_history() {
    for stale in [
        "wrong-pin",
        "account",
        "auth",
        "worker",
        "request",
        "revision",
        "thread",
    ] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
        prompt(&mut app);
        app.live
            .as_mut()
            .unwrap()
            .privacy
            .prompt
            .as_mut()
            .unwrap()
            .pending = Some(7);
        if stale != "wrong-pin" {
            policy(&mut app, &ctx, 3, LivePrivacyStatus::Ready, Some("d:22"));
        }
        if stale == "auth" {
            let state = app.live.as_mut().unwrap();
            state.login.start(state.login.generation() + 1, false);
        }
        if stale == "worker" {
            app.live.as_mut().unwrap().privacy.reset_worker();
        }
        apply(
            &mut app,
            &ctx,
            LiveEvent::HiddenUnlockResult {
                account_id: if stale == "account" { "33" } else { "11" }.into(),
                generation: if stale == "request" { 8 } else { 7 },
                revision: if matches!(stale, "wrong-pin" | "revision") {
                    2
                } else {
                    3
                },
                thread: if stale == "thread" { "g:22" } else { "d:22" }.into(),
                outcome: if stale == "wrong-pin" {
                    HiddenUnlockOutcome::IncorrectPin
                } else {
                    HiddenUnlockOutcome::Unlocked
                },
            },
        );
        assert!(app.model.selected.is_none(), "stale result {stale}");
        assert!(!app.privacy_visible("d:22"), "stale result {stale}");
        if stale == "wrong-pin" {
            let output = settle(&mut app, &ctx, vec2(640.0, 480.0));
            text_rect(&output, "Mã PIN không đúng. Vui lòng thử lại.");
        }
        cleanup(app, root);
    }
}

#[test]
fn verified_hidden_thread_keeps_header_facts_but_never_enters_normal_recents() {
    let ctx = egui::Context::default();
    let (mut app, root) = live::tests::fixture(&ctx);
    policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
    prompt(&mut app);
    app.live
        .as_mut()
        .unwrap()
        .privacy
        .prompt
        .as_mut()
        .unwrap()
        .pending = Some(7);
    policy(&mut app, &ctx, 3, LivePrivacyStatus::Ready, Some("d:22"));
    apply(
        &mut app,
        &ctx,
        LiveEvent::HiddenUnlockResult {
            account_id: "11".into(),
            generation: 7,
            revision: 3,
            thread: "d:22".into(),
            outcome: HiddenUnlockOutcome::Unlocked,
        },
    );
    assert_eq!(app.model.selected.as_deref(), Some("d:22"));
    apply(
        &mut app,
        &ctx,
        LiveEvent::Opened {
            account_id: "11".into(),
            generation: 7,
            conversation: conversation("d:22", PRIVATE_TITLE),
        },
    );
    apply(
        &mut app,
        &ctx,
        LiveEvent::Page {
            account_id: "11".into(),
            generation: 7,
            thread: "d:22".into(),
            messages: vec![message()],
            has_older: false,
        },
    );
    apply(
        &mut app,
        &ctx,
        LiveEvent::DirectoryChanged {
            account_id: "11".into(),
            conversations: vec![conversation("d:44", "Hội thoại thường")],
        },
    );
    assert_eq!(
        app.visible_conversation("d:22").unwrap().title,
        PRIVATE_TITLE
    );
    assert_eq!(app.model.messages, vec![message()]);
    assert_eq!(app.model.conversations.len(), 1);
    assert_eq!(app.model.conversations[0].id, "d:44");
    app.drafts.active.text = DRAFT.into();
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let output = settle(&mut app, &ctx, size);
        text_rect(&output, PRIVATE_BODY);
    }
    app.relock_hidden();
    assert!(app.model.selected.is_none() && app.model.messages.is_empty());
    app.drafts.select("d:22").unwrap();
    assert_eq!(app.drafts.active.text, DRAFT);
    cleanup(app, root);
}

#[test]
fn locked_ack_settles_the_matching_draft_without_reintroducing_private_content() {
    for content_free in [false, true] {
        for newer_edit in [false, true] {
            let ctx = egui::Context::default();
            let (mut app, root) = live::tests::fixture(&ctx);
            app.model.selected = Some("d:22".into());
            app.drafts.select("d:22").unwrap();
            app.drafts.active.text = DRAFT.into();
            let mut request_id = 0;
            app.drafts
                .active
                .send_live(0, "d:22".into(), |command| {
                    let LiveCommand::Send { request_id: id, .. } = command else {
                        unreachable!()
                    };
                    request_id = id;
                    Ok(())
                })
                .unwrap();
            if newer_edit {
                app.drafts.active.text = "Bản nháp sửa sau khi gửi".into();
                app.drafts.active.edited();
            }
            policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
            let event = if content_free {
                LiveEvent::SendSettled {
                    account_id: "11".into(),
                    thread: "d:22".into(),
                    request_id,
                    accepted: true,
                }
            } else {
                LiveEvent::Sent {
                    account_id: "11".into(),
                    generation: 0,
                    thread: "d:22".into(),
                    request_id,
                    message: message(),
                    conversation: conversation("d:22", PRIVATE_TITLE),
                }
            };
            apply(&mut app, &ctx, event);
            assert!(
                app.model.messages.is_empty()
                    && app.model.conversations.is_empty()
                    && app.model.selected.is_none()
            );
            app.drafts.select("d:22").unwrap();
            assert!(!app.drafts.active.in_flight());
            assert_eq!(
                app.drafts.active.text,
                if newer_edit {
                    "Bản nháp sửa sau khi gửi"
                } else {
                    ""
                }
            );
            cleanup(app, root);
        }
    }
}

#[test]
fn worker_loss_or_expiration_revokes_pending_pin_and_the_current_hidden_lease() {
    for expired in [false, true] {
        for opened in [false, true] {
            let ctx = egui::Context::default();
            let (mut app, root) = live::tests::fixture(&ctx);
            let (worker, _commands, events) = LiveWorker::presentation_fixture("11");
            app.live.as_mut().unwrap().worker = Some(worker);
            policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
            prompt(&mut app);
            app.live
                .as_mut()
                .unwrap()
                .privacy
                .prompt
                .as_mut()
                .unwrap()
                .pending = Some(7);
            let draft_thread = if opened {
                policy(&mut app, &ctx, 3, LivePrivacyStatus::Ready, Some("d:22"));
                apply(
                    &mut app,
                    &ctx,
                    LiveEvent::HiddenUnlockResult {
                        account_id: "11".into(),
                        generation: 7,
                        revision: 3,
                        thread: "d:22".into(),
                        outcome: HiddenUnlockOutcome::Unlocked,
                    },
                );
                apply(
                    &mut app,
                    &ctx,
                    LiveEvent::Opened {
                        account_id: "11".into(),
                        generation: 7,
                        conversation: conversation("d:22", PRIVATE_TITLE),
                    },
                );
                let generation = app.model.generation;
                apply(
                    &mut app,
                    &ctx,
                    LiveEvent::Page {
                        account_id: "11".into(),
                        generation,
                        thread: "d:22".into(),
                        messages: vec![message()],
                        has_older: false,
                    },
                );
                assert!(app.privacy_visible("d:22") && !app.model.messages.is_empty());
                "d:22"
            } else {
                app.model.selected = Some("d:33".into());
                "d:33"
            };
            app.drafts.select(draft_thread).unwrap();
            app.drafts.active.text = DRAFT.into();
            if expired {
                apply(
                    &mut app,
                    &ctx,
                    LiveEvent::Connection {
                        account_id: "11".into(),
                        state: crate::app::live::LiveConnection::Expired,
                    },
                );
            } else {
                drop(events);
                app.poll_live(&ctx);
            }
            let state = app.live.as_ref().unwrap();
            assert!(state.worker.is_none() && state.privacy.prompt.is_none());
            assert_eq!(state.privacy.status, LivePrivacyStatus::Failed);
            assert!(!app.privacy_visible("d:22"));
            assert!(app.model.selected.is_none() && app.model.messages.is_empty());
            apply(
                &mut app,
                &ctx,
                LiveEvent::HiddenUnlockResult {
                    account_id: "11".into(),
                    generation: 7,
                    revision: 3,
                    thread: "d:22".into(),
                    outcome: HiddenUnlockOutcome::Unlocked,
                },
            );
            assert!(app.model.selected.is_none() && !app.privacy_visible("d:22"));
            let output = settle(&mut app, &ctx, vec2(640.0, 480.0));
            assert!(texts(&output)
                .iter()
                .all(|text| !text.contains(PRIVATE_BODY) && !text.contains(DRAFT)));
            app.drafts.select(draft_thread).unwrap();
            assert_eq!(app.drafts.active.text, DRAFT);
            cleanup(app, root);
        }
    }
}

#[test]
fn outside_click_after_an_incorrect_pin_cannot_raise_the_curtain_above_cancel_or_close() {
    for close_icon in [false, true] {
        for size in [vec2(640.0, 480.0), vec2(1200.0, 780.0)] {
            let ctx = egui::Context::default();
            let (mut app, root) = live::tests::fixture(&ctx);
            policy(&mut app, &ctx, 2, LivePrivacyStatus::Ready, None);
            prompt(&mut app);
            app.live
                .as_mut()
                .unwrap()
                .privacy
                .prompt
                .as_mut()
                .unwrap()
                .pending = Some(7);
            apply(
                &mut app,
                &ctx,
                LiveEvent::HiddenUnlockResult {
                    account_id: "11".into(),
                    generation: 7,
                    revision: 2,
                    thread: "d:22".into(),
                    outcome: HiddenUnlockOutcome::IncorrectPin,
                },
            );
            let output = settle(&mut app, &ctx, size);
            text_rect(&output, "Mã PIN không đúng. Vui lòng thử lại.");
            // Click the disabled rail beneath the curtain; it must neither navigate
            // nor lift the full-screen curtain above the dialog's own controls.
            tap(&mut app, &ctx, size, pos2(32.0, 120.0));
            let output = settle(&mut app, &ctx, size);
            assert!(app.live.as_ref().unwrap().privacy.prompt.is_some());
            assert!(app.model.selected.is_none() && app.model.messages.is_empty());
            let action = if close_icon {
                let close = crate::theme::catalog::glyph("fa-close_24").unwrap();
                text_rect(&output, &close.text).center()
            } else {
                text_rect(&output, "Hủy").center()
            };
            assert_eq!(
                ctx.layer_id_at(action),
                Some(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("hidden_conversation_pin")
                ))
            );
            tap(&mut app, &ctx, size, action);
            let output = settle(&mut app, &ctx, size);
            assert!(app.live.as_ref().unwrap().privacy.prompt.is_none());
            assert!(!app.modal_blocked && !app.modal_open());
            assert!(!texts(&output).contains(&"Mở hội thoại ẩn"));
            cleanup(app, root);
        }
    }
}
