from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# Cargo: SQLite online backup API + SHA-256.
p='rust-web/Cargo.toml'; s=read(p)
s=rep(s,'rusqlite = { version = "0.37", features = ["bundled"] }','rusqlite = { version = "0.37", features = ["bundled", "backup"] }','rusqlite backup feature')
s=rep(s,'serde_json = "1"','serde_json = "1"\nsha2 = "0.10"','sha2')
write(p,s)

# Store online backup/restore support.
p='rust-web/src/store.rs'; s=read(p)
s=rep(s,'use rusqlite::{Connection, OptionalExtension, params};','use rusqlite::{Connection, OpenFlags, OptionalExtension, backup::Backup, params};','store backup imports')
needle='''    pub fn path(&self) -> &Path {
        &self.path
    }
'''
insert=needle+'''\n    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        if destination.exists() {
            anyhow::bail!("backup destination already exists: {}", destination.display());
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
        Ok(())
    }
'''
s=rep(s,needle,insert,'store backup methods')
write(p,s)

# Auth: restoring a database must never resurrect old authenticated browser sessions.
p='rust-web/src/auth.rs'; s=read(p)
needle='''    pub(crate) fn authorize_session_readonly(&self, headers: &HeaderMap) -> ApiResult<()> {
        self.session_from_headers(headers)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "login required".to_string()))?;
        Ok(())
    }
'''
insert=needle+'''\n    pub(crate) fn invalidate_all_sessions(&self) -> Result<usize> {
        let conn = self.conn()?;
        Ok(conn.execute("DELETE FROM auth_sessions", [])?)
    }
'''
s=rep(s,needle,insert,'auth invalidate sessions')
write(p,s)

# Safe backup policy settings.
p='rust-web/src/backend.rs'; s=read(p)
s=rep(s,'    "LOG_RETENTION_DAYS",\n];','    "LOG_RETENTION_DAYS",\n    "BACKUP_ENABLED",\n    "BACKUP_INTERVAL_HOURS",\n    "BACKUP_KEEP_COUNT",\n    "BACKUP_RETENTION_DAYS",\n];','safe backup settings')
write(p,s)

p='rust-web/src/primary_config.rs'; s=read(p)
s=rep(s,'            "LOG_RETENTION_DAYS" => validate_int(value, 0, 36_500, key)?,','            "LOG_RETENTION_DAYS" => validate_int(value, 0, 36_500, key)?,\n            "BACKUP_INTERVAL_HOURS" => validate_int(value, 1, 8_760, key)?,\n            "BACKUP_KEEP_COUNT" => validate_int(value, 0, 1_000, key)?,\n            "BACKUP_RETENTION_DAYS" => validate_int(value, 0, 36_500, key)?,','backup numeric validation')
s=rep(s,'            | "LOG_ENABLED" => validate_yes_no(value, key)?,','            | "LOG_ENABLED"\n            | "BACKUP_ENABLED" => validate_yes_no(value, key)?,','backup yesno validation')
write(p,s)

