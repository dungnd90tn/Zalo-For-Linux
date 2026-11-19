//! Native confirmation and result state for one group invite or leave request.
use super::{dialog, group_members::Scope, visuals};
use crate::app::live::{GroupActionOutcome, GroupInviteTarget, LiveEvent};
use eframe::egui::{self, vec2, RichText};

const MAX_TARGETS: usize = 25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GroupPermissions {
    pub invite: bool,
    pub leave: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Invite { target: String },
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    NotSent,
    InviteAccepted,
    ApprovalPending,
    InvitationSent,
    AlreadyInvited,
    AlreadyMember,
    Left,
    Rejected,
    Unknown,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::NotSent => "Chưa thực hiện được yêu cầu.",
            Self::InviteAccepted => "Zalo đã nhận yêu cầu mời.",
            Self::ApprovalPending => "Đã gửi yêu cầu. Đang chờ nhóm duyệt.",
            Self::InvitationSent => "Đã gửi lời mời.",
            Self::AlreadyInvited => "Liên hệ này đã được mời.",
            Self::AlreadyMember => "Liên hệ này đã là thành viên nhóm.",
            Self::Left => "Bạn đã rời nhóm. Lịch sử và bản nháp trên máy vẫn được giữ.",
            Self::Rejected => "Zalo từ chối yêu cầu.",
            Self::Unknown => "Chưa xác nhận được kết quả. Kiểm tra lại trạng thái nhóm.",
        }
    }

    fn failed(self) -> bool {
        matches!(self, Self::NotSent | Self::Rejected | Self::Unknown)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub contact: String,
    pub name: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Lookup {
    pub scope: Scope,
    pub serial: u64,
    pub query: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Request {
    pub scope: Scope,
    pub serial: u64,
    pub action: Action,
}

pub(super) enum Intent {
    Find(Lookup),
    Submit(Request),
}

#[derive(Default)]
struct Invite {
    query: String,
    lookup: Option<Lookup>,
    candidates: Vec<Target>,
    selected: Option<Target>,
    failed: bool,
}

enum Dialog {
    Invite(Box<Invite>),
    Leave,
}

struct View {
    scope: Scope,
    dialog: Dialog,
    result: Option<Outcome>,
}

#[derive(Default)]
pub(super) struct GroupActions {
    view: Option<View>,
    flight: Option<Request>,
    last: Option<(Request, Outcome)>,
    serial: u64,
}

impl GroupActions {
    pub(super) fn open_invite(&mut self, scope: Scope) -> Option<Lookup> {
        if self.flight.is_some() {
            return None;
        }
        self.view = Some(View {
            scope,
            dialog: Dialog::Invite(Box::default()),
            result: None,
        });
        self.lookup()
    }

    pub(super) fn open_leave(&mut self, scope: Scope) {
        if self.flight.is_some() {
            return;
        }
        let result = self.last.as_ref().and_then(|(prior, result)| {
            (prior.scope.account == scope.account
                && prior.scope.thread == scope.thread
                && prior.action == Action::Leave
                && matches!(result, Outcome::Unknown | Outcome::Left))
            .then_some(*result)
        });
        self.view = Some(View {
            scope,
            dialog: Dialog::Leave,
            result,
        });
    }

    pub(super) fn is_open(&self) -> bool {
        self.view.is_some()
    }

    pub(super) fn close(&mut self) {
        self.view = None;
    }

    pub(super) fn sync(&mut self, scope: Option<&Scope>) {
        if self
            .view
            .as_ref()
            .is_some_and(|view| scope != Some(&view.scope))
        {
            self.close();
        }
    }

    fn lookup(&mut self) -> Option<Lookup> {
        let view = self.view.as_mut()?;
        let Dialog::Invite(invite) = &mut view.dialog else {
            return None;
        };
        self.serial = self.serial.wrapping_add(1);
        let lookup = Lookup {
            scope: view.scope.clone(),
            serial: self.serial,
            query: invite.query.trim().to_owned(),
        };
        invite.lookup = Some(lookup.clone());
        invite.candidates.clear();
        invite.selected = None;
        invite.failed = false;
        Some(lookup)
    }

    pub(super) fn apply_targets(&mut self, lookup: &Lookup, found: Option<Vec<Target>>) {
        let Some(view) = &mut self.view else { return };
        let Dialog::Invite(invite) = &mut view.dialog else {
            return;
        };
        if invite.lookup.as_ref() != Some(lookup) || invite.query.trim() != lookup.query {
            return;
        }
        invite.lookup = None;
        if let Some(candidates) = found.filter(|targets| targets.len() <= MAX_TARGETS) {
            invite.candidates = candidates;
        } else {
            invite.failed = true;
        }
    }

    pub(super) fn settle(&mut self, request: &Request, outcome: Outcome) {
        if self.flight.as_ref() != Some(request) {
            return;
        }
        self.flight = None;
        self.last = Some((request.clone(), outcome));
        if let Some(view) = self
            .view
            .as_mut()
            .filter(|view| view.scope == request.scope)
        {
            view.result = Some(outcome);
        }
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::GroupInviteTargets {
                account_id,
                request_id,
                thread,
                privacy_revision,
                targets,
            } => {
                let Some(View {
                    dialog: Dialog::Invite(invite),
                    ..
                }) = &self.view
                else {
                    return;
                };
                let Some(lookup) = invite.lookup.clone().filter(|lookup| {
                    lookup.serial == *request_id
                        && lookup.scope.account == *account_id
                        && lookup.scope.thread == *thread
                        && lookup.scope.privacy_revision == *privacy_revision
                }) else {
                    return;
                };
                self.apply_targets(
                    &lookup,
                    targets.as_ref().map(|targets| {
                        targets
                            .iter()
                            .map(|GroupInviteTarget { contact, name }| Target {
                                contact: contact.clone(),
                                name: name.clone(),
                            })
                            .collect()
                    }),
                );
            }
            LiveEvent::GroupActionFinished {
                account_id,
                request_id,
                thread,
                privacy_revision,
                member,
                outcome,
            } => {
                let action = member
                    .as_ref()
                    .map_or(Action::Leave, |target| Action::Invite {
                        target: target.clone(),
                    });
                let Some(request) = self.flight.clone().filter(|request| {
                    request.serial == *request_id
                        && request.scope.account == *account_id
                        && request.scope.thread == *thread
                        && request.scope.privacy_revision == *privacy_revision
                        && request.action == action
                }) else {
                    return;
                };
                let outcome = match outcome {
                    GroupActionOutcome::NotSent => Outcome::NotSent,
                    GroupActionOutcome::InviteAccepted => Outcome::InviteAccepted,
                    GroupActionOutcome::ApprovalPending => Outcome::ApprovalPending,
                    GroupActionOutcome::InvitationSent => Outcome::InvitationSent,
                    GroupActionOutcome::AlreadyInvited => Outcome::AlreadyInvited,
                    GroupActionOutcome::AlreadyMember => Outcome::AlreadyMember,
                    GroupActionOutcome::Left => Outcome::Left,
                    GroupActionOutcome::Rejected => Outcome::Rejected,
                    GroupActionOutcome::Unknown => Outcome::Unknown,
                    _ => return,
                };
                self.settle(&request, outcome);
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope, scene: &str) {
        match scene {
            "leave" => self.open_leave(scope),
            "unknown" => {
                self.open_leave(scope.clone());
                let request = Request {
                    scope,
                    serial: 1,
                    action: Action::Leave,
                };
                self.flight = Some(request.clone());
                self.settle(&request, Outcome::Unknown);
            }
            _ => {
                if let Some(lookup) = self.open_invite(scope) {
                    let target = Target {
                        contact: "42".into(),
                        name: "Nguyễn An".into(),
                    };
                    self.apply_targets(
                        &lookup,
                        Some(vec![
                            target.clone(),
                            Target {
                                contact: "43".into(),
                                name: "Đặng Thu Hà".into(),
                            },
                        ]),
                    );
                    if let Some(View {
                        dialog: Dialog::Invite(invite),
                        ..
                    }) = &mut self.view
                    {
                        invite.selected = Some(target);
                    }
                }
            }
        }
    }

    pub(super) fn worker_closed(&mut self) {
        if let Some(request) = self.flight.clone() {
            self.settle(&request, Outcome::Unknown);
        }
        if let Some(View {
            dialog: Dialog::Invite(invite),
            ..
        }) = &mut self.view
        {
            invite.failed |= invite.lookup.take().is_some();
        }
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        if self.flight.is_some() {
            Some("Đang chờ Zalo xử lý yêu cầu nhóm…")
        } else {
            self.last.as_ref().map(|(_, result)| result.label())
        }
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        online: bool,
        permission: GroupPermissions,
        contact_visible: impl Fn(&str) -> bool,
    ) -> Option<Intent> {
        let last = self.last.as_ref();
        let view = self.view.as_mut()?;
        let pending = self.flight.is_some();
        let mut action = None;
        let mut find = false;
        let mut close = false;
        let (heading, submit, allowed) = match &view.dialog {
            Dialog::Invite(_) => ("Mời thành viên", "Gửi lời mời", permission.invite),
            Dialog::Leave => ("Rời nhóm", "Rời nhóm", permission.leave),
        };
        let mut offered = None;
        let response = dialog::show(ctx, "group_action", heading, 430.0, true, |ui| {
            dialog::body(ui, |ui| {
                let colors = crate::theme::palette(ctx);
                ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                if pending {
                    ui.label("Đang chờ Zalo xử lý…");
                    return;
                }
                if let Some(result) = view.result {
                    let ink = if result.failed() {
                        colors.error
                    } else {
                        colors.text_primary
                    };
                    ui.add(egui::Label::new(RichText::new(result.label()).color(ink)).wrap());
                    return;
                }
                let available = online && allowed;
                match &mut view.dialog {
                    Dialog::Leave => {
                        ui.add(
                            egui::Label::new(
                                "Sau khi rời nhóm, bạn không thể gửi tin nhắn mới trong nhóm.",
                            )
                            .wrap(),
                        );
                        offered = Some(Action::Leave);
                    }
                    Dialog::Invite(invite) => {
                        let edited = ui
                            .add_enabled(
                                available,
                                egui::TextEdit::singleline(&mut invite.query)
                                    .id_source("invite_contact")
                                    .hint_text("Tìm trong danh bạ")
                                    .desired_width(ui.available_width())
                                    .char_limit(128),
                            )
                            .changed();
                        if edited {
                            invite.lookup = None;
                            invite.candidates.clear();
                            invite.selected = None;
                            invite.failed = false;
                        }
                        find = ui
                            .add_enabled(
                                available,
                                egui::Button::new("Tìm").min_size(vec2(76.0, 44.0)),
                            )
                            .clicked();
                        if invite.lookup.is_some() {
                            ui.label("Đang tìm bạn bè…");
                        } else if invite.failed {
                            ui.colored_label(colors.error, "Chưa tìm được người để mời.");
                        } else if invite.candidates.is_empty() {
                            ui.label("Không có kết quả. Hãy thử tên khác.");
                        }
                        for target in invite.candidates.iter().filter(|target| {
                            crate::domain::reply::valid_source_id(&target.contact)
                                && contact_visible(&target.contact)
                        }) {
                            let name = target.name.trim();
                            let name = if name.is_empty()
                                || name == target.contact
                                || name.chars().all(|ch| ch.is_ascii_digit())
                            {
                                "Liên hệ"
                            } else {
                                name
                            };
                            let button = egui::Button::new(name)
                                .selected(invite.selected.as_ref() == Some(target))
                                .truncate();
                            if ui
                                .add_enabled(
                                    available,
                                    button.min_size(vec2(ui.available_width(), 44.0)),
                                )
                                .clicked()
                            {
                                invite.selected = Some(target.clone());
                            }
                        }
                        if let Some(target) = invite
                            .selected
                            .as_ref()
                            .filter(|target| contact_visible(&target.contact))
                        {
                            let invite_action = Action::Invite {
                                target: target.contact.clone(),
                            };
                            if last.is_some_and(|(old, result)| {
                                old.scope.account == view.scope.account
                                    && old.scope.thread == view.scope.thread
                                    && old.action == invite_action
                                    && matches!(
                                        result,
                                        Outcome::Unknown
                                            | Outcome::InviteAccepted
                                            | Outcome::ApprovalPending
                                            | Outcome::InvitationSent
                                            | Outcome::AlreadyInvited
                                    )
                            }) {
                                ui.label("Yêu cầu mời liên hệ này đã được ghi nhận hoặc đang chờ xác nhận.");
                            } else {
                                offered = Some(invite_action);
                            }
                        }
                    }
                }
                if !available {
                    ui.label(if online {
                        "Quyền thực hiện yêu cầu này chưa được xác nhận."
                    } else {
                        "Kết nối lại để thực hiện yêu cầu."
                    });
                }
            });
            dialog::footer(ui, |ui| {
                if !pending
                    && view.result.is_none()
                    && visuals::primary_button(
                        ui,
                        online && allowed && offered.is_some() && !find,
                        submit,
                        vec2(120.0, 44.0),
                    )
                    .clicked()
                {
                    action = offered.take();
                }
                close = ui
                    .add_sized(
                        vec2(72.0, 44.0),
                        egui::Button::new(if pending || view.result.is_some() {
                            "Đóng"
                        } else {
                            "Hủy"
                        }),
                    )
                    .clicked();
            });
        });
        if response.closed || close {
            self.close();
            return None;
        }
        if find {
            return self.lookup().map(Intent::Find);
        }
        action.map(|action| {
            self.serial = self.serial.wrapping_add(1);
            let request = Request {
                scope: view.scope.clone(),
                serial: self.serial,
                action,
            };
            self.flight = Some(request.clone());
            Intent::Submit(request)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(group: &str, privacy_revision: u64) -> Scope {
        Scope {
            account: "11".into(),
            auth: Some(2),
            thread: group.into(),
            generation: 3,
            privacy_revision,
        }
    }

    #[test]
    fn stale_candidate_results_do_not_reopen_closed_dialog() {
        let mut actions = GroupActions::default();
        let old = actions.open_invite(scope("g:71", 3)).unwrap();
        actions.sync(Some(&scope("g:71", 4)));
        assert!(!actions.is_open());
        let current = actions.open_invite(scope("g:72", 4)).unwrap();
        actions.apply_targets(
            &old,
            Some(vec![Target {
                contact: "d:42".into(),
                name: "Nguyễn An".into(),
            }]),
        );
        let Dialog::Invite(invite) = &actions.view.as_ref().unwrap().dialog else {
            panic!()
        };
        assert!(invite.candidates.is_empty());
        assert!(invite.lookup.as_ref() == Some(&current));
    }

    #[test]
    fn unknown_leave_cannot_be_resubmitted_from_reopened_confirmation() {
        let mut actions = GroupActions::default();
        let current = scope("g:71", 3);
        actions.open_leave(current.clone());
        let request = Request {
            scope: current.clone(),
            serial: 1,
            action: Action::Leave,
        };
        actions.flight = Some(request);
        actions.worker_closed();
        actions.close();
        actions.open_leave(current);
        assert_eq!(
            actions.view.as_ref().unwrap().result,
            Some(Outcome::Unknown)
        );
    }
}
