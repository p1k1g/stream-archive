use crate::{
    backend::{HIDDEN_SETTING_KEYS, SAFE_SETTING_KEYS},
    model::{Channel, LiveHistoryItem, VodJobStatus},
    primary_config::VOD_TOOL_KEYS,
    support::platform::PlatformId,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, TransactionBehavior, backup::Backup, params,
};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, RwLock,
        atomic::{AtomicI64, Ordering},
    },
};

static GLOBAL_STORE: OnceLock<Store> = OnceLock::new();

const DATABASE_FILE: &str = "stream-archive.db";
const LEGACY_DATABASE_FILE: &str = "soop.db";

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    source TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS channels (
    platform TEXT NOT NULL COLLATE NOCASE DEFAULT 'SOOP',
    account TEXT NOT NULL COLLATE NOCASE,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    outdir TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(platform, account)
);

CREATE TABLE IF NOT EXISTS live_recordings (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL DEFAULT 'SOOP',
    account TEXT NOT NULL,
    channel_name TEXT NOT NULL,
    bno TEXT,
    title TEXT,
    file_path TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    duration_seconds INTEGER NOT NULL DEFAULT 0,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_live_recordings_started
    ON live_recordings(started_at DESC);

CREATE TABLE IF NOT EXISTS vod_jobs (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL DEFAULT 'SOOP',
    kind TEXT NOT NULL,
    vod_url TEXT,
    title TEXT,
    streamer TEXT,
    part_count INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    output_file TEXT,
    message TEXT,
    started_at TEXT,
    finished_at TEXT,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_vod_jobs_started
    ON vod_jobs(started_at DESC, updated_at DESC);

CREATE TABLE IF NOT EXISTS vod_queue (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL DEFAULT 'SOOP',
    request_json TEXT NOT NULL,
    vod_url TEXT NOT NULL,
    output_directory TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    message TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    streamer TEXT NOT NULL DEFAULT '',
    output_file TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_vod_queue_state_created
    ON vod_queue(state, created_at);
"#;

#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<Connection>>,
    path: PathBuf,
    // Read-through snapshot of committed SQLite state. Mutations update this
    // only after their transaction commits; it is not a second authority.
    settings_cache: Arc<RwLock<BTreeMap<String, String>>>,
    channels_cache: Arc<RwLock<Vec<Channel>>>,
    last_data_version: Arc<AtomicI64>,
}

#[derive(Debug, Clone, Default)]
pub struct StorePreflightSettings {
    pub values: BTreeMap<String, String>,
    pub configured_secrets: BTreeMap<String, bool>,
}

pub fn init_global(store: Store) -> Result<()> {
    GLOBAL_STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("SQLite store already initialized"))
}

pub fn global() -> Result<Store> {
    GLOBAL_STORE
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("SQLite store is not initialized"))
}

impl Store {
    pub fn default_path(backend_dir: &Path) -> PathBuf {
        if let Ok(value) = env::var("STREAM_ARCHIVE_DATA_DIR")
            && !value.trim().is_empty()
        {
            return PathBuf::from(value).join(DATABASE_FILE);
        }
        backend_dir
            .parent()
            .unwrap_or(backend_dir)
            .join("data")
            .join(DATABASE_FILE)
    }

    /// Load diagnostics inputs through the canonical Store persistence rules
    /// without creating, migrating, or mutating the database.
    pub fn read_preflight_settings(path: &Path) -> Result<StorePreflightSettings> {
        if !path.is_file() {
            return Ok(StorePreflightSettings::default());
        }

        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let table_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings')",
            [],
            |row| row.get(0),
        )?;
        if !table_exists {
            return Ok(StorePreflightSettings::default());
        }

