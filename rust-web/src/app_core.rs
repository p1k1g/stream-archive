//! Shared application/service boundary for non-HTTP frontends.
//!
//! Phase 21 introduces this facade so the current Axum server, the Unix CLI,
//! and the Slint desktop UI can converge on the same Rust runtime instead of
//! duplicating SQLite, watcher, VOD, Queue, History, security, and lifecycle
//! logic. Browser authentication and HTTP concerns deliberately stay outside
//! this module.

use crate::{
    backend::LogBuffer,
    backup_service::{BackupManager, BackupPolicy, BackupSnapshot, RestoreOutcome},
    history_service::{HistoryFilter, load_history},
    model::{
        Channel, HistoryResponse, NativeWatcherStatus, VodAnalyzeRequest, VodDownloadRequest,
        VodJobStatus, VodQueueItem, VodQueueSnapshot,
    },
    native_watcher::NativeWatcherManager,
    primary_config::{
        apply_vod_tool_defaults, validate_channels, validate_secret_updates,
        validate_setting_updates, validate_vod_tool_updates,
    },
    queue_service::VodQueueManager,
    security::{protect_secret, unprotect_secret},
    store::{self, Store},
    support::{
        platform::{PlatformId, live::LiveSession},
        resolve_channel_name_for,
    },
    vod::VodManager,
};
use anyhow::{Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

const NATIVE_PROVIDER_SETTING_KEYS: &[&str] = &["SOOP_USERNAME", "CLOUDFLARE_WORKER_URL"];
const NATIVE_PROVIDER_SECRET_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
];

#[derive(Clone)]
pub struct StreamArchiveCore {
    backend_dir: Arc<PathBuf>,
    store: Store,
    logs: LogBuffer,
    watcher: Arc<NativeWatcherManager>,
    vod: Arc<VodManager>,
    queue: Arc<VodQueueManager>,
    backups: BackupManager,
    config_write_lock: Arc<Mutex<()>>,
    lifecycle_lock: Arc<Mutex<()>>,
}

pub struct CoreOpenResult {
    pub core: StreamArchiveCore,
    pub migrated_legacy_db: bool,
}

impl StreamArchiveCore {
    /// Open the canonical SQLite store and assemble the shared runtime spine.
    ///
    /// This is intended to become the common bootstrap entry point for the
    /// Axum server, Unix CLI, and Slint UI. Only one core may initialize the
    /// process-global store in a process.
    pub fn open(backend_dir: impl AsRef<Path>) -> Result<CoreOpenResult> {
        let backend_dir = backend_dir.as_ref().to_path_buf();
        let db_path = Store::default_path(&backend_dir);
        let migrated_legacy_db = Store::migrate_legacy_database(&db_path)?;
        let store = Store::open(db_path)?;
        store::init_global(store.clone())?;
        Ok(CoreOpenResult {
            core: Self::assemble(backend_dir, store)?,
            migrated_legacy_db,
        })
    }

    fn assemble(backend_dir: PathBuf, store: Store) -> Result<Self> {
        let logs = LogBuffer::new();
        let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
        let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
        let config_write_lock = Arc::new(Mutex::new(()));
        let lifecycle_lock = Arc::new(Mutex::new(()));
        let backups = BackupManager::open(store.clone(), &backend_dir)?;
        let queue = Arc::new(VodQueueManager::new(
            store.clone(),
            vod.clone(),
            logs.clone(),
            lifecycle_lock.clone(),
        )?);
        Ok(Self {
            backend_dir: Arc::new(backend_dir),
            store,
            logs,
            watcher,
            vod,
            queue,
            backups,
            config_write_lock,
            lifecycle_lock,
        })
    }

