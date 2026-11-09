//! Bounded GPU presentation cache. Decoding/downloads are delegated to app::media.
use crate::app::download::{DownloadEvent, DownloadFailure, FileDownloader, Progress};
use crate::app::media::{MediaEvent, MediaFailure, MediaKey, MediaLoader, MAX_THUMBNAIL_SIDE};
use eframe::egui;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
const TEXTURES: usize = 32;
const BYTES: usize = 16 * 1024 * 1024;
const STATES: usize = 64;
const RETRY: Duration = Duration::from_secs(30);
struct Texture {
    handle: egui::TextureHandle,
    bytes: usize,
}
enum State {
    Pending,
    Failed { at: Instant, reason: MediaFailure },
    Deferred { until: Instant },
}
enum Download {
    Running(Arc<Progress>),
    Saved(PathBuf),
    Failed(DownloadFailure),
}
/// What a file card shows; owned so the cache is not borrowed while drawing.
pub(super) enum DownloadView {
    Idle,
    Running { received: u64, total: Option<u64> },
    Saved(PathBuf),
    Failed(DownloadFailure),
}
pub(super) enum Presentation {
    Image(egui::TextureHandle),
    Loading,
    Unavailable(String),
}
pub(super) struct Viewer {
    pub key: MediaKey,
    pub caption: String,
    pub original_url: Option<String>,
}
pub(crate) struct MediaCache {
    account: Option<String>,
    scope: Option<(String, u64)>,
    loader: Option<MediaLoader>,
    startup_retry: Option<Instant>,
    textures: LruCache<MediaKey, Texture>,
    states: LruCache<MediaKey, State>,
    downloader: Option<FileDownloader>,
    downloads: LruCache<MediaKey, Download>,
    bytes: usize,
    pub(super) viewer: Option<Viewer>,
    #[cfg(test)]
    fixture_only: bool,
}
impl Default for MediaCache {
    fn default() -> Self {
        Self {
            account: None,
            scope: None,
            loader: None,
            startup_retry: None,
            textures: LruCache::new(NonZeroUsize::new(TEXTURES).unwrap()),
            states: LruCache::new(NonZeroUsize::new(STATES).unwrap()),
            downloader: None,
            downloads: LruCache::new(NonZeroUsize::new(STATES).unwrap()),
            bytes: 0,
            viewer: None,
            #[cfg(test)]
            fixture_only: false,
        }
    }
}
impl MediaCache {
    pub fn scope(&mut self, account: &str, thread: &str, generation: u64) {
        if self.account.as_deref() != Some(account) {
            *self = Self::default();
            self.account = Some(account.into());
        }
        if self
            .scope
            .as_ref()
            .map(|(id, revision)| (id.as_str(), *revision))
            != Some((thread, generation))
        {
            self.loader = None;
            self.states.clear();
            self.viewer = None;
            self.startup_retry = None;
            self.scope = Some((thread.into(), generation));
        }
    }
    fn matches(&self, key: &MediaKey, generation: u64) -> bool {
        self.account.as_deref() == Some(key.account_id.as_str())
            && self
                .scope
                .as_ref()
                .map(|(thread, revision)| (thread.as_str(), *revision))
                == Some((key.thread_id.as_str(), generation))
    }
    pub(super) fn visible(
        &mut self,
        ctx: &egui::Context,
        key: &MediaKey,
        generation: u64,
    ) -> Presentation {
        self.retire_closed_loader();
        if !self.matches(key, generation) {
            return Presentation::Unavailable("Ảnh không thuộc trang đang mở.".into());
        }
        if let Some(texture) = self.textures.get(key) {
            return Presentation::Image(texture.handle.clone());
        }
        match self.states.get(key) {
            Some(State::Pending) => return Presentation::Loading,
            Some(State::Failed { at, reason })
                if !matches!(reason, MediaFailure::Network | MediaFailure::Http(_))
                    || at.elapsed() < RETRY =>
            {
                if matches!(reason, MediaFailure::Network | MediaFailure::Http(_)) {
                    ctx.request_repaint_after(RETRY.saturating_sub(at.elapsed()));
                }
                return Presentation::Unavailable(reason.to_string());
            }
            Some(State::Deferred { until }) if Instant::now() < *until => {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                return Presentation::Loading;
            }
            _ => {}
        }
        #[cfg(test)]
        if self.fixture_only {
            return Presentation::Unavailable("Ảnh chưa có trong bộ kiểm thử.".into());
        }
        if self.loader.is_none() {
            if self.startup_retry.is_some_and(|at| at.elapsed() < RETRY) {
                return Presentation::Unavailable("Chưa mở được bộ tải ảnh.".into());
            }
            let repaint = ctx.clone();
            match MediaLoader::new(
                key.account_id.clone(),
                Arc::new(move || repaint.request_repaint()),
            ) {
                Ok(loader) => {
                    self.loader = Some(loader);
                    self.startup_retry = None;
                }
                Err(_) => {
                    self.startup_retry = Some(Instant::now());
                    ctx.request_repaint_after(RETRY);
                    return Presentation::Unavailable("Chưa mở được bộ tải ảnh.".into());
                }
            }
        }
        if self
            .loader
            .as_mut()
            .is_some_and(|loader| loader.request(key.clone(), generation))
        {
            self.states.put(key.clone(), State::Pending);
        } else {
            let delay = Duration::from_millis(250);
            self.states.put(
                key.clone(),
                State::Deferred {
                    until: Instant::now() + delay,
                },
            );
            ctx.request_repaint_after(delay);
        }
        Presentation::Loading
    }
    fn retire_closed_loader(&mut self) {
        if self.loader.as_ref().is_some_and(MediaLoader::is_closed) {
            self.loader = None;
            for (_, state) in self.states.iter_mut() {
                if matches!(state, State::Pending) {
                    *state = State::Failed {
                        at: Instant::now(),
                        reason: MediaFailure::Network,
                    };
                }
            }
        }
    }
    pub fn retry(&mut self, key: &MediaKey) {
        self.retire_closed_loader();
        self.states.pop(key);
        if let Some(loader) = &mut self.loader {
            loader.retry(key);
        }
        self.startup_retry = None;
    }
    pub fn poll(&mut self, ctx: &egui::Context, deadline: Instant, remaining: &mut usize) {
        if *remaining == 0 || Instant::now() >= deadline {
            return;
        }
        self.retire_closed_downloader();
        if let Some(event) = self.downloader.as_mut().and_then(FileDownloader::try_event) {
            *remaining -= 1;
            self.apply_download(event);
            ctx.request_repaint();
            return;
        }
        let Some(event) = self.loader.as_mut().and_then(MediaLoader::try_event) else {
            return;
        };
        *remaining -= 1;
        self.apply(ctx, event);
        ctx.request_repaint();
    }
    pub(super) fn download_view(&mut self, key: &MediaKey) -> DownloadView {
        match self.downloads.get(key) {
            None => DownloadView::Idle,
            Some(Download::Running(progress)) => DownloadView::Running {
                received: progress.received(),
                total: progress.total(),
            },
            Some(Download::Saved(path)) => DownloadView::Saved(path.clone()),
            Some(Download::Failed(reason)) => DownloadView::Failed(*reason),
        }
    }
    /// Explicit user action only. A refused start (queue full, worker gone) is shown as a failure.
    pub(super) fn start_download(&mut self, ctx: &egui::Context, key: MediaKey, name: &str) {
        if self.account.as_deref() != Some(key.account_id.as_str())
            || !self
                .scope
                .as_ref()
                .is_some_and(|(thread, _)| thread == &key.thread_id)
        {
            return;
        }
        #[cfg(test)]
        if self.fixture_only {
            // Synthetic native-window fixtures must never download from a public CDN.
            self.downloads
                .put(key, Download::Failed(DownloadFailure::Network));
            return;
        }
        self.retire_closed_downloader();
        if self.downloader.is_none() {
            let repaint = ctx.clone();
            self.downloader = FileDownloader::new(
                key.account_id.clone(),
                Arc::new(move || repaint.request_repaint()),
            )
            .ok();
        }
        let started = self
            .downloader
            .as_mut()
            .and_then(|downloader| downloader.start(key.clone(), name));
        self.downloads.put(
            key,
            match started {
                Some(progress) => Download::Running(progress),
                None => Download::Failed(DownloadFailure::Network),
            },
        );
    }
    pub(super) fn cancel_download(&mut self, key: &MediaKey) {
        if let Some(Download::Running(progress)) = self.downloads.get(key) {
            progress.cancel();
        }
    }
    pub(super) fn forget_download(&mut self, key: &MediaKey) {
        self.downloads.pop(key);
    }
    pub(super) fn apply_download(&mut self, event: DownloadEvent) {
        let key = match &event {
            DownloadEvent::Done { key, .. } | DownloadEvent::Failed { key, .. } => key,
        };
        if self.account.as_deref() != Some(key.account_id.as_str()) {
            return;
        }
        match event {
            DownloadEvent::Done { key, path, .. } => {
                self.downloads.put(key, Download::Saved(path));
            }
            // A cancelled transfer returns to the button; any other failure keeps its reason.
            DownloadEvent::Failed {
                key,
                reason: DownloadFailure::Cancelled,
            } => {
                self.downloads.pop(&key);
            }
            DownloadEvent::Failed { key, reason } => {
                self.downloads.put(key, Download::Failed(reason));
            }
        }
    }
    fn retire_closed_downloader(&mut self) {
        if self
            .downloader
            .as_ref()
            .is_some_and(FileDownloader::is_closed)
        {
            self.downloader = None;
            for (_, download) in self.downloads.iter_mut() {
                if matches!(download, Download::Running(_)) {
                    *download = Download::Failed(DownloadFailure::Network);
                }
            }
        }
    }
    fn apply(&mut self, ctx: &egui::Context, event: MediaEvent) {
        match event {
            MediaEvent::Failed {
                key,
                generation,
                reason,
            } => {
                if self.matches(&key, generation) {
                    self.states.put(
                        key,
                        State::Failed {
                            at: Instant::now(),
                            reason,
                        },
                    );
                }
            }
            MediaEvent::Ready {
                key,
                generation,
                width,
                height,
                rgba,
            } => {
                if !self.matches(&key, generation) {
                    return;
                }
                let Some(bytes) = valid_bytes(width, height, rgba.len()) else {
                    self.states.put(
                        key,
                        State::Failed {
                            at: Instant::now(),
                            reason: MediaFailure::Resource,
                        },
                    );
                    return;
                };
                if let Some(old) = self.textures.pop(&key) {
                    self.bytes -= old.bytes;
                }
                while self.textures.len() >= TEXTURES || self.bytes.saturating_add(bytes) > BYTES {
                    let Some((_, old)) = self.textures.pop_lru() else {
                        break;
                    };
                    self.bytes -= old.bytes;
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &rgba,
                );
                // Never put URL/account/message text into texture labels or diagnostics.
                let handle =
                    ctx.load_texture("zalo-media-thumbnail", image, egui::TextureOptions::LINEAR);
                self.textures.put(key.clone(), Texture { handle, bytes });
                self.states.pop(&key);
                self.bytes += bytes;
            }
        }
    }
    pub(super) fn texture(&mut self, key: &MediaKey) -> Option<egui::TextureHandle> {
        self.textures.get(key).map(|image| image.handle.clone())
    }
    #[cfg(test)]
    pub fn seed(
        &mut self,
        ctx: &egui::Context,
        key: MediaKey,
        generation: u64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) {
        self.scope(&key.account_id, &key.thread_id, generation);
        self.fixture_only = true;
        self.apply(
            ctx,
            MediaEvent::Ready {
                key,
                generation,
                width,
                height,
                rgba: Arc::new(rgba),
            },
        );
    }
    #[cfg(test)]
    pub fn seed_viewer(&mut self, key: MediaKey, caption: String, original_url: Option<String>) {
        self.viewer = Some(Viewer {
            key,
            caption,
            original_url,
        });
    }
    #[cfg(test)]
    pub fn seed_failure(&mut self, key: MediaKey, generation: u64) {
        self.scope(&key.account_id, &key.thread_id, generation);
        self.fixture_only = true;
        self.states.put(
            key,
            State::Failed {
                at: Instant::now(),
                reason: MediaFailure::Format,
            },
        );
    }
}
fn valid_bytes(width: u32, height: u32, len: usize) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_THUMBNAIL_SIDE || height > MAX_THUMBNAIL_SIDE {
        return None;
    }
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    (expected == len && expected <= BYTES).then_some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(id: &str) -> MediaKey {
        MediaKey {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: id.into(),
            url: "https://f12-zpcloud.zdn.vn/synthetic.jpg".into(),
        }
    }
    fn ready(key: MediaKey, generation: u64, side: u32) -> MediaEvent {
        MediaEvent::Ready {
            key,
            generation,
            width: side,
            height: side,
            rgba: Arc::new(vec![120; side as usize * side as usize * 4]),
        }
    }
    #[test]
    fn downloads_reject_foreign_scope_and_discard_late_events_after_account_change() {
        let ctx = egui::Context::default();
        let mut cache = MediaCache::default();
        cache.scope("11", "d:22", 1);
        let original = key("original");
        for foreign in [
            MediaKey {
                account_id: "99".into(),
                ..original.clone()
            },
            MediaKey {
                thread_id: "g:22".into(),
                ..original.clone()
            },
        ] {
            cache.start_download(&ctx, foreign, "photo.jxl");
            assert!(cache.downloader.is_none());
            assert!(cache.downloads.is_empty());
        }
        cache.apply_download(DownloadEvent::Done {
            key: original.clone(),
            path: "/tmp/photo.jxl".into(),
            bytes: 12,
        });
        assert!(matches!(
            cache.download_view(&original),
            DownloadView::Saved(_)
        ));
        // Same-account navigation retains the completed transfer, shared by the card and viewer.
        cache.scope("11", "d:44", 2);
        assert!(matches!(
            cache.download_view(&original),
            DownloadView::Saved(_)
        ));
        cache.scope("99", "d:22", 1);
        cache.apply_download(DownloadEvent::Done {
            key: original.clone(),
            path: "/tmp/photo.jxl".into(),
            bytes: 12,
        });
        assert!(matches!(cache.download_view(&original), DownloadView::Idle));
        cache.apply_download(DownloadEvent::Failed {
            key: original.clone(),
            reason: DownloadFailure::Http(403),
        });
        assert!(matches!(cache.download_view(&original), DownloadView::Idle));
    }

    #[test]
    fn stale_account_thread_and_page_results_cannot_replace_visible_assets() {
        let ctx = egui::Context::default();
        let mut cache = MediaCache::default();
        cache.scope("11", "d:22", 5);
        cache.apply(&ctx, ready(key("valid"), 5, 2));
        assert_eq!(cache.textures.len(), 1);
        cache.apply(&ctx, ready(key("old-page"), 4, 2));
        cache.apply(
            &ctx,
            ready(
                MediaKey {
                    account_id: "33".into(),
                    ..key("foreign-account")
                },
                5,
                2,
            ),
        );
        cache.apply(
            &ctx,
            ready(
                MediaKey {
                    thread_id: "d:44".into(),
                    ..key("foreign-thread")
                },
                5,
                2,
            ),
        );
        assert_eq!(cache.textures.len(), 1);
        cache.states.put(key("pending"), State::Pending);
        cache.loader = Some(MediaLoader::new("11".into(), Arc::new(|| {})).unwrap());
        cache.scope("11", "d:44", 6);
        assert!(cache.loader.is_none() && cache.states.is_empty());
        assert_eq!(cache.textures.len(), 1); // Bounded same-account cache is reusable.
        assert!(matches!(
            cache.visible(&ctx, &key("valid"), 5),
            Presentation::Unavailable(_)
        ));
        cache.scope("other", "d:44", 1);
        assert!(cache.textures.is_empty() && cache.bytes == 0);
    }
    #[test]
    fn actual_texture_uploads_enforce_payload_budget_and_reject_bad_dimensions() {
        let ctx = egui::Context::default();
        let mut cache = MediaCache::default();
        cache.scope("11", "d:22", 1);
        for i in 0..6 {
            cache.apply(&ctx, ready(key(&i.to_string()), 1, 1024));
            assert!(cache.bytes <= BYTES);
        }
        assert_eq!(cache.bytes, BYTES);
        assert_eq!(cache.textures.len(), 4);
        assert!(cache.texture(&key("0")).is_none());
        cache.apply(&ctx, ready(key("oversized"), 1, 1025));
        assert!(cache.texture(&key("oversized")).is_none());
        cache.scope("22", "d:22", 1);
        cache.scope("11", "d:22", 1);
        for i in 0..40 {
            cache.apply(&ctx, ready(key(&i.to_string()), 1, 2));
        }
        assert_eq!(cache.textures.len(), TEXTURES);
    }
    #[test]
    fn unavailable_fixture_never_starts_network_and_explicit_retry_clears_both_states() {
        let ctx = egui::Context::default();
        let mut cache = MediaCache::default();
        let key = key("missing");
        cache.seed_failure(key.clone(), 1);
        assert!(matches!(
            cache.visible(&ctx, &key, 1),
            Presentation::Unavailable(_)
        ));
        assert!(cache.loader.is_none());
        cache.retry(&key);
        assert!(!cache.states.contains(&key));
        assert!(matches!(
            cache.visible(&ctx, &key, 1),
            Presentation::Unavailable(_)
        ));
        assert!(cache.loader.is_none());
    }
    #[test]
    fn closed_worker_is_retired_and_retry_does_not_get_stuck_deferred() {
        let ctx = egui::Context::default();
        let mut cache = MediaCache::default();
        cache.scope("11", "d:22", 1);
        let key = key("dead-worker");
        cache.states.put(key.clone(), State::Pending);
        cache.loader = Some(MediaLoader::closed_fixture());
        assert!(matches!(
            cache.visible(&ctx, &key, 1),
            Presentation::Unavailable(_)
        ));
        assert!(cache.loader.is_none());
        cache.retry(&key);
        assert!(!cache.states.contains(&key));
        cache.fixture_only = true;
        assert!(matches!(
            cache.visible(&ctx, &key, 1),
            Presentation::Unavailable(_)
        ));
        assert!(cache.loader.is_none());
    }
}
