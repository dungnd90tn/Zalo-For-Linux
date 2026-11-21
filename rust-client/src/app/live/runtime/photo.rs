//! Background photo validation and preview for immutable upload bytes.
use super::super::photo::{CacheCharge, PhotoData, MAX_PREPARED_BYTES, PREPARED_BYTES};
use super::*;
use crate::domain::{Attachment, PhotoResolution};
use image::{ImageDecoder, ImageFormat, ImageReader};
use md5::{Digest, Md5};
use std::{
    io::{Cursor, Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    sync::atomic::{AtomicBool, Ordering},
};
const MAX_INPUT: u64 = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_JPEG: usize = 5 * 1024 * 1024;
static PROCESSING: AtomicBool = AtomicBool::new(false);
struct ProcessingPermit;
impl Drop for ProcessingPermit {
    fn drop(&mut self) {
        PROCESSING.store(false, Ordering::Release);
    }
}
struct Pending {
    generation: u64,
    thread: String,
    attachments: Vec<Attachment>,
    request_id: u64,
    resolution: PhotoResolution,
    album: bool,
}
struct Running {
    generation: u64,
    thread: String,
    request_id: u64,
    album: bool,
    cancel: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<std::result::Result<Vec<PreparedPhoto>, PhotoPreparationFailure>>,
}
#[derive(Default)]
pub(super) struct Photos {
    running: Option<Running>,
    pending: Option<Pending>,
}
impl Drop for Photos {
    fn drop(&mut self) {
        if let Some(job) = &self.running {
            job.cancel.store(true, Ordering::Release);
        }
    }
}
impl Photos {
    pub fn active(&self) -> bool {
        self.running.is_some() || self.pending.is_some()
    }
    pub async fn command(
        &mut self,
        command: LiveCommand,
        events: &Events,
    ) -> Result<Option<LiveCommand>> {
        let pending = match command {
            LiveCommand::PreparePhoto {
                generation,
                thread,
                attachment,
                request_id,
                resolution,
            } => Pending {
                generation,
                thread,
                attachments: vec![attachment],
                request_id,
                resolution,
                album: false,
            },
            LiveCommand::PrepareAlbum {
                generation,
                thread,
                attachments,
                request_id,
            } => Pending {
                generation,
                thread,
                attachments,
                request_id,
                resolution: PhotoResolution::Hd,
                album: true,
            },
            LiveCommand::CancelPhotoPreparation { request_id } => {
                self.cancel(events, request_id, None).await?;
                return Ok(None);
            }
            LiveCommand::CancelAlbumPreparation { thread, request_id } => {
                self.cancel(events, request_id, Some(&thread)).await?;
                return Ok(None);
            }
            command => return Ok(Some(command)),
        };
        if let Some(old) = self.pending.replace(pending) {
            self.emit(
                events,
                old.generation,
                old.thread,
                old.request_id,
                old.album,
                Err(PhotoPreparationFailure::Cancelled),
            )
            .await?;
        }
        if let Some(job) = &self.running {
            job.cancel.store(true, Ordering::Release);
        }
        Ok(None)
    }
    async fn cancel(
        &mut self,
        events: &Events,
        request_id: u64,
        album_thread: Option<&str>,
    ) -> Result<()> {
        let matches_scope = |id, thread: &str, album| {
            id == request_id
                && match album_thread {
                    Some(scope) => album && scope == thread,
                    None => !album,
                }
        };
        if let Some(job) = &self.running {
            if matches_scope(job.request_id, &job.thread, job.album) {
                job.cancel.store(true, Ordering::Release);
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|job| matches_scope(job.request_id, &job.thread, job.album))
        {
            let job = self.pending.take().expect("matched preparation");
            self.emit(
                events,
                job.generation,
                job.thread,
                job.request_id,
                job.album,
                Err(PhotoPreparationFailure::Cancelled),
            )
            .await?;
        }
        Ok(())
    }
    async fn emit(
        &self,
        events: &Events,
        generation: u64,
        thread: String,
        request_id: u64,
        album: bool,
        result: std::result::Result<Vec<PreparedPhoto>, PhotoPreparationFailure>,
    ) -> Result<()> {
        let event = if album {
            LiveEvent::AlbumPrepared {
                account_id: events.account.clone(),
                generation,
                thread,
                request_id,
                result,
            }
        } else {
            LiveEvent::PhotoPrepared {
                account_id: events.account.clone(),
                generation,
                thread,
                request_id,
                result: result
                    .and_then(|mut photos| photos.pop().ok_or(PhotoPreparationFailure::Storage)),
            }
        };
        events.emit(event).await
    }
    pub async fn poll(
        &mut self,
        profile: &Arc<Profile>,
        events: &Events,
        lease: u64,
        limits: crate::protocol::bootstrap::PhotoLimits,
        original_limits: Option<crate::protocol::bootstrap::PhotoLimits>,
        album_max_count: usize,
    ) -> Result<()> {
        if let Some(job) = &self.running {
            if !events.privacy.read().is_ok_and(|p| p.permits(&job.thread)) {
                job.cancel.store(true, Ordering::Release);
            }
        }
        if self
            .running
            .as_ref()
            .is_some_and(|job| job.task.is_finished())
        {
            let job = self.running.take().unwrap();
            let result = job
                .task
                .await
                .unwrap_or(Err(PhotoPreparationFailure::Storage));
            let result = if job.cancel.load(Ordering::Acquire) {
                Err(PhotoPreparationFailure::Cancelled)
            } else {
                result
            };
            self.emit(
                events,
                job.generation,
                job.thread,
                job.request_id,
                job.album,
                result,
            )
            .await?;
        }
        if self.running.is_none() {
            if let Some(job) = self.pending.take() {
                let album = job.album;
                if lease == 0 || !events.privacy.read().is_ok_and(|p| p.permits(&job.thread)) {
                    self.emit(
                        events,
                        job.generation,
                        job.thread,
                        job.request_id,
                        album,
                        Err(PhotoPreparationFailure::Cancelled),
                    )
                    .await?;
                    return Ok(());
                }
                let limits = match job.resolution {
                    PhotoResolution::Hd => Some(limits),
                    PhotoResolution::Original => original_limits,
                };
                let Some(limits) = limits
                    .filter(|_| !album || (2..=album_max_count).contains(&job.attachments.len()))
                else {
                    self.emit(
                        events,
                        job.generation,
                        job.thread,
                        job.request_id,
                        album,
                        Err(PhotoPreparationFailure::Unavailable),
                    )
                    .await?;
                    return Ok(());
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let flag = cancel.clone();
                let profile = profile.clone();
                let account = events.account.clone();
                let thread = job.thread.clone();
                let task = tokio::spawn(async move {
                    while PROCESSING
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                    {
                        if flag.load(Ordering::Acquire) {
                            return Err(PhotoPreparationFailure::Cancelled);
                        }
                        tokio::time::sleep(Duration::from_millis(30)).await;
                    }
                    let permit = ProcessingPermit;
                    tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        job.attachments
                            .iter()
                            .map(|attachment| {
                                prepare(
                                    &profile,
                                    &account,
                                    &thread,
                                    lease,
                                    attachment,
                                    &flag,
                                    (limits, job.resolution),
                                )
                            })
                            .collect()
                    })
                    .await
                    .unwrap_or(Err(PhotoPreparationFailure::Storage))
                });
                self.running = Some(Running {
                    generation: job.generation,
                    thread: job.thread,
                    request_id: job.request_id,
                    album,
                    cancel,
                    task,
                });
            }
        }
        Ok(())
    }
}
fn check(cancel: &AtomicBool) -> std::result::Result<(), PhotoPreparationFailure> {
    if cancel.load(Ordering::Acquire) {
        Err(PhotoPreparationFailure::Cancelled)
    } else {
        Ok(())
    }
}
fn reject_animation(
    bytes: &[u8],
    format: ImageFormat,
) -> std::result::Result<(), PhotoPreparationFailure> {
    if format == ImageFormat::Png {
        let mut at = 8usize;
        while at.checked_add(12).is_some_and(|end| end <= bytes.len()) {
            let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let kind = &bytes[at + 4..at + 8];
            if kind == b"acTL" || kind == b"fcTL" || kind == b"fdAT" {
                return Err(PhotoPreparationFailure::Animated);
            }
            at = at
                .checked_add(12)
                .and_then(|n| n.checked_add(length))
                .filter(|end| *end <= bytes.len())
                .ok_or(PhotoPreparationFailure::InvalidFormat)?;
            if kind == b"IEND" {
                break;
            }
        }
    }
    if format == ImageFormat::WebP {
        let mut at = 12usize;
        while at.checked_add(8).is_some_and(|end| end <= bytes.len()) {
            let kind = &bytes[at..at + 4];
            let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if kind == b"ANIM"
                || kind == b"ANMF"
                || (kind == b"VP8X" && bytes.get(at + 8).is_some_and(|flags| flags & 2 != 0))
            {
                return Err(PhotoPreparationFailure::Animated);
            }
            at = at
                .checked_add(8)
                .and_then(|n| n.checked_add(length))
                .and_then(|n| n.checked_add(length % 2))
                .filter(|end| *end <= bytes.len())
                .ok_or(PhotoPreparationFailure::InvalidFormat)?;
        }
    }
    Ok(())
}
struct LimitedBytes(Vec<u8>, usize);
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > self.1 {
            return Err(std::io::Error::other("Photo encoding exceeds limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn flatten_black(image: image::DynamicImage) -> image::RgbImage {
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in rgba.pixels() {
        let alpha = u32::from(pixel[3]);
        for value in &pixel.0[..3] {
            rgb.push(((u32::from(*value) * alpha + 127) / 255) as u8);
        }
    }
    image::RgbImage::from_raw(width, height, rgb).expect("bounded RGB dimensions")
}
fn prepare(
    profile: &Profile,
    account: &str,
    thread: &str,
    lease: u64,
    attachment: &Attachment,
    cancel: &AtomicBool,
    (limits, resolution): (crate::protocol::bootstrap::PhotoLimits, PhotoResolution),
) -> std::result::Result<PreparedPhoto, PhotoPreparationFailure> {
    check(cancel)?;
    let original = resolution == PhotoResolution::Original;
    let max_input = if original {
        limits.size.min(MAX_JPEG as u64)
    } else {
        MAX_INPUT
    };
    if attachment.size_bytes > max_input.min(limits.input_size) {
        return Err(PhotoPreparationFailure::Resource);
    }
    let mut snapshot = super::upload::snapshot(profile, attachment, cancel).map_err(|_| {
        if cancel.load(Ordering::Acquire) {
            PhotoPreparationFailure::Cancelled
        } else {
            PhotoPreparationFailure::Changed
        }
    })?;
    let mut bytes = Vec::with_capacity(snapshot.size as usize);
    snapshot
        .file
        .read_to_end(&mut bytes)
        .map_err(|_| PhotoPreparationFailure::Storage)?;
    let format = image::guess_format(&bytes).map_err(|_| PhotoPreparationFailure::InvalidFormat)?;
    if original && format != ImageFormat::Jpeg {
        return Err(PhotoPreparationFailure::OriginalFormat);
    }
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return Err(PhotoPreparationFailure::InvalidFormat);
    }
    reject_animation(&bytes, format)?;
    check(cancel)?;
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.input_width);
    decoder_limits.max_image_height = Some(limits.input_height);
    decoder_limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(decoder_limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| PhotoPreparationFailure::InvalidFormat)?;
    let (width, height) = decoder.dimensions();
    let (source_width, source_height) = (width, height);
    if width == 0
        || height == 0
        || width > limits.input_width
        || height > limits.input_height
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(PhotoPreparationFailure::Resource);
    }
    let orientation = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .as_deref()
        .and_then(image::metadata::Orientation::from_exif_chunk)
        .unwrap_or(
            decoder
                .orientation()
                .map_err(|_| PhotoPreparationFailure::InvalidFormat)?,
        );
    if original && orientation != image::metadata::Orientation::NoTransforms {
        return Err(PhotoPreparationFailure::OriginalOrientation);
    }
    let mut decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| PhotoPreparationFailure::InvalidFormat)?;
    check(cancel)?;
    if original {
        let (width, height) = (decoded.width(), decoded.height());
        if width > limits.width.min(2560)
            || height > limits.height.min(2560)
            || snapshot.size > limits.size
        {
            return Err(PhotoPreparationFailure::Resource);
        }
        let (file, size, checksum) = match strip_jpeg_app1(&bytes, cancel)? {
            Some(clean) => {
                let size = clean.len() as u64;
                let checksum = format!("{:x}", Md5::digest(&clean));
                (spool(profile, &clean)?, size, checksum)
            }
            None => (snapshot.file, snapshot.size, snapshot.checksum),
        };
        drop(bytes);
        let thumbnail = thumbnail(decoded);
        check(cancel)?;
        let charge = CacheCharge::reserve(
            &PREPARED_BYTES,
            size,
            thumbnail.rgba.len(),
            MAX_PREPARED_BYTES,
        )?;
        return Ok(PreparedPhoto(Arc::new(PhotoData {
            file,
            resolution,
            checksum,
            size,
            width,
            height,
            source_width,
            source_height,
            source_size: snapshot.size,
            thumbnail,
            account: account.into(),
            thread: thread.into(),
            lease,
            _charge: charge,
        })));
    }
    drop(bytes);
    decoded.apply_orientation(orientation);
    check(cancel)?;
    let rgb = flatten_black(decoded);
    let decoded = image::DynamicImage::ImageRgb8(rgb);
    let decoded = if decoded.width() > limits.width || decoded.height() > limits.height {
        decoded.resize(
            limits.width,
            limits.height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    let (width, height) = (decoded.width(), decoded.height());
    let mut encoded = None;
    for quality in [90, 80, 70, 60, 50] {
        check(cancel)?;
        let mut output = LimitedBytes(Vec::new(), (limits.size as usize).min(MAX_JPEG));
        if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality)
            .encode_image(&decoded)
            .is_ok()
        {
            encoded = Some(output.0);
            break;
        }
    }
    let encoded = encoded.ok_or(PhotoPreparationFailure::Resource)?;
    drop(decoded);
    check(cancel)?;
    let preview = image::load_from_memory_with_format(&encoded, ImageFormat::Jpeg)
        .map_err(|_| PhotoPreparationFailure::InvalidFormat)?;
    let thumbnail = thumbnail(preview);
    let checksum = format!("{:x}", Md5::digest(&encoded));
    let size = encoded.len() as u64;
    let file = spool(profile, &encoded)?;
    check(cancel)?;
    let charge = CacheCharge::reserve(
        &PREPARED_BYTES,
        size,
        thumbnail.rgba.len(),
        MAX_PREPARED_BYTES,
    )?;
    Ok(PreparedPhoto(Arc::new(PhotoData {
        file,
        resolution,
        checksum,
        size,
        width,
        height,
        source_width,
        source_height,
        source_size: attachment.size_bytes,
        thumbnail,
        account: account.into(),
        thread: thread.into(),
        lease,
        _charge: charge,
    })))
}

