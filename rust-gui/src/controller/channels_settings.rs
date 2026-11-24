//! Channel management, provider credentials, environment settings, backup, and log actions.

use super::{AppController, ProviderSaveInput};
use crate::{
    core_worker::{Request, localized_setting_description},
    maintenance_adapter,
    state::{
        ChannelConfigRow, MaintenanceBackupRow, MaintenanceDiagnosticRow, MaintenanceLogRow,
        SettingRow,
    },
};
use std::collections::BTreeMap;
use stream_archive_server::{
    backup_service::{BackupPolicy, BackupSnapshot},
    diagnostics::DiagnosticsSnapshot,
    environment_settings::SettingKind,
};

impl AppController {
    // ---- Channels actions ----

    pub fn set_channels_search(&mut self, query: String) {
        self.state.channels_search = query;
    }

    pub fn set_channels_platform_filter(&mut self, platform: &str) {
        self.state.channels_platform_filter = match platform {
            "SOOP" | "CHZZK" | "KICK" => platform.to_string(),
            _ => "ALL".to_string(),
        };
    }

    pub fn channels_add(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.channels_draft.add();
        if matches!(
            self.state.channels_platform_filter.as_str(),
            "SOOP" | "CHZZK" | "KICK"
        ) {
            let new_idx = self.channels_draft.rows.len().saturating_sub(1);
            let plat = self.state.channels_platform_filter.clone();
            self.channels_draft.select_platform(new_idx, &plat);
        }
        self.state.channels_search.clear();
        self.sync_channels_view(true);
    }

    pub fn channels_remove(&mut self, index: usize) {
        if self.state.config_busy {
            return;
        }
        self.channels_draft.remove(index);
        self.sync_channels_view(true);
    }

    pub fn channels_edit(&mut self, index: usize, field: &str, value: String) {
        if self.state.config_busy {
            return;
        }
        self.channels_draft.edit(index, field, value);
        self.sync_channels_view(false);
    }

    pub fn channels_set_enabled(&mut self, index: usize, enabled: bool) {
        if self.state.config_busy {
            return;
        }
        self.channels_draft.set_enabled(index, enabled);
        self.sync_channels_view(false);
    }

    pub fn channels_select_platform(&mut self, index: usize, platform: &str) {
        if self.state.config_busy {
            return;
        }
        self.channels_draft.select_platform(index, platform);
        self.sync_channels_view(false);
    }

    pub fn channels_pick_output(&mut self, index: usize) {
        if self.state.config_busy || index >= self.channels_draft.rows.len() {
            return;
        }
        let expected = self.channels_draft.snapshot();
        let initial = expected[index].outdir.clone();
        let _ = self.sender.send(Request::ChannelPickOutput {
            index,
            expected,
            initial,
        });
    }

    pub fn channels_resolve(&mut self, index: usize) {
        if self.state.config_busy || index >= self.channels_draft.rows.len() {
            return;
        }
        let row = &self.channels_draft.rows[index];
        if row.account.trim().is_empty() {
            self.state.config_message = "채널 ID / 계정을 먼저 입력하세요.".into();
            return;
        }
        let _ = self.sender.send(Request::ChannelResolve {
            index,
            platform: row.platform,
            account: row.account.clone(),
        });
    }

    pub fn channels_save(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        self.state.config_message = "채널 목록을 저장하는 중...".into();
        let _ = self
            .sender
            .send(Request::ChannelsSave(self.channels_draft.snapshot()));
    }

    pub fn reload_config(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        self.state.config_message = "채널 및 서비스 연결 정보를 다시 불러오는 중...".into();
        let _ = self.sender.send(Request::ConfigReload);
    }

    // ---- Provider credentials ----

    pub fn provider_save(&mut self, input: ProviderSaveInput) {
        if self.state.config_busy {
            return;
        }
        let mut secrets = BTreeMap::new();
        if !input.soop_password.trim().is_empty() {
            secrets.insert("SOOP_PASSWORD".into(), input.soop_password);
        }
        if !input.worker_api_key.trim().is_empty() {
            secrets.insert("CLOUDFLARE_API_KEY".into(), input.worker_api_key);
        }
        if !input.chzzk_nid_aut.trim().is_empty() {
            secrets.insert("CHZZK_NID_AUT".into(), input.chzzk_nid_aut);
        }
        if !input.chzzk_nid_ses.trim().is_empty() {
            secrets.insert("CHZZK_NID_SES".into(), input.chzzk_nid_ses);
        }
        if !input.kick_token.trim().is_empty() {
            secrets.insert("KICK_SESSION_TOKEN".into(), input.kick_token);
        }
        self.state.config_busy = true;
        self.state.config_message = "서비스 연결 정보를 저장하는 중...".into();
        let _ = self.sender.send(Request::ProviderSave {
            username: input.username,
            worker_url: input.worker_url,
            secrets,
        });
    }