# Phase 12 backup manager and APIs.
write('rust-web/src/backup.rs', r'''use crate::{ApiResult, AppState, authorize, internal_error, materialize_primary_files, store::Store};
use anyhow::{Context, Result, bail};
use axum::{Json, extract::{Path as AxumPath, State}, http::{HeaderMap, StatusCode}};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::{Path, PathBuf}, sync::Arc, time::{Duration, SystemTime}};
use tokio::sync::Mutex;

const DEFAULT_INTERVAL_HOURS: u64 = 24;
const DEFAULT_KEEP_COUNT: usize = 10;
const DEFAULT_RETENTION_DAYS: i64 = 30;
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub file_name: String,
    pub created_at: String,
    pub kind: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub integrity: String,
}

#[derive(Clone)]
pub struct BackupManager {
    store: Store,
    backup_dir: Arc<PathBuf>,
    operation: Arc<Mutex<()>>,
}

impl BackupManager {
    pub fn open(store: Store, backend_dir: &Path) -> Result<Self> {
        let backup_dir = resolve_backup_dir(backend_dir)?;
        fs::create_dir_all(&backup_dir)
            .with_context(|| format!("failed to create backup directory {}", backup_dir.display()))?;
        let manager = Self {
            store,
            backup_dir: Arc::new(backup_dir),
            operation: Arc::new(Mutex::new(())),
        };
        manager.ensure_policy_defaults()?;
        manager.migrate_legacy_backups()?;
        Ok(manager)
    }

    pub fn backup_dir(&self) -> &Path { self.backup_dir.as_path() }

    fn ensure_policy_defaults(&self) -> Result<()> {
        let mut defaults = std::collections::BTreeMap::new();
        for (key, value) in [
            ("BACKUP_ENABLED", "Y"),
            ("BACKUP_INTERVAL_HOURS", "24"),
            ("BACKUP_KEEP_COUNT", "10"),
            ("BACKUP_RETENTION_DAYS", "30"),
        ] {
            if self.store.setting_value(key)?.is_none() {
                defaults.insert(key.to_string(), value.to_string());
            }
        }
        if !defaults.is_empty() {
            self.store.sync_settings(&defaults, "phase12-default")?;
        }
        Ok(())
    }

    pub fn policy(&self) -> Result<BackupPolicy> {
        let enabled = self.store.setting_value("BACKUP_ENABLED")?
            .unwrap_or_else(|| "Y".into()).eq_ignore_ascii_case("Y");
        let interval_hours = parse_u64(self.store.setting_value("BACKUP_INTERVAL_HOURS")?, DEFAULT_INTERVAL_HOURS).max(1);
        let keep_count = parse_u64(self.store.setting_value("BACKUP_KEEP_COUNT")?, DEFAULT_KEEP_COUNT as u64) as usize;
        let retention_days = parse_i64(self.store.setting_value("BACKUP_RETENTION_DAYS")?, DEFAULT_RETENTION_DAYS).max(0);
        Ok(BackupPolicy { enabled, interval_hours, keep_count, retention_days })
    }

    fn migrate_legacy_backups(&self) -> Result<usize> {
        let Some(data_dir) = self.store.path().parent() else { return Ok(0) };
        let legacy = data_dir.join("backups");
        if !legacy.is_dir() || legacy == self.backup_dir.as_path() { return Ok(0) }
        let mut moved = 0;
        for entry in fs::read_dir(&legacy)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() { continue }
            let Some(name) = path.file_name() else { continue };
            let name_text = name.to_string_lossy();
            if !(name_text.ends_with(".db") || name_text.ends_with(".db.json")) { continue }
            let target = self.backup_dir.join(name);
            if target.exists() { continue }
            match fs::rename(&path, &target) {
                Ok(()) => moved += 1,
                Err(_) => { fs::copy(&path, &target)?; fs::remove_file(&path)?; moved += 1; }
            }
        }
        if legacy.read_dir().map(|mut it| it.next().is_none()).unwrap_or(false) {
            let _ = fs::remove_dir(&legacy);
        }
        Ok(moved)
    }

    pub async fn list(&self) -> Result<Vec<BackupInfo>> {
        let _guard = self.operation.lock().await;
        self.list_locked()
    }

    pub async fn create_manual(&self) -> Result<BackupInfo> {
        let _guard = self.operation.lock().await;
        self.create_locked("manual")
    }

    async fn create_auto_if_due(&self) -> Result<Option<BackupInfo>> {
        let _guard = self.operation.lock().await;
        let policy = self.policy()?;
        if !policy.enabled { return Ok(None) }
        let backups = self.list_locked()?;
        let newest_auto = backups.iter().filter(|item| item.kind == "auto")
            .filter_map(|item| DateTime::parse_from_rfc3339(&item.created_at).ok())
            .map(|dt| dt.with_timezone(&Utc)).max();
        let due = newest_auto.map(|last| Utc::now() - last >= ChronoDuration::hours(policy.interval_hours as i64)).unwrap_or(true);
        if due { Ok(Some(self.create_locked("auto")?)) } else { self.cleanup_locked(&policy)?; Ok(None) }
    }

    pub async fn restore(&self, file_name: &str) -> Result<(BackupInfo, BackupInfo)> {
        let _guard = self.operation.lock().await;
        let source = self.resolve_file(file_name)?;
        let selected = inspect_backup(&source)?;
        if selected.integrity != "OK" {
            bail!("backup integrity check failed: {}", selected.integrity);
        }
        let safety = self.create_locked("pre_restore")?;
        self.store.restore_from(&source)?;
        Ok((selected, safety))
    }

    fn create_locked(&self, kind: &str) -> Result<BackupInfo> {
        fs::create_dir_all(self.backup_dir.as_path())?;
        let stamp = Utc::now().format("%Y%m%d_%H%M%S_%3f");
        let file_name = format!("soop_{kind}_{stamp}.db");
        let path = self.backup_dir.join(&file_name);
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
        fs::write(path.with_extension("db.json"), serde_json::to_vec_pretty(&metadata)?)?;
        let policy = self.policy()?;
        self.cleanup_locked(&policy)?;
        Ok(BackupInfo { file_name, created_at: metadata.created_at, kind: metadata.kind, size_bytes, sha256, integrity: "OK".into() })
    }

    fn list_locked(&self) -> Result<Vec<BackupInfo>> {
        fs::create_dir_all(self.backup_dir.as_path())?;
        let mut items = Vec::new();
        for entry in fs::read_dir(self.backup_dir.as_path())? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("db") { continue }
            if let Ok(info) = inspect_backup(&path) { items.push(info) }
        }
        items.sort_by(|a,b| b.created_at.cmp(&a.created_at).then_with(|| b.file_name.cmp(&a.file_name)));
        Ok(items)
    }

    fn cleanup_locked(&self, policy: &BackupPolicy) -> Result<usize> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.backup_dir.as_path())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("db"))
            .collect();
        files.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)));
        let now = SystemTime::now();
        let mut removed = 0;
        for (index, path) in files.into_iter().enumerate() {
            let too_many = policy.keep_count > 0 && index >= policy.keep_count;
            let too_old = if policy.retention_days > 0 {
                fs::metadata(&path).and_then(|m| m.modified()).ok()
                    .and_then(|t| now.duration_since(t).ok())
                    .is_some_and(|age| age.as_secs() > policy.retention_days as u64 * 86_400)
            } else { false };
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
        if trimmed.is_empty() || !trimmed.ends_with(".db") || Path::new(trimmed).file_name().and_then(|v| v.to_str()) != Some(trimmed) {
            bail!("invalid backup file name");
        }
        let path = self.backup_dir.join(trimmed);
        if !path.is_file() { bail!("backup not found: {trimmed}") }
        Ok(path)
    }
}

pub fn spawn_auto_backup(manager: BackupManager, logs: crate::backend::LogBuffer) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            match manager.create_auto_if_due().await {
                Ok(Some(item)) => logs.push(format!("[BACKUP] automatic backup created file={} size={} sha256={}", item.file_name, item.size_bytes, item.sha256)).await,
                Ok(None) => {}
                Err(err) => logs.push(format!("[BACKUP:WARN] automatic backup failed: {err:#}")).await,
            }
            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}

pub(crate) async fn api_list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let policy = state.backups.policy().map_err(internal_error)?;
    let backups = state.backups.list().await.map_err(internal_error)?;
    Ok(Json(json!({"directory": state.backups.backup_dir().display().to_string(), "policy": policy, "backups": backups})))
}

pub(crate) async fn api_create(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<BackupInfo>> {
    authorize(&headers, &state)?;
    let item = state.backups.create_manual().await.map_err(internal_error)?;
    state.logs.push(format!("[BACKUP] manual backup created file={} size={} sha256={}", item.file_name, item.size_bytes, item.sha256)).await;
    Ok(Json(item))
}

pub(crate) async fn api_restore(State(state): State<AppState>, headers: HeaderMap, AxumPath(file_name): AxumPath<String>) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let watcher = state.watcher.status().await.map_err(internal_error)?;
    if watcher.running || watcher.recording_count > 0 {
        return Err((StatusCode::CONFLICT, "Watcher를 중지한 뒤 복원하세요.".into()));
    }
    if state.vod.status().await.running {
        return Err((StatusCode::CONFLICT, "VOD 작업을 중지한 뒤 복원하세요.".into()));
    }
    let (selected, safety) = state.backups.restore(&file_name).await.map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    materialize_primary_files(&state.store, &state.backend_dir).map_err(internal_error)?;
    let invalidated = state.auth.invalidate_all_sessions().map_err(internal_error)?;
    state.logs.push(format!("[BACKUP] database restored file={} safety={} sessions_invalidated={invalidated}", selected.file_name, safety.file_name)).await;
    Ok(Json(json!({"ok":true,"restored":selected,"safety_backup":safety,"sessions_invalidated":invalidated,"reauthenticate":true})))
}

fn resolve_backup_dir(backend_dir: &Path) -> Result<PathBuf> {
    if let Ok(value) = env::var("SOOP_BACKUP_DIR") {
        if !value.trim().is_empty() { return Ok(PathBuf::from(value)) }
    }
    let app_root = backend_dir.parent().unwrap_or(backend_dir);
    let parent = app_root.parent().unwrap_or(app_root);
    Ok(parent.join("soop-recorder-backups"))
}

fn inspect_backup(path: &Path) -> Result<BackupInfo> {
    let file_name = path.file_name().and_then(|v| v.to_str()).context("invalid backup file name")?.to_string();
    let metadata_path = path.with_extension("db.json");
    let metadata = fs::read(&metadata_path).ok().and_then(|bytes| serde_json::from_slice::<BackupMetadata>(&bytes).ok());
    let size_bytes = fs::metadata(path)?.len();
    let actual_sha = sha256_file(path)?;
    let db_ok = verify_sqlite(path).is_ok();
    let integrity = match &metadata {
        Some(meta) if !db_ok => "INVALID_SQLITE",
        Some(meta) if meta.sha256.eq_ignore_ascii_case(&actual_sha) && meta.size_bytes == size_bytes => "OK",
        Some(_) => "HASH_MISMATCH",
        None if db_ok => "NO_METADATA",
        None => "INVALID_SQLITE",
    }.to_string();
    let created_at = metadata.as_ref().map(|m| m.created_at.clone()).unwrap_or_else(|| {
        fs::metadata(path).and_then(|m| m.modified()).map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now()).to_rfc3339()
    });
    let kind = metadata.as_ref().map(|m| m.kind.clone()).unwrap_or_else(|| infer_kind(&file_name));
    Ok(BackupInfo { file_name, created_at, kind, size_bytes, sha256: actual_sha, integrity })
}

fn infer_kind(file_name: &str) -> String {
    for kind in ["manual", "auto", "pre_restore"] {
        if file_name.contains(&format!("_{kind}_")) { return kind.into() }
    }
    "legacy".into()
}

fn verify_sqlite(path: &Path) -> Result<()> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if !result.eq_ignore_ascii_case("ok") { bail!("SQLite quick_check: {result}") }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn parse_u64(value: Option<String>, default: u64) -> u64 { value.and_then(|v| v.parse().ok()).unwrap_or(default) }
fn parse_i64(value: Option<String>, default: i64) -> i64 { value.and_then(|v| v.parse().ok()).unwrap_or(default) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn backup_directory_is_sibling_of_application_root() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("soop-recorder");
        let backend = app.join("backend");
        fs::create_dir_all(&backend).unwrap();
        let resolved = resolve_backup_dir(&backend).unwrap();
        assert_eq!(resolved, dir.path().join("soop-recorder-backups"));
    }

    #[test]
    fn online_backup_round_trip_restores_database() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("soop-recorder");
        let backend = app.join("backend");
        fs::create_dir_all(&backend).unwrap();
        let store = Store::open(app.join("data").join("soop.db")).unwrap();
        store.sync_settings(&std::collections::BTreeMap::from([("TEST".into(), "before".into())]), "test").unwrap();
        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let backup = runtime.block_on(manager.create_manual()).unwrap();
        store.sync_settings(&std::collections::BTreeMap::from([("TEST".into(), "after".into())]), "test").unwrap();
        runtime.block_on(manager.restore(&backup.file_name)).unwrap();
        assert_eq!(store.setting_value("TEST").unwrap().as_deref(), Some("before"));
    }
}
''')

