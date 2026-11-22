//! Explicit member selection from the verified ordinary-group roster.
use super::{group_members::Scope, visuals, NativeApp};
use crate::{
    app::live::{GroupMemberPage, LiveCommand, LiveConnection, LiveEvent, LivePrivacyStatus},
    domain::{fold_search, mention},
};
use eframe::egui::{self, vec2, Align2, Rect, RichText, Stroke};
use std::ops::Range;

const MAX_ROSTER: usize = 100;
const RESULT_LIMIT: usize = 5;
const REQUEST_BIT: u64 = 1 << 63;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    bytes: Range<usize>,
    query: String,
}

impl Token {
    fn at(text: &str, caret: egui::text::CCursorRange) -> Option<Self> {
        if caret.primary.index != caret.secondary.index {
            return None;
        }
        let end = text
            .char_indices()
            .nth(caret.primary.index)
            .map_or(text.len(), |(i, _)| i);
        if text[end..]
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace())
        {
            return None;
        }
        let start = text[..end].rfind(char::is_whitespace).map_or(0, |i| {
            i + text[i..].chars().next().map_or(0, char::len_utf8)
        });
        let query = text[start..end].strip_prefix('@')?;
        (!query.contains('@') && query.len() <= 96 && !query.chars().any(char::is_control)).then(
            || Self {
                bytes: start..end,
                query: query.into(),
            },
        )
    }
}

fn completed_caret(text: &str, byte: usize) -> egui::text::CCursor {
    // Move past an existing separator as well as one inserted by the draft.
    let separator = text[byte..]
        .chars()
        .next()
        .filter(|character| character.is_whitespace())
        .map_or(0, char::len_utf8);
    egui::text::CCursor::new(text[..byte + separator].chars().count())
}

#[derive(Clone)]
struct Member {
    id: String,
    name: String,
    folded: String,
}

impl Member {
    fn everyone() -> Self {
        let name = mention::ALL_LABEL.trim_start_matches('@');
        Self {
            id: mention::ALL_ID.into(),
            name: name.into(),
            folded: fold_search(name),
        }
    }

    fn title(&self) -> &str {
        if self.id == mention::ALL_ID {
            "Nhắc tất cả · @All"
        } else {
            &self.name
        }
    }
}

#[derive(Default)]
pub(super) struct Picker {
    scope: Option<Scope>,
    serial: u64,
    pending: Option<u64>,
    loaded: bool,
    pages: usize,
    failed: bool,
    next: Option<String>,
    members: Vec<Member>,
    everyone: Option<Member>,
    token: Option<Token>,
    dismissed: Option<Token>,
    highlighted: Option<usize>,
    rect: Option<Rect>,
    #[cfg(test)]
    fixture: bool,
}

impl Picker {
    pub(super) fn set_scope(&mut self, scope: Option<Scope>) {
        if self.scope != scope {
            *self = Self {
                scope,
                serial: self.serial,
                #[cfg(test)]
                fixture: self.fixture,
                ..Default::default()
            };
        }
    }

    pub(super) fn observe(&mut self, event: &LiveEvent) {
        let (
            Some(scope),
            LiveEvent::GroupMembers {
                account_id,
                request_id,
                thread,
                privacy_revision,
                page,
            },
        ) = (&self.scope, event)
        else {
            return;
        };
        if self.pending != Some(*request_id)
            || scope.account != *account_id
            || scope.thread != *thread
            || scope.privacy_revision != *privacy_revision
        {
            return;
        }
        self.pending = None;
        self.failed = page.is_none();
        self.loaded = true;
        self.pages += 1;
        let Some(page) = page else {
            self.next = None;
            self.everyone = None;
            self.highlighted = None;
            return;
        };
        let own = scope.account.clone();
        self.apply_page(page, &own);
    }

