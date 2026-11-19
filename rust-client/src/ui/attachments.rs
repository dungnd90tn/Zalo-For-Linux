//! Choosing a file to attach: the desktop's file dialog and drag-and-drop. The dialog is an
//! external program (`zenity` or `kdialog`) and the file is only inspected, never read, so
//! everything here runs on a short-lived background thread and reports back through a channel.
use super::{visuals, NativeApp};
use crate::domain::Attachment;
use eframe::egui::{self, pos2, vec2, FontId, Sense, Stroke};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
};

/// What a finished choice delivered.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Picked {
    File(Attachment),
    Cancelled,
    Failed(String),
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PickerKind {
    #[default]
    File,
    Photo,
}

#[derive(Default)]
pub(super) struct AttachmentPicker {
    kind: PickerKind,
    pending: Option<mpsc::Receiver<Picked>>,
    /// The conversation the choice was started in; a result for another one is dropped.
    for_thread: Option<String>,
    for_account: Option<String>,
    for_auth_generation: Option<u64>,
    /// The last problem, shown in the attachment tray until the user dismisses it or chooses again.
    pub notice: Option<String>,
}

impl AttachmentPicker {
    pub fn open_scoped_dialog(
        &mut self,
        ctx: &egui::Context,
        thread: Option<&str>,
        account: &str,
        auth_generation: Option<u64>,
    ) {
        if self.busy() {
            return;
        }
        self.for_account = Some(account.into());
        self.for_auth_generation = auth_generation;
        self.kind = PickerKind::File;
        self.open_dialog(ctx, thread);
    }
    #[cfg(feature = "messaging")]
    pub fn open_photo_dialog(
        &mut self,
        ctx: &egui::Context,
        thread: Option<&str>,
        account: &str,
        auth_generation: Option<u64>,
    ) {
        if self.busy() {
            return;
        }
        self.for_account = Some(account.into());
        self.for_auth_generation = auth_generation;
        self.kind = PickerKind::Photo;
        self.open_dialog(ctx, thread);
    }
    fn photo_scope(
        &self,
        account: &str,
        auth_generation: Option<u64>,
        thread: Option<&str>,
    ) -> bool {
        self.kind == PickerKind::Photo
            && self.for_account.as_deref() == Some(account)
            && self.for_auth_generation == auth_generation
            && self.for_thread.as_deref() == thread
    }
    #[cfg(feature = "messaging")]
    pub fn invalidate_photo_choice(&mut self) {
        if self.kind == PickerKind::Photo {
            self.notice = None;
            self.for_thread = None;
            self.for_account = None;
        }
    }
    #[cfg(feature = "messaging")]
    fn inspect_photo_scoped(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        thread: Option<&str>,
        account: &str,
        auth_generation: Option<u64>,
    ) {
        if self.busy() {
            return;
        }
        self.for_account = Some(account.into());
        self.for_auth_generation = auth_generation;
        self.kind = PickerKind::Photo;
        self.inspect(ctx, path, thread);
    }
    fn inspect_scoped(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        thread: Option<&str>,
        account: &str,
        auth_generation: Option<u64>,
    ) {
        if self.busy() {
            return;
        }
        self.for_account = Some(account.into());
        self.for_auth_generation = auth_generation;
        self.kind = PickerKind::File;
        self.inspect(ctx, path, thread);
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    /// Open the desktop file dialog. Does nothing while a choice is already in progress.
    pub fn open_dialog(&mut self, ctx: &egui::Context, thread: Option<&str>) {
        if self.busy() {
            return;
        }
        self.notice = None;
        self.for_thread = thread.map(str::to_owned);
        let Some((program, mut args)) = dialog_command() else {
            self.notice = Some(
                "Không tìm thấy zenity hoặc kdialog để mở hộp thoại. Hãy kéo thả tệp vào cửa sổ."
                    .into(),
            );
            return;
        };
        let kind = self.kind;
        if kind == PickerKind::Photo {
            for arg in &mut args {
                if arg == "--title=Chọn tệp để gửi" {
                    *arg = "--title=Chọn ảnh HD".into();
                }
            }
            if program.file_name().is_some_and(|name| name == "zenity") {
                args.push("--file-filter=Ảnh JPEG, PNG, WebP | *.jpg *.jpeg *.png *.webp *.JPG *.JPEG *.PNG *.WEBP".into());
            } else {
                args.push("*.jpg *.jpeg *.png *.webp|Ảnh JPEG, PNG, WebP".into());
            }
        }
        self.spawn(ctx, move || {
            if kind == PickerKind::Photo {
                run_dialog_kind(&program, &args, kind)
            } else {
                run_dialog(&program, &args)
            }
        });
    }
    /// Inspect a path that arrived by drag-and-drop.
    pub fn inspect(&mut self, ctx: &egui::Context, path: PathBuf, thread: Option<&str>) {
        if self.busy() {
            return;
        }
        self.notice = None;
        self.for_thread = thread.map(str::to_owned);
        let kind = self.kind;
        self.spawn(ctx, move || inspect_path_kind(&path, kind));
    }
    fn spawn(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Picked + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("zalo-attachment-picker".into())
            .spawn(move || {
                let _ = sender.send(work());
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.pending = Some(receiver),
            Err(_) => self.notice = Some("Chưa mở được hộp chọn tệp.".into()),
        }
    }
    /// The finished choice, once, without blocking.
    pub fn poll(&mut self) -> Option<Picked> {
        let receiver = self.pending.as_ref()?;
        match receiver.try_recv() {
            Ok(picked) => {
                self.pending = None;
                Some(picked)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Picked::Failed("Hộp chọn tệp đã dừng đột ngột.".into()))
            }
        }
    }
}

fn inspect_path(path: &Path) -> Picked {
    match Attachment::inspect(path) {
        Ok(attachment) => Picked::File(attachment),
        Err(error) => Picked::Failed(error.to_string()),
    }
}
fn inspect_path_kind(path: &Path, kind: PickerKind) -> Picked {
    let picked = inspect_path(path);
    if kind == PickerKind::Photo {
        if let Picked::File(file) = &picked {
            if file.size_bytes > 20 * 1024 * 1024 {
                return Picked::Failed("Ảnh nguồn lớn hơn 20 MiB; hãy chọn ảnh nhỏ hơn.".into());
            }
            if !matches!(file.extension.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                return Picked::Failed("Ảnh HD chỉ nhận JPEG, PNG hoặc WebP tĩnh. Dùng Đính kèm tệp nếu muốn gửi định dạng khác.".into());
            }
        }
    }
    picked
}

/// The first dialog program found on `PATH` with its arguments, preferring the GNOME one.
fn dialog_command() -> Option<(PathBuf, Vec<OsString>)> {
    let path = std::env::var_os("PATH")?;
    dialog_command_in(&path)
}
fn dialog_command_in(path: &std::ffi::OsStr) -> Option<(PathBuf, Vec<OsString>)> {
    let find = |name: &str| {
        std::env::split_paths(path)
            .map(|dir| dir.join(name))
            .find(|candidate| is_executable(candidate))
    };
    if let Some(zenity) = find("zenity") {
        return Some((
            zenity,
            vec!["--file-selection".into(), "--title=Chọn tệp để gửi".into()],
        ));
    }
    find("kdialog").map(|kdialog| {
        let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
        (
            kdialog,
            vec![
                "--title".into(),
                "Chọn tệp để gửi".into(),
                "--getopenfilename".into(),
                home,
            ],
        )
    })
}
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Run a dialog program and turn its answer into a choice. Exit status 1 is the dialogs'
/// convention for "cancelled"; any other failure is reported, never guessed at.
fn run_dialog(program: &Path, args: &[OsString]) -> Picked {
    run_dialog_kind(program, args, PickerKind::File)
}
fn run_dialog_kind(program: &Path, args: &[OsString], kind: PickerKind) -> Picked {
    let output = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(_) => return Picked::Failed("Không mở được hộp thoại chọn tệp.".into()),
    };
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Picked::Cancelled,
        _ => return Picked::Failed("Hộp thoại chọn tệp báo lỗi.".into()),
    }
    let chosen = String::from_utf8_lossy(&output.stdout);
    // A dialog may allow several files joined by `|`; only the first is attached.
    let chosen = chosen.trim_end_matches(['\n', '\r']);
    let first = chosen.split('|').next().unwrap_or("");
    if first.is_empty() {
        return Picked::Cancelled;
    }
    inspect_path_kind(Path::new(first), kind)
}

