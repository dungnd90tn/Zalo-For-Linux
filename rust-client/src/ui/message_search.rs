//! "Tìm tin nhắn" in the information panel: literal search in the selected conversation's saved
//! history. The query runs on the storage worker; this file only draws and submits.
use super::{clock_label, date_label, senders, visuals, NativeApp};
use crate::app::SubmitError;
use crate::domain::{Conversation, Message, MessageHit, SEARCH_RESULT_LIMIT};
use eframe::egui::{self, text::LayoutJob, vec2, FontId, RichText, Sense, TextFormat};
use std::time::Instant;

/// The query typed for one conversation; another conversation starts empty.
#[derive(Default)]
pub(super) struct SearchInput {
    thread: String,
    pub(super) text: String,
}

impl NativeApp {
    pub(super) fn submit_message_search(&mut self, thread: &str) {
        self.submit_message_search_before(thread, None);
    }

    pub(super) fn submit_message_search_before(
        &mut self,
        thread: &str,
        before: Option<crate::domain::Cursor>,
    ) {
        let query = self.message_search.text.clone();
        #[cfg(feature = "messaging")]
        if let Some(worker) = self.live.as_ref().and_then(|state| state.worker.as_ref()) {
            let result = self.model.request_search_before(
                thread,
                &query,
                before,
                |request, query, before| {
                    worker.submit(crate::app::live::LiveCommand::Search {
                        request,
                        thread: thread.to_owned(),
                        query,
                        before,
                    })
                },
            );
            self.search_submitted(result);
            return;
        }
        if let Some(worker) = &self.worker {
            let result = self.model.request_search_before(
                thread,
                &query,
                before,
                |request, query, before| {
                    worker.submit(crate::app::Command::Search {
                        request,
                        thread: thread.to_owned(),
                        query,
                        before,
                    })
                },
            );
            self.search_submitted(result);
        }
    }

    fn search_submitted(&mut self, result: Result<(), SubmitError>) {
        match result {
            Ok(()) | Err(SubmitError::Invalid) => {}
            Err(error) => self.model.status = error.to_string(),
        }
    }

    /// Show `hit` in the timeline: scroll to it when the visible page has it, else load the
    /// page that contains it. The timeline highlights it once drawn. In the compact layout the
    /// panel covers the timeline, so it closes.
    pub(super) fn jump_to_hit(&mut self, thread: &str, hit: &MessageHit, compact: bool) {
        if self.model.selected.as_deref() != Some(thread) {
            return;
        }
        if compact {
            self.conversation_details = None;
        }
        let cursor = self.model.focus_hit(thread, hit);
        if self.model.hit_loaded(hit) {
            return;
        }
        match cursor {
            Some(cursor) => {
                self.load_page_before(cursor);
            }
            None => {
                self.load_latest(thread.to_owned());
            }
        }
    }

