//! Shared application/service boundary for non-HTTP frontends.
//!
//! Canonical presentation-neutral application facade shared by the Native UI,
//! Unix CLI and compatible headless runtime. It centralizes SQLite, watcher,
//! VOD, Queue, History, security, backup and lifecycle logic. Presentation
//! concerns deliberately stay outside this module.

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
        validate_setting_updates,
    },
    queue_service::VodQueueManager,
    runtime_owner::{RuntimeOwnerGuard, runtime_owner_active},
    security::{protect_secret, unprotect_secret},
    store::{self, Store},
    support::{
        platform::{PlatformId, live::LiveSession},
        resolve_channel_name_for,
    },
    vod::VodManager,
};
use anyhow::{Context, Result, bail};
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
    "KICK_SESSION_TOKEN",
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
    runtime_owner: Option<Arc<RuntimeOwnerGuard>>,
}

pub struct CoreOpenResult {
    pub core: StreamArchiveCore,
    pub migrated_legacy_db: bool,
}

const KICK_CLEANUP_KEY: &str = "STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS";

fn kick_secret_write_guard(store: &Store) -> Result<std::fs::File> {
    let mut path = store.path().as_os_str().to_os_string();
    path.push(".kick-secret.lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(PathBuf::from(path))
        .context("KICK 인증정보 변경 잠금 파일을 열지 못했습니다.")?;
    fs2::FileExt::try_lock_exclusive(&file)
        .context("다른 프로세스가 KICK 인증정보를 변경 중입니다. 잠시 후 재시도하세요.")?;
    Ok(file)
}

fn cleanup_kick_secrets(store: &Store, delete: &mut impl FnMut(&str) -> Result<()>) -> Result<()> {
    for reference in store.pending_secret_cleanup(KICK_CLEANUP_KEY)? {
        delete(&reference).context(
            "KICK 이전 인증정보 정리가 필요합니다. 저장된 정리 대기 참조로 재시도하세요.",
        )?;
        store.finish_secret_cleanup(KICK_CLEANUP_KEY, &reference)?;
    }
    Ok(())
}

fn commit_kick_configuration(
    store: &Store,
    updates: &BTreeMap<String, String>,
    delete: &mut impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    if let Err(error) = store.sync_settings_retiring_secret(
        updates,
        "native-provider",
        "KICK_SESSION_TOKEN",
        KICK_CLEANUP_KEY,
        |value| crate::security::native_cleanup_reference(value, "KICK_SESSION_TOKEN"),
    ) {
        if let Some(value) = updates.get("KICK_SESSION_TOKEN") {
            delete(value).context("KICK 저장 실패 후 새 native credential 정리가 필요합니다.")?;
            if let Some(reference) =
                crate::security::native_cleanup_reference(value, "KICK_SESSION_TOKEN")?
            {
                store.finish_secret_cleanup(KICK_CLEANUP_KEY, &reference)?;
            }
        }
        return Err(error);
    }
    cleanup_kick_secrets(store, delete)
        .context("KICK 새 인증정보는 저장됐지만 이전 native credential 정리가 완료되지 않았습니다.")
}

fn clear_kick_token_in_store(
    store: &Store,
    mut delete: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    store.sync_settings_retiring_secret(
        &BTreeMap::from([("KICK_SESSION_TOKEN".into(), String::new())]),
        "native-provider",
        "KICK_SESSION_TOKEN",
        KICK_CLEANUP_KEY,
        |value| crate::security::native_cleanup_reference(value, "KICK_SESSION_TOKEN"),
    )?;
    cleanup_kick_secrets(store, &mut delete)
}

impl StreamArchiveCore {
    /// Open the canonical SQLite store and assemble the shared runtime spine.
    ///
    /// This is intended to become the common bootstrap entry point for the
    /// Native UI, Unix CLI, and headless runtime. Only one core may initialize the
    /// process-global store in a process.
    pub fn open(backend_dir: impl AsRef<Path>) -> Result<CoreOpenResult> {
        let backend_dir = backend_dir.as_ref().to_path_buf();
        let db_path = Store::default_path(&backend_dir);
        let runtime_owner = Arc::new(RuntimeOwnerGuard::acquire(&db_path)?);
        let migrated_legacy_db = Store::migrate_legacy_database(&db_path)?;
        let store = Store::open(db_path)?;
        store::init_global(store.clone())?;
        Ok(CoreOpenResult {
            core: Self::assemble_with_mode(backend_dir, store, Some(runtime_owner), true)?,
            migrated_legacy_db,
        })
    }

    pub fn open_observer(backend_dir: impl AsRef<Path>) -> Result<CoreOpenResult> {
        let backend_dir = backend_dir.as_ref().to_path_buf();
        let db_path = Store::default_path(&backend_dir);
        let migrated_legacy_db = if runtime_owner_active(&db_path)? {
            false
        } else {
            Store::migrate_legacy_database(&db_path)?
        };
        let store = Store::open_observer(db_path)?;
        store::init_global(store.clone())?;
        Ok(CoreOpenResult {
            core: Self::assemble_with_mode(backend_dir, store, None, false)?,
            migrated_legacy_db,
        })
    }

    #[cfg(test)]
    fn assemble(backend_dir: PathBuf, store: Store) -> Result<Self> {
        Self::assemble_with_mode(backend_dir, store, None, true)
    }

    fn assemble_with_mode(
        backend_dir: PathBuf,
        store: Store,
        runtime_owner: Option<Arc<RuntimeOwnerGuard>>,
        recover_queue: bool,
    ) -> Result<Self> {
        let logs = LogBuffer::new();
        let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
        let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
        let config_write_lock = Arc::new(Mutex::new(()));
        let lifecycle_lock = Arc::new(Mutex::new(()));
        let backups = BackupManager::open(store.clone(), &backend_dir)?;
        let queue = Arc::new(if recover_queue {
            VodQueueManager::new(
                store.clone(),
                vod.clone(),
                logs.clone(),
                lifecycle_lock.clone(),
            )?
        } else {
            VodQueueManager::new_observer(
                store.clone(),
                vod.clone(),
                logs.clone(),
                lifecycle_lock.clone(),
            )?
        });
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
            runtime_owner,
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

    pub fn owns_runtime(&self) -> bool {
        self.runtime_owner.is_some()
    }

    pub fn another_runtime_active(&self) -> Result<bool> {
        if self.owns_runtime() {
            Ok(false)
        } else {
            runtime_owner_active(self.store.path())
        }
    }

    pub fn is_first_run_unconfigured(&self) -> Result<bool> {
        self.store.is_first_run_unconfigured()
    }

    pub fn settings(&self) -> Result<BTreeMap<String, String>> {
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
        if updates.contains_key("STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS") {
            self.vod.notification_settings_changed().await;
        }
        self.environment_settings()
    }

    pub async fn active_local_diagnostics(&self) -> crate::diagnostics::DiagnosticsSnapshot {
        crate::diagnostics::collect_active_local_preflight(self.backend_dir(), self.store.path())
            .await
    }

    pub fn diagnostics(&self) -> crate::diagnostics::DiagnosticsSnapshot {
        let values = self.settings().and_then(|mut values| {
            values.extend(self.vod_tool_settings()?);
            Ok(values)
        });
        let secrets = self.configured_secrets();
        match (values, secrets) {
            (Ok(values), Ok(secrets)) => match self.backups.policy() {
                Ok(policy) => crate::diagnostics::collect_with_backup_and_secrets(
                    self.backend_dir(),
                    self.store.path(),
                    &values,
                    &secrets,
                    &self.backups.backup_dir(),
                    &policy,
                ),
                Err(error) => crate::diagnostics::DiagnosticsSnapshot::unavailable(
                    Some(self.backend_dir()),
                    Some(self.store.path()),
                    &format!("backup policy load failed: {error:#}"),
                ),
            },
            (Err(error), _) | (_, Err(error)) => {
                crate::diagnostics::DiagnosticsSnapshot::unavailable(
                    Some(self.backend_dir()),
                    Some(self.store.path()),
                    &format!("{error:#}"),
                )
            }
        }
    }

    pub fn configured_secrets(&self) -> Result<BTreeMap<String, bool>> {
        self.store.configured_secrets()
    }

    /// Save provider-facing native configuration through the canonical
    /// validators and protected-secret boundary. Empty
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

        let _guard = self.config_write_lock.lock().await;
        let mut delete =
            |value: &str| crate::security::delete_protected_secret(value, "KICK_SESSION_TOKEN");
        let kick = secrets
            .get("KICK_SESSION_TOKEN")
            .filter(|value| !value.is_empty());
        let _secret_guard = if kick.is_some() {
            Some(kick_secret_write_guard(&self.store)?)
        } else {
            None
        };
        if kick.is_some() {
            cleanup_kick_secrets(&self.store, &mut delete)?;
        }
        let mut updates = settings.clone();
        for (key, value) in secrets {
            if key != "KICK_SESSION_TOKEN" && !value.is_empty() {
                updates.insert(key.clone(), protect_secret(value)?);
            }
        }
        // Create KICK last so another provider's protection failure cannot orphan it.
        if let Some(value) = kick {
            let protected =
                crate::security::protect_secret_with_cleanup_intent(value, |reference| {
                    self.store
                        .retain_secret_cleanup(KICK_CLEANUP_KEY, reference)
                })?;
            updates.insert("KICK_SESSION_TOKEN".into(), protected);
        }
        if !updates.is_empty() {
            if kick.is_some() {
                commit_kick_configuration(&self.store, &updates, &mut delete)?;
            } else {
                self.store.sync_settings(&updates, "native-provider")?;
            }
            self.logs
                .push(format!(
                    "[CORE] native provider configuration updated: {}",
                    updates.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
                .await;
        }
        self.store.configured_secrets()
    }

    /// Explicit deletion; empty provider drafts continue to preserve saved values.
    pub async fn clear_kick_token(&self) -> Result<()> {
        let _guard = self.config_write_lock.lock().await;
        let _secret_guard = kick_secret_write_guard(&self.store)?;
        let delete =
            |value: &str| crate::security::delete_protected_secret(value, "KICK_SESSION_TOKEN");
        clear_kick_token_in_store(&self.store, delete)
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

    /// Download only the current, already-probed broadcast's public snapshot.
    pub async fn live_thumbnail(
        &self,
        platform: PlatformId,
        account: &str,
        broadcast_id: &str,
        completion_revision: u64,
    ) -> Result<crate::thumbnail_service::ThumbnailImage> {
        let status = self.watcher_status().await?;
        let url = status
            .channels
            .iter()
            .find(|row| {
                status.running
                    && row.platform == platform
                    && row.account == account
                    && row.completion_revision == completion_revision
                    && row.bno.as_deref() == Some(broadcast_id)
                    && !matches!(
                        row.status.as_str(),
                        "OFFLINE" | "DISABLED" | "WATCHER_STOPPED"
                    )
            })
            .and_then(|row| row.thumbnail_url.clone())
            .ok_or_else(|| anyhow::anyhow!("현재 방송의 썸네일 정보가 없습니다."))?;
        let image = match crate::thumbnail_service::load(platform, &url).await {
            Ok(image) => image,
            Err(error) => {
                self.logs
                    .push(format!(
                        "[LIVE:THUMBNAIL:ERR] platform={platform} reason={}",
                        crate::thumbnail_service::failure_reason(&error)
                    ))
                    .await;
                return Err(error);
            }
        };
        let current = self.watcher_status().await?;
        if !current.running
            || !current.channels.iter().any(|row| {
                row.platform == platform
                    && row.account == account
                    && row.bno.as_deref() == Some(broadcast_id)
                    && row.completion_revision == completion_revision
                    && !matches!(
                        row.status.as_str(),
                        "OFFLINE" | "DISABLED" | "WATCHER_STOPPED"
                    )
            })
        {
            anyhow::bail!("썸네일 요청 중 방송 상태가 변경되었습니다.");
        }
        Ok(image)
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

    pub async fn add_channel(&self, channel: Channel) -> Result<Vec<Channel>> {
        validate_channels(std::slice::from_ref(&channel))?;
        let _guard = self.config_write_lock.lock().await;
        self.store.insert_channel(&channel)?;
        let saved = self.store.channels()?;
        self.logs
            .push(format!(
                "[CORE] channel added {}/{}",
                channel.platform, channel.account
            ))
            .await;
        Ok(saved)
    }

    pub async fn remove_channel(
        &self,
        platform: PlatformId,
        account: &str,
    ) -> Result<Vec<Channel>> {
        let _guard = self.config_write_lock.lock().await;
        self.store.delete_channel(platform, account)?;
        let saved = self.store.channels()?;
        self.logs
            .push(format!("[CORE] channel removed {platform}/{account}"))
            .await;
        Ok(saved)
    }

    pub async fn set_channel_enabled(
        &self,
        platform: PlatformId,
        account: &str,
        enabled: bool,
    ) -> Result<Vec<Channel>> {
        let _guard = self.config_write_lock.lock().await;
        self.store.set_channel_enabled(platform, account, enabled)?;
        let saved = self.store.channels()?;
        self.logs
            .push(format!(
                "[CORE] channel {} {platform}/{account}",
                if enabled { "enabled" } else { "disabled" }
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

    fn vod_tool_settings(&self) -> Result<BTreeMap<String, String>> {
        self.store.vod_tool_settings()
    }

    pub async fn vod_status(&self) -> Result<VodJobStatus> {
        let status = self.vod.status().await;
        self.store.upsert_vod(&status)?;
        Ok(status)
    }

    /// New session results only; notification failures never affect VOD lifecycle.
    pub fn subscribe_download_events(&self) -> crate::download_events::DownloadSubscription {
        self.vod.subscribe_download_events()
    }

    pub async fn local_vod_status(&self) -> VodJobStatus {
        self.vod.status().await
    }

    /// Presentation receives only a bounded image for the current analysis job.
    pub async fn vod_thumbnail(
        &self,
        job_id: &str,
        vod_url: &str,
    ) -> Result<crate::thumbnail_service::ThumbnailImage> {
        let status = self.vod.status().await;
        let analysis = status
            .analysis
            .as_ref()
            .filter(|analysis| {
                status.job_id.as_deref() == Some(job_id) && analysis.vod_url == vod_url
            })
            .ok_or_else(|| anyhow::anyhow!("현재 VOD 분석 결과와 일치하지 않습니다."))?;
        let url = analysis
            .thumbnail_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("VOD 썸네일 정보가 없습니다."))?;
        let image = crate::thumbnail_service::load_vod(status.platform, url).await?;
        let current = self.vod.status().await;
        if current.job_id.as_deref() != Some(job_id)
            || current
                .analysis
                .as_ref()
                .is_none_or(|analysis| analysis.vod_url != vod_url)
        {
            bail!("썸네일 요청 중 VOD 분석 결과가 변경되었습니다.");
        }
        Ok(image)
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

    pub fn storage_snapshot(&self) -> Result<crate::storage_service::StorageSnapshot> {
        crate::storage_service::snapshot(&self.store)
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
        let _cross_process_owner = if self.runtime_owner.is_none() {
            Some(RuntimeOwnerGuard::acquire(self.store.path()).context(
                "cannot restore while another Stream Archive runtime owns the canonical database",
            )?)
        } else {
            None
        };
        if self.runtime_owner.is_none() {
            self.store.recover_interrupted()?;
        }

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
        self.vod.invalidate_download_events();
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

    /// Keep VOD history synchronized for headless/native runtime callers.
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

    const OLD_KICK_REFERENCE: &str = "native-secret:v1:123e4567-e89b-12d3-a456-426614174000";
    const NEW_KICK_REFERENCE: &str = "native-secret:v1:123e4567-e89b-12d3-a456-426614174001";

    #[test]
    fn kick_replacement_removes_previous_native_item_only_after_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        store
            .retain_secret_cleanup(KICK_CLEANUP_KEY, NEW_KICK_REFERENCE)
            .unwrap();
        let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
        let mut deleted = Vec::new();
        commit_kick_configuration(&store, &updates, &mut |reference| {
            assert_eq!(
                store.setting_value("KICK_SESSION_TOKEN")?.unwrap(),
                NEW_KICK_REFERENCE
            );
            deleted.push(reference.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(deleted, vec![OLD_KICK_REFERENCE]);
        assert!(
            store
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn kick_replacement_cleans_new_native_item_on_sqlite_commit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        let conn = rusqlite::Connection::open(store.path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_kick_update BEFORE UPDATE ON settings WHEN NEW.key='KICK_SESSION_TOKEN' BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
        )
        .unwrap();
        let mut deleted = Vec::new();
        let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
        assert!(
            commit_kick_configuration(&store, &updates, &mut |reference| {
                deleted.push(reference.to_string());
                Ok(())
            })
            .is_err()
        );
        assert_eq!(deleted, vec![NEW_KICK_REFERENCE]);
        assert_eq!(
            store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
            OLD_KICK_REFERENCE
        );
        assert!(
            store
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn kick_secret_guard_serializes_observer_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = Store::open(path.clone()).unwrap();
        let observer = Store::open_observer(path).unwrap();
        let guard = kick_secret_write_guard(&owner).unwrap();
        assert!(kick_secret_write_guard(&observer).is_err());
        drop(guard);
        assert!(kick_secret_write_guard(&observer).is_ok());
    }

    #[test]
    fn kick_failed_save_retains_new_reference_when_rollback_cleanup_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        store
            .retain_secret_cleanup(KICK_CLEANUP_KEY, NEW_KICK_REFERENCE)
            .unwrap();
        let conn = rusqlite::Connection::open(store.path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_kick_update BEFORE UPDATE ON settings WHEN NEW.key='KICK_SESSION_TOKEN' BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
        )
        .unwrap();
        let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
        assert!(commit_kick_configuration(&store, &updates, &mut |_| bail!("locked")).is_err());
        assert_eq!(
            store.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
            vec![NEW_KICK_REFERENCE]
        );
        cleanup_kick_secrets(&store, &mut |reference| {
            assert_eq!(reference, NEW_KICK_REFERENCE);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
            OLD_KICK_REFERENCE
        );
    }

    #[test]
    fn kick_retired_native_cleanup_failure_survives_reopen_and_is_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let store = Store::open(path.clone()).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
        assert!(commit_kick_configuration(&store, &updates, &mut |_| bail!("locked")).is_err());
        drop(store);
        let reopened = Store::open(path).unwrap();
        assert_eq!(
            reopened
                .setting_value("KICK_SESSION_TOKEN")
                .unwrap()
                .unwrap(),
            NEW_KICK_REFERENCE
        );
        assert_eq!(
            reopened.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
            vec![OLD_KICK_REFERENCE]
        );
        cleanup_kick_secrets(&reopened, &mut |reference| {
            assert_eq!(reference, OLD_KICK_REFERENCE);
            Ok(())
        })
        .unwrap();
        assert!(
            reopened
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            reopened
                .setting_value("KICK_SESSION_TOKEN")
                .unwrap()
                .unwrap(),
            NEW_KICK_REFERENCE
        );
    }

    #[test]
    fn kick_clear_commits_cleanup_before_deletion_and_retries_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = Store::open(path.clone()).unwrap();
        let observer = Store::open_observer(path.clone()).unwrap();
        observer
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        assert!(
            clear_kick_token_in_store(&owner, |value| {
                assert_eq!(value, OLD_KICK_REFERENCE);
                observer.refresh_config_cache()?;
                assert_eq!(observer.setting_value("KICK_SESSION_TOKEN")?.unwrap(), "");
                bail!("credential store locked")
            })
            .is_err()
        );
        assert_eq!(
            owner.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
            ""
        );
        drop(owner);
        let reopened = Store::open(path).unwrap();
        assert_eq!(
            reopened.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
            vec![OLD_KICK_REFERENCE]
        );
        clear_kick_token_in_store(&reopened, |value| {
            assert_eq!(value, OLD_KICK_REFERENCE);
            Ok(())
        })
        .unwrap();
        assert!(
            reopened
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn kick_clear_does_not_delete_native_item_when_db_transition_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        store
            .sync_settings(
                &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        let conn = rusqlite::Connection::open(store.path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_clear BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
        )
        .unwrap();
        assert!(
            clear_kick_token_in_store(&store, |_| {
                panic!("native deletion must not happen before a committed DB transition")
            })
            .is_err()
        );
        assert_eq!(
            store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
            OLD_KICK_REFERENCE
        );
        assert!(
            store
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn kick_clear_preserves_replacement_written_during_native_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = Store::open(path.clone()).unwrap();
        let observer = Store::open_observer(path).unwrap();
        let key = "KICK_SESSION_TOKEN";
        observer
            .sync_settings(
                &BTreeMap::from([(key.into(), OLD_KICK_REFERENCE.into())]),
                "test",
            )
            .unwrap();
        clear_kick_token_in_store(&owner, |value| {
            assert_eq!(value, OLD_KICK_REFERENCE);
            observer.sync_settings(
                &BTreeMap::from([(key.into(), NEW_KICK_REFERENCE.into())]),
                "test",
            )
        })
        .unwrap();
        owner.refresh_config_cache().unwrap();
        assert_eq!(
            owner.setting_value(key).unwrap().unwrap(),
            NEW_KICK_REFERENCE
        );
    }

    #[tokio::test]
    async fn desktop_close_action_defaults_to_exit_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = Store::default_path(&backend);
        let core = StreamArchiveCore::assemble(backend, Store::open(db.clone()).unwrap()).unwrap();
        assert_eq!(
            core.settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
            "EXIT"
        );
        core.update_environment_settings(&BTreeMap::from([(
            "STREAM_ARCHIVE_CLOSE_ACTION".into(),
            "TRAY".into(),
        )]))
        .await
        .unwrap();
        assert!(
            core.update_environment_settings(&BTreeMap::from([(
                "STREAM_ARCHIVE_CLOSE_ACTION".into(),
                "invalid".into()
            ),]))
                .await
                .is_err()
        );
        core.shutdown().await;
        drop(core);
        let reopened = Store::open(db).unwrap();
        assert_eq!(
            reopened.safe_settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
            "TRAY"
        );
    }

    #[tokio::test]
    async fn desktop_close_action_defaults_to_exit_and_ask_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = Store::default_path(&backend);
        let core = StreamArchiveCore::assemble(backend, Store::open(db.clone()).unwrap()).unwrap();
        assert_eq!(
            core.settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
            "EXIT"
        );
        core.update_environment_settings(&BTreeMap::from([(
            "STREAM_ARCHIVE_CLOSE_ACTION".into(),
            "ASK".into(),
        )]))
        .await
        .unwrap();
        assert!(
            core.update_environment_settings(&BTreeMap::from([(
                "STREAM_ARCHIVE_CLOSE_ACTION".into(),
                "invalid".into()
            ),]))
                .await
                .is_err()
        );
        core.shutdown().await;
        drop(core);
        let reopened = Store::open(db).unwrap();
        assert_eq!(
            reopened.safe_settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
            "ASK"
        );
    }

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
    async fn native_backup_policy_round_trips_through_shared_manager() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("stream-archive.db")).unwrap();
        let core = StreamArchiveCore::assemble(backend, store).unwrap();

        let policy = BackupPolicy {
            enabled: false,
            interval_hours: 12,
            keep_count: 4,
            retention_days: 7,
        };
        let saved = core.update_backup_policy(&policy, None).await.unwrap();
        assert_eq!(saved.policy, policy);
        let loaded = core.backup_snapshot().await.unwrap();
        assert_eq!(loaded.policy, policy);
    }

    #[tokio::test]
    async fn native_storage_snapshot_uses_canonical_settings_and_store() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("stream-archive.db")).unwrap();
        let core = StreamArchiveCore::assemble(backend, store).unwrap();

        core.store
            .sync_settings(
                &BTreeMap::from([
                    (
                        "OUTPUT_DIR".into(),
                        dir.path().join("live").display().to_string(),
                    ),
                    ("MIN_FREE_SPACE_GB".into(), "3.5".into()),
                ]),
                "test",
            )
            .unwrap();

        let snapshot = core.storage_snapshot().unwrap();
        assert_eq!(snapshot.threshold_gb, 3.5);
        assert!(!snapshot.volumes.is_empty());
        assert!(
            snapshot
                .volumes
                .iter()
                .any(|volume| volume.roles.iter().any(|role| role == "LIVE 기본"))
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
    async fn notification_setting_defaults_enabled_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let core =
            StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db.clone()).unwrap())
                .unwrap();
        assert_eq!(
            core.settings().unwrap()["STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"],
            "true"
        );
        core.update_environment_settings(&BTreeMap::from([(
            "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS".into(),
            "false".into(),
        )]))
        .await
        .unwrap();
        drop(core);
        let reopened = Store::open(db).unwrap();
        assert_eq!(
            reopened.safe_settings().unwrap()["STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"],
            "false"
        );
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