        let all = load_all_settings_from_conn(&conn)?;
        let mut values = BTreeMap::new();
        for key in SAFE_SETTING_KEYS.iter().chain(VOD_TOOL_KEYS.iter()) {
            if let Some(value) = all.get(*key) {
                values.insert((*key).to_string(), value.clone());
            }
        }
        let configured_secrets = HIDDEN_SETTING_KEYS
            .iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    all.get(*key).is_some_and(|value| !value.trim().is_empty()),
                )
            })
            .collect();

        Ok(StorePreflightSettings {
            values,
            configured_secrets,
        })
    }

    pub fn migrate_legacy_database(path: &Path) -> Result<bool> {
        if path.is_file() {
            return Ok(false);
        }
        let Some(parent) = path.parent() else {
            return Ok(false);
        };
        let legacy = parent.join(LEGACY_DATABASE_FILE);
        if !legacy.is_file() {
            return Ok(false);
        }

        fs::create_dir_all(parent)?;
        let source = Connection::open(&legacy)
            .with_context(|| format!("failed to open legacy database {}", legacy.display()))?;
        let mut target = Connection::open(path).with_context(|| {
            format!(
                "failed to create Stream Archive database {}",
                path.display()
            )
        })?;
        {
            let backup = Backup::new(&source, &mut target)?;
            backup.run_to_completion(256, std::time::Duration::from_millis(2), None)?;
        }
        target.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(target);
        drop(source);

        fs::remove_file(&legacy)
            .with_context(|| format!("failed to remove legacy database {}", legacy.display()))?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = sidecar_path(&legacy, suffix);
            if sidecar.exists() {
                let _ = fs::remove_file(sidecar);
            }
        }
        Ok(true)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        Self::open_with_recovery(path, true)
    }

    pub fn open_observer(path: PathBuf) -> Result<Self> {
        Self::open_with_recovery(path, false)
    }

    fn open_with_recovery(path: PathBuf, recover_interrupted: bool) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create data directory {}", parent.display()))?;
        }
        let mut conn = Connection::open(&path)
            .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA_SQL)?;
        ensure_multiplatform_schema(&mut conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        let settings_cache = load_all_settings_from_conn(&conn)?;
        let channels_cache = load_channels_from_conn(&conn)?;
        let data_version = Self::query_data_version(&conn)?;

        let store = Self {
            inner: Arc::new(Mutex::new(conn)),
            path,
            settings_cache: Arc::new(RwLock::new(settings_cache)),
            channels_cache: Arc::new(RwLock::new(channels_cache)),
            last_data_version: Arc::new(AtomicI64::new(data_version)),
        };
        store.ensure_runtime_defaults()?;
        if recover_interrupted {
            store.recover_interrupted()?;
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn ensure_runtime_defaults(&self) -> Result<()> {
        let defaults = [
            ("CHECK_INTERVAL", "30"),
            ("CHANNEL_RELOAD_INTERVAL", "2"),
            ("RECORD_RETRY_INTERVAL", "5"),
            ("RECORD_STALL_TIMEOUT", "90"),
            ("RECORD_MONITOR_INTERVAL", "5"),
            ("WORKER_MAX_RETRY", "3"),
            ("CONSOLE_REFRESH_INTERVAL", "5"),
            ("CONSOLE_AUTO_FORMAT", "Y"),
            ("CHANNEL_NAME_WIDTH", "AUTO"),
            ("CONSOLE_COLOR", "Y"),
            ("CONSOLE_SHOW_PATH", "N"),
            ("GUI_NOTIFY_RECORD_START", "N"),
            ("GUI_NOTIFY_RECORD_FINISH", "Y"),
            ("GUI_NOTIFY_WARNING", "Y"),
            ("GUI_AUTO_START_WATCHER", "N"),
            ("GUI_START_MINIMIZED_TRAY", "N"),
            ("STREAM_ARCHIVE_CLOSE_ACTION", "EXIT"),
            ("STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS", "true"),
            ("MIN_FREE_SPACE_GB", "20"),
            ("OUTPUT_DIR", ""),
            ("QUALITY", "best"),
            ("FILE_NAME_PATTERN", "LEGACY"),
            ("LIVE_AUTO_REMUX", "OFF"),
            ("LIVE_REMUX_DELETE_SOURCE", "N"),
            ("WEBHOOK_ENABLED", "N"),
            ("WEBHOOK_NOTIFY_LIVE", "Y"),
            ("WEBHOOK_NOTIFY_VOD", "Y"),
            ("WEBHOOK_NOTIFY_WARNING", "Y"),
            ("TELEGRAM_CHAT_ID", ""),
            ("STREAMLINK_PATH", "AUTO"),
            ("STREAMLINK_FALLBACK", "AUTO"),
            ("SOOP_USERNAME", ""),
            ("CLOUDFLARE_WORKER_URL", ""),
            ("MASTER_QUALITY", "auto"),
            ("LOG_ENABLED", "Y"),
            ("LOG_DIR", ".\\logs"),
            ("LOG_RETENTION_DAYS", "30"),
            ("BACKUP_ENABLED", "Y"),
            ("BACKUP_INTERVAL_HOURS", "24"),
            ("BACKUP_KEEP_COUNT", "10"),
            ("BACKUP_RETENTION_DAYS", "3"),
            ("BACKUP_DIR", ""),
            ("YT_DLP_PATH", ""),
            ("FFMPEG_PATH", ""),
        ];
        let mut missing = BTreeMap::new();
        {
            let cache = self
                .settings_cache
                .read()
                .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?;
            for (key, value) in defaults {
                if !cache.contains_key(key) {
                    missing.insert(key.to_string(), value.to_string());
                }
            }
        }
        if !missing.is_empty() {
            self.sync_settings(&missing, "runtime-default")?;
        }
        Ok(())
    }

    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        if destination.exists() {
            anyhow::bail!(
                "backup destination already exists: {}",
                destination.display()
            );
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = self.conn()?;
        let mut target = Connection::open(destination)
            .with_context(|| format!("failed to create backup {}", destination.display()))?;
        let backup = Backup::new(&source, &mut target)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(2), None)?;
        drop(backup);
        target.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn restore_from(&self, source_path: &Path) -> Result<()> {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("failed to open backup {}", source_path.display()))?;
        let check: String = source.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if !check.eq_ignore_ascii_case("ok") {
            anyhow::bail!("backup SQLite quick_check failed: {check}");
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let _credential_guard = crate::security::kick_secret_guard(self).context(
            "KICK 인증정보 사용 중에는 백업을 복원할 수 없습니다. 잠시 후 재시도하세요.",
        )?;
        let mut target = self.conn()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let source = Self::restore_source_preserving_credentials(&source, &target)?;
        let backup = Backup::new(&source, &mut target)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(2), None)?;
        drop(backup);
        target.execute_batch("PRAGMA foreign_keys=ON;")?;
        target.execute_batch(SCHEMA_SQL)?;
        ensure_multiplatform_schema(&mut target)?;
        target.execute_batch(SCHEMA_SQL)?;
        target.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.refresh_config_cache_from_conn(&target)?;
        drop(target);
        self.ensure_runtime_defaults()?;
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn restore_source_preserving_credentials(
        source: &Connection,
        current: &Connection,
    ) -> Result<Connection> {
        // SQLite owns/removes this private temporary database. Do not modify the
        // selected backup or decrypt native secrets into a staging file.
        let mut staged = Connection::open("")?;
        let backup = Backup::new(source, &mut staged)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(2), None)?;
        drop(backup);
        staged.execute_batch(SCHEMA_SQL)?;
        ensure_multiplatform_schema(&mut staged)?;
        let tx = staged.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for key in [
            "KICK_SESSION_TOKEN",
            "STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS",
            "SOOP_USERNAME",
            "SOOP_PASSWORD",
            "CLOUDFLARE_WORKER_URL",
            "CLOUDFLARE_API_KEY",
            "CHZZK_NID_AUT",
            "CHZZK_NID_SES",
            "DISCORD_WEBHOOK_URL",
            "TELEGRAM_BOT_TOKEN",
            "TELEGRAM_CHAT_ID",
            "STREAM_ARCHIVE_SOOP_PASSWORD_CLEANUP_REFS",
            "STREAM_ARCHIVE_CLOUDFLARE_API_KEY_CLEANUP_REFS",
            "STREAM_ARCHIVE_CHZZK_NID_AUT_CLEANUP_REFS",
            "STREAM_ARCHIVE_CHZZK_NID_SES_CLEANUP_REFS",
            "STREAM_ARCHIVE_DISCORD_WEBHOOK_URL_CLEANUP_REFS",
            "STREAM_ARCHIVE_TELEGRAM_BOT_TOKEN_CLEANUP_REFS",
        ] {
            let row: Option<(String, String, String)> = current
                .query_row(
                    "SELECT value,source,updated_at FROM settings WHERE key=?1",
                    [key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            tx.execute("DELETE FROM settings WHERE key=?1", [key])?;
            if let Some((value, source, updated_at)) = row {
                tx.execute(
                    "INSERT INTO settings(key,value,source,updated_at) VALUES(?1,?2,?3,?4)",
                    params![key, value, source, updated_at],
                )?;
            }
        }
        tx.commit()?;
        // All preserved rows are present before the backup API touches the live
        // DB, so failures cannot leave a restored obsolete native reference.
        Ok(staged)
    }

    pub fn ensure_schema(&self) -> Result<()> {
        let mut conn = self.conn()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA_SQL)?;
        ensure_multiplatform_schema(&mut conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        self.refresh_config_cache_from_conn(&conn)?;
        drop(conn);
        self.ensure_runtime_defaults()?;
        self.recover_interrupted()?;
        Ok(())
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.inner
            .lock()
            .map_err(|_| anyhow::anyhow!("SQLite connection mutex poisoned"))
    }

    pub(crate) fn refresh_config_cache(&self) -> Result<()> {
        let conn = self.conn()?;
        self.refresh_config_cache_from_conn(&conn)
    }

    /// Refresh the in-memory settings/channels cache only if another SQLite
    /// connection has committed changes since the last observed `PRAGMA data_version`.
    pub(crate) fn refresh_config_cache_if_changed(&self) -> Result<bool> {
        let conn = self.conn()?;
        let current = Self::query_data_version(&conn)?;
        if current == self.last_data_version.load(Ordering::Acquire) {
            return Ok(false);
        }
        self.refresh_config_cache_from_conn(&conn)?;
        Ok(true)
    }

    fn query_data_version(conn: &Connection) -> Result<i64> {
        conn.query_row("PRAGMA data_version", [], |row| row.get(0))
            .context("failed to read SQLite PRAGMA data_version")
    }

    fn refresh_config_cache_from_conn(&self, conn: &Connection) -> Result<()> {
        let data_version = Self::query_data_version(conn)?;
        let settings = load_all_settings_from_conn(conn)?;
        let channels = load_channels_from_conn(conn)?;
        *self
            .settings_cache
            .write()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))? = settings;
        *self
            .channels_cache
            .write()
            .map_err(|_| anyhow::anyhow!("channels cache lock poisoned"))? = channels;
        self.last_data_version
            .store(data_version, Ordering::Release);
        Ok(())
    }

    pub(crate) fn recover_interrupted(&self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn()?;
        conn.execute(
            "UPDATE live_recordings SET ended_at=?1, reason='SERVER RESTART', status='INTERRUPTED' WHERE ended_at IS NULL",
            params![now],
        )?;
        conn.execute(
            "UPDATE vod_jobs SET state='INTERRUPTED', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','ANALYZING','DOWNLOADING','REFRESHING','MERGING','CANCELLING')",
            params![now],
        )?;
        conn.execute(
            "UPDATE vod_queue SET state='INTERRUPTED', message='서버 재시작으로 중단됨 · 재시도 가능', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','RUNNING','CANCELLING')",
            params![now],
        )?;
        Ok(())
    }

    pub fn is_first_run_unconfigured(&self) -> Result<bool> {
        let conn = self.conn()?;
        let has_user_state: i64 = conn.query_row(
            r#"SELECT
                EXISTS(SELECT 1 FROM settings WHERE source NOT IN ('runtime-default', 'native-window'))
                OR EXISTS(SELECT 1 FROM channels)
                OR EXISTS(SELECT 1 FROM live_recordings)
                OR EXISTS(SELECT 1 FROM vod_jobs)
                OR EXISTS(SELECT 1 FROM vod_queue)"#,
            [],
            |row| row.get(0),
        )?;
        Ok(has_user_state == 0)
    }

    pub fn safe_settings(&self) -> Result<BTreeMap<String, String>> {
        self.settings_for_keys(SAFE_SETTING_KEYS)
    }

    pub fn live_settings_with_secrets(&self) -> Result<BTreeMap<String, String>> {
        let mut keys = SAFE_SETTING_KEYS.to_vec();
        keys.extend_from_slice(HIDDEN_SETTING_KEYS);
        self.settings_for_keys(&keys)
    }

    pub fn vod_tool_settings(&self) -> Result<BTreeMap<String, String>> {
        let mut values = self.settings_for_keys(VOD_TOOL_KEYS)?;
        for key in VOD_TOOL_KEYS {
            values.entry((*key).to_string()).or_default();
        }
        Ok(values)
    }

    pub fn settings_for_keys(&self, keys: &[&str]) -> Result<BTreeMap<String, String>> {
        let cache = self
            .settings_cache
            .read()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?;
        Ok(keys
            .iter()
            .filter_map(|key| {
                cache
                    .get(*key)
                    .map(|value| ((*key).to_string(), value.clone()))
            })
            .collect())
    }

    pub fn configured_secrets(&self) -> Result<BTreeMap<String, bool>> {
        let values = self.settings_for_keys(HIDDEN_SETTING_KEYS)?;
        Ok(HIDDEN_SETTING_KEYS
            .iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    values
                        .get(*key)
                        .is_some_and(|value| !value.trim().is_empty()),
                )
            })
            .collect())
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        Ok(self
            .channels_cache
            .read()
            .map_err(|_| anyhow::anyhow!("channels cache lock poisoned"))?
            .clone())
    }

    /// Commit a replacement and its opaque cleanup references in one SQLite transaction.
    pub(crate) fn sync_settings_retiring_secret(
        &self,
        values: &BTreeMap<String, String>,
        source: &str,
        secret_key: &str,
        cleanup_key: &str,
        retire: impl FnOnce(&str) -> Result<Option<String>>,
    ) -> Result<()> {
        let mut retire = Some(retire);
        self.sync_settings_retiring_secrets(
            values,
            source,
            &[(secret_key.into(), cleanup_key.into())],
            |_, value| retire.take().expect("single retirement callback")(value),
        )
    }

    pub(crate) fn sync_settings_retiring_secrets(
        &self,
        values: &BTreeMap<String, String>,
        source: &str,
        secret_keys: &[(String, String)],
        mut retire: impl FnMut(&str, &str) -> Result<Option<String>>,
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let read = |key: &str| -> Result<String> {
            Ok(tx
                .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?
                .unwrap_or_default())
        };
        let mut journals = BTreeMap::new();
        for (secret_key, cleanup_key) in secret_keys {
            let previous = read(secret_key)?;
            let pending = read(cleanup_key)?;
            let mut pending: Vec<String> = if pending.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&pending).context("invalid secret cleanup references")?
            };
            if values
                .get(secret_key)
                .is_some_and(|value| value != &previous)
                && let Some(reference) = retire(secret_key, &previous)?
                && !pending.contains(&reference)
            {
                pending.push(reference);
            }
            if let Some(value) = values.get(secret_key) {
                pending.retain(|reference| reference != value);
            }
            let pending = serde_json::to_string(&pending)?;
            journals.insert(cleanup_key.clone(), pending);
        }
        let now = Utc::now().to_rfc3339();
        for (key, value) in values {
            tx.execute(
                "INSERT INTO settings(key,value,source,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(key) DO UPDATE SET value=excluded.value,source=excluded.source,updated_at=excluded.updated_at",
                params![key, value, source, now],
            )?;
        }
        for (cleanup_key, pending) in &journals {
            tx.execute(
            "INSERT INTO settings(key,value,source,updated_at) VALUES(?1,?2,'native-secret-cleanup',?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
            params![cleanup_key, pending, now],
        )?;
        }
        tx.commit()?;
        let mut cache = self
            .settings_cache
            .write()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?;
        for (key, value) in values {
            cache.insert(key.clone(), value.clone());
        }
        cache.extend(journals);
        Ok(())
    }

    pub(crate) fn pending_secret_cleanup(&self, key: &str) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let value: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        match value.filter(|value| !value.is_empty()) {
            Some(value) => {
                serde_json::from_str(&value).context("invalid secret cleanup references")
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) fn retain_secret_cleanup(&self, key: &str, reference: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value: Option<String> = tx
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let mut pending: Vec<String> = match value.filter(|value| !value.is_empty()) {
            Some(value) => {
                serde_json::from_str(&value).context("invalid secret cleanup references")?
            }
            None => Vec::new(),
        };
        if !pending.iter().any(|value| value == reference) {
            pending.push(reference.to_string());
        }
        let value = serde_json::to_string(&pending)?;
        tx.execute(
            "INSERT INTO settings(key,value,source,updated_at) VALUES(?1,?2,'native-secret-cleanup',?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        self.settings_cache
            .write()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?
            .insert(key.to_string(), value);
        Ok(())
    }

    pub(crate) fn finish_secret_cleanup(&self, key: &str, reference: &str) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value: Option<String> = tx
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let mut pending: Vec<String> = match value.filter(|value| !value.is_empty()) {
            Some(value) => {
                serde_json::from_str(&value).context("invalid secret cleanup references")?
            }
            None => Vec::new(),
        };
        pending.retain(|value| value != reference);
        let value = serde_json::to_string(&pending)?;
        tx.execute(
            "UPDATE settings SET value=?2,updated_at=?3 WHERE key=?1",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        self.settings_cache
            .write()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?
            .insert(key.to_string(), value);
        Ok(())
    }

    pub fn sync_settings(&self, values: &BTreeMap<String, String>, source: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for (key, value) in values {
            tx.execute(
                "INSERT INTO settings(key,value,source,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(key) DO UPDATE SET value=excluded.value, source=excluded.source, updated_at=excluded.updated_at",
                params![key, value, source, now],
            )?;
        }
        tx.commit()?;
        let mut cache = self
            .settings_cache
            .write()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?;
        for (key, value) in values {
            cache.insert(key.clone(), value.clone());
        }
        Ok(())
    }

    pub fn sync_channels(&self, channels: &[Channel]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM channels", [])?;
        for channel in channels {
            tx.execute(
                "INSERT INTO channels(platform,account,name,enabled,outdir,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![channel.platform.as_str(), channel.account, channel.name, i64::from(channel.enabled), channel.outdir, now],
            )?;
        }
        tx.commit()?;
        let sorted_channels = load_channels_from_conn(&conn)?;
        *self
            .channels_cache
            .write()
            .map_err(|_| anyhow::anyhow!("channels cache lock poisoned"))? = sorted_channels;
        Ok(())
    }

    pub fn insert_channel(&self, channel: &Channel) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn()?;
        let changed = conn.execute(
            "INSERT INTO channels(platform,account,name,enabled,outdir,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(platform,account) DO NOTHING",
            params![
                channel.platform.as_str(),
                channel.account,
                channel.name,
                i64::from(channel.enabled),
                channel.outdir,
                now
            ],
        )?;
        if changed == 0 {
            anyhow::bail!(
                "duplicate channel identity: {}/{}",
                channel.platform,
                channel.account
            );
        }
        self.refresh_config_cache_from_conn(&conn)
    }

    pub fn delete_channel(&self, platform: PlatformId, account: &str) -> Result<()> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "DELETE FROM channels WHERE platform=?1 AND account=?2",
            params![platform.as_str(), account],
        )?;
        if changed == 0 {
            anyhow::bail!("channel not found: {platform}/{account}");
        }
        self.refresh_config_cache_from_conn(&conn)
    }

    pub fn set_channel_enabled(
        &self,
        platform: PlatformId,
        account: &str,
        enabled: bool,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE channels SET enabled=?3, updated_at=?4 WHERE platform=?1 AND account=?2",
            params![platform.as_str(), account, i64::from(enabled), now],
        )?;
        if changed == 0 {
            anyhow::bail!("channel not found: {platform}/{account}");
        }
        self.refresh_config_cache_from_conn(&conn)
    }

    pub fn start_live(&self, item: &LiveHistoryItem) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO live_recordings(id,platform,account,channel_name,bno,title,file_path,started_at,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'RECORDING')",
            params![item.id, item.platform.as_str(), item.account, item.channel_name, item.bno, item.title, item.file_path, item.started_at],
        )?;
        Ok(())
    }

    pub fn finish_live(
        &self,
        id: &str,
        ended_at: &str,
        duration_seconds: i64,
        size_bytes: u64,
        reason: &str,
        status: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE live_recordings SET ended_at=?2,duration_seconds=?3,size_bytes=?4,reason=?5,status=?6 WHERE id=?1",
            params![id, ended_at, duration_seconds, size_bytes as i64, reason, status],
        )?;
        Ok(())
    }

    pub fn update_live_file(&self, id: &str, file_path: &str, size_bytes: u64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE live_recordings SET file_path=?2,size_bytes=?3 WHERE id=?1",
            params![id, file_path, size_bytes as i64],
        )?;
        Ok(())
    }

    pub fn upsert_vod(&self, status: &VodJobStatus) -> Result<()> {
        let Some(id) = status.job_id.as_deref() else {
            return Ok(());
        };
        let analysis = status.analysis.as_ref();
        let vod_url = analysis.map(|a| a.vod_url.as_str()).unwrap_or("");
        let title = analysis.map(|a| a.title.as_str()).unwrap_or("");
        let streamer = analysis.map(|a| a.streamer.as_str()).unwrap_or("");
        let kind = match status.state.as_str() {
            "READY" | "ANALYZING" => "ANALYZE",
            "COMPLETED" | "DOWNLOADING" | "REFRESHING" | "MERGING" => "DOWNLOAD",
            _ if status.output_file.is_some() => "DOWNLOAD",
            _ => "JOB",
        };
        let conn = self.conn()?;
        conn.execute(
            r#"INSERT INTO vod_jobs(id,platform,kind,vod_url,title,streamer,part_count,state,output_file,message,started_at,finished_at,updated_at)
               VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
               ON CONFLICT(id) DO UPDATE SET
                 platform=excluded.platform,
                 kind=CASE WHEN excluded.kind='JOB' THEN vod_jobs.kind ELSE excluded.kind END,
                 vod_url=CASE WHEN excluded.vod_url='' THEN vod_jobs.vod_url ELSE excluded.vod_url END,
                 title=CASE WHEN excluded.title='' THEN vod_jobs.title ELSE excluded.title END,
                 streamer=CASE WHEN excluded.streamer='' THEN vod_jobs.streamer ELSE excluded.streamer END,
                 part_count=MAX(vod_jobs.part_count, excluded.part_count),
                 state=excluded.state,
                 output_file=COALESCE(excluded.output_file, vod_jobs.output_file),
                 message=excluded.message,
                 started_at=COALESCE(vod_jobs.started_at, excluded.started_at),
                 finished_at=COALESCE(excluded.finished_at, vod_jobs.finished_at),
                 updated_at=excluded.updated_at
               WHERE
                 excluded.platform IS NOT vod_jobs.platform
                 OR (excluded.kind<>'JOB' AND excluded.kind IS NOT vod_jobs.kind)
                 OR (excluded.vod_url<>'' AND excluded.vod_url IS NOT vod_jobs.vod_url)
                 OR (excluded.title<>'' AND excluded.title IS NOT vod_jobs.title)
                 OR (excluded.streamer<>'' AND excluded.streamer IS NOT vod_jobs.streamer)
                 OR excluded.part_count>vod_jobs.part_count
                 OR excluded.state IS NOT vod_jobs.state
                 OR (excluded.output_file IS NOT NULL AND excluded.output_file IS NOT vod_jobs.output_file)
                 OR excluded.message IS NOT vod_jobs.message
                 OR (vod_jobs.started_at IS NULL AND excluded.started_at IS NOT NULL)
                 OR (excluded.finished_at IS NOT NULL AND excluded.finished_at IS NOT vod_jobs.finished_at)"#,
            params![
                id,
                status.platform.as_str(),
                kind,
                vod_url,
                title,
                streamer,
                status.part_count as i64,
                status.state,
                status.output_file,
                status.message,
                status.started_at,
                status.finished_at,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn setting_value(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .settings_cache
            .read()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?
            .get(key)
            .cloned())
    }
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn ensure_multiplatform_schema(conn: &mut Connection) -> Result<()> {
    migrate_channels_to_composite_identity(conn)?;
    add_platform_column_if_missing(conn, "live_recordings")?;
    add_platform_column_if_missing(conn, "vod_jobs")?;
    add_platform_column_if_missing(conn, "vod_queue")?;
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<(String, i64)>> {
    let sql = match table {
        "channels" => "PRAGMA table_info(channels)",
        "live_recordings" => "PRAGMA table_info(live_recordings)",
        "vod_jobs" => "PRAGMA table_info(vod_jobs)",
        "vod_queue" => "PRAGMA table_info(vod_queue)",
        _ => anyhow::bail!("unsupported schema table: {table}"),
    };
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
    })?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(Into::into)
}

fn add_platform_column_if_missing(conn: &Connection, table: &str) -> Result<()> {
    if table_columns(conn, table)?
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("platform"))
    {
        return Ok(());
    }
    let sql = match table {
        "live_recordings" => {
            "ALTER TABLE live_recordings ADD COLUMN platform TEXT NOT NULL DEFAULT 'SOOP'"
        }
        "vod_jobs" => "ALTER TABLE vod_jobs ADD COLUMN platform TEXT NOT NULL DEFAULT 'SOOP'",
        "vod_queue" => "ALTER TABLE vod_queue ADD COLUMN platform TEXT NOT NULL DEFAULT 'SOOP'",
        _ => anyhow::bail!("unsupported platform column table: {table}"),
    };
    conn.execute_batch(sql)?;
    Ok(())
}

