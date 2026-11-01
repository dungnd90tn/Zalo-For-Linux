//! Private, locked Rust profiles, independent of Electron's ZaloData directory.

use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

const NAMESPACE: &str = "zalo-rust";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct ProfilePaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub session_file: PathBuf,
}

/// Keep this guard alive for the entire application lifetime. OS locks are released
/// on process exit; the lock file is deliberately never unlinked.
#[derive(Debug)]
pub struct Profile {
    pub name: String,
    pub paths: ProfilePaths,
    _lock: File,
}

impl Profile {
    /// Check all durable owner authorities without creating or changing them.
    pub fn check_account(&self, account: &str) -> Result<()> {
        check_account_at(&self.paths.data_dir, account)
    }
    /// Permanently associate this profile with its first authenticated account.
    pub fn bind_account(&self, account: &str) -> Result<()> {
        bind_account_at(&self.paths.data_dir, account)
    }

    pub fn open(name: &str) -> Result<Self> {
        validate_profile_name(name)?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_root = xdg_root("XDG_DATA_HOME", home.as_deref(), ".local/share")?;
        let config_root = xdg_root("XDG_CONFIG_HOME", home.as_deref(), ".config")?;
        let cache_root = xdg_root("XDG_CACHE_HOME", home.as_deref(), ".cache")?;
        Self::from_paths(
            name,
            ProfilePaths {
                data_dir: data_root.join(NAMESPACE).join("profiles").join(name),
                config_dir: config_root.join(NAMESPACE).join("profiles").join(name),
                cache_dir: cache_root.join(NAMESPACE).join("profiles").join(name),
                session_file: data_root
                    .join(NAMESPACE)
                    .join("profiles")
                    .join(name)
                    .join("session.json"),
            },
        )
    }

    /// Explicit isolated root for testing or portable installations.
    pub fn at_root(base: &Path, name: &str) -> Result<Self> {
        validate_profile_name(name)?;
        if !base.is_absolute() {
            bail!("profile root must be an absolute path");
        }
        let root = base.join(name);
        Self::from_paths(
            name,
            ProfilePaths {
                data_dir: root.join("data"),
                config_dir: root.join("config"),
                cache_dir: root.join("cache"),
                session_file: root.join("data/session.json"),
            },
        )
    }

