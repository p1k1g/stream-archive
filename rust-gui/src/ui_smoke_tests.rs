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
