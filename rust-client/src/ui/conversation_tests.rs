//! A conversation is one interactive card, including its painted preview.
use super::*;

fn row_frame(ctx: &egui::Context, events: Vec<egui::Event>, enabled: bool) -> egui::Response {
    let mut response = None;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(400.0, 200.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    response = Some(conversation_row(
                        ui,
                        344.0,
                        &Conversation {
                            id: "g:22".into(),
                            title: "Datanomics".into(),
                            last_message_at_ms: 1_700_000_000_000,
                            last_text: "Cũng lâu lâu rồi ko biết họ có ý gì ko nữa".into(),
                            last_sender: "33".into(),
                        },
                        "Datanomics",
                        true,
                        Some("11"),
                        None,
                    ));
                });
            });
        },
    );
    response.unwrap()
}

fn tap(ctx: &egui::Context, position: egui::Pos2, enabled: bool) -> egui::Response {
    let _ = row_frame(
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        enabled,
    );
    row_frame(
        ctx,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        enabled,
    )
}

#[test]
fn conversation_card_hitbox_includes_preview_title_avatar_time_and_empty_padding() {
    for (name, offset) in [
        ("avatar", vec2(40.0, 38.0)),
        ("title", vec2(95.0, 22.0)),
        ("preview", vec2(100.0, 55.0)),
        ("timestamp", vec2(320.0, 22.0)),
        ("left padding", vec2(2.0, 40.0)),
        ("bottom padding", vec2(250.0, 76.0)),
    ] {
        let ctx = egui::Context::default();
        visuals::install(&ctx);
        let rect = row_frame(&ctx, vec![], true).rect;
        let _ = row_frame(&ctx, vec![], true);
        let response = tap(&ctx, rect.min + offset, true);
        assert!(
            response.clicked(),
            "{name} must activate the same whole-row widget"
        );
        assert!(response.has_focus(), "{name} must focus the row");
        assert!(
            !tap(&ctx, rect.max + vec2(20.0, 20.0), true).clicked(),
            "A click outside must not select the row"
        );
    }
}

#[test]
fn disabled_conversation_card_cannot_activate_and_keyboard_uses_the_same_focus_target() {
    let ctx = egui::Context::default();
    visuals::install(&ctx);
    let response = row_frame(&ctx, vec![], false);
    assert!(!tap(&ctx, response.rect.center(), false).clicked());
    let response = row_frame(&ctx, vec![], true);
    ctx.memory_mut(|memory| memory.request_focus(response.id));
    let response = row_frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        true,
    );
    assert!(
        response.clicked(),
        "Enter must activate the focused conversation card"
    );
}

#[test]
fn short_bubbles_hug_content_and_long_messages_stay_inside_wide_and_compact_bounds() {
    for (width, compact) in [(640.0, false), (320.0, true)] {
        for own in [false, true] {
            let ctx = egui::Context::default();
            visuals::install(&ctx);
            let render = |text: &str| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            message_bubble(
                                ui,
                                &Message {
                                    account_id: "11".into(),
                                    thread_id: "d:22".into(),
                                    id: "synthetic".into(),
                                    sender: if own { "11" } else { "22" }.into(),
                                    text: text.into(),
                                    sent_at_ms: 1_700_000_000_000,
                                    sticker: None,
                                    media: None,
                                },
                                compact,
                                Some("11"),
                                "Nguyễn Minh",
                                "Nguyễn Minh",
                                None,
                                #[cfg(feature = "messaging")]
                                None,
                                #[cfg(feature = "messaging")]
                                None,
                                1,
                            );
                        });
                    },
                )
            };
            let color = if own {
                crate::theme::palette(&ctx).bubble_own
            } else {
                crate::theme::palette(&ctx).bubble_incoming
            };
            let bubble = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == color && rect.rounding.nw == 18.0 =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("Real chat bubble must be painted")
            };
            let _ = render("Đã rõ.");
            let short = bubble(&render("Đã rõ."));
            assert!(
                short.width() < 250.0,
                "Short text must hug status/content, not reserve full maximum width: {short:?}"
            );
            let long = bubble(&render(
                &"Nội dung tiếng Việt dài giữ nguyên cỡ chữ và xuống dòng. ".repeat(12),
            ));
            assert!(
                long.width() > short.width(),
                "Long content must use more reading space"
            );
            assert!(
                long.left() >= 0.0 && long.right() <= width,
                "Long messages must stay in the viewport: {long:?}"
            );
        }
    }
}

