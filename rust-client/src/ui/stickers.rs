//! Lazy live-only sticker presentation. No HTTP, file I/O or image decoding runs here.
//! An animated sticker is one atlas texture; each frame is a sub-rectangle chosen from the
//! frame clock, so every copy of the same sticker on screen shows the same frame.
use crate::{
    app::stickers::{StickerAnimation, StickerEvent, StickerLoader},
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
const MAX_BYTES: usize = 16 * 1024 * 1024;
const RETRY: Duration = Duration::from_secs(30);
struct Atlas {
    handle: egui::TextureHandle,
    columns: u32,
    frame: egui::Vec2,
    size: egui::Vec2,
    delays_ms: Vec<u32>,
    total_ms: u64,
}
struct Texture {
    /// The still (first) frame.
    handle: egui::TextureHandle,
    atlas: Option<Atlas>,
    bytes: usize,
}
enum State {
    Pending,
    Failed { at: Instant, label: String },
    Deferred { until: Instant },
}
pub(super) enum Presentation {
    /// `uv` selects the visible frame inside `texture`; a still image uses the whole texture.
    Image {
        texture: egui::TextureHandle,
        uv: egui::Rect,
    },
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
            return present(ctx, texture);
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
                animation,
            } => {
                let animation = animation.filter(|animation| animation.is_valid());
                let Some(still_bytes) = validated_bytes(width, height, rgba.len()) else {
                    self.states.put(
                        sticker,
                        State::Failed {
                            at: Instant::now(),
                            label: "Ảnh nhãn dán không hợp lệ.".into(),
                        },
                    );
                    return;
                };
                let bytes = still_bytes + animation.as_ref().map_or(0, |a| a.bytes());
                if bytes > MAX_BYTES {
                    self.states.put(
                        sticker,
                        State::Failed {
                            at: Instant::now(),
                            label: "Ảnh nhãn dán vượt giới hạn xử lý.".into(),
                        },
                    );
                    return;
                }
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
                let name = format!(
                    "zalo-sticker-{}-{}-{}",
                    sticker.category_id, sticker.sticker_type, sticker.sticker_id
                );
                let handle = ctx.load_texture(&name, image, egui::TextureOptions::LINEAR);
                let max_side = ctx.input(|input| input.max_texture_side) as u32;
                // egui panics on a texture larger than the driver allows: such a sticker simply
                // stays a still image.
                let atlas = animation
                    .filter(|a| a.atlas_width.max(a.atlas_height) <= max_side)
                    .map(|animation| load_atlas(ctx, &name, &animation));
                self.textures.put(
                    sticker,
                    Texture {
                        handle,
                        atlas,
                        bytes,
                    },
                );
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
                animation: None,
            },
        );
    }
    #[cfg(test)]
    pub(super) fn seed_animation(
        &mut self,
        ctx: &egui::Context,
        sticker: StickerDescriptor,
        animation: StickerAnimation,
    ) {
        self.fixture_only = true;
        let (width, height) = (animation.frame_width, animation.frame_height);
        let first_row = width as usize * 4;
        let mut still = Vec::with_capacity(first_row * height as usize);
        for row in 0..height as usize {
            // Stride from the real layout, so a deliberately inconsistent header still seeds.
            let start = row * (animation.columns * width) as usize * 4;
            still.extend_from_slice(&animation.rgba[start..start + first_row]);
        }
        self.apply(
            ctx,
            StickerEvent::Ready {
                sticker,
                width,
                height,
                rgba: Arc::new(still),
                animation: Some(Arc::new(animation)),
            },
        );
    }
}
fn load_atlas(ctx: &egui::Context, name: &str, animation: &StickerAnimation) -> Atlas {
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [
            animation.atlas_width as usize,
            animation.atlas_height as usize,
        ],
        &animation.rgba,
    );
    Atlas {
        handle: ctx.load_texture(
            format!("{name}-frames"),
            image,
            egui::TextureOptions::LINEAR,
        ),
        columns: animation.columns,
        frame: egui::vec2(animation.frame_width as f32, animation.frame_height as f32),
        size: egui::vec2(animation.atlas_width as f32, animation.atlas_height as f32),
        delays_ms: animation.delays_ms.clone(),
        total_ms: animation.total_ms().max(1),
    }
}
/// The frame to draw now. Animation advances only while the window has focus, and a repaint is
/// requested only for the next frame boundary, so an idle or hidden sticker costs nothing.
fn present(ctx: &egui::Context, texture: &Texture) -> Presentation {
    let whole = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let Some(atlas) = &texture.atlas else {
        return Presentation::Image {
            texture: texture.handle.clone(),
            uv: whole,
        };
    };
    let (focused, now_ms) = ctx.input(|input| (input.focused, (input.time * 1000.0) as u64));
    let (index, wait_ms) = if focused {
        frame_at(&atlas.delays_ms, atlas.total_ms, now_ms)
    } else {
        (0, 0)
    };
    if focused {
        ctx.request_repaint_after(Duration::from_millis(wait_ms.max(1)));
    }
    let (column, row) = (index as u32 % atlas.columns, index as u32 / atlas.columns);
    let min = egui::pos2(
        column as f32 * atlas.frame.x / atlas.size.x,
        row as f32 * atlas.frame.y / atlas.size.y,
    );
    Presentation::Image {
        texture: atlas.handle.clone(),
        uv: egui::Rect::from_min_size(
            min,
            egui::vec2(atlas.frame.x / atlas.size.x, atlas.frame.y / atlas.size.y),
        ),
    }
}
/// Frame index at `now_ms` and the milliseconds left before the next frame starts.
fn frame_at(delays_ms: &[u32], total_ms: u64, now_ms: u64) -> (usize, u64) {
    let mut at = now_ms % total_ms;
    for (index, delay) in delays_ms.iter().enumerate() {
        let delay = u64::from(*delay);
        if at < delay {
            return (index, delay - at);
        }
        at -= delay;
    }
    (0, 1)
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
        for id in 100..=130 {
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
    fn animation(frames: usize, side: u32, delay: u32) -> StickerAnimation {
        // Frame n is filled with n + 1, laid out in a grid no wider than 2048 px.
        let columns = frames.min((2048 / side) as usize).max(1);
        let rows = frames.div_ceil(columns);
        let (width, height) = (columns as u32 * side, rows as u32 * side);
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        for frame in 0..frames {
            let (column, row) = (frame % columns, frame / columns);
            for y in 0..side as usize {
                for x in 0..side as usize {
                    let at =
                        ((row * side as usize + y) * width as usize + column * side as usize + x)
                            * 4;
                    rgba[at..at + 4].copy_from_slice(&[frame as u8 + 1, 0, 0, 255]);
                }
            }
        }
        StickerAnimation {
            frame_width: side,
            frame_height: side,
            columns: columns as u32,
            atlas_width: width,
            atlas_height: height,
            delays_ms: vec![delay; frames],
            rgba,
        }
    }
    fn shown(
        ctx: &egui::Context,
        cache: &mut StickerCache,
        time: f64,
        focused: bool,
    ) -> (egui::Rect, bool) {
        let mut uv = egui::Rect::NOTHING;
        let mut pass = |ctx: &egui::Context| {
            ctx.run(
                egui::RawInput {
                    time: Some(time),
                    focused,
                    ..Default::default()
                },
                |ctx| {
                    if let Presentation::Image { uv: shown, .. } = cache.visible(ctx, descriptor(7))
                    {
                        uv = shown;
                    }
                },
            )
        };
        // The first pass after a texture upload always asks for one more frame; measure the next.
        let _ = pass(ctx);
        let output = pass(ctx);
        let repaints = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay < Duration::from_secs(60));
        (uv, repaints)
    }
    #[test]
    fn frame_clock_picks_the_frame_and_the_wait_until_the_next_one() {
        let delays = [40, 80, 120];
        for (now, expected) in [
            (0, (0, 40)),
            (39, (0, 1)),
            (40, (1, 80)),
            (119, (1, 1)),
            (120, (2, 120)),
            (239, (2, 1)),
            (240, (0, 40)),
            (240 * 7 + 41, (1, 79)),
        ] {
            assert_eq!(frame_at(&delays, 240, now), expected, "at {now} ms");
        }
    }
    #[test]
    fn animated_sticker_shows_the_clocked_frame_only_while_the_window_is_focused() {
        let ctx = egui::Context::default();
        let mut cache = StickerCache::default();
        cache.seed_animation(&ctx, descriptor(7), animation(4, 16, 100));
        // 4 frames x 100 ms: each frame is a quarter of the atlas width.
        for (time, frame) in [
            (0.0, 0.0),
            (0.15, 1.0),
            (0.25, 2.0),
            (0.39, 3.0),
            (0.41, 0.0),
        ] {
            let (uv, repaints) = shown(&ctx, &mut cache, time, true);
            assert!((uv.min.x - frame / 4.0).abs() < 1e-6, "t={time}: {uv:?}");
            assert!((uv.width() - 0.25).abs() < 1e-6 && (uv.height() - 1.0).abs() < 1e-6);
            assert!(repaints, "a visible animation schedules its next frame");
        }
        // Unfocused: the first frame, and no repaint is requested for it.
        let (uv, repaints) = shown(&ctx, &mut cache, 0.25, false);
        assert_eq!(uv.min.x, 0.0);
        assert!(
            !repaints,
            "an unfocused window must not be woken by a sticker"
        );
    }
    #[test]
    fn still_sticker_uses_the_whole_texture_and_never_asks_for_repaints() {
        let ctx = egui::Context::default();
        let mut cache = StickerCache::default();
        cache.seed(&ctx, descriptor(7), 8, 8, vec![9; 8 * 8 * 4]);
        let (uv, repaints) = shown(&ctx, &mut cache, 1.0, true);
        assert_eq!(
            uv,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0))
        );
        assert!(!repaints);
    }
    #[test]
    fn animation_atlas_counts_toward_the_texture_budget_and_invalid_ones_are_dropped() {
        let ctx = egui::Context::default();
        let mut cache = StickerCache::default();
        let atlas = animation(22, 130, 100);
        let atlas_bytes = atlas.bytes();
        cache.seed_animation(&ctx, descriptor(7), atlas);
        assert_eq!(cache.bytes, 130 * 130 * 4 + atlas_bytes);
        // A frame table that does not match the atlas is ignored: the still remains.
        let mut broken = animation(3, 16, 100);
        broken.delays_ms.pop();
        broken.atlas_width = 999;
        let mut other = StickerCache::default();
        other.seed_animation(&ctx, descriptor(8), broken);
        let (uv, _) = {
            let mut uv = egui::Rect::NOTHING;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                if let Presentation::Image { uv: shown, .. } = other.visible(ctx, descriptor(8)) {
                    uv = shown;
                }
            });
            (uv, ())
        };
        assert_eq!(
            uv.width(),
            1.0,
            "an invalid animation falls back to the still"
        );
        // Enough animated stickers to exceed the budget evict the oldest.
        let mut many = StickerCache::default();
        for id in 0..20 {
            many.seed_animation(&ctx, descriptor(100 + id), animation(22, 130, 100));
        }
        assert!(many.bytes <= MAX_BYTES);
        assert!(!many.textures.contains(&descriptor(100)));
    }
}
