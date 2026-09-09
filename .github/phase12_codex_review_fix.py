from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    text = p.read_text(encoding='utf-8')
    if old not in text:
        raise SystemExit(f'missing patch target in {path}: {old[:120]!r}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')

# 1) Serialize restore against watcher/VOD starts.
replace_once(
    'rust-web/src/main.rs',
    '    config_write_lock: Arc<Mutex<()>>,\n}',
    '    config_write_lock: Arc<Mutex<()>>,\n    lifecycle_lock: Arc<Mutex<()>>,\n}',
)
replace_once(
    'rust-web/src/main.rs',
    '        config_write_lock: Arc::new(Mutex::new(())),\n    };',
    '        config_write_lock: Arc::new(Mutex::new(())),\n        lifecycle_lock: Arc::new(Mutex::new(())),\n    };',
)
replace_once(
    'rust-web/src/main.rs',
    'async fn api_watcher_start(\n    State(state): State<AppState>,\n    headers: HeaderMap,\n) -> ApiResult<Json<WatcherStatus>> {\n    authorize(&headers, &state)?;\n    Ok(Json(',
    'async fn api_watcher_start(\n    State(state): State<AppState>,\n    headers: HeaderMap,\n) -> ApiResult<Json<WatcherStatus>> {\n    authorize(&headers, &state)?;\n    let _lifecycle_guard = state.lifecycle_lock.lock().await;\n    Ok(Json(',
)
replace_once(
    'rust-web/src/main.rs',
    ') -> ApiResult<Json<VodJobStatus>> {\n    authorize(&headers, &state)?;\n    let tools = state.store.vod_tool_settings().map_err(internal_error)?;\n    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);\n    let status = state\n        .vod\n        .analyze(req)',
    ') -> ApiResult<Json<VodJobStatus>> {\n    authorize(&headers, &state)?;\n    let _lifecycle_guard = state.lifecycle_lock.lock().await;\n    let tools = state.store.vod_tool_settings().map_err(internal_error)?;\n    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);\n    let status = state\n        .vod\n        .analyze(req)',
)
replace_once(
    'rust-web/src/main.rs',
    ') -> ApiResult<Json<VodJobStatus>> {\n    authorize(&headers, &state)?;\n    let tools = state.store.vod_tool_settings().map_err(internal_error)?;\n    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);\n    let status = state\n        .vod\n        .download(req)',
    ') -> ApiResult<Json<VodJobStatus>> {\n    authorize(&headers, &state)?;\n    let _lifecycle_guard = state.lifecycle_lock.lock().await;\n    let tools = state.store.vod_tool_settings().map_err(internal_error)?;\n    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);\n    let status = state\n        .vod\n        .download(req)',
)

# 2) Recreate current Store schema immediately after restoring old databases.
store_insert = r'''

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
            "#,
        )?;
        drop(conn);
        self.recover_interrupted()?;
        Ok(())
    }
'''
replace_once(
    'rust-web/src/store.rs',
    '        target.execute_batch("PRAGMA foreign_keys=ON; PRAGMA wal_checkpoint(TRUNCATE);")?;\n        Ok(())\n    }\n\n    fn conn(&self)',
    '        target.execute_batch("PRAGMA foreign_keys=ON; PRAGMA wal_checkpoint(TRUNCATE);")?;\n        Ok(())\n    }' + store_insert + '\n    fn conn(&self)',
)

# Auth schema is separate from Store schema, so expose a narrow reinitializer.
replace_once(
    'rust-web/src/auth.rs',
    '        self.cleanup_expired()?;\n        Ok(())\n    }\n\n    fn configured(&self)',
    '        self.cleanup_expired()?;\n        Ok(())\n    }\n\n    pub(crate) fn reinitialize(&self) -> Result<()> {\n        self.initialize()\n    }\n\n    fn configured(&self)',
)

