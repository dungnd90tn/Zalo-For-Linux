//! Independent interaction regressions using disposable, network-free UI state.
use super::*;
use std::time::{Duration, Instant};

fn frame(
    app: &mut NativeApp,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(width, 480.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            app.update_modal_input(ctx);
            app.workspace(ctx);
            if app.settings_open {
                app.settings
                    .show(ctx, &mut app.settings_open, &mut app.theme);
            }
        },
    )
}

fn text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
            Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
        }
        _ => None,
    })
}

fn click(position: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn submit_key() -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        })
        .collect()
}

fn conversation(id: &str, title: &str) -> Conversation {
    Conversation {
        id: id.into(),
        title: title.into(),
        last_message_at_ms: 0,
        last_text: String::new(),
        last_sender: String::new(),
    }
}

#[test]
fn signed_out_parent_keeps_qr_tabs_and_cancel_visible_below_the_settings_header() {
    for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let state = app.live.as_mut().unwrap();
        state.account = None;
        state.login.start(1, false);
        assert!(state.login.pixels(&ctx, 1, 2, 2, &[255; 16]));
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    assert!(app.show_login(ctx));
                },
            );
        }
        for label in [
            "Zalo",
            "VỚI MÃ QR",
            "VỚI SỐ ĐIỆN THOẠI",
            "Quét mã QR bằng Zalo",
            "Hủy đăng nhập",
        ] {
            let rect =
                text_rect(&output, label).unwrap_or_else(|| panic!("Missing {label} at {size:?}"));
            assert!(
                rect.top() >= 32.0
                    && rect.bottom() <= size.y
                    && rect.left() >= 0.0
                    && rect.right() <= size.x,
                "Login control {label} must fit below the fixed header: {rect:?}"
            );
        }
        assert!(
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) =>
                    matches!(mesh.texture_id,egui::TextureId::Managed(id) if id > 0)
                        && mesh
                            .vertices
                            .iter()
                            .all(|vertex| vertex.pos.y >= 32.0 && vertex.pos.y <= size.y),
                _ => false,
            }),
            "The supplied QR texture must actually be painted inside the viewport"
        );
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn modal_blocks_background_pointer_and_submit_then_restores_only_matching_editor() {
    let ctx = egui::Context::default();
    let (mut app, root) = live::tests::fixture(&ctx);
    app.live = None;
    app.demo = true;
    app.worker = Some(
        Worker::demo(
            app._profile.paths.data_dir.join("modal-demo.sqlite3"),
            Arc::new(|| {}),
        )
        .unwrap(),
    );
    app.model.conversations = vec![
        conversation("d:22", "Hội thoại A"),
        conversation("d:33", "Hội thoại B"),
    ];
    app.model.selected = Some("d:22".into());
    app.drafts.select("d:22").unwrap();
    app.drafts.active.text = "Bản nháp chỉ lưu sau khi đóng hộp thoại".into();
    app.focus_composer = true;
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    let editor = ctx.memory(|memory| memory.focused()).unwrap();
    let original = app.drafts.active.text.clone();

    app.settings_open = true;
    let output = frame(&mut app, &ctx, 1200.0, submit_key());
    assert!(app.modal_blocked);
    assert!(!app.drafts.active.in_flight());
    let send = text_rect(&output, "Lưu").unwrap().center();
    let _ = frame(&mut app, &ctx, 1200.0, click(send));
    let _ = frame(&mut app, &ctx, 1200.0, click(pos2(160.0, 230.0)));
    app.send_local(); // The dispatch boundary also rejects accidental callers.
    assert_eq!(app.model.selected.as_deref(), Some("d:22"));
    assert_eq!(app.drafts.active.text, original);
    assert!(!app.drafts.active.in_flight());

    app.settings_open = false;
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    assert!(!app.modal_blocked);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(editor));
    let _ = frame(&mut app, &ctx, 1200.0, submit_key());
    assert!(
        app.drafts.active.in_flight(),
        "The same shortcut works after modal dismissal"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut stored = false;
    while Instant::now() < deadline {
        while let Ok(event) = app.worker.as_mut().unwrap().events.try_recv() {
            stored |= matches!(event, Event::DemoStored { .. });
        }
        if stored {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(stored);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn modal_focus_and_information_scope_do_not_cross_conversation_changes_or_resize() {
    let ctx = egui::Context::default();
    let (mut app, root) = live::tests::fixture(&ctx);
    app.model.conversations = vec![
        conversation("d:22", "Tên hội thoại cần giữ nguyên"),
        conversation("g:33", "Nhóm tổng hợp"),
    ];
    app.model.selected = Some("d:22".into());
    app.drafts.select("d:22").unwrap();
    app.drafts.active.text = "Bản nháp còn nguyên qua đổi kích thước".into();
    app.focus_composer = true;
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    let editor = ctx.memory(|memory| memory.focused()).unwrap();
    app.conversation_details = Some(("11".into(), "d:22".into()));
    let wide = frame(&mut app, &ctx, 1200.0, vec![]);
    let heading = text_rect(&wide, "Thông tin hội thoại").unwrap();
    assert!(heading.left() >= 1200.0 - 344.0);
    assert!(
        text_rect(&wide, "Gửi").is_some(),
        "Wide sidebar preserves the composer"
    );
    let compact = frame(&mut app, &ctx, 640.0, vec![]);
    assert!(text_rect(&compact, "Thông tin hội thoại").is_some());
    assert!(
        text_rect(&compact, "Gửi").is_none(),
        "Compact information replaces the chat page"
    );
    assert_eq!(
        app.drafts.active.text,
        "Bản nháp còn nguyên qua đổi kích thước"
    );
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    assert!(app.bound_conversation().is_some());

    app.settings_open = true;
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    app.model.selected = Some("g:33".into());
    app.model.generation = app.model.generation.wrapping_add(1);
    app.drafts.select("g:33").unwrap();
    app.drafts.active.text = "Bản nháp nhóm khác".into();
    app.settings_open = false;
    let _ = frame(&mut app, &ctx, 1200.0, vec![]);
    assert!(app.conversation_details.is_none());
    assert_ne!(ctx.memory(|memory| memory.focused()), Some(editor));
    assert_eq!(app.drafts.active.text, "Bản nháp nhóm khác");
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