# Main wiring, phase marker, routes, auto task.
p='rust-web/src/main.rs'; s=read(p)
s=rep(s,'mod auth;\n','mod auth;\nmod backup;\n','backup module')
s=rep(s,'use auth::AuthManager;','use auth::AuthManager;\nuse backup::BackupManager;','backup import')
s=rep(s,'    auth: Arc<AuthManager>,\n','    auth: Arc<AuthManager>,\n    backups: BackupManager,\n','AppState backup field')
s=rep(s,'    let auth = Arc::new(AuthManager::open(store.path().to_path_buf())?);\n','    let auth = Arc::new(AuthManager::open(store.path().to_path_buf())?);\n    let backups = BackupManager::open(store.clone(), &backend_dir)?;\n','backup init')
s=rep(s,'        auth: auth.clone(),\n','        auth: auth.clone(),\n        backups: backups.clone(),\n','state backups')
s=s.replace('[SERVER] Phase 11 realtime SSE ready;','[SERVER] Phase 12 backup/retention ready;')
s=rep(s,'    spawn_vod_history_sync(store.clone(), vod.clone(), logs.clone());\n','    spawn_vod_history_sync(store.clone(), vod.clone(), logs.clone());\n    backup::spawn_auto_backup(backups.clone(), logs.clone());\n','spawn auto backup')
s=rep(s,'        .route("/phase10.js", get(phase10_js))\n','        .route("/phase10.js", get(phase10_js))\n        .route("/phase12.js", get(phase12_js))\n','phase12 js route')
s=rep(s,'        .route("/api/storage/check", post(phase8::api_storage_check))\n','        .route("/api/storage/check", post(phase8::api_storage_check))\n        .route("/api/backups", get(backup::api_list).post(backup::api_create))\n        .route("/api/backups/{file_name}/restore", post(backup::api_restore))\n','backup api routes')
s=s.replace('SOOP Rust Web - Phase 11','SOOP Rust Web - Phase 12')
s=rep(s,'    println!("Realtime: SSE snapshot stream + automatic REST polling fallback");\n','    println!("Realtime: SSE snapshot stream + automatic REST polling fallback");\n    println!("Backup  : SQLite online backup + retention + guarded restore");\n    println!("BackupDir: {}", backups.backup_dir().display());\n','backup console info')
s=s.replace('phase11-realtime-sse','phase12-backup-retention')
needle='''async fn phase10_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase10.js"),
    )
}
'''
insert=needle+'''async fn phase12_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase12.js"),
    )
}
'''
s=rep(s,needle,insert,'phase12 handler')
write(p,s)

