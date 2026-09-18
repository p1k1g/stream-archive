mod channels_adapter;
mod controller;
mod history_adapter;
mod live_adapter;
mod maintenance_adapter;
mod native_picker;
mod queue_adapter;
mod settings_adapter;
mod vod_adapter;

use slint::ComponentHandle;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let _controller = controller::bind(&ui);
    ui.run()
}
