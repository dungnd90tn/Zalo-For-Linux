use super::{visuals, NativeApp};
use crate::{
    app::{
        live::{FileTransferPhase, LiveCommand, LiveEvent, TransferKind},
        media::MediaKey,
        voice::{self, Player, PreparedVoice, Recorder},
    },
    domain::{Message, VoiceDescriptor},
};
use eframe::egui::{self, pos2, vec2, Align2, FontId, Rect, RichText, Sense};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const TRAY_HEIGHT: f32 = 68.0;
const RECORD_REFRESH: Duration = Duration::from_millis(250);
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(PartialEq, Eq)]
struct Scope {
    account: String,
    thread: String,
    auth: Option<u64>,
}

enum Capture {
    Recording {
        recorder: Recorder,
        started: Instant,
        finishing: bool,
    },
    Ready(PreparedVoice),
    Failed,
}

struct Draft {
    scope: Scope,
    capture: Capture,
    request: Option<u64>,
}

/// Keep only one in-memory voice draft, bound to the account and conversation
/// where recording began. It never replaces text or attachment drafts.
#[derive(Default)]
pub(super) struct Composer {
    draft: Option<Draft>,
}

impl Composer {
    pub(super) fn observe(&mut self, event: &LiveEvent) {
        if let LiveEvent::FileTransfer {
            kind: TransferKind::Voice,
            phase: FileTransferPhase::Accepted,
            account_id,
            thread,
            request_id,
            ..
        } = event
        {
            if self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.scope.account == *account_id)
            {
                self.clear_receipt(thread, *request_id);
            }
        }
    }

    pub(super) fn clear_receipt(&mut self, thread: &str, request: u64) {
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.scope.thread == thread && draft.request == Some(request))
        {
            self.draft = None;
        }
    }
}

#[derive(PartialEq, Eq)]
enum AudioTarget {
    Recording,
    Message(MediaKey),
}

#[derive(Default)]
pub(in crate::ui) struct Playback {
    target: Option<AudioTarget>,
    player: Option<Player>,
    failed: bool,
}

impl Playback {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn clear_recording(&mut self) {
        if self.target == Some(AudioTarget::Recording) {
            self.clear();
        }
    }

    pub(in crate::ui) fn clear_message(&mut self, account: &str, thread: &str, id: &str) {
        if matches!(&self.target, Some(AudioTarget::Message(key))
            if key.account_id == account && key.thread_id == thread && key.message_id == id)
        {
            self.clear();
        }
    }

    fn active(&self, target: &AudioTarget) -> bool {
        self.player.is_some() && self.target.as_ref() == Some(target)
    }

    fn failed(&self, target: &AudioTarget) -> bool {
        self.failed && self.target.as_ref() == Some(target)
    }

    fn matches_message(&self, message: &Message, url: &str) -> bool {
        matches!(&self.target, Some(AudioTarget::Message(key)) if
            key.account_id == message.account_id &&
            key.thread_id == message.thread_id &&
            key.message_id == message.id &&
            key.url == url)
    }

    fn poll(&mut self) {
        if let Some(result) = self.player.as_mut().and_then(Player::poll) {
            self.player = None;
            self.failed = result.is_err();
        }
    }

    fn toggle<E>(&mut self, target: AudioTarget, start: impl FnOnce() -> Result<Player, E>) {
        let stop = self.active(&target);
        self.clear();
        if !stop {
            self.target = Some(target);
            match start() {
                Ok(player) => self.player = Some(player),
                Err(_) => self.failed = true,
            }
        }
    }
}

fn wake(ctx: &egui::Context) -> Arc<dyn Fn() + Send + Sync> {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint())
}

fn clock_text(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn audio_icon(ui: &mut egui::Ui, rect: Rect, glyph: &str, label: &str, enabled: bool) -> bool {
    let enabled = enabled && ui.is_enabled();
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.interact(rect, ui.id().with(glyph), Sense::click())
        })
        .inner;
    let colors = crate::theme::palette(ui.ctx());
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        ui.painter().rect_filled(
            rect,
            12.0,
            if response.is_pointer_button_down_on() {
                colors.pressed
            } else {
                colors.hover
            },
        );
    }
    visuals::source_icon(
        ui,
        Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
        glyph,
        if enabled {
            colors.text_primary
        } else {
            colors.text_disabled
        },
    );
    visuals::focus_outline(ui, &response, rect, 12.0);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response
        .on_hover_text(label)
        .on_disabled_hover_text(label)
        .clicked()
}

