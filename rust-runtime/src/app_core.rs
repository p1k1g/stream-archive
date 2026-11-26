//! Shared application/service boundary for non-HTTP frontends.
//!
//! Canonical presentation-neutral application facade shared by the Native UI,
//! Unix CLI and compatible headless runtime. It centralizes SQLite, watcher,
//! VOD, Queue, History, security, backup and lifecycle logic. Presentation
//! concerns deliberately stay outside this module.

use crate::{
    backend::LogBuffer,
    background_tasks::{BackgroundTasks, Kind, stopped},
    backup_service::{BackupManager, BackupPolicy, BackupSnapshot, RestoreOutcome},
    history_service::{HistoryFilter, load_history},
    model::{
        Channel, HistoryResponse, NativeWatcherStatus, VodAnalyzeRequest, VodBatchEnqueueResult,
        VodDownloadRequest, VodJobStatus, VodQueueItem, VodQueueSnapshot,
    },
    native_watcher::NativeWatcherManager,
    primary_config::{
        apply_vod_tool_defaults, validate_channels, validate_secret_updates,
        validate_setting_updates,
    },
    queue_service::VodQueueManager,
    remux_service::RemuxManager,
    runtime_owner::{RuntimeOwnerGuard, runtime_owner_active},
    security::read_provider_settings,
    store::{self, Store},
    support::{
        platform::{PlatformId, live::LiveSession},
        resolve_channel_name_for,
    },
    vod::VodManager,
    webhook_service::{WebhookDispatcher, WebhookEvent},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

const WINDOW_BOUNDS_SETTING_KEY: &str = "STREAM_ARCHIVE_WINDOW_BOUNDS";

const NATIVE_PROVIDER_SETTING_KEYS: &[&str] =
    &["SOOP_USERNAME", "CLOUDFLARE_WORKER_URL", "TELEGRAM_CHAT_ID"];
const NATIVE_PROVIDER_SECRET_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
    "KICK_SESSION_TOKEN",
    "DISCORD_WEBHOOK_URL",
    "TELEGRAM_BOT_TOKEN",
];

#[derive(Clone)]
pub struct StreamArchiveCore {
    background_tasks: Arc<BackgroundTasks>,
    backend_dir: Arc<PathBuf>,
    store: Store,
    logs: LogBuffer,
    resource_trace: crate::resource_trace::ResourceTrace,
    remux: Arc<RemuxManager>,
    webhooks: WebhookDispatcher,
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

fn validate_window_bounds_spec(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n']) {
        bail!("window bounds must be a single line of at most 128 bytes");
    }
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 5 {
        bail!("window bounds must contain x,y,width,height,maximized");
    }
    let _x: i32 = parts[0].parse().context("invalid window x coordinate")?;
    let _y: i32 = parts[1].parse().context("invalid window y coordinate")?;
    let width: u32 = parts[2].parse().context("invalid window width")?;
    let height: u32 = parts[3].parse().context("invalid window height")?;
    if !(1000..=7680).contains(&width) || !(650..=4320).contains(&height) {
        bail!("window dimensions out of bounds");
    }
    if !matches!(parts[4], "0" | "1") {
        bail!("window maximized flag must be 0 or 1");
    }
    Ok(())
}

const KICK_CLEANUP_KEY: &str = "STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS";

fn cleanup_kick_secrets(store: &Store, delete: &mut impl FnMut(&str) -> Result<()>) -> Result<()> {
    for reference in store.pending_secret_cleanup(KICK_CLEANUP_KEY)? {
        delete(&reference).context(
            "KICK 이전 인증정보 정리가 필요합니다. 저장된 정리 대기 참조로 재시도하세요.",
        )?;
        store.finish_secret_cleanup(KICK_CLEANUP_KEY, &reference)?;
    }
    Ok(())
}