# Realtime phase marker.
p='rust-web/src/realtime.rs'; s=read(p).replace('phase11-realtime-sse','phase12-backup-retention'); write(p,s)

# Base app knows backup settings too.
p='rust-web/web/app.js'; s=read(p)
for target in [
"'LOG_RETENTION_DAYS']",
"'LOG_RETENTION_DAYS']);"
]:
    if target in s:
        s=s.replace(target, "'LOG_RETENTION_DAYS','BACKUP_ENABLED','BACKUP_INTERVAL_HOURS','BACKUP_KEEP_COUNT','BACKUP_RETENTION_DAYS']" if target.endswith("]") else target, 1)
# exact replacements are safer for the first three declarations
s=s.replace("'LOG_DIR','LOG_RETENTION_DAYS'];", "'LOG_DIR','LOG_RETENTION_DAYS','BACKUP_ENABLED','BACKUP_INTERVAL_HOURS','BACKUP_KEEP_COUNT','BACKUP_RETENTION_DAYS'];", 1)
s=s.replace("'LOG_DIR','LOG_RETENTION_DAYS']);", "'LOG_DIR','LOG_RETENTION_DAYS','BACKUP_ENABLED','BACKUP_INTERVAL_HOURS','BACKUP_KEEP_COUNT','BACKUP_RETENTION_DAYS']);", 2)
write(p,s)