    pub fn backend_dir(&self) -> &Path {
        self.backend_dir.as_path()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn logs(&self) -> &LogBuffer {
        &self.logs
    }

    pub fn watcher(&self) -> Arc<NativeWatcherManager> {
        self.watcher.clone()
    }

    pub fn vod(&self) -> Arc<VodManager> {
        self.vod.clone()
    }

    pub fn queue(&self) -> Arc<VodQueueManager> {
        self.queue.clone()
    }

    /// Compatibility hooks are retained for the current Web adapter during
    /// Phase 21 migration. Native callers use the presentation-neutral methods
    /// below rather than taking ownership of the manager or SQLite store.
    pub fn backups(&self) -> BackupManager {
        self.backups.clone()
    }

    /// Compatibility hook for backup/Web adapters while they are moved behind
    /// the shared service boundary in later Phase 21 slices.
    pub fn config_write_lock(&self) -> Arc<Mutex<()>> {
        self.config_write_lock.clone()
    }

    /// Compatibility hook for serialized VOD/restore orchestration.
    pub fn lifecycle_lock(&self) -> Arc<Mutex<()>> {
        self.lifecycle_lock.clone()
    }

    pub fn settings(&self) -> Result<BTreeMap<String, String>> {
        self.store.safe_settings()
    }

    pub async fn update_settings(
        &self,
        updates: &BTreeMap<String, String>,
        source: &str,
    ) -> Result<BTreeMap<String, String>> {
        validate_setting_updates(updates)?;
        let _guard = self.config_write_lock.lock().await;
        self.store.sync_settings(updates, source)?;
        self.logs
            .push(format!(
                "[CORE] settings updated: {}",
                updates.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
            .await;
        self.store.safe_settings()
    }

    pub fn environment_settings(
        &self,
    ) -> Result<Vec<crate::environment_settings::EnvironmentSetting>> {
        let mut values = self.settings()?;
        values.extend(self.vod_tool_settings()?);
        Ok(crate::environment_settings::snapshot(&values))
    }

    /// Validate the whole changed patch before one canonical transaction. This
    /// avoids partially saving LIVE settings when a VOD path is invalid.
    pub async fn update_environment_settings(
        &self,
        updates: &BTreeMap<String, String>,
    ) -> Result<Vec<crate::environment_settings::EnvironmentSetting>> {
        crate::environment_settings::validate_updates(updates)?;
        let _guard = self.config_write_lock.lock().await;
        self.store.sync_settings(updates, "native-environment")?;
        self.environment_settings()
    }

    pub fn diagnostics(&self) -> crate::diagnostics::DiagnosticsSnapshot {
        let values = self.settings().and_then(|mut values| {
            values.extend(self.vod_tool_settings()?);
            Ok(values)
        });
        match values {
            Ok(values) => {
                crate::diagnostics::collect(self.backend_dir(), self.store.path(), &values)
            }
            Err(error) => crate::diagnostics::DiagnosticsSnapshot::unavailable(
                Some(self.backend_dir()),
                Some(self.store.path()),
                &format!("{error:#}"),
            ),
        }
    }

    pub fn configured_secrets(&self) -> Result<BTreeMap<String, bool>> {
        self.store.configured_secrets()
    }

    pub async fn update_secrets(
        &self,
        updates: &BTreeMap<String, String>,
        source: &str,
    ) -> Result<BTreeMap<String, bool>> {
        validate_secret_updates(updates)?;
        let _guard = self.config_write_lock.lock().await;
        let mut protected = BTreeMap::new();
        for (key, value) in updates {
            if !value.is_empty() {
                protected.insert(key.clone(), protect_secret(value)?);
            }
        }
        if !protected.is_empty() {
            self.store.sync_settings(&protected, source)?;
            self.logs
                .push(format!(
                    "[CORE] protected secrets updated: {}",
                    protected.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
                .await;
        }
        self.store.configured_secrets()
    }

    /// Save the provider-facing native configuration through the same
    /// validators and protected-secret boundary used by the Web UI. Empty
    /// secret drafts intentionally preserve the previously stored secret.
    pub async fn update_provider_configuration(
        &self,
        settings: &BTreeMap<String, String>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, bool>> {
        for key in settings.keys() {
            if !NATIVE_PROVIDER_SETTING_KEYS.contains(&key.as_str()) {
                bail!("unsupported native provider setting: {key}");
            }
        }
        for key in secrets.keys() {
            if !NATIVE_PROVIDER_SECRET_KEYS.contains(&key.as_str()) {
                bail!("unsupported native provider secret: {key}");
            }
        }
        validate_setting_updates(settings)?;
        validate_secret_updates(secrets)?;

        let mut updates = settings.clone();
        for (key, value) in secrets {
            if !value.is_empty() {
                updates.insert(key.clone(), protect_secret(value)?);
            }
        }

        let _guard = self.config_write_lock.lock().await;
        if !updates.is_empty() {
            self.store.sync_settings(&updates, "native-provider")?;
            self.logs
                .push(format!(
                    "[CORE] native provider configuration updated: {}",
                    updates.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
                .await;
        }
        self.store.configured_secrets()
    }

    /// Verify the currently saved SOOP login and Cloudflare Worker credentials.
    /// Secret values are decrypted only inside the shared service boundary and
    /// are never returned to the native frontend or written to logs.
    pub async fn test_soop_auth(&self) -> Result<String> {
        let settings = self.store.live_settings_with_secrets()?;
        let username = settings.get("SOOP_USERNAME").cloned().unwrap_or_default();
        let password = unprotect_secret(
            settings
                .get("SOOP_PASSWORD")
                .map(String::as_str)
                .unwrap_or(""),
            "SOOP_PASSWORD",
        )?;
        let worker_url = settings
            .get("CLOUDFLARE_WORKER_URL")
            .cloned()
            .unwrap_or_default();
        let worker_key = unprotect_secret(
            settings
                .get("CLOUDFLARE_API_KEY")
                .map(String::as_str)
                .unwrap_or(""),
            "CLOUDFLARE_API_KEY",
        )?;

        for (label, value) in [
            ("SOOP username", username.as_str()),
            ("SOOP password", password.as_str()),
            ("Worker URL", worker_url.as_str()),
            ("Worker API key", worker_key.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{label} is not configured");
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .no_proxy()
            .http1_only()
            .build()?;
        let mut session = LiveSession::new(PlatformId::Soop, client.clone())?;
        let login_id = session
            .login(&username, &password)
            .await
            .map_err(|error| anyhow::anyhow!("SOOP login test failed: {error:#}"))?;

        let response = client
            .post(&worker_url)
            .header("X-API-Key", &worker_key)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("Worker test failed: {error:#}"))?;
        let worker_status = response.status();
        if worker_status == reqwest::StatusCode::UNAUTHORIZED
            || worker_status == reqwest::StatusCode::FORBIDDEN
        {
            bail!("Worker API key authentication failed");
        }
        if worker_status != reqwest::StatusCode::BAD_REQUEST && !worker_status.is_success() {
            bail!("Worker endpoint test failed: HTTP {worker_status}");
        }

        self.logs
            .push(format!(
                "[AUTH] SOOP credential test passed login_id={login_id}"
            ))
            .await;
        Ok(format!(
            "SOOP login and Worker authentication passed (login_id={login_id})"
        ))
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        self.store.channels()
    }

    pub async fn resolve_channel_name(
        &self,
        platform: PlatformId,
        account: &str,
    ) -> Result<String> {
        resolve_channel_name_for(platform, account).await
    }

    pub async fn update_channels(&self, channels: &[Channel]) -> Result<Vec<Channel>> {
        validate_channels(channels)?;
        let _guard = self.config_write_lock.lock().await;
        self.store.sync_channels(channels)?;
        let saved = self.store.channels()?;
        self.logs
            .push(format!(
                "[CORE] channel list updated ({} channels)",
                saved.len()
            ))
            .await;
        Ok(saved)
    }

    pub async fn watcher_status(&self) -> Result<NativeWatcherStatus> {
        self.watcher.status().await
    }

    pub async fn start_watcher(&self) -> Result<NativeWatcherStatus> {
        let _guard = self.lifecycle_lock.lock().await;
        self.watcher.start().await
    }

    pub async fn stop_watcher(&self) -> Result<NativeWatcherStatus> {
        self.watcher.stop().await
    }

    pub async fn channel_action(&self, account: String, action: &str) -> Result<()> {
        self.watcher.channel_action(account, action).await
    }

    pub async fn channel_password(&self, account: String, password: String) -> Result<()> {
        self.watcher.channel_password(account, password).await
    }

    pub fn vod_tool_settings(&self) -> Result<BTreeMap<String, String>> {
        self.store.vod_tool_settings()
    }

    pub async fn update_vod_tool_settings(
        &self,
        updates: &BTreeMap<String, String>,
        source: &str,
    ) -> Result<BTreeMap<String, String>> {
        validate_vod_tool_updates(updates)?;
        let _guard = self.config_write_lock.lock().await;
        self.store.sync_settings(updates, source)?;
        self.store.vod_tool_settings()
    }

    pub async fn vod_status(&self) -> Result<VodJobStatus> {
        let status = self.vod.status().await;
        self.store.upsert_vod(&status)?;
        Ok(status)
    }

    pub async fn analyze_vod(&self, mut req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        let _guard = self.lifecycle_lock.lock().await;
        let tools = self.store.vod_tool_settings()?;
        apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
        let status = self.vod.analyze(req).await?;
        self.store.upsert_vod(&status)?;
        Ok(status)
    }

    pub async fn download_vod(&self, mut req: VodDownloadRequest) -> Result<VodJobStatus> {
        let _guard = self.lifecycle_lock.lock().await;
        let tools = self.store.vod_tool_settings()?;
        apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
        let status = self.vod.download(req).await?;
        self.store.upsert_vod(&status)?;
        Ok(status)
    }

    pub async fn cancel_vod(&self) -> Result<VodJobStatus> {
        let _guard = self.lifecycle_lock.lock().await;
        let status = self.vod.cancel().await?;
        self.store.upsert_vod(&status)?;
        Ok(status)
    }

    pub fn spawn_queue_worker(&self) -> bool {
        self.queue.clone().spawn()
    }

    pub async fn queue_snapshot(&self) -> Result<VodQueueSnapshot> {
        self.queue.snapshot().await
    }

    pub async fn enqueue_vod(&self, mut req: VodDownloadRequest) -> Result<VodQueueItem> {
        let _config_guard = self.config_write_lock.lock().await;
        let tools = self.store.vod_tool_settings()?;
        apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
        self.queue.enqueue(req).await
    }

    pub async fn cancel_queue_item(&self, id: &str) -> Result<VodQueueSnapshot> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.queue.cancel(id).await?;
        self.queue.snapshot().await
    }

    pub async fn retry_queue_item(&self, id: &str) -> Result<VodQueueSnapshot> {
        let _config_guard = self.config_write_lock.lock().await;
        self.queue.retry(id).await?;
        self.queue.snapshot().await
    }

    pub async fn remove_queue_item(&self, id: &str) -> Result<VodQueueSnapshot> {
        let _config_guard = self.config_write_lock.lock().await;
        self.queue.remove(id).await?;
        self.queue.snapshot().await
    }

    pub fn history(&self, filter: &HistoryFilter) -> Result<HistoryResponse> {
        load_history(self.store.path(), filter)
    }

    pub async fn backup_snapshot(&self) -> Result<BackupSnapshot> {
        self.backups.snapshot().await
    }

    pub async fn update_backup_policy(
        &self,
        policy: &BackupPolicy,
        directory: Option<&str>,
    ) -> Result<BackupSnapshot> {
        let _config_guard = self.config_write_lock.lock().await;
        let snapshot = self.backups.update_policy(policy, directory).await?;
        self.logs
            .push(format!(
                "[BACKUP] policy updated enabled={} interval_hours={} keep_count={} retention_days={} directory={}",
                policy.enabled,
                policy.interval_hours,
                policy.keep_count,
                policy.retention_days,
                snapshot.directory
            ))
            .await;
        Ok(snapshot)
    }

    pub async fn create_manual_backup(&self) -> Result<BackupSnapshot> {
        let item = self.backups.create_manual().await?;
        self.logs
            .push(format!(
                "[BACKUP] manual backup created file={} size={} sha256={}",
                item.file_name, item.size_bytes, item.sha256
            ))
            .await;
        self.backups.snapshot().await
    }

    pub async fn restore_backup(&self, file_name: &str) -> Result<RestoreOutcome> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let _config_guard = self.config_write_lock.lock().await;

        let watcher = self.watcher.status().await?;
        if watcher.running || watcher.recording_count > 0 {
            bail!("stop the LIVE watcher and active recordings before restore");
        }
        if self.vod.status().await.running {
            bail!("stop the active VOD operation before restore");
        }
        if self.queue.has_pending_or_active().await? {
            bail!("clear or finish the VOD Queue before restore");
        }

        let outcome = self.backups.restore(file_name).await?;
        self.logs
            .push(format!(
                "[BACKUP] database restored file={} safety={}",
                outcome.restored.file_name, outcome.safety_backup.file_name
            ))
            .await;
        Ok(outcome)
    }

    pub async fn runtime_logs(&self, max_lines: usize) -> Vec<String> {
        self.logs.tail(max_lines.clamp(1, 400)).await
    }

    pub fn spawn_auto_backup(&self) {
        crate::backup_service::spawn_auto_backup(self.backups.clone(), self.logs.clone());
    }

    /// Keep VOD history synchronized even when the caller is not the Web UI.
    pub fn spawn_vod_history_sync(&self) {
        let store = self.store.clone();
        let vod = self.vod.clone();
        let logs = self.logs.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let status = vod.status().await;
                if let Err(err) = store.upsert_vod(&status) {
                    logs.push(format!("[DB:WARN] VOD history sync failed: {err:#}"))
                        .await;
                }
            }
        });
    }

    /// Stop new Queue claims first, then only runtime children owned by Stream Archive.
    pub async fn shutdown(&self) {
        self.queue.shutdown().await;
        let _ = self.vod.cancel().await;
        let _ = self.watcher.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn assembled_core_keeps_one_canonical_store_backend_and_queue() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = dir
            .path()
            .join("app")
            .join("data")
            .join("stream-archive.db");
        let store = Store::open(db.clone()).unwrap();
        let core = StreamArchiveCore::assemble(backend.clone(), store).unwrap();

        assert_eq!(core.backend_dir(), backend.as_path());
        assert_eq!(core.store().path(), db.as_path());
        assert!(core.settings().unwrap().contains_key("STREAMLINK_PATH"));
        assert_eq!(core.queue_snapshot().await.unwrap().queued_count, 0);
        let backup = core.backup_snapshot().await.unwrap();
        assert_eq!(backup.policy.interval_hours, 24);
        assert!(backup.directory_editable);
    }

    #[tokio::test]
    async fn native_restore_requires_idle_runtime_and_refreshes_canonical_store() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(
            dir.path()
                .join("app")
                .join("data")
                .join("stream-archive.db"),
        )
        .unwrap();
        let core = StreamArchiveCore::assemble(backend, store).unwrap();

        core.store
            .sync_settings(
                &BTreeMap::from([("TEST_RESTORE".into(), "before".into())]),
                "test",
            )
            .unwrap();
        let snapshot = core.create_manual_backup().await.unwrap();
        let backup = snapshot
            .backups
            .iter()
            .find(|item| item.kind == "manual")
            .unwrap()
            .file_name
            .clone();

        core.store
            .sync_settings(
                &BTreeMap::from([("TEST_RESTORE".into(), "after".into())]),
                "test",
            )
            .unwrap();

        let outcome = core.restore_backup(&backup).await.unwrap();
        assert_eq!(outcome.restored.file_name, backup);
        assert_eq!(outcome.safety_backup.kind, "pre_restore");
        assert_eq!(
            core.store.setting_value("TEST_RESTORE").unwrap().as_deref(),
            Some("before")
        );
    }

    #[tokio::test]
    async fn runtime_logs_are_bounded_by_requested_tail() {
        let dir = tempfile::tempdir().unwrap();
        let core = StreamArchiveCore::assemble(
            dir.path().to_path_buf(),
            Store::open(dir.path().join("stream-archive.db")).unwrap(),
        )
        .unwrap();
        for index in 0..8 {
            core.logs.push(format!("line-{index}")).await;
        }
        let tail = core.runtime_logs(3).await;
        assert_eq!(tail, vec!["line-5", "line-6", "line-7"]);
    }

    #[tokio::test]
    async fn environment_patch_is_atomic_and_uses_existing_web_keys() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let core =
            StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db.clone()).unwrap())
                .unwrap();
        let before = core.settings().unwrap();
        let invalid = BTreeMap::from([
            ("CHECK_INTERVAL".into(), "42".into()),
            (
                "FFMPEG_PATH".into(),
                dir.path().join("missing.exe").display().to_string(),
            ),
        ]);
        assert!(core.update_environment_settings(&invalid).await.is_err());
        assert_eq!(core.settings().unwrap(), before);
        let valid = BTreeMap::from([
            ("CHECK_INTERVAL".into(), "42".into()),
            ("FFMPEG_PATH".into(), String::new()),
        ]);
        core.update_environment_settings(&valid).await.unwrap();
        assert_eq!(core.settings().unwrap()["CHECK_INTERVAL"], "42");
        assert_eq!(core.vod_tool_settings().unwrap()["FFMPEG_PATH"], "");
        drop(core);
        let reopened = Store::open(db).unwrap();
        assert_eq!(reopened.safe_settings().unwrap()["CHECK_INTERVAL"], "42");
    }

