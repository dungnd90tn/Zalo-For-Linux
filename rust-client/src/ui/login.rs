//! Native login presentation. Authentication decisions and image decoding belong to the worker.
use super::visuals::{self, Icon};
use eframe::egui::{self, vec2, Align, Color32, Layout, RichText, Sense};

const MAX_QR_SIDE: usize = 1024;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    SignedOut,
    CreatingQr,
    WaitingForScan,
    WaitingForPhone,
    VerifyingTrust,
    Bootstrapping,
    Restoring,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Begin,
    Resume,
    Cancel,
}

#[derive(Default)]
pub(super) struct LoginView {
    generation: u64,
    phase: Phase,
    error: Option<String>,
    qr: Option<egui::TextureHandle>,
    retry_restore: bool,
    startup_attempt: bool,
    phone_tab: bool,
}

impl LoginView {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn start(&mut self, generation: u64, restoring: bool) {
        self.generation = generation;
        self.retry_restore = restoring;
        self.startup_attempt = restoring;
        self.phase = if restoring {
            Phase::Restoring
        } else {
            Phase::CreatingQr
        };
        self.error = None;
        self.qr = None;
        self.phone_tab = false;
    }

    pub fn signed_out(&mut self, generation: u64) {
        self.generation = generation;
        self.phase = Phase::SignedOut;
        self.retry_restore = false;
        self.startup_attempt = false;
        self.error = None;
        self.qr = None;
        self.phone_tab = false;
    }

    pub fn progress(&mut self, generation: u64, phase: Phase) {
        if generation != self.generation {
            return;
        }
        self.phase = phase;
        if phase == Phase::CreatingQr {
            self.retry_restore = false;
        }
        if !matches!(phase, Phase::WaitingForScan) {
            self.qr = None;
        }
        self.error = None;
    }

    pub fn failed(&mut self, generation: u64, message: impl Into<String>) {
        if generation != self.generation {
            return;
        }
        self.phase = Phase::Failed;
        self.error = Some(message.into());
        self.qr = None;
    }

    pub fn pixels(
        &mut self,
        ctx: &egui::Context,
        generation: u64,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> bool {
        if generation != self.generation
            || !matches!(self.phase, Phase::CreatingQr | Phase::WaitingForScan)
        {
            return false;
        }
        let Some(expected) = validated_rgba_size(width, height) else {
            return false;
        };
        if expected != rgba.len() {
            return false;
        }
        self.qr = Some(ctx.load_texture(
            "current-login-qr",
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba),
            egui::TextureOptions::NEAREST,
        ));
        self.phase = Phase::WaitingForScan;
        true
    }

