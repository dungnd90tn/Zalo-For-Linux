//! Maximize changes the reading area, not the logical size of text or controls.
use super::*;

const TITLE: &str = "Kích thước cố định";
const BODY: &str = "Tin nhắn giữ nguyên cỡ chữ.";

fn fixture(ctx: &egui::Context) -> (NativeApp, std::path::PathBuf) {
    let (mut app, root) = live::tests::fixture(ctx);
    app.live = None;
    app.demo = true;
    app.model.generation = 7;
    app.model.conversations = vec![Conversation {
        id: "d:22".into(),
        title: TITLE.into(),
        last_message_at_ms: 1_700_000_000_000,
        last_text: BODY.into(),
        last_sender: "22".into(),
    }];
    app.model.selected = Some("d:22".into());
    app.model.messages = vec![Message {
        account_id: "0".into(),
        thread_id: "d:22".into(),
        id: "fixed-size-message".into(),
        sender: "22".into(),
        text: BODY.into(),
        sent_at_ms: 1_700_000_000_000,
        sticker: None,
        media: None,
    }];
    app.drafts.select("d:22").unwrap();
    app.drafts.active.text = "Bản nháp giữ nguyên qua phóng to và khôi phục".into();
    (app, root)
}

fn frame(
    app: &mut NativeApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    native_scale: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(native_scale);
    ctx.run(input, |ctx| app.workspace(ctx))
}

fn settle(
    app: &mut NativeApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    native_scale: f32,
) -> egui::FullOutput {
    let _ = frame(app, ctx, size, native_scale, vec![]);
    frame(app, ctx, size, native_scale, vec![])
}

fn text_size(output: &egui::FullOutput, value: &str, points: f32) -> egui::Vec2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text)
                if text.galley.job.text == value
                    && text
                        .galley
                        .job
                        .sections
                        .iter()
                        .all(|section| section.format.font_id.size == points) =>
            {
                Some(text.galley.size())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing actual rendered text {value:?} at {points} points"))
}

fn send_rect(output: &egui::FullOutput) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.rect.size() == vec2(82.0, 38.0) => {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("Painted send button must retain its 82×38 logical frame")
}

fn panel_rect(ctx: &egui::Context, id: &str) -> egui::Rect {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(id))
        .unwrap()
        .rect
}

#[test]
fn maximize_restore_and_compact_reflow_keep_fonts_frames_draft_and_full_send_hit_area() {
    for native_scale in [1.0, 2.0] {
        let ctx = egui::Context::default();
        let (mut app, root) = fixture(&ctx);
        app.worker = Some(
            Worker::demo(
                app._profile.paths.data_dir.join("fixed-layout.sqlite3"),
                Arc::new(|| {}),
            )
            .unwrap(),
        );
        let original_draft = app.drafts.active.text.clone();
        app.focus_composer = true;
        let mut baseline = None;
        let mut button = egui::Rect::NOTHING;
        for size in [
            vec2(1200.0, 780.0),
            vec2(1920.0, 1080.0),
            vec2(640.0, 480.0),
            vec2(1200.0, 780.0),
        ] {
            let output = settle(&mut app, &ctx, size, native_scale);
            assert_eq!(ctx.pixels_per_point(), native_scale);
            assert_eq!(ctx.zoom_factor(), 1.0);
            assert_eq!(panel_rect(&ctx, "navigation").width(), 64.0);
            if size.x >= 940.0 {
                assert_eq!(panel_rect(&ctx, "conversations").width(), 344.0);
                let _ = text_size(&output, TITLE, 14.0);
            }
            let header = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if rect.rect.height() == 68.0 && rect.rect.left() >= 64.0 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("Chat header must retain its 68-point frame");
            assert_eq!(header.height(), 68.0);
            let measured = (
                text_size(&output, TITLE, 18.0),
                text_size(&output, BODY, 15.0),
                text_size(&output, &clock_label(1_700_000_000_000), 12.0),
            );
            if let Some(baseline) = baseline {
                assert_eq!(
                    measured, baseline,
                    "Window resize must not scale rendered type"
                );
            } else {
                baseline = Some(measured);
            }
            let style = ctx.style();
            assert_eq!(style.text_styles[&egui::TextStyle::Body].size, 14.0);
            assert_eq!(style.text_styles[&egui::TextStyle::Small].size, 12.0);
            button = send_rect(&output);
            let editor = ctx
                .read_response(ctx.memory(|memory| memory.focused()).unwrap())
                .unwrap()
                .rect;
            assert!((size.x - button.right() - 20.0).abs() <= 1.0 / native_scale);
            assert!((button.left() - editor.right() - 14.0).abs() <= 1.0 / native_scale);
            assert!(button.center().y >= editor.top() && button.center().y <= editor.bottom());
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(button));
            assert_eq!(app.drafts.active.text, original_draft);
            assert_eq!(app.model.selected.as_deref(), Some("d:22"));
            assert_eq!(app.model.generation, 7);
        }
        // Exercise the actual hit area near its painted corner, outside the centered label.
        let point = button.min + vec2(2.0, 2.0);
        let pointer = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(
            &mut app,
            &ctx,
            vec2(1200.0, 780.0),
            native_scale,
            vec![egui::Event::PointerMoved(point), pointer(true)],
        );
        let _ = frame(
            &mut app,
            &ctx,
            vec2(1200.0, 780.0),
            native_scale,
            vec![pointer(false)],
        );
        assert!(app.drafts.active.in_flight());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn information_frame_stays_fixed_when_maximized_and_becomes_a_compact_page() {
    let ctx = egui::Context::default();
    let (mut app, root) = fixture(&ctx);
    let draft = app.drafts.active.text.clone();
    app.conversation_details = Some(("0".into(), "d:22".into()));
    for size in [
        vec2(1200.0, 780.0),
        vec2(1920.0, 1080.0),
        vec2(640.0, 480.0),
        vec2(1200.0, 780.0),
    ] {
        let output = settle(&mut app, &ctx, size, 1.0);
        let _ = text_size(&output, TITLE, 18.0);
        if size.x >= 940.0 {
            let info = panel_rect(&ctx, "conversation_information_sidebar");
            assert_eq!(info.width(), 344.0);
            assert_eq!(info.right(), size.x);
            let _ = send_rect(&output);
        } else {
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::epaint::Shape::Text(text) if text.galley.job.text == "Lưu")));
        }
        assert!(app.bound_conversation().is_some());
        assert_eq!(app.drafts.active.text, draft);
        assert_eq!(app.model.selected.as_deref(), Some("d:22"));
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