    fn apply_page(&mut self, page: &GroupMemberPage, own: &str) {
        if page.members.len() > super::group_members::PAGE_SIZE {
            self.next = None;
            self.failed = true;
            self.everyone = None;
            self.highlighted = None;
            return;
        }
        if page.mention_all != self.everyone.is_some() {
            self.everyone = page.mention_all.then(Member::everyone);
            self.highlighted = None;
        }
        for member in &page.members {
            let Some(name) = member.name.as_deref().map(str::trim).filter(|name| {
                !name.is_empty()
                    && name.chars().count() <= 100
                    && !name.chars().any(char::is_control)
            }) else {
                continue;
            };
            if member.id != own
                && crate::protocol::messaging::numeric_id(&member.id).is_ok()
                && !self.members.iter().any(|item| item.id == member.id)
            {
                self.members.push(Member {
                    id: member.id.clone(),
                    name: name.into(),
                    folded: fold_search(name),
                });
            }
        }
        self.next = if self.pages * super::group_members::PAGE_SIZE < MAX_ROSTER
            && page.next != self.next
        {
            page.next.clone()
        } else {
            None
        };
    }

    fn hide(&mut self) {
        self.token = None;
        self.highlighted = None;
        self.rect = None;
    }

    fn update_token(&mut self, token: Option<Token>) {
        if self.dismissed != token {
            self.dismissed = None;
        }
        let token = token.filter(|token| self.dismissed.as_ref() != Some(token));
        if self.token != token {
            self.highlighted = None;
        }
        self.token = token;
    }

    fn results(&self, allow_all: bool) -> impl Iterator<Item = &Member> {
        let query = self.token.as_ref().map(|token| fold_search(&token.query));
        self.everyone
            .iter()
            .filter(move |_| allow_all)
            .chain(self.members.iter())
            .filter(move |member| {
                query.as_ref().is_some_and(|q| {
                    member.folded.contains(q)
                        || (member.id == mention::ALL_ID && "nhac tat ca".contains(q))
                })
            })
            .take(RESULT_LIMIT)
    }

    #[cfg(test)]
    pub(super) fn fixture(&mut self, scope: Scope) {
        self.fixture = true;
        self.set_scope(Some(scope));
        self.loaded = true;
        for (id, name) in [
            ("22", "Nguyễn Minh"),
            ("33", "Nguyễn Thu Hà"),
            ("44", "Đặng Lan"),
        ] {
            self.members.push(Member {
                id: id.into(),
                name: name.into(),
                folded: fold_search(name),
            });
        }
    }

    #[cfg(test)]
    pub(super) fn fixture_everyone(&mut self, scope: Scope) {
        self.fixture(scope);
        self.everyone = Some(Member::everyone());
    }

    #[cfg(test)]
    pub(super) fn fixture_shown(&self) -> bool {
        self.rect.is_some()
            && if self.everyone.is_some() {
                self.results(true)
                    .any(|member| member.id == mention::ALL_ID)
            } else {
                self.results(true).count() == 2
            }
    }
}

pub(super) struct Choice {
    token: Token,
    member: Member,
}

pub(super) struct Editor {
    pub id: egui::Id,
    pub rect: Rect,
    pub caret: Option<egui::text::CCursorRange>,
    pub focused: bool,
}

impl NativeApp {
    pub(super) fn mention_scope(&self) -> Option<Scope> {
        let state = self.live.as_ref()?;
        let account = state.account.as_ref()?;
        let thread = self.model.selected.as_ref()?;
        let eligible = !self.demo
            && thread.starts_with("g:")
            && !self.selected_group_left()
            && !self.modal_open()
            && !self.modal_blocked
            && !self.model.pending
            && !self.directory_visible()
            && self.drafts.active.reply().is_none()
            && self.drafts.active.attachment().is_none()
            && !self.photo_selected()
            && state.privacy.status == LivePrivacyStatus::Ready
            && state.privacy.list_visible(thread);
        #[cfg(test)]
        let fixture = state.mention_picker.fixture;
        #[cfg(not(test))]
        let fixture = false;
        (eligible
            && (fixture || (state.worker.is_some() && state.connection == LiveConnection::Online)))
            .then(|| Scope {
                account: account.id.clone(),
                auth: self.auth_generation(),
                thread: thread.clone(),
                generation: self.model.generation,
                privacy_revision: state.privacy.revision,
            })
    }

