//! Native sticker picker for built-in and authenticated catalog results.
use super::{
    message_actions::Scope, stickers::Presentation, visuals, InteractionNotice, ModalFocus,
    NativeApp,
};
use crate::{
    app::{
        live::{LiveConnection, LiveEvent, StickerOutcome},
        stickers::builtin_stickers,
    },
    domain::StickerDescriptor,
    protocol::sticker_catalog::valid_query,
};
use eframe::egui::{self, pos2, vec2, Align2, Rect, RichText, Sense};

const COLUMNS: usize = 6;
const TILE: f32 = 44.0;
const GAP: f32 = 4.0;
const PADDING: f32 = 10.0;
const PICKER_WIDTH: f32 = COLUMNS as f32 * TILE + (COLUMNS - 1) as f32 * GAP;
#[cfg(test)]
const CATALOG_FIXTURE: [StickerDescriptor; 2] = [
    StickerDescriptor {
        sticker_id: 20695,
        category_id: 10047,
        sticker_type: 3,
    },
    StickerDescriptor {
        sticker_id: 20696,
        category_id: 10047,
        sticker_type: 3,
    },
];

struct Picker {
    scope: Scope,
    open_id: u64,
    anchor: Rect,
    just_opened: bool,
    action_focus: Option<egui::Id>,
    focus: Option<ModalFocus>,
    query: String,
    searching: bool,
    lookup: Lookup,
}

enum Lookup {
    Idle,
    Pending(u64),
    Found(u64, Vec<StickerDescriptor>),
    Unavailable,
}

#[derive(Default)]
pub(super) struct StickerSend {
    picker: Option<Picker>,
    request: u64,
    pending: Option<(String, u64)>,
    status: Option<InteractionNotice>,
    search_available: bool,
    #[cfg(test)]
    catalog_fixture: bool,
}

impl StickerSend {
    #[cfg(test)]
    pub(super) fn fixture_catalog(&mut self) {
        self.search_available = true;
        self.catalog_fixture = true;
    }

