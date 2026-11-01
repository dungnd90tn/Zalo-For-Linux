//! Read-only inventory of an explicitly supplied Electron database COPY.
//!
//! Evidence: main-dist/utility-process-sqlite.js defines `message` at line
//! 40889, JSON fields as TEXT at 11419, the msgId primary index at 41268,
//! and SQLite BasicEncryption of `message` plus partitionField `toUid` at
//! 41369–41383. This is NOT enough evidence to safely decode and map history.
//! Consequently no native-store import or credential migration is exposed.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

// Inventory is an interactive diagnostic, not an unbounded history loader.
const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TABLES: usize = 128;
const MAX_SCANNED_ROWS: u64 = 50_000;

#[derive(Debug, Serialize)]
pub struct InventoryReport {
    pub operation: &'static str,
    pub source_account_claim: String,
    pub source_account_verified: bool,
    pub source_sha256: String,
    pub source_bytes: u64,
    pub source_unchanged: bool,
    pub schema: &'static str,
    pub schema_supported: bool,
    pub table_names: Vec<String>,
    pub message_rows: Option<u64>,
    pub scanned_message_rows: Option<u64>,
    pub scan_limit_reached: bool,
    /// Raw timestamp strings; no guessed unit conversion.
    pub earliest_send_dttm_raw: Option<String>,
    pub latest_send_dttm_raw: Option<String>,
    pub timestamp_order: &'static str,
    pub timestamp_semantics_verified: bool,
    pub attachment_path_references: Option<u64>,
    pub native_import_supported: bool,
    pub imported_messages: u64,
    pub credential_migration_performed: bool,
    pub blockers: Vec<String>,
}

/// Inspect one closed, consistent SQLite snapshot. Never finds user profiles,
/// opens credentials, creates a destination DB, or performs network requests.
/// The account is a caller claim, not an identity inferred from message IDs.
pub fn inspect_snapshot(path: &Path, source_account: &str) -> Result<InventoryReport> {
    if source_account.is_empty()
        || source_account.len() > 64
        || !source_account.bytes().all(|b| b.is_ascii_digit())
    {
        bail!("source account must be an explicit numeric account ID (1–64 digits)");
    }
    if !path.is_absolute() {
        bail!("snapshot path must be absolute and refer to a copied database");
    }
    reject_unsafe_source(path)?;
    reject_sidecars(path)?;
    let before = fingerprint(path)?;
    let uri = immutable_uri(path);
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("open SQLite snapshot read-only (encrypted databases are unsupported)")?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
    let integrity: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .context("snapshot is not a readable SQLite database; decryption is not supported")?;
    if integrity != "ok" {
        bail!("snapshot failed SQLite quick_check");
    }
    let mut tables = conn.prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name LIMIT 129")?;
    let table_names: Vec<String> = tables
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if table_names.len() > MAX_TABLES {
        bail!("snapshot exceeds the 128-table inventory limit");
    }
    let mut report = InventoryReport {
        operation: "read-only-dry-run",
        source_account_claim: source_account.to_owned(),
        source_account_verified: false,
        source_sha256: before.1.clone(),
        source_bytes: before.0,
        source_unchanged: false,
        schema: "unsupported",
        schema_supported: false,
        table_names,
        message_rows: None,
        scanned_message_rows: None,
        scan_limit_reached: false,
        earliest_send_dttm_raw: None,
        latest_send_dttm_raw: None,
        timestamp_order: "lexicographic-text-extrema-only",
        timestamp_semantics_verified: false,
        attachment_path_references: None,
        native_import_supported: false,
        imported_messages: 0,
        credential_migration_performed: false,
        blockers: vec![
            "Account identity has not been verified against source metadata.".into(),
            "History import is disabled: Electron payload encryption, timestamp units and conversation mapping are not verified.".into(),
            "Attachments are counted as references only; no media has been copied or resolved.".into(),
            "Use a new QR login; Electron safeStorage credentials are never transferred.".into(),
        ],
    };
    if report.table_names.iter().any(|table| table == "message") {
        inspect_message_table(&conn, &mut report)?;
    } else {
        report.blockers.push("No recognized unpartitioned 'message' table; partitioned and backup schemas require further research.".into());
    }
    drop(tables);
    drop(conn);
    reject_sidecars(path)?;
    let after = fingerprint(path)?;
    if before != after {
        bail!("snapshot changed during inspection; use a consistent offline copy");
    }
    report.source_unchanged = true;
    Ok(report)
}

