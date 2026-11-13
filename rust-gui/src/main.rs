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

fn install_panic_log_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
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
                let thread = std::thread::current();
                let name = thread.name().unwrap_or("unnamed");
                let backtrace = std::backtrace::Backtrace::force_capture();
                let _ = writeln!(file, "[unix:{secs}] thread={name} {info}\n{backtrace}\n");
            }
        }
        default_hook(info);
    }));
}

fn main() -> Result<(), slint::PlatformError> {
    install_panic_log_hook();
    let ui = MainWindow::new()?;
    let _controller = controller::bind(&ui);
    #[cfg(windows)]
    {
        let _desktop = desktop::bind(&ui);
        ui.show()?;
        slint::run_event_loop_until_quit()
    }
    #[cfg(not(windows))]
    ui.run()
}
