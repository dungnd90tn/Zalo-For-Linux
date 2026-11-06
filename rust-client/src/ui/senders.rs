//! Presentation-only sender identities. Message IDs remain unchanged.
use crate::domain::{Conversation, Message};

pub(super) struct Label<'a> {
    pub name: &'a str,
    pub avatar: &'a str,
}

pub(super) fn label<'a>(
    message: &'a Message,
    account: Option<&str>,
    account_name: Option<&'a str>,
    conversation: Option<&'a Conversation>,
    resolved: Option<&'a str>,
) -> Label<'a> {
    let known = |name| Label { name, avatar: name };
    let Some(account) = account else {
        return if message.sender == "Demo guide" {
            Label {
                name: "Hướng dẫn demo",
                avatar: "Hướng dẫn",
            }
        } else {
            known(&message.sender)
        };
    };
    if message.account_id == account {
        if message.sender == account {
            return known(
                account_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("Bạn"),
            );
        }
        if let Some(name) = resolved.filter(|name| !name.trim().is_empty()) {
            return known(name);
        }
        // A direct thread's peer is its sender. A group title is never a member name.
        if let Some(thread) = conversation.filter(|thread| {
            thread.id == message.thread_id
                && thread.id.strip_prefix("d:") == Some(message.sender.as_str())
        }) {
            let name = thread.title.as_str();
            if !name.trim().is_empty()
                && name != message.sender
                && name != thread.id
                && !name.chars().all(|c| c.is_ascii_digit())
                && name.strip_prefix("Zalo ID ") != Some(message.sender.as_str())
            {
                return known(name);
            }
        }
    }
    Label {
        name: "Người gửi",
        avatar: "?",
    }
}

