use crate::{
    backend::{
        HIDDEN_SETTING_KEYS, SAFE_SETTING_KEYS, channels_path, read_channels, read_safe_settings,
        settings_path,
    },
    model::{Channel, LiveHistoryItem, VodJobStatus},
    primary_config::VOD_TOOL_KEYS,
    vod_tool_settings,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, OptionalExtension, backup::Backup, params};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock},
};

static GLOBAL_STORE: OnceLock<Store> = OnceLock::new();

#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<Connection>>,
    path: PathBuf,
    settings_cache: Arc<RwLock<BTreeMap<String, String>>>,
    channels_cache: Arc<RwLock<Vec<Channel>>>,
}

#[derive(Debug, Clone)]
pub struct MigrationSummary {
    pub settings: usize,
    pub channels: usize,
    pub imported: bool,
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
        if let Ok(value) = env::var("SOOP_DATA_DIR") {
            if !value.trim().is_empty() {
                return PathBuf::from(value).join("soop.db");
            }
        }
        backend_dir
            .parent()
            .unwrap_or(backend_dir)
            .join("data")
            .join("soop.db")
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create data directory {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS channels (
                account TEXT PRIMARY KEY COLLATE NOCASE,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                outdir TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS live_recordings (
                id TEXT PRIMARY KEY,
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
            "#,
        )?;
        let settings_cache = load_all_settings_from_conn(&conn)?;
        let channels_cache = load_channels_from_conn(&conn)?;

        let store = Self {
            inner: Arc::new(Mutex::new(conn)),
            path,
            settings_cache: Arc::new(RwLock::new(settings_cache)),
            channels_cache: Arc::new(RwLock::new(channels_cache)),
        };
        store.recover_interrupted()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
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
        target.execute_batch("PRAGMA foreign_keys=ON; PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.refresh_config_cache_from_conn(&target)?;
        Ok(())
    }

    pub fn ensure_schema(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS channels (
                account TEXT PRIMARY KEY COLLATE NOCASE,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                outdir TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS live_recordings (
                id TEXT PRIMARY KEY,
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
            "#,
        )?;
        self.refresh_config_cache_from_conn(&conn)?;
        drop(conn);
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

    pub fn bootstrap_primary_once(&self, backend_dir: &Path) -> Result<MigrationSummary> {
        if self.meta_value("sqlite_primary_bootstrap")?.as_deref() == Some("1") {
            return Ok(MigrationSummary {
                settings: self.settings_count()?,
                channels: self.channels()?.len(),
                imported: false,
            });
        }

        let mut live_settings = read_safe_settings(&settings_path(backend_dir))?;
        for (key, value) in read_hidden_settings(&settings_path(backend_dir))? {
            live_settings.insert(key, value);
        }
        let channels = read_channels(&channels_path(backend_dir))?;
        let vod_settings = vod_tool_settings::read(backend_dir)?;

        self.sync_settings(&live_settings, "legacy-import-live")?;
        self.sync_settings(&vod_settings, "legacy-import-vod")?;
        self.sync_channels(&channels)?;
        self.set_meta("sqlite_primary_bootstrap", "1")?;
        self.set_meta("sqlite_primary_bootstrap_at", &Utc::now().to_rfc3339())?;

        Ok(MigrationSummary {
            settings: live_settings.len() + vod_settings.len(),
            channels: channels.len(),
            imported: true,
        })
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
            .filter_map(|key| cache.get(*key).map(|value| ((*key).to_string(), value.clone())))
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
                "INSERT INTO channels(account,name,enabled,outdir,updated_at) VALUES(?1,?2,?3,?4,?5)",
                params![channel.account, channel.name, i64::from(channel.enabled), channel.outdir, now],
            )?;
        }
        tx.commit()?;
        *self
            .channels_cache
            .write()
            .map_err(|_| anyhow::anyhow!("channels cache lock poisoned"))? = channels.to_vec();
        Ok(())
    }

    fn meta_value(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn settings_count(&self) -> Result<usize> {
        Ok(self
            .settings_cache
            .read()
            .map_err(|_| anyhow::anyhow!("settings cache lock poisoned"))?
            .len())
    }

    pub fn start_live(&self, item: &LiveHistoryItem) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO live_recordings(id,account,channel_name,bno,title,file_path,started_at,status) VALUES(?1,?2,?3,?4,?5,?6,?7,'RECORDING')",
            params![item.id, item.account, item.channel_name, item.bno, item.title, item.file_path, item.started_at],
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
            r#"INSERT INTO vod_jobs(id,kind,vod_url,title,streamer,part_count,state,output_file,message,started_at,finished_at,updated_at)
               VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
               ON CONFLICT(id) DO UPDATE SET
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
                 (excluded.kind<>'JOB' AND excluded.kind IS NOT vod_jobs.kind)
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
        "SELECT name,account,enabled,outdir FROM channels ORDER BY name COLLATE NOCASE, account COLLATE NOCASE",
    )?;
    stmt.query_map([], |row| {
        Ok(Channel {
            name: row.get(0)?,
            account: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            outdir: row.get(3)?,
        })
    })?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(Into::into)
}

fn read_hidden_settings(path: &Path) -> Result<BTreeMap<String, String>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let hidden: HashSet<&str> = HIDDEN_SETTING_KEYS.iter().copied().collect();
    let mut values = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if hidden.contains(key) {
            values.insert(key.to_string(), value.trim().to_string());
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn completed_vod_status(message: &str) -> VodJobStatus {
        VodJobStatus {
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

    #[test]
    fn runtime_config_cache_tracks_committed_writes() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("soop.db")).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("OUTPUT_DIR".into(), "C:\\cached-live".into())]),
                "test",
            )
            .unwrap();
        store
            .sync_channels(&[Channel {
                enabled: true,
                name: "Cached".into(),
                account: "cached-account".into(),
                outdir: "C:\\cached-live".into(),
            }])
            .unwrap();

        assert_eq!(
            store.setting_value("OUTPUT_DIR").unwrap().as_deref(),
            Some("C:\\cached-live")
        );
        let channels = store.channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].account, "cached-account");
    }

    #[test]
    fn unchanged_vod_history_upsert_does_not_touch_updated_at() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("soop.db")).unwrap();
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
