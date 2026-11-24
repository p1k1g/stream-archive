//! Presentation state & controller smoke tests for the GPUI desktop shell.
//! No database, provider credentials, or live desktop session is required.

use crate::{
    controller::{AppController, DesktopSignal, bind_core_snapshot},
    core_worker::Request,
    state::{MaintenanceSection, Page, QueueDisplayRow, SettingsView},
};
use stream_archive_server::{
    diagnostics::{
        DiagnosticCategory, DiagnosticItem, DiagnosticRequirement, DiagnosticStatus,
        DiagnosticsSnapshot, DiagnosticsSummary,
    },
    model::{VodQueueItem, VodQueueSnapshot},
    support::platform::PlatformId,
};

#[test]
fn native_navigation_and_watcher_toggle_preserve_input_guards() {
    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    ctrl.state.runtime_ready = true;
    ctrl.state.live_loaded = true;
    ctrl.state.live_busy = false;
    ctrl.state.desktop_recovery_message =
        "Windows 트레이를 복구하지 못해 창을 다시 표시했습니다.".into();

    // Recovery banner must not block navigation, and remains dismissible.
    ctrl.select_page(Page::Channels);
    assert_eq!(ctrl.state.active_page, Page::Channels);
    assert!(!ctrl.state.desktop_recovery_message.is_empty());
    ctrl.dismiss_recovery_message();
    assert!(ctrl.state.desktop_recovery_message.is_empty());

    // LIVE watcher start/stop honor busy and runtime_ready guards.
    ctrl.select_page(Page::Live);
    let _ = rx.try_iter().count();
    ctrl.live_start();
    assert!(ctrl.state.live_busy);
    assert!(matches!(rx.try_recv(), Ok(Request::LiveStart)));
    // Busy guard suppresses duplicate dispatches.
    ctrl.live_start();
    assert!(rx.try_recv().is_err());

    ctrl.state.live_busy = false;
    ctrl.state.live_running = true;
    ctrl.live_stop();
    assert!(ctrl.state.live_busy);
    assert!(matches!(rx.try_recv(), Ok(Request::LiveStop)));

    // Diagnostics & Settings internal section boundaries.
    ctrl.state.maintenance_busy = false;
    ctrl.state.maintenance_loaded = true;
    ctrl.select_page(Page::Diagnostics);
    assert_eq!(ctrl.state.active_page, Page::Diagnostics);
    assert_eq!(
        ctrl.state.maintenance_section,
        MaintenanceSection::Diagnostics
    );
    ctrl.set_maintenance_section(MaintenanceSection::Logs);
    assert_eq!(ctrl.state.maintenance_section, MaintenanceSection::Logs);

    ctrl.select_page(Page::Settings);
    assert_eq!(ctrl.state.active_page, Page::Settings);
    assert_eq!(ctrl.state.settings_view, SettingsView::General);
    ctrl.set_settings_view(SettingsView::Manage);
    assert_eq!(ctrl.state.settings_view, SettingsView::Manage);
    assert_eq!(ctrl.state.maintenance_section, MaintenanceSection::Backup);

    // Every page is selectable cleanly.
    for page in Page::ALL {
        ctrl.select_page(page);
        assert_eq!(ctrl.state.active_page, page);
    }
}