fn rollback_provider_configuration(
    store: &Store,
    updates: &BTreeMap<String, String>,
    journals: &[(String, String)],
    delete: &mut impl FnMut(&str, &str) -> Result<()>,
) -> Result<()> {
    let mut first_error = None;
    for (key, cleanup_key) in journals {
        if let Some(value) = updates.get(key) {
            let result: Result<()> = (|| {
                delete(key, value)?;
                if let Some(reference) = crate::security::native_cleanup_reference(value, key)? {
                    store.finish_secret_cleanup(cleanup_key, &reference)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error).context("저장 실패 후 새 native credential 정리가 필요합니다."),
        None => Ok(()),
    }
}

fn save_provider_configuration(
    store: &Store,
    settings: &BTreeMap<String, String>,
    secrets: &BTreeMap<String, String>,
    mut protect: impl FnMut(&str, &str, &str) -> Result<String>,
    delete: &mut impl FnMut(&str, &str) -> Result<()>,
) -> Result<()> {
    let mut updates = settings.clone();
    let mut journals = Vec::new();
    let preparation = (|| -> Result<()> {
        for (key, value) in secrets.iter().filter(|(_, value)| !value.is_empty()) {
            let cleanup_key = if key == "KICK_SESSION_TOKEN" {
                KICK_CLEANUP_KEY.to_string()
            } else {
                format!("STREAM_ARCHIVE_{key}_CLEANUP_REFS")
            };
            cleanup_provider_secrets(store, key, &cleanup_key, &mut |reference| {
                delete(key, reference)
            })?;
            let protected = protect(key, value, &cleanup_key)?;
            updates.insert(key.clone(), protected);
            journals.push((key.clone(), cleanup_key));
        }
        Ok(())
    })();
    if let Err(error) = preparation {
        rollback_provider_configuration(store, &updates, &journals, delete)
            .with_context(|| format!("provider secret preparation failed: {error:#}"))?;
        return Err(error);
    }
    if !updates.is_empty() {
        commit_provider_configuration(store, &updates, &journals, delete)?;
    }
    Ok(())
}

fn commit_provider_configuration(
    store: &Store,
    updates: &BTreeMap<String, String>,
    journals: &[(String, String)],
    delete: &mut impl FnMut(&str, &str) -> Result<()>,
) -> Result<()> {
    if let Err(error) =
        store.sync_settings_retiring_secrets(updates, "native-provider", journals, |key, value| {
            crate::security::native_cleanup_reference(value, key)
        })
    {
        rollback_provider_configuration(store, updates, journals, delete)
            .with_context(|| format!("provider SQLite commit failed: {error:#}"))?;
        return Err(error);
    }
    for (key, cleanup_key) in journals {
        cleanup_provider_secrets(store, key, cleanup_key, &mut |value| delete(key, value))
            .context("새 인증정보는 저장됐지만 이전 인증정보 정리가 완료되지 않았습니다. 삭제 또는 저장을 다시 시도하세요.")?;
    }
    Ok(())
}

#[cfg(test)]
fn commit_kick_configuration(
    store: &Store,
    updates: &BTreeMap<String, String>,
    delete: &mut impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    commit_provider_configuration(
        store,
        updates,
        &[("KICK_SESSION_TOKEN".into(), KICK_CLEANUP_KEY.into())],
        &mut |_, value| delete(value),
    )
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

fn cleanup_provider_secrets(
    store: &Store,
    key: &str,
    cleanup_key: &str,
    delete: &mut impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    for reference in store.pending_secret_cleanup(cleanup_key)? {
        if store.setting_value(key)?.as_deref() == Some(&reference) {
            continue;
        }
        delete(&reference)?;
        store.finish_secret_cleanup(cleanup_key, &reference)?;
    }
    Ok(())
}

fn clear_provider_secrets_in_store(
    store: &Store,
    keys: &[&str],
    mut delete: impl FnMut(&str, &str) -> Result<()>,
) -> Result<()> {
    let values = keys
        .iter()
        .map(|key| ((*key).into(), String::new()))
        .collect();
    let journals: Vec<_> = keys
        .iter()
        .map(|key| {
            (
                key.to_string(),
                format!("STREAM_ARCHIVE_{key}_CLEANUP_REFS"),
            )
        })
        .collect();
    // Clear the entire credential set before any fallible native cleanup.
    store.sync_settings_retiring_secrets(&values, "native-provider", &journals, |key, value| {
        crate::security::native_cleanup_reference(value, key)
    })?;
    for (key, cleanup_key) in journals {
        for reference in store.pending_secret_cleanup(&cleanup_key)? {
            delete(&key, &reference)?;
            store.finish_secret_cleanup(&cleanup_key, &reference)?;
        }
    }
    Ok(())
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
        let data_dir = store.path().parent().unwrap_or(&backend_dir);
        let app_root = data_dir.parent().unwrap_or(data_dir);
        let logs = if recover_queue {
            let settings = store.safe_settings().unwrap_or_default();
            let enabled = settings
                .get("LOG_ENABLED")
                .map(|v| !v.trim().eq_ignore_ascii_case("N"))
                .unwrap_or(true);
            if enabled {
                let raw_dir = settings
                    .get("LOG_DIR")
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                    .unwrap_or("log");
                let cleaned = raw_dir.trim_start_matches(".\\").trim_start_matches("./");
                let log_dir = if cleaned.is_empty()
                    || cleaned.eq_ignore_ascii_case("logs")
                    || cleaned.eq_ignore_ascii_case("log")
                {
                    app_root.join("log")
                } else {
                    let candidate = Path::new(cleaned);
                    if candidate.is_absolute() {
                        candidate.to_path_buf()
                    } else {
                        app_root.join(candidate)
                    }
                };
                let retention_days = settings
                    .get("LOG_RETENTION_DAYS")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(30);
                LogBuffer::with_file_sink(log_dir, retention_days)
            } else {
                LogBuffer::new()
            }
        } else {
            LogBuffer::new()
        };
        let resource_trace = if recover_queue {
            crate::resource_trace::ResourceTrace::from_environment(data_dir)
        } else {
            Default::default()
        };
        let remux = Arc::new(RemuxManager::new(backend_dir.clone(), logs.clone()));
        let webhooks = WebhookDispatcher::new(store.clone(), logs.clone());
        let watcher = Arc::new(NativeWatcherManager::new_with_services(
            backend_dir.clone(),
            logs.clone(),
            resource_trace.clone(),
            remux.clone(),
            Some(webhooks.clone()),
        ));
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
            background_tasks: Arc::new(BackgroundTasks::default()),
            backend_dir: Arc::new(backend_dir),
            store,
            logs,
            resource_trace,
            remux,
            webhooks,
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

    pub fn window_bounds(&self) -> Result<Option<String>> {
        let Some(raw) = self.store.setting_value(WINDOW_BOUNDS_SETTING_KEY)? else {
            return Ok(None);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || validate_window_bounds_spec(trimmed).is_err() {
            return Ok(None);
        }
        Ok(Some(trimmed.to_string()))
    }

    pub fn save_window_bounds(&self, value: &str) -> Result<()> {
        let trimmed = value.trim();
        validate_window_bounds_spec(trimmed)?;
        self.store.sync_settings(
            &BTreeMap::from([(WINDOW_BOUNDS_SETTING_KEY.to_string(), trimmed.to_string())]),
            "native-window",
        )
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
        let _secret_guard = if secrets.values().any(|value| !value.is_empty()) {
            Some(crate::security::kick_secret_guard(&self.store)?)
        } else {
            None
        };
        self.store.refresh_config_cache()?;
        save_provider_configuration(
            &self.store,
            settings,
            secrets,
            |_, value, cleanup_key| {
                crate::security::protect_secret_with_cleanup_intent(value, |reference| {
                    self.store.retain_secret_cleanup(cleanup_key, reference)
                })
            },
            &mut |key, value| crate::security::delete_protected_secret(value, key),
        )?;
        self.store.configured_secrets()
    }

    /// Clear saved login secrets without changing Worker configuration or other providers.
    pub async fn clear_provider_credentials(&self, provider: &str) -> Result<()> {
        let keys: &[&str] = match provider {
            "SOOP" => &["SOOP_PASSWORD"],
            "CHZZK" => &["CHZZK_NID_AUT", "CHZZK_NID_SES"],
            "WEBHOOK" => &["DISCORD_WEBHOOK_URL", "TELEGRAM_BOT_TOKEN"],
            _ => bail!("unsupported credential deletion provider"),
        };
        let _guard = self.config_write_lock.lock().await;
        let _secret_guard = crate::security::kick_secret_guard(&self.store)?;
        clear_provider_secrets_in_store(&self.store, keys, |key, value| {
            crate::security::delete_protected_secret(value, key)
        })
    }

    pub async fn test_webhook(&self) -> Result<String> {
        self.webhooks.send_test().await
    }

    /// Explicit deletion; empty provider drafts continue to preserve saved values.
    pub async fn clear_kick_token(&self) -> Result<()> {
        let _guard = self.config_write_lock.lock().await;
        let _secret_guard = crate::security::kick_secret_guard(&self.store)?;
        let delete =
            |value: &str| crate::security::delete_protected_secret(value, "KICK_SESSION_TOKEN");
        clear_kick_token_in_store(&self.store, delete)
    }

    /// Verify the currently saved SOOP login and Cloudflare Worker credentials.
    /// Secret values are decrypted only inside the shared service boundary and
    /// are never returned to the native frontend or written to logs.
    pub async fn test_soop_auth(&self) -> Result<String> {
        let mut settings = read_provider_settings(
            &self.store,
            &[
                "SOOP_USERNAME",
                "SOOP_PASSWORD",
                "CLOUDFLARE_WORKER_URL",
                "CLOUDFLARE_API_KEY",
            ],
            &["SOOP_PASSWORD", "CLOUDFLARE_API_KEY"],
        )?;
        let username = settings.remove("SOOP_USERNAME").unwrap_or_default();
        let password = settings.remove("SOOP_PASSWORD").unwrap_or_default();
        let worker_url = settings.remove("CLOUDFLARE_WORKER_URL").unwrap_or_default();
        let worker_key = settings.remove("CLOUDFLARE_API_KEY").unwrap_or_default();

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

    pub fn resource_trace(&self) -> crate::resource_trace::ResourceTrace {
        self.resource_trace.clone()
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
                    && row.has_current_live_thumbnail()
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
                if platform != PlatformId::Kick {
                    return Err(error);
                }
                let source =
                    crate::support::platform::kick::live::preview_source(account, broadcast_id)
                        .await
                        .map_err(|_| anyhow::anyhow!("KICK LIVE 썸네일 fallback 조회 실패"))?;
                let settings = self.store.vod_tool_settings()?;
                let configured = settings
                    .get("FFMPEG_PATH")
                    .map(String::as_str)
                    .unwrap_or("");
                let tool = crate::tool_discovery::resolve_tool(
                    crate::tool_discovery::ToolKind::Ffmpeg,
                    self.backend_dir(),
                    &[("FFMPEG_PATH", configured)],
                )
                .path
                .ok_or_else(|| anyhow::anyhow!("KICK LIVE 썸네일 추출에는 FFmpeg가 필요합니다."))?;
                crate::thumbnail_service::load_kick_vod_frame(&tool, &source).await?
            }
        };
        let current = self.watcher_status().await?;
        if !current.running
            || !current.channels.iter().any(|row| {
                row.platform == platform
                    && row.account == account
                    && row.bno.as_deref() == Some(broadcast_id)
                    && row.completion_revision == completion_revision
                    && row.has_current_live_thumbnail()
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
        self.resource_trace.record(
            crate::resource_trace::Event::ChannelsSaved,
            crate::resource_trace::Sample {
                channels: Some(channels.len()),
                ..Default::default()
            },
        );
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
        let image = if status.platform == crate::support::platform::PlatformId::Kick {
            let source = crate::support::platform::vod::thumbnail_frame_source(
                status.platform,
                vod_url,
                url,
            )
            .await?;
            let settings = self.store.vod_tool_settings()?;
            let configured = settings
                .get("FFMPEG_PATH")
                .map(String::as_str)
                .unwrap_or("");
            let tool = crate::tool_discovery::resolve_tool(
                crate::tool_discovery::ToolKind::Ffmpeg,
                self.backend_dir(),
                &[("FFMPEG_PATH", configured)],
            )
            .path
            .ok_or_else(|| anyhow::anyhow!("KICK VOD 썸네일 추출에는 FFmpeg가 필요합니다."))?;
            crate::thumbnail_service::load_kick_vod_frame(&tool, &source).await?
        } else {
            crate::thumbnail_service::load_vod(status.platform, url).await?
        };
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

    pub async fn enqueue_vod_batch(
        &self,
        urls: &[String],
        quality: &str,
        output_directory: &str,
        merge: bool,
    ) -> Result<VodBatchEnqueueResult> {
        let _config_guard = self.config_write_lock.lock().await;
        let tools = self.store.vod_tool_settings()?;
        let mut template = VodDownloadRequest {
            vod_url: String::new(),
            output_directory: output_directory.trim().to_string(),
            parts: Vec::new(),
            quality: if quality.trim().is_empty() {
                "best".into()
            } else {
                quality.trim().to_string()
            },
            merge,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            max_retries: 5,
        };
        apply_vod_tool_defaults(&tools, &mut template.yt_dlp_path, &mut template.ffmpeg_path);
        self.queue.enqueue_batch(urls, template).await
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

    pub async fn clear_completed_queue_items(&self) -> Result<(usize, VodQueueSnapshot)> {
        let _config_guard = self.config_write_lock.lock().await;
        let removed = self.queue.clear_completed().await?;
        Ok((removed, self.queue.snapshot().await?))
    }

    pub async fn retry_failed_queue_items(&self) -> Result<(usize, VodQueueSnapshot)> {
        let _config_guard = self.config_write_lock.lock().await;
        let retried = self.queue.retry_failed().await?;
        Ok((retried, self.queue.snapshot().await?))
    }

    pub fn history(&self, filter: &HistoryFilter) -> Result<HistoryResponse> {
        load_history(self.store.path(), filter)
    }

    pub fn storage_snapshot(&self) -> Result<crate::storage_service::StorageSnapshot> {
        crate::storage_service::snapshot(&self.store)
    }

    pub async fn check_latest_release(&self) -> Result<crate::update_check::ReleaseUpdateInfo> {
        crate::update_check::check_latest_release().await
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
        if self.remux.active_count() > 0 {
            bail!("wait for active LIVE remux jobs to finish before restore");
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

    pub fn logs_dir(&self) -> Option<PathBuf> {
        self.logs.log_dir().map(Path::to_path_buf)
    }

    pub async fn clear_runtime_logs(&self) {
        self.logs.clear().await;
    }

    pub fn spawn_auto_backup(&self) {
        let manager = self.backups.clone();
        let logs = self.logs.clone();
        self.background_tasks.start(Kind::Backup, move |stop| {
            crate::backup_service::run_auto_backup(manager, logs, stop)
        });
    }

    /// Keep VOD history synchronized and dispatch external VOD webhooks for all runtimes.
    pub fn spawn_vod_history_sync(&self) {
        let store = self.store.clone();
        let vod = self.vod.clone();
        let logs = self.logs.clone();
        let webhooks = self.webhooks.clone();
        self.background_tasks
            .start(Kind::VodHistory, move |mut stop| async move {
                #[derive(PartialEq, Eq)]
                struct VodSyncKey {
                    job_id: Option<String>,
                    state: String,
                    running: bool,
                    part_count: usize,
                    output_file: Option<String>,
                    message: String,
                    finished_at: Option<String>,
                }
                let mut downloads = vod.subscribe_download_events();
                let mut last_synced: Option<VodSyncKey> = None;
                loop {
                    tokio::select! {
                        biased;
                        _ = stopped(&mut stop) => break,
                        event = downloads.recv() => {
                            if let Ok(event) = event {
                                let status = vod.status().await;
                                let (title, detail) = if status.job_id.as_deref() == Some(event.job_id.as_str()) {
                                    (
                                        status
                                            .analysis
                                            .as_ref()
                                            .map(|a| a.title.clone())
                                            .unwrap_or_default(),
                                        status.message.clone(),
                                    )
                                } else {
                                    (String::new(), String::new())
                                };
                                webhooks.notify(WebhookEvent::VodFinished {
                                    platform: event.platform,
                                    title,
                                    completed: event.completed,
                                    detail,
                                });
                            }
                            continue;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                    let status = vod.status().await;
                    let key = VodSyncKey {
                        job_id: status.job_id.clone(),
                        state: status.state.clone(),
                        running: status.running,
                        part_count: status.part_count,
                        output_file: status.output_file.clone(),
                        message: status.message.clone(),
                        finished_at: status.finished_at.clone(),
                    };
                    if status.job_id.is_none() || last_synced.as_ref() == Some(&key) {
                        continue;
                    }
                    if let Err(err) = store.upsert_vod(&status) {
                        logs.push(format!("[DB:WARN] VOD history sync failed: {err:#}"))
                            .await;
                    } else {
                        last_synced = Some(key);
                    }
                }
            });
    }

    /// Stop new Queue claims first, then only runtime children owned by Stream Archive.
    pub async fn shutdown(&self) {
        self.background_tasks.request_stop();
        self.queue.shutdown().await;
        let _ = self.vod.cancel().await;
        let _ = self.watcher.stop().await;
        self.remux.shutdown().await;
        self.background_tasks.shutdown().await;
        // Persist the terminal result after the last periodic writer has joined.
        if let Err(error) = self.store.upsert_vod(&self.vod.status().await) {
            self.logs
                .push(format!(
                    "[DB:WARN] final VOD history sync failed: {error:#}"
                ))
                .await;
        }
    }
}

#[cfg(test)]
#[path = "app_core/tests.rs"]
mod core_tests;

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
}
