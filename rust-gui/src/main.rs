mod channels_adapter;
mod controller;
mod live_adapter;
mod native_picker;
mod settings_adapter;

use slint::ComponentHandle;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let _controller = controller::bind(&ui);
    ui.run()
}
