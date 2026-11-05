//! Bounded account-scoped media thumbnails. No cookies, redirects, HTML, or arbitrary hosts.
use anyhow::Result;
use image::{ImageFormat, ImageReader};
use lru::LruCache;
use std::{
    collections::HashMap,
    io::Cursor,
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

pub const MAX_THUMBNAIL_SIDE: u32 = 1024;
const REQUESTS: usize = 16;
const EVENTS: usize = 2;
const CACHE_ENTRIES: usize = 32;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENCODED: usize = 8 * 1024 * 1024;
const MAX_DECODED: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const COOLDOWN: Duration = Duration::from_secs(30);

/// Full binding is retained through every asynchronous request/result. Never logged.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MediaKey {
    pub account_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub url: String,
}
impl std::fmt::Debug for MediaKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaKey { scoped: true }")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaFailure {
    Network,
    Http(u16),
    UnavailableOrigin,
    Format,
    Resource,
}
impl std::fmt::Display for MediaFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Network => "Chưa tải được ảnh. Bạn có thể thử lại.",
            Self::Http(_) => "Ảnh chưa khả dụng từ máy chủ.",
            Self::UnavailableOrigin => "Ảnh ngoài CDN Zalo không được tự động tải.",
            Self::Format => "Định dạng hoặc cấu hình ảnh này chưa được hỗ trợ.",
            Self::Resource => "Ảnh vượt giới hạn xem trước an toàn.",
        })
    }
}
pub enum MediaEvent {
    Ready {
        key: MediaKey,
        generation: u64,
        width: u32,
        height: u32,
        rgba: Arc<Vec<u8>>,
    },
    Failed {
        key: MediaKey,
        generation: u64,
        reason: MediaFailure,
    },
}
#[derive(Clone)]
struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
}
struct Request {
    key: MediaKey,
    generation: u64,
}
pub struct MediaLoader {
    account_id: String,
    commands: mpsc::Sender<Request>,
    events: mpsc::Receiver<MediaEvent>,
    cancel: watch::Sender<bool>,
    pending: HashMap<MediaKey, u64>,
    failed: LruCache<MediaKey, Instant>,
}
impl MediaLoader {
    pub fn new(account_id: String, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        crate::domain::validate_id(&account_id)?;
        let (commands, receiver) = mpsc::channel(REQUESTS);
        let (sender, events) = mpsc::channel(EVENTS);
        let (cancel, stop) = watch::channel(false);
        let owner = account_id.clone();
        std::thread::Builder::new()
            .name("zalo-media-thumbnails".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    // Client setup (including TLS configuration) belongs to this worker too.
                    let Ok(client) = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(4))
                        .timeout(Duration::from_secs(10))
                        .user_agent("ZaloPC-linux-24v681")
                        .build()
                    else {
                        return;
                    };
                    worker(owner, client, receiver, sender, stop, repaint).await;
                });
            })?;
        Ok(Self {
            account_id,
            commands,
            events,
            cancel,
            pending: HashMap::new(),
            failed: LruCache::new(NonZeroUsize::new(CACHE_ENTRIES).unwrap()),
        })
    }
    pub fn request(&mut self, key: MediaKey, generation: u64) -> bool {
        if key.account_id != self.account_id
            || crate::domain::validate_id(&key.thread_id).is_err()
            || crate::domain::validate_id(&key.message_id).is_err()
            || key.url.len() > 8192
            || key.url.is_empty()
            || self.pending.len() >= REQUESTS
            || self.pending.contains_key(&key)
            || self
                .failed
                .get(&key)
                .is_some_and(|at| at.elapsed() < COOLDOWN)
        {
            return false;
        }
        if self
            .commands
            .try_send(Request {
                key: key.clone(),
                generation,
            })
            .is_err()
        {
            return false;
        }
        self.pending.insert(key, generation);
        true
    }
    pub fn try_event(&mut self) -> Option<MediaEvent> {
        let event = match self.events.try_recv() {
            Ok(event) => event,
            Err(mpsc::error::TryRecvError::Empty) => return None,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                let (key, generation) = self.pending.iter().next()?;
                MediaEvent::Failed {
                    key: key.clone(),
                    generation: *generation,
                    reason: MediaFailure::Network,
                }
            }
        };
        let (key, generation) = match &event {
            MediaEvent::Ready {
                key, generation, ..
            } => {
                self.failed.pop(key);
                (key, *generation)
            }
            MediaEvent::Failed {
                key, generation, ..
            } => {
                self.failed.put(key.clone(), Instant::now());
                (key, *generation)
            }
        };
        if self.pending.get(key) == Some(&generation) {
            self.pending.remove(key);
        }
        Some(event)
    }
    pub fn is_closed(&self) -> bool {
        self.commands.is_closed()
    }
    #[cfg(test)]
    pub(crate) fn closed_fixture() -> Self {
        let (commands, receiver) = mpsc::channel(REQUESTS);
        let (sender, events) = mpsc::channel(EVENTS);
        let (cancel, _) = watch::channel(false);
        drop(receiver);
        drop(sender);
        Self {
            account_id: "11".into(),
            commands,
            events,
            cancel,
            pending: HashMap::new(),
            failed: LruCache::new(NonZeroUsize::new(CACHE_ENTRIES).unwrap()),
        }
    }
    pub fn retry(&mut self, key: &MediaKey) {
        self.failed.pop(key);
    }
}
impl Drop for MediaLoader {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

/// Literal image origins present in the original client: photolink and cloud/CDN resources.
/// Host suffix matching includes a dot boundary; redirect targets are never followed.
fn thumbnail_url(raw: &str) -> std::result::Result<url::Url, MediaFailure> {
    if raw.len() > 8192 {
        return Err(MediaFailure::Resource);
    }
    let mut parsed = url::Url::parse(raw).map_err(|_| MediaFailure::UnavailableOrigin)?;
    let host = parsed.host_str().ok_or(MediaFailure::UnavailableOrigin)?;
    let trusted = host.ends_with(".zdn.vn") || host.ends_with(".zadn.vn");
    // Original viewer upgrades CDN photo URLs to HTTPS before loading.
    if trusted && parsed.scheme() == "http" && parsed.port().is_none_or(|port| port == 80) {
        parsed
            .set_scheme("https")
            .map_err(|_| MediaFailure::UnavailableOrigin)?;
        parsed
            .set_port(None)
            .map_err(|_| MediaFailure::UnavailableOrigin)?;
    }
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some_and(|port| port != 443)
        || !trusted
    {
        return Err(MediaFailure::UnavailableOrigin);
    }
    Ok(parsed)
}
async fn worker(
    account: String,
    client: reqwest::Client,
    mut commands: mpsc::Receiver<Request>,
    events: mpsc::Sender<MediaEvent>,
    mut cancel: watch::Receiver<bool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut cache = LruCache::<MediaKey, Thumbnail>::new(NonZeroUsize::new(CACHE_ENTRIES).unwrap());
    let mut cached_bytes = 0usize;
    loop {
        let request = tokio::select! {biased;_=cancel.changed()=>break,request=commands.recv()=>match request {Some(request)=>request,None=>break}};
        let result = if request.key.account_id != account {
            Err(MediaFailure::UnavailableOrigin)
        } else if let Some(image) = cache.get(&request.key) {
            Ok(image.clone())
        } else {
            let result = match thumbnail_url(&request.key.url) {
                Ok(url) => {
                    tokio::select! {biased;_=cancel.changed()=>break,result=fetch(&client,url)=>result}
                }
                Err(error) => Err(error),
            };
            if let Ok(image) = &result {
                use crate::diagnostics::{Event, Kind, Reason, Stage};
                crate::diagnostics::record(
                    Event::new(Kind::Debug, Stage::MediaAssets, Reason::Ready)
                        .bytes(image.rgba.len()),
                );
                while cache.len() >= CACHE_ENTRIES
                    || cached_bytes.saturating_add(image.rgba.len()) > CACHE_BYTES
                {
                    let Some((_, old)) = cache.pop_lru() else {
                        break;
                    };
                    cached_bytes -= old.rgba.len();
                }
                cached_bytes += image.rgba.len();
                cache.put(request.key.clone(), image.clone());
            }
            result
        };
        if *cancel.borrow() {
            break;
        }
        let event = match result {
            Ok(image) => MediaEvent::Ready {
                key: request.key,
                generation: request.generation,
                width: image.width,
                height: image.height,
                rgba: image.rgba,
            },
            Err(reason) => {
                record_failure(reason);
                MediaEvent::Failed {
                    key: request.key,
                    generation: request.generation,
                    reason,
                }
            }
        };
        tokio::select! {biased;_=cancel.changed()=>break,result=events.send(event)=>if result.is_err(){break;}}
        repaint();
    }
}
fn record_failure(failure: MediaFailure) {
    use crate::diagnostics::{Event, Kind, Reason, Stage};
    let reason = match failure {
        MediaFailure::Network => Reason::Transport,
        MediaFailure::Http(_) => Reason::HttpStatus,
        MediaFailure::UnavailableOrigin | MediaFailure::Format => Reason::Unsupported,
        MediaFailure::Resource => Reason::ResourceLimit,
    };
    let status = match failure {
        MediaFailure::Http(status) => Some(status),
        _ => None,
    };
    crate::diagnostics::record(
        Event::new(Kind::Failure, Stage::MediaAssets, reason).http_status(status),
    );
}
async fn fetch(
    client: &reqwest::Client,
    url: url::Url,
) -> std::result::Result<Thumbnail, MediaFailure> {
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "image/jxl,image/webp,image/png,image/jpeg;q=0.9",
        )
        .send()
        .await
        .map_err(|_| MediaFailure::Network)?;
    if !response.status().is_success() {
        return Err(MediaFailure::Http(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_ENCODED as u64)
    {
        return Err(MediaFailure::Resource);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| MediaFailure::Network)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_ENCODED {
            return Err(MediaFailure::Resource);
        }
        bytes.extend_from_slice(&chunk);
    }
    // A previous pane can finish one synchronous decode after cancellation.
    // This process-wide permit prevents rapid pane changes from multiplying
    // simultaneous decoders; it contains no account or media data.
    let _permit = decode_slot().await?;
    decode(&bytes)
}
async fn decode_slot() -> std::result::Result<tokio::sync::SemaphorePermit<'static>, MediaFailure> {
    static DECODE_SLOT: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    DECODE_SLOT
        .get_or_init(|| tokio::sync::Semaphore::new(1))
        .acquire()
        .await
        .map_err(|_| MediaFailure::Resource)
}
fn decode(bytes: &[u8]) -> std::result::Result<Thumbnail, MediaFailure> {
    if bytes.len() > MAX_ENCODED {
        return Err(MediaFailure::Resource);
    }
    if bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\0\0\0\x0cJXL ") {
        return decode_jxl(bytes);
    }
    let format = image::guess_format(bytes).map_err(|_| MediaFailure::Format)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(MediaFailure::Format);
    }
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader.into_dimensions().map_err(|_| MediaFailure::Format)?;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || (width as u64) * (height as u64) > MAX_PIXELS
    {
        return Err(MediaFailure::Resource);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_DECODED);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| {
        if matches!(error, image::ImageError::Limits(_)) {
            MediaFailure::Resource
        } else {
            MediaFailure::Format
        }
    })?;
    thumbnail(image)
}
fn thumbnail(image: image::DynamicImage) -> std::result::Result<Thumbnail, MediaFailure> {
    let (width, height) = (image.width(), image.height());
    let thumbnail = if width > MAX_THUMBNAIL_SIDE || height > MAX_THUMBNAIL_SIDE {
        image.thumbnail(MAX_THUMBNAIL_SIDE, MAX_THUMBNAIL_SIDE)
    } else {
        image
    };
    let rgba = thumbnail.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Thumbnail {
        width,
        height,
        rgba: Arc::new(rgba.into_raw()),
    })
}