pub(super) fn message_card(
    ui: &mut egui::Ui,
    message: &Message,
    voice: &VoiceDescriptor,
    playback: Option<&mut Playback>,
) {
    let visible = ui.is_rect_visible(Rect::from_min_size(ui.cursor().min, vec2(44.0, 44.0)));
    let has_audio = voice.url.is_some();
    let url = voice.url.as_deref().unwrap_or_default();
    let enabled = visible && has_audio && playback.is_some() && voice::playback_available();
    let current = playback
        .as_ref()
        .filter(|playback| playback.matches_message(message, url));
    let active = current.is_some_and(|playback| playback.player.is_some());
    let failed = current.is_some_and(|playback| playback.failed);
    let colors = crate::theme::palette(ui.ctx());
    ui.push_id((&message.thread_id, &message.id), |ui| {
        ui.horizontal(|ui| {
            let tooltip = if active {
                "Dừng phát tin nhắn thoại"
            } else if !has_audio {
                "Tin nhắn thoại này chưa có âm thanh khả dụng"
            } else if !enabled {
                "Chưa có trình phát âm thanh khả dụng"
            } else {
                "Phát tin nhắn thoại"
            };
            if visuals::icon_button(
                ui,
                if active {
                    visuals::Icon::Stop
                } else {
                    visuals::Icon::Play
                },
                tooltip,
                enabled,
            )
            .clicked()
            {
                if let Some(playback) = playback {
                    let key = MediaKey {
                        account_id: message.account_id.clone(),
                        thread_id: message.thread_id.clone(),
                        message_id: message.id.clone(),
                        url: url.into(),
                    };
                    let repaint = wake(ui.ctx());
                    playback.toggle(AudioTarget::Message(key), || {
                        Player::start_remote(url, repaint)
                    });
                }
            }
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Tin nhắn thoại")
                        .size(visuals::TEXT_BODY)
                        .color(colors.text_primary),
                );
                if failed {
                    ui.label(
                        RichText::new("Chưa phát được âm thanh")
                            .size(visuals::TEXT_SMALL)
                            .color(colors.text_secondary),
                    );
                } else if let Some(duration_ms) = voice.duration_ms {
                    ui.label(
                        RichText::new(clock_text(duration_ms))
                            .size(visuals::TEXT_SMALL)
                            .color(colors.text_secondary),
                    );
                }
            });
        });
    });
}

impl NativeApp {
    fn voice_scope(&self) -> Option<Scope> {
        Some(Scope {
            account: self.account_id()?.into(),
            thread: self.model.selected.as_deref()?.into(),
            auth: self.auth_generation(),
        })
    }

    fn visible_voice(&self) -> Option<&Draft> {
        let draft = self.live.as_ref()?.voice.draft.as_ref()?;
        (Some(draft.scope.account.as_str()) == self.account_id()
            && Some(draft.scope.thread.as_str()) == self.model.selected.as_deref()
            && draft.scope.auth == self.auth_generation()
            && self.privacy_visible(&draft.scope.thread))
        .then_some(draft)
    }

    fn voice_can_start(&self) -> bool {
        voice::recording_available()
            && self.live_can_send()
            && self
                .model
                .selected
                .as_deref()
                .is_some_and(|thread| self.privacy_visible(thread))
            && self.drafts.active.attachment().is_none()
            && !self.photo_selected()
            && self.drafts.active.reply().is_none()
            && !self.has_draft_mentions()
            && !self.attachments.busy()
            && self.live.as_ref().is_some_and(|state| {
                state.uploads.voice_available
                    && !state.uploads.blocked()
                    && state.voice.draft.is_none()
            })
    }

    pub(super) fn voice_toolbar_button(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let available = enabled && self.voice_can_start();
        let label = if available {
            "Ghi âm tin nhắn thoại"
        } else if !voice::recording_available() {
            "Chưa có công cụ ghi âm khả dụng"
        } else if self
            .live
            .as_ref()
            .is_some_and(|state| state.voice.draft.is_some())
        {
            "Hoàn tất bản ghi ở hội thoại đang soạn"
        } else {
            "Cần kết nối và chọn hội thoại hỗ trợ gửi thoại"
        };
        if !visuals::icon_button(ui, visuals::Icon::Mic, label, available).clicked() {
            return;
        }
        let Some(scope) = self.voice_scope() else {
            return;
        };
        let Some(state) = &mut self.live else { return };
        state.media.voice.clear();
        let capture = match Recorder::start(wake(ui.ctx())) {
            Ok(recorder) => Capture::Recording {
                recorder,
                started: Instant::now(),
                finishing: false,
            },
            Err(_) => Capture::Failed,
        };
        state.voice.draft = Some(Draft {
            scope,
            capture,
            request: None,
        });
    }

