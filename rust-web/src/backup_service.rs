use crate::{backend::LogBuffer, primary_config::validate_setting_updates, store::Store};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::Mutex;

const BACKUP_PREFIX: &str = "stream_archive_";
const DEFAULT_INTERVAL_HOURS: u64 = 24;
const DEFAULT_KEEP_COUNT: usize = 10;
const DEFAULT_RETENTION_DAYS: i64 = 3;
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupPolicy {
    pub enabled: bool,
    pub interval_hours: u64,
    pub keep_count: usize,
    pub retention_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupMetadata {
    version: u32,
    created_at: String,
    kind: String,
    source: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupInfo {
    pub file_name: String,
    pub created_at: String,
    pub kind: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupSnapshot {
    pub directory: String,
    pub directory_editable: bool,
    pub policy: BackupPolicy,
    pub backups: Vec<BackupInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreOutcome {
    pub restored: BackupInfo,
    pub safety_backup: BackupInfo,
}

#[derive(Clone)]
pub struct BackupManager {
    store: Store,
    default_backup_dir: Arc<PathBuf>,
    env_override: bool,
    operation: Arc<Mutex<()>>,
}

impl BackupManager {
    pub fn open(store: Store, backend_dir: &Path) -> Result<Self> {
        let env_override = env::var("STREAM_ARCHIVE_BACKUP_DIR")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());
        let default_backup_dir = resolve_backup_dir(backend_dir)?;
        let manager = Self {
            store,
            default_backup_dir: Arc::new(default_backup_dir),
            env_override,
            operation: Arc::new(Mutex::new(())),
        };
        manager.ensure_policy_defaults()?;
        Ok(manager)
    }

    pub fn backup_dir(&self) -> PathBuf {
        if !self.env_override {
            if let Ok(Some(value)) = self.store.setting_value("BACKUP_DIR") {
                let value = value.trim();
                if !value.is_empty() {
                    return PathBuf::from(value);
                }
            }
        }
        self.default_backup_dir.as_ref().clone()
    }

    pub fn backup_dir_editable(&self) -> bool {
        !self.env_override
    }

    pub fn ensure_policy_defaults(&self) -> Result<()> {
        let mut defaults = BTreeMap::new();
        for (key, value) in [
            ("BACKUP_ENABLED", "Y"),
            ("BACKUP_INTERVAL_HOURS", "24"),
            ("BACKUP_KEEP_COUNT", "10"),
            ("BACKUP_RETENTION_DAYS", "3"),
            ("BACKUP_DIR", ""),
        ] {
            if self.store.setting_value(key)?.is_none() {
                defaults.insert(key.to_string(), value.to_string());
            }
        }
        if !defaults.is_empty() {
            self.store.sync_settings(&defaults, "runtime-default")?;
        }
        Ok(())
    }

    pub fn policy(&self) -> Result<BackupPolicy> {
        let enabled = self
            .store
            .setting_value("BACKUP_ENABLED")?
            .unwrap_or_else(|| "Y".into())
            .eq_ignore_ascii_case("Y");
        let interval_hours = parse_u64(
            self.store.setting_value("BACKUP_INTERVAL_HOURS")?,
            DEFAULT_INTERVAL_HOURS,
        )
        .max(1);
        let keep_count = parse_u64(
            self.store.setting_value("BACKUP_KEEP_COUNT")?,
            DEFAULT_KEEP_COUNT as u64,
        ) as usize;
        let retention_days = parse_i64(
            self.store.setting_value("BACKUP_RETENTION_DAYS")?,
            DEFAULT_RETENTION_DAYS,
        )
        .max(0);
        Ok(BackupPolicy {
            enabled,
            interval_hours,
            keep_count,
            retention_days,
        })
    }

    pub async fn snapshot(&self) -> Result<BackupSnapshot> {
        let _guard = self.operation.lock().await;
        Ok(BackupSnapshot {
            directory: self.backup_dir().display().to_string(),
            directory_editable: self.backup_dir_editable(),
            policy: self.policy()?,
            backups: self.list_locked()?,
        })
    }

    pub async fn update_policy(
        &self,
        policy: &BackupPolicy,
        directory: Option<&str>,
    ) -> Result<BackupSnapshot> {
        if policy.interval_hours < 1 {
            bail!("backup interval must be at least 1 hour");
        }
        if policy.retention_days < 0 {
            bail!("backup retention days must not be negative");
        }

        let _guard = self.operation.lock().await;
        let mut updates = BTreeMap::from([
            (
                "BACKUP_ENABLED".to_string(),
                if policy.enabled { "Y" } else { "N" }.to_string(),
            ),
            (
                "BACKUP_INTERVAL_HOURS".to_string(),
                policy.interval_hours.to_string(),
            ),
            (
                "BACKUP_KEEP_COUNT".to_string(),
                policy.keep_count.to_string(),
            ),
            (
                "BACKUP_RETENTION_DAYS".to_string(),
                policy.retention_days.to_string(),
            ),
        ]);

        if let Some(directory) = directory {
            if !self.backup_dir_editable() {
                bail!("backup directory is controlled by STREAM_ARCHIVE_BACKUP_DIR");
            }
            updates.insert("BACKUP_DIR".into(), directory.trim().to_string());
        }

        // Reuse the same canonical settings validation as the existing Web
        // settings path, including numeric ranges and writable-directory checks.
        validate_setting_updates(&updates)?;
        self.store.sync_settings(&updates, "native-backup")?;
        self.ensure_policy_defaults()?;
        Ok(BackupSnapshot {
            directory: self.backup_dir().display().to_string(),
            directory_editable: self.backup_dir_editable(),
            policy: self.policy()?,
            backups: self.list_locked()?,
        })
    }

    pub async fn create_manual(&self) -> Result<BackupInfo> {
        let _guard = self.operation.lock().await;
        self.create_locked("manual")
    }

    async fn create_auto_if_due(&self) -> Result<Option<BackupInfo>> {
        let _guard = self.operation.lock().await;
        let policy = self.policy()?;
        if !policy.enabled {
            return Ok(None);
        }
        let backups = self.list_locked()?;
        let newest_auto = backups
            .iter()
            .filter(|item| item.kind == "auto")
            .filter_map(|item| DateTime::parse_from_rfc3339(&item.created_at).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .max();
        let due = newest_auto
            .map(|last| Utc::now() - last >= ChronoDuration::hours(policy.interval_hours as i64))
            .unwrap_or(true);
        if due {
            Ok(Some(self.create_locked("auto")?))
        } else {
            self.cleanup_locked(&policy)?;
            Ok(None)
        }
    }

    pub async fn restore(&self, file_name: &str) -> Result<RestoreOutcome> {
        let _guard = self.operation.lock().await;
        let source = self.resolve_file(file_name)?;
        let selected = inspect_backup(&source)?;
        if selected.integrity != "OK" {
            bail!("backup integrity check failed: {}", selected.integrity);
        }
        let safety = self.create_locked_with_cleanup("pre_restore", false)?;
        self.store.restore_from(&source)?;
        self.store.ensure_schema()?;
        self.ensure_policy_defaults()?;
        let policy = self.policy()?;
        self.cleanup_locked(&policy)?;
        Ok(RestoreOutcome {
            restored: selected,
            safety_backup: safety,
        })
    }

    fn create_locked(&self, kind: &str) -> Result<BackupInfo> {
        self.create_locked_with_cleanup(kind, true)
    }

    fn create_locked_with_cleanup(&self, kind: &str, cleanup: bool) -> Result<BackupInfo> {
        let backup_dir = self.backup_dir();
        fs::create_dir_all(&backup_dir)?;
        let stamp = Utc::now().format("%Y%m%d_%H%M%S_%3f");
        let file_name = format!("{BACKUP_PREFIX}{kind}_{stamp}.db");
        let path = backup_dir.join(&file_name);
        self.store.backup_to(&path)?;
        verify_sqlite(&path)?;
        let sha256 = sha256_file(&path)?;
        let size_bytes = fs::metadata(&path)?.len();
        let metadata = BackupMetadata {
            version: 1,
            created_at: Utc::now().to_rfc3339(),
            kind: kind.to_string(),
            source: self.store.path().display().to_string(),
            sha256: sha256.clone(),
            size_bytes,
        };
        fs::write(
            path.with_extension("db.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        if cleanup {
            let policy = self.policy()?;
            self.cleanup_locked(&policy)?;
        }
        Ok(BackupInfo {
            file_name,
            created_at: metadata.created_at,
            kind: metadata.kind,
            size_bytes,
            sha256,
            integrity: "OK".into(),
        })
    }

    fn list_locked(&self) -> Result<Vec<BackupInfo>> {
        let backup_dir = self.backup_dir();
        fs::create_dir_all(&backup_dir)?;
        let mut items = Vec::new();
        for entry in fs::read_dir(&backup_dir)? {
            let path = entry?.path();
            if !is_backup_database_name(&path) {
                continue;
            }
            if let Ok(info) = inspect_backup(&path) {
                items.push(info);
            }
        }
        items.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.file_name.cmp(&a.file_name))
        });
        Ok(items)
    }

    fn cleanup_locked(&self, policy: &BackupPolicy) -> Result<usize> {
        let backup_dir = self.backup_dir();
        fs::create_dir_all(&backup_dir)?;
        let mut files: Vec<PathBuf> = fs::read_dir(&backup_dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| is_owned_backup(path))
            .collect();
        files.sort_by_key(|path| {
            std::cmp::Reverse(
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
            )
        });
        let now = SystemTime::now();
        let mut removed = 0;
        for (index, path) in files.into_iter().enumerate() {
            let too_many = policy.keep_count > 0 && index >= policy.keep_count;
            let too_old = if policy.retention_days > 0 {
                fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age.as_secs() > policy.retention_days as u64 * 86_400)
            } else {
                false
            };
            if too_many || too_old {
                fs::remove_file(&path)?;
                let _ = fs::remove_file(path.with_extension("db.json"));
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn resolve_file(&self, file_name: &str) -> Result<PathBuf> {
        let trimmed = file_name.trim();
        if trimmed.is_empty()
            || !trimmed.starts_with(BACKUP_PREFIX)
            || !trimmed.ends_with(".db")
            || Path::new(trimmed)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(trimmed)
        {
            bail!("invalid backup file name");
        }
        let path = self.backup_dir().join(trimmed);
        if !path.is_file() {
            bail!("backup not found: {trimmed}");
        }
        Ok(path)
    }
}

pub fn spawn_auto_backup(manager: BackupManager, logs: LogBuffer) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            match manager.create_auto_if_due().await {
                Ok(Some(item)) => {
                    logs.push(format!(
                        "[BACKUP] automatic backup created file={} size={} sha256={}",
                        item.file_name, item.size_bytes, item.sha256
                    ))
                    .await;
                }
                Ok(None) => {}
                Err(error) => {
                    logs.push(format!("[BACKUP:WARN] automatic backup failed: {error:#}"))
                        .await;
                }
            }
            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}

fn is_backup_database_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.starts_with(BACKUP_PREFIX) && name.ends_with(".db"))
}

fn is_owned_backup(path: &Path) -> bool {
    if !is_backup_database_name(path) || !path.with_extension("db.json").is_file() {
        return false;
    }
    inspect_backup(path)
        .map(|info| info.integrity == "OK")
        .unwrap_or(false)
}

pub fn resolve_backup_dir(backend_dir: &Path) -> Result<PathBuf> {
    if let Ok(value) = env::var("STREAM_ARCHIVE_BACKUP_DIR") {
        if !value.trim().is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    let app_root = backend_dir.parent().unwrap_or(backend_dir);
    let parent = app_root.parent().unwrap_or(app_root);
    Ok(parent.join("stream-archive-backups"))
}

fn inspect_backup(path: &Path) -> Result<BackupInfo> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("invalid backup file name")?
        .to_string();
    let metadata_path = path.with_extension("db.json");
    let metadata = fs::read(&metadata_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BackupMetadata>(&bytes).ok());
    let size_bytes = fs::metadata(path)?.len();
    let actual_sha = sha256_file(path)?;
    let db_ok = verify_sqlite(path).is_ok();
    let integrity = match &metadata {
        Some(_) if !db_ok => "INVALID_SQLITE",
        Some(meta)
            if meta.sha256.eq_ignore_ascii_case(&actual_sha) && meta.size_bytes == size_bytes =>
        {
            "OK"
        }
        Some(_) => "HASH_MISMATCH",
        None if db_ok => "NO_METADATA",
        None => "INVALID_SQLITE",
    }
    .to_string();
    let created_at = metadata
        .as_ref()
        .map(|metadata| metadata.created_at.clone())
        .unwrap_or_else(|| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(|_| Utc::now())
                .to_rfc3339()
        });
    let kind = metadata
        .as_ref()
        .map(|metadata| metadata.kind.clone())
        .unwrap_or_else(|| infer_kind(&file_name));
    Ok(BackupInfo {
        file_name,
        created_at,
        kind,
        size_bytes,
        sha256: actual_sha,
        integrity,
    })
}

