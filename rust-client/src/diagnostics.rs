//! Bounded, structural diagnostics. Public events cannot carry strings or payloads.
use anyhow::{ensure, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

pub const QUEUE_CAPACITY: usize = 256;
pub const FILE_LIMIT: u64 = 256 * 1024;
pub const FILE_COUNT: usize = 4;
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Lifecycle,
    Failure,
    Transition,
    Debug,
    QueueOverflow,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    InputMethod,
    StickerAssets,
    Startup,
    Shutdown,
    Profile,
    Login,
    Qr,
    Trust,
    Bootstrap,
    Resume,
    Authentication,
    Connection,
    Synchronization,
    Storage,
    Send,
    Receive,
    Heartbeat,
    Parser,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ResourceLimit,
    Started,
    CreatingQr,
    WaitingForScan,
    WaitingForPhone,
    VerifyingTrust,
    Bootstrapping,
    Restoring,
    FreshApprovalRequired,
    Stopped,
    Requested,
    Ready,
    Connected,
    Online,
    Offline,
    Cancelled,
    QueueFull,
    Transport,
    Timeout,
    HttpStatus,
    ServerCode,
    Invalid,
    Unsupported,
    MessageType,
    FrameLength,
    OuterJson,
    OuterEnvelope,
    AuthKey,
    EncryptionMode,
    OuterData,
    Base64,
    CipherIntegrity,
    Compression,
    InnerJson,
    InnerEnvelope,
    PlaintextAuth,
    QueueCommands,
    Io,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Absent,
    Null,
    Integer,
    Number,
    String,
    Boolean,
    Object,
    Array,
}
/// Unknown command IDs are collapsed, never printed as uncontrolled identifiers.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTag {
    Auth,
    Pong,
    QueueControl,
    Personal,
    Group,
    Signal,
    Other,
}
impl CommandTag {
    pub fn from_wire(command: u16) -> Self {
        match command {
            1 => Self::Auth,
            2 => Self::Pong,
            4 => Self::QueueControl,
            501 | 500 => Self::Personal,
            521 | 520 => Self::Group,
            540..=569 => Self::Signal,
            _ => Self::Other,
        }
    }
}
/// Fixed storage labels contain no SQL, row identifiers, paths, or payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageOperation {
    Open,
    RecoverOutgoing,
    ReconcileOutgoing,
    Actor,
    DirectoryWrite,
    Conversations,
    ReceiveCursor,
    ReceiveBatch,
    ReceiveCursorWrite,
    PersistAck,
    DirectoryRead,
    OpenConversation,
    LoadHistory,
    PrepareOutgoing,
    FailOutgoing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFailure {
    ConflictingMessage,
    ConflictingEcho,
    OutgoingMismatch,
    ConflictingMetadata,
    Validation,
    Sqlite,
    Other,
}
#[derive(Clone, Copy, Serialize)]
pub struct Event {
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_operation: Option<StorageOperation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_failure: Option<StorageFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sqlite_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_mismatch_mask: Option<u8>,
    kind: Kind,
    stage: Stage,
    reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<CommandTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subcommand: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encryption: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code_type: Option<ValueType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_type: Option<ValueType>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    server_code_out_of_range: bool,
}
impl Event {
    pub fn new(kind: Kind, stage: Stage, reason: Reason) -> Self {
        Self {
            storage_operation: None,
            storage_failure: None,
            sqlite_code: None,
            storage_mismatch_mask: None,
            kind,
            stage,
            reason,
            http_status: None,
            server_code: None,
            bytes: None,
            command: None,
            command_id: None,
            subcommand: None,
            encryption: None,
            error_code_type: None,
            data_type: None,
            server_code_out_of_range: false,
        }
    }
    pub fn storage(
        mut self,
        operation: StorageOperation,
        failure: StorageFailure,
        sqlite_code: Option<i32>,
    ) -> Self {
        self.storage_operation = Some(operation);
        self.storage_failure = Some(failure);
        self.sqlite_code = sqlite_code.and_then(|code| u16::try_from(code).ok());
        self
    }
    pub fn http_status(mut self, value: Option<u16>) -> Self {
        self.http_status = value.filter(|v| (100..=599).contains(v));
        self
    }
    /// Closed flags only: thread=1, body=2, rich content=4. Never payload data.
    pub fn storage_mismatch(mut self, mask: Option<u8>) -> Self {
        self.storage_mismatch_mask = mask.filter(|mask| (1..=7).contains(mask));
        self
    }
    pub fn server_code(mut self, value: Option<i64>) -> Self {
        self.server_code = value.filter(|v| (-1_000_000..=1_000_000).contains(v));
        self.server_code_out_of_range = value.is_some() && self.server_code.is_none();
        if value.is_some() {
            self.error_code_type = Some(ValueType::Integer);
        }
        self
    }
    pub fn bytes(mut self, value: usize) -> Self {
        self.bytes = Some(value.min(16 * 1024 * 1024));
        self
    }
    pub fn frame(mut self, command: u16, subcommand: u8, encryption: Option<u8>) -> Self {
        self.command = Some(CommandTag::from_wire(command));
        self.command_id = Some(command);
        self.subcommand = Some(subcommand);
        self.encryption = encryption.filter(|v| *v <= 3);
        self
    }
    pub fn envelope_types(mut self, error_code: ValueType, data: ValueType) -> Self {
        self.error_code_type = Some(error_code);
        self.data_type = Some(data);
        self
    }
}
#[derive(Clone)]
struct Sink {
    sender: SyncSender<Event>,
    dropped: Arc<AtomicU64>,
    verbose: bool,
    available: Arc<AtomicBool>,
}
impl Sink {
    fn record(&self, event: Event) {
        if matches!(event.kind, Kind::Debug) && !self.verbose {
            return;
        }
        if self.sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
static GLOBAL: OnceLock<Mutex<Option<(u64, Sink)>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Returns immediately, including when the log queue is full or unavailable.
pub fn record(event: Event) {
    if let Some(global) = GLOBAL.get() {
        if let Ok(guard) = global.try_lock() {
            if let Some((_, sink)) = guard.as_ref() {
                sink.record(event);
            }
        }
    }
}

pub struct Logger {
    id: u64,
    sink: Option<Sink>,
    writer: Option<JoinHandle<()>>,
    path: PathBuf,
}
impl Logger {
    /// Initialize before opening the GUI. Shutdown drains at most the bounded
    /// queue on the writer thread; no UI/network event performs filesystem I/O.
    #[cfg(all(test, feature = "messaging"))]
    pub(crate) fn isolated_for_test(cache: &Path, verbose: bool) -> Result<Self> {
        Self::spawn(cache, verbose, FILE_LIMIT)
    }
    pub fn start(cache: &Path, verbose: bool) -> Result<Self> {
        let logger = Self::spawn(cache, verbose, FILE_LIMIT)?;
        let mut global = GLOBAL
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| anyhow::anyhow!("Diagnostic logger unavailable"))?;
        ensure!(global.is_none(), "Diagnostic logger already active");
        *global = Some((logger.id, logger.sink.as_ref().unwrap().clone()));
        logger.record(Event::new(Kind::Lifecycle, Stage::Startup, Reason::Started));
        Ok(logger)
    }
    fn spawn(cache: &Path, verbose: bool, limit: u64) -> Result<Self> {
        let directory = cache.join("diagnostics");
        crate::profile::create_private_dir(&directory)?;
        let path = log_path(cache);
        validate_slots(&path, limit)?;
        let file = private_file(&path)?;
        let (sender, receiver) = mpsc::sync_channel::<Event>(QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let available = Arc::new(AtomicBool::new(true));
        let writer_available = available.clone();
        let worker_dropped = dropped.clone();
        let worker_path = path.clone();
        let writer = thread::Builder::new()
            .name("native-diagnostics".into())
            .spawn(move || {
                let mut writer = RotatingWriter {
                    path: worker_path,
                    file: Some(file),
                    limit,
                };
                let build = build_identity();
                let mut failed = false;
                for event in receiver {
                    let lost = worker_dropped.swap(0, Ordering::Relaxed);
                    if lost > 0 {
                        let overflow =
                            Event::new(Kind::QueueOverflow, Stage::Storage, Reason::QueueFull)
                                .bytes(lost as usize);
                        let _ = writer.write(overflow, None);
                    }
                    let startup = matches!(event.kind, Kind::Lifecycle)
                        && matches!(event.stage, Stage::Startup);
                    if writer
                        .write(event, if startup { Some(&build) } else { None })
                        .is_err()
                    {
                        writer_available.store(false, Ordering::Release);
                        failed = true;
                        break;
                    }
                }
                if writer.file.is_some()
                    && writer
                        .write(
                            Event::new(Kind::Lifecycle, Stage::Shutdown, Reason::Stopped),
                            None,
                        )
                        .is_err()
                {
                    failed = true;
                }
                writer_available.store(false, Ordering::Release);
                if let Some(file) = writer.file.as_mut() {
                    if file.flush().and_then(|_| file.sync_data()).is_err() {
                        failed = true;
                    }
                }
                if failed {
                    eprintln!("Diagnostic log unavailable: background writer I/O failure");
                }
            })?;
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            sink: Some(Sink {
                sender,
                dropped,
                verbose,
                available,
            }),
            writer: Some(writer),
            path,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn available(&self) -> bool {
        self.sink
            .as_ref()
            .is_some_and(|sink| sink.available.load(Ordering::Acquire))
    }
    pub fn record(&self, event: Event) {
        if let Some(sink) = &self.sink {
            sink.record(event);
        }
    }
}
impl Drop for Logger {
    fn drop(&mut self) {
        if let Some(global) = GLOBAL.get() {
            if let Ok(mut guard) = global.lock() {
                if guard.as_ref().is_some_and(|(id, _)| *id == self.id) {
                    guard.take();
                }
            }
        }
        self.sink.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
pub fn log_path(cache: &Path) -> PathBuf {
    cache.join("diagnostics/native.log")
}
fn validate_slots(path: &Path, limit: u64) -> Result<()> {
    for index in 0..FILE_COUNT {
        let path = slot(path, index);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                options.custom_flags(libc::O_NOFOLLOW);
                let file = options.open(path)?;
                crate::profile::check_private_file(&file)?;
                ensure!(
                    file.metadata()?.len() <= limit,
                    "Existing diagnostic file exceeds the rotation limit"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn slot(path: &Path, index: usize) -> PathBuf {
    if index == 0 {
        path.into()
    } else {
        path.with_file_name(format!("native.log.{index}"))
    }
}
fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    crate::profile::check_private_file(&file)?;
    Ok(file)
}
struct RotatingWriter {
    path: PathBuf,
    file: Option<File>,
    limit: u64,
}
impl RotatingWriter {
    fn write(&mut self, event: Event, build: Option<&BuildIdentity>) -> Result<()> {
        #[derive(Serialize)]
        struct Line<'a> {
            ts_ms: u128,
            #[serde(flatten)]
            event: Event,
            #[serde(skip_serializing_if = "Option::is_none")]
            build: Option<&'a BuildIdentity>,
        }
        let mut bytes = serde_json::to_vec(&Line {
            ts_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            event,
            build,
        })?;
        bytes.push(b'\n');
        ensure!(
            bytes.len() as u64 <= self.limit,
            "Diagnostic record exceeds rotation limit"
        );
        if self
            .file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Diagnostic writer unavailable"))?
            .metadata()?
            .len()
            + bytes.len() as u64
            > self.limit
        {
            self.rotate()?;
        }
        self.file.as_mut().unwrap().write_all(&bytes)?;
        Ok(())
    }
    fn rotate(&mut self) -> Result<()> {
        self.file.take();
        // Validate pre-existing slots before unlink/rename. Never follow links.
        for index in 0..FILE_COUNT {
            let path = slot(&self.path, index);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    let file = OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NOFOLLOW)
                        .open(path)?;
                    crate::profile::check_private_file(&file)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let oldest = slot(&self.path, FILE_COUNT - 1);
        if oldest.exists() {
            fs::remove_file(oldest)?;
        }
        for index in (0..FILE_COUNT - 1).rev() {
            let source = slot(&self.path, index);
            if source.exists() {
                fs::rename(source, slot(&self.path, index + 1))?;
            }
        }
        self.file = Some(private_file(&self.path)?);
        Ok(())
    }
}
#[derive(Serialize)]
struct BuildIdentity {
    version: &'static str,
    features: u8,
    executable_bytes: Option<u64>,
    executable_mtime_s: Option<u64>,
    executable_sha256: Option<String>,
}
fn build_identity() -> BuildIdentity {
    let mut build = BuildIdentity {
        version: env!("CARGO_PKG_VERSION"),
        features: u8::from(cfg!(feature = "network"))
            | u8::from(cfg!(feature = "live-login")) << 1
            | u8::from(cfg!(feature = "messaging")) << 2
            | u8::from(cfg!(feature = "signal-store")) << 3,
        executable_bytes: None,
        executable_mtime_s: None,
        executable_sha256: None,
    };
    // Executable hashing happens once on the background writer, never on egui.
    if let Ok(path) = std::env::current_exe() {
        if let Ok(mut file) = File::open(path) {
            if let Ok(meta) = file.metadata() {
                build.executable_bytes = Some(meta.len());
                build.executable_mtime_s = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
            }
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 16384];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        build.executable_sha256 =
                            Some(hash.finalize().iter().map(|b| format!("{b:02x}")).collect());
                        break;
                    }
                    Ok(size) => hash.update(&buffer[..size]),
                    Err(_) => break,
                }
            }
        }
    }
    build
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zalo-log-fixture-{}",
            crate::domain::local_message_id().unwrap()
        ));
        crate::profile::create_private_dir(&path).unwrap();
        path
    }
    #[test]
    fn rotation_is_bounded_private_and_every_line_is_structural_json() {
        let root = scratch();
        let directory = root.join("diagnostics");
        crate::profile::create_private_dir(&directory).unwrap();
        let path = log_path(&root);
        let mut writer = RotatingWriter {
            path: path.clone(),
            file: Some(private_file(&path).unwrap()),
            limit: 1024,
        };
        for _ in 0..200 {
            writer
                .write(
                    Event::new(Kind::Failure, Stage::Synchronization, Reason::OuterEnvelope)
                        .frame(549, 200, Some(2))
                        .bytes(128)
                        .envelope_types(ValueType::Absent, ValueType::Object),
                    None,
                )
                .unwrap();
        }
        drop(writer);
        let mut total = 0;
        for index in 0..FILE_COUNT {
            let path = slot(&path, index);
            let metadata = fs::metadata(&path).unwrap();
            assert!(metadata.len() <= 1024);
            total += metadata.len();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
                assert_eq!(
                    fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            for line in fs::read_to_string(path).unwrap().lines() {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["command_id"], 549);
                assert_eq!(value["subcommand"], 200);
                assert_eq!(value["error_code_type"], "absent");
                assert_eq!(value["data_type"], "object");
                assert_eq!(value["reason"], "outer_envelope");
            }
        }
        assert!(total <= 4 * 1024);
        assert!(!slot(&path, FILE_COUNT).exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn startup_shutdown_are_flushed_and_build_identifies_actual_executable() {
        let root = scratch();
        let logger = Logger::spawn(&root, false, FILE_LIMIT).unwrap();
        logger.record(Event::new(Kind::Lifecycle, Stage::Startup, Reason::Started));
        logger.record(Event::new(Kind::Debug, Stage::Parser, Reason::Ready));
        logger.record(Event::new(
            Kind::Failure,
            Stage::Synchronization,
            Reason::OuterEnvelope,
        ));
        let path = logger.path().to_owned();
        drop(logger);
        let lines: Vec<serde_json::Value> = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["stage"], "startup");
        assert_eq!(lines[0]["build"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            lines[0]["build"]["executable_sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert!(lines[0]["build"]["executable_bytes"].as_u64().unwrap() > 0);
        assert_eq!(lines[1]["kind"], "failure");
        assert_eq!(lines[2]["stage"], "shutdown");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn queue_backpressure_drops_with_counter_and_never_blocks_the_caller() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let sink = Sink {
            sender,
            dropped: dropped.clone(),
            verbose: false,
            available: Arc::new(AtomicBool::new(true)),
        };
        let event = Event::new(Kind::Failure, Stage::Storage, Reason::Io);
        sink.record(event);
        sink.record(event);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn event_api_has_no_freeform_payload_and_suppresses_unrecognized_numeric_values() {
        let event = Event::new(Kind::Failure, Stage::Parser, Reason::OuterEnvelope)
            .frame(563, 255, Some(99))
            .http_status(Some(12345))
            .server_code(Some(1234567890123456))
            .bytes(usize::MAX)
            .envelope_types(ValueType::Integer, ValueType::String);
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["command_id"], 563);
        assert_eq!(value["subcommand"], 255);
        assert!(value.get("encryption").is_none());
        assert!(value.get("http_status").is_none());
        assert!(value.get("server_code").is_none());
        assert_eq!(value["server_code_out_of_range"], true);
        assert_eq!(value["bytes"], 16 * 1024 * 1024);
        assert!(value.as_object().unwrap().keys().all(|key| matches!(
            key.as_str(),
            "kind"
                | "stage"
                | "reason"
                | "command"
                | "command_id"
                | "subcommand"
                | "bytes"
                | "error_code_type"
                | "data_type"
                | "server_code_out_of_range"
        )));
    }
    #[test]
    fn log_path_resolution_does_not_create_files_or_directories() {
        let root = std::env::temp_dir().join(format!(
            "zalo-log-no-write-{}",
            crate::domain::local_message_id().unwrap()
        ));
        let paths = crate::profile::Profile::planned_paths(Some(&root), "rust-live").unwrap();
        assert!(log_path(&paths.cache_dir).is_absolute());
        assert!(!root.exists());
    }
    #[test]
    fn existing_oversize_logs_are_refused_before_starting_writer() {
        let root = scratch();
        crate::profile::create_private_dir(&root.join("diagnostics")).unwrap();
        let path = slot(&log_path(&root), 3);
        let file = private_file(&path).unwrap();
        file.set_len(FILE_LIMIT + 1).unwrap();
        drop(file);
        assert!(Logger::spawn(&root, false, FILE_LIMIT).is_err());
        assert!(!log_path(&root).exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_and_public_permission_log_files_are_refused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = scratch();
        crate::profile::create_private_dir(&root.join("diagnostics")).unwrap();
        let target = root.join("untouched");
        fs::write(&target, b"fixture").unwrap();
        let path = log_path(&root);
        symlink(&target, &path).unwrap();
        assert!(Logger::spawn(&root, false, FILE_LIMIT).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"fixture");
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Logger::spawn(&root, false, FILE_LIMIT).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
