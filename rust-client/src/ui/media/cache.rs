//! Bounded GPU presentation cache. Decoding/downloads are delegated to app::media.
use crate::app::media::{MediaEvent, MediaFailure, MediaKey, MediaLoader, MAX_THUMBNAIL_SIDE};
use eframe::egui;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
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
        let Some(event) = self.loader.as_mut().and_then(MediaLoader::try_event) else {
            return;
        };
        *remaining -= 1;
        self.apply(ctx, event);
        ctx.request_repaint();
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
