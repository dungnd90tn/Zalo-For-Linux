//! HD and original JPEG photo selection. File I/O and preview preparation stay in the worker.
use super::{visuals, NativeApp};
use crate::app::live::{FileTransferPhase, LiveCommand, PhotoResolution, PreparedPhoto};
use crate::domain::Attachment;
use eframe::egui::{self, pos2, vec2, FontId, Sense};

const HEIGHT: f32 = 84.0;

pub(super) fn resolution_title(resolution: PhotoResolution) -> &'static str {
    match resolution {
        PhotoResolution::Hd => "Ảnh HD",
        PhotoResolution::Original => "Ảnh gốc",
    }
}

#[derive(Default)]
pub(super) struct Preview {
    identity: Option<std::sync::Weak<[u8]>>,
    texture: Option<egui::TextureHandle>,
    shown_at: Option<f64>,
    scope: Option<(String, String, Option<u64>)>,
}
impl Preview {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn scope(&mut self, scope: Option<(String, String, Option<u64>)>) {
        if self.scope != scope {
            self.clear();
            self.scope = scope;
        }
    }
    pub fn presented(&self, photo: &PreparedPhoto, now: f64) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|key| key.ptr_eq(&std::sync::Arc::downgrade(&photo.thumbnail().rgba)))
            && self.texture.is_some()
            && self.shown_at.is_some_and(|time| time < now)
    }
    fn texture(
        &mut self,
        ctx: &egui::Context,
        photo: &PreparedPhoto,
    ) -> Option<egui::TextureHandle> {
        if !self
            .identity
            .as_ref()
            .is_some_and(|key| key.ptr_eq(&std::sync::Arc::downgrade(&photo.thumbnail().rgba)))
        {
            let scope = self.scope.take();
            self.clear();
            self.scope = scope;
            let thumbnail = photo.thumbnail();
            let pixels = (thumbnail.width as usize).checked_mul(thumbnail.height as usize)?;
            if thumbnail.width == 0
                || thumbnail.height == 0
                || thumbnail.width > 256
                || thumbnail.height > 256
                || pixels.checked_mul(4)? != thumbnail.rgba.len()
            {
                return None;
            }
            self.texture = Some(ctx.load_texture(
                "selected_photo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [thumbnail.width as usize, thumbnail.height as usize],
                    &thumbnail.rgba,
                ),
                egui::TextureOptions::LINEAR,
            ));
            self.identity = Some(std::sync::Arc::downgrade(&photo.thumbnail().rgba));
            self.shown_at = Some(ctx.input(|input| input.time));
            ctx.request_repaint();
        }
        self.texture.clone()
    }
}