# Phase 10.2 setting UX: add Backup tab/settings and panel visibility hook.
p='rust-web/web/phase8.js'; s=read(p)
s=s.replace("'LOG_DIR','LOG_RETENTION_DAYS'];", "'LOG_DIR','LOG_RETENTION_DAYS','BACKUP_ENABLED','BACKUP_INTERVAL_HOURS','BACKUP_KEEP_COUNT','BACKUP_RETENTION_DAYS'];", 1)
s=s.replace("'LOG_DIR','LOG_RETENTION_DAYS']);", "'LOG_DIR','LOG_RETENTION_DAYS','BACKUP_ENABLED','BACKUP_INTERVAL_HOURS','BACKUP_KEEP_COUNT','BACKUP_RETENTION_DAYS']);", 1)
s=rep(s,"LOG_ENABLED:'logs',LOG_DIR:'logs',LOG_RETENTION_DAYS:'logs'};","LOG_ENABLED:'logs',LOG_DIR:'logs',LOG_RETENTION_DAYS:'logs',BACKUP_ENABLED:'backup',BACKUP_INTERVAL_HOURS:'backup',BACKUP_KEEP_COUNT:'backup',BACKUP_RETENTION_DAYS:'backup'};",'backup groups')
s=rep(s,'LOG_RETENTION_DAYS:[0,36500,1]};','LOG_RETENTION_DAYS:[0,36500,1],BACKUP_INTERVAL_HOURS:[1,8760,1],BACKUP_KEEP_COUNT:[0,1000,1],BACKUP_RETENTION_DAYS:[0,36500,1]};','backup number meta')
s=rep(s,"'GUI_NOTIFY_WARNING','LOG_ENABLED']);","'GUI_NOTIFY_WARNING','LOG_ENABLED','BACKUP_ENABLED']);",'backup yesno')
s=rep(s,"LOG_RETENTION_DAYS:{label:'로그 보관 기간',desc:'오래된 로그 파일을 보관할 기간입니다. 0은 정리하지 않는 설정으로 사용할 수 있습니다.',unit:'일',recommended:'30'},","LOG_RETENTION_DAYS:{label:'로그 보관 기간',desc:'오래된 로그 파일을 보관할 기간입니다. 0은 정리하지 않는 설정으로 사용할 수 있습니다.',unit:'일',recommended:'30'},\nBACKUP_ENABLED:{label:'자동 백업',desc:'SQLite 데이터베이스를 주기적으로 온라인 백업합니다. 녹화 중에도 일관된 백업을 만들 수 있습니다.',recommended:'켜기'},\nBACKUP_INTERVAL_HOURS:{label:'자동 백업 주기',desc:'마지막 자동 백업 이후 이 시간이 지나면 새 백업을 생성합니다.',unit:'시간',recommended:'24'},\nBACKUP_KEEP_COUNT:{label:'최대 백업 개수',desc:'최신 백업을 이 개수까지 유지합니다. 0은 개수 제한을 사용하지 않습니다.',unit:'개',recommended:'10'},\nBACKUP_RETENTION_DAYS:{label:'백업 보관 기간',desc:'이 기간보다 오래된 백업을 자동 정리합니다. 0은 기간 제한을 사용하지 않습니다.',unit:'일',recommended:'30'},",'backup setting metadata')
s=rep(s,"function p8ApplySettingsVisibility(name){const sec=$('p8SecurityPanel'),advanced=$('p102AdvancedPanel');if(sec)sec.hidden=name!=='soop';if(advanced)advanced.hidden=name!=='advanced';let visible=0;","function p8ApplySettingsVisibility(name){const sec=$('p8SecurityPanel'),advanced=$('p102AdvancedPanel'),backup=$('p12BackupPanel');if(sec)sec.hidden=name!=='soop';if(advanced)advanced.hidden=name!=='advanced';if(backup)backup.hidden=name!=='backup';let visible=0;",'backup panel visibility')
s=rep(s,"if(empty)empty.hidden=visible>0||name==='soop'||name==='advanced'}","if(empty)empty.hidden=visible>0||name==='soop'||name==='advanced'||name==='backup'}",'backup empty visibility')
s=rep(s,"const valid=['general','watcher','tools','soop','logs','advanced'];","const valid=['general','watcher','tools','soop','logs','backup','advanced'];",'backup tab valid')
s=rep(s,"if(!p8HasGroupedSettings()){p8EnsureSettingsUX(true).then(()=>p8ApplySettingsVisibility(name)).catch(e=>toast('설정 UI 로드 실패: '+e.message));return}p8ApplySettingsVisibility(name)}","if(!p8HasGroupedSettings()){p8EnsureSettingsUX(true).then(()=>{p8ApplySettingsVisibility(name);if(name==='backup'&&window.p12LoadBackups)window.p12LoadBackups()}).catch(e=>toast('설정 UI 로드 실패: '+e.message));return}p8ApplySettingsVisibility(name);if(name==='backup'&&window.p12LoadBackups)window.p12LoadBackups()}",'backup tab load hook')
write(p,s)

