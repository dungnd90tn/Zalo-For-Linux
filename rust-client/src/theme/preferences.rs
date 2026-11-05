use super::{apply_custom, catalog, parse_color, ThemeMode};
use anyhow::{bail, Context, Result};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

const MAX_BYTES: u64 = 128 * 1024;
const MAX_OVERRIDES: usize = 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    version: u8,
    revision: u64,
    mode: ThemeMode,
    overrides: BTreeMap<String, String>,
}
impl Preferences {
    fn initial() -> Self {
        Self {
            version: 1,
            ..Self::default()
        }
    }
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.overrides.len() > MAX_OVERRIDES {
            bail!("unsupported theme preferences");
        }
        for (name, value) in &self.overrides {
            validate_override(name, value)?;
        }
        Ok(())
    }
}
fn validate_override(name: &str, value: &str) -> Result<()> {
    if name.len() > 128
        || value.len() > 256
        || catalog::token(name).is_none()
        || !(value.starts_with('#')
            && parse_color(value).is_some()
            && super::resolve_color(name, false, None, 0).is_some()
            || super::BACKGROUND_TOKENS.contains(&name) && super::parse_gradient(value).is_some())
    {
        bail!("custom theme requires an original token and a valid hexadecimal color");
    }
    Ok(())
}
fn read_preferences(path: &Path) -> Result<Preferences> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::initial()),
        Err(e) => return Err(e.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        bail!("invalid theme preferences file");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("theme preferences must be private");
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("theme preferences exceed budget");
    }
    let prefs: Preferences = serde_json::from_slice(&bytes)?;
    prefs.validate()?;
    Ok(prefs)
}
fn locked<T>(directory: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    crate::profile::create_private_dir(directory)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let lock = options.open(directory.join("theme-settings.lock"))?;
    let metadata = lock.metadata()?;
    if !metadata.is_file() {
        bail!("theme lock is not regular");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("theme lock must be private");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    loop {
        match fs2::FileExt::try_lock_exclusive(&lock) {
            Ok(()) => break,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(error.into()),
        }
    }
    action()
}
fn persist(
    directory: &Path,
    expected: u64,
    mut prefs: Preferences,
    stop: &AtomicBool,
) -> Result<u64> {
    prefs.validate()?;
    locked(directory, || {
        let target = directory.join("theme.json");
        if read_preferences(&target)?.revision != expected {
            bail!("theme preferences changed concurrently");
        }
        prefs.revision = expected
            .checked_add(1)
            .context("theme preference revision exhausted")?;
        let bytes = serde_json::to_vec(&prefs)?;
        if bytes.len() as u64 > MAX_BYTES {
            bail!("theme preferences exceed budget");
        }
        let prepared = crate::profile::prepare_secret(&target, &bytes)?;
        // A cancelled preparation leaves the original file intact. After this
        // boundary a successful rename may finish. Window teardown instead
        // flushes its final active snapshot; preferences have no auth scope.
        if stop.load(Ordering::Acquire) {
            bail!("theme preference worker stopped");
        }
        prepared.commit()?;
        Ok(prefs.revision)
    })
}
enum Command {
    Save {
        sequence: u64,
        expected: u64,
        prefs: Preferences,
    },
    Final {
        prefs: Preferences,
        early: Option<EarlyEdits>,
    },
}
#[derive(Default)]
struct EarlyEdits {
    mode: Option<ThemeMode>,
    overrides: BTreeMap<String, Option<String>>,
    reset: bool,
}
impl EarlyEdits {
    fn apply(self, prefs: &mut Preferences) {
        if let Some(mode) = self.mode {
            prefs.mode = mode;
        }
        if self.reset {
            prefs.overrides.clear();
        }
        for (name, value) in self.overrides {
            if let Some(value) = value {
                prefs.overrides.insert(name, value);
            } else {
                prefs.overrides.remove(&name);
            }
        }
    }
}
enum Event {
    Loaded(Result<Preferences>),
    Saved { sequence: u64, result: Result<u64> },
}

/// One bounded background writer per window. Rendering methods never wait for I/O;
/// window teardown joins the writer so its last accepted edit survives restart.
pub struct ThemeController {
    commands: Option<mpsc::SyncSender<Command>>,
    events: mpsc::Receiver<Event>,
    worker: Option<std::thread::JoinHandle<()>>,
    prefs: Preferences,
    loaded: bool,
    dirty: bool,
    inflight: Option<u64>,
    sequence: u64,
    error: Option<&'static str>,
    applied: Option<(ThemeMode, bool, u64)>,
    visual_revision: u64,
    writable: bool,
    early: EarlyEdits,
}
impl ThemeController {
    pub fn new(directory: PathBuf, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let (commands, rx) = mpsc::sync_channel(2);
        let (tx, events) = mpsc::sync_channel(2);
        let worker = std::thread::Builder::new()
            .name("zalo-theme-settings".into())
            .spawn(move || {
                let loaded = locked(&directory, || {
                    read_preferences(&directory.join("theme.json"))
                });
                let mut revision = loaded.as_ref().ok().map(|prefs| prefs.revision);
                let initial = loaded.as_ref().ok().cloned();
                let _ = tx.try_send(Event::Loaded(loaded));
                repaint();
                while let Ok(command) = rx.recv() {
                    match command {
                        Command::Save { sequence,expected,prefs } => {
                            let result = persist(&directory, expected, prefs, &AtomicBool::new(false));
                            if result.is_err() { eprintln!("Theme settings unavailable: private configuration or I/O failure."); }
                            revision = result.as_ref().ok().copied();
                            let _ = tx.try_send(Event::Saved { sequence, result });
                            repaint();
                        },
                        Command::Final { mut prefs,early } => {
                            if let Some(early)=early {
                                if let Some(initial)=initial.as_ref() { prefs=initial.clone();early.apply(&mut prefs); }
                            }
                            if let Some(expected) = revision {
                                if persist(&directory,expected,prefs,&AtomicBool::new(false)).is_err() {
                                    eprintln!("Theme settings unavailable: private configuration or I/O failure.");
                                }
                            }
                        },
                    }
                }
            })?;
        Ok(Self {
            commands: Some(commands),
            events,
            worker: Some(worker),
            prefs: Preferences::initial(),
            loaded: false,
            dirty: false,
            inflight: None,
            sequence: 0,
            error: None,
            applied: None,
            visual_revision: 0,
            writable: true,
            early: EarlyEdits::default(),
        })
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        for _ in 0..2 {
            match self.events.try_recv() {
                Ok(Event::Loaded(result)) => {
                    self.loaded = true;
                    match result {
                        Ok(mut prefs) => {
                            if self.dirty {
                                std::mem::take(&mut self.early).apply(&mut prefs);
                            }
                            self.prefs = prefs;
                            self.visual_revision += 1;
                        }
                        Err(_) => {
                            self.error =
                                Some("Không đọc được giao diện đã lưu; chưa ghi đè cấu hình.");
                            self.writable = false;
                        }
                    }
                }
                Ok(Event::Saved { sequence, result }) if self.inflight == Some(sequence) => {
                    self.inflight = None;
                    match result {
                        Ok(revision) => {
                            self.prefs.revision = revision;
                            self.error = None;
                        }
                        Err(_) => {
                            self.error=Some("Không lưu được giao diện; thay đổi hiện chỉ áp dụng trong cửa sổ này.");
                            self.writable = false;
                        }
                    }
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.writable = false;
                    self.error = Some("Tiến trình lưu giao diện đã dừng.");
                    break;
                }
            }
        }
        self.flush();
        let system_dark = ctx.system_theme() == Some(egui::Theme::Dark);
        let key = (self.prefs.mode, system_dark, self.visual_revision);
        if self.applied != Some(key) {
            apply_custom(ctx, self.prefs.mode, &self.prefs.overrides);
            self.applied = Some(key);
        }
    }
    fn flush(&mut self) {
        if !self.loaded || !self.dirty || self.inflight.is_some() || !self.writable {
            return;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.writable = false;
            self.error = Some("Đã hết phiên bản cấu hình giao diện.");
            return;
        };
        let command = Command::Save {
            sequence,
            expected: self.prefs.revision,
            prefs: self.prefs.clone(),
        };
        if self
            .commands
            .as_ref()
            .is_some_and(|tx| tx.try_send(command).is_ok())
        {
            self.sequence = sequence;
            self.inflight = Some(sequence);
            self.dirty = false;
        } else {
            self.error = Some("Hàng đợi giao diện đang bận; thay đổi đang chờ lưu.");
        }
    }
    fn changed(&mut self) {
        self.dirty = true;
        self.visual_revision = self.visual_revision.wrapping_add(1);
        self.flush();
    }
    pub fn mode(&self) -> ThemeMode {
        self.prefs.mode
    }
    pub fn set_mode(&mut self, mode: ThemeMode) {
        if self.prefs.mode != mode || !self.loaded {
            if !self.loaded {
                self.early.mode = Some(mode);
            }
            self.prefs.mode = mode;
            self.changed();
        }
    }
    pub fn persistence_error(&self) -> Option<&str> {
        self.error
    }
    pub fn overrides(&self) -> &BTreeMap<String, String> {
        &self.prefs.overrides
    }
    pub fn color(&self, name: &str) -> Option<egui::Color32> {
        super::resolve_color(
            name,
            self.prefs
                .mode
                .effective_dark(self.applied.is_some_and(|(_, dark, _)| dark)),
            if self.prefs.mode == ThemeMode::Custom {
                Some(&self.prefs.overrides)
            } else {
                None
            },
            0,
        )
    }
    pub fn set_override(&mut self, name: &str, value: &str) -> Result<()> {
        validate_override(name, value)?;
        if !self.prefs.overrides.contains_key(name) && self.prefs.overrides.len() >= MAX_OVERRIDES {
            bail!("custom theme exceeds budget");
        }
        self.prefs.overrides.insert(name.into(), value.into());
        if !self.loaded {
            self.early.overrides.insert(name.into(), Some(value.into()));
        }
        self.changed();
        Ok(())
    }
    pub fn clear_override(&mut self, name: &str) {
        if name.len() > 128 || catalog::token(name).is_none() {
            return;
        }
        if !self.loaded {
            self.early.overrides.insert(name.into(), None);
        }
        if self.prefs.overrides.remove(name).is_some() || !self.loaded {
            self.changed();
        }
    }
    pub fn reset_custom(&mut self) {
        if !self.loaded {
            self.early.reset = true;
            self.early.overrides.clear();
        }
        if !self.prefs.overrides.is_empty() || !self.loaded {
            self.prefs.overrides.clear();
            self.changed();
        }
    }
}
impl Drop for ThemeController {
    fn drop(&mut self) {
        if self.dirty && self.writable {
            // There is at most one normal command in flight; the second queue
            // slot is reserved for the coalesced final edit, even before load.
            let final_edit = Command::Final {
                prefs: self.prefs.clone(),
                early: if !self.loaded {
                    Some(std::mem::take(&mut self.early))
                } else {
                    None
                },
            };
            if self
                .commands
                .as_ref()
                .is_some_and(|sender| sender.try_send(final_edit).is_err())
            {
                eprintln!("Theme settings unavailable: final edit could not be queued.");
            }
        }
        self.commands.take();
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            eprintln!("Theme settings unavailable: background writer stopped.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zalo-theme-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        crate::profile::create_private_dir(&path).unwrap();
        path
    }
    #[test]
    fn durable_restart_revision_cas_and_private_permissions() {
        let directory = directory();
        let stop = AtomicBool::new(false);
        let mut prefs = Preferences::initial();
        prefs.mode = ThemeMode::Dark;
        prefs
            .overrides
            .insert("--text-primary".into(), "#123456".into());
        assert_eq!(persist(&directory, 0, prefs.clone(), &stop).unwrap(), 1);
        let saved = read_preferences(&directory.join("theme.json")).unwrap();
        assert_eq!(saved.mode, ThemeMode::Dark);
        assert_eq!(saved.revision, 1);
        assert!(persist(&directory, 0, prefs, &stop).is_err());
        assert_eq!(
            read_preferences(&directory.join("theme.json"))
                .unwrap()
                .revision,
            1
        );
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(directory.join("theme.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn cancelled_write_and_unsafe_targets_preserve_existing_configuration() {
        let directory = directory();
        let stop = AtomicBool::new(false);
        persist(&directory, 0, Preferences::initial(), &stop).unwrap();
        let before = fs::read(directory.join("theme.json")).unwrap();
        stop.store(true, Ordering::Release);
        assert!(persist(&directory, 1, Preferences::initial(), &stop).is_err());
        assert_eq!(fs::read(directory.join("theme.json")).unwrap(), before);
        #[cfg(unix)]
        {
            fs::remove_file(directory.join("theme.json")).unwrap();
            std::os::unix::fs::symlink("sentinel", directory.join("theme.json")).unwrap();
            assert!(read_preferences(&directory.join("theme.json")).is_err());
        }
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn bounded_preferences_refuse_unknown_tokens_css_and_versions() {
        assert!(validate_override("--text-primary", "url(https://invalid)").is_err());
        assert!(validate_override("--missing-token", "#123456").is_err());
        assert!(validate_override("--H_Gradient_B55B30", "#123456").is_err());
        let mut prefs = Preferences::initial();
        prefs.version = 2;
        assert!(prefs.validate().is_err());
        let directory = directory();
        fs::write(
            directory.join("theme.json"),
            vec![0_u8; MAX_BYTES as usize + 1],
        )
        .unwrap();
        assert!(read_preferences(&directory.join("theme.json")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn two_background_writers_cannot_overwrite_the_same_disk_revision() {
        let directory = directory();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [ThemeMode::Dark, ThemeMode::Custom]
            .into_iter()
            .map(|mode| {
                let directory = directory.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut prefs = Preferences::initial();
                    prefs.mode = mode;
                    barrier.wait();
                    persist(&directory, 0, prefs, &AtomicBool::new(false))
                })
            })
            .collect();
        let outcomes: Vec<_> = workers
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            read_preferences(&directory.join("theme.json"))
                .unwrap()
                .revision,
            1
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn queued_early_changes_survive_async_load_and_latest_edit_wins_restart() {
        let directory = directory();
        let ctx = egui::Context::default();
        let mut controller = ThemeController::new(directory.clone(), Arc::new(|| {})).unwrap();
        controller.set_mode(ThemeMode::Dark);
        controller
            .set_override("--text-primary", "#123456")
            .unwrap();
        for _ in 0..200 {
            controller.poll(&ctx);
            if controller.loaded && controller.inflight.is_none() && !controller.dirty {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(controller.loaded && controller.inflight.is_none() && !controller.dirty);
        assert_eq!(
            read_preferences(&directory.join("theme.json"))
                .unwrap()
                .mode,
            ThemeMode::Dark
        );
        controller.set_mode(ThemeMode::Custom);
        controller.poll(&ctx);
        assert_eq!(
            super::super::palette(&ctx).text_primary,
            egui::Color32::from_rgb(18, 52, 86)
        );
        drop(controller);
        assert_eq!(
            read_preferences(&directory.join("theme.json"))
                .unwrap()
                .mode,
            ThemeMode::Custom
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn immediate_close_flushes_an_edit_made_before_background_load() {
        let directory = directory();
        let mut controller = ThemeController::new(directory.clone(), Arc::new(|| {})).unwrap();
        controller.set_mode(ThemeMode::Dark);
        controller
            .set_override("--text-primary", "#234567")
            .unwrap();
        drop(controller);
        let persisted = read_preferences(&directory.join("theme.json")).unwrap();
        assert_eq!(persisted.mode, ThemeMode::Dark);
        assert_eq!(persisted.overrides["--text-primary"], "#234567");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn early_mode_and_single_token_edits_merge_existing_saved_overrides() {
        let directory = directory();
        let mut original = Preferences::initial();
        original
            .overrides
            .insert("--text-primary".into(), "#123456".into());
        original
            .overrides
            .insert("--text-secondary".into(), "#456789".into());
        persist(&directory, 0, original, &AtomicBool::new(false)).unwrap();
        let mut controller = ThemeController::new(directory.clone(), Arc::new(|| {})).unwrap();
        controller.set_mode(ThemeMode::Custom);
        controller
            .set_override("--text-primary", "#abcdef")
            .unwrap();
        drop(controller);
        let saved = read_preferences(&directory.join("theme.json")).unwrap();
        assert_eq!(saved.mode, ThemeMode::Custom);
        assert_eq!(saved.overrides["--text-primary"], "#abcdef");
        assert_eq!(saved.overrides["--text-secondary"], "#456789");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn contested_writer_lock_has_a_bounded_wait_and_preserves_the_file() {
        let directory = directory();
        persist(
            &directory,
            0,
            Preferences::initial(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.join("theme-settings.lock"))
            .unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        let start = std::time::Instant::now();
        assert!(persist(
            &directory,
            1,
            Preferences::initial(),
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            read_preferences(&directory.join("theme.json"))
                .unwrap()
                .revision,
            1
        );
        drop(lock);
        fs::remove_dir_all(directory).unwrap();
    }
}