fn migrate_channels_to_composite_identity(conn: &mut Connection) -> Result<()> {
    let columns = table_columns(conn, "channels")?;
    let has_platform = columns
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("platform"));
    let platform_pk = columns
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("platform"))
        .map(|(_, pk)| *pk)
        .unwrap_or(0);
    let account_pk = columns
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("account"))
        .map(|(_, pk)| *pk)
        .unwrap_or(0);
    if has_platform && platform_pk == 1 && account_pk == 2 {
        return Ok(());
    }

    let platform_expr = if has_platform {
        "COALESCE(NULLIF(platform,''),'SOOP')"
    } else {
        "'SOOP'"
    };
    let tx = conn.transaction()?;
    tx.execute_batch(
        r#"DROP TABLE IF EXISTS channels_v16;
        CREATE TABLE channels_v16 (
            platform TEXT NOT NULL COLLATE NOCASE DEFAULT 'SOOP',
            account TEXT NOT NULL COLLATE NOCASE,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            outdir TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY(platform, account)
        );"#,
    )?;
    tx.execute(
        &format!(
            "INSERT OR REPLACE INTO channels_v16(platform,account,name,enabled,outdir,updated_at) SELECT {platform_expr},account,name,enabled,outdir,updated_at FROM channels"
        ),
        [],
    )?;
    tx.execute_batch("DROP TABLE channels; ALTER TABLE channels_v16 RENAME TO channels;")?;
    tx.commit()?;
    Ok(())
}