    pub fn show(&mut self, ui: &mut egui::Ui, enabled: bool) -> Option<Action> {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_column(ui, enabled))
            .inner
    }

    fn show_column(&mut self, outer: &mut egui::Ui, enabled: bool) -> Option<Action> {
        let colors = crate::theme::palette(outer.ctx());
        let mut action = None;
        let compact = outer.available_height() < 600.0;
        let width = outer.available_width().min(410.0);
        let mut column = outer.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    egui::pos2(
                        outer.max_rect().center().x - width / 2.0,
                        outer.max_rect().top(),
                    ),
                    vec2(width, outer.available_height()),
                ))
                .layout(Layout::top_down(Align::Center)),
        );
        let ui = &mut column;
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(if compact { 12.0 } else { 48.0 });
        ui.label(
            RichText::new("Zalo")
                .size(if compact { 30.0 } else { 40.0 })
                .color(colors.primary)
                .family(egui::FontFamily::Name("Zalo Bold".into())),
        );
        if !compact {
            ui.add_space(5.0);
            ui.label(
                RichText::new("Đăng nhập Zalo trên máy tính")
                    .size(18.0)
                    .color(colors.text_primary),
            );
        }
        ui.add_space(if compact { 12.0 } else { 26.0 });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (phone, label) in [(false, "VỚI MÃ QR"), (true, "VỚI SỐ ĐIỆN THOẠI")] {
                let selected = self.phone_tab == phone;
                let response = ui.add_sized(
                    vec2(width / 2.0, 36.0),
                    egui::Button::new(RichText::new(label).size(12.0).color(if selected {
                        colors.primary
                    } else {
                        colors.text_secondary
                    }))
                    .frame(false),
                );
                ui.painter().line_segment(
                    [response.rect.left_bottom(), response.rect.right_bottom()],
                    egui::Stroke::new(
                        if selected { 2.0_f32 } else { 1.0_f32 },
                        if selected {
                            colors.primary
                        } else {
                            colors.border
                        },
                    ),
                );
                visuals::focus_outline(ui, &response, response.rect, 0.0);
                if response.clicked() {
                    self.phone_tab = phone;
                }
            }
        });
        ui.add_space(if compact { 12.0 } else { 20.0 });

        let qr_size = if self.phone_tab
            || (matches!(self.phase, Phase::SignedOut | Phase::Failed) && compact)
        {
            80.0
        } else {
            224.0
        };
        let (qr_rect, _) = ui.allocate_exact_size(vec2(qr_size, qr_size), Sense::hover());
        ui.painter().rect_filled(qr_rect, 2.0, Color32::WHITE);
        if self.phone_tab {
            visuals::icon(ui, qr_rect.shrink(20.0), Icon::Phone, colors.text_secondary);
        } else if let Some(texture) = &self.qr {
            ui.painter().image(
                texture.id(),
                qr_rect.shrink(5.0),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter()
                .rect_stroke(qr_rect, 4.0, egui::Stroke::new(1.0_f32, colors.border));
            let busy = matches!(
                self.phase,
                Phase::CreatingQr | Phase::Restoring | Phase::VerifyingTrust | Phase::Bootstrapping
            );
            if busy {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                    egui::Rect::from_center_size(qr_rect.center(), vec2(28.0, 28.0)),
                ));
                child.add(egui::Spinner::new().size(26.0).color(colors.primary));
            } else {
                visuals::icon(
                    ui,
                    egui::Rect::from_center_size(qr_rect.center(), vec2(40.0, 40.0)),
                    if self.phase == Phase::WaitingForPhone {
                        Icon::Phone
                    } else {
                        Icon::Shield
                    },
                    colors.text_secondary,
                );
            }
        }
        ui.add_space(12.0);
        let (title, hint) = if self.phone_tab {
            ("Đăng nhập bằng số điện thoại", "Cách đăng nhập này chưa được hỗ trợ. Dùng mã QR để đăng nhập bằng Zalo trên điện thoại.")
        } else {
            match self.phase {
                Phase::SignedOut => (
                    "Đăng nhập bằng mã QR",
                    "Mở ứng dụng Zalo trên điện thoại để quét mã.",
                ),
                Phase::CreatingQr => (
                    "Đang tạo mã QR…",
                    "Kết nối an toàn tới dịch vụ đăng nhập Zalo.",
                ),
                Phase::WaitingForScan => (
                    "Quét mã QR bằng Zalo",
                    "Mở Zalo trên điện thoại → quét QR để tiếp tục.",
                ),
                Phase::WaitingForPhone => (
                    "Đã quét mã QR",
                    "Xác nhận yêu cầu đăng nhập trên điện thoại của bạn.",
                ),
                Phase::VerifyingTrust => (
                    "Đang xác minh thiết bị…",
                    "Vui lòng giữ Zalo trên điện thoại đang mở.",
                ),
                Phase::Bootstrapping => (
                    "Đang hoàn tất đăng nhập…",
                    "Xác minh tài khoản và thiết lập phiên làm việc.",
                ),
                Phase::Restoring => (
                    "Đang khôi phục phiên…",
                    "Kiểm tra phiên đã lưu trước khi mở hội thoại.",
                ),
                Phase::Failed if self.retry_restore => (
                    "Chưa khôi phục được phiên",
                    "Phiên đã lưu vẫn được giữ. Kiểm tra kết nối và thử lại.",
                ),
                Phase::Failed => ("Chưa thể đăng nhập", "Bạn có thể tạo mã mới và thử lại."),
            }
        };
        ui.label(
            RichText::new(title)
                .size(14.0)
                .family(egui::FontFamily::Name("Zalo Semibold".into())),
        );
        ui.add_space(3.0);
        ui.add(
            egui::Label::new(RichText::new(hint).size(12.0).color(colors.text_secondary)).wrap(),
        );
        if let Some(error) = &self.error {
            ui.add_space(7.0);
            ui.add(egui::Label::new(RichText::new(error).size(12.0).color(colors.error)).wrap());
        }
        ui.add_space(if compact { 10.0 } else { 18.0 });
        if self.phone_tab {
            if visuals::primary_button(ui, enabled, "Dùng mã QR", vec2(240.0, 36.0)).clicked() {
                self.phone_tab = false;
            }
            if !matches!(self.phase, Phase::SignedOut | Phase::Failed) {
                ui.add_space(6.0);
                if ui
                    .add_enabled(enabled, egui::Button::new("Hủy đăng nhập"))
                    .clicked()
                {
                    action = Some(Action::Cancel);
                }
            }
        } else if matches!(self.phase, Phase::SignedOut | Phase::Failed) {
            if visuals::primary_button(
                ui,
                enabled,
                if self.retry_restore {
                    "Thử lại phiên đã lưu"
                } else if self.phase == Phase::Failed {
                    "Tạo mã QR mới"
                } else {
                    "Đăng nhập bằng mã QR"
                },
                vec2(240.0, 36.0),
            )
            .clicked()
            {
                action = Some(if self.startup_attempt {
                    Action::Resume
                } else {
                    Action::Begin
                });
            }
            ui.add_space(3.0);
            if self.retry_restore
                && ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(RichText::new("Tạo mã QR mới").color(colors.primary))
                            .frame(false),
                    )
                    .clicked()
            {
                action = Some(Action::Begin);
            }
        } else {
            // A full-width horizontal row centers its own frame, not its children.
            // Give the action group a bounded frame on the same axis as the QR.
            let action_width = 144.0;
            let gap = ui.spacing().item_spacing.x;
            let has_refresh = self.phase == Phase::WaitingForScan;
            let group_width = if has_refresh {
                action_width * 2.0 + gap
            } else {
                action_width
            };
            ui.allocate_ui_with_layout(
                vec2(group_width, 36.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    if self.phase == Phase::WaitingForScan
                        && ui
                            .add_enabled_ui(enabled, |ui| {
                                ui.add_sized(
                                    vec2(action_width, 36.0),
                                    egui::Button::new("Tạo mã mới"),
                                )
                            })
                            .inner
                            .clicked()
                    {
                        action = Some(Action::Begin);
                    }
                    if ui
                        .add_enabled_ui(enabled, |ui| {
                            ui.add_sized(
                                vec2(action_width, 36.0),
                                egui::Button::new("Hủy đăng nhập"),
                            )
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Cancel);
                    }
                },
            );
        }
        if !enabled {
            ui.add_space(7.0);
            ui.label(
                RichText::new("Chạy client với --live để bật đăng nhập. Bản demo dùng --demo.")
                    .size(visuals::TEXT_SMALL)
                    .color(colors.text_secondary),
            );
        }
        outer.set_min_height(column.min_rect().height());
        action
    }
}

