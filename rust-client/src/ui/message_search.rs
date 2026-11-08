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
        let query = self.message_search.text.clone();
        #[cfg(feature = "messaging")]
        if let Some(worker) = self.live.as_ref().and_then(|state| state.worker.as_ref()) {
            let result = self.model.request_search(thread, &query, |request, query| {
                worker.submit(crate::app::live::LiveCommand::Search {
                    request,
                    thread: thread.to_owned(),
                    query,
                })
            });
            self.search_submitted(result);
            return;
        }
        if let Some(worker) = &self.worker {
            let result = self.model.request_search(thread, &query, |request, query| {
                worker.submit(crate::app::Command::Search {
                    request,
                    thread: thread.to_owned(),
                    query,
                })
            });
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
            return;
        }
        let Some(results) = self.model.search_results(&thread.id).cloned() else {
            ui.label(
                small("Chỉ tìm trong lịch sử đã lưu trên máy này, không hỏi máy chủ Zalo.".into())
                    .color(colors.text_secondary),
            );
            return;
        };
        if results.hits.is_empty() {
            ui.label(
                small(format!(
                    "Không có tin nhắn đã lưu nào chứa “{}”.",
                    results.query
                ))
                .color(colors.text_secondary),
            );
            return;
        }
        ui.label(
            small(if results.truncated {
                format!(
                    "{} kết quả mới nhất · có thể còn kết quả cũ hơn",
                    results.hits.len().min(SEARCH_RESULT_LIMIT)
                )
            } else {
                format!("{} kết quả", results.hits.len())
            })
            .color(colors.text_secondary),
        );
        let account = self.account_id().map(str::to_owned);
        let own_name = self.account_label().to_owned();
        let width = ui.available_width();
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt(("message-search", &thread.id))
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
