//! Independent prepared-image and actual TLS photo-upload checks; all pixels are synthetic.
use super::*;
use crate::domain::Attachment;
use image::{ImageDecoder, ImageFormat};
use md5::{Digest, Md5};

async fn online(events: &mut mpsc::Receiver<LiveEvent>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut connected, mut ready) = (false, false);
        while !connected || !ready {
            match events.recv().await.unwrap() {
                LiveEvent::Connection {
                    state: LiveConnection::Online,
                    ..
                } => connected = true,
                LiveEvent::PrivacyChanged {
                    status: LivePrivacyStatus::Ready,
                    ..
                } => ready = true,
                LiveEvent::Fault { error, .. } => panic!("photo startup: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}
async fn prepare(
    commands: &mpsc::Sender<LiveCommand>,
    events: &mut mpsc::Receiver<LiveEvent>,
    attachment: Attachment,
    thread: &str,
    request_id: u64,
) -> Result<PreparedPhoto, PhotoPreparationFailure> {
    prepare_resolution(
        commands,
        events,
        attachment,
        thread,
        request_id,
        PhotoResolution::Hd,
    )
    .await
}
async fn prepare_resolution(
    commands: &mpsc::Sender<LiveCommand>,
    events: &mut mpsc::Receiver<LiveEvent>,
    attachment: Attachment,
    thread: &str,
    request_id: u64,
    resolution: PhotoResolution,
) -> Result<PreparedPhoto, PhotoPreparationFailure> {
    commands
        .send(LiveCommand::PreparePhoto {
            resolution,
            generation: 1,
            thread: thread.into(),
            attachment,
            request_id,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::PhotoPrepared {
                    request_id: id,
                    result,
                    ..
                } if id == request_id => return result,
                LiveEvent::Fault { error, .. } => panic!("photo preparation: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
async fn terminal(events: &mut mpsc::Receiver<LiveEvent>, request: u64) -> FileTransferPhase {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::FileAccepted {
                    kind: TransferKind::Photo,
                    request_id,
                    message,
                    ..
                } if request_id == request => {
                    assert_eq!(message.id, "901");
                    return FileTransferPhase::Accepted;
                }
                LiveEvent::FileTransfer {
                    kind: TransferKind::Photo,
                    request_id,
                    phase,
                    ..
                } if request_id == request
                    && matches!(
                        phase,
                        FileTransferPhase::Accepted
                            | FileTransferPhase::Failed
                            | FileTransferPhase::Cancelled
                            | FileTransferPhase::Unknown
                            | FileTransferPhase::AcceptedLocalFailure
                    ) =>
                {
                    return phase
                }
                LiveEvent::Fault { error, .. } => panic!("photo upload: {error}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
fn oriented_gps_jpeg() -> Vec<u8> {
    let pixels = image::RgbImage::from_fn(32, 16, |x, _| {
        if x < 16 {
            image::Rgb([245, 5, 5])
        } else {
            image::Rgb([5, 5, 245])
        }
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&image::DynamicImage::ImageRgb8(pixels))
        .unwrap();
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&2u16.to_le_bytes());
    let entry = |data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32| {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&kind.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
    };
    entry(&mut tiff, 0x112, 3, 1, 6);
    entry(&mut tiff, 0x8825, 4, 1, 38);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    entry(&mut tiff, 1, 2, 2, u32::from(b'N'));
    entry(&mut tiff, 2, 5, 3, 92);
    entry(&mut tiff, 3, 2, 2, u32::from(b'E'));
    entry(&mut tiff, 4, 5, 3, 116);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    for n in [21u32, 0, 0, 105, 0, 0] {
        tiff.extend_from_slice(&n.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
    }
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(tiff);
    let mut input = jpeg[..2].to_vec();
    input.extend_from_slice(&[255, 225]);
    input.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    input.extend(exif);
    input.extend_from_slice(&jpeg[2..]);
    input
}
fn selected(f: &Fixture, name: &str, bytes: &[u8]) -> Attachment {
    let path = f.scratch.join(name);
    fs::write(&path, bytes).unwrap();
    Attachment::inspect(&path).unwrap()
}

#[tokio::test]
async fn photo_tls_preview_is_exact_uploaded_jpeg_oriented_and_metadata_free_with_scoped_caption() {
    for (thread, new_route) in [("d:22", false), ("g:22", true)] {
        let f = Fixture::new(Reply::PhotoUpload(0));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.photo_session(new_route),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let original = oriented_gps_jpeg();
        let selected = selected(&f, "synthetic-private-gps.jpg", &original);
        let source_path = selected.path.clone();
        let photo = prepare(&commands, &mut events, selected, thread, 801)
            .await
            .unwrap();
        assert_eq!((photo.width(), photo.height()), (16, 32));
        assert!(photo.size_bytes() <= 5 * 1024 * 1024);
        let thumb = photo.thumbnail().clone();
        assert_eq!((thumb.width, thumb.height), (16, 32));
        let mut previewed = vec![0; photo.size_bytes() as usize];
        std::os::unix::fs::FileExt::read_exact_at(&photo.0.file, &mut previewed, 0).unwrap();
        commands
            .send(LiveCommand::SendPhoto {
                generation: 1,
                thread: thread.into(),
                photo,
                caption: "Ảnh tổng hợp\nGiữ nguyên chú thích".into(),
                request_id: 802,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut events, 802).await,
            FileTransferPhase::Accepted
        );
        let uploads = f.state.photo.uploads.lock().unwrap().clone();
        let sent: Vec<u8> = uploads
            .iter()
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect();
        assert_eq!(
            sent, previewed,
            "The previewed immutable JPEG must be the uploaded byte stream"
        );
        let exact = image::load_from_memory_with_format(&sent, ImageFormat::Jpeg)
            .unwrap()
            .into_rgba8();
        assert_eq!((exact.width(), exact.height()), (16, 32));
        assert_eq!(exact.as_raw().as_slice(), thumb.rgba.as_ref());
        assert!(exact.get_pixel(8, 8)[0] > 220 && exact.get_pixel(8, 8)[2] < 30);
        assert!(exact.get_pixel(8, 24)[2] > 220 && exact.get_pixel(8, 24)[0] < 30);
        let mut decoder =
            image::ImageReader::with_format(std::io::Cursor::new(&sent), ImageFormat::Jpeg)
                .into_decoder()
                .unwrap();
        assert!(decoder.exif_metadata().unwrap().is_none());
        assert!(!sent.windows(6).any(|v| v == b"Exif\0\0"));
        assert_eq!(fs::read(source_path).unwrap(), original);
        for (p, _) in &uploads {
            assert_eq!(p["fileName"], "photo.jpg");
            assert!(p.get("jxl").is_none());
            assert!(p.get("checksum").is_none());
        }
        let finals = f.state.photo.finals.lock().unwrap().clone();
        assert_eq!(finals.len(), 1);
        let p = &finals[0];
        assert_eq!(p["desc"], "Ảnh tổng hợp\nGiữ nguyên chú thích");
        assert_eq!(p["width"], 16);
        assert_eq!(p["height"], 32);
        assert_eq!(p["language"], "VN");
        assert_eq!(p["imei"], "native-fixture");
        assert_eq!(p["ttl"], 0);
        assert!(p
            .get(if thread.starts_with("g:") {
                "oriUrl"
            } else {
                "normalUrl"
            })
            .is_some());
        let c = rusqlite::Connection::open_with_flags(
            f.profile.paths.data_dir.join("messages.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let checksum: String = c
            .query_row("SELECT checksum FROM photo_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(checksum, format!("{:x}", Md5::digest(&sent)));
        drop(c);
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[test]
fn retained_preview_charge_is_checked_and_released_only_after_the_last_shared_owner() {
    use crate::app::live::photo::{CacheCharge, MAX_PREPARED_BYTES};
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let bytes = 512 + 256 * 256 * 4;
    let charge =
        Arc::new(CacheCharge::reserve(&COUNTER, 512, 256 * 256 * 4, MAX_PREPARED_BYTES).unwrap());
    let clone = charge.clone();
    assert_eq!(COUNTER.load(Ordering::Acquire), bytes);
    drop(charge);
    assert_eq!(COUNTER.load(Ordering::Acquire), bytes);
    drop(clone);
    assert_eq!(COUNTER.load(Ordering::Acquire), 0);
    assert!(CacheCharge::reserve(&COUNTER, u64::MAX, 1, MAX_PREPARED_BYTES).is_err());
    assert_eq!(COUNTER.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn photo_tls_source_input_limit_does_not_reject_a_larger_prepared_jpeg() {
    let f = Fixture::new(Reply::PhotoUpload(0));
    let mut input = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(8, 4))
        .write_to(&mut std::io::Cursor::new(&mut input), ImageFormat::Png)
        .unwrap();
    let input_limit = input.len() as u64 + 1;
    let config=ServerConfig::parse(&json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"conversation":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"]},"zpw_ws":["wss://wpa.zaloapp.com"],"settings":{"sharefile":{"chunk_size_file":64000},"file_photo_limit":{"enable_new_endpoint_upload_photo":0,"resolutionLimit":{"hd":{"width":32,"height":16,"size":5242880,"maxInputSize":input_limit,"maxInputWidth":8,"maxInputHeight":4}}}}})).unwrap();
    let session = Arc::new(synthetic_session(f.profile.clone(), config));
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        session,
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    online(&mut events).await;
    let a = selected(&f, "tiny-source.png", &input);
    let photo = prepare(&commands, &mut events, a, "d:22", 811)
        .await
        .unwrap();
    assert!(
        photo.size_bytes() > input_limit,
        "Fixture must distinguish original input bytes from processed output bytes"
    );
    commands
        .send(LiveCommand::SendPhoto {
            generation: 1,
            thread: "d:22".into(),
            photo,
            caption: "Tiny source, valid JPEG".into(),
            request_id: 812,
        })
        .await
        .unwrap();
    assert_eq!(
        terminal(&mut events, 812).await,
        FileTransferPhase::Accepted
    );
    assert_eq!(f.state.photo.finals.lock().unwrap().len(), 1);
    stop.send(true).unwrap();
    task.await.unwrap();
}

fn noisy_png() -> Vec<u8> {
    let mut seed = 7u32;
    let data = (0..512 * 256 * 3)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let image = image::RgbImage::from_raw(512, 256, data).unwrap();
    let mut encoded = Vec::new();
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut std::io::Cursor::new(&mut encoded), ImageFormat::Png)
        .unwrap();
    encoded
}
fn prepared_bytes(photo: &PreparedPhoto) -> Vec<u8> {
    let mut bytes = vec![0; photo.size_bytes() as usize];
    std::os::unix::fs::FileExt::read_exact_at(&photo.0.file, &mut bytes, 0).unwrap();
    bytes
}

#[tokio::test]
async fn original_photo_tls_requires_current_plan_and_exact_original_resource() {
    let mut clean = Vec::new();
    let pixels = image::RgbImage::from_pixel(64, 32, image::Rgb([23, 110, 180]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut clean, 94)
        .encode_image(&image::DynamicImage::ImageRgb8(pixels))
        .unwrap();
    let mut source = clean[..2].to_vec();
    let metadata = b"http://ns.adobe.com/xap/1.0/\0synthetic-private-photo-metadata";
    source.extend_from_slice(&[255, 225]);
    source.extend_from_slice(&((metadata.len() + 2) as u16).to_be_bytes());
    source.extend_from_slice(metadata);
    source.extend_from_slice(&clean[2..]);
    for (mode, thread) in [(10, "d:22"), (10, "g:22"), (11, "d:22"), (12, "d:22")] {
        let f = Fixture::new(Reply::PhotoUpload(mode));
        let origin = json!(["https://wpa.zaloapp.com"]);
        let config = ServerConfig::parse(&json!({
            "zpw_service_map_v3":{
                "chat":origin,"group":origin,"conversation":origin,"file":origin,
                "zcloud":origin,"zcloud_up_file":origin
            },
            "zpw_ws":["wss://wpa.zaloapp.com"],
            "settings":{"features":{"file_photo_limit":{"enable_upload_original":1}}}
        }))
        .unwrap();
        let session = Arc::new(synthetic_session(f.profile.clone(), config));
        let (commands, requests) = mpsc::channel(16);
        let (events, mut incoming) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            session,
            requests,
            events,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            let (mut connected, mut paid) = (false, false);
            while !connected || !paid {
                match incoming.recv().await.unwrap() {
                    LiveEvent::Connection {
                        state: LiveConnection::Online,
                        ..
                    } => connected = true,
                    LiveEvent::PhotoCapability {
                        original_available,
                        available,
                        ..
                    } => {
                        assert!(available);
                        paid = original_available;
                    }
                    LiveEvent::Fault { error, .. } => panic!("original photo entitlement: {error}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let photo = prepare_resolution(
            &commands,
            &mut incoming,
            selected(&f, "synthetic-original.jpg", &source),
            thread,
            901,
            PhotoResolution::Original,
        )
        .await
        .unwrap();
        assert_eq!(photo.resolution(), PhotoResolution::Original);
        assert_eq!(prepared_bytes(&photo), clean);
        commands
            .send(LiveCommand::SendPhoto {
                generation: 1,
                thread: thread.into(),
                photo,
                caption: "Ảnh tổng hợp gốc".into(),
                request_id: 902,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut incoming, 902).await,
            if mode == 10 {
                FileTransferPhase::Accepted
            } else {
                FileTransferPhase::Failed
            }
        );
        assert_eq!(
            f.state.photo.original_plan_requests.load(Ordering::SeqCst),
            2
        );
        let uploads = f.state.photo.uploads.lock().unwrap().clone();
        assert_eq!(uploads.len(), usize::from(mode != 11));
        if let Some((params, bytes)) = uploads.first() {
            assert_eq!(bytes, &clean);
            assert_eq!(params["fileName"], "photo.jpg");
            assert_eq!(params["totalSize"], clean.len());
            assert!(params.get("checksum").is_none());
        }
        let finals = f.state.photo.finals.lock().unwrap().clone();
        assert_eq!(finals.len(), usize::from(mode == 10));
        if let Some(final_params) = finals.first() {
            assert_eq!(final_params["jcp"], "{\"is_original\":1}");
            assert_eq!(final_params["fileSize"], clean.len());
            assert_eq!(
                final_params["rawUrl"],
                "https://files.example.test/uploaded"
            );
            let db = rusqlite::Connection::open(f.profile.paths.data_dir.join("messages.sqlite"))
                .unwrap();
            let hash: String = db
                .query_row("SELECT checksum FROM photo_outbox", [], |row| row.get(0))
                .unwrap();
            assert_eq!(hash, format!("{:x}", Md5::digest(&clean)));
        }
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn photo_tls_online_completion_before_after_and_duplicate_upload_only_the_previewed_snapshot()
{
    for (mode, thread, new_route) in [(1, "d:22", true), (2, "g:22", false)] {
        let f = Fixture::new(Reply::PhotoUpload(mode));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.photo_session(new_route),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let selected = selected(&f, "source-noise.png", &noisy_png());
        let source_path = selected.path.clone();
        let photo = prepare(&commands, &mut events, selected, thread, 821)
            .await
            .unwrap();
        let bytes = prepared_bytes(&photo);
        assert!(bytes.len() > 64000);
        assert_eq!((photo.width(), photo.height()), (512, 256));
        fs::write(
            source_path,
            b"replaced after the immutable preview was ready",
        )
        .unwrap();
        commands
            .send(LiveCommand::SendPhoto {
                generation: 1,
                thread: thread.into(),
                photo,
                caption: "Ảnh từ bản xem trước".into(),
                request_id: 822,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut events, 822).await,
            FileTransferPhase::Accepted
        );
        let uploads = f.state.photo.uploads.lock().unwrap().clone();
        assert_eq!(
            uploads.len(),
            if mode == 2 {
                1
            } else {
                bytes.len().div_ceil(64000)
            }
        );
        let actual: Vec<u8> = uploads
            .iter()
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        assert_eq!(
            actual,
            if mode == 2 {
                bytes[..64000].to_vec()
            } else {
                bytes
            }
        );
        let paths = f.state.photo.paths.lock().unwrap().clone();
        let prefix = if thread.starts_with("g:") {
            "/api/group/"
        } else {
            "/api/message/"
        };
        assert!(paths.iter().any(|p| p
            == &format!(
                "{prefix}{}",
                if new_route {
                    "upload/photo"
                } else {
                    "photo_original/upload"
                }
            )));
        assert_eq!(
            paths
                .iter()
                .filter(|p| p.ends_with("/photo_original/send"))
                .count(),
            1
        );
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn photo_tls_own_echo_wins_lost_or_conflicting_http_and_unknown_jpeg_never_resends_on_restart(
) {
    for mode in [3, 4, 5, 6] {
        let f = Fixture::new(Reply::PhotoUpload(mode));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.photo_session(false),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let original = oriented_gps_jpeg();
        let photo = prepare(
            &commands,
            &mut events,
            selected(&f, "source.jpg", &original),
            "d:22",
            831,
        )
        .await
        .unwrap();
        let retained = photo.clone();
        commands
            .send(LiveCommand::SendPhoto {
                generation: 1,
                thread: "d:22".into(),
                photo,
                caption: "Ảnh nguyên bản".into(),
                request_id: 832,
            })
            .await
            .unwrap();
        assert_eq!(
            terminal(&mut events, 832).await,
            if mode == 3 {
                FileTransferPhase::Unknown
            } else {
                FileTransferPhase::Accepted
            },
            "mode{mode}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        if mode == 3 {
            let uploaded = f.state.photo.uploads.lock().unwrap().len();
            commands
                .send(LiveCommand::SendPhoto {
                    generation: 1,
                    thread: "d:22".into(),
                    photo: retained,
                    caption: "Chú thích mới vẫn cùng JPEG".into(),
                    request_id: 833,
                })
                .await
                .unwrap();
            assert_eq!(terminal(&mut events, 833).await, FileTransferPhase::Failed);
            assert_eq!(f.state.photo.uploads.lock().unwrap().len(), uploaded);
        }
        stop.send(true).unwrap();
        task.await.unwrap();
        let c =
            rusqlite::Connection::open(f.profile.paths.data_dir.join("messages.sqlite")).unwrap();
        let (state, server): (String, Option<String>) = c
            .query_row("SELECT state,server_id FROM photo_outbox", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(state, if mode == 3 { "unknown" } else { "accepted" });
        assert_eq!(
            server.as_deref(),
            if mode == 3 { None } else { Some("901") }
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM attachment_outbox", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(c);
        if mode == 3 {
            let (commands, rx) = mpsc::channel(16);
            let (tx, mut events) = mpsc::channel(64);
            let (stop, cancel) = watch::channel(false);
            let task = tokio::spawn(run_with_transports(
                f.profile.clone(),
                f.photo_session(false),
                rx,
                tx,
                Arc::new(|| {}),
                cancel,
                f.transports(),
            ));
            online(&mut events).await;
            let photo = prepare(
                &commands,
                &mut events,
                selected(&f, "source.jpg", &original),
                "d:22",
                834,
            )
            .await
            .unwrap();
            commands
                .send(LiveCommand::SendPhoto {
                    generation: 1,
                    thread: "d:22".into(),
                    photo,
                    caption: "Khởi động lại vẫn không tự gửi".into(),
                    request_id: 835,
                })
                .await
                .unwrap();
            assert_eq!(terminal(&mut events, 835).await, FileTransferPhase::Failed);
            assert_eq!(f.state.photo.finals.lock().unwrap().len(), 1);
            stop.send(true).unwrap();
            task.await.unwrap();
        }
    }
}

async fn stalled(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while !f.state.photo.stalled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn photo_tls_cancel_privacy_and_encryption_never_claim_unsent_after_final_dispatch() {
    for scenario in [7, 8, 9, 21, 27] {
        let mode = if scenario == 21 {
            0
        } else if scenario == 27 {
            7
        } else {
            scenario
        };
        let f = Fixture::new(Reply::PhotoUpload(mode));
        if scenario == 21 {
            f.state.eligibility_mode.store(21, Ordering::SeqCst);
        }
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.photo_session(false),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let photo = prepare(
            &commands,
            &mut events,
            selected(&f, "source.jpg", &oriented_gps_jpeg()),
            "d:22",
            841,
        )
        .await
        .unwrap();
        commands
            .send(LiveCommand::SendPhoto {
                generation: 1,
                thread: "d:22".into(),
                photo,
                caption: "Giữ chú thích".into(),
                request_id: 842,
            })
            .await
            .unwrap();
        if scenario != 21 {
            stalled(&f).await;
            if scenario == 27 {
                f.state.privacy_policy.store(1, Ordering::SeqCst);
                commands.send(LiveCommand::RefreshPrivacy).await.unwrap();
            } else {
                commands
                    .send(LiveCommand::CancelFile { request_id: 842 })
                    .await
                    .unwrap();
            }
        }
        assert_eq!(
            terminal(&mut events, 842).await,
            match scenario {
                9 => FileTransferPhase::Unknown,
                21 => FileTransferPhase::Failed,
                _ => FileTransferPhase::Cancelled,
            },
            "scenario{scenario}"
        );
        assert_eq!(
            f.state.photo.finals.lock().unwrap().len(),
            usize::from(scenario == 9)
        );
        if scenario == 8 || scenario == 21 {
            assert!(f.state.photo.uploads.lock().unwrap().is_empty());
        }
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn photo_tls_heartbeat_and_history_remain_live_while_jpeg_http_waits_and_reconnect_cancels() {
    let f = Fixture::new(Reply::PhotoUpload(7));
    let mut transports = f.transports();
    transports.http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_der(&material().root).unwrap())
        .resolve("wpa.zaloapp.com", f.addr)
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.photo_session(false),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        transports,
    ));
    online(&mut events).await;
    let photo = prepare(
        &commands,
        &mut events,
        selected(&f, "source.jpg", &oriented_gps_jpeg()),
        "d:22",
        851,
    )
    .await
    .unwrap();
    commands
        .send(LiveCommand::SendPhoto {
            generation: 1,
            thread: "d:22".into(),
            photo,
            caption: "Cooperative JPEG".into(),
            request_id: 852,
        })
        .await
        .unwrap();
    stalled(&f).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.state.pings.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    commands
        .send(LiveCommand::Load {
            generation: 992,
            thread: "d:22".into(),
            before: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match events.recv().await.unwrap() {
                LiveEvent::Page {
                    generation: 992, ..
                } => break,
                LiveEvent::FileTransfer {
                    kind: TransferKind::Photo,
                    phase:
                        FileTransferPhase::Failed
                        | FileTransferPhase::Cancelled
                        | FileTransferPhase::Unknown
                        | FileTransferPhase::Accepted
                        | FileTransferPhase::AcceptedLocalFailure,
                    ..
                }
                | LiveEvent::FileAccepted {
                    kind: TransferKind::Photo,
                    ..
                } => panic!("Photo terminated before heartbeat/history proof"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let c = rusqlite::Connection::open_with_flags(
        f.profile.paths.data_dir.join("messages.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        c.query_row("SELECT state FROM photo_outbox", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "uploading"
    );
    drop(c);
    assert!(!f.state.photo.final_started.load(Ordering::SeqCst));
    commands.send(LiveCommand::Reconnect).await.unwrap();
    assert_eq!(
        terminal(&mut events, 852).await,
        FileTransferPhase::Cancelled
    );
    stop.send(true).unwrap();
    task.await.unwrap();
}

fn rgba_input(format: ImageFormat) -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(48, 24, |_, y| {
        if y < 12 {
            image::Rgba([255, 255, 255, 0])
        } else {
            image::Rgba([0, 255, 0, 128])
        }
    });
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}
fn animated_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255])
            .unwrap();
        writer
            .write_image_data(&[0, 255, 0, 255, 255, 255, 0, 255])
            .unwrap();
        writer.finish().unwrap();
    }
    assert!(
        image::codecs::png::PngDecoder::new(std::io::Cursor::new(&bytes))
            .unwrap()
            .is_apng()
            .unwrap()
    );
    bytes
}
fn animated_webp() -> Vec<u8> {
    fn chunk(data: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        data.extend_from_slice(kind);
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(body);
        if !body.len().is_multiple_of(2) {
            data.push(0);
        }
    }
    let mut body = b"WEBP".to_vec();
    chunk(&mut body, b"VP8X", &[2, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    chunk(&mut body, b"ANIM", &[0; 6]);
    for color in [[255, 0, 0], [0, 0, 255]] {
        let mut still = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 1, image::Rgb(color)))
            .write_to(&mut std::io::Cursor::new(&mut still), ImageFormat::WebP)
            .unwrap();
        let mut frame = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 50, 0, 0, 0];
        frame.extend_from_slice(&still[12..]);
        chunk(&mut body, b"ANMF", &frame);
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend(body);
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
    assert!(decoder.has_animation());
    use image::AnimationDecoder;
    assert_eq!(decoder.into_frames().collect_frames().unwrap().len(), 2);
    bytes
}
fn pixel_bomb_header() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 1))
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes[16..20].copy_from_slice(&5000u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&4000u32.to_be_bytes());
    let mut crc = u32::MAX;
    for b in &bytes[12..29] {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    bytes
}

#[tokio::test]
async fn photo_preparation_accepts_static_alpha_formats_and_rejects_real_animation_and_input_bombs()
{
    let f = Fixture::new(Reply::PhotoUpload(0));
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.photo_session(false),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    online(&mut events).await;
    for (format, name, id) in [
        (ImageFormat::Png, "alpha.png", 861),
        (ImageFormat::WebP, "alpha.webp", 862),
    ] {
        let photo = prepare(
            &commands,
            &mut events,
            selected(&f, name, &rgba_input(format)),
            "d:22",
            id,
        )
        .await
        .unwrap();
        assert_eq!((photo.width(), photo.height()), (48, 24));
        let exact = image::load_from_memory_with_format(&prepared_bytes(&photo), ImageFormat::Jpeg)
            .unwrap()
            .into_rgba8();
        let top = exact.get_pixel(24, 3);
        assert!(top[0] < 5 && top[1] < 5 && top[2] < 5);
        let bottom = exact.get_pixel(24, 20);
        assert!(bottom[0] < 8 && (120..=136).contains(&bottom[1]) && bottom[2] < 8);
        assert_eq!(exact.as_raw().as_slice(), photo.thumbnail().rgba.as_ref());
        drop(photo);
    }
    let mut gif = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 1))
        .write_to(&mut std::io::Cursor::new(&mut gif), ImageFormat::Gif)
        .unwrap();
    for (name, bytes, expected, id) in [
        (
            "animation.png",
            animated_png(),
            PhotoPreparationFailure::Animated,
            863,
        ),
        (
            "animation.webp",
            animated_webp(),
            PhotoPreparationFailure::Animated,
            864,
        ),
        (
            "unsupported.gif",
            gif,
            PhotoPreparationFailure::InvalidFormat,
            865,
        ),
        (
            "pixel-bomb.png",
            pixel_bomb_header(),
            PhotoPreparationFailure::Resource,
            866,
        ),
    ] {
        assert_eq!(
            prepare(
                &commands,
                &mut events,
                selected(&f, name, &bytes),
                "d:22",
                id
            )
            .await
            .unwrap_err(),
            expected,
            "{name}"
        );
    }
    let path = f.scratch.join("too-large.jpg");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(20 * 1024 * 1024 + 1).unwrap();
    drop(file);
    let a = Attachment::inspect(&path).unwrap();
    assert_eq!(
        prepare(&commands, &mut events, a, "d:22", 867)
            .await
            .unwrap_err(),
        PhotoPreparationFailure::Resource
    );
    let a = selected(&f, "changed.png", &rgba_input(ImageFormat::Png));
    fs::write(&a.path, b"source replaced before preparation").unwrap();
    assert_eq!(
        prepare(&commands, &mut events, a, "d:22", 868)
            .await
            .unwrap_err(),
        PhotoPreparationFailure::Changed
    );
    assert!(f.state.photo.uploads.lock().unwrap().is_empty());
    assert!(f.state.photo.finals.lock().unwrap().is_empty());
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn photo_preparation_aspect_fit_and_server_byte_caps_are_enforced_before_any_upload() {
    for size in [10000, 1] {
        let f = Fixture::new(Reply::PhotoUpload(0));
        let (commands, rx) = mpsc::channel(16);
        let (tx, mut events) = mpsc::channel(64);
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(run_with_transports(
            f.profile.clone(),
            f.photo_session_with_limits(false, 80, 40, size),
            rx,
            tx,
            Arc::new(|| {}),
            cancel,
            f.transports(),
        ));
        online(&mut events).await;
        let result = prepare(
            &commands,
            &mut events,
            selected(&f, "aspect.png", &noisy_png()),
            "d:22",
            871,
        )
        .await;
        if size == 1 {
            assert_eq!(result.unwrap_err(), PhotoPreparationFailure::Resource);
        } else {
            let photo = result.unwrap();
            assert_eq!((photo.width(), photo.height()), (80, 40));
            assert!(photo.size_bytes() <= size);
            assert_eq!(
                (photo.thumbnail().width, photo.thumbnail().height),
                (80, 40)
            );
        }
        assert!(f.state.photo.uploads.lock().unwrap().is_empty());
        stop.send(true).unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn photo_tls_private_preview_is_bound_to_its_thread_and_worker_lease() {
    let f = Fixture::new(Reply::PhotoUpload(0));
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.photo_session(false),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    online(&mut events).await;
    let photo = prepare(
        &commands,
        &mut events,
        selected(&f, "scoped.jpg", &oriented_gps_jpeg()),
        "d:22",
        881,
    )
    .await
    .unwrap();
    commands
        .send(LiveCommand::SendPhoto {
            generation: 1,
            thread: "d:33".into(),
            photo: photo.clone(),
            caption: "wrong thread".into(),
            request_id: 882,
        })
        .await
        .unwrap();
    assert_eq!(terminal(&mut events, 882).await, FileTransferPhase::Failed);
    assert!(f.state.photo.uploads.lock().unwrap().is_empty());
    stop.send(true).unwrap();
    task.await.unwrap();
    let (commands, rx) = mpsc::channel(16);
    let (tx, mut events) = mpsc::channel(64);
    let (stop, cancel) = watch::channel(false);
    let task = tokio::spawn(run_with_transports(
        f.profile.clone(),
        f.photo_session(false),
        rx,
        tx,
        Arc::new(|| {}),
        cancel,
        f.transports(),
    ));
    online(&mut events).await;
    commands
        .send(LiveCommand::SendPhoto {
            generation: 1,
            thread: "d:22".into(),
            photo,
            caption: "old worker capability".into(),
            request_id: 883,
        })
        .await
        .unwrap();
    assert_eq!(terminal(&mut events, 883).await, FileTransferPhase::Failed);
    assert!(f.state.photo.uploads.lock().unwrap().is_empty());
    let fresh = prepare(
        &commands,
        &mut events,
        selected(&f, "scoped.jpg", &oriented_gps_jpeg()),
        "d:22",
        884,
    )
    .await
    .unwrap();
    commands
        .send(LiveCommand::SendPhoto {
            generation: 1,
            thread: "d:22".into(),
            photo: fresh,
            caption: "fresh matching preview".into(),
            request_id: 885,
        })
        .await
        .unwrap();
    assert_eq!(
        terminal(&mut events, 885).await,
        FileTransferPhase::Accepted
    );
    assert_eq!(f.state.photo.finals.lock().unwrap().len(), 1);
    stop.send(true).unwrap();
    task.await.unwrap();
}