# Backup UI logic.
write('rust-web/web/phase12.js', r'''(()=>{
function fmtDate(v){if(!v)return'-';try{return new Date(v).toLocaleString()}catch{return v}}
function kindText(v){return({manual:'수동',auto:'자동',pre_restore:'복원 전 안전백업',legacy:'기존 백업'})[v]||v||'-'}
function integrityText(v){return({OK:'정상',NO_METADATA:'메타데이터 없음',HASH_MISMATCH:'해시 불일치',INVALID_SQLITE:'DB 손상'})[v]||v||'-'}
function integrityClass(v){return v==='OK'?'ok':(v==='NO_METADATA'?'warn':'bad')}
async function loadBackups(){
  const body=$('p12BackupRows');if(!body)return;
  try{
    const d=await api('/api/backups');
    $('p12BackupDir').textContent=d.directory||'-';
    const p=d.policy||{};$('p12BackupPolicy').textContent=p.enabled?`자동 · ${p.interval_hours}시간 · 최대 ${p.keep_count||'무제한'}개 · ${p.retention_days||'기간 제한 없음'}일`:'자동 백업 꺼짐';
    body.replaceChildren();
    for(const b of d.backups||[]){
      const tr=document.createElement('tr');
      const canRestore=b.integrity==='OK';
      tr.innerHTML=`<td>${fmtDate(b.created_at)}</td><td>${kindText(b.kind)}</td><td class="mono smallcell">${esc(b.file_name)}</td><td>${bytes(b.size_bytes)}</td><td class="mono smallcell">${esc((b.sha256||'').slice(0,16))}${b.sha256?'…':''}</td><td class="${integrityClass(b.integrity)}"><b>${integrityText(b.integrity)}</b></td><td><button class="mini restore" ${canRestore?'':'disabled'}>복원</button></td>`;
      const btn=tr.querySelector('.restore');if(btn&&!btn.disabled)btn.onclick=()=>restoreBackup(b.file_name);
      body.appendChild(tr);
    }
    if(!(d.backups||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="7" class="muted">아직 생성된 백업이 없습니다.</td>';body.appendChild(tr)}
  }catch(e){body.innerHTML=`<tr><td colspan="7" class="bad">${esc(e.message)}</td></tr>`}
}
async function createBackup(){
  const btn=$('p12BackupNow');btn.disabled=true;
  try{const b=await api('/api/backups',{method:'POST'});toast(`백업 완료 · ${b.file_name}`);await loadBackups()}catch(e){alert('백업 실패: '+e.message)}finally{btn.disabled=false}
}
async function restoreBackup(file){
  if(!confirm(`${file} 백업으로 SQLite DB를 복원할까요?\n\nWatcher와 VOD가 중지되어 있어야 합니다.\n복원 직전에 현재 DB 안전백업을 자동 생성합니다.\n외부 로그인 세션은 복원 후 모두 종료됩니다.`))return;
  try{
    const d=await api('/api/backups/'+encodeURIComponent(file)+'/restore',{method:'POST'});
    alert(`복원 완료\n안전백업: ${d.safety_backup?.file_name||'-'}\n\n화면을 새로고침합니다.`);
    location.reload();
  }catch(e){alert('복원 실패: '+e.message)}
}
function init(){const refresh=$('p12BackupRefresh'),now=$('p12BackupNow');if(refresh)refresh.onclick=loadBackups;if(now)now.onclick=createBackup}
window.p12LoadBackups=loadBackups;
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init,{once:true});else init();
})();
''')