# 3) Backup ownership + restore schema/default hardening.
replace_once(
    'rust-web/src/backup.rs',
    '    fn ensure_policy_defaults(&self) -> Result<()> {',
    '    pub(crate) fn ensure_policy_defaults(&self) -> Result<()> {',
)
replace_once(
    'rust-web/src/backup.rs',
    '            let name_text = name.to_string_lossy();\n            if !(name_text.ends_with(".db") || name_text.ends_with(".db.json")) {\n                continue;\n            }',
    '            let name_text = name.to_string_lossy();\n            if !is_backup_artifact_name(&name_text) {\n                continue;\n            }',
)
replace_once(
    'rust-web/src/backup.rs',
    '        self.store.restore_from(&source)?;\n        let policy = self.policy()?;',
    '        self.store.restore_from(&source)?;\n        self.store.ensure_schema()?;\n        self.ensure_policy_defaults()?;\n        let policy = self.policy()?;',
)
replace_once(
    'rust-web/src/backup.rs',
    '            if path.extension().and_then(|v| v.to_str()) != Some("db") {\n                continue;\n            }',
    '            if !is_backup_database_name(&path) {\n                continue;\n            }',
)
replace_once(
    'rust-web/src/backup.rs',
    '            .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("db"))\n            .collect();',
    '            .filter(|p| is_owned_backup(p))\n            .collect();',
)
replace_once(
    'rust-web/src/backup.rs',
    '        if trimmed.is_empty()\n            || !trimmed.ends_with(".db")',
    '        if trimmed.is_empty()\n            || !trimmed.starts_with("soop_")\n            || !trimmed.ends_with(".db")',
)
replace_once(
    'rust-web/src/backup.rs',
    ') -> ApiResult<Json<Value>> {\n    authorize(&headers, &state)?;\n    let watcher = state.watcher.status().await.map_err(internal_error)?;',
    ') -> ApiResult<Json<Value>> {\n    authorize(&headers, &state)?;\n    let _lifecycle_guard = state.lifecycle_lock.lock().await;\n    let _config_guard = state.config_write_lock.lock().await;\n    let watcher = state.watcher.status().await.map_err(internal_error)?;',
)
replace_once(
    'rust-web/src/backup.rs',
    '    materialize_primary_files(&state.store, &state.backend_dir).map_err(internal_error)?;\n    let invalidated = state\n        .auth',
    '    state.auth.reinitialize().map_err(internal_error)?;\n    materialize_primary_files(&state.store, &state.backend_dir).map_err(internal_error)?;\n    let invalidated = state\n        .auth',
)

helpers = r'''

fn is_backup_database_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.starts_with("soop_") && name.ends_with(".db"))
}

fn is_backup_artifact_name(name: &str) -> bool {
    name.starts_with("soop_") && (name.ends_with(".db") || name.ends_with(".db.json"))
}

fn is_owned_backup(path: &Path) -> bool {
    if !is_backup_database_name(path) || !path.with_extension("db.json").is_file() {
        return false;
    }
    inspect_backup(path)
        .map(|info| info.integrity == "OK")
        .unwrap_or(false)
}
'''
replace_once(
    'rust-web/src/backup.rs',
    '\nfn resolve_backup_dir(backend_dir: &Path) -> Result<PathBuf> {',
    helpers + '\nfn resolve_backup_dir(backend_dir: &Path) -> Result<PathBuf> {',
)

# Add regression test: shared backup folders must never delete unrelated .db files.
backup_test = r'''

    #[test]
    fn retention_does_not_delete_unowned_databases() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("soop-recorder");
        let backend = app.join("backend");
        fs::create_dir_all(&backend).unwrap();
        let store = Store::open(app.join("data").join("soop.db")).unwrap();
        let manager = BackupManager::open(store, &backend).unwrap();
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
'''
replace_once(
    'rust-web/src/backup.rs',
    '\n    #[test]\n    fn legacy_metadata_without_kind_or_version_remains_restorable() {',
    backup_test + '\n    #[test]\n    fn legacy_metadata_without_kind_or_version_remains_restorable() {',
)

# 4) Maintenance script retention: only delete app-owned backups with sidecar metadata.
replace_once(
    'maintenance/Backup-SoopData.ps1',
    "if ($Keep -gt 0) {\n    $old = Get-ChildItem -LiteralPath $backupRoot -Filter 'soop_*.db' -File |",
    "function Get-OwnedBackupFiles {\n    Get-ChildItem -LiteralPath $backupRoot -Filter 'soop_*.db' -File | Where-Object {\n        Test-Path -LiteralPath \"$($_.FullName).json\" -PathType Leaf\n    }\n}\n\nif ($Keep -gt 0) {\n    $old = Get-OwnedBackupFiles |",
)
replace_once(
    'maintenance/Backup-SoopData.ps1',
    "$aged = Get-ChildItem -LiteralPath $backupRoot -Filter '*.db' -File | Where-Object { $_.LastWriteTime -lt $cutoff }",
    "$aged = Get-OwnedBackupFiles | Where-Object { $_.LastWriteTime -lt $cutoff }",
)

print('Phase 12 Codex review fixes applied.')