    fn from_paths(name: &str, paths: ProfilePaths) -> Result<Self> {
        for path in [&paths.data_dir, &paths.config_dir, &paths.cache_dir] {
            create_private_dir(path)?;
        }
        let lock_path = paths.data_dir.join(".profile.lock");
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let lock = options.open(&lock_path).context("open profile lock")?;
        check_private_file(&lock)?;
        lock.try_lock_exclusive()
            .with_context(|| format!("profile '{name}' is already in use"))?;
        Ok(Self {
            name: name.to_owned(),
            paths,
            _lock: lock,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountOwner {
    version: u8,
    account_id: String,
}

/// Call before accepting a QR-login result, loading history or creating a
/// session. The application must already hold this profile's lifetime lock.
pub fn bind_account(account: &str) -> Result<()> {
    bind_account_at(&std::env::current_dir()?, account)
}

fn check_account_at(data_dir: &Path, account: &str) -> Result<()> {
    if account.is_empty() || account.len() > 64 || !account.bytes().all(|b| b.is_ascii_digit()) {
        bail!("invalid account ID");
    }
    for (name, field, limit) in [
        (".account-owner.json", "account_id", 4096_u64),
        ("auth-state.json", "owner_uid", 3 * 1024 * 1024),
    ] {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let file = match options.open(data_dir.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        check_private_file(&file)?;
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            bail!("account record exceeds limit");
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).context("invalid account ownership record")?;
        if value.get("version").and_then(serde_json::Value::as_u64) != Some(1)
            || value.get(field).and_then(serde_json::Value::as_str) != Some(account)
        {
            bail!("this profile belongs to another account; choose a different --profile NAME");
        }
    }
    Ok(())
}

fn bind_account_at(data_dir: &Path, account: &str) -> Result<()> {
    check_account_at(data_dir, account)?;
    if account.is_empty() || account.len() > 64 || !account.bytes().all(|b| b.is_ascii_digit()) {
        bail!("authenticated account ID must contain 1–64 digits");
    }
    reject_symlink_components(data_dir)?;
    check_private_dir(data_dir)?;
    let owner_path = data_dir.join(".account-owner.json");
    match fs::symlink_metadata(&owner_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                bail!("account owner marker must not be a symlink");
            }
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            options.custom_flags(libc::O_NOFOLLOW);
            let owner_file = options.open(&owner_path)?;
            check_private_file(&owner_file)?;
            let owner: AccountOwner = serde_json::from_reader(owner_file)
                .context("invalid account owner marker; profile access refused")?;
            if owner.version != 1 || owner.account_id != account {
                bail!("this profile belongs to another account; choose a different --profile NAME");
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let owner = AccountOwner {
                version: 1,
                account_id: account.to_owned(),
            };
            atomic_write_secret(&owner_path, &serde_json::to_vec(&owner)?)?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("profile name must contain 1–64 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

fn xdg_root(variable: &str, home: Option<&Path>, fallback: &str) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(variable).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
        // The XDG specification requires relative values to be ignored.
    }
    let home = home.context("HOME must be set when XDG paths are unavailable")?;
    if !home.is_absolute() {
        bail!("HOME must be absolute");
    }
    Ok(home.join(fallback))
}

/// Reject symlink components before opening private files. Existing ancestor
/// permissions are left untouched; each newly created directory is owner-only.
fn reject_symlink_components(path: &Path) -> Result<()> {
    let mut part = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            bail!("path must not contain '.' or '..'");
        }
        part.push(component.as_os_str());
        match fs::symlink_metadata(&part) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("symlink is not permitted: {}", part.display())
            }
            Ok(_) => (),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    reject_symlink_components(path)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;
    reject_symlink_components(path)?;
    check_private_dir(path)
}

fn check_private_dir(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!(
            "private directory is not a real directory: {}",
            path.display()
        );
    }
    #[cfg(unix)]
    if meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o077 != 0 {
        bail!(
            "private directory must be owned by the current user with mode 0700: {}",
            path.display()
        );
    }
    Ok(())
}

fn check_private_file(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    if !meta.is_file() {
        bail!("private path is not a regular file");
    }
    #[cfg(unix)]
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        bail!("private file must be owner-only and must not have hard links");
    }
    Ok(())
}

/// Replace a secret without a partially written file becoming visible. The parent
/// must already be private. Sync the file and directory so a successful return
/// means both data and rename have been flushed. No credentials are logged.
pub fn atomic_write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // Support relative session paths without following the target through a
    // symlink. The enclosing directory must remain private.
    let absolute_parent = if parent == Path::new(".") {
        std::env::current_dir()?
    } else {
        std::env::current_dir()?.join(parent)
    };
    reject_symlink_components(&absolute_parent)?;
    check_private_dir(parent)?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            bail!("secret target must be a regular file");
        }
        #[cfg(unix)]
        if meta.uid() != unsafe { libc::geteuid() } || meta.nlink() != 1 {
            bail!("secret target must be owned by the current user without hard links");
        }
    }
    let nonce = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".secret-{}-{nonce}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(&temp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Fully written/fsynced private replacement, not visible at its final path yet.
