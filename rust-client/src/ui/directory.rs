//! Bounded account-scoped directory presentation; all cache/network work belongs to the worker.
use super::{visuals, NativeApp};
use crate::{
    app::live::{LiveCommand, LiveEvent, LiveWorker, ThreadKind},
    store::directory::{DirectoryCursor, DirectoryRecord},
};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense};
use std::time::{Duration, Instant};

const PAGE_SIZE: usize = 100;
const MAX_PREVIOUS: usize = crate::store::directory::MAX_DIRECTORY_ROWS / PAGE_SIZE;
const SEARCH_DELAY: Duration = Duration::from_millis(180);

#[derive(Default)]
pub(super) struct DirectoryView {
    pub visible: bool,
    is_group: bool,
    search: String,
    query: String,
    generation: u64,
    entries: Vec<DirectoryRecord>,
    after: Option<DirectoryCursor>,
    next: Option<DirectoryCursor>,
    previous: Vec<Option<DirectoryCursor>>,
    pending: bool,
    block_rows: bool,
    dirty: bool,
    deadline: Option<Instant>,
    error: Option<String>,
    loaded: bool,
    sync_generation: u64,
    syncing: bool,
    summary: String,
    detail: String,
}

impl DirectoryView {
    pub fn activate(&mut self, ctx: &egui::Context) {
        self.visible = true;
        if self.dirty || (!self.loaded && !self.pending) {
            self.queue_reload(ctx);
        }
    }
    fn reset_query(&mut self, ctx: &egui::Context) {
        self.query = self.search.trim().to_owned();
        self.entries.clear();
        self.after = None;
        self.next = None;
        self.previous.clear();
        self.pending = false;
        self.loaded = false;
        self.error = None;
        self.queue_reload(ctx);
    }
    fn queue_reload(&mut self, ctx: &egui::Context) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        self.block_rows = false;
        self.deadline = Some(Instant::now() + SEARCH_DELAY);
        ctx.request_repaint_after(SEARCH_DELAY);
    }
    pub fn poll_request(&mut self, worker: Option<&LiveWorker>, ctx: &egui::Context) {
        let Some(deadline) = self.deadline else {
            return;
        };
        if Instant::now() < deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            return;
        }
        self.deadline = None;
        self.request(worker, self.after.clone());
    }
    fn request(&mut self, worker: Option<&LiveWorker>, after: Option<DirectoryCursor>) -> bool {
        let Some(worker) = worker else {
            self.error =
                Some("Chưa đọc được danh bạ. Chọn Thử lại để khởi động lại gửi nhận.".into());
            return false;
        };
        let generation = self.generation.wrapping_add(1);
        match worker.submit(LiveCommand::Directory {
            generation,
            is_group: self.is_group,
            query: self.query.clone(),
            after: after.clone(),
        }) {
            Ok(()) => {
                self.generation = generation;
                self.block_rows = self.after != after;
                self.after = after;
                self.pending = true;
                self.dirty = false;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }
    pub fn worker_closed(&mut self) {
        if self.pending || self.syncing {
            self.error =
                Some("Chưa đọc xong danh bạ. Dữ liệu đã lưu được giữ nguyên; chọn Thử lại.".into());
        }
        self.pending = false;
        self.syncing = false;
        self.deadline = None;
    }
    pub fn apply(&mut self, event: &LiveEvent, account: &str, ctx: &egui::Context) {
        if event.account_id() != account {
            return;
        }
        match event {
            LiveEvent::DirectoryPage {
                generation,
                is_group,
                query,
                entries,
                next,
                ..
            } if *generation == self.generation
                && *is_group == self.is_group
                && query == &self.query =>
            {
                if entries.len() > PAGE_SIZE
                    || entries.iter().any(|entry| entry.is_group != self.is_group)
                {
                    self.pending = false;
                    self.error = Some("Trang danh bạ không hợp lệ. Hãy thử lại.".into());
                    return;
                }
                self.entries = entries.clone();
                self.next = next.clone();
                self.pending = false;
                self.loaded = true;
                self.error = None;
            }
            LiveEvent::DirectoryFailed {
                generation, error, ..
            } if *generation == self.generation => {
                self.pending = false;
                self.error = Some(error.to_string());
            }
            LiveEvent::DirectoryStatus {
                generation,
                syncing,
                report,
                ..
            } if *generation == self.sync_generation => {
                self.syncing = *syncing;
                self.summary = if *syncing {
                    "Đang cập nhật danh bạ từ Zalo…".into()
                } else if report.failures.is_empty()
                    && report.completed_friends
                    && report.completed_groups
                {
                    format!(
                        "Đã cập nhật {} liên hệ · {} nhóm",
                        report.contacts, report.groups
                    )
                } else {
                    "Cập nhật chưa đầy đủ · vẫn dùng được dữ liệu đã lưu".into()
                };
                self.detail = report
                    .failures
                    .iter()
                    .take(8)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            LiveEvent::DirectoryChanged { .. } => {
                self.dirty = true;
                if self.visible {
                    self.queue_reload(ctx);
                }
            }
            _ => {}
        }
    }
    fn show(&mut self, ui: &mut egui::Ui, selected: Option<&str>) -> Option<Action> {
        let mut action = None;
        ui.spacing_mut().item_spacing.y = 8.0;
        egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Danh bạ").strong().size(18.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!self.syncing, egui::Button::new("Cập nhật").small()).on_hover_text("Cập nhật liên hệ, nhóm và các bản xem trước từ Zalo; không phụ thuộc kết nối nhận tin.").clicked() { action = Some(Action::Refresh); }
                });
            });
            ui.horizontal(|ui| {
                let changed = ui.selectable_value(&mut self.is_group, false, "Liên hệ").changed()
                    | ui.selectable_value(&mut self.is_group, true, "Nhóm").changed();
                if changed { self.reset_query(ui.ctx()); }
            });
            let response = ui.add(egui::TextEdit::singleline(&mut self.search).id_source("directory_search").hint_text(if self.is_group { "Tìm tên nhóm" } else { "Tìm tên trong danh bạ" }).char_limit(128).desired_width(f32::INFINITY));
            if response.changed() { self.reset_query(ui.ctx()); }
            ui.label(RichText::new("Danh bạ đã lưu trên thiết bị").size(10.5).color(visuals::MUTED));
            if !self.summary.is_empty() {
                ui.add(egui::Label::new(RichText::new(&self.summary).size(10.5).color(visuals::MUTED)).wrap()).on_hover_text(&self.detail);
            }
            if let Some(error) = &self.error {
                ui.add(egui::Label::new(RichText::new(error).size(11.0).color(visuals::INK)).wrap());
                if ui.small_button("Thử lại").clicked() { action = Some(Action::Retry); }
            }
        });
        let footer_height = 43.0;
        let height = (ui.available_height() - footer_height).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt(("directory_page", self.is_group, self.generation))
            .auto_shrink([false, false])
            .max_height(height)
            .show(ui, |ui| {
                ui.set_min_height(height);
                if self.pending || self.deadline.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Đang đọc danh bạ…");
                    });
                }
                if self.loaded && self.entries.is_empty() {
                    egui::Frame::none().inner_margin(18.0).show(ui, |ui| {
                        ui.label(if self.query.is_empty() {
                            "Chưa có dữ liệu danh bạ trên thiết bị. Chọn Cập nhật để lấy từ Zalo."
                        } else {
                            "Không có tên phù hợp trong danh bạ đã lưu."
                        });
                    });
                }
                for entry in &self.entries {
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(ui.available_width(), 68.0),
                        if self.pending && self.block_rows {
                            Sense::hover()
                        } else {
                            Sense::click()
                        },
                    );
                    let is_selected = selected == Some(entry.thread_id().as_str());
                    if is_selected || response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            if is_selected {
                                visuals::PALE_BLUE
                            } else {
                                visuals::HOVER
                            },
                        );
                    }
                    visuals::avatar(
                        ui,
                        egui::Rect::from_center_size(
                            egui::pos2(rect.left() + 35.0, rect.center().y),
                            vec2(40.0, 40.0),
                        ),
                        &entry.display_name,
                    );
                    let mut text =
                        ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                            rect.left_top() + vec2(66.0, 12.0),
                            rect.right_bottom() - vec2(12.0, 8.0),
                        )));
                    text.add(
                        egui::Label::new(
                            RichText::new(&entry.display_name)
                                .size(14.0)
                                .color(visuals::INK),
                        )
                        .truncate(),
                    );
                    let subtitle = if entry.is_blocked == Some(true) {
                        "Đã chặn".into()
                    } else if entry.is_group {
                        entry
                            .member_count
                            .map(|n| format!("{n} thành viên"))
                            .unwrap_or_else(|| "Nhóm đã lưu".into())
                    } else if entry.is_friend == Some(true) {
                        "Bạn bè".into()
                    } else {
                        "Liên hệ đã lưu".into()
                    };
                    text.label(RichText::new(subtitle).size(11.0).color(visuals::MUTED));
                    if response.clicked() && (!self.pending || !self.block_rows) {
                        action = Some(Action::Open(entry.clone()));
                    }
                    response.on_hover_text(format!("Mở hội thoại · ID {}", entry.remote_id));
                }
            });
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(12.0, 5.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.pending && !self.previous.is_empty(),
                            egui::Button::new("Trước").small(),
                        )
                        .clicked()
                    {
                        action = Some(Action::Previous);
                    }
                    ui.label(
                        RichText::new(format!(
                            "Trang {} · {} {}",
                            self.previous.len() + 1,
                            self.entries.len(),
                            if self.is_group { "nhóm" } else { "liên hệ" }
                        ))
                        .size(10.5)
                        .color(visuals::MUTED),
                    );
                    if ui
                        .add_enabled(
                            !self.pending
                                && self.next.is_some()
                                && self.previous.len() < MAX_PREVIOUS,
                            egui::Button::new("Tiếp").small(),
                        )
                        .clicked()
                    {
                        action = Some(Action::Next);
                    }
                });
            });
        action
    }
}