    pub(super) fn message_search_section(&mut self, ui: &mut egui::Ui, thread: &Conversation) {
        let colors = crate::theme::palette(ui.ctx());
        if self.message_search.thread != thread.id {
            self.message_search = SearchInput {
                thread: thread.id.clone(),
                text: String::new(),
            };
        }
        super::information::section_title(ui, "Tìm tin nhắn");
        let mut submit = false;
        ui.horizontal(|ui| {
            let button_width = 56.0;
            let gap = ui.spacing().item_spacing.x;
            // TextEdit adds its inner margin outside the desired width.
            let margin = 2.0 * 8.0;
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.message_search.text)
                    .hint_text("Nhập từ cần tìm")
                    .margin(egui::Margin::symmetric(8.0, 7.0))
                    .desired_width((ui.available_width() - button_width - gap - margin).max(60.0)),
            );
            submit |= field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            submit |= ui
                .add_sized(
                    vec2(button_width, field.rect.height()),
                    egui::Button::new("Tìm"),
                )
                .on_hover_text("Tìm trong lịch sử đã lưu trên máy này")
                .clicked();
        });
        if submit {
            self.submit_message_search(&thread.id);
        }
        let small = |text: String| RichText::new(text).size(visuals::TEXT_SMALL);
        if self.model.search_pending(&thread.id) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(small("Đang tìm…".into()).color(colors.text_secondary));
            });
            return;
        }
        if self.model.search_failed(&thread.id) {
            ui.label(
                small("Chưa tìm được trong lịch sử đã lưu. Hãy thử lại.".into())
                    .color(colors.text_secondary),
            );
        }
        let Some(results) = self.model.search_results(&thread.id).cloned() else {
            ui.label(
                small("Chỉ tìm trong lịch sử đã lưu trên máy này, không hỏi máy chủ Zalo.".into())
                    .color(colors.text_secondary),
            );
            return;
        };
        ui.horizontal(|ui| {
            let same_query = self.message_search.text.trim() == results.query;
            if ui
                .add_enabled(
                    same_query && results.next.is_some(),
                    egui::Button::new("Tìm cũ hơn").min_size(vec2(0.0, 44.0)),
                )
                .on_disabled_hover_text(
                    "Nhấn Tìm để bắt đầu lại khi đổi từ khóa; chỉ tìm tiếp khi còn lịch sử cũ hơn.",
                )
                .clicked()
            {
                self.submit_message_search_before(&thread.id, results.next.clone());
            }
            if ui
                .add(egui::Button::new("Mới nhất").min_size(vec2(0.0, 44.0)))
                .on_hover_text("Tìm lại từ tin nhắn mới nhất")
                .clicked()
            {
                self.submit_message_search(&thread.id);
            }
        });
        if results.hits.is_empty() {
            ui.label(
                small(if results.next.is_some() {
                    format!("Chưa thấy “{}” trong phần lịch sử vừa tìm. Có thể tìm tiếp các tin cũ hơn.", results.query)
                } else {
                    format!("Không còn kết quả cho “{}” trong phần lịch sử đã tìm.", results.query)
                })
                .color(colors.text_secondary),
            );
            return;
        }
        ui.label(
            small(if results.truncated {
                format!(
                    "{} kết quả trên trang · còn lịch sử cũ hơn",
                    results.hits.len().min(SEARCH_RESULT_LIMIT)
                )
            } else {
                format!("{} kết quả trên trang", results.hits.len())
            })
            .color(colors.text_secondary),
        );
        let account = self.account_id().map(str::to_owned);
        let own_name = self.account_label().to_owned();
        let width = ui.available_width();
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt((
                "message-search",
                &thread.id,
                results.hits.first().map(|hit| &hit.message_id),
            ))
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for hit in &results.hits {
                    let probe = Message {
                        account_id: account
                            .clone()
                            .unwrap_or_else(|| crate::app::DEMO_ACCOUNT.into()),
                        thread_id: thread.id.clone(),
                        id: hit.message_id.clone(),
                        sender: hit.sender.clone(),
                        text: String::new(),
                        sticker: None,
                        media: None,
                        sent_at_ms: hit.sent_at_ms,
                    };
                    #[cfg(feature = "messaging")]
                    let resolved = self.live.as_ref().and_then(|state| {
                        state.account.as_ref().and_then(|own| {
                            state.sender_names.get(
                                &own.id,
                                self.model.generation,
                                &thread.id,
                                &hit.sender,
                            )
                        })
                    });
                    #[cfg(not(feature = "messaging"))]
                    let resolved = None;
                    let sender = senders::label(
                        &probe,
                        account.as_deref(),
                        Some(&own_name),
                        Some(thread),
                        resolved,
                    )
                    .name
                    .to_owned();
                    if hit_row(ui, hit, &sender, width).clicked() {
                        chosen = Some(hit.clone());
                    }
                }
            });
        if let Some(hit) = chosen {
            let compact = ui.ctx().screen_rect().width() < super::COMPACT_BREAKPOINT;
            self.jump_to_hit(&thread.id, &hit, compact);
        }
    }
}

fn hit_row(ui: &mut egui::Ui, hit: &MessageHit, sender: &str, width: f32) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let body = FontId::proportional(visuals::TEXT_BODY);
    let mut job = LayoutJob::default();
    let (start, end) = hit.highlight;
    let plain = TextFormat::simple(body.clone(), colors.text_primary);
    let marked = TextFormat {
        background: colors.accent_soft,
        color: colors.accent_ink,
        ..TextFormat::simple(body, colors.accent_ink)
    };
    let valid = start <= end
        && end <= hit.snippet.len()
        && hit.snippet.is_char_boundary(start)
        && hit.snippet.is_char_boundary(end);
    if valid {
        job.append(&hit.snippet[..start], 0.0, plain.clone());
        job.append(&hit.snippet[start..end], 0.0, marked);
        job.append(&hit.snippet[end..], 0.0, plain);
    } else {
        job.append(&hit.snippet, 0.0, plain);
    }
    job.wrap.max_width = width - 20.0;
    job.wrap.max_rows = 2;
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let height = galley.size().y + 30.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 10.0, colors.hover);
    }
    let meta = format!(
        "{sender} · {} {}",
        date_label(hit.sent_at_ms),
        clock_label(hit.sent_at_ms)
    );
    ui.painter().text(
        rect.left_top() + vec2(10.0, 6.0),
        egui::Align2::LEFT_TOP,
        meta,
        FontId::proportional(visuals::TEXT_SMALL),
        colors.text_secondary,
    );
    ui.painter().galley(
        rect.left_top() + vec2(10.0, 24.0),
        galley,
        colors.text_primary,
    );
    visuals::focus_outline(ui, &response, rect, 10.0);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, hit.snippet.as_str())
    });
    response.on_hover_text("Mở tin nhắn này trong hội thoại")
}

