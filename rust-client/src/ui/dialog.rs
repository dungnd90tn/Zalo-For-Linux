//! Original Zalo modal chrome. Callers keep all action and authentication decisions.
use eframe::egui::{self, vec2, Align, Align2, Id, Layout, RichText, Sense, Stroke};

pub(super) struct DialogResponse {
    pub closed: bool,
}

pub(super) fn show<R>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    width: f32,
    top_aligned: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> DialogResponse {
    let colors = crate::theme::palette(ctx);
    egui::Area::new(Id::new((id, "curtain")))
        // Clicking the curtain may raise its Area; keep it below the modal Window.
        .order(egui::Order::Middle)
        .fixed_pos(ctx.screen_rect().min)
        .show(ctx, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(ctx.screen_rect().size(), Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, colors.curtain);
        });
    let width = width.min((ctx.screen_rect().width() - 32.0).max(240.0));
    let mut closed =
        ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    egui::Window::new(title)
        .id(Id::new(id))
        .order(egui::Order::Foreground)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .auto_sized()
        .min_width(width)
        .max_width(width)
        .default_width(width)
        .frame(
            egui::Frame::none()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0_f32, colors.border))
                .rounding(4.0),
        )
        .anchor(
            if top_aligned {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_CENTER
            },
            if top_aligned {
                vec2(0.0, 24.0)
            } else {
                egui::Vec2::ZERO
            },
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
            let close_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 20.0, rect.center().y),
                vec2(24.0, 24.0),
            );
            let close_hit_rect = close_rect.expand(3.0);
            let close = ui.interact(close_hit_rect, Id::new((id, "close")), Sense::click());
            if close.hovered() || close.has_focus() {
                ui.painter().rect_filled(close_hit_rect, 4.0, colors.hover);
            }
            super::visuals::source_icon(ui, close_rect, "fa-close_24", colors.text_secondary);
            super::visuals::focus_outline(ui, &close, close_hit_rect, 4.0);
            closed |= close.clicked();
            close.on_hover_text("Đóng");
            let mut heading = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_max(
                        rect.left_top() + vec2(16.0, 0.0),
                        egui::pos2(close_hit_rect.left() - 8.0, rect.bottom()),
                    ))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            heading
                .add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(16.0)
                            .family(egui::FontFamily::Name("Zalo Semibold".into())),
                    )
                    .truncate(),
                )
                .on_hover_text(title);
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0_f32, colors.divider),
            );
            contents(ui)
        });
    DialogResponse { closed }
}

pub(super) fn body<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let height = (ui.ctx().screen_rect().height() - 48.0 - 64.0 - 64.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("dialog_body")
        .auto_shrink([false, true])
        .max_height(height)
        .show(ui, |ui| {
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    contents(ui)
                })
                .inner
        })
        .inner
}

/// Rightmost action is added first; callers decide which actions are valid.
pub(super) fn footer<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let colors = crate::theme::palette(ui.ctx());
    let top = ui.cursor().top();
    ui.painter().line_segment(
        [
            egui::pos2(ui.max_rect().left(), top),
            egui::pos2(ui.max_rect().right(), top),
        ],
        Stroke::new(1.0_f32, colors.divider),
    );
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().interact_size.y = 32.0;
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 32.0),
                Layout::right_to_left(Align::Center),
                contents,
            )
            .inner
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_buttons_keep_the_source_right_inset_and_common_row_axis() {
        for size in [vec2(640.0, 480.0), vec2(1200.0, 780.0)] {
            for native_scale in [1.0, 1.5, 2.0] {
                let ctx = egui::Context::default();
                super::super::visuals::install(&ctx);
                let mut metrics = None;
                for _ in 0..3 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .unwrap()
                        .native_pixels_per_point = Some(native_scale);
                    let _ = ctx.run(input, |ctx| {
                        show(
                            ctx,
                            "aligned_footer",
                            "Hộp thoại kiểm thử",
                            450.0,
                            false,
                            |ui| {
                                body(ui, |ui| {
                                    ui.label("Nội dung kiểm thử.");
                                });
                                let right = ui.max_rect().right();
                                let actions = footer(ui, |ui| {
                                    let primary = super::super::visuals::primary_button(
                                        ui,
                                        true,
                                        "Xong",
                                        vec2(80.0, 32.0),
                                    );
                                    let cancel = ui.button("Hủy");
                                    (primary.rect, cancel.rect)
                                });
                                metrics = Some((right, actions));
                            },
                        );
                    });
                }
                let (right, (primary, cancel)) = metrics.unwrap();
                let epsilon = 1.0 / native_scale;
                assert!(
                    (right - primary.right() - 16.0).abs() <= epsilon,
                    "Footer actions must retain the source 16px right inset"
                );
                assert!((primary.center().y - cancel.center().y).abs() <= epsilon);
                assert!((primary.left() - cancel.right() - 8.0).abs() <= epsilon);
            }
        }
    }

    #[test]
    fn footer_action_row_stays_next_to_body_at_wide_and_compact_sizes() {
        for size in [vec2(1200.0, 780.0), vec2(640.0, 480.0)] {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let render = || {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ctx| {
                        show(
                            ctx,
                            "bounded_footer_fixture",
                            "Hộp thoại kiểm thử",
                            450.0,
                            true,
                            |ui| {
                                body(ui, |ui| {
                                    ui.label("Nội dung ngắn để kiểm tra bố cục.");
                                });
                                footer(ui, |ui| {
                                    super::super::visuals::primary_button(
                                        ui,
                                        true,
                                        "Xong",
                                        vec2(80.0, 32.0),
                                    );
                                });
                            },
                        );
                    },
                )
            };
            for _ in 0..3 {
                render();
            }
            let output = render();
            let position = |label: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos),
                        _ => None,
                    })
                    .unwrap()
            };
            let body = position("Nội dung ngắn để kiểm tra bố cục.");
            let action = position("Xong");
            assert!(
                action.y > body.y && action.y - body.y < 100.0,
                "Footer must not expand into the unused viewport: {body:?} {action:?}"
            );
            assert!(action.y < size.y - 24.0);
        }
    }
}