impl NativeApp {
    pub(super) fn photo_can_choose(&self, resolution: PhotoResolution) -> bool {
        !self.demo
            && !self.selected_group_left()
            && self
                .model
                .selected
                .as_deref()
                .is_some_and(|thread| self.privacy_visible(thread))
            && self.drafts.active.reply().is_none()
            && !self.has_draft_mentions()
            && self.drafts.active.attachment().is_none()
            && !self.drafts.active.photo_preparing()
            && self.live.as_ref().is_some_and(|state| {
                state.uploads.supports_photo(resolution) && state.worker.is_some()
            })
    }
    pub(super) fn photo_can_send(&self) -> bool {
        !self.photo_picker_pending()
            && self.live_can_send()
            && self.live.as_ref().is_some_and(|state| {
                !state.uploads.blocked()
                    && self.drafts.active.photo().is_some_and(|photo| {
                        self.photo_can_choose(photo.resolution())
                            && state
                                .photo_preview
                                .presented(photo, state.context.input(|input| input.time))
                    })
            })
    }
    pub(super) fn photo_tray_height(&self) -> f32 {
        if self.photo_selected() {
            HEIGHT
        } else {
            0.0
        }
    }
    pub(super) fn photo_toolbar_buttons(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let available = enabled && !self.attachments.busy();
        if visuals::icon_button(
            ui,
            visuals::Icon::Image,
            if self.has_draft_mentions() {
                "Xóa phần nhắc thành viên để gửi ảnh HD."
            } else {
                "Ảnh HD · JPEG, PNG hoặc WebP tĩnh, tối đa 20 MiB."
            },
            available && self.photo_can_choose(PhotoResolution::Hd),
        )
        .clicked()
        {
            self.choose_photo(ui.ctx(), PhotoResolution::Hd);
        }
        if self.live.as_ref().is_some_and(|state| {
            state.uploads.photo_available || state.uploads.photo_original_available
        }) {
            let response = ui.add_enabled(
                available && self.photo_can_choose(PhotoResolution::Original),
                egui::Button::new("Ảnh gốc").min_size(vec2(80.0, 44.0)),
            );
            visuals::focus_outline(ui, &response, response.rect, 12.0);
            if response.clicked() {
                self.choose_photo(ui.ctx(), PhotoResolution::Original);
            }
            let unavailable = if self
                .live
                .as_ref()
                .is_some_and(|state| state.uploads.photo_original_available)
            {
                "Bỏ tệp, trích dẫn hoặc phần nhắc tên và chờ xử lý ảnh trước khi chọn ảnh gốc."
            } else {
                "Zalo chưa cấp quyền gửi ảnh gốc cho phiên này."
            };
            response.on_hover_text("JPEG tối đa 5 MiB, 2560 px mỗi chiều. Giữ nguyên dữ liệu ảnh và bỏ metadata APP1.")
                .on_disabled_hover_text(unavailable);
        }
    }

