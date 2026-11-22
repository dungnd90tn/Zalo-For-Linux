//! Bounded, scoped album preparation and preview in the native composer.
use super::{visuals, NativeApp};
use crate::{
    app::live::{FileTransferPhase, LiveCommand, PhotoResolution, MAX_ALBUM_PHOTOS},
    domain::Attachment,
};
use eframe::egui::{self, pos2, vec2, FontId, Sense, Stroke};

pub(super) const HEIGHT: f32 = 116.0;

impl NativeApp {
    pub(super) fn album_limit(&self) -> Option<usize> {
        let limit = self
            .live
            .as_ref()?
            .uploads
            .album_max_count
            .min(MAX_ALBUM_PHOTOS);
        (limit >= 2 && self.photo_can_choose(PhotoResolution::Hd)).then_some(limit)
    }

    pub(super) fn album_selected(&self) -> bool {
        !self.drafts.active.album_sources().is_empty()
    }

    pub(super) fn begin_album_preparation(&mut self, attachments: Vec<Attachment>) {
        if !self
            .album_limit()
            .is_some_and(|limit| (2..=limit).contains(&attachments.len()))
        {
            self.model.status =
                "Chưa thể chuẩn bị album. Kiểm tra quyền gửi ảnh và chọn lại.".into();
            return;
        }
        let Some(thread) = self.model.selected.clone() else {
            return;
        };
        let Some(state) = &mut self.live else {
            return;
        };
        let Some(worker) = &state.worker else {
            return;
        };
        match self.drafts.active.prepare_live_album(
            self.model.generation,
            thread,
            attachments,
            |command| worker.submit(command),
        ) {
            Ok(()) => state.photo_preview.clear(),
            Err(error) => self.model.status = error.to_string(),
        }
        self.focus_composer = true;
    }

    fn remove_selected_album(&mut self) {
        let pending = self.drafts.active.album_preparation_request();
        if let Some(state) = &mut self.live {
            if let (Some(request_id), Some(thread), Some(worker)) =
                (pending, self.model.selected.clone(), state.worker.as_ref())
            {
                let _ = worker.submit(LiveCommand::CancelAlbumPreparation { thread, request_id });
            }
            state.photo_preview.clear();
        }
        self.drafts.active.clear_album();
        self.focus_composer = true;
    }

    pub(super) fn album_can_send(&self) -> bool {
        !self.photo_picker_pending()
            && self.live_can_send()
            && self.drafts.active.album_problem().is_none()
            && self
                .album_limit()
                .zip(self.drafts.active.album())
                .is_some_and(|(limit, photos)| {
                    (2..=limit).contains(&photos.len())
                        && self.live.as_ref().is_some_and(|state| {
                            !state.uploads.blocked()
                                && state.photo_preview.album_presented(
                                    photos,
                                    state.context.input(|input| input.time),
                                )
                        })
                })
    }

    pub(super) fn send_live_album(&mut self) {
        if !self.album_can_send() || self.model.pending {
            self.model.status = "Chưa thể gửi album. Kiểm tra bản xem trước và kết nối.".into();
            return;
        }
        let Some(thread) = self.model.selected.clone() else {
            return;
        };
        let Some(photos) = self.drafts.active.album() else {
            return;
        };
        let binding = super::file_uploads::AlbumBinding::new(photos);
        let Some(state) = &mut self.live else {
            return;
        };
        let Some(worker) = &state.worker else {
            return;
        };
        let mut request = None;
        match self
            .drafts
            .active
            .send_live_album(self.model.generation, thread.clone(), |command| {
                let id = match &command {
                    LiveCommand::SendAlbum { request_id, .. } => Some(*request_id),
                    _ => None,
                };
                worker.submit(command)?;
                request = id;
                Ok(())
            }) {
            Ok(()) => {
                if let Some(request) = request {
                    state.uploads.submitted_album(&thread, request, binding);
                }
            }
            Err(error) => self.model.status = error.to_string(),
        }
        self.focus_composer = true;
    }

