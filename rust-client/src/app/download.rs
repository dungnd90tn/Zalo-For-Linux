//! Explicit, bounded download of an attachment or original photo received in a conversation.
//!
//! A download only starts after the user presses the save button on a file/photo card or its viewer. The URL must pass the
//! same CDN policy as thumbnails (HTTPS, `*.zdn.vn`/`*.zadn.vn`, no credentials, no redirects).
//! Bytes stream to a private `.part` file, are capped, and are renamed to a collision-free name only
//! once complete, so a failed or cancelled transfer never leaves a partial file under the final name
//! and an existing file is never overwritten. The file is never opened or executed.
use super::media::{thumbnail_url, MediaKey};
use anyhow::Result;
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch, Notify, Semaphore};

/// The original treats files up to this size as "small" (direct upload); larger ones use Zalo Cloud.
pub const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;
const REQUESTS: usize = 8;
const ACTIVE: usize = 2;
const EVENTS: usize = 4;
const MAX_NAME_BYTES: usize = 200;
const FALLBACK_NAME: &str = "tep-dinh-kem";
const FOLDER: &str = "Zalo Native";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadFailure {
    Origin,
    Network,
    Http(u16),
    TooLarge,
    Disk,
    Cancelled,
}
impl std::fmt::Display for DownloadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Origin => f.write_str("Liên kết tệp không nằm trên CDN Zalo nên không được tải."),
            Self::Network => {
                f.write_str("Chưa tải được tệp (lỗi mạng hoặc tệp bị cắt). Bạn có thể thử lại.")
            }
            Self::Http(status) => write!(f, "Máy chủ từ chối tải tệp (HTTP {status})."),
            Self::TooLarge => f.write_str("Tệp lớn hơn 100 MB nên không được tải trực tiếp."),
            Self::Disk => f.write_str("Không lưu được tệp vào thư mục Tải xuống."),
            Self::Cancelled => f.write_str("Đã hủy tải."),
        }
    }
}

/// Shared between the worker and the card so the UI can draw a bar without any event flood.
#[derive(Default)]
pub struct Progress {
    received: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    wake: Notify,
}
impl Progress {
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }
    /// `None` while the server has not announced a length.
    pub fn total(&self) -> Option<u64> {
        Some(self.total.load(Ordering::Relaxed)).filter(|total| *total > 0)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }
    async fn wait_cancelled(&self) {
        // notify_one retains a permit if cancellation races the first poll.
        if !self.cancelled() {
            self.wake.notified().await;
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub enum DownloadEvent {
    Done {
        key: MediaKey,
        path: PathBuf,
        bytes: u64,
    },
    Failed {
        key: MediaKey,
        reason: DownloadFailure,
    },
}
struct Request {
    key: MediaKey,
    name: String,
    progress: Arc<Progress>,
}
pub struct FileDownloader {
    account_id: String,
    commands: mpsc::Sender<Request>,
    events: mpsc::Receiver<DownloadEvent>,
    cancel: watch::Sender<bool>,
    pending: HashMap<MediaKey, Arc<Progress>>,
}
impl FileDownloader {
    pub fn new(account_id: String, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        crate::domain::validate_id(&account_id)?;
        let (commands, receiver) = mpsc::channel(REQUESTS);
        let (sender, events) = mpsc::channel(EVENTS);
        let (cancel, stop) = watch::channel(false);
        std::thread::Builder::new()
            .name("zalo-file-download".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    // No cookies and no redirects, like the thumbnail client. A stalled body is cut
                    // by the read timeout rather than a total timeout so large files can finish.
                    let Ok(client) = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(8))
                        .read_timeout(Duration::from_secs(30))
                        .user_agent("ZaloPC-linux-24v681")
                        .build()
                    else {
                        return;
                    };
                    worker(client, receiver, sender, stop, repaint).await;
                });
            })?;
        Ok(Self {
            account_id,
            commands,
            events,
            cancel,
            pending: HashMap::new(),
        })
    }
    /// Starts one transfer. `None` means refused (wrong account, bad ids, already running, queue full).
    pub fn start(&mut self, key: MediaKey, name: &str) -> Option<Arc<Progress>> {
        if key.account_id != self.account_id
            || crate::domain::validate_id(&key.thread_id).is_err()
            || crate::domain::validate_id(&key.message_id).is_err()
            || key.url.is_empty()
            || key.url.len() > 8192
            || self.pending.len() >= REQUESTS
            || self.pending.contains_key(&key)
        {
            return None;
        }
        let progress = Arc::new(Progress::default());
        self.commands
            .try_send(Request {
                key: key.clone(),
                name: name.into(),
                progress: progress.clone(),
            })
            .ok()?;
        self.pending.insert(key, progress.clone());
        Some(progress)
    }
    pub fn try_event(&mut self) -> Option<DownloadEvent> {
        let event = match self.events.try_recv() {
            Ok(event) => event,
            Err(mpsc::error::TryRecvError::Empty) => return None,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                let key = self.pending.keys().next()?.clone();
                DownloadEvent::Failed {
                    key,
                    reason: DownloadFailure::Network,
                }
            }
        };
        match &event {
            DownloadEvent::Done { key, .. } | DownloadEvent::Failed { key, .. } => {
                self.pending.remove(key);
            }
        }
        Some(event)
    }
    pub fn is_closed(&self) -> bool {
        self.commands.is_closed()
    }
}
impl Drop for FileDownloader {
    fn drop(&mut self) {
        for progress in self.pending.values() {
            progress.cancel();
        }
        self.cancel.send_replace(true);
    }
}

