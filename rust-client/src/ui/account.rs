//! Account presence and anchored menus. Rendering never starts authentication or I/O.
use super::{avatar, visuals, ModalFocus, NativeApp, RAIL_WIDTH};
use eframe::egui::{self, pos2, vec2, Align, Align2, FontId, Layout, RichText, Sense, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PopupKind {
    Status,
    Settings,
}

#[derive(Clone)]
pub(super) struct Popup {
    kind: PopupKind,
    anchor: egui::Rect,
    account: Option<String>,
    auth_generation: Option<u64>,
    thread: Option<String>,
    generation: u64,
    just_opened: bool,
    pub(super) focus: Option<ModalFocus>,
}

impl NativeApp {
    pub(super) fn auth_generation(&self) -> Option<u64> {
        #[cfg(feature = "live-login")]
        return self.live.as_ref().map(|state| state.login.generation());
        #[cfg(not(feature = "live-login"))]
        None
    }

    pub(super) fn modal_focus_snapshot(&self, ctx: &egui::Context) -> Option<ModalFocus> {
        ctx.memory(|memory| memory.focused()).map(|id| ModalFocus {
            id,
            account: self.account_id().map(str::to_owned),
            auth_generation: self.auth_generation(),
            thread: self.model.selected.clone(),
            generation: self.model.generation,
        })
    }

    pub(super) fn prepare_account_popup_trigger(
        &mut self,
        kind: PopupKind,
        anchor: egui::Rect,
        response: &egui::Response,
    ) {
        // Keep an open popup attached to the rail/login control after a resize.
        if let Some(popup) = self
            .account_popup
            .as_mut()
            .filter(|popup| popup.kind == kind)
        {
            popup.anchor = anchor;
        }
        // TextEdit blurs on the press; the menu itself opens on release.
        if !self.modal_open()
            && response.is_pointer_button_down_on()
            && response
                .ctx
                .input(|input| input.pointer.button_pressed(egui::PointerButton::Primary))
        {
            self.account_trigger_focus = self.modal_focus_snapshot(&response.ctx);
        }
    }

    pub(super) fn account_popup_is(&self, kind: PopupKind) -> bool {
        self.account_popup
            .as_ref()
            .is_some_and(|popup| popup.kind == kind)
    }

    pub(super) fn toggle_account_popup(&mut self, kind: PopupKind, anchor: egui::Rect) {
        if self.account_popup_is(kind) {
            self.account_popup = None;
            return;
        }
        let focus = self.account_trigger_focus.take().filter(|focus| {
            focus.account.as_deref() == self.account_id()
                && focus.auth_generation == self.auth_generation()
                && focus.thread == self.model.selected
                && focus.generation == self.model.generation
        });
        self.account_popup = Some(Popup {
            kind,
            anchor,
            account: self.account_id().map(str::to_owned),
            auth_generation: self.auth_generation(),
            thread: self.model.selected.clone(),
            generation: self.model.generation,
            just_opened: true,
            focus,
        });
    }

    pub(super) fn sync_account_popup(&mut self) {
        if self.account_popup.as_ref().is_some_and(|popup| {
            popup.account.as_deref() != self.account_id()
                || popup.auth_generation != self.auth_generation()
                || popup.thread != self.model.selected
                || popup.generation != self.model.generation
                || self.settings_open
                || self.details
        }) {
            self.account_popup = None;
        }
    }

    fn account_status_with_palette(&self, colors: crate::theme::Palette) -> (&str, egui::Color32) {
        if self.demo {
            return ("Bản demo · không kết nối Zalo", colors.status_offline);
        }
        if !self.is_signed_in() {
            return ("Chưa đăng nhập", colors.status_offline);
        }
        #[cfg(feature = "messaging")]
        if let Some(state) = &self.live {
            use crate::app::live::LiveConnection;
            return match state.connection {
                LiveConnection::Online => ("Đã kết nối Zalo", colors.status_online),
                LiveConnection::Connecting => ("Đang kết nối…", colors.status_connecting),
                LiveConnection::Reconnecting => ("Đang kết nối lại…", colors.status_connecting),
                LiveConnection::Expired => ("Phiên đã hết hạn", colors.error),
                LiveConnection::Offline => ("Ngoại tuyến", colors.status_offline),
            };
        }
        (self.connection_label(), colors.status_offline)
    }

    pub(super) fn account_avatar(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let (row, response) = ui.allocate_exact_size(vec2(RAIL_WIDTH, 44.0), Sense::click());
        let rect = egui::Rect::from_center_size(row.center(), vec2(40.0, 40.0));
        avatar(ui, rect, self.account_label());
        let (status, color) = self.account_status_with_palette(colors);
        let badge = rect.right_bottom() - vec2(4.0, 4.0);
        ui.painter().circle_filled(badge, 7.0, colors.status_ring);
        ui.painter().circle_filled(badge, 5.0, color);
        visuals::nav_focus_outline(ui, &response, rect.expand(3.0));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("{} · {status}", self.account_label()),
            )
        });
        let response = response.on_hover_text(format!(
            "{}\n{status}\nBấm để xem trạng thái",
            self.account_label()
        ));
        self.prepare_account_popup_trigger(PopupKind::Status, rect, &response);
        if response.clicked() {
            self.toggle_account_popup(PopupKind::Status, rect);
        }
    }

    pub(super) fn show_account_popup(&mut self, ctx: &egui::Context) {
        self.sync_account_popup();
        let Some(popup) = self.account_popup.clone() else {
            return;
        };
        self.latch_modal_input(ctx);
        let colors = crate::theme::palette(ctx);
        let width: f32 = if popup.kind == PopupKind::Settings {
            244.0
        } else {
            320.0
        };
        let (position, pivot) = match popup.kind {
            PopupKind::Settings => (
                popup.anchor.right_bottom() + vec2(8.0, 0.0),
                Align2::LEFT_BOTTOM,
            ),
            PopupKind::Status => (popup.anchor.right_top() + vec2(8.0, 0.0), Align2::LEFT_TOP),
        };
        let mut appearance = false;
        let mut logout = false;
        let popup_id = egui::Id::new(match popup.kind {
            PopupKind::Settings => "account_settings_popup",
            PopupKind::Status => "account_status_popup",
        });
        let max_body_height = (ctx.screen_rect().height() - 48.0).max(100.0);
        let response = egui::Area::new(popup_id)
            .kind(egui::UiKind::Popup)
            .order(egui::Order::Foreground)
            .movable(false)
            .pivot(pivot)
            .fixed_pos(position)
            .constrain_to(ctx.screen_rect().shrink(8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(width.min((ctx.screen_rect().width() - 32.0).max(160.0)));
                        // An Area remembers its previous content size. Give the scroll
                        // body a viewport budget so a short menu/status cannot trap
                        // later recovery controls below the remembered clip height.
                        ui.set_max_height(max_body_height);
                        ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                            if popup.kind == PopupKind::Settings {
                                appearance =
                                    menu_entry(ui, "Thiết lập giao diện", colors.text_primary)
                                        .clicked();
                                if self.is_signed_in() {
                                    ui.separator();
                                    logout = menu_entry(ui, "Đăng xuất", colors.error).clicked();
                                }
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("account_status_scroll")
                                    .max_height(max_body_height)
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(self.account_label()).family(
                                                    egui::FontFamily::Name("Zalo Semibold".into()),
                                                ),
                                            )
                                            .wrap(),
                                        );
                                        let (status, _) = self.account_status_with_palette(colors);
                                        ui.label(
                                            RichText::new(status).color(colors.text_secondary),
                                        );
                                        #[cfg(feature = "messaging")]
                                        self.live_recovery(ui);
                                        #[cfg(feature = "messaging")]
                                        self.privacy_status_controls(ui);
                                        if self.demo {
                                            ui.add(
                                                egui::Label::new(
                                                    "Tin nhắn minh họa chỉ được lưu trên máy.",
                                                )
                                                .wrap(),
                                            );
                                        }
                                    });
                            }
                        });
                    });
            });
        let dismiss = ctx.input(|input| input.key_pressed(egui::Key::Escape))
            || (!popup.just_opened && response.response.clicked_elsewhere());
        if appearance {
            self.settings_open = true;
        }
        #[cfg(feature = "live-login")]
        if logout {
            if let Some(state) = &mut self.live {
                if state.account.is_some() {
                    state.logout_dialog = true;
                }
            }
        }
        if dismiss || appearance || logout {
            self.account_popup = None;
        } else if let Some(popup) = &mut self.account_popup {
            popup.just_opened = false;
        }
    }
}

fn menu_entry(ui: &mut egui::Ui, label: &str, color: egui::Color32) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if response.hovered() || response.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            rect,
            3.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else {
                colors.hover
            },
        );
    }
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(visuals::TEXT_BODY),
        color,
    );
    visuals::focus_outline(ui, &response, rect, 3.0);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
}