fn load_all_settings_from_conn(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key,value FROM settings ORDER BY key")?;
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?
    .collect::<rusqlite::Result<BTreeMap<_, _>>>()
    .map_err(Into::into)
}

fn load_channels_from_conn(conn: &Connection) -> Result<Vec<Channel>> {
    let mut stmt = conn.prepare(
        "SELECT platform,name,account,enabled,outdir FROM channels ORDER BY platform COLLATE NOCASE, name COLLATE NOCASE, account COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(platform, name, account, enabled, outdir)| {
            Ok(Channel {
                platform: platform.parse::<PlatformId>()?,
                name,
                account,
                enabled: enabled != 0,
                outdir,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod store_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn channel(name: &str, account: &str) -> Channel {
        Channel {
            platform: PlatformId::Soop,
            enabled: true,
            name: name.into(),
            account: account.into(),
            outdir: String::new(),
        }
    }

    #[test]
    fn runtime_config_cache_tracks_committed_writes() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("OUTPUT_DIR".into(), "C:\\cached-live".into())]),
                "test",
            )
            .unwrap();
        let mut cached = channel("Cached", "cached-account");
        cached.outdir = "C:\\cached-live".into();
        store.sync_channels(&[cached]).unwrap();

        assert_eq!(
            store.setting_value("OUTPUT_DIR").unwrap().as_deref(),
            Some("C:\\cached-live")
        );
        let channels = store.channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, PlatformId::Soop);
        assert_eq!(channels[0].account, "cached-account");
    }

    #[test]
    fn atomic_channel_inserts_preserve_changes_from_stale_observers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channels-atomic.db");
        let first = Store::open_observer(path.clone()).unwrap();
        let second = Store::open_observer(path.clone()).unwrap();

        let alpha = channel("Alpha", "alpha");
        let beta = channel("Beta", "beta");

        first.insert_channel(&alpha).unwrap();
        second.insert_channel(&beta).unwrap();

        let reopened = Store::open_observer(path).unwrap();
        let accounts = reopened
            .channels()
            .unwrap()
            .into_iter()
            .map(|channel| channel.account)
            .collect::<Vec<_>>();
        assert_eq!(accounts, vec!["alpha", "beta"]);
    }
}