fn inspect_message_table(conn: &Connection, report: &mut InventoryReport) -> Result<()> {
    let mut statement = conn.prepare("PRAGMA table_info('message')")?;
    let columns: Vec<(String, String, i64)> = statement
        .query_map([], |row| Ok((row.get(1)?, row.get(2)?, row.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let required = [
        ("msgId", "TEXT"),
        ("cliMsgId", "TEXT"),
        ("toUid", "TEXT"),
        ("msgType", "INTEGER"),
        ("sendDttm", "TEXT"),
        ("message", "TEXT"),
    ];
    if !required.iter().all(|(name, kind)| {
        columns
            .iter()
            .any(|(column, declared, _)| column == name && declared.eq_ignore_ascii_case(kind))
    }) || !columns
        .iter()
        .any(|(name, _, pk)| name == "msgId" && *pk == 1)
        || columns.iter().filter(|(_, _, pk)| *pk > 0).count() != 1
    {
        report.blockers.push("The 'message' table does not match the bundled Electron column types and msgId primary key.".into());
        return Ok(());
    }
    report.schema = "electron-core-message-structural-v1";
    report.schema_supported = true;
    let has_paths = columns
        .iter()
        .any(|(name, kind, _)| name == "localPath" && kind.eq_ignore_ascii_case("TEXT"));
    let path_column = if has_paths { "localPath" } else { "NULL" };
    // One bounded streaming scan, no JSON payload loading and no sort. MIN/MAX
    // preserve original strings and make no claim about timestamp units.
    let sql = format!("SELECT count(*),min(NULLIF(sendDttm,'')),max(NULLIF(sendDttm,'')),sum(CASE WHEN localPath IS NOT NULL AND localPath <> '' THEN 1 ELSE 0 END) FROM (SELECT sendDttm,{path_column} AS localPath FROM message LIMIT ?1)");
    let (rows, earliest, latest, attachments): (u64, Option<String>, Option<String>, Option<u64>) =
        conn.query_row(&sql, [MAX_SCANNED_ROWS + 1], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
    report.scanned_message_rows = Some(rows);
    report.scan_limit_reached = rows > MAX_SCANNED_ROWS;
    if report.scan_limit_reached {
        report.blockers.push("Row scan limit reached; total count, timestamp extrema and attachment totals are intentionally omitted.".into());
    } else {
        report.message_rows = Some(rows);
        report.earliest_send_dttm_raw = earliest;
        report.latest_send_dttm_raw = latest;
        if has_paths {
            report.attachment_path_references = Some(attachments.unwrap_or(0));
        }
    }
    report.blockers.push("A structural schema match does not establish plaintext payloads or safe native-store compatibility.".into());
    Ok(())
}

fn reject_unsafe_source(path: &Path) -> Result<()> {
    let mut component_path = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            bail!("snapshot path must not contain '.' or '..'");
        }
        component_path.push(component.as_os_str());
        if fs::symlink_metadata(&component_path)?
            .file_type()
            .is_symlink()
        {
            bail!("snapshot paths must not contain symlinks");
        }
    }
    if !fs::metadata(path)?.is_file() {
        bail!("snapshot must be a regular copied database file");
    }
    // Lexical deny-list only: never enumerate or open the real Electron profile.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for live in [
            home.join(".config/ZaloData"),
            home.join("Library/Application Support/ZaloData"),
        ] {
            if path.starts_with(live) {
                bail!("refusing a live Electron profile; supply an offline copy elsewhere");
            }
        }
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        if config.is_absolute() && path.starts_with(config.join("ZaloData")) {
            bail!("refusing a live Electron profile; supply an offline copy elsewhere");
        }
    }
    Ok(())
}

