use crate::{
    backend::{HIDDEN_SETTING_KEYS, SAFE_SETTING_KEYS},
    model::{Channel, LiveHistoryItem, VodJobStatus},
    primary_config::VOD_TOOL_KEYS,
    support::platform::PlatformId,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, backup::Backup, params};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock},
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
        if let Ok(value) = env::var("STREAM_ARCHIVE_DATA_DIR") {
            if !value.trim().is_empty() {
                return PathBuf::from(value).join(DATABASE_FILE);
            }
        }
        backend_dir
            .parent()
            .unwrap_or(backend_dir)
            .join("data")
            .join(DATABASE_FILE)
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

        let store = Self {
            inner: Arc::new(Mutex::new(conn)),
            path,
            settings_cache: Arc::new(RwLock::new(settings_cache)),
            channels_cache: Arc::new(RwLock::new(channels_cache)),
        };
        store.ensure_runtime_defaults()?;
        store.recover_interrupted()?;
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
            ("MIN_FREE_SPACE_GB", "20"),
            ("OUTPUT_DIR", ""),
            ("QUALITY", "best"),
            ("FILE_NAME_PATTERN", "LEGACY"),
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
        let backup = Backup::new(&*source, &mut target)?;
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
        let mut target = self.conn()?;
        let backup = Backup::new(&source, &mut *target)?;
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

    fn refresh_config_cache_from_conn(&self, conn: &Connection) -> Result<()> {
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
        Ok(())
    }

    fn recover_interrupted(&self) -> Result<()> {
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
                EXISTS(SELECT 1 FROM settings WHERE source <> 'runtime-default')
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
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn completed_vod_status(message: &str) -> VodJobStatus {
        VodJobStatus {
            platform: PlatformId::Soop,
            state: "COMPLETED".into(),
            running: false,
            job_id: Some("vod-history-test".into()),
            message: message.into(),
            current_part: 1,
            part_count: 1,
            percent: 100.0,
            output_file: Some("C:\\SOOP_VOD\\done.mp4".into()),
            started_at: Some("2026-09-10T00:00:00Z".into()),
            finished_at: Some("2026-09-10T00:01:00Z".into()),
            analysis: None,
        }
    }

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
    fn fresh_store_is_first_run_until_user_configuration_is_written() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
        assert!(store.is_first_run_unconfigured().unwrap());

        store
            .sync_settings(
                &BTreeMap::from([("CHECK_INTERVAL".into(), "30".into())]),
                "native-environment",
            )
            .unwrap();
        assert!(!store.is_first_run_unconfigured().unwrap());
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
    fn channel_cache_preserves_database_sort_order_after_write() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(DATABASE_FILE);
        let store = Store::open(db_path.clone()).unwrap();
        store
            .sync_channels(&[
                channel("Zulu", "z"),
                channel("alpha", "b"),
                channel("ALPHA", "a"),
            ])
            .unwrap();

        let immediate = store
            .channels()
            .unwrap()
            .into_iter()
            .map(|channel| channel.account)
            .collect::<Vec<_>>();
        assert_eq!(immediate, vec!["a", "b", "z"]);

        let reopened = Store::open(db_path).unwrap();
        let after_restart = reopened
            .channels()
            .unwrap()
            .into_iter()
            .map(|channel| channel.account)
            .collect::<Vec<_>>();
        assert_eq!(after_restart, immediate);
    }

    #[test]
    fn legacy_database_filename_is_migrated_once() {
        let dir = tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_DATABASE_FILE);
        let legacy_store = Store::open(legacy.clone()).unwrap();
        legacy_store
            .sync_settings(
                &BTreeMap::from([("OUTPUT_DIR".into(), "C:\\legacy-live".into())]),
                "test",
            )
            .unwrap();
        drop(legacy_store);

        let current = dir.path().join(DATABASE_FILE);
        assert!(Store::migrate_legacy_database(&current).unwrap());
        let migrated = Store::open(current.clone()).unwrap();
        assert_eq!(
            migrated.setting_value("OUTPUT_DIR").unwrap().as_deref(),
            Some("C:\\legacy-live")
        );
        assert!(current.is_file());
        assert!(!legacy.is_file());
        assert!(!Store::migrate_legacy_database(&current).unwrap());
    }

    #[test]
    fn legacy_schema_is_promoted_to_soop_platform_identity() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("legacy.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"CREATE TABLE channels (
                    account TEXT PRIMARY KEY COLLATE NOCASE,
                    name TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    outdir TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                INSERT INTO channels(account,name,enabled,outdir,updated_at)
                VALUES('legacy','Legacy',1,'','2026-09-10T00:00:00Z');"#,
            )
            .unwrap();
        }
        let store = Store::open(db_path).unwrap();
        let channels = store.channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].platform, PlatformId::Soop);
        assert_eq!(channels[0].account, "legacy");

        let conn = store.conn().unwrap();
        let columns = table_columns(&conn, "channels").unwrap();
        assert_eq!(
            columns
                .iter()
                .find(|(name, _)| name == "platform")
                .map(|(_, pk)| *pk),
            Some(1)
        );
        assert_eq!(
            columns
                .iter()
                .find(|(name, _)| name == "account")
                .map(|(_, pk)| *pk),
            Some(2)
        );
    }

    #[test]
    fn unchanged_vod_history_upsert_does_not_touch_updated_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
        let status = completed_vod_status("완료");
        store.upsert_vod(&status).unwrap();
        {
            let conn = store.conn().unwrap();
            conn.execute(
                "UPDATE vod_jobs SET updated_at='sentinel' WHERE id='vod-history-test'",
                [],
            )
            .unwrap();
        }

        store.upsert_vod(&status).unwrap();
        let unchanged: String = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT updated_at FROM vod_jobs WHERE id='vod-history-test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, "sentinel");

        let mut changed = status.clone();
        changed.message = "완료됨".into();
        store.upsert_vod(&changed).unwrap();
        let changed_at: String = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT updated_at FROM vod_jobs WHERE id='vod-history-test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(changed_at, "sentinel");
    }
}
