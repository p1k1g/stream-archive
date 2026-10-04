#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod channels_adapter;
mod controller;
mod desktop;
mod diagnostic_text;
mod formatting;
mod history_adapter;
mod live_adapter;
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

fn main() -> Result<(), slint::PlatformError> {
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