/// Read only codestream events; ancillary boxes (including compressed metadata)
/// are deliberately never expanded for a thumbnail.
fn jxl_codestream(bytes: &[u8]) -> std::result::Result<Vec<u8>, MediaFailure> {
    let mut parser = jxl_bitstream::ContainerParser::new();
    let mut stream = Vec::new();
    for event in parser.feed_bytes(bytes) {
        if let jxl_bitstream::ParseEvent::Codestream(part) =
            event.map_err(|_| MediaFailure::Format)?
        {
            if stream.len().saturating_add(part.len()) > MAX_ENCODED {
                return Err(MediaFailure::Resource);
            }
            stream.extend_from_slice(part);
        }
    }
    if stream.is_empty() {
        return Err(MediaFailure::Format);
    }
    Ok(stream)
}
fn decode_jxl(bytes: &[u8]) -> std::result::Result<Thumbnail, MediaFailure> {
    decode_jxl_with_budget(bytes, MAX_DECODED as usize)
}
fn decode_jxl_with_budget(
    bytes: &[u8],
    budget: usize,
) -> std::result::Result<Thumbnail, MediaFailure> {
    use jxl_oxide_common::Bundle;
    let stream = jxl_codestream(bytes)?;
    let mut bits = jxl_bitstream::Bitstream::new(&stream);
    if bits.read_bits(16).map_err(|_| MediaFailure::Format)? != 0xaff {
        return Err(MediaFailure::Format);
    }
    // SizeHeader precedes all metadata and frame allocations.
    let size =
        jxl_oxide::image::SizeHeader::parse(&mut bits, ()).map_err(|_| MediaFailure::Format)?;
    if size.width == 0
        || size.height == 0
        || size.width > 8192
        || size.height > 8192
        || u64::from(size.width) * u64::from(size.height) > MAX_PIXELS
    {
        return Err(MediaFailure::Resource);
    }
    let mut bits = jxl_bitstream::Bitstream::new(&stream[..stream.len().min(64 * 1024)]);
    let header = jxl_oxide::ImageHeader::parse(&mut bits, ()).map_err(|_| MediaFailure::Format)?;
    // ICC expansion is not covered by upstream AllocTracker. Refuse that
    // profile rather than advertise a memory cap that does not constrain it.
    if header.metadata.colour_encoding.want_icc()
        || header.metadata.preview.is_some()
        || header.metadata.animation.is_some()
        || header.metadata.ec_info.len() > 1
    {
        return Err(MediaFailure::Format);
    }
    let mut uninit = jxl_oxide::JxlImage::builder()
        .alloc_tracker(jxl_oxide::AllocTracker::with_limit(budget))
        .build_uninit();
    let mut offset = 0usize;
    let mut image = loop {
        let end = (offset + 4096).min(stream.len());
        if end == offset {
            return Err(MediaFailure::Format);
        }
        let consumed = uninit
            .feed_bytes(&stream[offset..end])
            .map_err(|_| MediaFailure::Format)?;
        offset += consumed;
        match uninit.try_init().map_err(|_| MediaFailure::Resource)? {
            jxl_oxide::InitializeResult::NeedMoreData(next) => {
                if consumed == 0 {
                    return Err(MediaFailure::Format);
                }
                uninit = next;
            }
            jxl_oxide::InitializeResult::Initialized(image) => break image,
        }
    };
    image.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
        jxl_oxide::RenderingIntent::Relative,
    ));
    while image.num_loaded_keyframes() == 0 && offset < stream.len() {
        let end = (offset + 4096).min(stream.len());
        let consumed = image
            .feed_bytes(&stream[offset..end])
            .map_err(|_| MediaFailure::Format)?;
        if consumed == 0 {
            return Err(MediaFailure::Format);
        }
        offset += consumed;
    }
    let rendered = image.render_frame(0).map_err(|_| MediaFailure::Resource)?;
    let mut pixels = rendered.stream();
    let (width, height, channels) = (pixels.width(), pixels.height(), pixels.channels());
    if !(1..=4).contains(&channels) || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(MediaFailure::Resource);
    }
    let mut row = vec![0u8; width as usize * channels as usize];
    let mut rgba = image::RgbaImage::new(width, height);
    for y in 0..height {
        if pixels.write_to_buffer(&mut row) != row.len() {
            return Err(MediaFailure::Format);
        }
        for (x, sample) in row.chunks_exact(channels as usize).enumerate() {
            let color = match sample {
                [v] => [*v, *v, *v, 255],
                [v, a] => [*v, *v, *v, *a],
                [r, g, b] => [*r, *g, *b, 255],
                [r, g, b, a] => [*r, *g, *b, *a],
                _ => unreachable!(),
            };
            rgba.put_pixel(x as u32, y, image::Rgba(color));
        }
    }
    thumbnail(image::DynamicImage::ImageRgba8(rgba))
}