#[test]
fn queue_expanded_row_survives_polling_replacement_by_id() {
    let (mut ctrl, _rx) = AppController::new_disconnected_for_test();
    ctrl.state.queue_busy = false;
    ctrl.state.queue_loaded = true;
    ctrl.state.queue_rows = vec![QueueDisplayRow {
        id: "polling-task".into(),
        title: "Queue task".into(),
        platform: "SOOP".into(),
        ..Default::default()
    }];

    ctrl.queue_toggle_expand("polling-task");
    assert_eq!(ctrl.state.expanded_queue_id, "polling-task");

    // Simulate a polling snapshot replacing the entire queue row vector with the same task ID.
    let snapshot = VodQueueSnapshot {
        items: vec![VodQueueItem {
            id: "polling-task".into(),
            title: "Queue task".into(),
            platform: PlatformId::Soop,
            state: "QUEUED".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (mut ctrl2, _rx2) = AppController::new_disconnected_for_test();
    ctrl2.state.expanded_queue_id = "polling-task".into();
    ctrl2.apply_queue_snapshot_for_test(snapshot);
    assert_eq!(ctrl2.state.expanded_queue_id, "polling-task");

    // Toggling again collapses it rather than re-opening.
    ctrl2.queue_toggle_expand("polling-task");
    assert!(ctrl2.state.expanded_queue_id.is_empty());

    // When the expanded task disappears from the snapshot, expanded_queue_id clears.
    ctrl2.queue_toggle_expand("polling-task");
    ctrl2.apply_queue_snapshot_for_test(VodQueueSnapshot::default());
    assert!(ctrl2.state.expanded_queue_id.is_empty());
}

#[test]
fn close_action_and_dialog_enforce_guards_and_signals() {
    let (mut ctrl, rx) = AppController::new_disconnected_for_test();

    // Blocked while initial settings load is still in progress.
    ctrl.state.settings_loaded = false;
    ctrl.state.settings_busy = true;
    assert!(!ctrl.handle_window_close_requested());
    assert!(ctrl.take_desktop_signals().is_empty());
    assert!(!ctrl.state.close_dialog_visible);

    // Loaded with TRAY action emits HideToTray signal cleanly.
    ctrl.state.settings_loaded = true;
    ctrl.state.settings_busy = false;
    ctrl.state.close_action = "TRAY".into();
    assert!(!ctrl.handle_window_close_requested());
    assert_eq!(ctrl.take_desktop_signals(), vec![DesktopSignal::HideToTray]);

    // ASK opens in-window dialog; cancel dismisses; remember dispatches RememberClose.
    ctrl.state.close_action = "ASK".into();
    assert!(!ctrl.handle_window_close_requested());
    assert!(ctrl.state.close_dialog_visible);
    ctrl.cancel_close_dialog();
    assert!(!ctrl.state.close_dialog_visible);

    assert!(!ctrl.handle_window_close_requested());
    ctrl.submit_close_choice("TRAY", true);
    assert!(ctrl.state.close_choice_busy);
    assert!(matches!(rx.try_recv(), Ok(Request::RememberClose(ref a)) if a == "TRAY"));

    // EXIT dispatches PrepareExit without quitting prematurely.
    ctrl.state.close_choice_busy = false;
    ctrl.state.close_dialog_visible = false;
    ctrl.state.close_action = "EXIT".into();
    assert!(!ctrl.handle_window_close_requested());
    assert!(ctrl.state.desktop_exit_pending);
    assert!(matches!(rx.try_recv(), Ok(Request::PrepareExit)));
}

#[test]
fn password_prompt_and_core_snapshot_binding_work_cleanly() {
    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    ctrl.state.live_busy = false;

    ctrl.open_password_prompt("SOOP:secret".into(), "비방 채널".into());
    assert_eq!(
        ctrl.state.password_prompt_target.as_deref(),
        Some("SOOP:secret")
    );
    ctrl.submit_live_password("1234".into());
    assert!(ctrl.state.password_prompt_target.is_none());
    assert!(ctrl.state.live_busy);
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::LivePassword { ref target, ref password }) if target == "SOOP:secret" && password == "1234"
    ));

    let diagnostics = DiagnosticsSnapshot {
        status: DiagnosticStatus::Ok,
        runtime_ready: true,
        attention_required: false,
        summary: DiagnosticsSummary {
            ok: 1,
            ..Default::default()
        },
        items: vec![DiagnosticItem {
            id: "database.integrity".into(),
            category: DiagnosticCategory::Database,
            requirement: DiagnosticRequirement::Required,
            name: "SQLite".into(),
            status: DiagnosticStatus::Ok,
            summary: "정상".into(),
            detail: "WAL mode".into(),
            remediation: String::new(),
        }],
    };
    bind_core_snapshot(&mut ctrl.state, diagnostics);
    assert_eq!(ctrl.state.diagnostics_rows.len(), 1);
    assert_eq!(ctrl.state.diagnostics_rows[0].name, "SQLite");
}

