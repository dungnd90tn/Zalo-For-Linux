//! Built-in sticker picker. The worker validates and sends each explicit selection.
use super::{
    message_actions::Scope, stickers::Presentation, visuals, InteractionNotice, ModalFocus,
    NativeApp,
};
use crate::app::{
    live::{LiveConnection, LiveEvent, StickerOutcome},
    stickers::builtin_stickers,
};
use eframe::egui::{self, pos2, vec2, Align2, Rect, RichText, Sense};

const COLUMNS: usize = 6;
const TILE: f32 = 44.0;
const GAP: f32 = 4.0;
const PADDING: f32 = 10.0;
const PICKER_WIDTH: f32 = COLUMNS as f32 * TILE + (COLUMNS - 1) as f32 * GAP;

struct Picker {
    scope: Scope,
    anchor: Rect,
    just_opened: bool,
    action_focus: Option<egui::Id>,
    focus: Option<ModalFocus>,
}

#[derive(Default)]
pub(super) struct StickerSend {
    picker: Option<Picker>,
    request: u64,
    pending: Option<(String, u64)>,
    status: Option<InteractionNotice>,
}

impl StickerSend {
    pub(super) fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    pub(super) fn take_focus(&mut self) -> Option<ModalFocus> {
        self.picker.as_mut()?.focus.take()
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        let LiveEvent::StickerFinished {
            request_id,
            thread,
            outcome,
            ..
        } = event
        else {
            return;
        };
        if !self
            .pending
            .as_ref()
            .is_some_and(|(expected_thread, expected_id)| {
                expected_thread == thread && expected_id == request_id
            })
        {
            return;
        }
        self.pending = None;
        self.status = Some(InteractionNotice::new(
            thread.clone(),
            match outcome {
                StickerOutcome::Accepted => "Zalo đã nhận nhãn dán.",
                StickerOutcome::Rejected => "Zalo từ chối gửi nhãn dán.",
                StickerOutcome::Unavailable => "Hội thoại này chưa thể gửi nhãn dán.",
                StickerOutcome::Unknown => "Chưa rõ Zalo đã nhận nhãn dán.",
            },
        ));
    }

    pub(super) fn worker_closed(&mut self) {
        if let Some((thread, _)) = self.pending.take() {
            self.status = Some(InteractionNotice::new(
                thread,
                "Chưa rõ Zalo đã nhận nhãn dán.",
            ));
        }
    }
}

impl NativeApp {
    fn sticker_scope(&self) -> Option<Scope> {
        let scope = self.message_menu_scope()?;
        (!self.demo
            && self.privacy_list_visible(&scope.thread)
            && scope.thread.strip_prefix("d:") != Some(scope.account.as_str())
            && !self.directory_visible()
            && !self.selected_group_left())
        .then_some(scope)
    }

    fn can_pick_sticker(&self) -> bool {
        if self.sticker_scope().is_none() || self.model.pending {
            return false;
        }
        #[cfg(test)]
        if self
            .live
            .as_ref()
            .is_some_and(|state| state.sticker_fixture)
        {
            return true;
        }
        self.live.as_ref().is_some_and(|state| {
            state.worker.is_some()
                && state.connection == LiveConnection::Online
                && state.sticker_send.pending.is_none()
        })
    }

    pub(super) fn sticker_status(&self) -> Option<(&'static str, std::time::Instant)> {
        let thread = self.model.selected.as_deref()?;
        if !self.privacy_list_visible(thread) {
            return None;
        }
        self.live
            .as_ref()?
            .sticker_send
            .status
            .as_ref()
            .and_then(|notice| notice.for_thread(thread))
    }

    pub(super) fn sync_sticker_picker(&mut self, ctx: &egui::Context) {
        let scope = self.sticker_scope();
        let hidden = ctx.screen_rect().width() < super::COMPACT_BREAKPOINT && self.compact_list;
        if let Some(state) = &mut self.live {
            if state.sticker_send.picker.as_ref().is_some_and(|picker| {
                scope.as_ref() != Some(&picker.scope)
                    || hidden
                    || self.settings_open
                    || self.details
                    || self.account_popup.is_some()
                    || self.message_menu.is_some()
                    || state.logout_dialog
                    || state.open_dialog
                    || state.review_dialog
                    || state.privacy.prompt.is_some()
                    || state.attention.settings_open
                    || state.forwarding.is_open()
                    || state.recall.is_open()
                    || state.groups.is_open()
            }) {
                state.sticker_send.picker = None;
                self.modal_focus = None;
            }
        }
    }