async fn worker(
    client: reqwest::Client,
    mut commands: mpsc::Receiver<Request>,
    events: mpsc::Sender<DownloadEvent>,
    mut cancel: watch::Receiver<bool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let slots = Arc::new(Semaphore::new(ACTIVE));
    loop {
        let request = tokio::select! {biased;
            _ = cancel.changed() => break,
            request = commands.recv() => match request {Some(request) => request, None => break},
        };
        let (client, events, repaint, slots, mut stop) = (
            client.clone(),
            events.clone(),
            repaint.clone(),
            slots.clone(),
            cancel.clone(),
        );
        tokio::spawn(async move {
            let result = tokio::select! {biased;
                _ = stop.changed() => Err(DownloadFailure::Cancelled),
                _ = request.progress.wait_cancelled() => Err(DownloadFailure::Cancelled),
                result = async {
                    let _permit = slots.acquire().await.map_err(|_| DownloadFailure::Cancelled)?;
                    run(&client, &request).await
                } => result,
            };
            let event = match result {
                Ok((path, bytes)) => DownloadEvent::Done {
                    key: request.key,
                    path,
                    bytes,
                },
                Err(reason) => DownloadEvent::Failed {
                    key: request.key,
                    reason,
                },
            };
            let _ = events.send(event).await;
            repaint();
        });
    }
}

async fn run(
    client: &reqwest::Client,
    request: &Request,
) -> std::result::Result<(PathBuf, u64), DownloadFailure> {
    use crate::diagnostics::{Event, Kind, Reason, Stage};
    let result = async {
        let url = thumbnail_url(&request.key.url).map_err(|_| DownloadFailure::Origin)?;
        let dir = destination_dir().ok_or(DownloadFailure::Disk)?;
        stream_to_dir(client, url, &dir, &request.name, &request.progress).await
    }
    .await;
    // Only categories, a status code and a byte count: never the URL, name or path.
    let event = match &result {
        Ok((_, bytes)) => Event::new(Kind::Debug, Stage::MediaAssets, Reason::Ready)
            .http_status(Some(200))
            .bytes(*bytes as usize),
        Err(failure) => Event::new(
            Kind::Failure,
            Stage::MediaAssets,
            match failure {
                DownloadFailure::Origin => Reason::Unsupported,
                DownloadFailure::Network => Reason::Transport,
                DownloadFailure::Http(_) => Reason::HttpStatus,
                DownloadFailure::TooLarge => Reason::ResourceLimit,
                DownloadFailure::Disk => Reason::Io,
                DownloadFailure::Cancelled => Reason::Started,
            },
        )
        .http_status(match failure {
            DownloadFailure::Http(status) => Some(*status),
            _ => None,
        }),
    };
    crate::diagnostics::record(event);
    result
}