# HTML Phase 12 and Backup settings panel.
p='rust-web/web/index.html'; s=read(p)
s=s.replace('/style.css?v=p11-sse1','/style.css?v=p12-backup1')
s=s.replace('Rust Web Phase 11 · 실시간 UI','Rust Web Phase 12 · 백업 / 보관')
s=rep(s,'    <button type="button" role="tab" data-settings-tab="logs" aria-selected="false">로그</button>\n    <button type="button" role="tab" data-settings-tab="advanced" aria-selected="false">고급</button>','    <button type="button" role="tab" data-settings-tab="logs" aria-selected="false">로그</button>\n    <button type="button" role="tab" data-settings-tab="backup" aria-selected="false">백업</button>\n    <button type="button" role="tab" data-settings-tab="advanced" aria-selected="false">고급</button>','backup tab html')
needle='''  <div id="p102AdvancedPanel" hidden>
'''
panel='''  <div id="p12BackupPanel" hidden>
    <div class="title inner-title"><h3>데이터 백업 / 복원</h3><div><button id="p12BackupRefresh">새로고침</button> <button id="p12BackupNow">지금 백업</button></div></div>
    <div class="summary"><span>백업 위치 <b id="p12BackupDir">-</b></span><span>정책 <b id="p12BackupPolicy">-</b></span></div>
    <p class="hint">기본 백업 위치는 SOOP Recorder 폴더 바깥의 <code>soop-recorder-backups</code>입니다. 따라서 portable 패키지를 다시 만들거나 <code>soop-recorder</code> 폴더를 교체해도 백업이 같이 삭제되지 않습니다. 필요하면 SOOP_BACKUP_DIR 환경변수로 위치를 바꿀 수 있습니다.</p>
    <div class="table"><table><thead><tr><th>생성</th><th>종류</th><th>파일</th><th>크기</th><th>SHA-256</th><th>무결성</th><th></th></tr></thead><tbody id="p12BackupRows"></tbody></table></div>
    <p class="hint">복원은 Watcher와 VOD가 모두 중지된 상태에서만 가능합니다. 복원 직전에 현재 DB를 자동 안전백업하고, 과거 로그인 세션이 되살아나지 않도록 모든 세션을 무효화합니다.</p>
  </div>
'''+needle
s=rep(s,needle,panel,'backup panel insertion')
s=s.replace('/phase10.js?v=p11-sse1','/phase10.js?v=p12-backup1').replace('/app.js?v=p11-sse1','/app.js?v=p12-backup1').replace('/phase8.js?v=p11-sse1','/phase8.js?v=p12-backup1').replace('/phase9_1.js?v=p11-sse1','/phase9_1.js?v=p12-backup1')
s=s.replace('</body></html>','<script src="/phase12.js?v=p12-backup1"></script>\n</body></html>')
write(p,s)

