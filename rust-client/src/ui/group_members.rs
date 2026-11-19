//! Scoped presentation for the read-only group roster. The worker owns membership and paging proof.
use super::{
    dialog,
    group_actions::{GroupActions, GroupPermissions, Intent as ActionIntent},
    visuals, NativeApp,
};
use crate::app::live::{
    GroupMemberPage, GroupMemberRole, LiveCommand, LiveConnection, LiveEvent, LivePrivacyStatus,
    LiveWorker,
};
use crate::domain::Conversation;
use eframe::egui::{self, pos2, vec2, RichText};
use std::collections::HashSet;

pub(super) const PAGE_SIZE: usize = 50;
const ROW_HEIGHT: f32 = 54.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    pub account: String,
    pub auth: Option<u64>,
    pub thread: String,
    pub generation: u64,
    pub privacy_revision: u64,
}

#[derive(Clone, Copy)]
pub(super) enum Role {
    Owner,
    Deputy,
    Member,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Self::Owner => "Trưởng nhóm",
            Self::Deputy => "Phó nhóm",
            Self::Member => "Thành viên",
        }
    }
}

pub(super) struct Member {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<Role>,
}

pub(super) struct Page {
    pub members: Vec<Member>,
    pub total: Option<u32>,
    pub restricted: bool,
    pub next: Option<String>,
    pub permissions: GroupPermissions,
}

impl From<GroupMemberPage> for Page {
    fn from(page: GroupMemberPage) -> Self {
        Self {
            members: page
                .members
                .into_iter()
                .map(|member| Member {
                    id: member.id,
                    name: member.name,
                    role: member.role.map(|role| match role {
                        GroupMemberRole::Owner => Role::Owner,
                        GroupMemberRole::Deputy => Role::Deputy,
                        GroupMemberRole::Member => Role::Member,
                    }),
                })
                .collect(),
            total: page.total,
            restricted: page.restricted,
            next: page.next,
            permissions: GroupPermissions {
                invite: page.permissions.invite,
                leave: page.permissions.leave,
            },
        }
    }
}

enum Intent {
    Load(Request),
    Invite(GroupPermissions),
    Leave(GroupPermissions),
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Request {
    pub scope: Scope,
    pub serial: u64,
    pub after: Option<String>,
}

#[derive(Default)]
pub(super) struct Roster {
    view: Option<View>,
    serial: u64,
}

struct View {
    scope: Scope,
    after: Option<String>,
    request: Option<Request>,
    page: Option<Page>,
    failed: bool,
}

impl Roster {
    pub(super) fn open(&mut self, scope: Scope) {
        self.view = Some(View {
            scope,
            after: None,
            request: None,
            page: None,
            failed: false,
        });
    }

    pub(super) fn is_open(&self) -> bool {
        self.view.is_some()
    }

    pub(super) fn sync(&mut self, scope: Option<&Scope>) -> Option<u64> {
        if self
            .view
            .as_ref()
            .is_some_and(|view| scope != Some(&view.scope))
        {
            return self.close();
        }
        None
    }

    pub(super) fn close(&mut self) -> Option<u64> {
        self.view
            .take()
            .and_then(|view| view.request)
            .map(|request| request.serial)
    }

    fn matches_result(
        &self,
        account: &str,
        request_id: u64,
        thread: &str,
        revision: u64,
    ) -> Option<Request> {
        self.view
            .as_ref()?
            .request
            .as_ref()
            .filter(|request| {
                request.scope.account == account
                    && request.serial == request_id
                    && request.scope.thread == thread
                    && request.scope.privacy_revision == revision
            })
            .cloned()
    }

    pub(super) fn request(&mut self, after: Option<String>) -> Option<Request> {
        let view = self.view.as_mut()?;
        if view.request.is_some() {
            return None;
        }
        self.serial = self.serial.wrapping_add(1);
        let request = Request {
            scope: view.scope.clone(),
            serial: self.serial,
            after,
        };
        view.request = Some(request.clone());
        view.failed = false;
        Some(request)
    }

