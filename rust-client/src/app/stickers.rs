//! Bounded, credential-free sticker images from the original client's fixed public endpoints.
//! Catalog stickers try the animated sprite resource first and fall back to the still thumbnail.
//! Decoded formats: PNG (still, or a horizontal sprite strip), JPEG, WebP (still or animated) and
//! GIF. Detection uses the file signature, never the Content-Type: the CDN labels some JPEG
//! bytes `image/png`.
use crate::domain::StickerDescriptor;
use anyhow::Result;
use image::{AnimationDecoder, ImageDecoder};
use lru::LruCache;
use std::{
    collections::HashSet,
    io::{BufReader, Cursor},
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

const QUEUE: usize = 32;
const CACHE: usize = 64;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const EVENT_QUEUE: usize = 4;
/// Largest decoded still image (512×512 RGBA) and the cap used by the plain PNG decoder.
const MAX_BYTES: usize = 1024 * 1024;
const MAX_DIMENSION: u32 = 512;
/// Largest compressed download; sprite strips and animated WebP/GIF are bigger than a still.
const MAX_DOWNLOAD: usize = 2 * 1024 * 1024;
const MAX_FRAMES: usize = 64;
/// Largest side of the decoded atlas. 2048 is the texture size egui guarantees; a larger texture
/// makes egui panic, so wider animations wrap onto more rows instead.
const MAX_ATLAS_SIDE: u32 = 2048;
/// A sprite strip is one row of up to MAX_FRAMES frames, so it can be much wider than the atlas.
const MAX_STRIP_WIDTH: u32 = MAX_FRAMES as u32 * MAX_DIMENSION;
const MAX_ANIMATION_BYTES: usize = 4 * 1024 * 1024;
/// The sticker detail API that carries the real per-frame duration needs an authenticated
/// request, which this credential-free loader does not make. Sprite strips therefore play at a
/// fixed rate; animated WebP/GIF keep their own delays.
const SPRITE_FRAME_MS: u32 = 100;
const MIN_FRAME_MS: u32 = 20;
const MAX_FRAME_MS: u32 = 1000;
const RETRY_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickerFailure {
    Network,
    Http(u16),
    Format,
    Resource,
}
impl std::fmt::Display for StickerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Network => "Không tải được ảnh sticker.",
            Self::Http(_) => "Ảnh sticker chưa khả dụng.",
            Self::Format => "Định dạng ảnh sticker chưa được hỗ trợ.",
            Self::Resource => "Ảnh sticker vượt giới hạn xử lý.",
        })
    }
}
#[derive(Debug, Clone)]
pub enum StickerEvent {
    Ready {
        sticker: StickerDescriptor,
        /// The still (first) frame, always present.
        width: u32,
        height: u32,
        rgba: Arc<Vec<u8>>,
        /// Every frame, when the sticker animates.
        animation: Option<Arc<StickerAnimation>>,
    },
    Failed {
        sticker: StickerDescriptor,
        reason: StickerFailure,
    },
}
/// Frames of an animated sticker packed into one RGBA atlas, left to right then top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickerAnimation {
    pub frame_width: u32,
    pub frame_height: u32,
    pub columns: u32,
    pub atlas_width: u32,
    pub atlas_height: u32,
    /// Display time of every frame in milliseconds; its length is the frame count.
    pub delays_ms: Vec<u32>,
    pub rgba: Vec<u8>,
}
impl StickerAnimation {
    pub fn frames(&self) -> usize {
        self.delays_ms.len()
    }
    pub fn total_ms(&self) -> u64 {
        self.delays_ms.iter().map(|ms| u64::from(*ms)).sum()
    }
    /// Whether the geometry is self-consistent and within the decoder's limits.
    pub fn is_valid(&self) -> bool {
        let frames = self.frames();
        let rows = frames.div_ceil(self.columns.max(1) as usize);
        (2..=MAX_FRAMES).contains(&frames)
            && self.frame_width > 0
            && self.frame_height > 0
            && self.frame_width <= MAX_ATLAS_SIDE
            && self.frame_height <= MAX_DIMENSION
            && self.columns > 0
            && self.columns as usize <= frames
            && self.atlas_width == self.columns * self.frame_width
            && self.atlas_height as usize == rows * self.frame_height as usize
            && self.atlas_width <= MAX_ATLAS_SIDE
            && self.atlas_height <= MAX_ATLAS_SIDE
            && self.rgba.len() == self.atlas_width as usize * self.atlas_height as usize * 4
            && self.rgba.len() <= MAX_ANIMATION_BYTES
            && self.delays_ms.iter().all(|ms| *ms > 0)
    }
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}
#[derive(Clone)]
struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
    animation: Option<Arc<StickerAnimation>>,
}
impl Thumbnail {
    fn still(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba: Arc::new(rgba),
            animation: None,
        }
    }
    fn bytes(&self) -> usize {
        self.rgba.len() + self.animation.as_ref().map_or(0, |a| a.bytes())
    }
}
pub struct StickerLoader {
    commands: mpsc::Sender<StickerDescriptor>,
    events: mpsc::Receiver<StickerEvent>,
    cancel: watch::Sender<bool>,
    pending: HashSet<StickerDescriptor>,
    failed: LruCache<StickerDescriptor, Instant>,
}
impl StickerLoader {
    pub fn new(repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(4))
            .user_agent("ZaloPC-linux-24v681")
            .build()?;
        let (commands, receiver) = mpsc::channel(QUEUE);
        let (sender, events) = mpsc::channel(EVENT_QUEUE);
        let (cancel, stop) = watch::channel(false);
        std::thread::Builder::new()
            .name("zalo-sticker-thumbnail".into())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(worker(
                        client,
                        receiver,
                        sender,
                        stop,
                        repaint,
                        thumbnail_url,
                        sprite_url,
                    ));
                }
            })?;
        Ok(Self {
            commands,
            events,
            cancel,
            pending: HashSet::new(),
            failed: LruCache::new(NonZeroUsize::new(CACHE).unwrap()),
        })
    }
    /// Nonblocking and bounded. Failed IDs have a 30-second retry cooldown.
    pub fn request(&mut self, sticker: StickerDescriptor) -> bool {
        if sticker.validate().is_err()
            || self.pending.len() >= QUEUE
            || self.pending.contains(&sticker)
            || self
                .failed
                .get(&sticker)
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return false;
        }
        if self.commands.try_send(sticker).is_err() {
            return false;
        }
        self.pending.insert(sticker);
        true
    }
    pub fn try_event(&mut self) -> Option<StickerEvent> {
        let event = match self.events.try_recv() {
            Ok(event) => event,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                let id = *self.pending.iter().next()?;
                StickerEvent::Failed {
                    sticker: id,
                    reason: StickerFailure::Network,
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => return None,
        };
        let id = match &event {
            StickerEvent::Ready { sticker, .. } => {
                self.failed.pop(sticker);
                *sticker
            }
            StickerEvent::Failed { sticker, .. } => {
                self.failed.put(*sticker, Instant::now());
                *sticker
            }
        };
        self.pending.remove(&id);
        Some(event)
    }
}
impl Drop for StickerLoader {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}
fn thumbnail_url(sticker: StickerDescriptor) -> Option<url::Url> {
    sticker.validate().ok()?;
    // N519573 builtin namespace is distinct from catalog sticker IDs.
    if sticker.category_id == 0 {
        if !(1..=30).contains(&sticker.sticker_id) {
            return None;
        }
        return url::Url::parse(&format!(
            "https://stc-chat.zdn.vn/images/stickers/default/thumb/{}.png",
            sticker.sticker_id
        ))
        .ok();
    }
    // N227150 config + N27412 actual image: no arbitrary URL/cookies/auth scope.
    url::Url::parse(&format!(
        "https://api.zaloapp.com/api/emoticon/sticker/webpc?size=130&eid={}",
        sticker.sticker_id
    ))
    .ok()
}