    pub(super) fn sticker_button(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let focus = self.modal_focus_snapshot(ui.ctx());
        let can_pick = self.can_pick_sticker();
        let label = if can_pick {
            "Mở bảng nhãn dán"
        } else if self.demo {
            "Chỉ gửi được nhãn dán trong phiên Zalo."
        } else if self
            .live
            .as_ref()
            .is_some_and(|state| state.sticker_send.pending.is_some())
        {
            "Đang chờ Zalo xử lý lượt gửi nhãn dán."
        } else {
            "Cần kết nối Zalo và mở một hội thoại thông thường để gửi nhãn dán."
        };
        let button = visuals::icon_button(ui, visuals::Icon::Sticker, label, enabled && can_pick);
        let scope = self.sticker_scope();
        if let Some(state) = &mut self.live {
            if let Some(picker) = &mut state.sticker_send.picker {
                picker.anchor = button.rect;
            } else {
                #[cfg(test)]
                let fixture_open = std::mem::take(&mut state.sticker_fixture_open);
                #[cfg(not(test))]
                let fixture_open = false;
                if let Some(scope) = scope.filter(|_| button.clicked() || fixture_open) {
                    state.sticker_send.picker = Some(Picker {
                        scope,
                        anchor: button.rect,
                        just_opened: true,
                        action_focus: None,
                        focus,
                    });
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    pub(super) fn show_sticker_picker(&mut self, ctx: &egui::Context) {
        self.sync_sticker_picker(ctx);
        if !self
            .live
            .as_ref()
            .is_some_and(|state| state.sticker_send.is_open())
        {
            return;
        }
        self.latch_modal_input(ctx);
        let can_pick = self.can_pick_sticker();
        let Some(state) = &mut self.live else { return };
        let Some(picker) = &mut state.sticker_send.picker else {
            return;
        };
        let bounds = ctx.screen_rect().shrink(8.0);
        let x = picker
            .anchor
            .left()
            .min(bounds.right() - PICKER_WIDTH - PADDING * 2.0);
        let y = picker.anchor.top() - 8.0;
        let grid_height = (y - bounds.top() - 58.0).clamp(TILE, 240.0);
        let mut selection = None;
        let mut buttons = Vec::new();
        let tab = super::message_actions::popup_tab(ctx, picker.action_focus);
        let response = egui::Area::new(egui::Id::new("sticker_picker"))
            .kind(egui::UiKind::Popup)
            .order(egui::Order::Foreground)
            .movable(false)
            .sense(Sense::hover())
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(pos2(x, y))
            .constrain_to(bounds)
            .show(ctx, |ui| {
                let colors = crate::theme::palette(ctx);
                egui::Frame::popup(ui.style())
                    .inner_margin(PADDING)
                    .fill(colors.surface)
                    .show(ui, |ui| {
                        ui.set_width(PICKER_WIDTH);
                        ui.label(RichText::new("Nhãn dán mặc định").strong());
                        ui.label(
                            RichText::new(if can_pick {
                                "Chọn nhãn dán để gửi ngay."
                            } else {
                                "Chưa thể gửi nhãn dán trong hội thoại này."
                            })
                            .size(visuals::TEXT_SMALL)
                            .color(colors.text_secondary),
                        );
                        egui::ScrollArea::vertical()
                            .max_height(grid_height)
                            .show(ui, |ui| {
                                let mut catalog = builtin_stickers().peekable();
                                ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                                while catalog.peek().is_some() {
                                    ui.horizontal(|ui| {
                                        for sticker in catalog.by_ref().take(COLUMNS) {
                                            let (rect, _) = ui.allocate_exact_size(
                                                vec2(TILE, TILE),
                                                Sense::hover(),
                                            );
                                            if !ui.is_rect_visible(rect) {
                                                continue;
                                            }
                                            let preview = state.stickers.thumbnail(ctx, sticker);
                                            let enabled = can_pick
                                                && matches!(&preview, Presentation::Image { .. });
                                            let id = ui.id().with(sticker.sticker_id);
                                            let button = ui
                                                .add_enabled_ui(enabled, |ui| {
                                                    ui.interact(rect, id, Sense::click())
                                                })
                                                .inner;
                                            let label =
                                                format!("Gửi nhãn dán {}", sticker.sticker_id);
                                            button.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::WidgetType::Button,
                                                    enabled,
                                                    &label,
                                                )
                                            });
                                            let hint = match &preview {
                                                Presentation::Image { .. } => label,
                                                Presentation::Loading => {
                                                    "Đang tải nhãn dán…".into()
                                                }
                                                Presentation::Unavailable(reason) => reason.clone(),
                                            };
                                            if enabled {
                                                buttons.push(button.id);
                                            }
                                            if button.clicked() {
                                                selection = Some(sticker);
                                            }
                                            if button.is_pointer_button_down_on()
                                                || button.hovered()
                                            {
                                                ui.painter().rect_filled(
                                                    rect,
                                                    12.0,
                                                    if button.is_pointer_button_down_on() {
                                                        colors.pressed
                                                    } else {
                                                        colors.hover
                                                    },
                                                );
                                            }
                                            visuals::focus_outline(ui, &button, rect, 12.0);
                                            paint_preview(ui, rect, preview);
                                            button
                                                .on_hover_text(&hint)
                                                .on_disabled_hover_text(hint);
                                        }
                                    });
                                }
                            });
                    });
            });
        super::message_actions::focus_popup_actions(ctx, tab, &buttons, &mut picker.action_focus);
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if let Some(sticker) = selection {
            let thread = picker.scope.thread.clone();
            let send = &mut state.sticker_send;
            send.request = send.request.wrapping_add(1).max(1);
            let accepted = state
                .worker
                .as_ref()
                .is_some_and(|worker| worker.send_sticker(send.request, thread.clone(), sticker));
            send.status = Some(InteractionNotice::new(
                thread.clone(),
                if accepted {
                    "Đang gửi nhãn dán…"
                } else {
                    "Chưa xếp được lượt gửi nhãn dán. Vui lòng thử lại."
                },
            ));
            if accepted {
                send.pending = Some((thread, send.request));
            }
            send.picker = None;
        } else if escape || (!picker.just_opened && response.response.clicked_elsewhere()) {
            state.sticker_send.picker = None;
        } else {
            picker.just_opened = false;
        }
    }
}

fn paint_preview(ui: &egui::Ui, rect: Rect, preview: Presentation) {
    if let Presentation::Image { texture, uv } = preview {
        let size = texture.size_vec2();
        let source = vec2(size.x * uv.width(), size.y * uv.height());
        let size = visuals::fit_image(source, rect.shrink(5.0).size(), ui.ctx().pixels_per_point());
        ui.painter().image(
            texture.id(),
            Rect::from_center_size(rect.center(), size),
            uv,
            egui::Color32::WHITE,
        );
    } else {
        visuals::source_icon(
            ui,
            Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
            "fa-chatbar-sticker",
            crate::theme::palette(ui.ctx()).text_disabled,
        );
    }
}

#[cfg(test)]
pub(super) fn fixture_thumbnails(cache: &mut super::stickers::StickerCache, ctx: &egui::Context) {
    const COLORS: [[u8; 4]; 6] = [
        [255, 204, 81, 255],
        [245, 151, 154, 255],
        [149, 203, 177, 255],
        [140, 196, 240, 255],
        [187, 163, 233, 255],
        [251, 166, 115, 255],
    ];
    const INK: [u8; 4] = [42, 53, 69, 255];
    for (index, sticker) in builtin_stickers().enumerate() {
        let mut bytes = Vec::new();
        for y in -32i32..32 {
            for x in -32i32..32 {
                let body = x.pow(2) + y.pow(2) < 28i32.pow(2);
                let eyes = (x.abs() - 10).pow(2) + (y + 7).pow(2) < 10;
                let smile = y > 9 && x.abs() + y < 20 && y + x.abs() / 3 > 12;
                let color = if body && (eyes || smile) {
                    INK
                } else if body {
                    COLORS[(index + index / 6) % COLORS.len()]
                } else {
                    [0; 4]
                };
                bytes.extend_from_slice(&color);
            }
        }
        cache.seed(ctx, sticker, 64, 64, bytes);
    }
}