    pub(super) fn album_send_label(&self) -> &'static str {
        if self.album_transfer().is_some_and(|job| {
            matches!(
                job.phase,
                FileTransferPhase::Failed | FileTransferPhase::Cancelled
            )
        }) {
            "Thử lại"
        } else {
            "Gửi album"
        }
    }

    fn album_transfer(&self) -> Option<&super::file_uploads::Transfer> {
        let thread = self.model.selected.as_deref()?;
        self.live
            .as_ref()?
            .uploads
            .transfer
            .as_ref()
            .filter(|job| job.is_album(thread, self.drafts.active.album()))
    }

    pub(super) fn album_tray(&mut self, ui: &mut egui::Ui, card: egui::Rect) {
        let palette = crate::theme::palette(ui.ctx());
        let tray = egui::Rect::from_min_size(card.min, vec2(card.width(), HEIGHT));
        let close = egui::Rect::from_min_size(
            pos2(tray.right() - 51.0, tray.top() + 4.0),
            vec2(44.0, 44.0),
        );
        let title = format!(
            "Album · {} ảnh HD",
            self.drafts.active.album_sources().len()
        );
        let job = self.album_transfer().map(|job| {
            (
                job.label(),
                job.busy(),
                job.review(),
                job.can_cancel(),
                (job.total > 0).then(|| job.uploaded.min(job.total) as f32 / job.total as f32),
            )
        });
        let (busy, review, can_cancel) = job
            .as_ref()
            .map_or((false, false, false), |(_, busy, review, cancel, _)| {
                (*busy, *review, *cancel)
            });
        let description = if let Some((label, ..)) = &job {
            label.as_str()
        } else if self.drafts.active.album_preparing() {
            "Đang chuẩn bị ảnh…"
        } else {
            self.drafts
                .active
                .album_problem()
                .unwrap_or("Nội dung đang soạn được giữ riêng.")
        };
        let left = tray.left() + 14.0;
        let text_width = (close.left() - left - 8.0).max(0.0);
        super::row_text(
            ui,
            pos2(left, tray.top() + 8.0),
            text_width,
            &title,
            FontId::new(
                visuals::TEXT_BODY,
                egui::FontFamily::Name("Zalo Semibold".into()),
            ),
            palette.text_primary,
        );
        let detail =
            egui::Rect::from_min_size(pos2(left, tray.top() + 29.0), vec2(text_width, 21.0));
        super::row_text(
            ui,
            detail.min,
            text_width,
            description,
            FontId::proportional(visuals::TEXT_SMALL),
            palette.text_secondary,
        );
        ui.interact(detail, ui.id().with("album_description"), Sense::hover())
            .on_hover_text(description);
        if let (Some(photos), Some(state)) = (self.drafts.active.album(), &mut self.live) {
            let side = ((tray.width() - 26.0 - 6.0 * (photos.len() - 1) as f32)
                / photos.len() as f32)
                .clamp(4.0, 54.0);
            let textures = state.photo_preview.album_textures(ui.ctx(), photos);
            for (index, texture) in textures.enumerate() {
                let tile = egui::Rect::from_min_size(
                    pos2(left + (side + 6.0) * index as f32, tray.top() + 52.0),
                    vec2(side, 54.0),
                );
                ui.painter().rect_filled(tile, 8.0, palette.selected);
                if let Some(texture) = texture {
                    let fitted = visuals::fit_image(
                        texture.size_vec2(),
                        tile.size() - vec2(4.0, 4.0),
                        ui.ctx().pixels_per_point(),
                    );
                    egui::Image::new(&texture)
                        .paint_at(ui, egui::Rect::from_center_size(tile.center(), fitted));
                }
            }
        }
        let rule = tray.left_bottom() + vec2(12.0, -1.0);
        let length = (tray.width() - 24.0).max(0.0);
        ui.painter().line_segment(
            [rule, rule + vec2(length, 0.0)],
            Stroke::new(1.0_f32, palette.border),
        );
        if let Some((_, true, _, _, Some(progress))) = job.as_ref() {
            ui.painter().line_segment(
                [rule, rule + vec2(length * *progress, 0.0)],
                Stroke::new(3.0_f32, palette.primary),
            );
        }
        let response = ui
            .add_enabled_ui(!busy || can_cancel, |ui| {
                ui.interact(close, ui.id().with("album_dismiss"), Sense::click())
            })
            .inner;
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.painter().rect_filled(close, 12.0, palette.hover);
        }
        visuals::source_icon(
            ui,
            close.shrink(10.0),
            "fa-close_24",
            if busy && !can_cancel {
                palette.text_placeholder
            } else {
                palette.text_secondary
            },
        );
        visuals::focus_outline(ui, &response, close, 12.0);
        let label = if review {
            "Đóng cảnh báo và bỏ album khỏi bản nháp · giữ nội dung đang soạn"
        } else if busy {
            "Hủy tải album · giữ bản nháp"
        } else {
            "Bỏ album · giữ nội dung đang soạn"
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy || can_cancel, label)
        });
        if response.clicked() {
            if review {
                self.dismiss_file_review();
            } else if busy {
                self.cancel_file_upload();
            } else {
                self.remove_selected_album();
            }
        }
        response
            .on_hover_text(label)
            .on_disabled_hover_text("Đang chờ Zalo xác nhận lượt gửi album.");
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        app::live::{LiveConnection, LiveEvent, PreparedPhoto, TransferKind},
        domain::{MediaDescriptor, Message, PhotoAlbumMember, PhotoDescriptor},
    };

    fn message(index: u32) -> Message {
        Message {
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            source_client_id: None,
            quote: None,
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: format!("synthetic-album-{index}"),
            sender: "22".into(),
            text: "Ảnh".into(),
            sent_at_ms: 1_700_000_000_500 + i64::from(index),
            sticker: None,
            media: Some(MediaDescriptor::Photo(PhotoDescriptor {
                title: "Ảnh".into(),
                caption: String::new(),
                thumbnail_url: None,
                original_url: None,
                width: None,
                height: None,
                album: PhotoAlbumMember::new(101, index, 2),
            })),
        }
    }

    #[test]
    fn album_timeline_keeps_each_message_and_rejects_collisions() {
        use crate::ui::{album_positions, AlbumPosition::*};

        let first = message(0);
        let last = message(1);
        assert_eq!(
            album_positions(&[first.clone(), last.clone()]),
            [First(2), Last]
        );
        assert_eq!(
            album_positions(&[last.clone(), first.clone()]),
            [Single, Single]
        );
        assert_eq!(album_positions(std::slice::from_ref(&last)), [Single]);

        let mut other_sender = last.clone();
        other_sender.sender = "33".into();
        assert_eq!(
            album_positions(&[first.clone(), other_sender]),
            [Single, Single]
        );
        let mut duplicate_index = last.clone();
        duplicate_index.id = "different-id".into();
        assert_eq!(
            album_positions(&[first.clone(), last.clone(), duplicate_index]),
            [Single, Single, Single]
        );
        let mut interleaved = last.clone();
        interleaved.id = "unrelated-id".into();
        interleaved.media = None;
        assert_eq!(
            album_positions(&[first, interleaved, last]),
            [Single, Single, Single]
        );
    }

    fn seed_app(app: &mut NativeApp, count: u32) -> Vec<PreparedPhoto> {
        let mut attachments = Vec::new();
        let mut photos = Vec::new();
        for index in 0..count {
            let path = app
                ._profile
                .paths
                .data_dir
                .join(format!("synthetic-{index}.png"));
            let source = image::RgbImage::from_fn(96, 64, |x, y| {
                let base = [(x + 60) as u8, (y + 84) as u8, (index * 30 + 80) as u8];
                image::Rgb(match index % 3 {
                    0 => base,
                    1 => [base[2], base[0], base[1]],
                    _ => [base[1], base[2], base[0]],
                })
            });
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            source.save(&path).unwrap();
            let path = Attachment::inspect(&path).unwrap();
            photos.push(
                crate::app::live::runtime::photo::prepare_fixture(
                    &app._profile,
                    &path,
                    "11",
                    "d:22",
                )
                .unwrap(),
            );
            attachments.push(path);
        }
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.compact_list = false;
        app.drafts.active.clear_reply();
        app.drafts.active.text = "Hẹn bạn ở buổi thử nghiệm.".into();
        let mut request_id = 0;
        app.drafts
            .active
            .prepare_live_album(
                app.model.generation,
                "d:22".into(),
                attachments,
                |command| {
                    let LiveCommand::PrepareAlbum { request_id: id, .. } = command else {
                        panic!("expected album preparation");
                    };
                    request_id = id;
                    Ok(())
                },
            )
            .unwrap();
        app.drafts.apply_live(
            &LiveEvent::AlbumPrepared {
                account_id: "11".into(),
                generation: app.model.generation,
                thread: "d:22".into(),
                request_id,
                result: Ok(photos.clone()),
            },
            "11",
        );
        let state = app.live.as_mut().unwrap();
        state.uploads.photo_available = true;
        state.uploads.album_max_count = MAX_ALBUM_PHOTOS;
        photos
    }

    pub(in crate::ui) fn scene(app: &mut NativeApp, ctx: &egui::Context, phase: &str) {
        let photos = seed_app(app, MAX_ALBUM_PHOTOS as u32);
        let state = app.live.as_mut().unwrap();
        state.uploads.photo_available = true;
        state.uploads.album_max_count = MAX_ALBUM_PHOTOS;
        state.connection = LiveConnection::Online;
        app.fixture_connection = Some(LiveConnection::Online);
        if phase == "partial" {
            let binding = super::super::file_uploads::AlbumBinding::new(&photos);
            let mut request = 0;
            app.drafts
                .active
                .send_live_album(app.model.generation, "d:22".into(), |command| {
                    let LiveCommand::SendAlbum { request_id, .. } = command else {
                        panic!("expected album send");
                    };
                    request = request_id;
                    Ok(())
                })
                .unwrap();
            state.uploads.submitted_album("d:22", request, binding);
            let event = LiveEvent::FileTransfer {
                kind: TransferKind::Album,
                account_id: "11".into(),
                thread: "d:22".into(),
                request_id: request,
                phase: FileTransferPhase::Partial,
                uploaded: 2048,
                total: 8192,
                message: Some("Zalo đã nhận 2/6 ảnh. Kiểm tra lịch sử trước khi chọn lại.".into()),
            };
            state.uploads.observe(&event);
            app.drafts.apply_live(&event, "11");
        }
        let mut messages = Vec::new();
        for (index, photo) in photos.iter().take(2).enumerate() {
            let mut message = message(index as u32);
            let url = format!("https://example.invalid/album-{index}.jpg");
            let Some(MediaDescriptor::Photo(media)) = &mut message.media else {
                panic!("fixture must have photo");
            };
            media.thumbnail_url = Some(url.clone());
            let thumbnail = photo.thumbnail();
            state.media.seed(
                ctx,
                crate::app::media::MediaKey {
                    account_id: "11".into(),
                    thread_id: "d:22".into(),
                    message_id: message.id.clone(),
                    url,
                },
                app.model.generation,
                thumbnail.width,
                thumbnail.height,
                thumbnail.rgba.to_vec(),
            );
            messages.push(message);
        }
        app.model.messages = messages;
    }

    #[test]
    fn album_preview_preserves_separate_draft_when_dismissed() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let photos = seed_app(&mut app, 2);
        let screen = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0));
        let card = egui::Rect::from_min_size(pos2(20.0, 20.0), vec2(500.0, 210.0));
        let close = pos2(card.right() - 29.0, card.top() + 26.0);
        let render = |app: &mut NativeApp, time, events| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.album_tray(ui, card);
                });
            });
        };
        render(&mut app, 10.0, vec![]);
        let preview = &app.live.as_ref().unwrap().photo_preview;
        assert!(preview.album_presented(&photos, 11.0));
        assert!(!preview.album_presented(&photos[..1], 11.0));
        assert!(!preview.album_presented(&[photos[1].clone(), photos[0].clone()], 11.0));
        let button = |pressed| egui::Event::PointerButton {
            pos: close,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        render(
            &mut app,
            11.0,
            vec![egui::Event::PointerMoved(close), button(true)],
        );
        // The release arrives on the next frame, within egui's click duration.
        render(&mut app, 11.0 + 1.0 / 60.0, vec![button(false)]);
        assert!(!app.album_selected());
        assert_eq!(app.drafts.active.text, "Hẹn bạn ở buổi thử nghiệm.");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