enum Action {
    Refresh,
    Retry,
    Next,
    Previous,
    Open(DirectoryRecord),
}

impl NativeApp {
    pub(super) fn directory_visible(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|state| state.account.is_some() && state.directory.visible)
    }
    pub(super) fn activate_directory(&mut self, ctx: &egui::Context) {
        if let Some(state) = &mut self.live {
            if state.account.is_some() {
                state.ensure_worker(self._profile.clone());
                state.directory.activate(ctx);
            }
        }
    }
    pub(super) fn directory_panel(&mut self, ui: &mut egui::Ui) {
        let Some(state) = &mut self.live else {
            return;
        };
        let action = state.directory.show(ui, self.model.selected.as_deref());
        match action {
            Some(Action::Refresh) => {
                state.ensure_worker(self._profile.clone());
                let generation = state.directory.sync_generation.wrapping_add(1);
                if let Some(worker) = &state.worker {
                    match worker.submit(LiveCommand::RefreshDirectory { generation }) {
                        Ok(()) => {
                            state.directory.sync_generation = generation;
                            state.directory.syncing = true;
                            state.directory.summary = "Đang cập nhật danh bạ từ Zalo…".into();
                            state.directory.detail.clear();
                        }
                        Err(error) => state.directory.error = Some(error.to_string()),
                    }
                }
            }
            Some(Action::Retry) => {
                state.ensure_worker(self._profile.clone());
                state
                    .directory
                    .request(state.worker.as_ref(), state.directory.after.clone());
            }
            Some(Action::Next) => {
                let current = state.directory.after.clone();
                if state
                    .directory
                    .request(state.worker.as_ref(), state.directory.next.clone())
                {
                    state.directory.previous.push(current);
                }
            }
            Some(Action::Previous) => {
                if let Some(previous) = state.directory.previous.last().cloned() {
                    if state.directory.request(state.worker.as_ref(), previous) {
                        state.directory.previous.pop();
                    }
                }
            }
            Some(Action::Open(entry)) => {
                self.open_directory_entry(entry);
            }
            None => {}
        }
    }
    pub(super) fn open_directory_entry(&mut self, entry: DirectoryRecord) {
        let thread = entry.thread_id();
        if !self.drafts.can_select(&thread) {
            self.model.status = "Đã đạt giới hạn bản nháp đang mở.".into();
            return;
        }
        let Some(state) = &mut self.live else {
            return;
        };
        if state.account.is_none() || !state.ensure_worker(self._profile.clone()) {
            return;
        }
        let generation = self.model.generation.wrapping_add(1);
        let Some(worker) = &state.worker else {
            return;
        };
        match worker.submit(LiveCommand::Open {
            generation,
            kind: if entry.is_group {
                ThreadKind::Group
            } else {
                ThreadKind::Direct
            },
            remote_id: entry.remote_id,
        }) {
            Ok(()) => {
                self.model.generation = generation;
                self.model.selected = Some(thread.clone());
                self.model.messages.clear();
                self.model.has_older = false;
                self.model.pending = true;
                let _ = self.drafts.select(&thread);
                state.delivery.edited(&thread, &self.drafts.active.text);
                state.review_dialog = state.delivery.review(&thread).is_some();
                state.directory.visible = false;
                self.compact_list = false;
                self.latest_page = true;
            }
            Err(error) => state.directory.error = Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, name: &str, is_group: bool) -> DirectoryRecord {
        DirectoryRecord {
            remote_id: id.into(),
            is_group,
            display_name: name.into(),
            avatar_url: None,
            is_friend: None,
            is_blocked: None,
            member_count: None,
        }
    }
    fn page(account: &str, generation: u64, is_group: bool, query: &str) -> LiveEvent {
        LiveEvent::DirectoryPage {
            account_id: account.into(),
            generation,
            is_group,
            query: query.into(),
            entries: vec![record("22", "Đặng Minh", is_group)],
            next: None,
        }
    }
    #[test]
    fn directory_result_requires_account_generation_tab_and_query() {
        let ctx = egui::Context::default();
        let mut view = DirectoryView {
            generation: 9,
            query: "dang".into(),
            pending: true,
            ..Default::default()
        };
        for event in [
            page("12", 9, false, "dang"),
            page("11", 8, false, "dang"),
            page("11", 9, true, "dang"),
            page("11", 9, false, "other"),
        ] {
            view.apply(&event, "11", &ctx);
            assert!(view.entries.is_empty());
            assert!(view.pending);
        }
        view.apply(&page("11", 9, false, "dang"), "11", &ctx);
        assert_eq!(view.entries[0].display_name, "Đặng Minh");
        assert!(!view.pending);
        assert!(view.loaded);
    }
    #[test]
    fn search_switch_invalidates_old_page_without_losing_query_and_group_identity() {
        let ctx = egui::Context::default();
        let mut view = DirectoryView {
            generation: 9,
            query: "dang".into(),
            pending: true,
            is_group: false,
            ..Default::default()
        };
        view.search = "Nhóm thiết kế".into();
        view.is_group = true;
        view.reset_query(&ctx);
        view.apply(&page("11", 9, false, "dang"), "11", &ctx);
        assert!(view.entries.is_empty());
        assert_eq!(view.query, "Nhóm thiết kế");
        assert!(view.is_group);
        assert!(view.previous.is_empty());
    }
    #[test]
    fn directory_rows_and_paging_controls_fit_existing_wide_and_compact_list() {
        for width in [280.0, 344.0, 576.0] {
            let ctx = egui::Context::default();
            visuals::install(&ctx);
            let mut view = DirectoryView {
                loaded: true,
                entries: vec![record("22", "Đặng Minh", false)],
                next: Some(DirectoryCursor {
                    name: "Đặng Minh".into(),
                    thread_id: "d:22".into(),
                }),
                ..Default::default()
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(width, 390.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            view.show(ui, None);
                        });
                },
            );
            for expected in [
                "Danh bạ",
                "Cập nhật",
                "Liên hệ",
                "Nhóm",
                "Đặng Minh",
                "Tiếp",
            ] {
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                            Some(text)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing {expected} at {width}"));
                assert!(
                    text.pos.y + text.galley.size().y <= 390.0,
                    "{expected} clipped vertically at {width}"
                );
                assert!(
                    text.pos.x + text.galley.size().x <= width,
                    "{expected} clipped horizontally at {width}"
                );
            }
        }
    }
    #[test]
    fn incremental_commit_coalesces_current_page_reload_without_clearing_rows_or_cursor() {
        let ctx = egui::Context::default();
        let cursor = DirectoryCursor {
            name: "Liên hệ 099".into(),
            thread_id: "d:199".into(),
        };
        let mut view = DirectoryView {
            visible: true,
            generation: 8,
            query: "liên hệ".into(),
            search: "liên hệ".into(),
            after: Some(cursor.clone()),
            previous: vec![None],
            loaded: true,
            pending: true,
            entries: vec![record("200", "Liên hệ 100", false)],
            ..Default::default()
        };
        for _ in 0..3 {
            view.apply(
                &LiveEvent::DirectoryChanged {
                    account_id: "11".into(),
                    conversations: vec![],
                },
                "11",
                &ctx,
            );
        }
        assert_eq!(view.after, Some(cursor));
        assert_eq!(view.previous, vec![None]);
        assert_eq!(view.query, "liên hệ");
        assert_eq!(view.entries[0].remote_id, "200");
        assert!(!view.pending);
        assert!(view.deadline.is_some());
        assert!(view.dirty);
        view.apply(&page("11", 8, false, "liên hệ"), "11", &ctx);
        assert_eq!(
            view.entries[0].remote_id, "200",
            "stale pre-commit page must not replace current cache"
        );
    }
    #[test]
    fn oversized_page_is_rejected_and_changed_events_are_account_scoped() {
        let ctx = egui::Context::default();
        let mut view = DirectoryView {
            generation: 1,
            visible: true,
            loaded: true,
            ..Default::default()
        };
        let event = LiveEvent::DirectoryPage {
            account_id: "11".into(),
            generation: 1,
            is_group: false,
            query: String::new(),
            entries: (1..=101)
                .map(|i| record(&i.to_string(), "Liên hệ", false))
                .collect(),
            next: None,
        };
        view.apply(&event, "11", &ctx);
        assert!(view.entries.is_empty());
        assert!(view.error.is_some());
        view.apply(
            &LiveEvent::DirectoryChanged {
                account_id: "12".into(),
                conversations: vec![],
            },
            "11",
            &ctx,
        );
        assert!(view.loaded);
        view.apply(
            &LiveEvent::DirectoryChanged {
                account_id: "11".into(),
                conversations: vec![],
            },
            "11",
            &ctx,
        );
        assert!(
            view.loaded,
            "committed metadata must retain visible cached rows"
        );
        assert!(view.dirty);
        assert!(view.deadline.is_some());
    }
}