    pub fn provider_test_soop(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_message = "SOOP / Worker 연결을 테스트하는 중...".into();
        let _ = self.sender.send(Request::ProviderTestSoop);
    }

    pub fn provider_clear(&mut self, provider: &str) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        if provider == "KICK" {
            let _ = self.sender.send(Request::ClearKickToken);
        } else {
            let _ = self
                .sender
                .send(Request::ClearProviderCredentials(provider.to_string()));
        }
    }

    // ---- General Settings actions ----

    pub fn settings_edit(&mut self, index: usize, value: String) {
        if self.state.settings_busy {
            return;
        }
        self.settings_draft.edit(index, value);
        self.sync_settings_view(false);
    }

    pub fn settings_set_key(&mut self, key: &str, value: String) {
        if self.state.settings_busy {
            return;
        }
        if let Some(index) = self
            .settings_draft
            .fields
            .iter()
            .position(|field| field.key == key)
        {
            self.settings_draft.edit(index, value);
            self.sync_settings_view(false);
        }
    }

    pub fn settings_pick(&mut self, index: usize) {
        if self.state.settings_busy {
            return;
        }
        let Some(field) = self.settings_draft.fields.get(index) else {
            return;
        };
        if field.kind == SettingKind::Text {
            return;
        }
        let _ = self
            .sender
            .send(Request::Pick(index, field.kind, field.value.clone()));
    }

    pub fn settings_save(&mut self) {
        if self.state.settings_busy {
            return;
        }
        let patch = self.settings_draft.patch();
        if patch.is_empty() {
            self.state.settings_message = "변경된 설정이 없습니다.".into();
            return;
        }
        self.state.settings_busy = true;
        self.state.settings_message = "설정을 저장하는 중...".into();
        let _ = self.sender.send(Request::Save(patch));
    }

    pub fn settings_reload(&mut self) {
        if self.state.settings_busy {
            return;
        }
        self.state.settings_busy = true;
        self.state.settings_message = "저장된 설정을 다시 불러오는 중...".into();
        let _ = self.sender.send(Request::Reload);
    }

    pub fn refresh_requested(&mut self) {
        if self.state.settings_busy {
            return;
        }
        self.state.settings_busy = true;
        let _ = self.sender.send(Request::Refresh);
    }

    // ---- Maintenance / Backup / Logs actions ----

    pub fn refresh_maintenance(&mut self, active_tools: bool) {
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = if active_tools {
            "로컬 도구 버전 및 관리 상태를 확인하는 중...".into()
        } else {
            "관리 상태를 새로고침하는 중...".into()
        };
        let _ = self.sender.send(Request::MaintenanceLoad { active_tools });
    }

    pub fn backup_pick_directory(&mut self) {
        if self.state.backup_directory_locked || self.state.maintenance_busy {
            return;
        }
        let _ = self.sender.send(Request::BackupPickDirectory {
            initial: self.state.backup_directory.clone(),
        });
    }

    pub fn backup_save_policy(
        &mut self,
        enabled: bool,
        interval_hours: String,
        keep_count: String,
        retention_days: String,
        directory: String,
    ) {
        if self.state.maintenance_busy {
            return;
        }
        let Ok(interval_hours) = interval_hours.trim().parse::<u64>() else {
            self.state.maintenance_message = "백업 간격(시간)은 1 이상의 정수여야 합니다.".into();
            return;
        };
        let Ok(keep_count) = keep_count.trim().parse::<usize>() else {
            self.state.maintenance_message = "보관 개수는 1 이상의 정수여야 합니다.".into();
            return;
        };
        let Ok(retention_days) = retention_days.trim().parse::<i64>() else {
            self.state.maintenance_message = "보관 일수는 1 이상의 정수여야 합니다.".into();
            return;
        };
        let policy = BackupPolicy {
            enabled,
            interval_hours,
            keep_count,
            retention_days,
        };
        let dir_opt = (!self.state.backup_directory_locked).then_some(directory);
        self.state.maintenance_busy = true;
        self.state.maintenance_message = "백업 정책을 저장하는 중...".into();
        let _ = self.sender.send(Request::BackupSave {
            policy,
            directory: dir_opt,
        });
    }

    pub fn backup_create(&mut self) {
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = "수동 백업을 생성하는 중...".into();
        let _ = self.sender.send(Request::BackupCreate);
    }

    pub fn request_restore(&mut self, file_name: String) {
        self.state.restore_confirm_file = Some(file_name);
    }

    pub fn cancel_restore(&mut self) {
        self.state.restore_confirm_file = None;
    }

    pub fn confirm_restore(&mut self) {
        let Some(file_name) = self.state.restore_confirm_file.take() else {
            return;
        };
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = format!("백업 복원 중: {file_name}");
        let _ = self.sender.send(Request::BackupRestore { file_name });
    }

    pub fn refresh_logs(&mut self) {
        let _ = self.sender.send(Request::LogsLoad { poll: false });
    }

    pub fn toggle_log_auto_refresh(&mut self) {
        self.state.log_auto_refresh = !self.state.log_auto_refresh;
    }

    pub fn open_logs_folder(&mut self) {
        let _ = self.sender.send(Request::OpenLogsFolder);
    }

    pub fn clear_logs(&mut self) {
        let _ = self.sender.send(Request::LogsClear);
    }

    pub fn set_windows_startup(&mut self, enabled: bool) {
        if self.state.windows_startup_busy {
            return;
        }
        self.state.windows_startup_busy = true;
        let _ = self.sender.send(Request::SetWindowsStartup(enabled));
    }

    pub fn check_updates_manual(&mut self) {
        if self.state.update_checking {
            return;
        }
        self.state.update_checking = true;
        self.state.update_status_message = "GitHub 최신 릴리스를 확인하는 중...".into();
        let _ = self.sender.send(Request::CheckUpdates { manual: true });
    }

    pub fn open_release_url(&mut self) {
        let url = if self.state.update_release_url.trim().is_empty() {
            stream_archive_server::update_check::RELEASES_PAGE_URL.to_string()
        } else {
            self.state.update_release_url.clone()
        };
        let _ = self.sender.send(Request::OpenReleaseUrl(url));
    }

    // ---- View synchronization helpers ----

    pub(super) fn sync_settings_view(&mut self, bump_revision: bool) {
        self.state.settings_rows = self
            .settings_draft
            .fields
            .iter()
            .map(|field| SettingRow {
                key: field.key.clone(),
                value: field.value.clone(),
                description: localized_setting_description(&field.key, &field.description).into(),
                pickable: field.kind != SettingKind::Text,
            })
            .collect();
        self.state.settings_dirty = !self.settings_draft.patch().is_empty();
        if bump_revision {
            self.state.settings_revision = self.state.settings_revision.wrapping_add(1);
        }
    }

    pub(super) fn sync_channels_view(&mut self, bump_revision: bool) {
        self.state.channels_rows = self
            .channels_draft
            .rows
            .iter()
            .map(|row| ChannelConfigRow {
                platform: row.platform.to_string(),
                enabled: row.enabled,
                name: row.name.clone(),
                account: row.account.clone(),
                outdir: row.outdir.clone(),
            })
            .collect();
        self.state.channels_dirty = self.channels_draft.dirty();
        if bump_revision {
            self.state.channels_revision = self.state.channels_revision.wrapping_add(1);
        }
    }

    pub(super) fn render_logs(&mut self, lines: Vec<String>) {
        self.state.log_rows = maintenance_adapter::log_rows(lines, 200)
            .into_iter()
            .map(|row| MaintenanceLogRow { text: row.text })
            .collect();
    }

    pub(super) fn render_maintenance(
        &mut self,
        snapshot: BackupSnapshot,
        diagnostics: DiagnosticsSnapshot,
        logs: Vec<String>,
    ) {
        self.state.backup_enabled = snapshot.policy.enabled;
        self.state.backup_interval_hours = snapshot.policy.interval_hours.to_string();
        self.state.backup_keep_count = snapshot.policy.keep_count.to_string();
        self.state.backup_retention_days = snapshot.policy.retention_days.to_string();
        self.state.backup_directory = snapshot.directory.clone();
        self.state.backup_directory_locked = !snapshot.directory_editable;
        self.state.backup_revision = self.state.backup_revision.wrapping_add(1);
        self.state.backup_dir_revision = self.state.backup_dir_revision.wrapping_add(1);
        self.state.backup_rows = maintenance_adapter::backup_rows(&snapshot)
            .into_iter()
            .map(|row| MaintenanceBackupRow {
                file_name: row.file_name,
                kind: row.kind,
                created_at: row.created_at,
                size: row.size,
                sha256: row.sha256,
                integrity: row.integrity,
                integrity_tone: row.integrity_tone,
                can_restore: row.can_restore,
            })
            .collect();
        self.state.maintenance_diagnostic_rows = maintenance_adapter::diagnostic_rows(&diagnostics)
            .into_iter()
            .map(|row| MaintenanceDiagnosticRow {
                category: row.category,
                requirement: row.requirement,
                name: row.name,
                status: row.status,
                summary: row.summary,
                detail: row.detail,
                remediation: row.remediation,
                status_tone: row.status_tone,
            })
            .collect();
        self.render_logs(logs);
    }
}