fn spool(profile: &Profile, bytes: &[u8]) -> Result<std::fs::File, PhotoPreparationFailure> {
    let path = profile.paths.cache_dir.join(format!(
        ".photo-{}",
        crate::domain::local_message_id().map_err(|_| PhotoPreparationFailure::Storage)?
    ));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| PhotoPreparationFailure::Storage)?;
    std::fs::remove_file(path).map_err(|_| PhotoPreparationFailure::Storage)?;
    file.write_all(bytes)
        .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
        .map_err(|_| PhotoPreparationFailure::Storage)?;
    Ok(file)
}

/// Delete APP1 segments without decoding or changing any compressed scan data.
/// Segment boundaries also remain checked between scans of progressive JPEGs.
fn strip_jpeg_app1(
    bytes: &[u8],
    cancel: &AtomicBool,
) -> Result<Option<Vec<u8>>, PhotoPreparationFailure> {
    use PhotoPreparationFailure::OriginalFormat;
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(OriginalFormat);
    }
    let mut offset = 2;
    let mut copy_from = 0;
    let mut scan = false;
    let mut output: Option<Vec<u8>> = None;
    while offset < bytes.len() {
        check(cancel)?;
        if scan {
            offset += bytes[offset..]
                .iter()
                .position(|byte| *byte == 0xff)
                .ok_or(OriginalFormat)?;
        }
        if bytes.get(offset) != Some(&0xff) {
            return Err(OriginalFormat);
        }
        let marker_start = offset;
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset).ok_or(OriginalFormat)?;
        offset += 1;
        if scan && matches!(marker, 0x00 | 0xd0..=0xd7) {
            continue;
        }
        match marker {
            0xd9 if offset == bytes.len() => {
                if let Some(output) = &mut output {
                    output.extend_from_slice(&bytes[copy_from..]);
                }
                return Ok(output);
            }
            0x01 => {
                scan = false;
                continue;
            }
            0x00 | 0xd0..=0xd9 => return Err(OriginalFormat),
            _ => {}
        }
        let length = bytes.get(offset..offset + 2).ok_or(OriginalFormat)?;
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        if length < 2 || offset + length > bytes.len() {
            return Err(OriginalFormat);
        }
        offset += length;
        scan = marker == 0xda;
        if marker == 0xe1 {
            output
                .get_or_insert_with(|| Vec::with_capacity(bytes.len() - length))
                .extend_from_slice(&bytes[copy_from..marker_start]);
            copy_from = offset;
        }
    }
    Err(OriginalFormat)
}