#[cfg(feature = "messaging")]
mod cache {
    use crate::{
        app::{
            live::{LiveCommand, LiveEvent, LiveWorker, MAX_SENDER_NAMES},
            Model,
        },
        domain::validate_id,
    };
    use eframe::egui;
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };

    const COALESCE: Duration = Duration::from_millis(180);
    const BUSY_RETRY: Duration = Duration::from_millis(100);

    #[derive(Default)]
    pub(in crate::ui) struct Names {
        account: String,
        thread: String,
        page_generation: u64,
        query_generation: u64,
        ids: Vec<String>,
        names: HashMap<String, String>,
        due: Option<Instant>,
        in_flight: Option<(u64, String, String)>,
    }
    impl Names {
        pub fn sync_scope(&mut self, account: &str, model: &Model, ctx: &egui::Context) {
            if self.account != account
                || self.thread != model.selected.as_deref().unwrap_or_default()
                || self.page_generation != model.generation
            {
                self.prepare(account, model, false, ctx);
            }
        }
        pub fn worker_closed(&mut self) {
            self.in_flight = None;
            if !self.ids.is_empty() {
                self.due = Some(Instant::now());
            }
        }
        pub fn prepare(&mut self, account: &str, model: &Model, force: bool, ctx: &egui::Context) {
            let thread = model.selected.as_deref().unwrap_or_default();
            // The normal pane is PAGE_SIZE=50; the defensive cap also bounds
            // malformed fixture input, preferring the latest visible messages.
            let mut ids = Vec::new();
            for message in model.messages.iter().rev().filter(|message| {
                message.account_id == account
                    && message.thread_id == thread
                    && message.sender != account
            }) {
                if !ids.contains(&message.sender) && validate_id(&message.sender).is_ok() {
                    ids.push(message.sender.clone());
                    if ids.len() == MAX_SENDER_NAMES {
                        break;
                    }
                }
            }
            ids.sort_unstable();
            let changed = self.account != account
                || self.thread != thread
                || self.page_generation != model.generation;
            if !changed && self.ids == ids && !force {
                return;
            }
            if changed {
                self.names.clear();
                if self.account != account {
                    self.in_flight = None;
                }
            }
            self.account = account.into();
            self.thread = thread.into();
            self.page_generation = model.generation;
            self.ids = ids;
            self.query_generation = self.query_generation.wrapping_add(1);
            self.due = if self.ids.is_empty() {
                None
            } else {
                Some(Instant::now() + if force { COALESCE } else { Duration::ZERO })
            };
            if let Some(due) = self.due {
                ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
        }
        pub fn poll(&mut self, worker: Option<&LiveWorker>, ctx: &egui::Context) {
            let Some(worker) = worker else {
                return;
            };
            self.submit(ctx, |command| worker.submit(command).is_ok());
        }
        fn submit(&mut self, ctx: &egui::Context, submit: impl FnOnce(LiveCommand) -> bool) {
            if self.in_flight.is_some() {
                return;
            }
            let Some(due) = self.due else {
                return;
            };
            if Instant::now() < due {
                ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
                return;
            }
            if submit(LiveCommand::ResolveSenders {
                generation: self.query_generation,
                thread: self.thread.clone(),
                sender_ids: self.ids.clone(),
            }) {
                self.due = None;
                self.in_flight = Some((
                    self.query_generation,
                    self.account.clone(),
                    self.thread.clone(),
                ));
            } else {
                self.due = Some(Instant::now() + BUSY_RETRY);
                ctx.request_repaint_after(BUSY_RETRY);
            }
        }
        pub fn apply(&mut self, event: &LiveEvent, account: &str, model: &Model) -> bool {
            let LiveEvent::SenderNames {
                account_id,
                generation,
                thread,
                names,
            } = event
            else {
                return false;
            };
            if self
                .in_flight
                .as_ref()
                .is_some_and(|(pending, pending_account, pending_thread)| {
                    pending == generation
                        && pending_account == account_id
                        && pending_thread == thread
                })
            {
                self.in_flight = None;
            }
            if account_id != account
                || self.account != account
                || model.selected.as_deref() != Some(thread.as_str())
                || self.thread != *thread
                || self.page_generation != model.generation
                || self.query_generation != *generation
                || names.len() > MAX_SENDER_NAMES
            {
                return false;
            }
            self.names = names
                .iter()
                .filter(|entry| {
                    self.ids.binary_search(&entry.sender_id).is_ok()
                        && !entry.display_name.trim().is_empty()
                        && entry.display_name.len() <= 512
                        && !entry.display_name.chars().any(char::is_control)
                })
                .map(|entry| (entry.sender_id.clone(), entry.display_name.clone()))
                .collect();
            true
        }
        pub fn get<'a>(
            &'a self,
            account: &str,
            generation: u64,
            thread: &str,
            sender: &str,
        ) -> Option<&'a str> {
            if self.account != account
                || self.page_generation != generation
                || self.thread != thread
            {
                return None;
            }
            self.names.get(sender).map(String::as_str)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{app::live::SenderName, domain::Message};
        fn model() -> Model {
            let mut model = Model {
                selected: Some("g:90".into()),
                generation: 3,
                ..Default::default()
            };
            model.messages = (1..=50)
                .map(|i| Message {
                    account_id: "11".into(),
                    thread_id: "g:90".into(),
                    id: i.to_string(),
                    sender: (100 + i).to_string(),
                    text: "Synthetic".into(),
                    sticker: None,
                    media: None,
                    sent_at_ms: i,
                })
                .collect();
            model
        }
        fn event(generation: u64, name: &str) -> LiveEvent {
            LiveEvent::SenderNames {
                account_id: "11".into(),
                generation,
                thread: "g:90".into(),
                names: vec![SenderName {
                    sender_id: "101".into(),
                    display_name: name.into(),
                }],
            }
        }
        #[test]
        fn page_and_account_scope_reject_late_names_and_directory_rename_preserves_rows() {
            let ctx = egui::Context::default();
            let mut names = Names::default();
            let mut model = model();
            names.prepare("11", &model, false, &ctx);
            let first = names.query_generation;
            assert!(names.apply(&event(first, "Đặng Minh"), "11", &model));
            assert_eq!(names.get("11", 3, "g:90", "101"), Some("Đặng Minh"));
            let before = model.messages.clone();
            names.prepare("11", &model, true, &ctx);
            assert!(!names.apply(&event(first, "Tên cũ"), "11", &model));
            assert!(names.apply(&event(names.query_generation, "Nguyễn Hà"), "11", &model));
            assert_eq!(model.messages, before);
            model.generation += 1;
            assert!(!names.apply(&event(names.query_generation, "Cũ"), "11", &model));
            names.prepare("12", &model, false, &ctx);
            assert!(names.names.is_empty());
            assert!(names.get("11", 3, "g:90", "101").is_none());
        }
        #[test]
        fn rename_during_in_flight_lookup_discards_old_answer_and_submits_only_latest_snapshot() {
            let ctx = egui::Context::default();
            let mut names = Names::default();
            let model = model();
            names.prepare("11", &model, false, &ctx);
            let first = names.query_generation;
            let mut sent = 0;
            names.submit(&ctx, |_| {
                sent += 1;
                true
            });
            names.prepare("11", &model, true, &ctx);
            names.prepare("11", &model, true, &ctx);
            let newest = names.query_generation;
            names.due = Some(Instant::now());
            names.submit(&ctx, |_| {
                sent += 1;
                true
            });
            assert_eq!(sent, 1, "one lookup in flight");
            assert!(!names.apply(&event(first, "Tên trước đồng bộ"), "11", &model));
            assert!(names.in_flight.is_none());
            names.submit(&ctx,|command|{sent+=1;assert!(matches!(command,LiveCommand::ResolveSenders{generation,..}if generation==newest));true});
            assert!(names.apply(&event(newest, "Đặng Minh mới"), "11", &model));
            assert_eq!(sent, 2);
            assert_eq!(names.get("11", 3, "g:90", "101"), Some("Đặng Minh mới"));
        }
        #[test]
        fn complete_fifty_sender_page_is_bounded_and_missing_names_do_not_poll_again() {
            let ctx = egui::Context::default();
            let mut names = Names::default();
            let model = model();
            names.prepare("11", &model, false, &ctx);
            assert_eq!(names.ids.len(), crate::app::PAGE_SIZE);
            let mut count = 0;
            names.submit(&ctx,|command|{count+=1;assert!(matches!(command,LiveCommand::ResolveSenders{sender_ids,..}if sender_ids.len()==50));true});
            let empty = LiveEvent::SenderNames {
                account_id: "11".into(),
                generation: names.query_generation,
                thread: "g:90".into(),
                names: vec![],
            };
            assert!(names.apply(&empty, "11", &model));
            names.prepare("11", &model, false, &ctx);
            names.submit(&ctx, |_| {
                count += 1;
                true
            });
            assert_eq!(count, 1);
            names.prepare("11", &model, true, &ctx);
            assert!(names.due.is_some());
        }
    }
}
#[cfg(feature = "messaging")]
pub(super) use cache::Names;