/// Removes the `.part` file unless the transfer finished.
struct Part {
    path: PathBuf,
    keep: bool,
}
impl Drop for Part {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Streams an already validated URL into `dir`. Split from `run` so tests can use a local server.
pub(crate) async fn stream_to_dir(
    client: &reqwest::Client,
    url: url::Url,
    dir: &Path,
    name: &str,
    progress: &Progress,
) -> std::result::Result<(PathBuf, u64), DownloadFailure> {
    tokio::select! {biased;
        _ = progress.wait_cancelled() => Err(DownloadFailure::Cancelled),
        result = stream_to_dir_inner(client, url, dir, name, progress) => result,
    }
}

async fn stream_to_dir_inner(
    client: &reqwest::Client,
    url: url::Url,
    dir: &Path,
    name: &str,
    progress: &Progress,
) -> std::result::Result<(PathBuf, u64), DownloadFailure> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| DownloadFailure::Network)?;
    // No range was requested: accepting 206 could silently save a truncated original.
    if response.status() != reqwest::StatusCode::OK {
        return Err(DownloadFailure::Http(response.status().as_u16()));
    }
    let total = response.content_length();
    if total.is_some_and(|total| total > MAX_DOWNLOAD_BYTES) {
        return Err(DownloadFailure::TooLarge);
    }
    progress.total.store(total.unwrap_or(0), Ordering::Relaxed);
    create_dir(dir).map_err(|_| DownloadFailure::Disk)?;
    let part_path = dir.join(format!(
        ".zalo-download-{:016x}.part",
        rand::random::<u64>()
    ));
    let mut file = create_private(&part_path).map_err(|_| DownloadFailure::Disk)?;
    let mut part = Part {
        path: part_path,
        keep: false,
    };
    let mut received = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| DownloadFailure::Network)?
    {
        if progress.cancelled() {
            return Err(DownloadFailure::Cancelled);
        }
        received += chunk.len() as u64;
        if received > MAX_DOWNLOAD_BYTES {
            return Err(DownloadFailure::TooLarge);
        }
        file.write_all(&chunk).map_err(|_| DownloadFailure::Disk)?;
        progress.received.store(received, Ordering::Relaxed);
    }
    if total.is_some_and(|total| total != received) {
        return Err(DownloadFailure::Network);
    }
    if progress.cancelled() {
        return Err(DownloadFailure::Cancelled);
    }
    file.sync_all().map_err(|_| DownloadFailure::Disk)?;
    drop(file);
    let final_path = reserve_and_move(&part.path, dir, &safe_file_name(name))?;
    part.keep = true;
    Ok((final_path, received))
}
fn create_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o755);
    }
    builder.create(dir)
}
/// Claims a free name with `create_new` (never overwrites), then renames the finished part over it.
fn reserve_and_move(
    part: &Path,
    dir: &Path,
    name: &str,
) -> std::result::Result<PathBuf, DownloadFailure> {
    let (stem, extension) = match name.rfind('.') {
        Some(index) if index > 0 => name.split_at(index),
        _ => (name, ""),
    };
    for attempt in 0..1000u32 {
        let candidate = if attempt == 0 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({attempt}){extension}"))
        };
        match create_private(&candidate) {
            Ok(reserved) => {
                drop(reserved);
                return match std::fs::rename(part, &candidate) {
                    Ok(()) => Ok(candidate),
                    Err(_) => {
                        let _ = std::fs::remove_file(&candidate);
                        Err(DownloadFailure::Disk)
                    }
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(DownloadFailure::Disk),
        }
    }
    Err(DownloadFailure::Disk)
}

/// Peer-controlled title to a single safe file name: no separators, controls, bidirectional
/// overrides, leading dots, or more than 200 bytes (the extension is kept when truncating).
pub fn safe_file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069 | 0xFEFF)
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        return FALLBACK_NAME.into();
    }
    if cleaned.len() <= MAX_NAME_BYTES {
        return cleaned;
    }
    let extension = cleaned
        .rfind('.')
        .map(|index| &cleaned[index..])
        .filter(|ext| ext.len() <= 16)
        .unwrap_or("");
    let mut end = MAX_NAME_BYTES - extension.len();
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &cleaned[..end], extension)
}

