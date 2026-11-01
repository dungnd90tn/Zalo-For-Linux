//! Native login presentation. Authentication decisions and image decoding belong to the worker.
use super::visuals::{self, Icon, BLUE, HOVER, INK, LINE, MUTED};
use eframe::egui::{self, vec2, Align, Color32, FontId, Layout, RichText, Sense};

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
}

impl LoginView {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn start(&mut self, generation: u64, restoring: bool) {
        self.generation = generation;
        self.phase = if restoring {
            Phase::Restoring
        } else {
            Phase::CreatingQr
        };
        self.error = None;
        self.qr = None;
    }

    pub fn signed_out(&mut self, generation: u64) {
        self.generation = generation;
        self.phase = Phase::SignedOut;
        self.error = None;
        self.qr = None;
    }

    pub fn progress(&mut self, generation: u64, phase: Phase) {
        if generation != self.generation {
            return;
        }
        self.phase = phase;
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

    pub fn show(&self, ui: &mut egui::Ui, enabled: bool) -> Option<Action> {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_column(ui, enabled))
            .inner
    }

    fn show_column(&self, outer: &mut egui::Ui, enabled: bool) -> Option<Action> {
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
                .color(BLUE)
                .strong(),
        );
        if !compact {
            ui.add_space(5.0);
            ui.label(
                RichText::new("Đăng nhập Zalo trên máy tính")
                    .size(18.0)
                    .color(INK),
            );
        }
        ui.add_space(if compact { 12.0 } else { 26.0 });
        let (tabs, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
        ui.painter().line_segment(
            [tabs.left_bottom(), tabs.right_bottom()],
            egui::Stroke::new(1.0_f32, LINE),
        );
        ui.painter().text(
            tabs.left_center() + vec2(55.0, 0.0),
            egui::Align2::CENTER_CENTER,
            "VỚI MÃ QR",
            FontId::proportional(13.0),
            BLUE,
        );
        ui.painter().line_segment(
            [
                tabs.left_bottom() + vec2(12.0, 0.0),
                tabs.left_bottom() + vec2(135.0, 0.0),
            ],
            egui::Stroke::new(2.0_f32, BLUE),
        );
        let phone = ui.interact(
            egui::Rect::from_min_max(tabs.center_top(), tabs.right_bottom()),
            ui.id().with("phone_login_unavailable"),
            Sense::hover(),
        );
        ui.painter().text(
            tabs.center() + vec2(90.0, 0.0),
            egui::Align2::CENTER_CENTER,
            "VỚI SỐ ĐIỆN THOẠI",
            FontId::proportional(12.0),
            MUTED,
        );
        phone.on_hover_text("Đăng nhập bằng số điện thoại chưa được hỗ trợ trong client native.");
        ui.add_space(if compact { 12.0 } else { 20.0 });

        let qr_size = if matches!(self.phase, Phase::SignedOut | Phase::Failed) && compact {
            80.0
        } else {
            224.0
        };
        let (qr_rect, _) = ui.allocate_exact_size(vec2(qr_size, qr_size), Sense::hover());
        ui.painter().rect_filled(qr_rect, 2.0, Color32::WHITE);
        if let Some(texture) = &self.qr {
            ui.painter().image(
                texture.id(),
                qr_rect.shrink(5.0),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter()
                .rect_stroke(qr_rect, 4.0, egui::Stroke::new(1.0_f32, LINE));
            let busy = matches!(
                self.phase,
                Phase::CreatingQr | Phase::Restoring | Phase::VerifyingTrust | Phase::Bootstrapping
            );
            if busy {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                    egui::Rect::from_center_size(qr_rect.center(), vec2(28.0, 28.0)),
                ));
                child.add(egui::Spinner::new().size(26.0).color(BLUE));
            } else {
                visuals::icon(
                    ui,
                    egui::Rect::from_center_size(qr_rect.center(), vec2(40.0, 40.0)),
                    if self.phase == Phase::WaitingForPhone {
                        Icon::Phone
                    } else {
                        Icon::Shield
                    },
                    MUTED,
                );
            }
        }
        ui.add_space(12.0);
        let (title, hint) = match self.phase {
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
            Phase::Failed => ("Chưa thể đăng nhập", "Bạn có thể tạo mã mới và thử lại."),
        };
        ui.label(RichText::new(title).size(14.0).strong());
        ui.add_space(3.0);
        ui.label(RichText::new(hint).size(12.0).color(MUTED));
        if let Some(error) = &self.error {
            ui.add_space(7.0);
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .size(12.0)
                        .color(Color32::from_rgb(195, 24, 24)),
                )
                .wrap(),
            );
        }
        ui.add_space(if compact { 10.0 } else { 18.0 });
        if matches!(self.phase, Phase::SignedOut | Phase::Failed) {
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(
                        RichText::new(if self.phase == Phase::Failed {
                            "Tạo mã QR mới"
                        } else {
                            "Đăng nhập bằng mã QR"
                        })
                        .color(Color32::WHITE),
                    )
                    .fill(BLUE)
                    .min_size(vec2(240.0, 36.0)),
                )
                .clicked()
            {
                action = Some(Action::Begin);
            }
            ui.add_space(3.0);
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(RichText::new("Tiếp tục phiên đã lưu").color(BLUE))
                        .frame(false),
                )
                .clicked()
            {
                action = Some(Action::Resume);
            }
        } else {
            ui.horizontal(|ui| {
                if self.phase == Phase::WaitingForScan
                    && ui
                        .add_enabled(enabled, egui::Button::new("Tạo mã mới").fill(HOVER))
                        .clicked()
                {
                    action = Some(Action::Begin);
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Hủy đăng nhập"))
                    .clicked()
                {
                    action = Some(Action::Cancel);
                }
            });
        }
        if !enabled {
            ui.add_space(7.0);
            ui.label(
                RichText::new("Chạy client với --live để bật đăng nhập. Bản demo dùng --demo.")
                    .size(11.0)
                    .color(MUTED),
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
            (Phase::SignedOut, "Tiếp tục phiên đã lưu"),
            (Phase::WaitingForScan, "Hủy đăng nhập"),
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