/// Height of the tray above the composer's toolbar while a file, a problem or a check is shown.
pub(super) const TRAY_HEIGHT: f32 = 56.0;

pub(super) fn size_text(bytes: u64) -> String {
    super::information::size_label(bytes)
}

impl NativeApp {
    pub(super) fn photo_picker_pending(&self) -> bool {
        self.attachments.photo_scope(
            self.account_id().unwrap_or(crate::app::DEMO_ACCOUNT),
            self.auth_generation(),
            self.model.selected.as_deref(),
        ) && (self.attachments.busy() || self.attachments.notice.is_some())
    }
    pub(super) fn photo_selected(&self) -> bool {
        self.drafts.active.has_photo_intent() || self.photo_picker_pending()
    }
    #[cfg(feature = "messaging")]
    pub(super) fn photo_picker_notice(&self) -> Option<&str> {
        self.photo_picker_pending()
            .then_some(self.attachments.notice.as_deref())
            .flatten()
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn photo_can_choose(&self) -> bool {
        false
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn photo_can_send(&self) -> bool {
        false
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn photo_tray_height(&self) -> f32 {
        0.0
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn photo_send_label(&self) -> &'static str {
        "Gửi ảnh"
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn file_can_choose(&self) -> bool {
        self.demo && self.drafts.active.reply().is_none() && !self.photo_selected()
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn file_can_send(&self) -> bool {
        false
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn file_upload_visible(&self) -> bool {
        false
    }
    #[cfg(not(feature = "messaging"))]
    pub(super) fn file_send_label(&self) -> &'static str {
        "Gửi tệp"
    }
    pub(super) fn attachment_tray_height(&self) -> f32 {
        if self.drafts.active.attachment().is_some()
            || (self.attachments.kind == PickerKind::File
                && (self.attachments.busy() || self.attachments.notice.is_some()))
            || self.file_upload_visible()
        {
            TRAY_HEIGHT
        } else {
            0.0
        }
    }

    /// Collect a finished file choice and any file dropped on the window. `accepting` is false
    /// when files cannot be sent from here; a drop then explains why instead of doing nothing.
    pub(super) fn poll_attachments(&mut self, ctx: &egui::Context, accepting: bool, reason: &str) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if let Some(first) = dropped.first().filter(|_| !self.attachments.busy()) {
            let thread = self.model.selected.clone();
            let account = self
                .account_id()
                .unwrap_or(crate::app::DEMO_ACCOUNT)
                .to_owned();
            let auth_generation = self.auth_generation();
            let extension = first
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let image = matches!(
                extension.as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "jxl" | "bmp" | "avif" | "heic"
            );
            let photo_drop = !self.demo && image && !ctx.input(|input| input.modifiers.shift);
            #[cfg(feature = "messaging")]
            if photo_drop {
                self.drafts.active.edited();
                if self.photo_can_choose() {
                    self.attachments.inspect_photo_scoped(
                        ctx,
                        first.clone(),
                        thread.as_deref(),
                        &account,
                        auth_generation,
                    );
                } else {
                    self.attachments.kind = PickerKind::Photo;
                    self.attachments.for_thread = thread.clone();
                    self.attachments.for_account = Some(account.clone());
                    self.attachments.for_auth_generation = auth_generation;
                    self.attachments.notice = Some(
                        "Chưa thể chọn ảnh HD. Bỏ tệp hoặc trích dẫn và kiểm tra kết nối.".into(),
                    );
                }
            }
            if !photo_drop {
                if !accepting {
                    self.attachments.notice = Some(reason.into());
                } else {
                    self.attachments.inspect_scoped(
                        ctx,
                        first.clone(),
                        thread.as_deref(),
                        &account,
                        auth_generation,
                    );
                }
            }
            if dropped.len() > 1 {
                self.attachments.notice =
                    Some("Mỗi lần chỉ chọn một mục; đã chọn mục đầu tiên.".into());
            }
        }
        match self.attachments.poll() {
            Some(Picked::File(attachment)) => {
                let account = self.account_id().unwrap_or(crate::app::DEMO_ACCOUNT);
                let scoped = self.attachments.for_thread.as_deref()
                    == self.model.selected.as_deref()
                    && self.attachments.for_account.as_deref() == Some(account)
                    && self.attachments.for_auth_generation == self.auth_generation();
                if scoped && self.attachments.kind == PickerKind::Photo {
                    self.attachments.notice = None;
                    #[cfg(feature = "messaging")]
                    self.begin_photo_preparation(attachment);
                } else if scoped
                    && accepting
                    && self.drafts.active.reply().is_none()
                    && !self.photo_selected()
                {
                    self.drafts.active.set_attachment(attachment);
                } else {
                    self.attachments.notice = Some(
                        "Đã đổi phiên hoặc hội thoại trước khi chọn xong; hãy chọn lại tệp.".into(),
                    );
                }
            }
            Some(Picked::Cancelled) => self.attachments.notice = None,
            Some(Picked::Failed(message)) => self.attachments.notice = Some(message),
            None => {}
        }
    }

    /// The tray: the chosen file with a remove button, a check in progress, or a problem.
    pub(super) fn attachment_tray(&mut self, ui: &mut egui::Ui, card: egui::Rect) {
        let colors = crate::theme::palette(ui.ctx());
        let tray = egui::Rect::from_min_size(card.min, vec2(card.width(), TRAY_HEIGHT));
        ui.painter().line_segment(
            [
                tray.left_bottom() + vec2(12.0, 0.0),
                tray.right_bottom() - vec2(12.0, 0.0),
            ],
            Stroke::new(1.0_f32, colors.border),
        );
        let badge = egui::Rect::from_min_size(tray.min + vec2(14.0, 8.0), vec2(40.0, 40.0));
        let close = egui::Rect::from_center_size(
            pos2(tray.right() - 30.0, tray.center().y),
            vec2(44.0, 44.0),
        );
        let text_left = badge.right() + 12.0;
        let text_width = (close.left() - text_left - 8.0).max(0.0);
        #[cfg(feature = "messaging")]
        let transfer =
            self.live
                .as_ref()
                .and_then(|state| state.uploads.transfer.as_ref())
                .filter(|job| {
                    job.busy()
                        || job.review()
                        || self.model.selected.as_deref().is_some_and(|thread| {
                            job.is_retry(thread, self.drafts.active.attachment())
                        })
                })
                .map(|job| {
                    let visible = self.model.selected.as_deref() == Some(job.thread.as_str())
                        && self.privacy_visible(&job.thread);
                    let title = if visible {
                        job.filename().unwrap_or("Lượt gửi tệp").to_owned()
                    } else {
                        "Đang xử lý một lượt gửi tệp".into()
                    };
                    let subtitle = if visible {
                        job.label()
                    } else {
                        job.generic_label()
                    };
                    let progress = (visible && job.busy() && job.total > 0)
                        .then(|| (job.uploaded as f64 / job.total as f64).clamp(0.0, 1.0) as f32);
                    let dismiss = if job.can_cancel() {
                        Dismiss::TransferCancel
                    } else if job.review() {
                        Dismiss::TransferReview
                    } else if visible && !job.busy() && !job.review() {
                        Dismiss::Attachment
                    } else {
                        Dismiss::None
                    };
                    (title, subtitle, dismiss, progress)
                });
        #[cfg(not(feature = "messaging"))]
        let transfer: Option<(String, String, Dismiss, Option<f32>)> = None;
        let progress = transfer.as_ref().and_then(|(_, _, _, progress)| *progress);
        let (title, subtitle, glyph, dismiss) =
            if let Some((title, subtitle, dismiss, _)) = transfer {
                (title, subtitle, "fa-chatbar-file", dismiss)
            } else if let Some(file) = self.drafts.active.attachment() {
                (
                    file.name.clone(),
                    if file.extension.is_empty() {
                        size_text(file.size_bytes)
                    } else {
                        format!(
                            "{} · {}",
                            file.extension.to_uppercase(),
                            size_text(file.size_bytes)
                        )
                    },
                    "fa-chatbar-file",
                    Dismiss::Attachment,
                )
            } else if self.attachments.busy() {
                (
                    "Đang kiểm tra tệp…".to_owned(),
                    String::new(),
                    "fa-chatbar-file",
                    Dismiss::None,
                )
            } else {
                (
                    self.attachments.notice.clone().unwrap_or_default(),
                    String::new(),
                    "fa-warning-lock-send-message",
                    Dismiss::Notice,
                )
            };
        let problem = matches!(dismiss, Dismiss::Notice);
        ui.painter().rect_filled(
            badge,
            10.0,
            if problem {
                colors.hover
            } else {
                colors.accent_soft
            },
        );
        visuals::source_icon(
            ui,
            egui::Rect::from_center_size(badge.center(), vec2(22.0, 22.0)),
            glyph,
            if problem {
                colors.error
            } else {
                colors.accent_ink
            },
        );
        super::row_text(
            ui,
            pos2(
                text_left,
                tray.top() + if subtitle.is_empty() { 19.0 } else { 11.0 },
            ),
            text_width,
            &title,
            FontId::new(
                visuals::TEXT_BODY,
                egui::FontFamily::Name("Zalo Semibold".into()),
            ),
            if problem {
                colors.error
            } else {
                colors.text_primary
            },
        );
        if !subtitle.is_empty() {
            super::row_text(
                ui,
                pos2(text_left, tray.top() + 33.0),
                text_width,
                &subtitle,
                FontId::proportional(visuals::TEXT_SMALL),
                colors.text_secondary,
            );
        }
        if let Some(progress) = progress {
            let line = egui::Rect::from_min_size(
                tray.left_bottom() + vec2(12.0, -3.0),
                vec2((tray.width() - 24.0).max(0.0) * progress, 3.0),
            );
            ui.painter().rect_filled(line, 1.5, colors.primary);
        }
        if dismiss == Dismiss::None {
            return;
        }
        let response = ui.interact(close, ui.id().with("attachment_dismiss"), Sense::click());
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.painter().rect_filled(close, 10.0, colors.hover);
        }
        visuals::source_icon(ui, close.shrink(10.0), "fa-close", colors.text_secondary);
        visuals::focus_outline(ui, &response, close, 10.0);
        let label = if dismiss == Dismiss::TransferReview {
            "Đóng cảnh báo và bỏ tệp khỏi bản nháp đang mở · không gửi lại lượt cũ"
        } else if dismiss == Dismiss::TransferCancel {
            "Hủy gửi tệp · giữ tệp và nội dung đang soạn"
        } else if dismiss == Dismiss::Attachment {
            "Bỏ tệp đính kèm"
        } else {
            "Đóng thông báo"
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        if response.clicked() {
            match dismiss {
                #[cfg(feature = "messaging")]
                Dismiss::TransferCancel => self.cancel_file_upload(),
                #[cfg(feature = "messaging")]
                Dismiss::TransferReview => self.dismiss_file_review(),
                Dismiss::Attachment => self.drafts.active.clear_attachment(),
                _ => self.attachments.notice = None,
            }
            self.focus_composer = true;
        }
        response.on_hover_text(label);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dismiss {
    None,
    Attachment,
    Notice,
    TransferCancel,
    TransferReview,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zalo-picker-{tag}-{}",
            crate::domain::local_message_id().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "#!/bin/sh\n{body}").unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn zenity_is_preferred_then_kdialog_and_nothing_is_a_clear_absence() {
        let dir = temp_dir("which");
        assert!(dialog_command_in(dir.as_os_str()).is_none());
        let kdialog = script(&dir, "kdialog", "exit 0");
        let (program, args) = dialog_command_in(dir.as_os_str()).unwrap();
        assert_eq!(program, kdialog);
        assert!(args.iter().any(|arg| arg == "--getopenfilename"));
        let zenity = script(&dir, "zenity", "exit 0");
        let (program, args) = dialog_command_in(dir.as_os_str()).unwrap();
        assert_eq!(program, zenity);
        assert_eq!(args[0], "--file-selection");
        // A file that is not executable is not a dialog program.
        std::fs::set_permissions(&zenity, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(dialog_command_in(dir.as_os_str()).unwrap().0, kdialog);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dialog_answer_becomes_an_inspected_attachment() {
        let dir = temp_dir("answer");
        let file = dir.join("bao-cao.txt");
        std::fs::write(&file, b"noi dung").unwrap();
        let chosen = script(&dir, "dialog", &format!("echo '{}'", file.display()));
        match run_dialog(&chosen, &[]) {
            Picked::File(attachment) => {
                assert_eq!(attachment.name, "bao-cao.txt");
                assert_eq!(attachment.size_bytes, 8);
            }
            other => panic!("{other:?}"),
        }
        // Several answers joined by '|': only the first is used.
        let multi = script(
            &dir,
            "multi",
            &format!("echo '{}|/does/not/matter'", file.display()),
        );
        let picked = run_dialog(&multi, &[]);
        assert!(
            matches!(&picked, Picked::File(_)),
            "multi-answer chooser returned {picked:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelling_failing_and_bad_answers_are_told_apart() {
        let dir = temp_dir("fail");
        assert_eq!(
            run_dialog(&script(&dir, "cancel", "exit 1"), &[]),
            Picked::Cancelled
        );
        assert!(matches!(
            run_dialog(&script(&dir, "crash", "exit 5"), &[]),
            Picked::Failed(_)
        ));
        assert_eq!(
            run_dialog(&script(&dir, "silent", "exit 0"), &[]),
            Picked::Cancelled,
            "an empty answer attaches nothing"
        );
        match run_dialog(&script(&dir, "gone", "echo /no/such/file.txt"), &[]) {
            Picked::Failed(message) => assert!(message.contains("Không tìm thấy")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            run_dialog(&dir.join("not-a-program"), &[]),
            Picked::Failed(_)
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dropped_path_is_inspected_off_thread_and_reported_once() {
        let dir = temp_dir("drop");
        let file = dir.join("anh.png");
        std::fs::write(&file, [1u8; 10]).unwrap();
        let ctx = egui::Context::default();
        let mut picker = AttachmentPicker::default();
        assert!(!picker.busy());
        picker.inspect(&ctx, file.clone(), Some("d:1"));
        assert!(picker.busy(), "one choice at a time");
        picker.inspect(&ctx, dir.join("other"), Some("d:1"));
        let picked = loop {
            if let Some(picked) = picker.poll() {
                break picked;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert!(matches!(&picked, Picked::File(a) if a.name == "anh.png" && a.extension == "png"));
        assert!(!picker.busy());
        assert!(picker.poll().is_none(), "delivered exactly once");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(test, feature = "messaging"))]
mod flow_tests {
    use super::*;
    use crate::domain::Conversation;
    use std::time::{Duration, Instant};

    struct Demo {
        ctx: egui::Context,
        app: NativeApp,
        root: PathBuf,
    }
    impl Demo {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let (mut app, root) = super::super::live::tests::fixture(&ctx);
            app.live = None;
            app.demo = true;
            app.worker = Some(
                crate::app::Worker::demo(
                    root.join("attach-demo.sqlite"),
                    std::sync::Arc::new(|| {}),
                )
                .unwrap(),
            );
            app.model.conversations = vec![Conversation {
                id: "welcome".into(),
                title: "Chào mừng".into(),
                last_message_at_ms: 1,
                last_text: "xin chào".into(),
                last_sender: "x".into(),
                group_left: false,
            }];
            app.model.selected = Some("welcome".into());
            app.model.generation = 1;
            app.drafts.select("welcome").unwrap();
            Self { ctx, app, root }
        }
        fn frame(&mut self, input: egui::RawInput) -> egui::FullOutput {
            self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(1200.0, 780.0),
                    )),
                    focused: true,
                    ..input
                },
                |ctx| {
                    // The real `update` drains worker events before drawing.
                    self.app.poll_events(ctx);
                    self.app.workspace(ctx)
                },
            )
        }
        fn idle(&mut self) -> egui::FullOutput {
            self.frame(egui::RawInput::default())
        }
        fn drop_file(&mut self, path: &Path) {
            let _ = self.frame(egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    path: Some(path.to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        /// Run frames until `done` or two seconds pass.
        fn until(&mut self, mut done: impl FnMut(&mut Self) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !done(self) {
                assert!(Instant::now() < deadline, "timed out");
                let _ = self.idle();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    impl Drop for Demo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }
    fn centre(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label:?}"))
    }
    fn file(dir: &Path, name: &str, size: usize) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![9u8; size]).unwrap();
        path
    }

    #[test]
    fn a_dropped_file_shows_in_the_tray_is_sent_as_a_stored_file_message_and_leaves_the_text() {
        let mut demo = Demo::new();
        let path = file(&demo.root, "bao-cao.pdf", 2048);
        demo.app.drafts.active.text = "chú thích chưa gửi".into();
        let before = demo.app.attachment_tray_height();
        assert_eq!(before, 0.0, "no tray without a file");
        demo.drop_file(&path);
        demo.until(|d| d.app.drafts.active.attachment().is_some());
        let output = demo.idle();
        let all = texts(&output);
        assert!(all.iter().any(|t| t == "bao-cao.pdf"), "{all:?}");
        assert!(all.iter().any(|t| t == "PDF · 2 KB"), "{all:?}");
        assert_eq!(demo.app.attachment_tray_height(), TRAY_HEIGHT);
        // Send with nothing typed would be impossible; with a file it is allowed even when the
        // text is empty, and sending the file keeps the typed text.
        demo.app.send_local();
        demo.until(|d| !d.app.drafts.active.in_flight());
        assert!(
            demo.app.drafts.active.attachment().is_none(),
            "the file left the draft"
        );
        assert_eq!(
            demo.app.drafts.active.text, "chú thích chưa gửi",
            "the text stays"
        );
        // The stored message is a real file entry in the history. The panel asks for the same
        // summary; here it is requested directly and picked up by the app's own event polling.
        {
            let worker = demo.app.worker.as_ref().unwrap();
            demo.app
                .model
                .request_overview("welcome", |generation| {
                    worker.submit(crate::app::Command::Overview {
                        generation,
                        thread: "welcome".into(),
                    })
                })
                .unwrap();
        }
        demo.until(|d| d.app.model.overview("welcome").is_some());
        let files = demo.app.model.overview("welcome").unwrap().files.clone();
        assert_eq!(files.len(), 1);
        assert_eq!(
            (
                files[0].title.as_str(),
                files[0].extension.as_str(),
                files[0].size_bytes
            ),
            ("bao-cao.pdf", "pdf", Some(2048))
        );
        assert!(
            files[0].url.is_none(),
            "nothing was uploaded, so there is no address"
        );
    }

    #[test]
    fn removing_the_file_drops_the_tray_and_a_bad_file_explains_itself() {
        let mut demo = Demo::new();
        let path = file(&demo.root, "a.txt", 10);
        demo.drop_file(&path);
        demo.until(|d| d.app.drafts.active.attachment().is_some());
        let output = demo.idle();
        // The remove button is the cross at the right end of the tray, level with the name.
        let name = centre(&output, "a.txt");
        let cross = egui::pos2(1200.0 - 24.0 - 30.0, name.y + 8.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: cross,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = demo.frame(egui::RawInput {
            events: vec![egui::Event::PointerMoved(cross), press(true)],
            ..Default::default()
        });
        let _ = demo.frame(egui::RawInput {
            events: vec![press(false)],
            ..Default::default()
        });
        let _ = demo.idle();
        assert!(
            demo.app.drafts.active.attachment().is_none(),
            "clicking the cross removes the file"
        );
        assert_eq!(demo.app.attachment_tray_height(), 0.0);
        // An empty file is refused with its reason shown in the tray.
        let empty = file(&demo.root, "empty.txt", 0);
        demo.drop_file(&empty);
        demo.until(|d| d.app.attachments.notice.is_some());
        let all = texts(&demo.idle());
        assert!(all.iter().any(|t| t.contains("Tệp trống")), "{all:?}");
        assert!(demo.app.drafts.active.attachment().is_none());
        assert_eq!(demo.app.attachment_tray_height(), TRAY_HEIGHT);
    }

    #[test]
    fn outside_the_demo_a_dropped_file_is_refused_with_the_reason_and_never_attached() {
        let mut demo = Demo::new();
        demo.app.demo = false;
        let path = file(&demo.root, "tai-lieu.docx", 100);
        demo.drop_file(&path);
        let all = texts(&demo.idle());
        assert!(
            all.iter()
                .any(|t| t.contains("Chưa thể gửi tệp trong phiên này")),
            "{all:?}"
        );
        assert!(demo.app.drafts.active.attachment().is_none());
        assert!(
            !demo.app.attachments.busy(),
            "the file is not even inspected"
        );
    }

    #[test]
    fn a_choice_finished_after_changing_conversation_is_not_attached_to_the_new_one() {
        let mut demo = Demo::new();
        let path = file(&demo.root, "x.bin", 5);
        let ctx = demo.ctx.clone();
        demo.app
            .attachments
            .inspect(&ctx, path, Some("another-thread"));
        demo.until(|d| d.app.attachments.notice.is_some());
        assert!(demo.app.drafts.active.attachment().is_none());
        assert!(demo
            .app
            .attachments
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("hãy chọn lại tệp")));
    }

    #[test]
    fn late_picker_result_cannot_cross_account_auth_generation_or_a_new_quote() {
        for changed in ["account", "auth-generation", "quote"] {
            let mut demo = Demo::new();
            demo.app.drafts.active.text = "Bản nháp phải được giữ lại".into();
            let attachment =
                Attachment::inspect(&file(&demo.root, "Tệp tổng hợp.pdf", 32)).unwrap();
            let (tx, rx) = mpsc::channel();
            tx.send(Picked::File(attachment)).unwrap();
            demo.app.attachments.pending = Some(rx);
            demo.app.attachments.for_thread = Some("welcome".into());
            demo.app.attachments.for_account = Some(
                if changed == "account" {
                    "another-account"
                } else {
                    crate::app::DEMO_ACCOUNT
                }
                .into(),
            );
            demo.app.attachments.for_auth_generation = (changed == "auth-generation").then_some(99);
            if changed == "quote" {
                demo.app
                    .drafts
                    .active
                    .set_reply(crate::domain::ReplyReference {
                        account_id: crate::app::DEMO_ACCOUNT.into(),
                        thread_id: "welcome".into(),
                        message_id: "9001".into(),
                        sender: "22".into(),
                        text: "Trích dẫn được giữ lại".into(),
                    });
            }
            let ctx = demo.ctx.clone();
            demo.app.poll_attachments(&ctx, true, "Không thể chọn tệp");
            assert!(demo.app.drafts.active.attachment().is_none(), "{changed}");
            assert_eq!(demo.app.drafts.active.text, "Bản nháp phải được giữ lại");
            assert!(demo.app.attachments.notice.is_some());
            if changed == "quote" {
                assert!(demo.app.drafts.active.reply().is_some());
            }
        }
    }
}
