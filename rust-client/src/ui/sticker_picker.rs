//! Native sticker picker for built-in stickers, search and authenticated pack browsing.
use super::{
    message_actions::Scope, stickers::Presentation, visuals, InteractionNotice, ModalFocus,
    NativeApp,
};
use crate::{
    app::{
        live::{LiveConnection, LiveEvent, LivePrivacyStatus, StickerOutcome, StickerPack},
        stickers::builtin_stickers,
    },
    domain::StickerDescriptor,
    protocol::sticker_catalog::{valid_query, MAX_PACKS, MAX_PACK_ITEMS},
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
    view: View,
    lookup: Lookup<StickerDescriptor>,
    packs: Lookup<StickerPack>,
    pack: Option<StickerPack>,
    pack_tiles: Lookup<StickerDescriptor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum View {
    BuiltIn,
    Search,
    Packs,
}

enum Lookup<T> {
    Idle,
    Pending(u64),
    Found(u64, Vec<T>),
    Unavailable,
}

#[derive(Clone, Copy)]
enum CatalogAction {
    Search,
    LoadPacks,
    BrowsePack { list_id: u64, category_id: u64 },
}

#[cfg(test)]
enum PickerFixture {
    Search,
    Packs { detail: bool },
}

impl Picker {
    fn new(scope: Scope, open_id: u64, anchor: Rect, focus: Option<ModalFocus>) -> Self {
        Self {
            scope,
            open_id,
            anchor,
            focus,
            just_opened: true,
            action_focus: None,
            query: String::new(),
            view: View::BuiltIn,
            lookup: Lookup::Idle,
            packs: Lookup::Idle,
            pack: None,
            pack_tiles: Lookup::Idle,
        }
    }

    fn enter(&mut self, view: View) {
        if self.view != view {
            // Changing tabs discards actionable results, while keeping the query and pack list.
            self.view = view;
            self.lookup = Lookup::Idle;
            self.back_to_packs();
            if matches!(self.packs, Lookup::Pending(_)) {
                self.packs = Lookup::Idle;
            }
        }
    }

    fn back_to_packs(&mut self) {
        self.pack = None;
        self.pack_tiles = Lookup::Idle;
    }
}

#[derive(Default)]
pub(super) struct StickerSend {
    picker: Option<Picker>,
    request: u64,
    pending: Option<(String, u64)>,
    status: Option<InteractionNotice>,
    search_available: bool,
    #[cfg(test)]
    catalog_fixture: Option<PickerFixture>,
}

impl StickerSend {
    #[cfg(test)]
    pub(super) fn fixture_catalog(&mut self) {
        self.search_available = true;
        self.catalog_fixture = Some(PickerFixture::Search);
    }

    #[cfg(test)]
    pub(super) fn fixture_packs(&mut self, detail: bool) {
        self.search_available = true;
        self.catalog_fixture = Some(PickerFixture::Packs { detail });
    }

    pub(super) fn clear_picker(&mut self) {
        self.picker = None;
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
                account_id,
                request_id,
                thread,
                stickers,
                ..
            } => {
                if let Some(picker) = self.picker.as_mut().filter(|picker| {
                    picker.view == View::Search
                        && picker.scope.account == *account_id
                        && picker.scope.thread == *thread
                        && matches!(picker.lookup, Lookup::Pending(id) if id == *request_id)
                }) {
                    picker.lookup = match stickers {
                        Some(stickers) => Lookup::Found(*request_id, stickers.clone()),
                        None => Lookup::Unavailable,
                    };
                }
                return;
            }
            LiveEvent::StickerPacksFound {
                account_id,
                request_id,
                thread,
                packs,
            } => {
                if let Some(picker) = self.picker.as_mut().filter(|picker| {
                    picker.view == View::Packs
                        && picker.scope.account == *account_id
                        && picker.scope.thread == *thread
                        && matches!(picker.packs, Lookup::Pending(id) if id == *request_id)
                }) {
                    picker.packs = match packs {
                        Some(packs) if packs.len() <= MAX_PACKS => {
                            Lookup::Found(*request_id, packs.clone())
                        }
                        _ => Lookup::Unavailable,
                    };
                }
                return;
            }
            LiveEvent::StickerPackFound {
                account_id,
                request_id,
                thread,
                category_id,
                stickers,
            } => {
                if let Some(picker) = self.picker.as_mut().filter(|picker| {
                    picker.view == View::Packs
                        && picker.scope.account == *account_id
                        && picker.scope.thread == *thread
                        && picker
                            .pack
                            .as_ref()
                            .is_some_and(|pack| pack.category_id == *category_id)
                        && matches!(picker.pack_tiles, Lookup::Pending(id) if id == *request_id)
                }) {
                    picker.pack_tiles = match stickers {
                        Some(stickers)
                            if stickers.len() <= MAX_PACK_ITEMS
                                && stickers.iter().all(|sticker| {
                                    sticker.category_id == *category_id && sticker.sticker_type == 3
                                }) =>
                        {
                            Lookup::Found(*request_id, stickers.clone())
                        }
                        _ => Lookup::Unavailable,
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
            && self
                .live
                .as_ref()
                .is_some_and(|state| state.privacy.status == LivePrivacyStatus::Ready)
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
                    let picker = Picker::new(scope, state.sticker_send.request, button.rect, focus);
                    #[cfg(test)]
                    let picker = {
                        let mut picker = picker;
                        match state.sticker_send.catalog_fixture.take() {
                            Some(PickerFixture::Search) => {
                                picker.view = View::Search;
                                picker.query = "chào".into();
                                picker.lookup = Lookup::Found(1, CATALOG_FIXTURE.to_vec());
                                picker
                            }
                            Some(PickerFixture::Packs { detail }) => {
                                picker.view = View::Packs;
                                let packs = [
                                    (10047, "Mèo nhỏ chào bạn"),
                                    (10048, "Một ngày nhiều niềm vui và những điều bất ngờ"),
                                    (10049, "Những chú cún vui vẻ"),
                                    (10050, "Bếp nhà ngày mưa"),
                                    (10051, "Chuyến đi cuối tuần"),
                                ]
                                .into_iter()
                                .map(|(category_id, name)| StickerPack {
                                    category_id,
                                    name: name.into(),
                                })
                                .collect::<Vec<_>>();
                                if detail {
                                    picker.pack = packs.first().cloned();
                                    picker.pack_tiles = Lookup::Found(2, CATALOG_FIXTURE.to_vec());
                                }
                                picker.packs = Lookup::Found(1, packs);
                                picker
                            }
                            None => picker,
                        }
                    };
                    state.sticker_send.picker = Some(picker);
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
        let mut action = None;
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
                            for (view, label, width) in [
                                (View::BuiltIn, "Mặc định", 68.0),
                                (View::Search, "Tìm nhãn dán", 98.0),
                                (View::Packs, "Bộ nhãn dán", 96.0),
                            ] {
                                let tab = ui.add_enabled(
                                    view != View::Packs || search_available,
                                    egui::Button::new(label).selected(picker.view == view)
                                        .rounding(10.0).min_size(vec2(width, 36.0)),
                                );
                                if tab.enabled() {
                                    buttons.push(tab.id);
                                }
                                if tab.clicked() {
                                    picker.enter(view);
                                }
                                visuals::focus_outline(ui, &tab, tab.rect, 10.0);
                                tab.on_disabled_hover_text("Phiên này chưa có danh mục bộ nhãn dán.");
                            }
                        });
                        let mut search_id = None;
                        let mut entries = Vec::new();
                        let label = if picker.view == View::Search {
                            ui.horizontal(|ui| {
                                let edit = ui.add_enabled(
                                    can_pick && search_available,
                                    egui::TextEdit::singleline(&mut picker.query)
                                        .id(egui::Id::new((
                                            "sticker_catalog_query", &picker.scope.account, &picker.scope.thread,
                                            picker.scope.generation, picker.scope.auth_generation, picker.open_id,
                                        )))
                                        .hint_text("Nhập từ khóa")
                                        .vertical_align(egui::Align::Center)
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
                                if submit.clicked() || enter {
                                    action = Some(CatalogAction::Search);
                                }
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
                        } else if picker.view == View::Packs && !search_available {
                            Some("Danh mục bộ nhãn dán hiện không khả dụng.")
                        } else if picker.view == View::Packs {
                            show_packs(ui, picker, can_pick && search_available, y - bounds.top(), &mut buttons, &mut action);
                            match &picker.pack_tiles {
                                Lookup::Found(id, stickers) if !stickers.is_empty() => {
                                    search_id = Some(*id);
                                    entries.extend(stickers.iter().copied());
                                    None
                                }
                                Lookup::Found(..) => Some("Bộ này chưa có nhãn dán phù hợp."),
                                Lookup::Unavailable => Some("Chưa mở được bộ nhãn dán. Hãy thử lại hoặc cập nhật danh sách."),
                                Lookup::Pending(_) => Some("Đang tải nhãn dán trong bộ…"),
                                Lookup::Idle if picker.pack.is_some() => Some("Đang tải nhãn dán trong bộ…"),
                                Lookup::Idle => None,
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
                        let header = if picker.view == View::BuiltIn { 75.0 } else { 118.0 };
                        let grid_height = (y - bounds.top() - header).clamp(TILE, 240.0);
                        egui::ScrollArea::vertical()
                            .id_salt((picker.view, search_id))
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
        if let Some(action) = action {
            state.sticker_send.request = state.sticker_send.request.wrapping_add(1).max(1);
            let id = state.sticker_send.request;
            let thread = picker.scope.thread.clone();
            let queued = state.worker.as_ref().is_some_and(|worker| match action {
                CatalogAction::Search => {
                    worker.search_stickers(id, thread, picker.query.trim().into())
                }
                CatalogAction::LoadPacks => worker.sticker_packs(id, thread),
                CatalogAction::BrowsePack {
                    list_id,
                    category_id,
                } => worker.browse_sticker_pack(id, thread, list_id, category_id),
            });
            picker.lookup = Lookup::Idle;
            picker.pack_tiles = Lookup::Idle;
            match action {
                CatalogAction::Search => picker.lookup = lookup_started(queued, id),
                CatalogAction::LoadPacks => picker.packs = lookup_started(queued, id),
                CatalogAction::BrowsePack { .. } => picker.pack_tiles = lookup_started(queued, id),
            }
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
        } else if escape && picker.view == View::Packs && picker.pack.is_some() {
            picker.back_to_packs();
            picker.action_focus = None;
        } else if escape || (!picker.just_opened && response.response.clicked_elsewhere()) {
            state.sticker_send.picker = None;
        } else {
            picker.just_opened = false;
        }
    }
}

fn lookup_started<T>(queued: bool, id: u64) -> Lookup<T> {
    if queued {
        Lookup::Pending(id)
    } else {
        Lookup::Unavailable
    }
}

fn show_packs(
    ui: &mut egui::Ui,
    picker: &mut Picker,
    available: bool,
    height: f32,
    buttons: &mut Vec<egui::Id>,
    action: &mut Option<CatalogAction>,
) {
    let colors = crate::theme::palette(ui.ctx());
    if let Some(pack) = &picker.pack {
        let mut back_clicked = false;
        ui.horizontal(|ui| {
            let back = ui.add(egui::Button::new("Các bộ").min_size(vec2(72.0, 36.0)));
            buttons.push(back.id);
            visuals::focus_outline(ui, &back, back.rect, 10.0);
            back_clicked = back.clicked();
            back.on_hover_text("Quay lại danh sách bộ nhãn dán (Esc)");
            ui.add(egui::Label::new(RichText::new(&pack.name).strong()).truncate())
                .on_hover_text(&pack.name);
        });
        if back_clicked {
            picker.back_to_packs();
            return;
        }
        if matches!(picker.pack_tiles, Lookup::Unavailable) {
            if let Lookup::Found(list_id, _) = &picker.packs {
                let retry = ui.add_enabled(
                    available,
                    egui::Button::new("Thử lại").min_size(vec2(80.0, 36.0)),
                );
                if retry.enabled() {
                    buttons.push(retry.id);
                }
                visuals::focus_outline(ui, &retry, retry.rect, 10.0);
                if retry.clicked() {
                    *action = Some(CatalogAction::BrowsePack {
                        list_id: *list_id,
                        category_id: pack.category_id,
                    });
                }
            }
        }
        return;
    }

    let refresh = ui.add_enabled(
        available && !matches!(picker.packs, Lookup::Pending(_)),
        egui::Button::new("Cập nhật").min_size(vec2(88.0, 36.0)),
    );
    if refresh.enabled() {
        buttons.push(refresh.id);
    }
    visuals::focus_outline(ui, &refresh, refresh.rect, 10.0);
    if refresh.clicked() || (available && matches!(picker.packs, Lookup::Idle)) {
        *action = Some(CatalogAction::LoadPacks);
    }
    let status = match &picker.packs {
        Lookup::Idle if !available => "Kết nối lại để tải danh sách bộ nhãn dán.",
        Lookup::Idle | Lookup::Pending(_) => "Đang tải danh sách bộ nhãn dán…",
        Lookup::Unavailable => "Chưa tải được danh sách bộ nhãn dán. Hãy thử cập nhật.",
        Lookup::Found(_, packs) if packs.is_empty() => "Danh mục chưa có bộ nhãn dán phù hợp.",
        Lookup::Found(id, packs) => {
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .id_salt(("sticker_packs", picker.open_id))
                .max_height((height - 126.0).clamp(TILE, 240.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for pack in packs {
                        let response = ui
                            .push_id(pack.category_id, |ui| {
                                super::message_actions::action_row_height(
                                    ui,
                                    &pack.name,
                                    "fa-chatbar-sticker",
                                    available,
                                    false,
                                    TILE,
                                )
                            })
                            .inner;
                        if response.enabled() {
                            buttons.push(response.id);
                            if response.has_focus() && !ui.clip_rect().contains_rect(response.rect)
                            {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        if response.clicked() {
                            chosen = Some(pack.clone());
                        }
                        response.on_hover_text(&pack.name);
                    }
                });
            if let Some(pack) = chosen {
                *action = Some(CatalogAction::BrowsePack {
                    list_id: *id,
                    category_id: pack.category_id,
                });
                picker.pack = Some(pack);
            }
            return;
        }
    };
    ui.label(
        RichText::new(status)
            .size(visuals::TEXT_SMALL)
            .color(colors.text_secondary),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn picker() -> Picker {
        Picker::new(
            Scope {
                account: "11".into(),
                thread: "d:22".into(),
                generation: 4,
                auth_generation: Some(3),
            },
            1,
            Rect::ZERO,
            None,
        )
    }

    fn pack(category_id: u64) -> StickerPack {
        StickerPack {
            category_id,
            name: "Bộ kiểm thử".into(),
        }
    }

    #[test]
    fn pack_rows_and_tiles_accept_only_matching_scope_and_navigation() {
        let mut picker = picker();
        picker.enter(View::Packs);
        picker.packs = Lookup::Pending(2);
        let mut send = StickerSend {
            picker: Some(picker),
            ..Default::default()
        };
        let list = |account: &str, thread: &str, request_id| LiveEvent::StickerPacksFound {
            account_id: account.into(),
            thread: thread.into(),
            request_id,
            packs: Some(vec![pack(10047), pack(10048)]),
        };
        send.observe(&list("12", "d:22", 2));
        send.observe(&list("11", "d:33", 2));
        send.observe(&list("11", "d:22", 9));
        assert!(matches!(
            send.picker.as_ref().unwrap().packs,
            Lookup::Pending(2)
        ));
        send.observe(&list("11", "d:22", 2));
        let picker = send.picker.as_mut().unwrap();
        assert!(matches!(&picker.packs, Lookup::Found(2, packs) if packs.len() == 2));
        picker.pack = Some(pack(10047));
        picker.pack_tiles = Lookup::Pending(3);
        let detail = |category_id, request_id, stickers| LiveEvent::StickerPackFound {
            account_id: "11".into(),
            thread: "d:22".into(),
            request_id,
            category_id,
            stickers,
        };
        send.observe(&detail(10048, 3, Some(CATALOG_FIXTURE.to_vec())));
        send.observe(&detail(10047, 9, Some(CATALOG_FIXTURE.to_vec())));
        assert!(matches!(
            send.picker.as_ref().unwrap().pack_tiles,
            Lookup::Pending(3)
        ));
        send.picker.as_mut().unwrap().back_to_packs();
        send.observe(&detail(10047, 3, Some(CATALOG_FIXTURE.to_vec())));
        assert!(matches!(
            send.picker.as_ref().unwrap().pack_tiles,
            Lookup::Idle
        ));

        let picker = send.picker.as_mut().unwrap();
        picker.pack = Some(pack(10047));
        picker.pack_tiles = Lookup::Pending(4);
        send.observe(&detail(10047, 4, Some(CATALOG_FIXTURE.to_vec())));
        assert!(
            matches!(&send.picker.as_ref().unwrap().pack_tiles, Lookup::Found(4, stickers) if stickers == &CATALOG_FIXTURE)
        );
        let picker = send.picker.as_mut().unwrap();
        picker.enter(View::Search);
        assert!(picker.pack.is_none());
        assert!(matches!(picker.pack_tiles, Lookup::Idle));
        assert!(matches!(picker.packs, Lookup::Found(2, _)));
    }

    #[test]
    fn failed_pack_list_does_not_prevent_default_or_keyword_navigation() {
        let mut picker = picker();
        picker.query = "chào".into();
        picker.enter(View::Packs);
        picker.packs = Lookup::Pending(2);
        let mut send = StickerSend {
            picker: Some(picker),
            ..Default::default()
        };
        send.observe(&LiveEvent::StickerPacksFound {
            account_id: "11".into(),
            thread: "d:22".into(),
            request_id: 2,
            packs: None,
        });
        let picker = send.picker.as_mut().unwrap();
        assert!(matches!(picker.packs, Lookup::Unavailable));
        picker.enter(View::BuiltIn);
        assert_eq!(picker.view, View::BuiltIn);
        picker.enter(View::Search);
        assert_eq!(picker.query, "chào");
        assert!(matches!(picker.lookup, Lookup::Idle));
    }
}