    fn choose_photo(&mut self, ctx: &egui::Context, resolution: PhotoResolution) {
        if !self.photo_can_choose(resolution) || self.attachments.busy() {
            return;
        }
        let thread = self.model.selected.clone();
        let account = self
            .account_id()
            .unwrap_or(crate::app::DEMO_ACCOUNT)
            .to_owned();
        let auth_generation = self.auth_generation();
        // A new selection is a draft intent even before its background result.
        // A late ACK for the previous photo must not erase this caption.
        self.drafts.active.edited();
        self.attachments.open_photo_dialog(
            ctx,
            thread.as_deref(),
            &account,
            auth_generation,
            self.model.generation,
            resolution,
        );
    }
    pub(super) fn begin_photo_preparation(
        &mut self,
        attachment: Attachment,
        resolution: PhotoResolution,
    ) {
        if !self.photo_can_choose(resolution) {
            self.model.status = "Chưa thể chuẩn bị ảnh. Bỏ tệp hoặc trích dẫn, rồi thử lại.".into();
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
        match self.drafts.active.prepare_live_photo_with_resolution(
            self.model.generation,
            thread,
            attachment,
            resolution,
            |command| worker.submit(command),
        ) {
            Ok(()) => state.photo_preview.clear(),
            Err(error) => self.model.status = error.to_string(),
        }
        self.focus_composer = true;
    }
    pub(super) fn remove_selected_photo(&mut self) {
        let pending = self.drafts.active.photo_preparation_request();
        if let Some(state) = &mut self.live {
            if let (Some(request_id), Some(worker)) = (pending, state.worker.as_ref()) {
                let _ = worker.submit(LiveCommand::CancelPhotoPreparation { request_id });
            }
            state.photo_preview.clear();
        }
        self.attachments.invalidate_photo_choice();
        self.drafts.active.clear_photo();
        self.focus_composer = true;
    }
    pub(super) fn send_live_photo(&mut self) {
        if !self.photo_can_send() || self.model.pending {
            self.model.status = "Xem ảnh đã chuẩn bị và kiểm tra kết nối trước khi gửi; nội dung đang soạn được giữ lại.".into();
            return;
        }
        let Some(thread) = self.model.selected.clone() else {
            return;
        };
        let Some(photo) = self.drafts.active.photo().cloned() else {
            return;
        };
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
            .send_live_photo(self.model.generation, thread.clone(), |command| {
                let id = match &command {
                    LiveCommand::SendPhoto { request_id, .. } => Some(*request_id),
                    _ => None,
                };
                worker.submit(command)?;
                request = id;
                Ok(())
            }) {
            Ok(()) => {
                if let Some(request) = request {
                    state.uploads.submitted_photo(&thread, request, photo);
                }
            }
            Err(error) => self.model.status = error.to_string(),
        }
        self.focus_composer = true;
    }
    pub(super) fn photo_send_label(&self) -> &'static str {
        if self
            .live
            .as_ref()
            .and_then(|state| state.uploads.transfer.as_ref())
            .is_some_and(|job| {
                self.model
                    .selected
                    .as_deref()
                    .is_some_and(|thread| job.is_photo(thread, self.drafts.active.photo()))
                    && matches!(
                        job.phase,
                        FileTransferPhase::Failed | FileTransferPhase::Cancelled
                    )
            })
        {
            "Thử lại"
        } else {
            "Gửi ảnh"
        }
    }
    pub(super) fn photo_tray(&mut self, ui: &mut egui::Ui, card: egui::Rect) {
        if !self.photo_selected() {
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        let tray = egui::Rect::from_min_size(card.min, vec2(card.width(), HEIGHT));
        let preview = egui::Rect::from_min_size(tray.min + vec2(12.0, 10.0), vec2(64.0, 64.0));
        let close = egui::Rect::from_center_size(
            pos2(tray.right() - 28.0, tray.center().y),
            vec2(44.0, 44.0),
        );
        let picker_problem = self.photo_picker_notice().map(str::to_owned);
        let picker_resolution = self.photo_picker_resolution();
        let resolution = picker_resolution.unwrap_or_else(|| self.drafts.active.photo_resolution());
        let prepared = if picker_resolution.is_some() {
            None
        } else {
            self.drafts.active.photo().cloned()
        };
        let preparing = self.drafts.active.photo_preparing()
            || (self.photo_picker_pending() && self.attachments.busy());
        let problem = self
            .drafts
            .active
            .photo_problem()
            .map(str::to_owned)
            .or(picker_problem);
        let retry = !preparing && prepared.is_none() && problem.is_some();
        let retry_same_source = resolution == PhotoResolution::Hd
            && picker_resolution.is_none()
            && self.drafts.active.photo_source().is_some();
        let retry_rect = egui::Rect::from_center_size(
            pos2(close.left() - 40.0, tray.center().y),
            vec2(72.0, 44.0),
        );
        let width = ((if retry {
            retry_rect.left()
        } else {
            close.left()
        }) - preview.right()
            - 20.0)
            .max(1.0);
        let job = self
            .live
            .as_ref()
            .and_then(|state| state.uploads.transfer.as_ref())
            .filter(|job| {
                self.model
                    .selected
                    .as_deref()
                    .is_some_and(|thread| job.is_photo(thread, prepared.as_ref()))
            })
            .map(|job| {
                (
                    job.label(),
                    job.busy(),
                    job.review(),
                    job.can_cancel(),
                    job.cancel_pending,
                    (job.total > 0)
                        .then(|| (job.uploaded as f64 / job.total as f64).clamp(0.0, 1.0) as f32),
                )
            });
        let texture = self.live.as_mut().and_then(|state| {
            if let Some(photo) = &prepared {
                state.photo_preview.texture(ui.ctx(), photo)
            } else {
                state.photo_preview.clear();
                None
            }
        });
        ui.painter().rect_filled(preview, 10.0, colors.accent_soft);
        if let Some(texture) = texture {
            let size = visuals::fit_image(
                texture.size_vec2(),
                preview.size(),
                ui.ctx().pixels_per_point(),
            );
            ui.painter().image(
                texture.id(),
                egui::Rect::from_center_size(preview.center(), size),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else if preparing {
            let mut spinner = ui.new_child(egui::UiBuilder::new().max_rect(preview.shrink(16.0)));
            spinner.add(egui::Spinner::new().size(24.0).color(colors.primary));
        } else {
            visuals::icon(
                ui,
                preview.shrink(16.0),
                super::Icon::Image,
                colors.text_secondary,
            );
        }
        let title = prepared
            .as_ref()
            .map(|photo| {
                format!(
                    "{} · {} × {}",
                    resolution_title(photo.resolution()),
                    photo.width(),
                    photo.height()
                )
            })
            .unwrap_or_else(|| {
                if preparing {
                    format!(
                        "Đang chuẩn bị {}…",
                        match resolution {
                            PhotoResolution::Hd => "ảnh HD",
                            PhotoResolution::Original => "ảnh gốc",
                        }
                    )
                } else {
                    format!("{} · chưa chuẩn bị được", resolution_title(resolution))
                }
            });
        let detail = if let Some((label, ..)) = &job {
            label.clone()
        } else if let Some(problem) = &problem {
            problem.clone()
        } else {
            prepared
                .as_ref()
                .map(|photo| {
                    format!(
                        "JPEG · {}",
                        super::attachments::size_text(photo.size_bytes())
                    )
                })
                .unwrap_or_else(|| "Xem ảnh trước khi gửi".into())
        };
        let left = preview.right() + 12.0;
        super::row_text(
            ui,
            pos2(left, tray.top() + 12.0),
            width,
            &title,
            FontId::new(
                visuals::TEXT_BODY,
                egui::FontFamily::Name("Zalo Semibold".into()),
            ),
            colors.text_primary,
        );
        ui.interact(
            egui::Rect::from_min_size(pos2(left, tray.top() + 30.0), vec2(width, 44.0)),
            ui.id().with("photo_detail"),
            Sense::hover(),
        )
        .on_hover_text(&detail);
        super::row_text(
            ui,
            pos2(left, tray.top() + 34.0),
            width,
            &detail,
            FontId::proportional(visuals::TEXT_SMALL),
            if problem.is_some() {
                colors.error
            } else {
                colors.text_secondary
            },
        );
        super::row_text(
            ui,
            pos2(left, tray.top() + 55.0),
            width,
            match resolution {
                PhotoResolution::Hd => "Nén JPEG HD",
                PhotoResolution::Original if prepared.is_some() => {
                    "JPEG không nén lại · bỏ metadata APP1"
                }
                PhotoResolution::Original if problem.is_some() => {
                    "Bỏ ảnh để đính kèm dưới dạng tệp"
                }
                PhotoResolution::Original => "JPEG · tối đa 5 MiB, 2560 px mỗi chiều",
            },
            FontId::proportional(visuals::TEXT_SMALL),
            colors.text_secondary,
        );
        if let Some((_, _, _, _, _, Some(progress))) = job.as_ref() {
            let line = egui::Rect::from_min_size(
                tray.left_bottom() - vec2(-12.0, 3.0),
                vec2((tray.width() - 24.0) * *progress, 3.0),
            );
            ui.painter().rect_filled(line, 1.5, colors.primary);
        }
        if retry {
            let response = ui
                .add_enabled_ui(self.photo_can_choose(resolution), |ui| {
                    ui.put(
                        retry_rect,
                        egui::Button::new(if retry_same_source {
                            "Thử lại"
                        } else {
                            "Chọn lại"
                        }),
                    )
                })
                .inner;
            if response.clicked() {
                if let Some(source) = self
                    .drafts
                    .active
                    .photo_source()
                    .cloned()
                    .filter(|_| retry_same_source)
                {
                    self.begin_photo_preparation(source, resolution);
                } else {
                    self.choose_photo(ui.ctx(), resolution);
                }
            }
        }
        let (busy, review, can_cancel, cancel_pending) = job
            .as_ref()
            .map(|(_, busy, review, cancel, pending, _)| (*busy, *review, *cancel, *pending))
            .unwrap_or((false, false, false, false));
        let response = ui
            .add_enabled_ui(!busy || can_cancel, |ui| {
                ui.interact(close, ui.id().with("photo_dismiss"), Sense::click())
            })
            .inner;
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(close, 10.0, colors.hover);
        }
        visuals::source_icon(
            ui,
            close.shrink(10.0),
            "fa-close_24",
            if cancel_pending {
                colors.text_placeholder
            } else {
                colors.text_secondary
            },
        );
        visuals::focus_outline(ui, &response, close, 10.0);
        if response.clicked() {
            if review {
                self.dismiss_file_review();
            } else if busy {
                self.cancel_file_upload();
            } else {
                self.remove_selected_photo();
            }
        }
        response
            .on_hover_text(if review {
                "Đóng cảnh báo · không tự gửi lại ảnh"
            } else if busy {
                "Hủy gửi ảnh · giữ bản nháp"
            } else {
                "Bỏ ảnh · giữ nội dung đang soạn"
            })
            .on_disabled_hover_text("Đang chờ Zalo xác nhận; không tự gửi lại ảnh.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::live::{LiveEvent, LiveWorker, PhotoPreparationFailure};

    #[test]
    fn gpu_preview_does_not_retain_prepared_capability_and_scope_clears_texture() {
        let root = std::env::temp_dir().join(format!(
            "zalo-photo-preview-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let profile = crate::profile::Profile::at_root(&root, "synthetic").unwrap();
        let path = root.join("synthetic.png");
        image::RgbImage::from_pixel(32, 20, image::Rgb([30, 120, 180]))
            .save(&path)
            .unwrap();
        let source = Attachment::inspect(&path).unwrap();
        let photo =
            crate::app::live::runtime::photo::prepare_fixture(&profile, &source, "11", "d:22")
                .unwrap();
        let weak = std::sync::Arc::downgrade(&photo.thumbnail().rgba);
        let ctx = egui::Context::default();
        let mut preview = Preview::default();
        preview.scope(Some(("11".into(), "d:22".into(), Some(1))));
        let _ = ctx.run(Default::default(), |ctx| {
            assert!(preview.texture(ctx, &photo).is_some());
        });
        drop(photo);
        assert!(
            weak.upgrade().is_none(),
            "GPU cache must not keep prepared snapshot or its thumbnail alive"
        );
        preview.scope(Some(("11".into(), "d:33".into(), Some(1))));
        assert!(preview.texture.is_none());
        assert!(preview.identity.is_none());
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn original_uses_its_own_capability_and_preserves_caption_after_preparation_failure() {
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let (worker, mut commands, _events) = LiveWorker::presentation_fixture("11");
        let state = app.live.as_mut().unwrap();
        state.worker = Some(worker);
        state.uploads.observe(&LiveEvent::PhotoCapability {
            account_id: "11".into(),
            available: true,
            original_available: false,
        });
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Chú thích đang soạn".into();
        let file = root.join("preview.jpg");
        std::fs::write(&file, b"synthetic picker metadata").unwrap();
        let attachment = Attachment::inspect(&file).unwrap();
        assert!(app.photo_can_choose(PhotoResolution::Hd));
        assert!(!app.photo_can_choose(PhotoResolution::Original));
        app.begin_photo_preparation(attachment.clone(), PhotoResolution::Original);
        assert!(commands.try_recv().is_err());
        app.live
            .as_mut()
            .unwrap()
            .uploads
            .observe(&LiveEvent::PhotoCapability {
                account_id: "11".into(),
                available: false,
                original_available: true,
            });
        assert!(!app.photo_can_choose(PhotoResolution::Hd));
        assert!(app.photo_can_choose(PhotoResolution::Original));
        app.begin_photo_preparation(attachment, PhotoResolution::Original);
        let Ok(LiveCommand::PreparePhoto {
            request_id,
            generation,
            thread,
            resolution,
            ..
        }) = commands.try_recv()
        else {
            panic!("expected an original photo preparation");
        };
        assert_eq!(thread, "d:22");
        assert_eq!(resolution, PhotoResolution::Original);
        app.drafts.apply_live(
            &LiveEvent::PhotoPrepared {
                account_id: "11".into(),
                generation,
                thread,
                request_id,
                result: Err(PhotoPreparationFailure::OriginalOrientation),
            },
            "11",
        );
        assert_eq!(
            app.drafts.active.photo_resolution(),
            PhotoResolution::Original
        );
        assert!(app.drafts.active.photo_problem().is_some());
        assert_eq!(app.drafts.active.text, "Chú thích đang soạn");
        assert!(!app.photo_can_send());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