#[cfg(test)]
mod tests {
    use super::*;
    fn message() -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: "7".into(),
            sender: "22".into(),
            text: "Synthetic".into(),
            sticker: None,
            media: None,
            sent_at_ms: 1,
        }
    }
    #[test]
    fn direct_peer_title_is_bound_to_account_thread_and_sender_but_group_title_is_never_a_member_name(
    ) {
        let mut message = message();
        let mut conversation = Conversation {
            id: "d:22".into(),
            title: "Đặng Minh".into(),
            last_message_at_ms: 1,
            last_text: "Synthetic".into(),
            last_sender: "22".into(),
        };
        assert_eq!(
            label(&message, Some("11"), None, Some(&conversation), None).name,
            "Đặng Minh"
        );
        message.sender = "33".into();
        assert_eq!(
            label(&message, Some("11"), None, Some(&conversation), None).avatar,
            "?"
        );
        message.sender = "22".into();
        message.thread_id = "g:90".into();
        conversation.id = "g:90".into();
        conversation.title = "Nhóm dự án".into();
        assert_eq!(
            label(&message, Some("11"), None, Some(&conversation), None).name,
            "Người gửi"
        );
        assert_eq!(
            label(
                &message,
                Some("11"),
                None,
                Some(&conversation),
                Some("Nguyễn Hà")
            )
            .name,
            "Nguyễn Hà"
        );
        message.account_id = "12".into();
        assert_eq!(
            label(
                &message,
                Some("11"),
                None,
                Some(&conversation),
                Some("Tên tài khoản khác")
            )
            .name,
            "Người gửi"
        );
    }
    #[test]
    fn long_vietnamese_sender_label_fits_compact_bubble_and_retains_original_identity() {
        let ctx = eframe::egui::Context::default();
        super::super::visuals::install(&ctx);
        let name = "Nguyễn Thị Ánh Hồng ".repeat(12);
        assert!(name.len() > 256 && name.len() <= 512);
        let message = message();
        let output = ctx.run(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(640.0, 480.0),
                )),
                ..Default::default()
            },
            |ctx| {
                eframe::egui::CentralPanel::default().show(ctx, |ui| {
                    super::super::message_bubble(
                        ui,
                        &message,
                        true,
                        Some("11"),
                        &name,
                        &name,
                        None,
                        #[cfg(feature = "messaging")]
                        None,
                        #[cfg(feature = "messaging")]
                        None,
                        0,
                    );
                });
            },
        );
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,eframe::egui::epaint::Shape::Text(text)if text.galley.job.text==name && text.galley.size().x<=490.0)));
        assert_eq!(message.sender, "22");
    }
    #[test]
    fn numeric_fallback_stays_neutral_and_own_display_name_does_not_change_sender_identity() {
        let mut message = message();
        let conversation = Conversation {
            id: "d:22".into(),
            title: "Zalo ID 22".into(),
            last_message_at_ms: 1,
            last_text: String::new(),
            last_sender: String::new(),
        };
        let unknown = label(&message, Some("11"), None, Some(&conversation), None);
        assert_eq!((unknown.name, unknown.avatar), ("Người gửi", "?"));
        let mut technical = conversation.clone();
        technical.title = technical.id.clone();
        assert_eq!(
            label(&message, Some("11"), None, Some(&technical), None).name,
            "Người gửi"
        );
        assert_eq!(
            label(&message, Some("11"), None, Some(&technical), Some("12345")).name,
            "12345",
            "explicit cached display names may legitimately be numeric"
        );
        message.sender = "11".into();
        assert_eq!(
            label(&message, Some("11"), Some("Tài khoản kiểm thử"), None, None).name,
            "Tài khoản kiểm thử"
        );
        assert_eq!(message.sender, "11");
    }
}
