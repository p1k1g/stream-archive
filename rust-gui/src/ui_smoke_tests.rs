//! Exercise the compiled presentation with a headless software window. No core,
//! provider credentials, database, or native desktop session is required.
use crate::{AppState, MainWindow, MaintenanceState, QueueDisplayRow, QueueHistoryState};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::{cell::Cell, rc::Rc};

struct UiTestPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for UiTestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn click(ui: &MainWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn render(window: &MinimalSoftwareWindow, width: usize, height: usize) {
    window.request_redraw();
    let mut pixels = vec![Rgb8Pixel::default(); width * height];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width);
    }));
}

#[test]
fn native_navigation_and_watcher_toggle_preserve_input_guards() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(UiTestPlatform(window.clone()))).unwrap();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(PhysicalSize::new(1120, 720));
    ui.show().unwrap();
    let state = ui.global::<AppState>();
    state.set_runtime_ready(true);
    state.set_live_loaded(true);
    state.set_live_busy(false);
    let starts = Rc::new(Cell::new(0));
    let stops = Rc::new(Cell::new(0));
    let counter = starts.clone();
    let weak = ui.as_weak();
    state.on_live_start(move || {
        counter.set(counter.get() + 1);
        weak.upgrade()
            .unwrap()
            .global::<AppState>()
            .set_live_running(true);
    });
    let counter = stops.clone();
    let weak = ui.as_weak();
    state.on_live_stop(move || {
        counter.set(counter.get() + 1);
        weak.upgrade()
            .unwrap()
            .global::<AppState>()
            .set_live_running(false);
    });
    render(&window, 1120, 720);
    // The single header action dispatches the correct existing callback.
    click(&ui, 970.0, 107.0);
    assert_eq!(starts.get(), 1);
    assert!(state.get_live_running());
    render(&window, 1120, 720);
    click(&ui, 970.0, 107.0);
    assert_eq!(stops.get(), 1);
    assert!(!state.get_live_running());
    state.set_live_busy(true);
    render(&window, 1120, 720);
    click(&ui, 970.0, 107.0);
    assert_eq!(starts.get(), 1);
    state.set_live_busy(false);
    render(&window, 1120, 720);
    click(&ui, 970.0, 107.0); // Focus follows a real pointer activation.
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    assert_eq!(starts.get(), 2);
    assert_eq!(stops.get(), 2);

    let maintenance = ui.global::<MaintenanceState>();
    maintenance.set_busy(false);
    maintenance.set_loaded(true);
    render(&window, 1120, 720);
    click(&ui, 100.0, 405.0);
    assert_eq!(state.get_active_page(), "Diagnostics");
    assert_eq!(maintenance.get_section(), "Diagnostics");
    render(&window, 1120, 720);
    click(&ui, 1000.0, 107.0);
    assert_eq!(maintenance.get_section(), "Logs");
    click(&ui, 100.0, 358.0);
    assert_eq!(state.get_active_page(), "Settings");
    render(&window, 1120, 720);
    click(&ui, 1040.0, 107.0);
    assert_eq!(state.get_settings_view(), "Manage");
    assert_eq!(maintenance.get_section(), "Backup");

    let queue = ui.global::<QueueHistoryState>();
    queue.set_queue_busy(false);
    queue.set_queue_loaded(true);
    let task = QueueDisplayRow {
        id: "polling-task".into(),
        title: "Queue task".into(),
        platform: "SOOP".into(),
        ..Default::default()
    };
    state.set_active_page("Queue".into());
    queue.set_queue_rows(ModelRc::new(VecModel::from(vec![task.clone()])));
    render(&window, 1120, 720);
    click(&ui, 1060.0, 240.0);
    assert_eq!(queue.get_expanded_queue_id(), "polling-task");
    // Polling replaces the whole display model. A repeated row must recover its
    // expanded state by ID, so the next click closes it rather than reopens it.
    queue.set_queue_rows(ModelRc::new(VecModel::from(vec![task])));
    render(&window, 1120, 720);
    click(&ui, 1060.0, 240.0);
    assert!(queue.get_expanded_queue_id().is_empty());

    // Every page must still construct and render at the existing minimum size.
    ui.window().set_size(PhysicalSize::new(1000, 650));
    for page in [
        "LIVE",
        "Channels",
        "VOD",
        "Queue",
        "History",
        "Settings",
        "Diagnostics",
    ] {
        state.set_active_page(page.into());
        render(&window, 1000, 650);
    }
    ui.hide().unwrap();
}
