//! Optional Linux audio worker. PCM and MP3 stay off disk and out of logs.
use crate::domain::media::voice_url;
use std::{
    ffi::OsString,
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, TryRecvError},
        Arc, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::watch;

pub const MAX_VOICE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_VOICE_DURATION_MS: u32 = 60_000;
const MIN_VOICE_DURATION_MS: u32 = 120;
const MAX_PLAYBACK_BYTES: usize = 8 * 1024 * 1024;
const DOWNLOAD_LIMIT: Duration = Duration::from_secs(10);
const PLAYBACK_DEADLINE: Duration = Duration::from_secs(315);
const RECORDING_DEADLINE: Duration = Duration::from_secs(65);
const STOP_GRACE: Duration = Duration::from_secs(2);
const CHILD_POLL: Duration = Duration::from_millis(25);

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct PreparedVoice {
    bytes: Arc<[u8]>,
    duration_ms: u32,
}

impl PreparedVoice {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    #[cfg(test)]
    pub(crate) fn fixture(bytes: Vec<u8>, duration_ms: u32) -> Self {
        Self {
            bytes: bytes.into(),
            duration_ms,
        }
    }
}

impl fmt::Debug for PreparedVoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedVoice")
            .field("size", &self.bytes.len())
            .field("duration_ms", &self.duration_ms)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AudioError {
    Unavailable,
    Device,
    TooShort,
    Invalid,
    Download,
    Timeout,
    Cancelled,
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "Cần FFmpeg và máy chủ âm thanh tương thích PulseAudio.",
            Self::Device => "Không mở được thiết bị âm thanh.",
            Self::TooShort => "Đoạn ghi âm quá ngắn.",
            Self::Invalid => "Tệp âm thanh có định dạng hoặc kích thước không hợp lệ.",
            Self::Download => "Chưa tải được tin nhắn thoại.",
            Self::Timeout => "Đã hết thời gian xử lý âm thanh.",
            Self::Cancelled => "Đã hủy tác vụ âm thanh.",
        })
    }
}

impl std::error::Error for AudioError {}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Control {
    Running,
    Stop,
    Cancel,
}

struct Task<T> {
    control: watch::Sender<Control>,
    result: Receiver<Result<T, AudioError>>,
}

impl<T: Send + 'static> Task<T> {
    fn start(
        name: &str,
        wake: Wake,
        work: impl FnOnce(watch::Receiver<Control>) -> Result<T, AudioError> + Send + 'static,
    ) -> Result<Self, AudioError> {
        let (control, receiver) = watch::channel(Control::Running);
        let (sender, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                if sender.send(work(receiver)).is_ok() {
                    wake();
                }
            })
            .map_err(|_| AudioError::Unavailable)?;
        Ok(Self { control, result })
    }

    fn poll(&self) -> Option<Result<T, AudioError>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(AudioError::Device)),
        }
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.control.send_replace(Control::Cancel);
    }
}

pub struct Recorder(Task<PreparedVoice>);

impl Recorder {
    pub fn start(wake: Wake) -> Result<Self, AudioError> {
        let ffmpeg = ffmpeg().ok_or(AudioError::Unavailable)?;
        Task::start("zalo-voice-record", wake, move |control| {
            record(ffmpeg, &control)
        })
        .map(Self)
    }

    pub fn stop(&self) {
        self.0.control.send_replace(Control::Stop);
    }

    pub fn poll(&mut self) -> Option<Result<PreparedVoice, AudioError>> {
        self.0.poll()
    }
}

pub struct Player(Task<()>);

impl Player {
    pub fn start_local(recording: PreparedVoice, wake: Wake) -> Result<Self, AudioError> {
        let ffmpeg = ffmpeg().ok_or(AudioError::Unavailable)?;
        Task::start("zalo-voice-play", wake, move |control| {
            play(ffmpeg, recording.bytes(), &control)
        })
        .map(Self)
    }

    pub fn start_remote(url: &str, wake: Wake) -> Result<Self, AudioError> {
        let url = voice_url(url).ok_or(AudioError::Invalid)?;
        let ffmpeg = ffmpeg().ok_or(AudioError::Unavailable)?;
        Task::start("zalo-voice-play", wake, move |mut control| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| AudioError::Download)?;
            let bytes = runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = control.changed() => Err(AudioError::Cancelled),
                    result = fetch(&url) => result,
                }
            })?;
            play(ffmpeg, &bytes, &control)
        })
        .map(Self)
    }

    pub fn poll(&mut self) -> Option<Result<(), AudioError>> {
        self.0.poll()
    }
}