#[test]
fn backup_directory_pick_preserves_policy_revision_and_history_queues_busy_filter_changes() {
    use crate::core_worker::Response;
    use stream_archive_server::model::HistoryResponse;

    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    let initial_policy_rev = ctrl.state.backup_revision;
    let initial_dir_rev = ctrl.state.backup_dir_revision;

    // Picking a backup folder updates only backup_dir_revision so edited policy inputs stay intact.
    ctrl.apply_response_for_test(Response::MaintenancePicked(Some(
        "D:/custom-backups".into(),
    )));
    assert_eq!(ctrl.state.backup_directory, "D:/custom-backups");
    assert_eq!(ctrl.state.backup_revision, initial_policy_rev);
    assert_ne!(ctrl.state.backup_dir_revision, initial_dir_rev);

    // Changing history filters while an older query is in flight queues a follow-up query with the latest filters.
    ctrl.state.history_busy = false;
    ctrl.history_set_view("LIVE");
    assert!(ctrl.state.history_busy);
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::HistoryLoad { ref view, .. }) if view == "LIVE"
    ));

    // User changes status + view while busy.
    ctrl.history_set_status("FAILED");
    ctrl.history_set_view("VOD");
    assert!(rx.try_recv().is_err());

    // When the stale LIVE response arrives, controller immediately dispatches the queued VOD/FAILED query.
    ctrl.apply_response_for_test(Response::History {
        history: HistoryResponse::default(),
        view: "LIVE".into(),
        message: None,
    });
    assert!(ctrl.state.history_busy);
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::HistoryLoad { ref view, ref filter })
            if view == "VOD" && filter.status.as_deref() == Some("FAILED")
    ));
}

#[test]
fn disconnected_worker_exit_emits_exit_now_and_busy_config_blocks_channel_edits() {
    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    // Drop receiver to simulate a terminated/disconnected core worker after startup failure.
    drop(rx);
    ctrl.begin_exit_check();
    assert_eq!(ctrl.take_desktop_signals(), vec![DesktopSignal::ExitNow]);

    // Channel mutations must be ignored while a save/reload is in flight.
    let (mut ctrl2, _rx2) = AppController::new_disconnected_for_test();
    ctrl2.state.config_busy = false;
    ctrl2.channels_add();
    assert_eq!(ctrl2.state.channels_rows.len(), 1);
    ctrl2.state.config_busy = true;
    ctrl2.channels_add();
    ctrl2.channels_edit(0, "account", "mutated".into());
    ctrl2.channels_select_platform(0, "CHZZK");
    ctrl2.channels_set_enabled(0, false);
    ctrl2.channels_remove(0);
    assert_eq!(ctrl2.state.channels_rows.len(), 1);
    assert!(ctrl2.state.channels_rows[0].account.is_empty());
    assert_eq!(ctrl2.state.channels_rows[0].platform, "SOOP");
    assert!(ctrl2.state.channels_rows[0].enabled);

    // General setting mutations must also be ignored while settings_busy is true.
    use crate::core_worker::Response;
    use stream_archive_server::environment_settings::{EnvironmentSetting, SettingKind};
    ctrl2.apply_response_for_test(Response::Snapshot {
        fields: Some(vec![EnvironmentSetting {
            key: "OUTPUT_DIR".into(),
            value: "C:/archive".into(),
            description: "output".into(),
            kind: SettingKind::Directory,
        }]),
        diagnostics: DiagnosticsSnapshot {
            status: DiagnosticStatus::Ok,
            runtime_ready: true,
            attention_required: false,
            summary: DiagnosticsSummary::default(),
            items: Vec::new(),
        },
        window_bounds: Some("100,80,1280,800,0".into()),
        first_run: false,
        startup_failed: false,
        message: String::new(),
    });
    assert!(!ctrl2.state.settings_busy);
    assert_eq!(
        ctrl2.state.window_bounds_spec.as_deref(),
        Some("100,80,1280,800,0")
    );
    ctrl2.state.settings_busy = true;
    ctrl2.settings_edit(0, "D:/ignored".into());
    ctrl2.settings_set_key("OUTPUT_DIR", "E:/ignored".into());
    assert_eq!(ctrl2.state.settings_rows[0].value, "C:/archive");
    assert!(!ctrl2.state.settings_dirty);
}