    pub(super) fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    pub(super) fn take_focus(&mut self) -> Option<ModalFocus> {
        self.picker.as_mut()?.focus.take()
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::StickerSearchCapability { available, .. } => {
                self.search_available = *available;
                return;
            }
            LiveEvent::StickersFound {
                request_id,
                thread,
                stickers,
                ..
            } => {
                if let Some(picker) = self.picker.as_mut().filter(|picker| {
                    picker.scope.thread == *thread
                        && matches!(picker.lookup, Lookup::Pending(id) if id == *request_id)
                }) {
                    picker.lookup = match stickers {
                        Some(stickers) => Lookup::Found(*request_id, stickers.clone()),
                        None => Lookup::Unavailable,
                    };
                }
                return;
            }
            _ => {}
        }
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
        self.search_available = false;
        self.picker = None;
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
                    state.sticker_send.request = state.sticker_send.request.wrapping_add(1).max(1);
                    #[cfg(test)]
                    let (searching, query, lookup) =
                        if std::mem::take(&mut state.sticker_send.catalog_fixture) {
                            (
                                true,
                                "chào".into(),
                                Lookup::Found(1, CATALOG_FIXTURE.to_vec()),
                            )
                        } else {
                            (false, String::new(), Lookup::Idle)
                        };
                    #[cfg(not(test))]
                    let (searching, query, lookup) = (false, String::new(), Lookup::Idle);
                    state.sticker_send.picker = Some(Picker {
                        scope,
                        open_id: state.sticker_send.request,
                        anchor: button.rect,
                        just_opened: true,
                        action_focus: None,
                        focus,
                        query,
                        searching,
                        lookup,
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
        let ime_active = self.ime_composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
            });
        let Some(state) = &mut self.live else { return };
        let search_available = state.sticker_send.search_available;
        let Some(picker) = &mut state.sticker_send.picker else {
            return;
        };
        let bounds = ctx.screen_rect().shrink(8.0);
        let x = picker
            .anchor
            .left()
            .min(bounds.right() - PICKER_WIDTH - PADDING * 2.0);
        let y = picker.anchor.top() - 8.0;
        let mut selection = None;
        let mut search = false;
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
                        ui.horizontal(|ui| {
                            for (searching, label) in [(false, "Mặc định"), (true, "Tìm nhãn dán")] {
                                let tab = ui.add_sized(
                                    vec2(68.0, 36.0),
                                    egui::SelectableLabel::new(picker.searching == searching, label),
                                );
                                buttons.push(tab.id);
                                if tab.clicked() {
                                    picker.searching = searching;
                                }
                                visuals::focus_outline(ui, &tab, tab.rect, 4.0);
                            }
                        });
                        let mut search_id = None;
                        let mut entries = Vec::new();
                        let label = if picker.searching {
                            ui.horizontal(|ui| {
                                let edit = ui.add_enabled(
                                    can_pick && search_available,
                                    egui::TextEdit::singleline(&mut picker.query)
                                        .id(egui::Id::new((
                                            "sticker_catalog_query", &picker.scope.account, &picker.scope.thread,
                                            picker.scope.generation, picker.scope.auth_generation, picker.open_id,
                                        )))
                                        .hint_text("Nhập từ khóa")
                                        .char_limit(64)
                                        .return_key(None)
                                        .min_size(vec2(0.0, 36.0))
                                        .desired_width(PICKER_WIDTH - 58.0),
                                );
                                if edit.changed() {
                                    picker.lookup = Lookup::Idle;
                                }
                                if edit.enabled() {
                                    buttons.push(edit.id);
                                }
                                let ready =
                                    can_pick && search_available && valid_query(picker.query.trim())
                                        && !matches!(picker.lookup, Lookup::Pending(_)) && !ime_active;
                                let submit = ui.add_enabled(
                                    ready, egui::Button::new("Tìm").min_size(vec2(44.0, 36.0)),
                                );
                                if submit.enabled() {
                                    buttons.push(submit.id);
                                }
                                visuals::focus_outline(ui, &submit, submit.rect, 4.0);
                                let enter = ready && edit.has_focus()
                                    && ctx.input(|input| input.events.iter().any(|event| matches!(
                                        event, egui::Event::Key {
                                            key: egui::Key::Enter, pressed: true, repeat: false, modifiers, ..
                                        } if *modifiers == egui::Modifiers::NONE
                                    )))
                                    && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                                search = submit.clicked() || enter;
                            });
                            if !search_available {
                                Some("Danh mục nhãn dán hiện không khả dụng.")
                            } else {
                                match &picker.lookup {
                                    Lookup::Idle => Some("Tìm theo từ khóa tiếng Việt."),
                                    Lookup::Pending(_) => Some("Đang tìm nhãn dán…"),
                                    Lookup::Unavailable => Some("Chưa tải được nhãn dán. Vui lòng thử lại."),
                                    Lookup::Found(_, stickers) if stickers.is_empty() => {
                                        Some("Không có nhãn dán phù hợp.")
                                    }
                                    Lookup::Found(id, stickers) => {
                                        search_id = Some(*id);
                                        entries.extend(stickers.iter().copied());
                                        None
                                    }
                                }
                            }
                        } else {
                            entries.extend(builtin_stickers());
                            Some(if can_pick {
                                "Chọn nhãn dán để gửi ngay."
                            } else {
                                "Chưa thể gửi nhãn dán trong hội thoại này."
                            })
                        };
                        if let Some(label) = label {
                            ui.label(RichText::new(label)
                                .size(visuals::TEXT_SMALL)
                                .color(colors.text_secondary));
                        }
                        let header = if picker.searching { 118.0 } else { 75.0 };
                        let grid_height = (y - bounds.top() - header).clamp(TILE, 240.0);
                        egui::ScrollArea::vertical()
                            .id_salt(picker.searching)
                            .max_height(grid_height)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                                for row in entries.chunks(COLUMNS) {
                                    ui.horizontal(|ui| {
                                        for &sticker in row {
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
                                            let id = ui.id().with(sticker);
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
                                                selection = Some((sticker, search_id));
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
        if let Some(focused) = ctx
            .memory(|memory| memory.focused())
            .filter(|id| buttons.contains(id))
        {
            picker.action_focus = Some(focused);
        }
        let escape = !ime_active
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if search {
            state.sticker_send.request = state.sticker_send.request.wrapping_add(1).max(1);
            let id = state.sticker_send.request;
            let queued = state.worker.as_ref().is_some_and(|worker| {
                worker.search_stickers(id, picker.scope.thread.clone(), picker.query.trim().into())
            });
            picker.lookup = if queued {
                Lookup::Pending(id)
            } else {
                Lookup::Unavailable
            };
            picker.just_opened = false;
        } else if let Some((sticker, search_id)) = selection {
            let thread = picker.scope.thread.clone();
            let send = &mut state.sticker_send;
            send.request = send.request.wrapping_add(1).max(1);
            let accepted = state.worker.as_ref().is_some_and(|worker| {
                if let Some(search_id) = search_id {
                    worker.send_catalog_sticker(send.request, search_id, thread.clone(), sticker)
                } else {
                    worker.send_sticker(send.request, thread.clone(), sticker)
                }
            });
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
    for (index, sticker) in builtin_stickers().chain(CATALOG_FIXTURE).enumerate() {
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
