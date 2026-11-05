//! Bounded, credential-free thumbnails from the original client's fixed public endpoints.
//! This is a still PNG thumbnail, not a replacement for catalog/sprite animation.
use crate::domain::StickerDescriptor;
use anyhow::Result;
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
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const EVENT_QUEUE: usize = 4;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_DIMENSION: u32 = 512;
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
        width: u32,
        height: u32,
        rgba: Arc<Vec<u8>>,
    },
    Failed {
        sticker: StickerDescriptor,
        reason: StickerFailure,
    },
}
#[derive(Clone)]
struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
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

async fn worker(
    client: reqwest::Client,
    mut commands: mpsc::Receiver<StickerDescriptor>,
    events: mpsc::Sender<StickerEvent>,
    mut cancel: watch::Receiver<bool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    endpoint: fn(StickerDescriptor) -> Option<url::Url>,
) {
    let mut cache =
        LruCache::<StickerDescriptor, Thumbnail>::new(NonZeroUsize::new(CACHE).unwrap());
    let mut cached_bytes = 0usize;
    loop {
        let id = tokio::select! { biased; _=cancel.changed()=>break, value=commands.recv()=>match value { Some(id)=>id, None=>break } };
        let result = if let Some(image) = cache.get(&id) {
            Ok(image.clone())
        } else {
            let result = if let Some(url) = endpoint(id) {
                tokio::select! { biased; _=cancel.changed()=>break, result=fetch(&client,url)=>result }
            } else {
                Err(StickerFailure::Format)
            };
            if let Ok(image) = &result {
                use crate::diagnostics::{Event, Kind, Reason, Stage};
                crate::diagnostics::record(
                    Event::new(Kind::Debug, Stage::StickerAssets, Reason::Ready)
                        .bytes(image.rgba.len()),
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
        *bytes -= previous.rgba.len();
    }
    while cache.len() >= CACHE || bytes.saturating_add(image.rgba.len()) > CACHE_BYTES {
        let Some((_, previous)) = cache.pop_lru() else {
            break;
        };
        *bytes -= previous.rgba.len();
    }
    *bytes += image.rgba.len();
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
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(StickerFailure::Resource);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| StickerFailure::Network)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err(StickerFailure::Resource);
        }
        bytes.extend_from_slice(&chunk);
    }
    decode_png(&bytes)
}
fn decode_png(bytes: &[u8]) -> std::result::Result<Thumbnail, StickerFailure> {
    if bytes.len() > MAX_BYTES {
        return Err(StickerFailure::Resource);
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(StickerFailure::Format);
    }
    let mut decoder = png::Decoder::new(BufReader::new(Cursor::new(bytes)));
    decoder.set_limits(png::Limits {
        bytes: MAX_BYTES * 4,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| StickerFailure::Format)?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > MAX_DIMENSION
        || info.height > MAX_DIMENSION
    {
        return Err(StickerFailure::Resource);
    }
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= MAX_BYTES)
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
    if rgba.len() != info.width as usize * info.height as usize * 4 {
        return Err(StickerFailure::Format);
    }
    Ok(Thumbnail {
        width: info.width,
        height: info.height,
        rgba: Arc::new(rgba),
    })
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
                Thumbnail {
                    width: 512,
                    height: 512,
                    rgba: Arc::new(vec![0; MAX_BYTES]),
                },
            );
            assert!(bytes <= CACHE_BYTES);
            assert!(cache.len() <= CACHE);
        }
        assert_eq!(cache.len(), 8);
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
                            MAX_BYTES + 1
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
}