    pub(super) fn mention_keys(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        enabled: bool,
        ime_active: bool,
    ) -> Option<Choice> {
        let scope = enabled.then(|| self.mention_scope()).flatten();
        let allow_all = !self.drafts.active.has_all_mention();
        let state = self.live.as_mut()?;
        let picker = &mut state.mention_picker;
        picker.set_scope(scope);
        let token = picker.token.as_ref()?;
        if ime_active
            || !ctx.memory(|memory| memory.has_focus(id) || memory.had_focus_last_frame(id))
        {
            return None;
        }
        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            picker.dismissed = Some(token.clone());
            picker.hide();
            ctx.memory_mut(|memory| memory.request_focus(id));
            return None;
        }
        if !ctx.memory(|memory| memory.has_focus(id)) {
            return None;
        }
        let len = picker.results(allow_all).count();
        if len == 0 {
            return None;
        }
        let down =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let up =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        if down || up {
            picker.highlighted = Some(match (picker.highlighted, down) {
                (Some(index), true) => (index + 1) % len,
                (Some(index), false) => (index + len - 1) % len,
                (None, true) => 0,
                (None, false) => len - 1,
            });
        }
        let member = picker.results(allow_all).nth(picker.highlighted?)?.clone();
        let select = ctx.input_mut(|input| {
            let mut select = false;
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    if modifiers.is_none() {
                        select |= !repeat;
                        return false;
                    }
                }
                true
            });
            select
        });
        select.then(|| Choice {
            token: token.clone(),
            member,
        })
    }

    pub(super) fn show_mentions(
        &mut self,
        ctx: &egui::Context,
        editor: Editor,
        choice: Option<Choice>,
        ime_active: bool,
    ) {
        let allow_all = !self.drafts.active.has_all_mention();
        let current = editor
            .caret
            .and_then(|caret| Token::at(&self.drafts.active.text, caret));
        let Some(state) = &mut self.live else { return };
        let picker = &mut state.mention_picker;
        let Some(scope) = picker.scope.as_ref() else {
            return;
        };
        let pointer_over = picker.rect.is_some_and(|rect| {
            ctx.input(|input| {
                input
                    .pointer
                    .hover_pos()
                    .is_some_and(|pos| rect.contains(pos))
            })
        });
        if !ctx.input(|input| input.focused) || (!editor.focused && !pointer_over) {
            picker.hide();
            return;
        }
        if ime_active {
            picker.hide();
            return;
        }
        let token = current.or_else(|| pointer_over.then(|| picker.token.clone()).flatten());
        let scope = scope.clone();
        picker.update_token(token);
        let Some(token) = &picker.token else { return };
        if picker.pending.is_none() && (!picker.loaded || picker.next.is_some()) {
            picker.serial = (picker.serial.wrapping_add(1) & !REQUEST_BIT).max(1);
            let request_id = REQUEST_BIT | picker.serial;
            let submitted = state.worker.as_ref().is_some_and(|worker| {
                worker
                    .submit(LiveCommand::GroupMembers {
                        request_id,
                        thread: scope.thread.clone(),
                        privacy_revision: scope.privacy_revision,
                        after: picker.next.clone(),
                    })
                    .is_ok()
            });
            if submitted {
                picker.pending = Some(request_id);
            } else {
                picker.loaded = true;
                picker.failed = true;
                picker.next = None;
            }
        }
        let mut chosen = choice.filter(|choice| {
            choice.token == *token
                && (choice.member.id != mention::ALL_ID || (allow_all && picker.everyone.is_some()))
        });
        let width = editor.rect.width().clamp(200.0, 304.0);
        let mut retry = false;
        let response = egui::Area::new(egui::Id::new(("mention_members", editor.id)))
            .kind(egui::UiKind::Popup)
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(editor.rect.left_top() - vec2(0.0, 8.0))
            .constrain_to(ctx.screen_rect().shrink(8.0))
            .movable(false)
            .show(ctx, |ui| {
                let colors = crate::theme::palette(ctx);
                egui::Frame::popup(ui.style())
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .rounding(12.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("Nhắc thành viên").strong());
                        let mut seen = false;
                        for (i, member) in picker.results(allow_all).enumerate() {
                            seen = true;
                            let row = super::message_actions::action_row_height(
                                ui,
                                member.title(),
                                "fa-tab-contact",
                                true,
                                false,
                                visuals::ICON_BUTTON,
                            );
                            if picker.highlighted == Some(i) {
                                ui.painter().rect_stroke(
                                    row.rect.shrink(1.0),
                                    7.0,
                                    Stroke::new(1.5_f32, colors.focus),
                                );
                            }
                            if row.clicked() {
                                chosen = Some(Choice {
                                    token: token.clone(),
                                    member: member.clone(),
                                });
                            }
                            row.on_hover_text(member.title());
                        }
                        if picker.failed {
                            retry = super::message_actions::action_row_height(
                                ui,
                                "Thử lại",
                                "fa-outline-recall",
                                true,
                                false,
                                visuals::ICON_BUTTON,
                            )
                            .clicked();
                        }
                        if !seen {
                            ui.label(
                                RichText::new(if picker.pending.is_some() {
                                    "Đang tải thành viên…"
                                } else if picker.failed {
                                    "Chưa lấy được danh sách thành viên."
                                } else {
                                    "Không tìm thấy thành viên phù hợp."
                                })
                                .size(visuals::TEXT_SMALL)
                                .color(colors.text_secondary),
                            );
                        }
                    });
            })
            .response;
        picker.rect = Some(response.rect);
        if retry {
            picker.loaded = false;
            picker.pages = 0;
            picker.failed = false;
            picker.next = None;
            picker.members.clear();
            picker.everyone = None;
            picker.highlighted = None;
            ctx.memory_mut(|memory| memory.request_focus(editor.id));
            ctx.request_repaint();
        }
        if let Some(Choice { token, member }) = chosen {
            picker.hide();
            match self
                .drafts
                .active
                .apply_mention(token.bytes, &member.name, &member.id)
            {
                Ok(caret_byte) => {
                    if let Some(mut edit) = egui::TextEdit::load_state(ctx, editor.id) {
                        edit.cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(completed_caret(
                                &self.drafts.active.text,
                                caret_byte,
                            ))));
                        edit.store(ctx, editor.id);
                    }
                    ctx.memory_mut(|memory| memory.request_focus(editor.id));
                    state
                        .delivery
                        .edited(&scope.thread, &self.drafts.active.text);
                }
                Err(error) => self.model.status = error.to_string(),
            }
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::live::LiveWorker, ui::live};

    fn frame(app: &mut NativeApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 480.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx, 160.0));
            },
        );
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect()
    }

    #[test]
    fn explicit_keyboard_choice_survives_ime_guard_and_sends_the_scoped_annotation() {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.model.selected = Some("g:44".into());
        app.drafts.select("g:44").unwrap();
        app.drafts.active.text = "👋 @ng".into();
        app.focus_composer = true;
        let auth = app.auth_generation();
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        state.mention_picker.fixture(Scope {
            account: "11".into(),
            auth,
            thread: "g:44".into(),
            generation: app.model.generation,
            privacy_revision: state.privacy.revision,
        });
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        assert!(app.live.as_ref().unwrap().mention_picker.token.is_some());
        frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        app.ime_composing = true;
        frame(&mut app, &ctx, key(egui::Key::Enter));
        assert_eq!(app.drafts.active.text, "👋 @ng");
        assert!(!app.drafts.active.has_mentions());
        assert!(commands.try_recv().is_err());
        app.ime_composing = false;
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        frame(&mut app, &ctx, key(egui::Key::Enter));
        assert_eq!(app.drafts.active.text, "👋 @Nguyễn Minh ");
        assert!(app.drafts.active.has_mentions());
        assert!(commands.try_recv().is_err(), "select must not send");
        frame(&mut app, &ctx, key(egui::Key::Enter));
        let command = commands.try_recv().unwrap();
        assert!(matches!(command, LiveCommand::SendMentions {
            thread, mentions, ..
        } if thread == "g:44" && mentions.len() == 1
            && mentions[0].uid == "22" && mentions[0].pos == 3));
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn everyone_is_a_scoped_explicit_choice() {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.model.selected = Some("g:44".into());
        app.drafts.select("g:44").unwrap();
        app.drafts.active.text = "👋 @all".into();
        app.focus_composer = true;
        let auth = app.auth_generation();
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        let scope = Scope {
            account: "11".into(),
            auth,
            thread: "g:44".into(),
            generation: app.model.generation,
            privacy_revision: state.privacy.revision,
        };
        state.mention_picker.fixture_everyone(scope.clone());
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        assert!(app.live.as_ref().unwrap().mention_picker.fixture_shown());
        assert!(
            !app.drafts.active.has_mentions(),
            "typing must stay plain text"
        );
        frame(&mut app, &ctx, key(egui::Key::ArrowDown));
        frame(&mut app, &ctx, key(egui::Key::Enter));
        assert_eq!(app.drafts.active.text, "👋 @All ");
        assert!(app.drafts.active.has_all_mention());
        assert!(commands.try_recv().is_err(), "selection must not send");
        frame(&mut app, &ctx, key(egui::Key::Enter));
        let command = commands.try_recv().unwrap();
        assert!(matches!(command, LiveCommand::SendMentions {
            thread, mentions, ..
        } if thread == "g:44" && mentions.len() == 1
            && mentions[0].is_all() && mentions[0].pos == 3 && mentions[0].len == 4));

        let picker = &mut app.live.as_mut().unwrap().mention_picker;
        let next_scope = Scope {
            generation: scope.generation + 1,
            ..scope
        };
        picker.set_scope(Some(next_scope.clone()));
        picker.pending = Some(REQUEST_BIT | 1);
        picker.observe(&LiveEvent::GroupMembers {
            account_id: "11".into(),
            request_id: REQUEST_BIT | 2,
            thread: next_scope.thread.clone(),
            privacy_revision: next_scope.privacy_revision,
            page: Some(GroupMemberPage {
                members: vec![],
                total: Some(3),
                restricted: false,
                next: None,
                permissions: Default::default(),
                mention_all: true,
            }),
        });
        assert!(
            picker.everyone.is_none(),
            "old roster response must not restore capability"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn enter_without_choosing_everyone_sends_plain_text() {
        let ctx = egui::Context::default();
        let (mut app, root) = live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        app.model.selected = Some("g:44".into());
        app.drafts.select("g:44").unwrap();
        app.drafts.active.text = "@All".into();
        app.focus_composer = true;
        let auth = app.auth_generation();
        let state = app.live.as_mut().unwrap();
        state.connection = LiveConnection::Online;
        state.worker = Some(worker);
        state.mention_picker.fixture_everyone(Scope {
            account: "11".into(),
            auth,
            thread: "g:44".into(),
            generation: app.model.generation,
            privacy_revision: state.privacy.revision,
        });
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        assert!(app.live.as_ref().unwrap().mention_picker.fixture_shown());
        frame(&mut app, &ctx, key(egui::Key::Enter));
        assert!(matches!(commands.try_recv().unwrap(), LiveCommand::Send {
            thread, text, ..
        } if thread == "g:44" && text == "@All"));
        assert!(!app.drafts.active.has_mentions());
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn token_bounds_ignore_email_selected_text_and_partial_word() {
        let caret = |index| egui::text::CCursorRange::one(egui::text::CCursor::new(index));
        assert!(Token::at("email@example", caret(13)).is_none());
        assert!(Token::at("@Nguyễn", caret(2)).is_none());
        let token = Token::at("👋 @Vy", caret(5)).unwrap();
        assert_eq!(&"👋 @Vy"[token.bytes], "@Vy");
        assert!(Token::at(
            "@Vy",
            egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(3)),
        )
        .is_none());
    }

    #[test]
    fn selecting_a_single_word_in_the_middle_keeps_existing_space_and_closes_suggestions() {
        let mut draft = crate::app::Composer::default();
        draft.text = "👋 @an nhé".into();
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(5));
        let token = Token::at(&draft.text, cursor).unwrap();
        let end = draft.apply_mention(token.bytes, "An", "33").unwrap();
        assert_eq!(draft.text, "👋 @An nhé");
        let cursor = completed_caret(&draft.text, end);
        assert_eq!(cursor.index, 6, "caret follows one existing separator");
        assert!(Token::at(&draft.text, egui::text::CCursorRange::one(cursor)).is_none());
    }
}
