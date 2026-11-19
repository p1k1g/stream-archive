#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod channels_adapter;
mod controller;
mod desktop;
mod diagnostic_text;
mod formatting;
mod history_adapter;
mod live_adapter;
mod live_model;
mod maintenance_adapter;
mod native_picker;
mod native_shell;
mod notifications;
mod queue_adapter;
mod settings_adapter;
mod storage_adapter;
mod thumbnail_adapter;
mod vod_adapter;

#[cfg(test)]
mod ui_smoke_tests;

use slint::ComponentHandle;

slint::include_modules!();

pub(crate) fn append_gui_crash_log(entry: &str) {
    let base_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok());
    if let Some(base) = base_dir {
        let log_dir = base.join("log");
        let _ = std::fs::create_dir_all(&log_dir);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("panic.log"))
        {
            use std::io::Write;
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[unix:{secs}] {entry}\n");
        }
    }
}

fn install_panic_log_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let backtrace = std::backtrace::Backtrace::force_capture();
        append_gui_crash_log(&format!("thread={name} {info}\n{backtrace}"));
        default_hook(info);
    }));
}

#[cfg(windows)]
fn run_windows_event_loop(ui: &MainWindow) -> Result<(), slint::PlatformError> {
    const MAX_RECOVERY_ATTEMPTS: u32 = 5;
    const RECOVERY_RESET_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
    let mut consecutive_errors = 0u32;
    let mut last_error_at: Option<std::time::Instant> = None;
    let mut in_tray_standby = false;
    let mut saved_maximized = false;

    loop {
        match slint::run_event_loop_until_quit() {
            Ok(()) => return Ok(()),
            Err(err) => {
                if last_error_at.is_some_and(|t| t.elapsed() > RECOVERY_RESET_WINDOW) {
                    consecutive_errors = 0;
                    in_tray_standby = false;
                }
                last_error_at = Some(std::time::Instant::now());
                consecutive_errors += 1;
                append_gui_crash_log(&format!(
                    "event loop PlatformError (attempt {consecutive_errors}, standby={in_tray_standby}): {err}"
                ));
                if ui.global::<AppState>().get_desktop_exit_pending() {
                    return Err(err);
                }

                let was_visible = ui.window().is_visible();
                if was_visible {
                    saved_maximized = ui.window().is_maximized();
                }
                // With SLINT_DESTROY_WINDOW_ON_HIDE=1, hide() suspends the renderer and drops
                // the stale wgpu Device/Surface so the next show() recreates them on the new driver.
                let hide_ok = match ui.hide() {
                    Ok(()) => true,
                    Err(hide_err) => {
                        append_gui_crash_log(&format!("recovery ui.hide failed: {hide_err}"));
                        false
                    }
                };

                if in_tray_standby {
                    if consecutive_errors > MAX_RECOVERY_ATTEMPTS {
                        return Err(err);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(
                        (500 * u64::from(consecutive_errors)).min(5_000),
                    ));
                    continue;
                }

                if consecutive_errors <= MAX_RECOVERY_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(
                        500 * u64::from(consecutive_errors),
                    ));
                    if was_visible || !hide_ok {
                        if let Err(show_err) = ui.show() {
                            append_gui_crash_log(&format!("recovery ui.show failed: {show_err}"));
                        } else {
                            desktop::sync_recreated_window(ui, saved_maximized);
                        }
                    }
                } else {
                    // Window creation/rendering kept failing (e.g. GPU driver mid-reinstall).
                    // Enter hidden tray standby so active recordings continue uninterrupted;
                    // if the hidden loop itself fails repeatedly, the in_tray_standby guard exits.
                    in_tray_standby = true;
                    consecutive_errors = 0;
                    std::thread::sleep(std::time::Duration::from_millis(1_000));
                    ui.global::<AppState>().set_desktop_recovery_message(
                        "그래픽 장치 재설정으로 창을 복구 대기 상태로 전환했습니다. 트레이 아이콘을 클릭해 다시 열어주세요."
                            .into(),
                    );
                }
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    if std::env::var_os("SLINT_DESTROY_WINDOW_ON_HIDE").is_none() {
        // SAFETY: executed at startup on the main thread before any background threads are spawned.
        // Destroying the hidden native window releases the wgpu GPU device/surface while in the tray,
        // preventing GPU driver updates or TDR resets from terminating the app upon restore.
        unsafe {
            std::env::set_var("SLINT_DESTROY_WINDOW_ON_HIDE", "1");
        }
    }
    install_panic_log_hook();
    let ui = match MainWindow::new() {
        Ok(ui) => ui,
        Err(err) => {
            append_gui_crash_log(&format!("MainWindow::new failed: {err}"));
            return Err(err);
        }
    };
    let _controller = controller::bind(&ui);
    #[cfg(windows)]
    {
        let _desktop = desktop::bind(&ui);
        if let Err(err) = ui.show() {
            append_gui_crash_log(&format!("initial ui.show failed: {err}"));
            return Err(err);
        }
        run_windows_event_loop(&ui)
    }
    #[cfg(not(windows))]
    ui.run()
}