    pub(super) fn sync_voice(&mut self, ctx: &egui::Context) {
        let auth = self.auth_generation();
        let selected = self.model.selected.as_deref();
        let focused = ctx.input(|input| input.focused);
        let chat_visible = focused
            && !self.modal_open()
            && !self.modal_blocked
            && !self.directory_visible()
            && !self.compact_list
            && (self.conversation_details.is_none()
                || ctx.screen_rect().width() >= super::COMPACT_BREAKPOINT);
        let Some(state) = &mut self.live else { return };
        let account = state.account.as_ref().map(|account| account.id.as_str());
        let chat_visible = chat_visible
            && !state.media.has_viewer()
            && account.is_some()
            && state.privacy.ready()
            && selected.is_some_and(|thread| state.privacy.visible(thread));
        if chat_visible {
            state.media.voice.poll();
        } else {
            state.media.voice.clear();
        }
        let Some(draft) = &mut state.voice.draft else {
            return;
        };
        if Some(draft.scope.account.as_str()) != account
            || draft.scope.auth != auth
            || !state.privacy.ready()
            || !state.privacy.visible(&draft.scope.thread)
        {
            state.voice.draft = None;
            state.media.voice.clear();
            return;
        }
        if let Capture::Recording { recorder, .. } = &mut draft.capture {
            if !chat_visible
                || selected != Some(draft.scope.thread.as_str())
                || !state.uploads.voice_available
            {
                state.voice.draft = None;
            } else if let Some(result) = recorder.poll() {
                draft.capture = match result {
                    Ok(clip) => Capture::Ready(clip),
                    Err(_) => Capture::Failed,
                };
            } else {
                ctx.request_repaint_after(RECORD_REFRESH);
            }
        }
    }

    pub(super) fn voice_tray_height(&self) -> f32 {
        if self.visible_voice().is_some() {
            TRAY_HEIGHT
        } else {
            0.0
        }
    }

    pub(super) fn voice_tray_shows(&self, job: &super::file_uploads::Transfer) -> bool {
        job.kind == TransferKind::Voice
            && self.visible_voice().is_some_and(|draft| {
                job.thread == draft.scope.thread && Some(job.request) == draft.request
            })
    }

    fn submit_voice(&mut self) {
        if !self.live_can_send() || self.modal_blocked || self.modal_open() {
            return;
        }
        let Some(draft) = self.visible_voice() else {
            return;
        };
        let Capture::Ready(clip) = &draft.capture else {
            return;
        };
        let recording = clip.clone();
        let total = recording.bytes().len() as u64;
        let thread = draft.scope.thread.clone();
        let Some(state) = &mut self.live else { return };
        let Some(worker) = &state.worker else { return };
        if state.uploads.blocked() || !state.uploads.voice_available {
            return;
        }
        let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        match worker.submit(LiveCommand::SendVoice {
            generation: self.model.generation,
            thread: thread.clone(),
            recording,
            request_id: request,
        }) {
            Ok(()) => {
                if let Some(draft) = &mut state.voice.draft {
                    draft.request = Some(request);
                }
                state.media.voice.clear();
                state.uploads.submitted_voice(&thread, request, total);
            }
            Err(_) => {
                self.model.status = "Chưa thể gửi tin nhắn thoại. Bản ghi được giữ lại.".into()
            }
        }
    }