fn ffmpeg() -> Option<&'static Path> {
    static PROGRAM: OnceLock<Option<&'static Path>> = OnceLock::new();
    *PROGRAM.get_or_init(|| {
        ["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.is_file())
    })
}

pub fn recording_available() -> bool {
    ffmpeg().is_some()
}

pub fn playback_available() -> bool {
    ffmpeg().is_some()
}

fn pulse_server() -> OsString {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        // SAFETY: geteuid has no pointer arguments and only reads the process UID.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    let mut server = OsString::from("unix:");
    server.push(dir.join("pulse/native"));
    server
}

fn ffmpeg_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .args(["-hide_banner", "-loglevel", "error"])
        .env_remove("FFREPORT")
        .stderr(Stdio::null());
    command
}

// Own the child even through early I/O errors or a cancelled task.
struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn record(ffmpeg: &Path, control: &watch::Receiver<Control>) -> Result<PreparedVoice, AudioError> {
    let mut command = ffmpeg_command(ffmpeg);
    command
        .args(["-stdin", "-f", "pulse", "-server"])
        .arg(pulse_server())
        .args([
            "-name",
            "Zalo Native",
            "-sample_rate",
            "24000",
            "-channels",
            "1",
            "-i",
            "default",
        ]);
    mp3_output(&mut command);
    capture(command, control)
}

fn mp3_output(command: &mut Command) {
    command.args([
        // Leave room for the final encoder frames inside the one-minute limit.
        "-t",
        "59.9",
        "-ac",
        "1",
        "-ar",
        "24000",
        "-c:a",
        "libmp3lame",
        "-b:a",
        "48k",
        "-id3v2_version",
        "0",
        "-write_xing",
        "0",
        "-f",
        "mp3",
        "pipe:1",
    ]);
}