fn validated_rgba_size(width: u32, height: u32) -> Option<usize> {
    let (width, height) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if width == 0 || height == 0 || width > MAX_QR_SIDE || height > MAX_QR_SIDE {
        return None;
    }
    width.checked_mul(height)?.checked_mul(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_action_groups_and_phone_cta_share_the_card_axis_at_native_scales() {
        for size in [vec2(640.0, 480.0), vec2(1200.0, 780.0)] {
            for native_scale in [1.0, 1.5, 2.0] {
                let ctx = egui::Context::default();
                super::super::visuals::install(&ctx);
                let mut view = LoginView::default();
                view.start(7, false);
                for (phase, phone) in [
                    (Phase::WaitingForScan, false),
                    (Phase::WaitingForPhone, false),
                    (Phase::CreatingQr, false),
                    (Phase::Restoring, false),
                    (Phase::SignedOut, false),
                    (Phase::Failed, false),
                    (Phase::WaitingForScan, true),
                ] {
                    view.phase = phase;
                    view.phone_tab = phone;
                    let render = |view: &mut LoginView| {
                        let mut input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .unwrap()
                            .native_pixels_per_point = Some(native_scale);
                        ctx.run(input, |ctx| {
                            egui::CentralPanel::default()
                                .frame(egui::Frame::none())
                                .show(ctx, |ui| {
                                    assert!(view.show(ui, true).is_none());
                                });
                        })
                    };
                    let _ = render(&mut view);
                    let output = render(&mut view);
                    let qr = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.fill == Color32::WHITE
                                    && rect.rect.width() == rect.rect.height()
                                    && matches!(rect.rect.width(), 80.0 | 224.0) =>
                            {
                                Some(rect.rect)
                            }
                            _ => None,
                        })
                        .expect("QR/phone card must be visible");
                    let epsilon = 1.0 / native_scale;
                    assert!((qr.center().x - size.x / 2.0).abs() <= epsilon);
                    if !phone && !matches!(phase, Phase::SignedOut | Phase::Failed) {
                        let actions: Vec<_> = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect)
                                    if rect.rect.size() == vec2(144.0, 36.0) =>
                                {
                                    Some(rect.rect)
                                }
                                _ => None,
                            })
                            .collect();
                        assert_eq!(
                            actions.len(),
                            if phase == Phase::WaitingForScan { 2 } else { 1 }
                        );
                        let group = actions.iter().copied().reduce(egui::Rect::union).unwrap();
                        assert!(
                            (group.center().x - qr.center().x).abs() <= epsilon,
                            "Actual QR actions must share card center: group={group:?}, qr={qr:?}"
                        );
                        assert!(actions
                            .iter()
                            .all(|rect| (rect.center().y - group.center().y).abs() <= epsilon));
                    } else {
                        let center = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect)
                                    if rect.rect.size() == vec2(240.0, 36.0) =>
                                {
                                    Some(rect.rect.center().x)
                                }
                                _ => None,
                            })
                            .unwrap();
                        assert!((center - qr.center().x).abs() <= epsilon);
                    }
                    assert_eq!(view.generation(), 7);
                }
            }
        }
    }

    #[test]
    fn phone_info_tab_preserves_qr_attempt_and_keeps_cancel_accessible() {
        for width in [640.0, 1200.0] {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let mut view = LoginView::default();
            view.start(7, false);
            assert!(view.pixels(&ctx, 7, 1, 1, &[255; 4]));
            let qr = view.qr.as_ref().unwrap().id();
            let frame = |view: &mut LoginView, events| {
                let mut action = None;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width, 480.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            action = view.show(ui, true);
                        });
                    },
                );
                (output, action)
            };
            for (label, expected_phone, expected_action) in [
                ("VỚI SỐ ĐIỆN THOẠI", true, None),
                ("Dùng mã QR", false, None),
                ("VỚI SỐ ĐIỆN THOẠI", true, None),
                ("Hủy đăng nhập", true, Some(Action::Cancel)),
            ] {
                let (output, _) = frame(&mut view, vec![]);
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => {
                            Some(text.pos + text.galley.size() / 2.0)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing {label} at {width}"));
                for pressed in [true, false] {
                    let (_, action) = frame(
                        &mut view,
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: Default::default(),
                            },
                        ],
                    );
                    if !pressed {
                        assert!(action == expected_action);
                    }
                }
                assert_eq!(view.phone_tab, expected_phone);
                assert_eq!(view.generation(), 7);
                assert!(view.phase == Phase::WaitingForScan);
                assert_eq!(view.qr.as_ref().unwrap().id(), qr);
            }
        }
    }
    #[test]
    fn late_qr_and_progress_cannot_replace_the_current_login_attempt() {
        let ctx = egui::Context::default();
        let mut view = LoginView::default();
        view.start(2, false);
        assert!(!view.pixels(&ctx, 1, 1, 1, &[255; 4]));
        view.progress(1, Phase::WaitingForPhone);
        assert!(view.phase == Phase::CreatingQr);
        view.signed_out(3);
        assert!(!view.pixels(&ctx, 2, 1, 1, &[255; 4]));
        assert!(view.qr.is_none());
    }
    #[test]
    fn startup_failure_primary_retry_reevaluates_worker_state_and_logout_stays_manual() {
        for (width, height) in [(1200.0, 780.0), (640.0, 480.0)] {
            let ctx = egui::Context::default();
            super::super::visuals::install(&ctx);
            let mut view = LoginView::default();
            for (qr_started, label, expected) in [
                (false, "Thử lại phiên đã lưu", Action::Resume),
                (true, "Tạo mã QR mới", Action::Resume),
            ] {
                view.start(1, true);
                if qr_started {
                    view.progress(1, Phase::CreatingQr);
                }
                view.failed(1, "Không thể kết nối");
                let mut action = None;
                let mut frame = |events: Vec<egui::Event>| {
                    ctx.run(
                        egui::RawInput {
                            events,
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                action = view.show(ui, true);
                            });
                        },
                    )
                };
                let output = frame(vec![]);
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                            Some(text.pos + text.galley.size() / 2.0)
                        }
                        _ => None,
                    })
                    .unwrap();
                frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ]);
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }]);
                assert!(action == Some(expected));
            }
            view.signed_out(2);
            assert!(!view.startup_attempt && !view.retry_restore);
            view.progress(1, Phase::Restoring);
            assert!(
                view.phase == Phase::SignedOut,
                "late progress must not revive login after logout"
            );
        }
    }

    #[test]
    fn qr_rgba_validation_rejects_unbounded_and_truncated_images() {
        assert_eq!(validated_rgba_size(0, 200), None);
        assert_eq!(validated_rgba_size(1025, 1), None);
        let ctx = egui::Context::default();
        let mut view = LoginView::default();
        view.start(1, false);
        assert!(!view.pixels(&ctx, 1, 256, 256, &[255; 4]));
        assert!(!view.pixels(&ctx, 1, u32::MAX, u32::MAX, &[]));
        assert!(view.qr.is_none());
    }

    #[test]
    fn compact_login_keeps_start_resume_and_cancel_visible() {
        let ctx = egui::Context::default();
        super::super::visuals::install(&ctx);
        let mut view = LoginView::default();
        for (phase, expected) in [
            (Phase::SignedOut, "Đăng nhập bằng mã QR"),
            (Phase::WaitingForScan, "Hủy đăng nhập"),
            (Phase::Restoring, "Hủy đăng nhập"),
        ] {
            view.phase = phase;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            view.show(ui, true);
                        });
                },
            );
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) =>
                        text.galley.job.text == expected
                            && text.pos.y >= 0.0
                            && text.pos.y + text.galley.size().y <= 480.0,
                    _ => false,
                }),
                "compact action must be visible: {expected}"
            );
        }
    }
}
