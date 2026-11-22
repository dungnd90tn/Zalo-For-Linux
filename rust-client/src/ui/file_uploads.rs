//! One account-scoped file transfer presentation; the worker owns upload and dispatch.
use super::NativeApp;
use crate::app::live::{
    FileTransferPhase, LiveCommand, LiveEvent, PhotoResolution, PreparedPhoto, TransferKind,
};
use crate::domain::Attachment;
use eframe::egui::{self, vec2, RichText};

pub(super) struct Transfer {
    pub kind: TransferKind,
    pub thread: String,
    pub request: u64,
    pub phase: FileTransferPhase,
    pub uploaded: u64,
    pub total: u64,
    pub note: Option<String>,
    pub cancel_pending: bool,
    attachment: Option<Attachment>,
    photo: Option<PreparedPhoto>,
    album: Option<AlbumBinding>,
}

pub(super) struct AlbumBinding {
    images: Vec<std::sync::Weak<[u8]>>,
    size_bytes: u64,
}

impl AlbumBinding {
    pub fn new(photos: &[PreparedPhoto]) -> Self {
        Self {
            images: photos
                .iter()
                .map(|photo| std::sync::Arc::downgrade(&photo.thumbnail().rgba))
                .collect(),
            size_bytes: photos.iter().map(PreparedPhoto::size_bytes).sum(),
        }
    }
    fn matches(&self, photos: &[PreparedPhoto]) -> bool {
        self.images.len() == photos.len()
            && self.images.iter().zip(photos).all(|(image, photo)| {
                image.ptr_eq(&std::sync::Arc::downgrade(&photo.thumbnail().rgba))
            })
    }
}
impl Transfer {
    fn stage(phase: FileTransferPhase) -> Option<u8> {
        match phase {
            FileTransferPhase::Preparing => Some(0),
            FileTransferPhase::Uploading => Some(1),
            FileTransferPhase::WaitingForResource => Some(2),
            FileTransferPhase::Finalizing => Some(3),
            _ => None,
        }
    }
    pub fn filename(&self) -> Option<&str> {
        match self.kind {
            TransferKind::Photo => Some(self.photo.as_ref().map_or("Ảnh", |photo| {
                super::photos::resolution_title(photo.resolution())
            })),
            TransferKind::Album => Some("Album"),
            TransferKind::Voice => Some("Tin nhắn thoại"),
            TransferKind::File => self.attachment.as_ref().map(|file| file.name.as_str()),
        }
    }
    fn noun(&self) -> &'static str {
        match self.kind {
            TransferKind::Photo => "ảnh",
            TransferKind::Album => "album",
            TransferKind::Voice => "tin nhắn thoại",
            TransferKind::File => "tệp",
        }
    }
    pub fn generic_label(&self) -> String {
        let noun = self.noun();
        if self.cancel_pending {
            return format!("Đang hủy một lượt gửi {noun}…");
        }
        match self.phase {
            FileTransferPhase::Preparing => format!("Đang chuẩn bị một {noun}…"),
            FileTransferPhase::Uploading => format!("Đang tải lên một {noun}…"),
            FileTransferPhase::WaitingForResource => format!("Đang chờ Zalo xử lý một {noun}…"),
            FileTransferPhase::Finalizing => format!("Đang chờ xác nhận lượt gửi {noun}…"),
            FileTransferPhase::Unknown => {
                format!("Một lượt gửi {noun} chưa rõ kết quả. Không tự gửi lại.")
            }
            FileTransferPhase::Partial => "Zalo đã nhận một phần album. Không tự gửi lại.".into(),
            FileTransferPhase::AcceptedLocalFailure => {
                format!("Zalo đã nhận một {noun}; chưa lưu được xác nhận.")
            }
            _ => format!("Gửi {noun}"),
        }
    }
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            FileTransferPhase::Preparing
                | FileTransferPhase::Uploading
                | FileTransferPhase::WaitingForResource
                | FileTransferPhase::Finalizing
        )
    }
    pub fn review(&self) -> bool {
        matches!(
            self.phase,
            FileTransferPhase::Partial
                | FileTransferPhase::Unknown
                | FileTransferPhase::AcceptedLocalFailure
        )
    }
    pub fn can_cancel(&self) -> bool {
        !self.cancel_pending
            && matches!(
                self.phase,
                FileTransferPhase::Preparing
                    | FileTransferPhase::Uploading
                    | FileTransferPhase::WaitingForResource
            )
    }
    pub fn is_retry(&self, thread: &str, attachment: Option<&Attachment>) -> bool {
        self.thread == thread
            && self.kind == TransferKind::File
            && self.attachment.as_ref() == attachment
            && matches!(
                self.phase,
                FileTransferPhase::Failed | FileTransferPhase::Cancelled
            )
    }
    pub fn is_photo(&self, thread: &str, photo: Option<&PreparedPhoto>) -> bool {
        self.kind == TransferKind::Photo
            && self.thread == thread
            && self.photo.is_some()
            && self.photo.as_ref() == photo
    }
    pub fn is_album(&self, thread: &str, photos: Option<&[PreparedPhoto]>) -> bool {
        self.kind == TransferKind::Album
            && self.thread == thread
            && self
                .album
                .as_ref()
                .zip(photos)
                .is_some_and(|(album, photos)| album.matches(photos))
    }
    pub fn label(&self) -> String {
        let noun = self.noun();
        if self.cancel_pending {
            return format!("Đang hủy gửi {noun}…");
        }
        match self.phase {
            FileTransferPhase::Preparing => format!("Đang kiểm tra quyền gửi và {noun}…"),
            FileTransferPhase::Uploading => format!(
                "Đang tải lên · {} / {}",
                super::attachments::size_text(self.uploaded.min(self.total)),
                super::attachments::size_text(self.total)
            ),
            FileTransferPhase::WaitingForResource => format!("Đang chờ Zalo xử lý {noun}…"),
            FileTransferPhase::Finalizing => "Đang chờ Zalo xác nhận · không gửi lại".into(),
            FileTransferPhase::Accepted => format!("Zalo đã nhận {noun}"),
            FileTransferPhase::Cancelled => format!("Đã hủy · {noun} và nội dung chữ được giữ lại"),
            FileTransferPhase::Failed => self
                .note
                .clone()
                .unwrap_or_else(|| format!("Chưa gửi được {noun} · có thể thử lại")),
            FileTransferPhase::Partial | FileTransferPhase::Unknown
                if self.kind == TransferKind::Album =>
            {
                self.note.clone().unwrap_or_else(|| self.generic_label())
            }
            FileTransferPhase::Partial => self.generic_label(),
            FileTransferPhase::Unknown => format!("Chưa rõ {noun} đã gửi chưa · không tự gửi lại"),
            FileTransferPhase::AcceptedLocalFailure => {
                format!("Zalo đã nhận {noun} · chưa lưu được xác nhận")
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Uploads {
    pub available: bool,
    pub photo_available: bool,
    pub photo_original_available: bool,
    pub voice_available: bool,
    pub album_max_count: usize,
    pub transfer: Option<Transfer>,
    dismissed: std::collections::VecDeque<(String, u64, TransferKind)>,
}
impl Uploads {
    pub fn supports_photo(&self, resolution: PhotoResolution) -> bool {
        match resolution {
            PhotoResolution::Hd => self.photo_available,
            PhotoResolution::Original => self.photo_original_available,
        }
    }
    pub fn blocked(&self) -> bool {
        self.transfer
            .as_ref()
            .is_some_and(|job| job.busy() || job.review())
    }
    pub fn submitted(&mut self, thread: &str, request: u64, attachment: Attachment) {
        self.transfer = Some(Transfer {
            kind: TransferKind::File,
            thread: thread.into(),
            request,
            phase: FileTransferPhase::Preparing,
            uploaded: 0,
            total: attachment.size_bytes,
            note: None,
            cancel_pending: false,
            attachment: Some(attachment),
            photo: None,
            album: None,
        });
    }
    pub fn submitted_photo(&mut self, thread: &str, request: u64, photo: PreparedPhoto) {
        self.transfer = Some(Transfer {
            kind: TransferKind::Photo,
            thread: thread.into(),
            request,
            phase: FileTransferPhase::Preparing,
            uploaded: 0,
            total: photo.size_bytes(),
            note: None,
            cancel_pending: false,
            attachment: None,
            photo: Some(photo),
            album: None,
        });
    }
    pub fn submitted_album(&mut self, thread: &str, request: u64, album: AlbumBinding) {
        self.transfer = Some(Transfer {
            kind: TransferKind::Album,
            thread: thread.into(),
            request,
            phase: FileTransferPhase::Preparing,
            uploaded: 0,
            total: album.size_bytes,
            note: None,
            cancel_pending: false,
            attachment: None,
            photo: None,
            album: Some(album),
        });
    }
    pub fn submitted_voice(&mut self, thread: &str, request: u64, total: u64) {
        self.transfer = Some(Transfer {
            kind: TransferKind::Voice,
            thread: thread.into(),
            request,
            phase: FileTransferPhase::Preparing,
            uploaded: 0,
            total,
            note: None,
            cancel_pending: false,
            attachment: None,
            photo: None,
            album: None,
        });
    }
    pub fn observe(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::FileCapability { available, .. } => self.available = *available,
            LiveEvent::VoiceCapability { available, .. } => self.voice_available = *available,
            LiveEvent::PhotoCapability {
                available,
                original_available,
                album_max_count,
                ..
            } => {
                self.photo_available = *available;
                self.photo_original_available = *original_available;
                self.album_max_count = *album_max_count;
            }
            LiveEvent::FileTransfer {
                kind,
                thread,
                request_id,
                phase,
                uploaded,
                total,
                message,
                ..
            } => {
                if self
                    .dismissed
                    .iter()
                    .any(|(old_thread, old_request, old_kind)| {
                        old_thread == thread && old_request == request_id && old_kind == kind
                    })
                {
                    return;
                }
                // Unknown durable outcomes may be restored by a new worker, but a
                // late request must never replace another active upload.
                if self.transfer.is_none()
                    && matches!(
                        phase,
                        FileTransferPhase::Partial
                            | FileTransferPhase::Unknown
                            | FileTransferPhase::AcceptedLocalFailure
                    )
                {
                    self.transfer = Some(Transfer {
                        kind: *kind,
                        thread: thread.clone(),
                        request: *request_id,
                        phase: *phase,
                        uploaded: *uploaded,
                        total: *total,
                        note: message.clone(),
                        cancel_pending: false,
                        attachment: None,
                        photo: None,
                        album: None,
                    });
                }
                if let Some(job) = self.transfer.as_mut().filter(|job| {
                    job.thread == *thread && job.request == *request_id && job.kind == *kind
                }) {
                    if !job.busy()
                        && *phase != FileTransferPhase::Accepted
                        && !(job.phase == FileTransferPhase::Unknown
                            && *phase == FileTransferPhase::AcceptedLocalFailure)
                    {
                        return;
                    }
                    if matches!((Transfer::stage(job.phase), Transfer::stage(*phase)), (Some(old), Some(new)) if new < old)
                    {
                        return;
                    }
                    job.phase = *phase;
                    job.uploaded = job.uploaded.max(*uploaded).min(*total);
                    job.total = *total;
                    job.note = message.clone();
                    if !job.busy() {
                        job.cancel_pending = false;
                    }
                }
            }
            LiveEvent::FileAccepted {
                kind,
                account_id,
                thread,
                request_id,
                message,
                conversation,
            } if *kind != TransferKind::Album
                && message.account_id == *account_id
                && message.thread_id == *thread
                && conversation.id == *thread =>
            {
                if let Some(job) = self.transfer.as_mut().filter(|job| {
                    job.thread == *thread && job.request == *request_id && job.kind == *kind
                }) {
                    job.phase = FileTransferPhase::Accepted;
                    job.cancel_pending = false;
                }
            }
            _ => {}
        }
    }
    pub fn worker_closed(&mut self) {
        self.available = false;
        self.photo_available = false;
        self.photo_original_available = false;
        self.voice_available = false;
        self.album_max_count = 0;
        if let Some(job) = self.transfer.as_mut().filter(|job| job.busy()) {
            job.phase = FileTransferPhase::Unknown;
            job.note = None;
            job.cancel_pending = false;
        }
    }
    fn dismiss_review(&mut self) -> Option<Transfer> {
        if !self.transfer.as_ref().is_some_and(|job| job.review()) {
            return None;
        }
        let job = self.transfer.take()?;
        if self.dismissed.len() == 64 {
            self.dismissed.pop_front();
        }
        self.dismissed
            .push_back((job.thread.clone(), job.request, job.kind));
        Some(job)
    }
}

impl NativeApp {
    pub(super) fn send_live_attachment(&mut self) {
        if !self.file_can_send() || self.model.pending {
            self.model.status =
                "Chưa thể gửi tệp. Kiểm tra kết nối và lượt gửi trước; bản nháp được giữ lại."
                    .into();
            return;
        }
        let Some(thread) = self.model.selected.clone() else {
            return;
        };
        let Some(attachment) = self.drafts.active.attachment().cloned() else {
            return;
        };
        let Some(state) = self.live.as_mut() else {
            return;
        };
        let Some(worker) = &state.worker else {
            return;
        };
        let mut request = None;
        match self
            .drafts
            .active
            .send_live_file(self.model.generation, thread.clone(), |command| {
                let id = match &command {
                    LiveCommand::SendFile { request_id, .. } => Some(*request_id),
                    _ => None,
                };
                worker.submit(command)?;
                request = id;
                Ok(())
            }) {
            Ok(()) => {
                if let Some(request) = request {
                    state.uploads.submitted(&thread, request, attachment);
                }
            }
            Err(error) => self.model.status = error.to_string(),
        }
        self.focus_composer = true;
    }
    pub(super) fn file_can_choose(&self) -> bool {
        self.drafts.active.reply().is_none()
            && !self.has_draft_mentions()
            && !self.photo_selected()
            && self.voice_tray_height() == 0.0
            && (self.demo
                || self
                    .live
                    .as_ref()
                    .is_some_and(|state| state.uploads.available && !state.uploads.blocked()))
    }
    pub(super) fn file_can_send(&self) -> bool {
        self.file_can_choose() && self.live_can_send()
    }
    pub(super) fn file_upload_visible(&self) -> bool {
        self.live
            .as_ref()
            .and_then(|state| state.uploads.transfer.as_ref())
            .is_some_and(|job| {
                (job.busy() || job.review())
                    && !self.voice_tray_shows(job)
                    && !self.model.selected.as_deref().is_some_and(|thread| {
                        (job.is_photo(thread, self.drafts.active.photo())
                            || job.is_album(thread, self.drafts.active.album()))
                            && self.privacy_visible(thread)
                    })
            })
    }
    pub(super) fn file_send_label(&self) -> &'static str {
        if self
            .live
            .as_ref()
            .and_then(|state| state.uploads.transfer.as_ref())
            .is_some_and(|job| {
                self.model
                    .selected
                    .as_deref()
                    .is_some_and(|thread| job.is_retry(thread, self.drafts.active.attachment()))
            })
        {
            "Thử lại"
        } else {
            "Gửi tệp"
        }
    }
    pub(super) fn cancel_file_upload(&mut self) {
        let Some(state) = self.live.as_mut() else {
            return;
        };
        let Some(job) = state
            .uploads
            .transfer
            .as_ref()
            .filter(|job| job.can_cancel())
        else {
            return;
        };
        let request = job.request;
        let Some(worker) = &state.worker else {
            return;
        };
        let command = match job.kind {
            TransferKind::Album => LiveCommand::CancelAlbum {
                thread: job.thread.clone(),
                request_id: request,
            },
            TransferKind::Voice => LiveCommand::CancelVoice {
                thread: job.thread.clone(),
                request_id: request,
            },
            _ => LiveCommand::CancelFile {
                request_id: request,
            },
        };
        match worker.submit(command) {
            Ok(()) => {
                if let Some(job) = state
                    .uploads
                    .transfer
                    .as_mut()
                    .filter(|job| job.request == request)
                {
                    job.cancel_pending = true;
                }
            }
            Err(_) => {
                self.model.status =
                    "Chưa gửi được yêu cầu hủy lượt gửi. Bản nháp được giữ lại.".into()
            }
        }
    }
    pub(super) fn dismiss_file_review(&mut self) {
        let dismissed = self
            .live
            .as_mut()
            .and_then(|state| state.uploads.dismiss_review());
        if let Some(job) = dismissed {
            if job.kind == TransferKind::Voice {
                if let Some(state) = &mut self.live {
                    state.voice.clear_receipt(&job.thread, job.request);
                    state.media.voice.clear_recording();
                }
            }
            if self.model.selected.as_deref() == Some(job.thread.as_str()) {
                if job
                    .attachment
                    .as_ref()
                    .is_some_and(|file| self.drafts.active.attachment() == Some(file))
                {
                    self.drafts.active.clear_attachment();
                }
                if job
                    .photo
                    .as_ref()
                    .is_some_and(|photo| self.drafts.active.photo() == Some(photo))
                {
                    self.drafts.active.clear_photo();
                    if let Some(state) = self.live.as_mut() {
                        state.photo_preview.clear();
                    }
                }
                if job.is_album(&job.thread, self.drafts.active.album()) {
                    self.drafts.active.clear_album();
                    if let Some(state) = self.live.as_mut() {
                        state.photo_preview.clear();
                    }
                }
            }
        }
        self.focus_composer = true;
    }
    /// Also reachable from the avatar on compact-list/navigation screens. Never
    /// include the source thread, file name or path outside its visible pane.
    pub(super) fn file_upload_controls(&mut self, ui: &mut egui::Ui) {
        let Some(job) = self
            .live
            .as_ref()
            .and_then(|state| state.uploads.transfer.as_ref())
        else {
            return;
        };
        if !job.busy() && !job.review() {
            return;
        }
        let label = job.generic_label();
        let can_cancel = job.can_cancel();
        let review = job.review();
        let noun = job.noun();
        ui.separator();
        ui.add(egui::Label::new(RichText::new(format!("Gửi {noun}")).strong()).wrap());
        ui.add(egui::Label::new(label).wrap());
        if can_cancel
            && ui
                .add_sized(
                    vec2(ui.available_width(), 44.0),
                    egui::Button::new(format!("Hủy gửi {noun}")),
                )
                .clicked()
        {
            self.cancel_file_upload();
        }
        if review {
            ui.add(
                egui::Label::new(
                    "Đóng cảnh báo không gửi lại nội dung. Lượt gửi cũ vẫn được giữ để tránh gửi trùng.",
                )
                .wrap(),
            );
            if ui
                .add_sized(
                    vec2(ui.available_width(), 44.0),
                    egui::Button::new(format!("Đóng cảnh báo gửi {noun}")),
                )
                .clicked()
            {
                self.dismiss_file_review();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Files(std::path::PathBuf);
    impl Files {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "zalo-file-upload-ui-{}",
                crate::domain::local_message_id().unwrap()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn attachment(&self, name: &str) -> Attachment {
            let path = self.0.join(name);
            std::fs::write(&path, [7u8; 100]).unwrap();
            Attachment::inspect(&path).unwrap()
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn progress(thread: &str, request: u64, phase: FileTransferPhase) -> LiveEvent {
        LiveEvent::FileTransfer {
            kind: crate::app::live::TransferKind::File,
            account_id: "11".into(),
            thread: thread.into(),
            request_id: request,
            phase,
            uploaded: 50,
            total: 100,
            message: None,
        }
    }
    #[test]
    fn unknown_review_dismissal_allows_another_file_without_reopening_old_events() {
        let files = Files::new();
        let file_a = files.attachment("A.pdf");
        let mut uploads = Uploads {
            available: true,
            ..Default::default()
        };
        uploads.submitted("d:22", 1, file_a.clone());
        uploads.observe(&progress("d:22", 1, FileTransferPhase::Unknown));
        assert!(uploads.blocked());
        let dismissed = uploads.dismiss_review().unwrap();
        assert_eq!(dismissed.thread, "d:22");
        assert_eq!(dismissed.attachment, Some(file_a));
        assert!(!uploads.blocked());
        assert!(uploads.available);
        uploads.submitted("d:22", 2, files.attachment("B.pdf"));
        uploads.observe(&progress("d:22", 1, FileTransferPhase::Unknown));
        assert_eq!(uploads.transfer.as_ref().unwrap().request, 2);
        assert_eq!(uploads.transfer.as_ref().unwrap().filename(), Some("B.pdf"));
        assert_eq!(
            uploads.transfer.as_ref().unwrap().phase,
            FileTransferPhase::Preparing
        );
    }
    #[test]
    fn stale_progress_cannot_change_another_transfer_or_reenable_cancel_after_finalizing() {
        let files = Files::new();
        let mut uploads = Uploads::default();
        uploads.submitted("d:22", 7, files.attachment("A.pdf"));
        uploads.observe(&progress("g:22", 7, FileTransferPhase::Uploading));
        assert_eq!(
            uploads.transfer.as_ref().unwrap().phase,
            FileTransferPhase::Preparing
        );
        uploads.observe(&progress("d:22", 7, FileTransferPhase::Finalizing));
        uploads.observe(&progress("d:22", 7, FileTransferPhase::Uploading));
        let job = uploads.transfer.as_ref().unwrap();
        assert_eq!(job.phase, FileTransferPhase::Finalizing);
        assert!(!job.can_cancel());
        uploads.observe(&progress("d:22", 8, FileTransferPhase::Failed));
        assert_eq!(uploads.transfer.as_ref().unwrap().request, 7);
        uploads.observe(&progress("d:22", 7, FileTransferPhase::Accepted));
        uploads.observe(&progress("d:22", 7, FileTransferPhase::Failed));
        assert_eq!(
            uploads.transfer.as_ref().unwrap().phase,
            FileTransferPhase::Accepted
        );
        assert!(!uploads.blocked());
    }
    #[test]
    fn dismissing_unknown_a_allows_b_and_a_late_ack_cannot_clear_b() {
        let files = Files::new();
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        app.model.selected = Some("d:22".into());
        app.drafts.select("d:22").unwrap();
        app.drafts.active.text = "Chữ được giữ lại".into();
        let file_a = files.attachment("A.pdf");
        app.drafts.active.set_attachment(file_a.clone());
        app.drafts
            .active
            .send_live_file(1, "d:22".into(), |_| Ok(()))
            .unwrap();
        let state = app.live.as_mut().unwrap();
        state.uploads.available = true;
        state.uploads.submitted("d:22", 1, file_a);
        let unknown = progress("d:22", 1, FileTransferPhase::Unknown);
        state.uploads.observe(&unknown);
        app.drafts.apply_live(&unknown, "11");
        assert!(!app.file_can_choose());
        app.dismiss_file_review();
        assert!(app.file_can_choose());
        assert!(app.drafts.active.attachment().is_none());
        let file_b = files.attachment("B.pdf");
        app.drafts.active.set_attachment(file_b.clone());
        app.drafts
            .active
            .send_live_file(1, "d:22".into(), |_| Ok(()))
            .unwrap();
        app.live
            .as_mut()
            .unwrap()
            .uploads
            .submitted("d:22", 2, file_b.clone());
        let accepted_a = LiveEvent::FileAccepted {
            kind: crate::app::live::TransferKind::File,
            account_id: "11".into(),
            thread: "d:22".into(),
            request_id: 1,
            message: crate::domain::Message {
                account_id: "11".into(),
                thread_id: "d:22".into(),
                id: "9001".into(),
                sender: "11".into(),
                text: "A.pdf".into(),
                source_client_id: Some("9002".into()),
                mentions: Vec::new(),
                reply_blocked: true,
                expires_at_ms: None,
                recalled: false,
                quote: None,
                sticker: None,
                media: Some(crate::domain::MediaDescriptor::File(
                    crate::domain::FileDescriptor {
                        title: "A.pdf".into(),
                        extension: "pdf".into(),
                        size_bytes: Some(100),
                        url: None,
                        encrypted: false,
                        is_folder: false,
                    },
                )),
                sent_at_ms: 1_700_000_000_000,
            },
            conversation: crate::domain::Conversation {
                id: "d:22".into(),
                title: "Liên hệ tổng hợp".into(),
                last_message_at_ms: 1_700_000_000_000,
                last_text: "A.pdf".into(),
                last_sender: "11".into(),
                group_left: false,
            },
        };
        app.drafts.apply_live(&accepted_a, "11");
        app.live.as_mut().unwrap().uploads.observe(&accepted_a);
        assert_eq!(app.drafts.active.attachment(), Some(&file_b));
        assert!(app.drafts.active.in_flight());
        assert_eq!(app.drafts.active.text, "Chữ được giữ lại");
        assert_eq!(
            app.live
                .as_ref()
                .unwrap()
                .uploads
                .transfer
                .as_ref()
                .unwrap()
                .request,
            2
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn redacted_accepted_proof_settles_progress_without_publishing_a_message() {
        let files = Files::new();
        let mut uploads = Uploads::default();
        uploads.submitted("d:22", 7, files.attachment("A.pdf"));
        uploads.observe(&progress("d:22", 7, FileTransferPhase::Finalizing));
        uploads.worker_closed();
        assert!(uploads.blocked());
        uploads.observe(&LiveEvent::FileTransfer {
            kind: crate::app::live::TransferKind::File,
            account_id: "11".into(),
            thread: "d:22".into(),
            request_id: 7,
            phase: FileTransferPhase::Accepted,
            uploaded: 0,
            total: 0,
            message: None,
        });
        let job = uploads.transfer.as_ref().unwrap();
        assert_eq!(job.phase, FileTransferPhase::Accepted);
        assert!(!job.busy() && !job.review());
        assert!(!uploads.blocked());
        assert_eq!((job.uploaded, job.total), (0, 0));
        assert!(job.note.is_none());
    }

    #[test]
    fn navigation_controls_show_no_file_name_path_or_size() {
        let files = Files::new();
        let ctx = egui::Context::default();
        let (mut app, root) = super::super::live::tests::fixture(&ctx);
        let state = app.live.as_mut().unwrap();
        state
            .uploads
            .submitted("d:22", 7, files.attachment("Private-project-name.pdf"));
        state
            .uploads
            .observe(&progress("d:22", 7, FileTransferPhase::Uploading));
        app.model.selected = Some("g:90".into());
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.file_upload_controls(ui));
        });
        let text: String = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Hủy gửi tệp"));
        assert!(!text.contains("Private-project-name"));
        assert!(!text.contains("/tmp"));
        assert!(!text.contains("100"));
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