fn capture(
    mut command: Command,
    control: &watch::Receiver<Control>,
) -> Result<PreparedVoice, AudioError> {
    match *control.borrow() {
        Control::Cancel => return Err(AudioError::Cancelled),
        Control::Stop => return Err(AudioError::TooShort),
        Control::Running => {}
    }
    thread::scope(|scope| {
        let mut child = Reap(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|_| AudioError::Device)?,
        );
        let stdout = child.0.stdout.take().ok_or(AudioError::Device)?;
        let output = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take((MAX_VOICE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let deadline = Instant::now() + RECORDING_DEADLINE;
        let mut stop_at = None;
        loop {
            let now = Instant::now();
            match *control.borrow() {
                Control::Cancel => return Err(AudioError::Cancelled),
                Control::Stop if stop_at.is_none() => {
                    if let Some(mut stdin) = child.0.stdin.take() {
                        let _ = stdin.write_all(b"q\n");
                    }
                    stop_at = Some(now + STOP_GRACE);
                }
                _ => {}
            }
            if let Some(status) = child.0.try_wait().map_err(|_| AudioError::Device)? {
                if !status.success() {
                    return Err(AudioError::Device);
                }
                let bytes = output
                    .join()
                    .map_err(|_| AudioError::Device)?
                    .map_err(|_| AudioError::Device)?;
                let duration_ms = captured_duration(&bytes)?;
                return Ok(PreparedVoice {
                    bytes: bytes.into(),
                    duration_ms,
                });
            }
            if now >= deadline || stop_at.is_some_and(|limit| now >= limit) {
                return Err(AudioError::Timeout);
            }
            thread::sleep(CHILD_POLL);
        }
    })
}

fn captured_duration(bytes: &[u8]) -> Result<u32, AudioError> {
    // MPEG-2 Layer III at 48 kbps/24 kHz stores 576 samples in each 144-byte
    // frame. The command uses constant bitrate and disables stream tags.
    const FRAME_BYTES: usize = 144;
    const FRAME_MS: u32 = 24;
    let (frames, remainder) = bytes.as_chunks::<FRAME_BYTES>();
    if bytes.len() > MAX_VOICE_BYTES
        || !remainder.is_empty()
        || !frames.iter().all(|frame| {
            frame[0] == 0xff
                && frame[1] & 0xfe == 0xf2
                && frame[2] & 0xfe == 0x64
                && frame[3] & 0xc0 == 0xc0
        })
    {
        return Err(AudioError::Invalid);
    }
    let duration_ms = frames.len() as u32 * FRAME_MS;
    match duration_ms {
        ..MIN_VOICE_DURATION_MS => Err(AudioError::TooShort),
        MIN_VOICE_DURATION_MS..=MAX_VOICE_DURATION_MS => Ok(duration_ms),
        _ => Err(AudioError::Invalid),
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, AudioError> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(DOWNLOAD_LIMIT)
        .user_agent("ZaloPC-linux-24v681")
        .build()
        .map_err(|_| AudioError::Download)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| AudioError::Download)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(AudioError::Download);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PLAYBACK_BYTES as u64)
    {
        return Err(AudioError::Invalid);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| AudioError::Download)? {
        if bytes.len() + chunk.len() > MAX_PLAYBACK_BYTES {
            return Err(AudioError::Invalid);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn play(ffmpeg: &Path, bytes: &[u8], control: &watch::Receiver<Control>) -> Result<(), AudioError> {
    if bytes.is_empty() || bytes.len() > MAX_PLAYBACK_BYTES {
        return Err(AudioError::Invalid);
    }
    if *control.borrow() != Control::Running {
        return Err(AudioError::Cancelled);
    }
    let mut command = ffmpeg_command(ffmpeg);
    command.args([
        "-nostdin",
        "-protocol_whitelist",
        "pipe",
        "-format_whitelist",
        "mp3,aac,amr",
        "-i",
        "pipe:0",
        "-map",
        "0:a:0",
        "-vn",
        "-sn",
        "-dn",
        "-t",
        "300",
        "-f",
        "pulse",
        "-server",
    ]);
    command
        .arg(pulse_server())
        .args(["-name", "Zalo Native", "voice"]);
    thread::scope(|scope| {
        let mut child = Reap(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .map_err(|_| AudioError::Device)?,
        );
        let mut stdin = child.0.stdin.take().ok_or(AudioError::Device)?;
        scope.spawn(move || {
            // FFmpeg can close the pipe after the playback duration cap.
            let _ = stdin.write_all(bytes);
        });
        let deadline = Instant::now() + PLAYBACK_DEADLINE;
        loop {
            if *control.borrow() != Control::Running {
                return Err(AudioError::Cancelled);
            }
            if let Some(status) = child.0.try_wait().map_err(|_| AudioError::Device)? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(AudioError::Device)
                };
            }
            if Instant::now() >= deadline {
                return Err(AudioError::Timeout);
            }
            thread::sleep(CHILD_POLL);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_frames_enforce_clip_duration_and_format() {
        let frame = {
            let mut frame = [0; 144];
            frame[..4].copy_from_slice(&[0xff, 0xf3, 0x64, 0xc4]);
            frame
        };
        let mut bytes = frame.repeat(6);
        assert_eq!(captured_duration(&bytes), Ok(144));
        assert_eq!(captured_duration(&bytes[..144]), Err(AudioError::TooShort));
        assert_eq!(
            captured_duration(&frame.repeat(2501)),
            Err(AudioError::Invalid)
        );
        assert_eq!(captured_duration(&bytes[..145]), Err(AudioError::Invalid));
        bytes[145] = 0;
        assert_eq!(captured_duration(&bytes), Err(AudioError::Invalid));
    }

    #[test]
    #[ignore = "requires FFmpeg with lavfi and libmp3lame"]
    fn ffmpeg_synthetic_tone_encodes_in_memory_when_available() {
        let program = ffmpeg().expect("FFmpeg installed");
        let mut command = ffmpeg_command(program);
        command.args([
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=24000:duration=0.25",
        ]);
        mp3_output(&mut command);
        let (_sender, receiver) = watch::channel(Control::Running);
        let recording = capture(command, &receiver).expect("synthetic MP3");
        assert!((240..=350).contains(&recording.duration_ms()));
        assert_eq!(recording.bytes().len() % 144, 0);
    }

    #[test]
    fn remote_playback_rejects_untrusted_input_before_worker() {
        let wake = Arc::new(|| {});
        for url in [
            "https://example.test/voice.mp3",
            "https://zdn.vn.example.test/voice.mp3",
            "https://a.zdn.vn:8443/voice.mp3",
            "https://user@a.zdn.vn/voice.mp3",
            "file:///tmp/voice.mp3",
        ] {
            assert!(matches!(
                Player::start_remote(url, wake.clone()),
                Err(AudioError::Invalid)
            ));
        }
    }
}