fn reject_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-journal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match fs::symlink_metadata(Path::new(&sidecar)) {
            Ok(_) => bail!("snapshot has a SQLite sidecar ({suffix}); checkpoint and copy a closed database first"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn immutable_uri(path: &Path) -> String {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().as_bytes().to_vec();
    let mut uri = String::from("file:");
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b':') {
            uri.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(uri, "%{byte:02X}").expect("write to string");
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

fn fingerprint(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_SNAPSHOT_BYTES {
        bail!("snapshot exceeds the 512 MiB diagnostic limit; inspect a smaller offline copy");
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_read = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total_read += read as u64;
        if total_read > MAX_SNAPSHOT_BYTES {
            bail!("snapshot grew beyond the diagnostic size limit");
        }
        hash.update(&buffer[..read]);
    }
    Ok((size, format!("{:x}", hash.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "zalo-inspection-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> PathBuf {
            self.0.join("copy ?#%.db")
        }
        fn create_core(&self) {
            let conn = Connection::open(self.path()).unwrap();
            // Synthetic data using the exact verified core columns. This fixture
            // makes no claim about a decoded real-user payload.
            conn.execute_batch("CREATE TABLE message (msgId TEXT PRIMARY KEY, cliMsgId TEXT, toUid TEXT, msgType INTEGER, sendDttm TEXT, message TEXT, localPath TEXT);
                INSERT INTO message VALUES ('one','cli-one','42',1,'1700000000123','opaque encrypted payload','/original/image.jpg');
                INSERT INTO message VALUES ('two','cli-two','42',1,'1700000000456','{\"synthetic\":true}',NULL);").unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reports_real_structural_schema_but_never_claims_import_or_decryption() {
        let fixture = Fixture::new();
        fixture.create_core();
        let original = fs::read(fixture.path()).unwrap();
        let first = inspect_snapshot(&fixture.path(), "123456").unwrap();
        let second = inspect_snapshot(&fixture.path(), "654321").unwrap();
        assert!(first.schema_supported && first.source_unchanged);
        assert_eq!(first.message_rows, Some(2));
        assert_eq!(first.attachment_path_references, Some(1));
        assert_eq!(
            first.earliest_send_dttm_raw.as_deref(),
            Some("1700000000123")
        );
        assert!(!first.native_import_supported && !first.source_account_verified);
        assert_eq!(first.imported_messages, 0);
        assert!(!first.credential_migration_performed);
        assert_ne!(first.source_account_claim, second.source_account_claim);
        assert_eq!(first.source_sha256, second.source_sha256);
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn unknown_and_lookalike_schema_fail_closed() {
        let fixture = Fixture::new();
        let conn = Connection::open(fixture.path()).unwrap();
        conn.execute_batch("CREATE TABLE message (msgId TEXT, cliMsgId TEXT, toUid TEXT, msgType INTEGER, sendDttm TEXT, message TEXT);").unwrap();
        drop(conn);
        let report = inspect_snapshot(&fixture.path(), "1").unwrap();
        assert!(!report.schema_supported && !report.native_import_supported);
        assert_eq!(report.message_rows, None);
        assert_eq!(report.imported_messages, 0);
    }

    #[test]
    fn large_inventory_is_bounded_without_reporting_partial_totals() {
        let fixture = Fixture::new();
        fixture.create_core();
        let conn = Connection::open(fixture.path()).unwrap();
        conn.execute_batch("WITH RECURSIVE seq(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM seq WHERE x < 50001) INSERT INTO message (msgId,sendDttm) SELECT 'bulk-'||x,'1700000000123' FROM seq;").unwrap();
        drop(conn);
        let report = inspect_snapshot(&fixture.path(), "1").unwrap();
        assert!(report.scan_limit_reached);
        assert_eq!(report.scanned_message_rows, Some(MAX_SCANNED_ROWS + 1));
        assert_eq!(report.message_rows, None);
        assert_eq!(report.earliest_send_dttm_raw, None);
        assert_eq!(report.attachment_path_references, None);
    }

    #[test]
    fn oversized_snapshot_fails_before_hash_or_sqlite_scan() {
        let fixture = Fixture::new();
        let file = File::create(fixture.path()).unwrap();
        file.set_len(MAX_SNAPSHOT_BYTES + 1).unwrap();
        let err = inspect_snapshot(&fixture.path(), "1").unwrap_err();
        assert!(err.to_string().contains("512 MiB"));
    }

    #[test]
    fn encrypted_or_invalid_database_and_live_sidecars_are_rejected_unchanged() {
        let fixture = Fixture::new();
        fs::write(fixture.path(), b"not SQLite: encrypted input").unwrap();
        assert!(inspect_snapshot(&fixture.path(), "1").is_err());
        assert_eq!(
            fs::read(fixture.path()).unwrap(),
            b"not SQLite: encrypted input"
        );
        fs::remove_file(fixture.path()).unwrap();
        fixture.create_core();
        let mut wal = fixture.path().into_os_string();
        wal.push("-wal");
        fs::write(PathBuf::from(wal), b"uncheckpointed").unwrap();
        assert!(inspect_snapshot(&fixture.path(), "1").is_err());
    }

    #[test]
    fn rejects_missing_identity_relative_paths_and_source_symlink() {
        let fixture = Fixture::new();
        fixture.create_core();
        assert!(inspect_snapshot(&fixture.path(), "").is_err());
        assert!(inspect_snapshot(&fixture.path(), "../1").is_err());
        assert!(inspect_snapshot(Path::new("relative.db"), "1").is_err());
        #[cfg(unix)]
        {
            let link = fixture.0.join("linked.db");
            std::os::unix::fs::symlink(fixture.path(), &link).unwrap();
            assert!(inspect_snapshot(&link, "1").is_err());
        }
    }
}