/// Highlight drawn behind a message reached from a search, fading out.
pub(super) fn focus_alpha(shown_at: Option<Instant>, now: Instant) -> f32 {
    let Some(shown_at) = shown_at else {
        return 1.0;
    };
    let total = crate::app::search::FOCUS_HIGHLIGHT.as_secs_f32();
    let elapsed = now.saturating_duration_since(shown_at).as_secs_f32();
    (1.0 - elapsed / total).clamp(0.0, 1.0)
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::domain::{Cursor, MessageSearch};

    fn frame(
        app: &mut NativeApp,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(width, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.message_search_section(
                        ui,
                        &Conversation {
                            id: "welcome".into(),
                            title: "Tổng hợp".into(),
                            last_message_at_ms: 0,
                            last_text: String::new(),
                            last_sender: String::new(),
                        },
                    );
                });
            },
        )
    }

    fn text_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing label {label}"))
    }

    #[test]
    fn empty_page_can_continue_and_changed_query_disables_continuation_in_both_themes() {
        for dark in [false, true] {
            // Sidebar widths at 1200×780 and 640×480, plus the full compact information page.
            for width in [344.0, 300.0, 560.0] {
                let ctx = egui::Context::default();
                let (mut app, root) = super::super::live::tests::fixture(&ctx);
                app.live = None;
                app.worker = Some(
                    crate::app::Worker::demo(
                        root.join("pages.sqlite3"),
                        std::sync::Arc::new(|| {}),
                    )
                    .unwrap(),
                );
                crate::theme::apply(
                    &ctx,
                    if dark {
                        crate::theme::ThemeMode::Dark
                    } else {
                        crate::theme::ThemeMode::Light
                    },
                );
                app.message_search = SearchInput {
                    thread: "welcome".into(),
                    text: "offline".into(),
                };
                let boundary = Cursor {
                    sent_at_ms: 10,
                    id: "boundary".into(),
                };
                let mut request = 0;
                app.model
                    .request_search("welcome", "offline", |r, _| {
                        request = r;
                        Ok(())
                    })
                    .unwrap();
                app.model.apply_search(
                    request,
                    "welcome",
                    MessageSearch {
                        query: "offline".into(),
                        hits: vec![],
                        truncated: true,
                        next: Some(boundary),
                    },
                );
                let _ = frame(&mut app, &ctx, width, vec![]);
                let output = frame(&mut app, &ctx, width, vec![]);
                let older = text_rect(&output, "Tìm cũ hơn");
                let newest = text_rect(&output, "Mới nhất");
                assert!(older.right() < width && newest.right() < width);
                assert!(older.bottom() <= 480.0 && newest.bottom() <= 480.0);
                // Changing the query must not dispatch an old boundary under a new query.
                app.message_search.text = "changed".into();
                let click = |pressed| egui::Event::PointerButton {
                    pos: older.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let _ = frame(
                    &mut app,
                    &ctx,
                    width,
                    vec![egui::Event::PointerMoved(older.center()), click(true)],
                );
                let _ = frame(&mut app, &ctx, width, vec![click(false)]);
                assert!(!app.model.search_pending("welcome"));
                app.message_search.text = "offline".into();
                let _ = frame(&mut app, &ctx, width, vec![]);
                let _ = frame(&mut app, &ctx, width, vec![click(true)]);
                let _ = frame(&mut app, &ctx, width, vec![click(false)]);
                assert!(
                    app.model.search_pending("welcome"),
                    "empty scan pages must remain navigable"
                );
                drop(app);
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