# Package console guidance: backup destination is now outside the rebuild target.
p='PACKAGE_RUST_WEB.bat'; s=read(p)
s=rep(s,'echo Included: launcher, maintenance scripts, operations docs, Caddy template, release metadata, SHA256 checksums.','echo Included: launcher, maintenance scripts, operations docs, Caddy template, release metadata, SHA256 checksums.\necho Backups: default to a sibling soop-recorder-backups folder outside the replaceable portable package directory.','package backup note')
write(p,s)

# Maintenance scripts use the same safer sibling default and retention-by-age.
p='maintenance/Backup-SoopData.ps1'; s=read(p)
s=s.replace('[int]$Keep = 10\n)', '[int]$Keep = 10,\n    [int]$RetentionDays = 30\n)')
s=rep(s,"    $BackupDir = Join-Path $dataRoot 'backups'","    $appRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))\n    $BackupDir = Join-Path (Split-Path $appRoot -Parent) 'soop-recorder-backups'",'ps backup dir')
s=s.replace('"soop_$stamp.db"','"soop_manual_$stamp.db"')
s=rep(s,'    size_bytes = (Get-Item -LiteralPath $backupPath).Length\n}', '    size_bytes = (Get-Item -LiteralPath $backupPath).Length\n    kind = \'manual\'\n    version = 1\n}', 'ps metadata')
s=rep(s,"        Select-Object -Skip $Keep\n", "        Select-Object -Skip $Keep\n",'keep block marker')
# add age cleanup after keep cleanup
insert='''\nif ($RetentionDays -gt 0) {
    $cutoff = (Get-Date).AddDays(-$RetentionDays)
    $aged = Get-ChildItem -LiteralPath $backupRoot -Filter '*.db' -File | Where-Object { $_.LastWriteTime -lt $cutoff }
    foreach ($item in $aged) {
        Remove-Item -LiteralPath $item.FullName -Force
        $metaPath = "$($item.FullName).json"
        if (Test-Path -LiteralPath $metaPath) { Remove-Item -LiteralPath $metaPath -Force }
    }
}
'''
s=s.replace('\nWrite-Host "Backup complete: $backupPath"',insert+'\nWrite-Host "Backup complete: $backupPath"')
write(p,s)

p='maintenance/Restore-SoopData.ps1'; s=read(p)
s=rep(s,"    $safetyDir = Join-Path $dataRoot 'backups'","    $appRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))\n    $safetyDir = Join-Path (Split-Path $appRoot -Parent) 'soop-recorder-backups'",'ps restore safety dir')
write(p,s)

# Operations docs note.
p='docs/OPERATIONS.md'; s=read(p)
s += '''\n\n## Phase 12 backup / retention\n\n- Web UI: Settings -> Backup.\n- Default backup directory: sibling `soop-recorder-backups` next to the portable application folder, not inside `data`.\n- Override with `SOOP_BACKUP_DIR`.\n- Automatic defaults: enabled, every 24 hours, keep 10, remove backups older than 30 days.\n- SQLite backups use the online backup API and may be created while LIVE recording is active.\n- Restore requires Watcher and VOD to be stopped. A `pre_restore` safety backup is created first.\n- Restore invalidates all browser sessions so an old database cannot resurrect a previously valid session.\n- `BACKUP_DATA.bat` remains available for offline/manual maintenance and uses the same external backup location.\n'''
write(p,s)

# Static sanity.
checks={
'rust-web/src/backup.rs':['soop-recorder-backups','pre_restore','invalidate_all_sessions'],
'rust-web/src/main.rs':['mod backup;','/api/backups','phase12-backup-retention'],
'rust-web/web/index.html':['data-settings-tab="backup"','p12BackupPanel','phase12.js'],
'rust-web/web/phase8.js':['BACKUP_ENABLED','BACKUP_RETENTION_DAYS'],
}
for path,needles in checks.items():
    text=read(path)
    for needle in needles:
        assert needle in text,(path,needle)
print('Phase 12 patch sanity: PASS')