fn thumbnail(image: image::DynamicImage) -> PhotoThumbnail {
    let rgba = if image.width() > 256 || image.height() > 256 {
        image.thumbnail(256, 256)
    } else {
        image
    }
    .into_rgba8();
    PhotoThumbnail {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw().into(),
    }
}

#[cfg(test)]
pub(crate) fn prepare_fixture(
    profile: &Profile,
    attachment: &Attachment,
    account: &str,
    thread: &str,
) -> std::result::Result<PreparedPhoto, PhotoPreparationFailure> {
    prepare_fixture_with_resolution(profile, attachment, account, thread, PhotoResolution::Hd)
}

#[cfg(test)]
pub(crate) fn prepare_fixture_with_resolution(
    profile: &Profile,
    attachment: &Attachment,
    account: &str,
    thread: &str,
    resolution: PhotoResolution,
) -> std::result::Result<PreparedPhoto, PhotoPreparationFailure> {
    prepare(
        profile,
        account,
        thread,
        1,
        attachment,
        &AtomicBool::new(false),
        (
            crate::protocol::bootstrap::PhotoLimits::default(),
            resolution,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (std::path::PathBuf, Profile) {
        let root = std::env::temp_dir().join(format!(
            "zalo-prepared-photo-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let profile = Profile::at_root(&root, "synthetic").unwrap();
        (root, profile)
    }
    #[test]
    fn alpha_is_composited_on_black_and_preview_comes_from_the_encoded_private_jpeg() {
        let (root, profile) = fixture();
        let path = root.join("synthetic.png");
        let mut pixels = image::RgbaImage::new(48, 24);
        for (y, row) in pixels.rows_mut().enumerate() {
            for pixel in row {
                *pixel = if y < 12 {
                    image::Rgba([255, 255, 255, 0])
                } else {
                    image::Rgba([255, 255, 255, 128])
                };
            }
        }
        pixels.save(&path).unwrap();
        let selected = Attachment::inspect(&path).unwrap();
        let prepared = prepare_fixture(&profile, &selected, "11", "d:22").unwrap();
        assert_eq!((prepared.width(), prepared.height()), (48, 24));
        assert_eq!(
            (prepared.thumbnail().width, prepared.thumbnail().height),
            (48, 24)
        );
        let mut encoded = vec![0; prepared.size_bytes() as usize];
        std::os::unix::fs::FileExt::read_exact_at(&prepared.0.file, &mut encoded, 0).unwrap();
        let exact = image::load_from_memory_with_format(&encoded, ImageFormat::Jpeg)
            .unwrap()
            .into_rgba8();
        assert_eq!(
            exact.as_raw().as_slice(),
            prepared.thumbnail().rgba.as_ref()
        );
        assert!(exact.get_pixel(24, 3)[0] < 5);
        assert!((120..=136).contains(&exact.get_pixel(24, 20)[0]));
        assert_eq!(prepared.0.checksum, format!("{:x}", Md5::digest(&encoded)));
        assert!(prepared.permits("11", "d:22", 1));
        assert!(!prepared.permits("11", "d:33", 1));
        assert!(!prepared.permits("11", "d:22", 2));
        assert!(!format!("{prepared:?}").contains("synthetic"));
        drop(prepared);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tiny_source_png_can_have_a_larger_valid_processed_jpeg() {
        let (root, profile) = fixture();
        let path = root.join("tiny.png");
        image::RgbImage::new(2, 1).save(&path).unwrap();
        let source = Attachment::inspect(&path).unwrap();
        let limits = crate::protocol::bootstrap::PhotoLimits {
            input_size: source.size_bytes,
            input_width: 2,
            input_height: 1,
            ..Default::default()
        };
        let photo = prepare(
            &profile,
            "11",
            "d:22",
            1,
            &source,
            &AtomicBool::new(false),
            (limits, PhotoResolution::Hd),
        )
        .unwrap();
        assert!(photo.size_bytes() > source.size_bytes);
        assert!(photo.fits_limits(limits));
        assert!(!photo.fits_limits(crate::protocol::bootstrap::PhotoLimits {
            input_size: source.size_bytes - 1,
            ..limits
        }));
        drop(photo);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn photo_selection_is_immutable_and_animated_containers_are_not_first_frame_decoded() {
        let (root, profile) = fixture();
        let path = root.join("synthetic.png");
        image::RgbImage::new(2, 1).save(&path).unwrap();
        let selected = Attachment::inspect(&path).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert_eq!(
            prepare_fixture(&profile, &selected, "11", "d:22").unwrap_err(),
            PhotoPreparationFailure::Changed
        );
        let apng = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &[0, 0, 0, 0],
            b"acTL",
            &[0, 0, 0, 0],
        ]
        .concat();
        assert_eq!(
            reject_animation(&apng, ImageFormat::Png),
            Err(PhotoPreparationFailure::Animated)
        );
        let webp = [b"RIFF\x0e\0\0\0WEBPVP8X\x02\0\0\0".as_slice(), &[2, 0]].concat();
        assert_eq!(
            reject_animation(&webp, ImageFormat::WebP),
            Err(PhotoPreparationFailure::Animated)
        );
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn exif_orientation_is_applied_before_metadata_is_removed_and_server_smaller_limits_are_honored(
    ) {
        let (root, profile) = fixture();
        let path = root.join("synthetic.jpg");
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&image::DynamicImage::ImageRgb8(image::RgbImage::new(8, 4)))
            .unwrap();
        let exif = [
            b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".as_slice(),
        ];
        let data = exif.concat();
        let length = (data.len() + 2) as u16;
        let mut input = jpeg[..2].to_vec();
        input.extend_from_slice(&[0xff, 0xe1]);
        input.extend_from_slice(&length.to_be_bytes());
        input.extend_from_slice(&data);
        input.extend_from_slice(&jpeg[2..]);
        std::fs::write(&path, input).unwrap();
        let selected = Attachment::inspect(&path).unwrap();
        let limits = crate::protocol::bootstrap::PhotoLimits {
            width: 2,
            height: 4,
            ..Default::default()
        };
        let prepared = prepare(
            &profile,
            "11",
            "d:22",
            1,
            &selected,
            &AtomicBool::new(false),
            (limits, PhotoResolution::Hd),
        )
        .unwrap();
        assert_eq!((prepared.width(), prepared.height()), (2, 4));
        let original_bounds = crate::protocol::bootstrap::PhotoLimits {
            input_width: 8,
            input_height: 4,
            ..Default::default()
        };
        let oriented = prepare(
            &profile,
            "11",
            "d:22",
            1,
            &selected,
            &AtomicBool::new(false),
            (original_bounds, PhotoResolution::Hd),
        )
        .unwrap();
        assert_eq!((oriented.width(), oriented.height()), (4, 8));
        assert!(
            oriented.fits_limits(original_bounds),
            "Processed axes must not be compared to original source axes"
        );
        assert!(
            !oriented.fits_limits(crate::protocol::bootstrap::PhotoLimits {
                input_height: 3,
                ..original_bounds
            })
        );
        drop(oriented);
        let mut encoded = vec![0; prepared.size_bytes() as usize];
        std::os::unix::fs::FileExt::read_exact_at(&prepared.0.file, &mut encoded, 0).unwrap();
        assert!(!encoded.windows(6).any(|bytes| bytes == b"Exif\0\0"));
        assert_eq!(
            prepare(
                &profile,
                "11",
                "d:22",
                1,
                &selected,
                &AtomicBool::new(false),
                (original_bounds, PhotoResolution::Original),
            )
            .unwrap_err(),
            PhotoPreparationFailure::OriginalOrientation
        );
        drop(prepared);
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn original_jpeg_preserves_compressed_bytes_and_removes_app1_without_reencoding() {
        let (root, profile) = fixture();
        let path = root.join("synthetic.jpg");
        let mut jpeg = Vec::new();
        let pixels = image::RgbImage::from_pixel(40, 12, image::Rgb([17, 115, 86]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 63)
            .encode_image(&image::DynamicImage::ImageRgb8(pixels))
            .unwrap();
        let mut tagged = jpeg[..2].to_vec();
        for value in [
            b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0".as_slice(),
            b"http://ns.adobe.com/xap/1.0/\0Synthetic metadata".as_slice(),
        ] {
            tagged.extend_from_slice(&[0xff, 0xe1]);
            tagged.extend_from_slice(&((value.len() + 2) as u16).to_be_bytes());
            tagged.extend_from_slice(value);
        }
        tagged.extend_from_slice(&jpeg[2..]);
        let limits = crate::protocol::bootstrap::PhotoLimits::default();
        let cancel = AtomicBool::new(false);
        for source in [&jpeg, &tagged] {
            std::fs::write(&path, source).unwrap();
            let selected = Attachment::inspect(&path).unwrap();
            let photo = prepare(
                &profile,
                "11",
                "g:22",
                1,
                &selected,
                &cancel,
                (limits, PhotoResolution::Original),
            )
            .unwrap();
            assert_eq!(photo.resolution(), PhotoResolution::Original);
            let mut actual = vec![0; photo.size_bytes() as usize];
            std::os::unix::fs::FileExt::read_exact_at(&photo.0.file, &mut actual, 0).unwrap();
            assert_eq!(actual, jpeg);
            assert_eq!(photo.0.checksum, format!("{:x}", Md5::digest(&jpeg)));
            assert_eq!(
                photo.thumbnail().rgba.as_ref(),
                image::load_from_memory_with_format(&jpeg, ImageFormat::Jpeg)
                    .unwrap()
                    .into_rgba8()
                    .as_raw()
                    .as_slice()
            );
            assert!(photo.permits("11", "g:22", 1));
            assert!(!photo.permits("11", "d:22", 1));
        }
        let png = root.join("synthetic.png");
        image::RgbImage::new(2, 1).save(&png).unwrap();
        assert_eq!(
            prepare(
                &profile,
                "11",
                "g:22",
                1,
                &Attachment::inspect(&png).unwrap(),
                &cancel,
                (limits, PhotoResolution::Original),
            )
            .unwrap_err(),
            PhotoPreparationFailure::OriginalFormat
        );
        drop(profile);
        std::fs::remove_dir_all(root).unwrap();
    }
}
