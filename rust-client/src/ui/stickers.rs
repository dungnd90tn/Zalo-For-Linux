//! Lazy live-only thumbnail presentation. No HTTP, file I/O or PNG decoding runs here.
use crate::{
    app::stickers::{StickerEvent, StickerLoader},
    domain::StickerDescriptor,
};
use eframe::egui;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_TEXTURES: usize = 64;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const RETRY: Duration = Duration::from_secs(30);
struct Texture {
    handle: egui::TextureHandle,
    bytes: usize,
}
enum State {
    Pending,
    Failed { at: Instant, label: String },
    Deferred { until: Instant },
}
pub(super) enum Presentation {
    Image(egui::TextureHandle),
    Loading,
    Unavailable(String),
}
pub(super) struct StickerCache {
    loader: Option<StickerLoader>,
    startup_retry: Option<Instant>,
    textures: LruCache<StickerDescriptor, Texture>,
    states: LruCache<StickerDescriptor, State>,
    bytes: usize,
    #[cfg(test)]
    fixture_only: bool,
}
impl Default for StickerCache {
    fn default() -> Self {
        Self {
            loader: None,
            startup_retry: None,
            textures: LruCache::new(NonZeroUsize::new(MAX_TEXTURES).unwrap()),
            states: LruCache::new(NonZeroUsize::new(MAX_TEXTURES).unwrap()),
            bytes: 0,
            #[cfg(test)]
            fixture_only: false,
        }
    }
}
impl StickerCache {
    /// Called only for visible typed stickers in an authenticated live account.
    pub fn visible(&mut self, ctx: &egui::Context, sticker: StickerDescriptor) -> Presentation {
        if let Some(texture) = self.textures.get(&sticker) {
            return Presentation::Image(texture.handle.clone());
        }
        match self.states.get(&sticker) {
            Some(State::Pending) => return Presentation::Loading,
            Some(State::Failed { at, label }) if at.elapsed() < RETRY => {
                ctx.request_repaint_after(RETRY.saturating_sub(at.elapsed()));
                return Presentation::Unavailable(label.clone());
            }
            Some(State::Deferred { until }) if Instant::now() < *until => {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                return Presentation::Loading;
            }
            _ => {}
        }
        #[cfg(test)]
        if self.fixture_only {
            return Presentation::Unavailable("Ảnh nhãn dán chưa có trong bộ kiểm thử.".into());
        }
        if self.loader.is_none() {
            if self.startup_retry.is_some_and(|at| at.elapsed() < RETRY) {
                if let Some(at) = self.startup_retry {
                    ctx.request_repaint_after(RETRY.saturating_sub(at.elapsed()));
                }
                return Presentation::Unavailable("Chưa khởi tạo được tải nhãn dán.".into());
            }
            let repaint = ctx.clone();
            match StickerLoader::new(Arc::new(move || repaint.request_repaint())) {
                Ok(loader) => {
                    self.loader = Some(loader);
                    self.startup_retry = None;
                }
                Err(_) => {
                    self.startup_retry = Some(Instant::now());
                    ctx.request_repaint_after(RETRY);
                    return Presentation::Unavailable("Chưa khởi tạo được tải nhãn dán.".into());
                }
            }
        }
        if self
            .loader
            .as_mut()
            .is_some_and(|loader| loader.request(sticker))
        {
            self.states.put(sticker, State::Pending);
        } else {
            let delay = Duration::from_millis(250);
            self.states.put(
                sticker,
                State::Deferred {
                    until: Instant::now() + delay,
                },
            );
            ctx.request_repaint_after(delay);
        }
        Presentation::Loading
    }
    pub fn poll(&mut self, ctx: &egui::Context, deadline: Instant, remaining: &mut usize) {
        if self.loader.is_none() {
            return;
        }
        if *remaining == 0 || Instant::now() >= deadline {
            if self
                .states
                .iter()
                .any(|(_, state)| matches!(state, State::Pending))
            {
                ctx.request_repaint();
            }
            return;
        }
        let mut drained = 0;
        while drained < 2 && *remaining > 0 && Instant::now() < deadline {
            let Some(event) = self.loader.as_mut().and_then(StickerLoader::try_event) else {
                break;
            };
            drained += 1;
            *remaining -= 1;
            self.apply(ctx, event);
        }
        if drained == 2 {
            ctx.request_repaint();
        }
    }
    fn apply(&mut self, ctx: &egui::Context, event: StickerEvent) {
        match event {
            StickerEvent::Failed { sticker, reason } => {
                self.states.put(
                    sticker,
                    State::Failed {
                        at: Instant::now(),
                        label: reason.to_string(),
                    },
                );
            }
            StickerEvent::Ready {
                sticker,
                width,
                height,
                rgba,
            } => {
                let Some(bytes) = validated_bytes(width, height, rgba.len()) else {
                    self.states.put(
                        sticker,
                        State::Failed {
                            at: Instant::now(),
                            label: "Ảnh nhãn dán không hợp lệ.".into(),
                        },
                    );
                    return;
                };
                self.states.pop(&sticker);
                if let Some(old) = self.textures.pop(&sticker) {
                    self.bytes -= old.bytes;
                }
                while self.bytes + bytes > MAX_BYTES || self.textures.len() >= MAX_TEXTURES {
                    let Some((_, old)) = self.textures.pop_lru() else {
                        break;
                    };
                    self.bytes -= old.bytes;
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    rgba.as_slice(),
                );
                let handle = ctx.load_texture(
                    format!(
                        "zalo-sticker-{}-{}-{}",
                        sticker.category_id, sticker.sticker_type, sticker.sticker_id
                    ),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                self.textures.put(sticker, Texture { handle, bytes });
                self.bytes += bytes;
            }
        }
    }
    #[cfg(test)]
    pub(super) fn seed_failure(&mut self, ctx: &egui::Context, sticker: StickerDescriptor) {
        self.fixture_only = true;
        self.apply(
            ctx,
            StickerEvent::Failed {
                sticker,
                reason: crate::app::stickers::StickerFailure::Format,
            },
        );
    }
    #[cfg(test)]
    pub(super) fn seed(
        &mut self,
        ctx: &egui::Context,
        sticker: StickerDescriptor,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) {
        self.fixture_only = true;
        self.apply(
            ctx,
            StickerEvent::Ready {
                sticker,
                width,
                height,
                rgba: Arc::new(rgba),
            },
        );
    }
}
fn validated_bytes(width: u32, height: u32, len: usize) -> Option<usize> {
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    (bytes == len).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(id: u64) -> StickerDescriptor {
        StickerDescriptor {
            sticker_id: id,
            category_id: 4,
            sticker_type: 3,
        }
    }
    #[test]
    fn texture_cache_enforces_entry_and_aggregate_byte_budgets_and_namespace_identity() {
        let ctx = egui::Context::default();
        let mut cache = StickerCache::default();
        for id in 1..=65 {
            cache.seed(&ctx, descriptor(id), 130, 130, vec![200; 130 * 130 * 4]);
        }
        assert_eq!(cache.textures.len(), MAX_TEXTURES);
        assert!(cache.bytes <= MAX_BYTES);
        assert!(!cache.textures.contains(&descriptor(1)));
        let builtin = StickerDescriptor {
            sticker_id: 65,
            category_id: 0,
            sticker_type: 7,
        };
        cache.seed(&ctx, builtin, 16, 16, vec![128; 16 * 16 * 4]);
        assert!(cache.textures.contains(&builtin));
        assert!(
            cache.textures.contains(&descriptor(65)),
            "equal IDs in different sticker namespaces must not collide"
        );
        for id in 100..=110 {
            cache.seed(&ctx, descriptor(id), 512, 512, vec![220; 512 * 512 * 4]);
        }
        assert!(cache.bytes <= MAX_BYTES);
        assert!(cache.textures.len() <= MAX_TEXTURES);
        assert!(!cache.textures.contains(&descriptor(100)));
        assert!(
            cache.loader.is_none(),
            "preloaded pixels must not initialize HTTP"
        );
    }
    #[test]
    fn invalid_pixels_and_download_failure_backoff_never_create_a_loader_or_texture() {
        let ctx = egui::Context::default();
        let mut cache = StickerCache::default();
        cache.seed(&ctx, descriptor(1), 513, 1, vec![0; 513 * 4]);
        cache.seed(&ctx, descriptor(2), 1, 1, vec![0; 3]);
        for id in [1, 2] {
            assert!(matches!(
                cache.visible(&ctx, descriptor(id)),
                Presentation::Unavailable(_)
            ));
        }
        assert_eq!(cache.bytes, 0);
        assert!(cache.textures.is_empty());
        assert!(cache.loader.is_none());
    }
}