#[test]
fn live_search_status_sort_and_open_stream_work_cleanly() {
    use crate::core_worker::Response;
    use stream_archive_server::model::{ChannelRuntimeStatus, NativeWatcherStatus};

    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    ctrl.set_window_visible(true);
    let generation = ctrl.live_model.generation();
    let _ = rx.try_iter().count();

    let status = NativeWatcherStatus {
        running: true,
        engine: "test",
        channel_count: 4,
        recording_count: 1,
        offline_count: 1,
        error_count: 1,
        channels: vec![
            ChannelRuntimeStatus {
                platform: PlatformId::Soop,
                account: "alpha".into(),
                name: "알파".into(),
                status: "OFFLINE".into(),
                ..Default::default()
            },
            ChannelRuntimeStatus {
                platform: PlatformId::Chzzk,
                account: "beta".into(),
                name: "베타".into(),
                status: "ERROR".into(),
                detail: Some("HTTP 500".into()),
                ..Default::default()
            },
            ChannelRuntimeStatus {
                platform: PlatformId::Soop,
                account: "gamma".into(),
                name: "감마".into(),
                status: "PASSWORD_REQUIRED".into(),
                ..Default::default()
            },
            ChannelRuntimeStatus {
                platform: PlatformId::Kick,
                account: "delta".into(),
                name: "델타".into(),
                status: "RECORDING".into(),
                title: Some("라이브 방송 중".into()),
                bno: Some("999".into()),
                started_at: Some("2026-11-22T01:00:00Z".into()),
                size_bytes: 2 * 1024 * 1024,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    ctrl.apply_response_for_test(Response::Live {
        generation: Some(generation),
        status,
        refresh_thumbnails: false,
        message: None,
        poll: false,
    });

    // Default sort mode is STATUS: RECORDING > PASSWORD_REQUIRED > ERROR > OFFLINE.
    let order: Vec<&str> = ctrl
        .live_model
        .rows
        .iter()
        .map(|r| r.status.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["RECORDING", "PASSWORD_REQUIRED", "ERROR", "OFFLINE"]
    );
    assert!(!ctrl.live_model.rows[0].elapsed.is_empty());

    // Switching to ORDER restores upstream order.
    ctrl.set_live_sort_mode("ORDER");
    let order2: Vec<&str> = ctrl
        .live_model
        .rows
        .iter()
        .map(|r| r.account.as_str())
        .collect();
    assert_eq!(order2, vec!["alpha", "beta", "gamma", "delta"]);

    // Search filters by name, account, title, or platform.
    ctrl.set_live_search("라이브".into());
    assert_eq!(ctrl.live_model.rows.len(), 1);
    assert_eq!(ctrl.live_model.rows[0].account, "delta");

    // Open stream dispatches Request::OpenLiveStream.
    ctrl.live_open_stream("KICK".into(), "delta".into());
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::OpenLiveStream { ref platform, ref account })
            if platform == "KICK" && account == "delta"
    ));
}