    pub(super) fn apply(&mut self, request: &Request, result: Option<Page>) {
        let Some(view) = self
            .view
            .as_mut()
            .filter(|view| view.request.as_ref() == Some(request))
        else {
            return;
        };
        view.request = None;
        if let Some(page) = result.filter(|page| valid_page(page, request.after.as_deref())) {
            view.after.clone_from(&request.after);
            view.page = Some(page);
        } else {
            // Keep the last confirmed page visible when the next request fails.
            view.failed = true;
        }
    }

    /// Renders the same dialog from the docked or compact group Details pane.
    /// Returns a bounded page request for the caller to send through its worker.
    fn show(&mut self, ctx: &egui::Context, online: bool) -> (Option<Intent>, Option<u64>) {
        let Some(view) = self.view.as_ref() else {
            return (None, None);
        };
        let mut dismiss = false;
        let mut request = None;
        let mut action = None;
        let response = dialog::show(
            ctx,
            "group_members",
            "Thành viên nhóm",
            450.0,
            true,
            |ui| {
                ui.push_id((&view.scope.thread, &view.after), |ui| {
                    dialog::body(ui, |ui| {
                        let colors = crate::theme::palette(ctx);
                        if let Some(page) = &view.page {
                            if let Some(total) = page.total {
                                ui.label(
                                    RichText::new(format!("{total} thành viên"))
                                        .color(colors.text_secondary),
                                );
                            }
                            if page.restricted {
                                ui.label(
                                    RichText::new("Danh sách hiển thị theo quyền của bạn.")
                                        .size(visuals::TEXT_SMALL)
                                        .color(colors.text_secondary),
                                );
                            }
                            if page.permissions.invite || page.permissions.leave {
                                ui.horizontal_wrapped(|ui| {
                                    let ready = online && view.request.is_none();
                                    if page.permissions.invite
                                        && ui
                                            .add_enabled(
                                                ready,
                                                egui::Button::new("Mời thành viên")
                                                    .min_size(vec2(112.0, 44.0)),
                                            )
                                            .clicked()
                                    {
                                        action = Some(Intent::Invite(page.permissions));
                                    }
                                    if page.permissions.leave
                                        && ui
                                            .add_enabled(
                                                ready,
                                                egui::Button::new(
                                                    RichText::new("Rời nhóm").color(colors.error),
                                                )
                                                .min_size(vec2(80.0, 44.0)),
                                            )
                                            .clicked()
                                    {
                                        action = Some(Intent::Leave(page.permissions));
                                    }
                                });
                            }
                            if page.members.is_empty() {
                                ui.label("Trang này không có thành viên.");
                            }
                            for member in &page.members {
                                member_row(ui, member);
                            }
                        } else if view.request.is_some() {
                            ui.label("Đang tải danh sách thành viên…");
                        } else if !online {
                            ui.label("Kết nối lại để xem thành viên nhóm.");
                        }
                        if view.failed {
                            ui.colored_label(colors.error, "Chưa tải được danh sách thành viên.");
                        }
                    })
                });
                dialog::footer(ui, |ui| {
                    dismiss = ui
                        .add_sized(vec2(72.0, 44.0), egui::Button::new("Đóng"))
                        .clicked();
                    let ready = online && view.request.is_none();
                    if let Some(next) = view.page.as_ref().and_then(|page| page.next.as_ref()) {
                        if visuals::primary_button(ui, ready, "Trang sau", vec2(110.0, 44.0))
                            .clicked()
                        {
                            request = Some(Some(next.clone()));
                        }
                    }
                    let load = view.page.is_none()
                        && ready
                        && visuals::primary_button(
                            ui,
                            true,
                            if view.failed {
                                "Thử lại"
                            } else {
                                "Tải danh sách"
                            },
                            vec2(126.0, 44.0),
                        )
                        .clicked();
                    let first = view.after.is_some()
                        && ui
                            .add_enabled(
                                ready,
                                egui::Button::new("Trang đầu").min_size(vec2(100.0, 44.0)),
                            )
                            .clicked();
                    if load || first {
                        request = Some(None);
                    }
                });
            },
        );
        if response.closed || dismiss {
            return (None, self.close());
        }
        if action.is_some() {
            return (action, self.close());
        }
        (
            request
                .and_then(|after| self.request(after))
                .map(Intent::Load),
            None,
        )
    }
}

fn valid_page(page: &Page, after: Option<&str>) -> bool {
    let mut ids = HashSet::with_capacity(page.members.len().min(PAGE_SIZE));
    page.members.len() <= PAGE_SIZE
        && page
            .total
            .is_none_or(|total| total as usize >= page.members.len())
        && page.next.as_deref().is_none_or(|next| {
            !next.is_empty() && next.len() <= 512 && Some(next) != after && !page.members.is_empty()
        })
        && page.members.iter().all(|member| {
            !member.id.is_empty()
                && member.id.len() <= 64
                && member.name.as_ref().is_none_or(|name| name.len() <= 512)
                && ids.insert(member.id.as_str())
        })
}

fn member_row(ui: &mut egui::Ui, member: &Member) {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let known_name = member.name.as_deref().map(str::trim).filter(|name| {
        !name.is_empty() && *name != member.id && !name.chars().all(|ch| ch.is_ascii_digit())
    });
    let title = known_name.unwrap_or("Thành viên");
    let avatar =
        egui::Rect::from_center_size(pos2(rect.left() + 21.0, rect.center().y), vec2(38.0, 38.0));
    visuals::avatar(ui, avatar, known_name.unwrap_or("?"));
    let left = rect.left() + 53.0;
    let width = (rect.right() - left - 6.0).max(0.0);
    super::row_text(
        ui,
        pos2(
            left,
            rect.top() + if member.role.is_some() { 7.0 } else { 17.0 },
        ),
        width,
        title,
        egui::FontId::proportional(visuals::TEXT_BODY),
        colors.text_primary,
    );
    if let Some(role) = member.role {
        super::row_text(
            ui,
            pos2(left, rect.top() + 29.0),
            width,
            role.label(),
            egui::FontId::proportional(visuals::TEXT_SMALL),
            colors.text_secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(thread: &str, privacy_revision: u64) -> Scope {
        Scope {
            account: "11".into(),
            auth: Some(2),
            thread: thread.into(),
            generation: 3,
            privacy_revision,
        }
    }

    fn page() -> Page {
        Page {
            members: vec![Member {
                id: "22".into(),
                name: Some("Nguyễn An".into()),
                role: Some(Role::Owner),
            }],
            total: Some(2),
            restricted: false,
            next: Some("opaque".into()),
            permissions: GroupPermissions::default(),
        }
    }

    #[test]
    fn stale_group_or_privacy_change_never_reopens_roster() {
        let mut roster = Roster::default();
        roster.open(scope("g:70", 3));
        let old = roster.request(None).unwrap();
        roster.sync(Some(&scope("g:70", 4)));
        assert!(!roster.is_open());
        roster.open(scope("g:71", 4));
        let current = roster.request(None).unwrap();
        roster.apply(&old, Some(page()));
        assert!(roster.view.as_ref().unwrap().page.is_none());
        roster.apply(&current, Some(page()));
        assert_eq!(
            roster.view.as_ref().unwrap().page.as_ref().unwrap().total,
            Some(2)
        );
    }

    #[test]
    fn repeated_cursor_does_not_replace_confirmed_page() {
        let mut roster = Roster::default();
        roster.open(scope("g:70", 3));
        let first = roster.request(None).unwrap();
        roster.apply(&first, Some(page()));
        let next = roster.request(Some("opaque".into())).unwrap();
        roster.apply(&next, Some(page()));
        let view = roster.view.as_ref().unwrap();
        assert!(view.failed);
        assert!(view.after.is_none());
        assert_eq!(view.page.as_ref().unwrap().members.len(), 1);
    }
}

#[derive(Default)]
pub(super) struct Groups {
    roster: Roster,
    actions: GroupActions,
    action_permissions: GroupPermissions,
    cancel_pending: Option<u64>,
}

impl Groups {
    pub(super) fn is_open(&self) -> bool {
        self.roster.is_open() || self.actions.is_open()
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        self.actions.status_label()
    }

    pub(super) fn clear_projection(&mut self) {
        self.cancel_pending = self.roster.close().or(self.cancel_pending);
        self.actions.close();
        self.action_permissions = GroupPermissions::default();
    }

    pub(super) fn worker_closed(&mut self) {
        self.clear_projection();
        self.cancel_pending = None;
        self.actions.worker_closed();
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        self.actions.observe(event);
        match event {
            LiveEvent::GroupMembers {
                account_id,
                request_id,
                thread,
                privacy_revision,
                page,
            } => {
                if let Some(request) =
                    self.roster
                        .matches_result(account_id, *request_id, thread, *privacy_revision)
                {
                    self.roster.apply(&request, page.clone().map(Page::from));
                }
            }
            LiveEvent::GroupActionFinished {
                outcome: crate::app::live::GroupActionOutcome::Left,
                thread,
                ..
            } if self
                .roster
                .view
                .as_ref()
                .is_some_and(|view| view.scope.thread == *thread) =>
            {
                self.cancel_pending = self.roster.close().or(self.cancel_pending);
            }
            _ => {}
        }
    }

    fn load(&mut self, worker: Option<&LiveWorker>, request: Request) {
        let submitted = worker.is_some_and(|worker| {
            worker
                .submit(LiveCommand::GroupMembers {
                    request_id: request.serial,
                    thread: request.scope.thread.clone(),
                    privacy_revision: request.scope.privacy_revision,
                    after: request.after.clone(),
                })
                .is_ok()
        });
        if !submitted {
            self.roster.apply(&request, None);
        }
    }

    fn lookup(&mut self, worker: Option<&LiveWorker>, lookup: super::group_actions::Lookup) {
        let submitted = worker.is_some_and(|worker| {
            worker
                .submit(LiveCommand::GroupInviteTargets {
                    request_id: lookup.serial,
                    thread: lookup.scope.thread.clone(),
                    privacy_revision: lookup.scope.privacy_revision,
                    query: lookup.query.clone(),
                })
                .is_ok()
        });
        if !submitted {
            self.actions.apply_targets(&lookup, None);
        }
    }

    fn submit(&mut self, worker: Option<&LiveWorker>, request: &super::group_actions::Request) {
        use super::group_actions::{Action, Outcome};
        let command = match &request.action {
            Action::Invite { target } => LiveCommand::GroupInvite {
                request_id: request.serial,
                thread: request.scope.thread.clone(),
                privacy_revision: request.scope.privacy_revision,
                member: target.clone(),
            },
            Action::Leave => LiveCommand::GroupLeave {
                request_id: request.serial,
                thread: request.scope.thread.clone(),
                privacy_revision: request.scope.privacy_revision,
            },
        };
        if !worker.is_some_and(|worker| worker.submit(command).is_ok()) {
            self.actions.settle(request, Outcome::NotSent);
        }
    }

    fn flush_cancel(&mut self, worker: Option<&LiveWorker>, ctx: &egui::Context) {
        let Some(request_id) = self.cancel_pending.take() else {
            return;
        };
        if let Some(worker) = worker {
            if worker.submit(LiveCommand::CancelGroupMembers { request_id })
                == Err(crate::app::SubmitError::Busy)
            {
                self.cancel_pending = Some(request_id);
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope, scene: &str) {
        let permissions = GroupPermissions {
            invite: true,
            leave: true,
        };
        if scene == "roster" {
            self.roster.open(scope);
            let request = self.roster.request(None).unwrap();
            self.roster.apply(
                &request,
                Some(Page {
                    members: vec![
                        Member {
                            id: "22".into(),
                            name: Some("Nguyễn Minh".into()),
                            role: Some(Role::Owner),
                        },
                        Member {
                            id: "33".into(),
                            name: Some("Đặng Hà".into()),
                            role: Some(Role::Deputy),
                        },
                        Member {
                            id: "11".into(),
                            name: None,
                            role: Some(Role::Member),
                        },
                    ],
                    total: Some(3),
                    restricted: false,
                    permissions,
                    next: None,
                }),
            );
        } else if scene != "details" && scene != "left" {
            self.action_permissions = permissions;
            self.actions.fixture(scope, scene);
        }
    }
}

impl NativeApp {
    fn group_scope(&self) -> Option<Scope> {
        let state = self.live.as_ref()?;
        let account = state.account.as_ref()?;
        let group = self.bound_conversation()?;
        (group.id.starts_with("g:")
            && state.privacy.status == LivePrivacyStatus::Ready
            && state.privacy.list_visible(&group.id))
        .then(|| Scope {
            account: account.id.clone(),
            auth: self.auth_generation(),
            thread: group.id.clone(),
            generation: self.model.generation,
            privacy_revision: state.privacy.revision,
        })
    }

    pub(super) fn group_section(&mut self, ui: &mut egui::Ui, thread: &Conversation) {
        if self.demo || !thread.id.starts_with("g:") {
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        super::information::section_title(ui, "Nhóm");
        if thread.group_left {
            ui.label(
                RichText::new("Đã rời nhóm · có thể xem lịch sử trên máy")
                    .color(colors.text_secondary)
                    .size(visuals::TEXT_SMALL),
            );
            return;
        }
        let scope = self.group_scope();
        let Some(state) = &mut self.live else { return };
        let enabled =
            scope.is_some() && state.connection == LiveConnection::Online && state.worker.is_some();
        if ui
            .add_enabled(
                enabled,
                egui::Button::new("Thành viên").min_size(vec2(126.0, 44.0)),
            )
            .on_disabled_hover_text("Kết nối lại và xác minh quyền riêng tư để xem thành viên.")
            .clicked()
        {
            if let Some(scope) = scope {
                state.groups.roster.open(scope);
                if let Some(request) = state.groups.roster.request(None) {
                    state.groups.load(state.worker.as_ref(), request);
                }
            }
        }
    }

    pub(super) fn sync_groups(&mut self, ctx: &egui::Context) {
        let scope = self.group_scope();
        if let Some(state) = &mut self.live {
            let groups = &mut state.groups;
            groups.cancel_pending = groups.roster.sync(scope.as_ref()).or(groups.cancel_pending);
            groups.actions.sync(scope.as_ref());
            groups.flush_cancel(state.worker.as_ref(), ctx);
        }
    }

    pub(super) fn show_group_dialogs(&mut self, ctx: &egui::Context) {
        self.sync_groups(ctx);
        self.latch_modal_input(ctx);
        let scope = self.group_scope();
        let Some(group) = self.bound_conversation().cloned() else {
            return;
        };
        let Some(state) = &mut self.live else { return };
        let groups = &mut state.groups;
        let online = !group.group_left
            && state.connection == LiveConnection::Online
            && state.worker.is_some();
        let (intent, cancel) = groups.roster.show(ctx, online);
        groups.cancel_pending = cancel.or(groups.cancel_pending);
        groups.flush_cancel(state.worker.as_ref(), ctx);
        if let Some(intent) = intent {
            if let Some(scope) = scope {
                match intent {
                    Intent::Load(request) => groups.load(state.worker.as_ref(), request),
                    Intent::Invite(permissions) if online => {
                        groups.action_permissions = permissions;
                        if let Some(lookup) = groups.actions.open_invite(scope) {
                            groups.lookup(state.worker.as_ref(), lookup);
                        }
                    }
                    Intent::Leave(permissions) if online => {
                        groups.action_permissions = permissions;
                        groups.actions.open_leave(scope);
                    }
                    _ => {}
                }
            }
            ctx.request_repaint();
            return; // Consume the originating click in just one modal.
        }
        let permissions = if group.group_left {
            GroupPermissions::default()
        } else {
            groups.action_permissions
        };
        let action = groups.actions.show(
            ctx,
            &super::display_title(&group),
            online,
            permissions,
            |id| {
                state
                    .account
                    .as_ref()
                    .is_some_and(|account| account.id != id)
                    && state.privacy.list_visible(&format!("d:{id}"))
            },
        );
        match action {
            Some(ActionIntent::Find(lookup)) => groups.lookup(state.worker.as_ref(), lookup),
            Some(ActionIntent::Submit(request)) => groups.submit(state.worker.as_ref(), &request),
            None => {}
        }
    }
}