    #[tokio::test]
    async fn native_provider_update_reuses_safe_setting_validation() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let core = StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db).unwrap())
            .unwrap();

        let valid = BTreeMap::from([
            ("SOOP_USERNAME".into(), "tester".into()),
            (
                "CLOUDFLARE_WORKER_URL".into(),
                "https://worker.example.test".into(),
            ),
        ]);
        core.update_provider_configuration(&valid, &BTreeMap::new())
            .await
            .unwrap();
        let settings = core.settings().unwrap();
        assert_eq!(settings["SOOP_USERNAME"], "tester");
        assert_eq!(
            settings["CLOUDFLARE_WORKER_URL"],
            "https://worker.example.test"
        );

        let invalid = BTreeMap::from([("CHECK_INTERVAL".into(), "1".into())]);
        assert!(
            core.update_provider_configuration(&invalid, &BTreeMap::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn queue_facade_applies_existing_tool_defaults_and_history_filter_validates() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let core = StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db).unwrap())
            .unwrap();
        let req = VodDownloadRequest {
            vod_url: "https://vod.sooplive.com/player/123456789".into(),
            output_directory: dir.path().display().to_string(),
            parts: vec![],
            quality: "best".into(),
            merge: true,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            max_retries: 5,
        };
        let queued = core.enqueue_vod(req).await.unwrap();
        assert_eq!(queued.state, "QUEUED");
        assert!(
            core.history(&HistoryFilter {
                from: Some("bad-date".into()),
                ..Default::default()
            })
            .is_err()
        );
    }
}