fn infer_kind(file_name: &str) -> String {
    for kind in ["manual", "auto", "pre_restore"] {
        if file_name.contains(&format!("_{kind}_")) {
            return kind.into();
        }
    }
    "unknown".into()
}

fn verify_sqlite(path: &Path) -> Result<()> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if !result.eq_ignore_ascii_case("ok") {
        bail!("SQLite quick_check: {result}");
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn parse_u64(value: Option<String>, default: u64) -> u64 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_i64(value: Option<String>, default: i64) -> i64 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, Store) {
        let dir = tempdir().unwrap();
        let app = dir.path().join("stream-archive");
        let backend = app.join("backend");
        fs::create_dir_all(&backend).unwrap();
        let store = Store::open(app.join("data").join("stream-archive.db")).unwrap();
        (dir, app, backend, store)
    }

    #[test]
    fn backup_directory_is_sibling_of_application_root() {
        let (dir, _app, backend, _store) = setup();
        let resolved = resolve_backup_dir(&backend).unwrap();
        assert_eq!(resolved, dir.path().join("stream-archive-backups"));
    }

    #[test]
    fn default_backup_retention_is_three_days() {
        let (_dir, _app, backend, store) = setup();
        let manager = BackupManager::open(store, &backend).unwrap();
        assert_eq!(manager.policy().unwrap().retention_days, 3);
    }

    #[test]
    fn persisted_backup_directory_updates_without_restart() {
        let (dir, _app, backend, store) = setup();
        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        let first = dir.path().join("backup-a");
        let second = dir.path().join("backup-b");
        store
            .sync_settings(
                &BTreeMap::from([("BACKUP_DIR".into(), first.display().to_string())]),
                "test",
            )
            .unwrap();
        assert_eq!(manager.backup_dir(), first);
        store
            .sync_settings(
                &BTreeMap::from([("BACKUP_DIR".into(), second.display().to_string())]),
                "test",
            )
            .unwrap();
        assert_eq!(manager.backup_dir(), second);
    }

    #[test]
    fn online_backup_round_trip_restores_database_and_creates_safety_copy() {
        let (_dir, _app, backend, store) = setup();
        store
            .sync_settings(&BTreeMap::from([("TEST".into(), "before".into())]), "test")
            .unwrap();
        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("BACKUP_KEEP_COUNT".into(), "10".into())]),
                "test",
            )
            .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let backup = runtime.block_on(manager.create_manual()).unwrap();
        store
            .sync_settings(&BTreeMap::from([("TEST".into(), "after".into())]), "test")
            .unwrap();
        let outcome = runtime
            .block_on(manager.restore(&backup.file_name))
            .unwrap();
        assert_eq!(
            store.setting_value("TEST").unwrap().as_deref(),
            Some("before")
        );
        assert_eq!(outcome.restored.file_name, backup.file_name);
        assert_eq!(outcome.safety_backup.kind, "pre_restore");
    }

    #[test]
    fn retention_does_not_delete_unowned_databases() {
        let (_dir, _app, backend, store) = setup();
        let manager = BackupManager::open(store, &backend).unwrap();
        std::fs::create_dir_all(manager.backup_dir()).unwrap();
        let unrelated = manager.backup_dir().join("other-application.db");
        rusqlite::Connection::open(&unrelated).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(manager.create_manual()).unwrap();
        let policy = BackupPolicy {
            enabled: true,
            interval_hours: 24,
            keep_count: 1,
            retention_days: 1,
        };
        manager.cleanup_locked(&policy).unwrap();
        assert!(unrelated.is_file());
    }

    #[test]
    fn policy_validation_rejects_zero_interval() {
        let (_dir, _app, backend, store) = setup();
        let manager = BackupManager::open(store, &backend).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let policy = BackupPolicy {
            enabled: true,
            interval_hours: 0,
            keep_count: 10,
            retention_days: 3,
        };
        assert!(
            runtime
                .block_on(manager.update_policy(&policy, None))
                .is_err()
        );
    }
}
