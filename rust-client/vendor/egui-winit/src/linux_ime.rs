//! Linux input-context availability is distinct from active composition (#5008).
use egui::{Event, Id, ImeEvent};
use winit::event::Ime;

#[derive(Default)]
pub(crate) struct State {
    active: bool,
    owner: Option<Id>,
    input_widget: Option<Id>,
}

impl State {
    pub(crate) fn translate(&mut self, events: &mut Vec<Event>, ime: &Ime, focused: Option<Id>) {
        match ime {
            Ime::Enabled => {} // Input context availability must not suppress navigation.
            Ime::Preedit(text, Some(_)) if !text.is_empty() => {
                self.begin(events, focused);
                events.push(Event::Ime(ImeEvent::Preedit(text.clone())));
            }
            Ime::Commit(text) if !text.is_empty() => {
                // egui resets its cursor on widget focus changes. Initialize the
                // CURRENT selection even for commit-only XIM/Wayland input.
                self.begin(events, focused);
                events.push(Event::Ime(ImeEvent::Commit(text.clone())));
                self.end(events);
            }
            Ime::Preedit(_, _) | Ime::Disabled | Ime::Commit(_) => {
                if self.active {
                    // A delayed cancel belongs to its composing widget. Clearing
                    // another widget's selection would delete unrelated text.
                    if self.owner == focused {
                        events.push(Event::Ime(ImeEvent::Preedit(String::new())));
                    }
                    self.end(events);
                }
            }
        }
    }

    /// Rebind the platform input context only when editable widget identity changes.
    pub(crate) fn update_focus(
        &mut self,
        events: &mut Vec<Event>,
        allowed: bool,
        focused: Option<Id>,
    ) -> bool {
        let next = if allowed { focused } else { None };
        let reset = allowed && self.input_widget.is_some() && self.input_widget != next;
        self.input_widget = next;
        if reset && self.active {
            // The old provisional text stays in its own widget. Never clear a
            // selection in the new widget while resetting the platform engine.
            self.end(events);
        }
        reset
    }

    fn begin(&mut self, events: &mut Vec<Event>, focused: Option<Id>) {
        // Repeated payloads can arrive after focus changes without context
        // Disabled/Enabled. Refresh egui's composition cursor for each payload.
        events.push(Event::Ime(ImeEvent::Enabled));
        self.active = true;
        self.owner = focused;
    }

    fn end(&mut self, events: &mut Vec<Event>) {
        events.push(Event::Ime(ImeEvent::Disabled));
        self.active = false;
        self.owner = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translated(state: &mut State, focused: Option<Id>, ime: Ime) -> Vec<Event> {
        let mut output = vec![];
        state.translate(&mut output, &ime, focused);
        output
    }

    #[test]
    fn editor_identity_rebinds_once_without_resetting_each_frame_or_deleting_text() {
        let mut state = State::default();
        let mut events = vec![];
        let a = Some(Id::new("a"));
        let b = Some(Id::new("b"));
        assert!(!state.update_focus(&mut events, true, a));
        assert!(!state.update_focus(&mut events, true, a));
        state.translate(&mut events, &Ime::Preedit("chào".into(), Some((4, 4))), a);
        events.clear();
        assert!(state.update_focus(&mut events, true, b));
        assert_eq!(events, vec![Event::Ime(ImeEvent::Disabled)]);
        assert!(!state.update_focus(&mut events, true, b));
        assert!(!state.update_focus(&mut events, false, None));
        assert!(!state.update_focus(&mut events, true, a));
    }

    #[test]
    fn context_enabled_alone_never_enters_composition_mode() {
        let mut state = State::default();
        for event in [
            Ime::Enabled,
            Ime::Preedit(String::new(), None),
            Ime::Disabled,
        ] {
            assert!(translated(&mut state, Some(Id::new("a")), event).is_empty());
        }
        assert!(!state.active);
    }

    fn frame(ctx: &egui::Context, a: &mut String, b: &mut String, events: Vec<Event>) {
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(a).id(Id::new("a")));
                    ui.add(egui::TextEdit::singleline(b).id(Id::new("b")));
                });
            },
        );
    }

    fn payload(ctx: &egui::Context, state: &mut State, ime: Ime) -> Vec<Event> {
        translated(state, ctx.memory(|m| m.focused()), ime)
    }

    #[test]
    fn preedit_replacement_end_then_commit_inserts_once_and_navigation_recovers() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = ("prefix ".to_owned(), "other".to_owned());
        let mut state = State::default();
        ctx.memory_mut(|m| m.request_focus(Id::new("a")));
        frame(&ctx, &mut a, &mut b, vec![]);
        for ime in [
            Ime::Preedit("viee".into(), Some((4, 4))),
            Ime::Preedit("việt".into(), Some((4, 4))),
            Ime::Preedit(String::new(), None),
            Ime::Commit("việt".into()),
            Ime::Preedit(String::new(), None),
        ] {
            frame(&ctx, &mut a, &mut b, payload(&ctx, &mut state, ime));
        }
        assert_eq!(a, "prefix việt");
        assert!(!state.active);
        frame(
            &ctx,
            &mut a,
            &mut b,
            vec![Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(a, "prefix việ");
    }

    #[test]
    fn commit_only_after_focus_change_uses_current_widgets_cursor() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = ("prefix ".to_owned(), "other ".to_owned());
        let mut state = State::default();
        ctx.memory_mut(|m| m.request_focus(Id::new("a")));
        frame(&ctx, &mut a, &mut b, vec![]);
        frame(
            &ctx,
            &mut a,
            &mut b,
            payload(&ctx, &mut state, Ime::Preedit("việt".into(), Some((4, 4)))),
        );
        ctx.memory_mut(|m| m.request_focus(Id::new("b")));
        frame(&ctx, &mut a, &mut b, vec![]);
        frame(
            &ctx,
            &mut a,
            &mut b,
            payload(&ctx, &mut state, Ime::Commit("đ".into())),
        );
        assert_eq!(b, "other đ");
        assert!(!state.active);
    }

    #[test]
    fn delayed_cancel_does_not_delete_another_widgets_selected_text() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = ("prefix ".to_owned(), "other".to_owned());
        let mut state = State::default();
        ctx.memory_mut(|m| m.request_focus(Id::new("a")));
        frame(&ctx, &mut a, &mut b, vec![]);
        frame(
            &ctx,
            &mut a,
            &mut b,
            payload(&ctx, &mut state, Ime::Preedit("việt".into(), Some((4, 4)))),
        );
        ctx.memory_mut(|m| m.request_focus(Id::new("b")));
        frame(&ctx, &mut a, &mut b, vec![]);
        let mut edit = egui::TextEdit::load_state(&ctx, Id::new("b")).unwrap();
        edit.cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(5),
            )));
        edit.store(&ctx, Id::new("b"));
        frame(
            &ctx,
            &mut a,
            &mut b,
            payload(&ctx, &mut state, Ime::Preedit(String::new(), None)),
        );
        assert_eq!(b, "other");
        assert!(!state.active);
    }
}
