//! Exercise the compiled presentation with a headless software window. No core,
//! provider credentials, database, or native desktop session is required.
use crate::{AppState, MainWindow, MaintenanceState, QueueDisplayRow, QueueHistoryState};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::{cell::Cell, rc::Rc};

#[path = "../tests/support/ui_snapshot.rs"]
mod ui_snapshot;

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
    let _ = render_pixels(window, width, height);
}

fn render_pixels(window: &MinimalSoftwareWindow, width: usize, height: usize) -> Vec<Rgb8Pixel> {
    window.request_redraw();
    let mut pixels = vec![Rgb8Pixel::default(); width * height];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width);
    }));
    ui_snapshot::save(&pixels, width, height);
    pixels
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

#[test]
fn live_actions_and_close_dialog_remain_accessible_at_minimum_and_default_size() {
    use crate::{ChannelConfigRow, LiveChannelRow, StorageDisplayRow};
    use std::{cell::RefCell, collections::BTreeSet};
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(UiTestPlatform(window.clone()))).unwrap();
    let ui = MainWindow::new().unwrap();
    ui.show().unwrap();
    let state = ui.global::<AppState>();
    state.set_runtime_ready(true);
    state.set_live_loaded(true);
    state.set_live_busy(false);
    state.set_settings_busy(false);
    state.set_storage_rows(ModelRc::new(VecModel::from(vec![
        StorageDisplayRow {
            volume: "C:\\".into(),
            capacity: "여유 120 GB / 전체 500 GB".into(),
            used: "76%".into(),
            status: "정상".into(),
            status_tone: "ok".into(),
            ..Default::default()
        },
        StorageDisplayRow {
            volume: "G:\\".into(),
            capacity: "여유 1.2 TB / 전체 4 TB".into(),
            used: "70%".into(),
            status: "정상".into(),
            status_tone: "ok".into(),
            ..Default::default()
        },
    ])));
    state.set_live_rows(ModelRc::new(VecModel::from(vec![
        LiveChannelRow {
            target: "CHZZK:paused".into(),
            platform: "CHZZK".into(),
            name: "일시중지 채널".into(),
            account: "0123456789abcdef0123456789abcdef".into(),
            status_label: "일시중지".into(),
            status_tone: "warn".into(),
            suppressed: true,
            can_resume: true,
            can_recheck: true,
            ..Default::default()
        },
        LiveChannelRow {
            thumbnail_image: slint::Image::from_rgba8(
                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &vec![128u8; 320 * 180 * 4],
                    320,
                    180,
                ),
            ),
            target: "SOOP:recording".into(),
            platform: "SOOP".into(),
            name: "녹화 중 채널".into(),
            account: "recording".into(),
            status_label: "녹화 중".into(),
            status_tone: "ok".into(),
            title: "긴 방송 제목을 표시해도 동작 버튼이 다음 행에 가려지면 안 됩니다".into(),
            file: "G:/archive/recording.ts".into(),
            size: "1.2 GB".into(),
            can_stop_once: true,
            can_recheck: true,
            ..Default::default()
        },
    ])));
    let actions = Rc::new(RefCell::new(BTreeSet::new()));
    let observed = actions.clone();
    state.on_live_action(move |target, action| {
        observed
            .borrow_mut()
            .insert((target.to_string(), action.to_string()));
    });
    let folders = Rc::new(Cell::new(0));
    let observed = folders.clone();
    state.on_live_open_folder(move |_| observed.set(observed.get() + 1));
    let picked_channels = Rc::new(RefCell::new(BTreeSet::new()));
    let observed = picked_channels.clone();
    state.on_channel_pick_output(move |index| {
        observed.borrow_mut().insert(index);
    });
    for (width, height) in [(1000, 650), (1120, 720), (1440, 900)] {
        ui.window().set_size(PhysicalSize::new(width, height));
        let pixels = render_pixels(&window, width as usize, height as usize);
        // The entire CHZZK glyph must fit inside its 40px tile. A naturally sized
        // 512px child clipped by the tile renders a solid mint square instead.
        let mint_pixels = (200..400)
            .flat_map(|y| (224..264).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let p = pixels[*y * width as usize + *x];
                p.g > 200 && p.r < 30 && p.b < 200
            })
            .count();
        assert!(
            (100..1100).contains(&mint_pixels),
            "{width}x{height}: clipped platform logo ({mint_pixels} mint pixels)"
        );
        actions.borrow_mut().clear();
        folders.set(0);
        // Exercise the visible hit regions rather than invoking callbacks directly.
        for y in (185..height.min(550)).step_by(6) {
            for x in (220..width - 20).step_by(6) {
                click(&ui, x as f32, y as f32);
            }
        }
        // Multiple storage volumes reduce the LIVE viewport; the remaining
        // actions must still be reachable by scrolling inside the channel list.
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position: LogicalPosition::new(500.0, 350.0),
            delta_x: 0.0,
            delta_y: -250.0,
        });
        render(&window, width as usize, height as usize);
        for y in (215..height.min(440)).step_by(6) {
            for x in (220..width - 20).step_by(6) {
                click(&ui, x as f32, y as f32);
            }
        }
        for (target, action) in [
            ("CHZZK:paused", "resume"),
            ("CHZZK:paused", "recheck"),
            ("SOOP:recording", "stop"),
            ("SOOP:recording", "recheck"),
        ] {
            assert!(
                actions.borrow().contains(&(target.into(), action.into())),
                "{width}x{height}: missing {target}/{action}"
            );
        }
        assert!(folders.get() > 0);
        state.set_active_page("Channels".into());
        state.set_config_busy(false);
        state.set_channel_config_rows(ModelRc::new(VecModel::from(vec![
            ChannelConfigRow {
                platform: "CHZZK".into(),
                enabled: true,
                name: "길이가 긴 채널 이름도 모두 온전히 보존합니다".into(),
                account: "0123456789abcdef0123456789abcdef".into(),
                outdir: "G:/archive/a/long/path/that/remains/editable".into(),
            },
            ChannelConfigRow {
                platform: "SOOP".into(),
                enabled: true,
                name: "명아츄".into(),
                account: "1004ysus".into(),
                outdir: "G:/archive".into(),
            },
        ])));
        render(&window, width as usize, height as usize);
        if width == 1120 {
            assert_eq!(ui.get_channel_name_column_width(), 180.0);
        } else if width == 1440 {
            assert!(ui.get_channel_name_column_width() > 180.0);
            assert!(ui.get_channel_name_column_width() <= 320.0);
        }
        assert_eq!(
            state
                .get_channel_config_rows()
                .row_data(0)
                .unwrap()
                .account
                .len(),
            32
        );
        picked_channels.borrow_mut().clear();
        for y in (175..410).step_by(5) {
            for x in (width - 300..width - 15).step_by(5) {
                click(&ui, x as f32, y as f32);
            }
        }
        assert_eq!(
            *picked_channels.borrow(),
            BTreeSet::from([0, 1]),
            "{width}x{height}: both folder buttons must remain accessible"
        );
        state.set_config_busy(true);
        picked_channels.borrow_mut().clear();
        render(&window, width as usize, height as usize);
        for y in (175..410).step_by(5) {
            for x in (width - 300..width - 15).step_by(5) {
                click(&ui, x as f32, y as f32);
            }
        }
        assert!(picked_channels.borrow().is_empty());
        state.set_config_busy(false);
        state.set_active_page("LIVE".into());
    }
    state.set_close_dialog_visible(true);
    render(&window, 1440, 900);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: "\u{1b}".into(),
    });
    assert!(!state.get_close_dialog_visible());
    assert_eq!(state.get_close_action(), "EXIT");
    ui.hide().unwrap();
}