    pub(super) fn voice_tray(&mut self, ui: &mut egui::Ui, card: Rect, enabled: bool) {
        let Some(draft) = self.visible_voice() else {
            return;
        };
        let live = self.live.as_ref().expect("scoped voice");
        let transfer = live
            .uploads
            .transfer
            .as_ref()
            .filter(|job| self.voice_tray_shows(job));
        let busy = transfer.is_some_and(|job| job.busy());
        let can_cancel = transfer.is_some_and(|job| job.can_cancel());
        let review = transfer.is_some_and(|job| job.review());
        let active = live.media.voice.active(&AudioTarget::Recording);
        let failed = live.media.voice.failed(&AudioTarget::Recording);
        let message = transfer.map(|job| job.label());
        let (title, status, play, stop, ready) = match &draft.capture {
            Capture::Recording {
                started, finishing, ..
            } => (
                "Đang ghi âm",
                if *finishing {
                    "Đang xử lý bản ghi…".into()
                } else {
                    clock_text(started.elapsed().as_millis().min(60_000) as u32)
                },
                false,
                !*finishing,
                false,
            ),
            Capture::Ready(clip) => (
                "Tin nhắn thoại",
                message.unwrap_or_else(|| {
                    if failed {
                        "Chưa phát được âm thanh".into()
                    } else {
                        clock_text(clip.duration_ms())
                    }
                }),
                true,
                false,
                true,
            ),
            Capture::Failed => (
                "Chưa ghi được âm thanh",
                "Kiểm tra micro và thử lại.".into(),
                false,
                false,
                false,
            ),
        };
        let can_send = enabled
            && ready
            && self.live_can_send()
            && !live.uploads.blocked()
            && live.uploads.voice_available;
        let colors = crate::theme::palette(ui.ctx());
        let tray = Rect::from_min_size(card.min, vec2(card.width(), TRAY_HEIGHT));
        let close = Rect::from_min_size(
            pos2(tray.right() - 51.0, tray.top() + 11.0),
            vec2(44.0, 44.0),
        );
        let playback_button =
            Rect::from_min_size(pos2(tray.left() + 8.0, close.top()), vec2(44.0, 44.0));
        let send = Rect::from_min_size(pos2(close.left() - 97.0, close.top()), vec2(93.0, 44.0));
        let text_left = if play || stop {
            playback_button.right() + 5.0
        } else {
            tray.left() + 16.0
        };
        let text_width =
            ((if ready { send.left() } else { close.left() }) - text_left - 8.0).max(0.0);
        super::row_text(
            ui,
            pos2(text_left, tray.top() + 13.0),
            text_width,
            title,
            FontId::new(
                visuals::TEXT_BODY,
                egui::FontFamily::Name("Zalo Semibold".into()),
            ),
            colors.text_primary,
        );
        let status_pos = pos2(text_left, tray.top() + 34.0);
        super::row_text(
            ui,
            status_pos,
            text_width,
            &status,
            FontId::proportional(visuals::TEXT_SMALL),
            colors.text_secondary,
        );
        ui.interact(
            Rect::from_min_size(status_pos, vec2(text_width, 21.0)),
            ui.id().with("voice_status"),
            Sense::hover(),
        )
        .on_hover_text(&status);
        ui.painter().hline(
            (tray.left() + 14.0)..=(tray.right() - 14.0),
            tray.bottom(),
            egui::Stroke::new(1.0_f32, colors.border),
        );
        if stop
            && audio_icon(
                ui,
                playback_button,
                "fa-Stop_24_Filled",
                "Dừng ghi âm",
                enabled,
            )
        {
            if let Some(Draft {
                capture:
                    Capture::Recording {
                        recorder,
                        finishing,
                        ..
                    },
                ..
            }) = self
                .live
                .as_mut()
                .and_then(|state| state.voice.draft.as_mut())
            {
                recorder.stop();
                *finishing = true;
            }
        }
        if play
            && audio_icon(
                ui,
                playback_button,
                if active {
                    "fa-Stop_24_Filled"
                } else {
                    "fa-Play_24_Filled"
                },
                if active {
                    "Dừng nghe lại bản ghi"
                } else {
                    "Nghe lại bản ghi"
                },
                enabled && voice::playback_available() && !busy,
            )
        {
            if let Some(state) = &mut self.live {
                if let Some(Draft {
                    capture: Capture::Ready(clip),
                    ..
                }) = &state.voice.draft
                {
                    let clip = clip.clone();
                    let repaint = wake(ui.ctx());
                    state.media.voice.toggle(AudioTarget::Recording, || {
                        Player::start_local(clip, repaint)
                    });
                }
            }
        }
        let discard_label = if busy {
            "Hủy lượt gửi thoại"
        } else if review {
            "Đóng và bỏ bản ghi sau khi kiểm tra kết quả"
        } else {
            "Bỏ bản ghi"
        };
        if audio_icon(
            ui,
            close,
            "fa-close_24",
            discard_label,
            enabled && (!busy || can_cancel),
        ) {
            if busy {
                self.cancel_file_upload();
            } else if review {
                self.dismiss_file_review();
            } else if let Some(state) = &mut self.live {
                state.voice.draft = None;
                state.media.voice.clear();
                self.focus_composer = true;
            }
            return;
        }
        if ready {
            let response = ui
                .add_enabled_ui(can_send, |ui| {
                    ui.interact(send, ui.id().with("send_voice"), Sense::click())
                })
                .inner;
            ui.painter().rect_filled(
                send,
                12.0,
                if !can_send {
                    colors.button_primary_disabled
                } else if response.is_pointer_button_down_on() {
                    colors.button_primary_pressed
                } else if response.hovered() {
                    colors.button_primary_hover
                } else {
                    colors.primary
                },
            );
            ui.painter().text(
                send.center(),
                Align2::CENTER_CENTER,
                "Gửi thoại",
                FontId::new(
                    visuals::TEXT_BODY,
                    egui::FontFamily::Name("Zalo Semibold".into()),
                ),
                if can_send {
                    colors.on_color
                } else {
                    colors.text_disabled
                },
            );
            visuals::focus_outline(ui, &response, send, 12.0);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, can_send, "Gửi thoại")
            });
            if response
                .on_hover_text("Gửi bản ghi, giữ lại phần chữ đang soạn")
                .clicked()
            {
                self.submit_voice();
            }
        }
    }
}