/// The animation resource of a catalog sticker: the original client's `stickerSpriteUrl`.
/// It returns a still PNG for static stickers, a horizontal PNG strip for sprite animations and
/// an animated WebP for the others. Built-in stickers have no such resource.
fn sprite_url(sticker: StickerDescriptor) -> Option<url::Url> {
    sticker.validate().ok()?;
    if sticker.category_id == 0 {
        return None;
    }
    url::Url::parse(&format!(
        "https://api.zaloapp.com/api/emoticon/sprite?size=130&eid={}",
        sticker.sticker_id
    ))
    .ok()
}

async fn worker(
    client: reqwest::Client,
    mut commands: mpsc::Receiver<StickerDescriptor>,
    events: mpsc::Sender<StickerEvent>,
    mut cancel: watch::Receiver<bool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    endpoint: fn(StickerDescriptor) -> Option<url::Url>,
    animated: fn(StickerDescriptor) -> Option<url::Url>,
) {
    let mut cache =
        LruCache::<StickerDescriptor, Thumbnail>::new(NonZeroUsize::new(CACHE).unwrap());
    let mut cached_bytes = 0usize;
    loop {
        let id = tokio::select! { biased; _=cancel.changed()=>break, value=commands.recv()=>match value { Some(id)=>id, None=>break } };
        let result = if let Some(image) = cache.get(&id) {
            Ok(image.clone())
        } else {
            // Animated resource first. Any failure except a transport error falls back to the
            // still thumbnail, so a sticker without a sprite still shows its poster frame.
            let mut preferred = None;
            if let Some(url) = animated(id) {
                let attempt = tokio::select! { biased; _=cancel.changed()=>break, result=fetch(&client,url)=>result };
                match attempt {
                    Ok(image) => preferred = Some(Ok(image)),
                    Err(StickerFailure::Network) => preferred = Some(Err(StickerFailure::Network)),
                    Err(_) => {}
                }
            }
            let result = match preferred {
                Some(result) => result,
                None => {
                    if let Some(url) = endpoint(id) {
                        tokio::select! { biased; _=cancel.changed()=>break, result=fetch(&client,url)=>result }
                    } else {
                        Err(StickerFailure::Format)
                    }
                }
            };
            if let Ok(image) = &result {
                use crate::diagnostics::{Event, Kind, Reason, Stage};
                crate::diagnostics::record(
                    Event::new(Kind::Debug, Stage::StickerAssets, Reason::Ready)
                        .bytes(image.bytes()),
                );
                cache_image(&mut cache, &mut cached_bytes, id, image.clone());
            }
            result
        };
        let event = match result {
            Ok(image) => StickerEvent::Ready {
                sticker: id,
                width: image.width,
                height: image.height,
                rgba: image.rgba,
                animation: image.animation,
            },
            Err(reason) => {
                record_failure(reason);
                StickerEvent::Failed {
                    sticker: id,
                    reason,
                }
            }
        };
        tokio::select! { biased; _=cancel.changed()=>break, result=events.send(event)=>if result.is_err(){break} }
        repaint();
    }
}
fn cache_image(
    cache: &mut LruCache<StickerDescriptor, Thumbnail>,
    bytes: &mut usize,
    id: StickerDescriptor,
    image: Thumbnail,
) {
    if let Some(previous) = cache.pop(&id) {
        *bytes -= previous.bytes();
    }
    while cache.len() >= CACHE || bytes.saturating_add(image.bytes()) > CACHE_BYTES {
        let Some((_, previous)) = cache.pop_lru() else {
            break;
        };
        *bytes -= previous.bytes();
    }
    *bytes += image.bytes();
    cache.put(id, image);
}
fn record_failure(failure: StickerFailure) {
    use crate::diagnostics::{Event, Kind, Reason, Stage};
    let reason = match failure {
        StickerFailure::Network => Reason::Transport,
        StickerFailure::Http(_) => Reason::HttpStatus,
        StickerFailure::Format => Reason::Unsupported,
        StickerFailure::Resource => Reason::ResourceLimit,
    };
    let status = match failure {
        StickerFailure::Http(status) => Some(status),
        _ => None,
    };
    crate::diagnostics::record(
        Event::new(Kind::Failure, Stage::StickerAssets, reason).http_status(status),
    );
}
async fn fetch(
    client: &reqwest::Client,
    url: url::Url,
) -> std::result::Result<Thumbnail, StickerFailure> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| StickerFailure::Network)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(StickerFailure::Http(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_DOWNLOAD as u64)
    {
        return Err(StickerFailure::Resource);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| StickerFailure::Network)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD {
            return Err(StickerFailure::Resource);
        }
        bytes.extend_from_slice(&chunk);
    }
    decode_image(&bytes)
}
/// Strict single-image PNG: at most 512×512 and 1 MiB of RGBA, first frame only.
#[cfg(test)]
fn decode_png(bytes: &[u8]) -> std::result::Result<Thumbnail, StickerFailure> {
    if bytes.len() > MAX_BYTES {
        return Err(StickerFailure::Resource);
    }
    decode_png_within(bytes, MAX_DIMENSION, MAX_DIMENSION, MAX_BYTES)
}
fn decode_png_within(
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
    max_rgba: usize,
) -> std::result::Result<Thumbnail, StickerFailure> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(StickerFailure::Format);
    }
    let mut decoder = png::Decoder::new(BufReader::new(Cursor::new(bytes)));
    decoder.set_limits(png::Limits {
        bytes: max_rgba.saturating_mul(4),
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| StickerFailure::Format)?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > max_width || info.height > max_height {
        return Err(StickerFailure::Resource);
    }
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= max_rgba.saturating_mul(4))
        .ok_or(StickerFailure::Resource)?;
    let mut output = vec![0; size];
    let info = reader
        .next_frame(&mut output)
        .map_err(|_| StickerFailure::Format)?;
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    let samples = &output[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(samples),
        png::ColorType::Rgb => {
            for pixel in samples.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for gray in samples {
                rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in samples.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        _ => return Err(StickerFailure::Format),
    }
    if rgba.len() != info.width as usize * info.height as usize * 4 || rgba.len() > max_rgba {
        return Err(StickerFailure::Format);
    }
    Ok(Thumbnail::still(info.width, info.height, rgba))
}

/// Decode any supported sticker image, choosing the decoder from the file signature.
fn decode_image(bytes: &[u8]) -> std::result::Result<Thumbnail, StickerFailure> {
    if bytes.len() > MAX_DOWNLOAD {
        return Err(StickerFailure::Resource);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // A sprite strip is wider than the 512 px still limit, so allow the atlas width here and
        // classify afterwards.
        let image = decode_png_within(bytes, MAX_STRIP_WIDTH, MAX_DIMENSION, MAX_ANIMATION_BYTES)?;
        return classify_png(image);
    }
    let format = image::guess_format(bytes).map_err(|_| StickerFailure::Format)?;
    let limits = {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some((MAX_ANIMATION_BYTES * 2) as u64);
        limits
    };
    match format {
        image::ImageFormat::Jpeg => decode_still(bytes, format, limits),
        image::ImageFormat::WebP => {
            let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(map_image_error)?;
            decoder
                .set_limits(limits.clone())
                .map_err(map_image_error)?;
            if decoder.has_animation() {
                animated_frames(decoder.into_frames())
            } else {
                decode_still(bytes, format, limits)
            }
        }
        image::ImageFormat::Gif => {
            let mut decoder =
                image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(map_image_error)?;
            decoder.set_limits(limits).map_err(map_image_error)?;
            animated_frames(decoder.into_frames())
        }
        _ => Err(StickerFailure::Format),
    }
}
fn map_image_error(error: image::ImageError) -> StickerFailure {
    if matches!(error, image::ImageError::Limits(_)) {
        StickerFailure::Resource
    } else {
        StickerFailure::Format
    }
}
fn decode_still(
    bytes: &[u8],
    format: image::ImageFormat,
    limits: image::Limits,
) -> std::result::Result<Thumbnail, StickerFailure> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader.decode().map_err(map_image_error)?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(StickerFailure::Resource);
    }
    Ok(Thumbnail::still(width, height, image.into_raw()))
}
/// A PNG that is a whole number of square frames wide is a sprite strip (the original client's
/// `totalFrames` = width / height). Anything else must be an ordinary still within 512×512.
fn classify_png(image: Thumbnail) -> std::result::Result<Thumbnail, StickerFailure> {
    let (width, height) = (image.width, image.height);
    let frames = (width / height) as usize;
    if width > height && width % height == 0 && (2..=MAX_FRAMES).contains(&frames) {
        let side = height as usize;
        let row = width as usize * 4;
        let cut: Vec<(Vec<u8>, u32)> = (0..frames)
            .map(|frame| {
                let mut pixels = Vec::with_capacity(side * side * 4);
                for y in 0..side {
                    let start = y * row + frame * side * 4;
                    pixels.extend_from_slice(&image.rgba[start..start + side * 4]);
                }
                (pixels, SPRITE_FRAME_MS)
            })
            .collect();
        let still = Thumbnail::still(height, height, cut[0].0.clone());
        return Ok(match pack_atlas(&cut, height, height) {
            Some(animation) => Thumbnail {
                animation: Some(Arc::new(animation)),
                ..still
            },
            None => still,
        });
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION || image.rgba.len() > MAX_BYTES {
        return Err(StickerFailure::Resource);
    }
    Ok(image)
}
/// Collect decoded frames of an animated WebP/GIF. An animation that is too long or too large
/// degrades to its first frame instead of failing, so the sticker still shows.
fn animated_frames(frames: image::Frames<'_>) -> std::result::Result<Thumbnail, StickerFailure> {
    let mut decoded: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut size = None;
    let mut too_long = false;
    for frame in frames {
        if decoded.len() >= MAX_FRAMES {
            too_long = true;
            break;
        }
        let frame = frame.map_err(map_image_error)?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = if denominator == 0 || numerator == 0 {
            SPRITE_FRAME_MS
        } else {
            ((numerator + denominator / 2) / denominator).clamp(MIN_FRAME_MS, MAX_FRAME_MS)
        };
        let buffer = frame.into_buffer();
        let dimensions = buffer.dimensions();
        if dimensions.0 == 0
            || dimensions.1 == 0
            || dimensions.0 > MAX_DIMENSION
            || dimensions.1 > MAX_DIMENSION
        {
            return Err(StickerFailure::Resource);
        }
        if *size.get_or_insert(dimensions) != dimensions {
            return Err(StickerFailure::Format);
        }
        decoded.push((buffer.into_raw(), delay));
    }
    let (width, height) = size.ok_or(StickerFailure::Format)?;
    let first = decoded[0].0.clone();
    let still = Thumbnail::still(width, height, first);
    if decoded.len() < 2 || too_long {
        return Ok(still);
    }
    match pack_atlas(&decoded, width, height) {
        Some(animation) => Ok(Thumbnail {
            animation: Some(Arc::new(animation)),
            ..still
        }),
        None => Ok(still),
    }
}
fn pack_atlas(frames: &[(Vec<u8>, u32)], width: u32, height: u32) -> Option<StickerAnimation> {
    let count = frames.len();
    let columns = count.min((MAX_ATLAS_SIDE / width.max(1)) as usize).max(1);
    let rows = count.div_ceil(columns);
    let atlas_width = columns as u32 * width;
    let atlas_height = rows as u32 * height;
    let bytes = (atlas_width as usize)
        .checked_mul(atlas_height as usize)?
        .checked_mul(4)?;
    if atlas_width > MAX_ATLAS_SIDE || atlas_height > MAX_ATLAS_SIDE || bytes > MAX_ANIMATION_BYTES
    {
        return None;
    }
    let mut rgba = vec![0u8; bytes];
    let row_bytes = width as usize * 4;
    for (index, (pixels, _)) in frames.iter().enumerate() {
        let (column, row) = (index % columns, index / columns);
        for y in 0..height as usize {
            let source = y * row_bytes;
            let target =
                ((row * height as usize + y) * atlas_width as usize + column * width as usize) * 4;
            rgba[target..target + row_bytes].copy_from_slice(&pixels[source..source + row_bytes]);
        }
    }
    let animation = StickerAnimation {
        frame_width: width,
        frame_height: height,
        columns: columns as u32,
        atlas_width,
        atlas_height,
        delays_ms: frames.iter().map(|(_, delay)| *delay).collect(),
        rgba,
    };
    animation.is_valid().then_some(animation)
}
/// Shares the real bounded PNG decoder with the disposable GUI fixture.
#[cfg(all(test, feature = "messaging"))]
pub(crate) fn decode_fixture_png(
    bytes: &[u8],
) -> std::result::Result<(u32, u32, Arc<Vec<u8>>), StickerFailure> {
    let image = decode_png(bytes)?;
    Ok((image.width, image.height, image.rgba))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(id: u64) -> StickerDescriptor {
        StickerDescriptor {
            sticker_id: id,
            category_id: 10047,
            sticker_type: 3,
        }
    }
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![127; width as usize * height as usize * 4])
                .unwrap();
        }
        bytes
    }
    #[test]
    fn fixed_url_and_bounded_png_decode_do_not_accept_arbitrary_assets() {
        let url = thumbnail_url(descriptor(20695)).unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.zaloapp.com/api/emoticon/sticker/webpc?size=130&eid=20695"
        );
        assert!(thumbnail_url(descriptor(u64::MAX)).is_none());
        let builtin = StickerDescriptor {
            sticker_id: 1,
            category_id: 0,
            sticker_type: 7,
        };
        assert_eq!(
            thumbnail_url(builtin).unwrap().as_str(),
            "https://stc-chat.zdn.vn/images/stickers/default/thumb/1.png"
        );
        assert_eq!(
            thumbnail_url(StickerDescriptor {
                sticker_type: 3,
                ..builtin
            })
            .unwrap(),
            thumbnail_url(builtin).unwrap()
        );
        assert!(thumbnail_url(StickerDescriptor {
            sticker_id: 31,
            sticker_type: 0,
            ..builtin
        })
        .is_none());
        assert_ne!(
            thumbnail_url(builtin).unwrap(),
            thumbnail_url(descriptor(1)).unwrap()
        );
        assert!(thumbnail_url(StickerDescriptor {
            sticker_id: 31,
            ..builtin
        })
        .is_none());
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/stickers/wire.json")).unwrap();
        for builtin in fixture["builtins"].as_array().unwrap() {
            let descriptor = StickerDescriptor {
                sticker_id: builtin["id"].as_u64().unwrap(),
                category_id: builtin["catId"].as_u64().unwrap(),
                sticker_type: builtin["type"].as_u64().unwrap() as u32,
            };
            assert_eq!(
                thumbnail_url(descriptor).unwrap().as_str(),
                builtin["thumbnail"].as_str().unwrap()
            );
        }
        let image = decode_png(&png(2, 3)).unwrap();
        assert_eq!((image.width, image.height, image.rgba.len()), (2, 3, 24));
        assert!(matches!(
            decode_png(&png(513, 1)),
            Err(StickerFailure::Resource)
        ));
        assert!(matches!(
            decode_png(&vec![0; MAX_BYTES + 1]),
            Err(StickerFailure::Resource)
        ));
        assert!(matches!(decode_png(b"GIF89a"), Err(StickerFailure::Format)));
        assert!(matches!(
            decode_png(b"\x89PNG\r\n\x1a\ninvalid"),
            Err(StickerFailure::Format)
        ));
    }
    #[test]
    fn decoded_cache_is_bounded_by_aggregate_bytes_and_count() {
        let mut cache = LruCache::new(NonZeroUsize::new(CACHE).unwrap());
        let mut bytes = 0;
        for id in 0..100 {
            cache_image(
                &mut cache,
                &mut bytes,
                descriptor(id),
                Thumbnail::still(512, 512, vec![0; MAX_BYTES]),
            );
            assert!(bytes <= CACHE_BYTES);
            assert!(cache.len() <= CACHE);
        }
        assert_eq!(cache.len(), CACHE_BYTES / MAX_BYTES);
        assert_eq!(bytes, CACHE_BYTES);
        assert!(cache.peek(&descriptor(0)).is_none());
        assert!(cache.peek(&descriptor(99)).is_some());
    }
    #[test]
    fn render_requests_are_deduped_bounded_and_failed_ids_cool_down() {
        let (commands, mut receiver) = mpsc::channel(QUEUE);
        let (sender, events) = mpsc::channel(QUEUE);
        let (cancel, _) = watch::channel(false);
        let mut loader = StickerLoader {
            commands,
            events,
            cancel,
            pending: HashSet::new(),
            failed: LruCache::new(NonZeroUsize::new(CACHE).unwrap()),
        };
        assert!(loader.request(descriptor(1)));
        assert!(!loader.request(descriptor(1)));
        for id in 2..=QUEUE as u64 {
            assert!(loader.request(descriptor(id)));
        }
        assert!(!loader.request(descriptor(33)));
        sender
            .try_send(StickerEvent::Failed {
                sticker: descriptor(1),
                reason: StickerFailure::Network,
            })
            .unwrap();
        assert!(loader.try_event().is_some());
        assert!(!loader.request(descriptor(1)));
        receiver.try_recv().unwrap();
        assert!(loader.request(descriptor(33)));
    }
    #[tokio::test]
    async fn unsupported_builtin_reports_failure_once_without_fetching_or_sticking_loading() {
        let client = reqwest::Client::new();
        let (commands, receiver) = mpsc::channel(QUEUE);
        let (sender, mut events) = mpsc::channel(EVENT_QUEUE);
        let (stop, cancel) = watch::channel(false);
        let worker = tokio::spawn(worker(
            client,
            receiver,
            sender,
            cancel,
            Arc::new(|| {}),
            thumbnail_url,
            |_| None,
        ));
        let sticker = StickerDescriptor {
            sticker_id: 31,
            category_id: 0,
            sticker_type: 7,
        };
        commands.send(sticker).await.unwrap();
        assert!(
            matches!(tokio::time::timeout(Duration::from_secs(1),events.recv()).await.unwrap(),Some(StickerEvent::Failed {sticker:s,reason:StickerFailure::Format}) if s==sticker)
        );
        stop.send_replace(true);
        worker.await.unwrap();
    }
    struct TlsFixture {
        client: reqwest::Client,
        address: std::net::SocketAddr,
        headers: Arc<std::sync::Mutex<Vec<String>>>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl TlsFixture {
        fn new(mode: u8) -> Self {
            use std::io::{Read, Write};
            use std::sync::atomic::{AtomicBool, Ordering};
            let material = crate::protocol::http::tls_tests::material();
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(
                    material.leaf.clone(),
                )],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(material.key.clone()),
                ),
            )
            .unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let headers = Arc::new(std::sync::Mutex::new(Vec::new()));
            let seen = headers.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let halt = stop.clone();
            let thread = std::thread::spawn(move || {
                let config = Arc::new(config);
                while !halt.load(Ordering::Acquire) {
                    let Ok((socket, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_millis(250)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_millis(250)))
                        .unwrap();
                    let mut stream = rustls::StreamOwned::new(
                        rustls::ServerConnection::new(config.clone()).unwrap(),
                        socket,
                    );
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                        let mut byte = [0];
                        if stream.read(&mut byte).ok() != Some(1) {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    if !request.ends_with(b"\r\n\r\n") {
                        continue;
                    }
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8(request).unwrap());
                    if mode == 3 {
                        let mut byte = [0];
                        let _ = stream.read(&mut byte);
                        continue;
                    }
                    if mode == 1 {
                        let _=stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://private.invalid/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    } else if mode == 2 {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_DOWNLOAD + 1
                        );
                    } else {
                        let body = png(2, 3);
                        let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                        let _ = stream.write_all(&body);
                    }
                    let _ = stream.flush();
                }
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .resolve("wpa.zaloapp.com", address)
                .add_root_certificate(reqwest::Certificate::from_der(&material.root).unwrap())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            Self {
                client,
                address,
                headers,
                stop,
                thread: Some(thread),
            }
        }
    }
    impl Drop for TlsFixture {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    fn test_endpoint(id: StickerDescriptor) -> Option<url::Url> {
        Some(
            url::Url::parse(&format!(
                "https://wpa.zaloapp.com/api/emoticon/sticker/webpc?size=130&eid={}",
                id.sticker_id
            ))
            .unwrap(),
        )
    }
    #[tokio::test]
    async fn actual_tls_thumbnail_loader_decodes_without_credentials_and_refuses_redirect_or_oversize(
    ) {
        for (mode, reason) in [
            (0, None),
            (1, Some(StickerFailure::Http(302))),
            (2, Some(StickerFailure::Resource)),
        ] {
            let fixture = TlsFixture::new(mode);
            assert!(fixture.address.ip().is_loopback());
            let result = fetch(&fixture.client, test_endpoint(descriptor(20695)).unwrap()).await;
            match reason {
                None => {
                    let image = result.unwrap();
                    assert_eq!((image.width, image.height, image.rgba.len()), (2, 3, 24));
                }
                Some(reason) => assert!(matches!(result,Err(r) if r==reason)),
            }
            let headers = fixture.headers.lock().unwrap();
            assert_eq!(headers.len(), 1);
            let header = headers[0].to_ascii_lowercase();
            assert!(header
                .starts_with("get /api/emoticon/sticker/webpc?size=130&eid=20695 http/1.1\r\n"));
            for forbidden in ["\r\ncookie:", "\r\nauthorization:", "\r\nreferer:"] {
                assert!(!header.contains(forbidden));
            }
        }
    }
    #[tokio::test]
    async fn actual_thumbnail_worker_cancels_stalled_http_and_full_event_queue() {
        for mode in [0, 3] {
            let fixture = TlsFixture::new(mode);
            let (commands, receiver) = mpsc::channel(QUEUE);
            let (sender, _events) = mpsc::channel(1);
            let (stop, cancel) = watch::channel(false);
            let worker = tokio::spawn(worker(
                fixture.client.clone(),
                receiver,
                sender,
                cancel,
                Arc::new(|| {}),
                test_endpoint,
                |_| None,
            ));
            commands.send(descriptor(1)).await.unwrap();
            commands.send(descriptor(2)).await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while fixture.headers.lock().unwrap().len() < if mode == 0 { 2 } else { 1 } {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            })
            .await
            .unwrap();
            stop.send_replace(true);
            tokio::time::timeout(Duration::from_millis(100), worker)
                .await
                .unwrap()
                .unwrap();
        }
    }
    fn strip(frames: u32, side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, side * frames, side);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            // Frame n is filled with the single value n + 1 so frames are told apart by pixel.
            let mut pixels = Vec::new();
            for _ in 0..side {
                for frame in 0..frames {
                    for _ in 0..side {
                        pixels.extend_from_slice(&[frame as u8 + 1, 0, 0, 255]);
                    }
                }
            }
            writer.write_image_data(&pixels).unwrap();
        }
        bytes
    }
    fn first_red(image: &Thumbnail) -> u8 {
        image.rgba[0]
    }
    #[test]
    fn sprite_strip_png_becomes_an_animation_with_the_first_frame_as_its_still() {
        let image = decode_image(&strip(9, 130)).unwrap();
        // Like the original: frames = width / height, shown in order.
        assert_eq!((image.width, image.height), (130, 130));
        assert_eq!(first_red(&image), 1);
        let animation = image.animation.as_ref().expect("a 9-frame strip animates");
        assert_eq!(animation.frames(), 9);
        assert_eq!((animation.frame_width, animation.frame_height), (130, 130));
        assert_eq!((animation.atlas_width, animation.atlas_height), (1170, 130));
        assert_eq!(animation.columns, 9);
        assert!(animation.delays_ms.iter().all(|ms| *ms == SPRITE_FRAME_MS));
        // The still is cropped, not the whole strip.
        assert_eq!(image.rgba.len(), 130 * 130 * 4);
        for frame in 0..9u32 {
            let offset = (frame * 130) as usize * 4;
            assert_eq!(animation.rgba[offset], frame as u8 + 1);
        }
        assert!(animation.is_valid());
    }
    #[test]
    fn square_and_irregular_pngs_stay_stills_and_oversize_ones_are_refused() {
        assert!(decode_image(&strip(1, 130)).unwrap().animation.is_none());
        // 260×130 is two frames; 200×130 is not a whole number of frames, so it is one image.
        assert_eq!(
            decode_image(&strip(2, 130))
                .unwrap()
                .animation
                .unwrap()
                .frames(),
            2
        );
        assert!(matches!(
            decode_image(&png(513, 513)),
            Err(StickerFailure::Resource)
        ));
        // More frames than the player accepts, and a strip wider than the atlas limit.
        assert!(
            matches!(decode_image(&strip(65, 8)), Err(StickerFailure::Resource)),
            "65 frames is neither a valid strip nor a valid still"
        );
        // A long strip wraps onto rows instead of exceeding the texture size.
        let long = decode_image(&strip(22, 130)).unwrap();
        let animation = long.animation.unwrap();
        assert_eq!(animation.frames(), 22);
        assert_eq!(animation.columns, 15, "2048 / 130 frames per row");
        assert_eq!((animation.atlas_width, animation.atlas_height), (1950, 260));
        assert!(animation.is_valid());
        // Frame 16 sits in the second row, first column.
        let at = (130 * animation.atlas_width as usize) * 4;
        assert_eq!(animation.rgba[at], 16);
    }
    #[test]
    fn animated_webp_and_gif_keep_their_own_frame_delays() {
        for (name, bytes, delays) in [
            (
                "webp",
                &include_bytes!("../../tests/fixtures/stickers/anim.webp")[..],
                [40, 80, 120],
            ),
            (
                "gif",
                &include_bytes!("../../tests/fixtures/stickers/anim.gif")[..],
                [40, 80, 120],
            ),
        ] {
            let image = decode_image(bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let animation = image.animation.as_ref().unwrap_or_else(|| panic!("{name}"));
            assert_eq!((image.width, image.height), (16, 16), "{name}");
            assert_eq!(animation.delays_ms, delays, "{name}");
            assert_eq!((animation.atlas_width, animation.atlas_height), (48, 16));
            assert!(animation.is_valid(), "{name}");
            // Frames are distinct: red, green, blue in order.
            let pixel = |frame: usize| &animation.rgba[frame * 16 * 4..frame * 16 * 4 + 3];
            assert!(pixel(0)[0] > 200 && pixel(0)[1] < 60, "{name} frame 0");
            assert!(pixel(1)[1] > 150 && pixel(1)[0] < 60, "{name} frame 1");
            assert!(pixel(2)[2] > 200 && pixel(2)[0] < 60, "{name} frame 2");
            assert_eq!(
                image.rgba[..4],
                animation.rgba[..4],
                "{name} still is frame 0"
            );
        }
    }
    #[test]
    fn jpeg_and_still_webp_decode_as_stills_whatever_the_server_calls_them() {
        let jpeg = decode_image(include_bytes!("../../tests/fixtures/stickers/still.jpg")).unwrap();
        assert_eq!((jpeg.width, jpeg.height), (16, 16));
        assert!(jpeg.animation.is_none());
        let webp =
            decode_image(include_bytes!("../../tests/fixtures/stickers/still.webp")).unwrap();
        assert_eq!((webp.width, webp.height), (16, 16));
        assert!(webp.animation.is_none());
    }
    #[test]
    fn unsupported_or_corrupt_images_fail_closed() {
        // A zip package (effect stickers) and truncated data are refused, not guessed.
        assert!(matches!(
            decode_image(b"PK\x03\x04\x14\0\x08\x08rest"),
            Err(StickerFailure::Format)
        ));
        assert!(matches!(decode_image(b""), Err(StickerFailure::Format)));
        let gif = include_bytes!("../../tests/fixtures/stickers/anim.gif");
        assert!(decode_image(&gif[..gif.len() / 2]).is_err());
        assert!(matches!(
            decode_image(&vec![0; MAX_DOWNLOAD + 1]),
            Err(StickerFailure::Resource)
        ));
    }
    #[test]
    fn long_or_heavy_animations_degrade_to_their_still_frame() {
        let frame = |value: u8| (vec![value; 16 * 16 * 4], 50);
        let many: Vec<_> = (0..MAX_FRAMES as u8 + 1).map(frame).collect();
        assert!(pack_atlas(&many, 16, 16).is_none() || many.len() <= MAX_FRAMES);
        let heavy: Vec<_> = (0..40).map(|_| (vec![1; 512 * 512 * 4], 50)).collect();
        assert!(
            pack_atlas(&heavy, 512, 512).is_none(),
            "an atlas above the byte cap must not be built"
        );
        // Rows wrap when the frames would exceed the atlas width.
        let small: Vec<_> = (0..20).map(|v| (vec![v; 300 * 100 * 4], 50)).collect();
        let atlas = pack_atlas(&small, 300, 100).unwrap();
        assert_eq!(atlas.columns, 6, "2048 / 300 columns, then more rows");
        assert_eq!((atlas.atlas_width, atlas.atlas_height), (1800, 400));
        assert!(atlas.is_valid());
    }
    #[test]
    fn catalog_stickers_try_the_sprite_resource_and_builtins_do_not() {
        assert_eq!(
            sprite_url(descriptor(23000)).unwrap().as_str(),
            "https://api.zaloapp.com/api/emoticon/sprite?size=130&eid=23000"
        );
        let builtin = StickerDescriptor {
            sticker_id: 1,
            category_id: 0,
            sticker_type: 7,
        };
        assert!(sprite_url(builtin).is_none());
        assert!(sprite_url(descriptor(u64::MAX)).is_none());
    }
}