#[test]
fn gpui_exit_modal_window_bounds_and_tray_alerts_deduplicate() {
    use crate::core_worker::Response;
    use stream_archive_server::{
        model::{ChannelRuntimeStatus, NativeWatcherStatus},
        storage_service::{StorageSnapshot, StorageVolume},
    };

    let (mut ctrl, rx) = AppController::new_disconnected_for_test();
    ctrl.state.notify_record_start = true;
    ctrl.state.notify_warning = true;

    // Before the initial Snapshot arrives, periodic bounds capture must NOT overwrite saved bounds.
    ctrl.mark_initial_window_opened();
    ctrl.save_window_bounds("0,0,1180,740,0".into());
    assert!(rx.try_recv().is_err());

    // When a late initial Snapshot arrives with saved bounds, emit ApplyWindowBounds and unlock saving.
    ctrl.apply_response_for_test(Response::Snapshot {
        fields: None,
        diagnostics: DiagnosticsSnapshot {
            status: DiagnosticStatus::Ok,
            runtime_ready: true,
            attention_required: false,
            summary: DiagnosticsSummary::default(),
            items: Vec::new(),
        },
        window_bounds: Some("200,150,1400,900,1".into()),
        first_run: false,
        startup_failed: false,
        message: String::new(),
    });
    assert_eq!(
        ctrl.take_desktop_signals(),
        vec![DesktopSignal::ApplyWindowBounds(
            "200,150,1400,900,1".into()
        )]
    );

    // Saving identical window bounds only dispatches once after initialization.
    ctrl.save_window_bounds("120,90,1280,820,0".into());
    ctrl.save_window_bounds("120,90,1280,820,0".into());
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::SaveWindowBounds(ref s)) if s == "120,90,1280,820,0"
    ));
    assert!(rx.try_recv().is_err());

    // ExitCheck(Ok(true)) opens GPUI exit modal and requests window visibility.
    ctrl.state.desktop_exit_pending = true;
    ctrl.apply_response_for_test(Response::ExitCheck(Ok(true)));
    assert!(ctrl.state.exit_confirm_visible);
    assert!(
        ctrl.take_desktop_signals()
            .contains(&DesktopSignal::EnsureWindowVisible)
    );

    // Cancelling closes the modal and clears desktop_exit_pending.
    ctrl.complete_exit_confirmation(false);
    assert!(!ctrl.state.exit_confirm_visible);
    assert!(!ctrl.state.desktop_exit_pending);

    // ExitCheck(Ok(false)) shuts down immediately without modal.
    ctrl.state.desktop_exit_pending = true;
    ctrl.apply_response_for_test(Response::ExitCheck(Ok(false)));
    assert!(!ctrl.state.exit_confirm_visible);
    assert!(matches!(rx.try_recv(), Ok(Request::Shutdown)));

    // Initial LIVE snapshot seeds alert baseline without firing stale alerts,
    // but emits WatcherStateChanged(true).
    let base_live = NativeWatcherStatus {
        running: true,
        channels: vec![ChannelRuntimeStatus {
            platform: PlatformId::Chzzk,
            account: "ch1".into(),
            name: "채널1".into(),
            status: "OFFLINE".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    ctrl.apply_response_for_test(Response::Live {
        generation: None,
        status: base_live,
        refresh_thumbnails: false,
        message: None,
        poll: true,
    });
    let signals = ctrl.take_desktop_signals();
    assert_eq!(signals, vec![DesktopSignal::WatcherStateChanged(true)]);

    // Transition to RECORDING and ERROR fires exactly one alert each; repeat does not spam.
    let active_live = NativeWatcherStatus {
        running: true,
        channels: vec![
            ChannelRuntimeStatus {
                platform: PlatformId::Chzzk,
                account: "ch1".into(),
                name: "채널1".into(),
                status: "RECORDING".into(),
                bno: Some("555".into()),
                ..Default::default()
            },
            ChannelRuntimeStatus {
                platform: PlatformId::Soop,
                account: "ch2".into(),
                name: "채널2".into(),
                status: "ERROR".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    ctrl.apply_response_for_test(Response::Live {
        generation: None,
        status: active_live.clone(),
        refresh_thumbnails: false,
        message: None,
        poll: true,
    });
    let signals = ctrl.take_desktop_signals();
    assert_eq!(
        signals
            .iter()
            .filter(|s| matches!(s, DesktopSignal::TrayAlert { .. }))
            .count(),
        2
    );

    // Polling the exact same state emits zero duplicate alerts.
    ctrl.apply_response_for_test(Response::Live {
        generation: None,
        status: active_live,
        refresh_thumbnails: false,
        message: None,
        poll: true,
    });
    assert!(ctrl.take_desktop_signals().is_empty());

    // CRITICAL storage volume alerts once and does not repeat while still critical.
    let crit_storage = StorageSnapshot {
        threshold_gb: 20.0,
        database_size_bytes: 0,
        volumes: vec![StorageVolume {
            probe_path: "G:\\".into(),
            paths: vec!["G:\\chzzk".into()],
            roles: vec!["채널 저장".into()],
            total_bytes: 1_000_000_000,
            free_bytes: 1_000_000,
            used_percent: 99.9,
            threshold_gb: 20.0,
            status: "CRITICAL".into(),
            error: None,
        }],
    };
    ctrl.apply_response_for_test(Response::Storage {
        generation: 0,
        snapshot: crit_storage.clone(),
        message: None,
        poll: false,
    });
    assert!(
        ctrl.take_desktop_signals()
            .iter()
            .any(|s| matches!(s, DesktopSignal::TrayAlert { warning: true, .. }))
    );
    ctrl.apply_response_for_test(Response::Storage {
        generation: 0,
        snapshot: crit_storage,
        message: None,
        poll: false,
    });
    assert!(ctrl.take_desktop_signals().is_empty());

    // Channels search & platform filter state, plus adding while filtered.
    ctrl.state.config_busy = false;
    ctrl.set_channels_platform_filter("CHZZK");
    ctrl.set_channels_search("테스트".into());
    assert_eq!(ctrl.state.channels_platform_filter, "CHZZK");
    assert_eq!(ctrl.state.channels_search, "테스트");
    ctrl.channels_add();
    assert!(ctrl.state.channels_search.is_empty());
    assert_eq!(ctrl.state.channels_rows.last().unwrap().platform, "CHZZK");
    ctrl.set_channels_platform_filter("INVALID");
    assert_eq!(ctrl.state.channels_platform_filter, "ALL");
}

#[test]
fn v1_1_0_automation_playback_startup_and_update_check_flows() {
    use crate::core_worker::Response;
    use stream_archive_server::{
        environment_settings::{EnvironmentSetting, SettingKind},
        model::NativeWatcherStatus,
        update_check::ReleaseUpdateInfo,
    };

    let (mut ctrl, rx) = AppController::new_disconnected_for_test();

    // Snapshot with auto_start_watcher=Y & start_minimized_tray=Y (non-first-run).
    ctrl.apply_response_for_test(Response::Snapshot {
        fields: Some(vec![
            EnvironmentSetting {
                key: "GUI_AUTO_START_WATCHER".into(),
                value: "Y".into(),
                description: String::new(),
                kind: SettingKind::Text,
            },
            EnvironmentSetting {
                key: "GUI_START_MINIMIZED_TRAY".into(),
                value: "Y".into(),
                description: String::new(),
                kind: SettingKind::Text,
            },
            EnvironmentSetting {
                key: "FILE_NAME_PATTERN".into(),
                value: "BJ_TITLE".into(),
                description: String::new(),
                kind: SettingKind::Text,
            },
        ]),
        diagnostics: DiagnosticsSnapshot::from_items(Vec::new()),
        window_bounds: None,
        first_run: false,
        startup_failed: false,
        message: String::new(),
    });
    assert!(ctrl.state.auto_start_watcher);
    assert!(ctrl.should_start_minimized_to_tray());
    // Not triggered before initial Live state arrives.
    assert!(rx.try_recv().is_err());

    // Initial Live snapshot arrives (stopped) -> auto-starts watcher exactly once.
    ctrl.apply_response_for_test(Response::Live {
        generation: None,
        status: NativeWatcherStatus {
            running: false,
            ..Default::default()
        },
        refresh_thumbnails: false,
        message: None,
        poll: false,
    });
    assert!(matches!(rx.try_recv(), Ok(Request::LiveStart)));
    assert!(ctrl.state.live_busy);

    // Subsequent stopped Live response does not re-trigger auto-start.
    ctrl.apply_response_for_test(Response::Live {
        generation: None,
        status: NativeWatcherStatus {
            running: false,
            ..Default::default()
        },
        refresh_thumbnails: false,
        message: None,
        poll: false,
    });
    assert!(rx.try_recv().is_err());

    // Media file open dispatches Request::OpenMediaFile and reports errors on active History tab.
    ctrl.state.active_page = Page::History;
    ctrl.open_media_file(r"D:\recordings\test.ts".into());
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::OpenMediaFile(p)) if p == r"D:\recordings\test.ts"
    ));
    ctrl.apply_response_for_test(Response::MediaFileOpened(Err("missing".into())));
    assert!(ctrl.state.history_message.contains("영상 재생 실패"));

    // Windows startup toggle round-trip.
    ctrl.set_windows_startup(true);
    assert!(ctrl.state.windows_startup_busy);
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::SetWindowsStartup(true))
    ));
    ctrl.apply_response_for_test(Response::WindowsStartup(Ok(true)));
    assert!(!ctrl.state.windows_startup_busy);
    assert!(ctrl.state.windows_startup_enabled);

    // Manual update check + banner dismiss + release URL open.
    ctrl.check_updates_manual();
    assert!(ctrl.state.update_checking);
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::CheckUpdates { manual: true })
    ));
    ctrl.apply_response_for_test(Response::UpdateCheck {
        manual: true,
        result: Ok(ReleaseUpdateInfo {
            current_version: "1.1.0".into(),
            latest_version: "1.2.0".into(),
            update_available: true,
            release_url: "https://github.com/p1k1g/stream-archive/releases/tag/v1.2.0".into(),
        }),
    });
    assert!(!ctrl.state.update_checking);
    assert!(ctrl.state.update_available);
    assert!(!ctrl.state.update_dismissed);
    ctrl.open_release_url();
    assert!(matches!(
        rx.try_recv(),
        Ok(Request::OpenReleaseUrl(url)) if url.ends_with("/tag/v1.2.0")
    ));
    ctrl.dismiss_update_banner();
    assert!(ctrl.state.update_dismissed);
}
