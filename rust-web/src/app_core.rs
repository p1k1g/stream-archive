//! Shared application/service boundary for non-HTTP frontends.
//!
//! Phase 21 introduces this facade so the current Axum server, the Unix CLI,
//! and the upcoming Slint desktop UI can converge on the same Rust runtime
//! instead of duplicating SQLite, watcher, VOD, security, and lifecycle logic.
//! Browser authentication and HTTP concerns deliberately stay outside this
//! module.

use crate::{
    backend::LogBuffer,
    model::{Channel, NativeWatcherStatus, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus},
    native_watcher::NativeWatcherManager,
    primary_config::{
        apply_vod_tool_defaults, validate_channels, validate_secret_updates,
        validate_setting_updates, validate_vod_tool_updates,
    },
    security::protect_secret,
    store::{self, Store},
    vod::VodManager,
};
use anyhow::Result;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct StreamArchiveCore {
    backend_dir: Arc<PathBuf>,
    store: Store,
    logs: LogBuffer,
    watcher: Arc<NativeWatcherManager>,
    vod: Arc<VodManager>,
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
            core: Self::assemble(backend_dir, store),
            migrated_legacy_db,
        })
    }

    fn assemble(backend_dir: PathBuf, store: Store) -> Self {
        let logs = LogBuffer::new();
        let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
        let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
        Self {
            backend_dir: Arc::new(backend_dir),
            store,
            logs,
            watcher,
            vod,
            config_write_lock: Arc::new(Mutex::new(())),
            lifecycle_lock: Arc::new(Mutex::new(())),
        }
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

    /// Compatibility hook for queue/backup adapters while they are moved
    /// behind this service boundary in later Phase 21 slices.
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

    pub fn channels(&self) -> Result<Vec<Channel>> {
        self.store.channels()
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

    /// Stop only runtime children owned by Stream Archive.
    pub async fn shutdown(&self) {
        let _ = self.vod.cancel().await;
        let _ = self.watcher.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembled_core_keeps_one_canonical_store_and_backend() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = dir
            .path()
            .join("app")
            .join("data")
            .join("stream-archive.db");
        let store = Store::open(db.clone()).unwrap();
        let core = StreamArchiveCore::assemble(backend.clone(), store);

        assert_eq!(core.backend_dir(), backend.as_path());
        assert_eq!(core.store().path(), db.as_path());
        assert!(core.settings().unwrap().contains_key("STREAMLINK_PATH"));
    }
    #[tokio::test]
    async fn environment_patch_is_atomic_and_uses_existing_web_keys() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let core =
            StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db.clone()).unwrap());
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
}