#[cfg(test)]
pub(crate) fn decode_fixture(
    bytes: &[u8],
) -> std::result::Result<(u32, u32, Vec<u8>), MediaFailure> {
    let image = decode(bytes)?;
    Ok((image.width, image.height, image.rgba.as_ref().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(id: &str) -> MediaKey {
        MediaKey {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            message_id: id.into(),
            url: "https://f12-zpcloud.zdn.vn/synthetic.jpg?private=not-logged".into(),
        }
    }
    #[test]
    fn source_cdn_boundary_upgrade_and_diagnostics_never_include_private_url() {
        for raw in [
            "https://f12-zpcloud.zdn.vn/a.jpg?sig=a%2Bb",
            "https://res-zalo.zadn.vn/a.png",
        ] {
            assert!(thumbnail_url(raw).is_ok());
        }
        assert_eq!(
            thumbnail_url("http://f12-zpcloud.zdn.vn/a.jpg?sig=a%2Bb")
                .unwrap()
                .as_str(),
            "https://f12-zpcloud.zdn.vn/a.jpg?sig=a%2Bb"
        );
        for raw in [
            "file:///private/photo",
            "https://zdn.vn.evil.example/a",
            "https://evilzdn.vn/a",
            "https://a.zdn.vn@evil.example/a",
            "http://evil.example/a",
            "https://a.zdn.vn:8443/a",
            "https://u:p@a.zdn.vn/a",
        ] {
            assert!(thumbnail_url(raw).is_err());
        }
        let debug = format!("{:?}", key("private-message"));
        assert!(!debug.contains("private") && !debug.contains("zdn.vn"));
        for reason in [
            MediaFailure::Network,
            MediaFailure::Format,
            MediaFailure::Resource,
            MediaFailure::Http(403),
        ] {
            assert!(!reason.to_string().contains("private"));
        }
    }
    #[test]
    fn real_png_jpeg_webp_and_jxl_decode_and_malformed_budget_guards() {
        for bytes in [
            include_bytes!("../../tests/fixtures/media/photo.png").as_slice(),
            include_bytes!("../../tests/fixtures/media/photo.jpg").as_slice(),
            include_bytes!("../../tests/fixtures/media/photo.webp").as_slice(),
            include_bytes!("../../tests/fixtures/media/photo.jxl").as_slice(),
        ] {
            let image = decode(bytes).unwrap();
            assert_eq!(
                (image.width, image.height, image.rgba.len()),
                (96, 64, 96 * 64 * 4)
            );
            assert!(image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255));
        }
        assert!(matches!(
            decode_jxl_with_budget(include_bytes!("../../tests/fixtures/media/photo.jxl"), 0),
            Err(MediaFailure::Resource)
        ));
        assert!(matches!(
            decode(&vec![0; MAX_ENCODED + 1]),
            Err(MediaFailure::Resource)
        ));
        assert!(matches!(decode(b"\xff\x0a\x00"), Err(MediaFailure::Format)));
        let jxl = include_bytes!("../../tests/fixtures/media/photo.jxl");
        for end in [0, 1, 2, 8, jxl.len() / 2] {
            assert!(decode(&jxl[..end]).is_err());
        }
        assert!(matches!(decode(b"not an image"), Err(MediaFailure::Format)));
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 8193, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![120; 8193 * 4])
                .unwrap();
        }
        assert!(matches!(decode(&bytes), Err(MediaFailure::Resource)));
    }
    #[test]
    fn jxl_container_discards_compressed_auxiliary_boxes_without_expanding_them() {
        let raw = include_bytes!("../../tests/fixtures/media/photo.jxl");
        let mut container = b"\0\0\0\x0cJXL \r\n\x87\n".to_vec();
        fn add_box(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
            out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(payload);
        }
        add_box(&mut container, b"ftyp", b"jxl \0\0\0\0jxl ");
        add_box(
            &mut container,
            b"brob",
            b"Exif\xff\xff\xffmalicious compressed metadata",
        );
        add_box(&mut container, b"jxlc", raw);
        assert_eq!(jxl_codestream(&container).unwrap(), raw);
        let image = decode(&container).unwrap();
        assert_eq!((image.width, image.height), (96, 64));
        container.truncate(15);
        assert!(decode(&container).is_err());
    }
    #[test]
    fn request_queue_scope_dedup_backoff_and_explicit_retry_are_bounded() {
        let (commands, mut receiver) = mpsc::channel(REQUESTS);
        let (sender, events) = mpsc::channel(EVENTS);
        let (cancel, stop) = watch::channel(false);
        let mut loader = MediaLoader {
            account_id: "11".into(),
            commands,
            events,
            cancel,
            pending: HashMap::new(),
            failed: LruCache::new(NonZeroUsize::new(CACHE_ENTRIES).unwrap()),
        };
        let original = key("1");
        assert!(loader.request(original.clone(), 5));
        assert!(!loader.request(original.clone(), 5));
        assert!(!loader.request(
            MediaKey {
                account_id: "other".into(),
                ..key("2")
            },
            5
        ));
        for i in 2..=REQUESTS {
            assert!(loader.request(key(&i.to_string()), 5));
        }
        assert!(!loader.request(key("overflow"), 5));
        sender
            .try_send(MediaEvent::Failed {
                key: original.clone(),
                generation: 5,
                reason: MediaFailure::Network,
            })
            .ok()
            .unwrap();
        assert!(loader.try_event().is_some());
        receiver.try_recv().unwrap();
        assert!(!loader.request(original.clone(), 5));
        loader.retry(&original);
        assert!(loader.request(original, 5));
        drop(loader);
        assert!(*stop.borrow());
    }
    #[tokio::test]
    async fn pane_drop_cancels_worker_even_when_critical_event_queue_is_full() {
        let (commands, receiver) = mpsc::channel(REQUESTS);
        let (sender, _events) = mpsc::channel(EVENTS);
        let (stop, cancel) = watch::channel(false);
        let worker = tokio::spawn(worker(
            "11".into(),
            reqwest::Client::new(),
            receiver,
            sender,
            cancel,
            Arc::new(|| {}),
        ));
        for id in 0..5 {
            let mut key = key(&id.to_string());
            key.url = "file:///not-network".into();
            commands.send(Request { key, generation: 1 }).await.unwrap();
        }
        tokio::task::yield_now().await;
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn cross_pane_decode_slot_is_exclusive_and_waiters_are_cancelable() {
        let first = decode_slot().await.unwrap();
        let (stop, mut cancel) = watch::channel(false);
        let waiter = tokio::spawn(async move {
            tokio::select! {biased;_=cancel.changed()=>false,result=decode_slot()=>{drop(result.unwrap());true}}
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        stop.send_replace(true);
        assert!(!tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap());
        drop(first);
        assert!(tokio::time::timeout(Duration::from_secs(1), decode_slot())
            .await
            .unwrap()
            .is_ok());
    }
}