#[cfg(test)]
pub(in crate::ui) mod tests {
    use super::*;
    use crate::domain::MediaDescriptor;

    fn clip() -> PreparedVoice {
        PreparedVoice::fixture(
            include_bytes!("../../tests/fixtures/messaging/voice_synthetic.mp3").to_vec(),
            1000,
        )
    }

    fn install_draft(app: &mut NativeApp, request: Option<u64>) {
        let scope = app.voice_scope().unwrap();
        let state = app.live.as_mut().unwrap();
        state.uploads.voice_available = true;
        state.voice.draft = Some(Draft {
            scope,
            capture: Capture::Ready(clip()),
            request,
        });
    }

    pub(in crate::ui) fn scene(app: &mut NativeApp, scene: &str) {
        app.drafts.active.text = "Mình sẽ phản hồi sau cuộc họp.".into();
        app.model.messages[0].media = Some(MediaDescriptor::Voice(VoiceDescriptor {
            url: Some("https://voice.talk.zdn.vn/synthetic.mp3".into()),
            duration_ms: Some(12_000),
        }));
        if scene == "received" {
            return;
        }
        let request = (scene == "unknown").then_some(211);
        install_draft(app, request);
        if let Some(request_id) = request {
            let state = app.live.as_mut().unwrap();
            state.uploads.observe(&LiveEvent::FileTransfer {
                kind: TransferKind::Voice,
                account_id: "11".into(),
                thread: "d:22".into(),
                request_id,
                phase: FileTransferPhase::Unknown,
                uploaded: 6480,
                total: 6480,
                message: None,
            });
        }
    }

    fn transfer(
        kind: TransferKind,
        phase: FileTransferPhase,
        account: &str,
        thread: &str,
        request_id: u64,
    ) -> LiveEvent {
        LiveEvent::FileTransfer {
            kind,
            phase,
            account_id: account.into(),
            thread: thread.into(),
            request_id,
            uploaded: 6480,
            total: 6480,
            message: None,
        }
    }

    #[test]
    fn voice_draft_accepts_only_its_durable_receipt_and_keeps_text() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Bản nháp văn bản".into();
        install_draft(&mut app, Some(211));
        let state = app.live.as_mut().unwrap();
        let accepted = FileTransferPhase::Accepted;
        let voice = TransferKind::Voice;
        for event in [
            transfer(voice, accepted, "99", "d:22", 211),
            transfer(voice, accepted, "11", "d:33", 211),
            transfer(voice, accepted, "11", "d:22", 210),
            transfer(TransferKind::File, accepted, "11", "d:22", 211),
            transfer(voice, FileTransferPhase::Unknown, "11", "d:22", 211),
        ] {
            state.voice.observe(&event);
            assert!(state.voice.draft.is_some());
        }
        state
            .voice
            .observe(&transfer(voice, accepted, "11", "d:22", 211));
        assert!(state.voice.draft.is_none());
        assert_eq!(app.drafts.active.text, "Bản nháp văn bản");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_voice_transfer_uses_one_tray_and_can_be_dismissed_without_retry() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Bản nháp văn bản".into();
        app.live.as_mut().unwrap().privacy = super::super::privacy::Privacy::fixture_ready();
        install_draft(&mut app, Some(211));
        let unknown = transfer(
            TransferKind::Voice,
            FileTransferPhase::Unknown,
            "11",
            "d:22",
            211,
        );
        app.live.as_mut().unwrap().uploads.observe(&unknown);
        assert_eq!(app.voice_tray_height(), TRAY_HEIGHT);
        assert_eq!(app.attachment_tray_height(), 0.0);
        app.model.selected = Some("d:33".into());
        assert_eq!(app.voice_tray_height(), 0.0);
        assert!(app.file_upload_visible());
        app.model.selected = Some("d:22".into());
        app.dismiss_file_review();
        assert_eq!(app.voice_tray_height(), 0.0);
        let state = app.live.as_mut().unwrap();
        assert!(state.uploads.transfer.is_none());
        state.uploads.observe(&unknown);
        assert!(state.uploads.transfer.is_none());
        assert_eq!(app.drafts.active.text, "Bản nháp văn bản");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