/// `$XDG_DOWNLOAD_DIR`, then `user-dirs.dirs`, then `~/Downloads`, plus a `Zalo Native` folder.
pub fn destination_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let user_dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).ok();
    resolve_destination(
        std::env::var_os("XDG_DOWNLOAD_DIR").map(PathBuf::from),
        user_dirs.as_deref(),
        &home,
    )
}
fn resolve_destination(
    environment: Option<PathBuf>,
    user_dirs: Option<&str>,
    home: &Path,
) -> Option<PathBuf> {
    let from_file = user_dirs.and_then(|text| {
        let value = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?
            .trim()
            .trim_matches('"');
        match value.strip_prefix("$HOME") {
            Some(rest) => Some(home.join(rest.trim_start_matches('/'))),
            None => Some(PathBuf::from(value)),
        }
    });
    let base = environment
        .into_iter()
        .chain(from_file)
        // A relative path or the bare home directory means "unset" for xdg-user-dirs.
        .find(|path| path.is_absolute() && path != home)
        .unwrap_or_else(|| home.join("Downloads"));
    Some(base.join(FOLDER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn names_are_reduced_to_one_safe_component() {
        assert_eq!(safe_file_name("báo cáo.pdf"), "báo cáo.pdf");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\x\\a.docx"), "a.docx");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
        assert_eq!(safe_file_name("..."), FALLBACK_NAME);
        assert_eq!(safe_file_name("a\0b\n.txt"), "ab.txt");
        assert_eq!(safe_file_name("photo\u{202e}gpj.exe"), "photogpj.exe");
        assert_eq!(safe_file_name("   "), FALLBACK_NAME);
        let long = format!("{}.pdf", "đ".repeat(300));
        let name = safe_file_name(&long);
        assert!(name.len() <= MAX_NAME_BYTES && name.ends_with(".pdf"));
    }

    #[test]
    fn destination_prefers_environment_then_user_dirs_then_downloads() {
        let home = Path::new("/home/u");
        let dirs = "# c\nXDG_DOWNLOAD_DIR=\"$HOME/Tải về\"\n";
        assert_eq!(
            resolve_destination(Some("/data/dl".into()), Some(dirs), home),
            Some("/data/dl/Zalo Native".into())
        );
        assert_eq!(
            resolve_destination(None, Some(dirs), home),
            Some("/home/u/Tải về/Zalo Native".into())
        );
        assert_eq!(
            resolve_destination(Some("relative".into()), None, home),
            Some("/home/u/Downloads/Zalo Native".into())
        );
        assert_eq!(
            resolve_destination(None, Some("XDG_DOWNLOAD_DIR=\"$HOME/\"\n"), home),
            Some("/home/u/Downloads/Zalo Native".into())
        );
    }

    #[test]
    fn only_zalo_cdn_https_urls_are_downloadable() {
        assert!(thumbnail_url("https://f1-zpc.zdn.vn/abc/file.pdf").is_ok());
        for bad in [
            "http://evil.example/file.pdf",
            "https://evil.example/file.pdf",
            "https://zdn.vn.evil.example/f",
            "file:///etc/passwd",
            "https://u:p@f1.zdn.vn/f",
        ] {
            assert!(thumbnail_url(bad).is_err(), "{bad}");
        }
    }

    /// One-shot plain-HTTP server answering with a fixed head and body.
    async fn serve(head: &'static str, body: Vec<u8>) -> url::Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 1024];
            let _ = socket.read(&mut buffer).await;
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        });
        url::Url::parse(&format!("http://{address}/f")).unwrap()
    }
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zalo-dl-{tag}-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn saves_without_overwriting_and_leaves_no_part_file() {
        let dir = scratch("ok");
        std::fs::write(dir.join("a.txt"), b"existing").unwrap();
        let url = serve(
            "HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\n",
            b"hello".to_vec(),
        )
        .await;
        let progress = Progress::default();
        let (path, bytes) = stream_to_dir(&client(), url, &dir, "a.txt", &progress)
            .await
            .unwrap();
        assert_eq!(
            (path.file_name().unwrap().to_str().unwrap(), bytes),
            ("a (1).txt", 5)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"existing");
        assert_eq!((progress.received(), progress.total()), (5, Some(5)));
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o111,
                0
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn refusals_and_truncation_leave_nothing_behind() {
        let dir = scratch("fail");
        let denied = serve(
            "HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            vec![],
        )
        .await;
        assert_eq!(
            stream_to_dir(&client(), denied, &dir, "a.txt", &Progress::default())
                .await
                .unwrap_err(),
            DownloadFailure::Http(403)
        );
        let huge = serve(
            "HTTP/1.1 200 OK\r\ncontent-length: 104857601\r\nconnection: close\r\n\r\n",
            vec![],
        )
        .await;
        assert_eq!(
            stream_to_dir(&client(), huge, &dir, "a.txt", &Progress::default())
                .await
                .unwrap_err(),
            DownloadFailure::TooLarge
        );
        let short = serve(
            "HTTP/1.1 200 OK\r\ncontent-length: 10\r\nconnection: close\r\n\r\n",
            b"abc".to_vec(),
        )
        .await;
        assert_eq!(
            stream_to_dir(&client(), short, &dir, "a.txt", &Progress::default())
                .await
                .unwrap_err(),
            DownloadFailure::Network
        );
        let cancelled = serve(
            "HTTP/1.1 200 OK\r\ncontent-length: 3\r\nconnection: close\r\n\r\n",
            b"abc".to_vec(),
        )
        .await;
        let progress = Progress::default();
        progress.cancel();
        assert_eq!(
            stream_to_dir(&client(), cancelled, &dir, "a.txt", &progress)
                .await
                .unwrap_err(),
            DownloadFailure::Cancelled
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn original_photo_bytes_are_saved_unchanged_and_partial_responses_are_refused() {
        let dir = scratch("photo");
        let bytes = include_bytes!("../../tests/fixtures/media/photo.jxl").to_vec();
        // Original JXL bytes are saved, not the decoded/resized preview.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = bytes.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut input = [0; 1024];
            let _ = socket.read(&mut input).await;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&body).await.unwrap();
        });
        let (path, _) = stream_to_dir(
            &client(),
            url::Url::parse(&format!("http://{address}/photo.jxl")).unwrap(),
            &dir,
            "anh-1.jxl",
            &Progress::default(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        let partial = serve(
            "HTTP/1.1 206 Partial Content\r\ncontent-length: 3\r\nconnection: close\r\n\r\n",
            b"abc".to_vec(),
        )
        .await;
        assert_eq!(
            stream_to_dir(
                &client(),
                partial,
                &dir,
                "partial.jpg",
                &Progress::default()
            )
            .await
            .unwrap_err(),
            DownloadFailure::Http(206)
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_body_and_removes_the_part_file() {
        let dir = scratch("stalled");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (written, ready) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 1000\r\n\r\nx")
                .await
                .unwrap();
            let _ = written.send(());
            let _ = socket.read(&mut request).await;
        });
        let progress = Arc::new(Progress::default());
        let running = progress.clone();
        let destination = dir.clone();
        let task = tokio::spawn(async move {
            stream_to_dir(
                &client(),
                url::Url::parse(&format!("http://{address}/original.jpg")).unwrap(),
                &destination,
                "photo.jpg",
                &running,
            )
            .await
        });
        ready.await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while progress.received() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        progress.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            DownloadFailure::Cancelled
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn cancelling_before_start_does_not_connect_or_create_any_file() {
        let dir = scratch("not-started");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = url::Url::parse(&format!(
            "http://{}/photo.jpg",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let progress = Progress::default();
        progress.cancel();
        assert_eq!(
            stream_to_dir(&client(), url, &dir, "photo.jpg", &progress)
                .await
                .unwrap_err(),
            DownloadFailure::Cancelled
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn downloader_refuses_foreign_accounts_duplicates_and_bad_ids() {
        let mut downloader =
            FileDownloader::new("11".into(), Arc::new(|| {})).expect("worker thread");
        let key = |account: &str, thread: &str, url: &str| MediaKey {
            account_id: account.into(),
            thread_id: thread.into(),
            message_id: "9".into(),
            url: url.into(),
        };
        // The origin check happens in the worker; nothing here reaches the network because the
        // host is not on the CDN allowlist, so it fails as `Origin` without a request.
        assert!(downloader
            .start(key("22", "u:5", "https://f.zdn.vn/x"), "a")
            .is_none());
        assert!(downloader
            .start(key("11", "", "https://f.zdn.vn/x"), "a")
            .is_none());
        assert!(downloader.start(key("11", "u:5", ""), "a").is_none());
        let first = key("11", "u:5", "https://evil.example/x");
        assert!(downloader.start(first.clone(), "a").is_some());
        assert!(downloader.start(first, "a").is_none());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(DownloadEvent::Failed { reason, .. }) = downloader.try_event() {
                assert_eq!(reason, DownloadFailure::Origin);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no event");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