/// A sticker is the picture alone: no bubble fill or border, and the message is only as wide
/// as the picture (plus the status line when that is wider), never the full bubble width.
#[cfg(feature = "messaging")]
#[test]
fn sticker_messages_have_no_bubble_and_fit_the_picture() {
    use crate::domain::{StickerDescriptor, LEGACY_STICKER_PLACEHOLDER};
    for own in [false, true] {
        for animated in [false, true] {
            let ctx = egui::Context::default();
            visuals::install(&ctx);
            let sticker = StickerDescriptor {
                sticker_id: 7,
                category_id: 4,
                sticker_type: 3,
            };
            let mut cache = stickers::StickerCache::default();
            if animated {
                // A strip of three 130 px frames: one frame, not the whole strip, is displayed.
                let mut rgba = vec![0u8; 390 * 130 * 4];
                rgba.iter_mut().for_each(|byte| *byte = 200);
                cache.seed_animation(
                    &ctx,
                    sticker,
                    crate::app::stickers::StickerAnimation {
                        frame_width: 130,
                        frame_height: 130,
                        columns: 3,
                        atlas_width: 390,
                        atlas_height: 130,
                        delays_ms: vec![100; 3],
                        rgba,
                    },
                );
            } else {
                cache.seed(&ctx, sticker, 130, 130, vec![200; 130 * 130 * 4]);
            }
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(640.0, 480.0),
                    )),
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        message_bubble(
                            ui,
                            &Message {
                                account_id: "11".into(),
                                thread_id: "d:22".into(),
                                id: "sticker".into(),
                                sender: if own { "11" } else { "22" }.into(),
                                text: LEGACY_STICKER_PLACEHOLDER.into(),
                                sent_at_ms: 1_700_000_000_000,
                                sticker: Some(sticker),
                                media: None,
                            },
                            false,
                            Some("11"),
                            "Nguyễn Minh",
                            "Nguyễn Minh",
                            None,
                            Some(&mut cache),
                            None,
                            1,
                        );
                    });
                },
            );
            let colors = crate::theme::palette(&ctx);
            let label = format!("own={own} animated={animated}");
            assert!(
                !output.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::Shape::Rect(rect)
                        if (rect.fill == colors.bubble_incoming || rect.fill == colors.bubble_own)
                            && rect.rounding.nw == 18.0
                )),
                "{label}: a sticker must not be drawn inside a bubble"
            );
            // egui draws an image as a white-tinted rectangle that samples the texture.
            let picture = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == egui::Color32::WHITE
                            && (rect.rect.width() - 130.0).abs() < 1.5
                            && (rect.rect.height() - 130.0).abs() < 1.5 =>
                    {
                        Some(*rect)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{label}: a 130 px sticker picture must be painted"));
            if animated {
                assert!(
                    (picture.uv.width() - 1.0 / 3.0).abs() < 1e-4
                        && (picture.uv.height() - 1.0).abs() < 1e-4,
                    "{label}: one frame of the strip is shown, not the whole atlas: {:?}",
                    picture.uv
                );
            }
            let mut extent = egui::Rect::NOTHING;
            for shape in &output.shapes {
                let bounds = shape.shape.visual_bounding_rect();
                if bounds.is_positive() && bounds.width() < 600.0 {
                    extent = extent.union(bounds);
                }
            }
            // Picture 130, plus the sender avatar and gap for an incoming message, plus the
            // status line when it is wider than the picture.
            let limit = 165.0 + if own { 0.0 } else { 40.0 };
            assert!(
                extent.width() <= limit,
                "{label}: the message is {}px wide, expected at most {limit}",
                extent.width()
            );
        }
    }
}