/// Commit only after the caller's generation/authorization linearization point.
pub struct PreparedSecret {
    temporary: PathBuf,
    target: PathBuf,
    parent: PathBuf,
}
impl PreparedSecret {
    pub fn commit(self) -> Result<()> {
        fs::rename(&self.temporary, &self.target)?;
        File::open(&self.parent)?.sync_all()?;
        Ok(())
    }
}
impl Drop for PreparedSecret {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}
pub fn prepare_secret(path: &Path, bytes: &[u8]) -> Result<PreparedSecret> {
    let parent = path.parent().context("missing private parent")?;
    reject_symlink_components(parent)?;
    check_private_dir(parent)?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            bail!("secret target must be regular");
        }
        #[cfg(unix)]
        if meta.uid() != unsafe { libc::geteuid() } || meta.nlink() != 1 {
            bail!("secret target ownership invalid");
        }
    }
    let temporary = parent.join(format!(
        ".prepared-{}-{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(&temporary)?;
    let prepared = PreparedSecret {
        temporary,
        target: path.to_owned(),
        parent: parent.to_owned(),
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zalo-profile-test-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_names_that_escape_profile() {
        for name in [
            "",
            ".",
            "..",
            "../electron",
            "a/b",
            "a\\b",
            "/tmp",
            "a b",
            "a\0b",
        ] {
            assert!(validate_profile_name(name).is_err(), "{name:?}");
        }
        assert!(validate_profile_name(&"a".repeat(65)).is_err());
        assert!(validate_profile_name("test_1-A").is_ok());
    }

    #[test]
    fn locks_one_profile_and_releases_on_drop() {
        let base = TestDir::new();
        let first = Profile::at_root(&base.0, "one").unwrap();
        assert!(Profile::at_root(&base.0, "one").is_err());
        let second = Profile::at_root(&base.0, "two").unwrap();
        assert_ne!(first.paths.data_dir, second.paths.data_dir);
        drop(first);
        assert!(Profile::at_root(&base.0, "one").is_ok());
    }

    #[test]
    fn account_binding_is_permanent_and_separate_profiles_can_use_other_accounts() {
        let base = TestDir::new();
        let one = Profile::at_root(&base.0, "one").unwrap();
        let two = Profile::at_root(&base.0, "two").unwrap();
        one.bind_account("123").unwrap();
        one.bind_account("123").unwrap();
        atomic_write_secret(&one.paths.data_dir.join("history.db"), b"account A history").unwrap();
        let owner = fs::read(one.paths.data_dir.join(".account-owner.json")).unwrap();
        assert!(one.bind_account("456").is_err());
        assert_eq!(
            fs::read(one.paths.data_dir.join(".account-owner.json")).unwrap(),
            owner
        );
        assert_eq!(
            fs::read(one.paths.data_dir.join("history.db")).unwrap(),
            b"account A history"
        );
        two.bind_account("456").unwrap();
        assert!(two.bind_account("123").is_err());
    }

    #[test]
    fn malformed_owner_marker_fails_closed() {
        let base = TestDir::new();
        let profile = Profile::at_root(&base.0, "one").unwrap();
        atomic_write_secret(
            &profile.paths.data_dir.join(".account-owner.json"),
            b"malformed",
        )
        .unwrap();
        assert!(profile.bind_account("123").is_err());
        assert_eq!(
            fs::read(profile.paths.data_dir.join(".account-owner.json")).unwrap(),
            b"malformed"
        );
    }

    #[test]
    fn atomic_secret_replaces_without_affecting_other_profile() {
        let base = TestDir::new();
        let one = Profile::at_root(&base.0, "one").unwrap();
        let two = Profile::at_root(&base.0, "two").unwrap();
        atomic_write_secret(&one.paths.session_file, b"old").unwrap();
        atomic_write_secret(&one.paths.session_file, b"new").unwrap();
        assert_eq!(fs::read(&one.paths.session_file).unwrap(), b"new");
        assert!(!two.paths.session_file.exists());
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&one.paths.data_dir)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&one.paths.session_file)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(fs::read_dir(&one.paths.data_dir).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_profiles_lock_and_secret_targets() {
        use std::os::unix::fs::symlink;
        let base = TestDir::new();
        let outside = base.0.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, base.0.join("escape")).unwrap();
        assert!(Profile::at_root(&base.0, "escape").is_err());
        let profile = Profile::at_root(&base.0, "one").unwrap();
        let target = outside.join("untouched");
        fs::write(&target, b"safe").unwrap();
        symlink(&target, &profile.paths.session_file).unwrap();
        assert!(atomic_write_secret(&profile.paths.session_file, b"bad").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"safe");
        let lock_path = profile.paths.data_dir.join(".profile.lock");
        drop(profile);
        fs::remove_file(&lock_path).unwrap();
        symlink(&target, lock_path).unwrap();
        assert!(Profile::at_root(&base.0, "one").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_world_readable_existing_profile_without_chmod() {
        let base = TestDir::new();
        let profile = Profile::at_root(&base.0, "one").unwrap();
        let data = profile.paths.data_dir.clone();
        drop(profile);
        fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Profile::at_root(&base.0, "one").is_err());
        assert_eq!(
            fs::metadata(&data).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
