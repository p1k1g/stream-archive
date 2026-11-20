#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod channels_adapter;
mod controller;
mod core_worker;
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
mod state;
mod storage_adapter;
mod theme;
mod thumbnail_adapter;
mod ui;
mod vod_adapter;

#[cfg(test)]
mod ui_smoke_tests;

use crate::{
    controller::{AppController, DesktopSignal},
    state::Page,
};
use gpui_kit::*;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

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

fn open_or_activate_main_window(
    controller: &Entity<AppController>,
    active_window: &Rc<RefCell<Option<AnyWindowHandle>>>,
    cx: &mut App,
) {
    if let Some(handle) = *active_window.borrow() {
        if handle
            .update(cx, |_, window, _| {
                window.activate_window();
            })
            .is_ok()
        {
            controller.update(cx, |c, cx| {
                c.set_window_visible(true);
                cx.notify();
            });
            return;
        }
        *active_window.borrow_mut() = None;
    }

    let bounds = WindowBounds::Windowed(Bounds::centered(None, size(px(1180.0), px(740.0)), cx));
    let mut options = gpui_kit::component::TitleBar::window_options();
    options.window_bounds = Some(bounds);
    options.window_min_size = Some(size(px(1000.0), px(650.0)));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some("Stream Archive".into());
    }
    options.app_id = Some("stream-archive".into());

    let controller_for_view = controller.clone();
    let controller_for_close = controller.clone();
    match gpui_kit::open_window(options, cx, move |window, cx| {
        desktop::apply_native_window_icon(window);
        window.on_window_should_close(cx, move |_window, cx| {
            controller_for_close.update(cx, |c, cx| {
                let allow = c.handle_window_close_requested();
                cx.notify();
                allow
            })
        });
        cx.new(|cx| ui::MainView::new(controller_for_view, window, cx))
    }) {
        Ok((handle, _view)) => {
            *active_window.borrow_mut() = Some(handle);
            controller.update(cx, |c, cx| {
                c.set_window_visible(true);
                cx.notify();
            });
        }
        Err(err) => {
            append_gui_crash_log(&format!("gpui open_window failed: {err}"));
        }
    }
}

fn main() {
    if stream_archive_server::release_tooling::handle_native_cli_args() {
        return;
    }
    install_panic_log_hook();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::configure_theme(cx);

            let controller = cx.new(|_| AppController::start());
            let active_window: Rc<RefCell<Option<AnyWindowHandle>>> = Rc::new(RefCell::new(None));

            #[cfg(windows)]
            let tray = Rc::new(RefCell::new(match desktop::windows_tray::TrayHost::new() {
                Ok(host) => Some(host),
                Err(err) => {
                    append_gui_crash_log(&format!("TrayHost::new failed: {err}"));
                    controller.update(cx, |c, cx| {
                        c.state.desktop_recovery_message = format!(
                            "Windows 트레이 아이콘을 초기화하지 못했습니다 ({err}). 창을 열어 둔 상태로 사용하세요."
                        );
                        cx.notify();
                    });
                    None
                }
            }));

            open_or_activate_main_window(&controller, &active_window, cx);

            let controller_loop = controller.clone();
            let window_loop = active_window.clone();
            #[cfg(windows)]
            let tray_loop = tray.clone();

            cx.spawn(async move |cx: &mut AsyncApp| {
                let mut last_status_poll = Instant::now();
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;

                    let mut should_break = false;
                    cx.update(|cx| {
                        let (exit_check, signals, visible, exit_pending) =
                            controller_loop.update(cx, |c, cx| {
                                let (changed, exit_check) = c.poll_tick();
                                let signals = c.take_desktop_signals();
                                if changed || !signals.is_empty() {
                                    cx.notify();
                                }
                                (
                                    exit_check,
                                    signals,
                                    c.is_window_visible(),
                                    c.state.desktop_exit_pending,
                                )
                            });

                        if let Some(check_result) = exit_check {
                            let confirmed = match check_result {
                                Ok(false) => true,
                                Ok(true) => {
                                    #[cfg(windows)]
                                    let owner = window_loop
                                        .borrow()
                                        .and_then(|h| {
                                            h.update(cx, |_, w, _| desktop::extract_hwnd(w)).ok()
                                        })
                                        .flatten();
                                    #[cfg(not(windows))]
                                    let owner = None;
                                    desktop::confirm_exit(owner)
                                }
                                Err(err) => {
                                    append_gui_crash_log(&format!("ExitCheck error: {err}"));
                                    #[cfg(windows)]
                                    let owner = window_loop
                                        .borrow()
                                        .and_then(|h| {
                                            h.update(cx, |_, w, _| desktop::extract_hwnd(w)).ok()
                                        })
                                        .flatten();
                                    #[cfg(not(windows))]
                                    let owner = None;
                                    desktop::confirm_exit(owner)
                                }
                            };
                            controller_loop.update(cx, |c, cx| {
                                c.complete_exit_confirmation(confirmed);
                                cx.notify();
                            });
                        }

                        for signal in signals {
                            match signal {
                                DesktopSignal::HideToTray => {
                                    #[cfg(windows)]
                                    let has_tray = tray_loop.borrow().is_some();
                                    #[cfg(not(windows))]
                                    let has_tray = false;

                                    if has_tray {
                                        if let Some(handle) = window_loop.borrow_mut().take() {
                                            let _ = handle.update(cx, |_, w, _| w.remove_window());
                                        }
                                        controller_loop.update(cx, |c, _| {
                                            c.set_window_visible(false);
                                        });
                                    } else {
                                        controller_loop.update(cx, |c, cx| {
                                            c.state.desktop_recovery_message = "트레이 아이콘이 없어 창을 숨길 수 없습니다. 설정에서 창 닫기 동작을 변경하거나 창을 열어 두세요.".into();
                                            cx.notify();
                                        });
                                    }
                                }
                                DesktopSignal::ExitNow => {
                                    #[cfg(windows)]
                                    {
                                        let _ = tray_loop.borrow_mut().take();
                                    }
                                    if let Some(handle) = window_loop.borrow_mut().take() {
                                        let _ = handle.update(cx, |_, w, _| w.remove_window());
                                    }
                                    cx.quit();
                                    should_break = true;
                                    return;
                                }
                                DesktopSignal::StatusUpdated(_summary) => {
                                    #[cfg(windows)]
                                    if let Some(t) = tray_loop.borrow().as_ref() {
                                        t.update_status(&_summary);
                                    }
                                }
                                DesktopSignal::DownloadNotice {
                                    completed: _c,
                                    failed: _f,
                                } => {
                                    #[cfg(windows)]
                                    if let Some(t) = tray_loop.borrow().as_ref() {
                                        t.enqueue_notice(_c, _f);
                                    }
                                }
                                DesktopSignal::ClearNotices => {
                                    #[cfg(windows)]
                                    if let Some(t) = tray_loop.borrow().as_ref() {
                                        t.clear_notices();
                                    }
                                }
                            }
                        }

                        #[cfg(windows)]
                        {
                            let (events, force_restore) = if let Some(t) = tray_loop.borrow().as_ref() {
                                t.poll(visible)
                            } else {
                                (Vec::new(), false)
                            };
                            for ev in events {
                                match ev {
                                    desktop::windows_tray::TrayEvent::Open => {
                                        open_or_activate_main_window(&controller_loop, &window_loop, cx);
                                    }
                                    desktop::windows_tray::TrayEvent::History => {
                                        open_or_activate_main_window(&controller_loop, &window_loop, cx);
                                        controller_loop.update(cx, |c, cx| {
                                            c.select_page(Page::History);
                                            c.refresh_history();
                                            cx.notify();
                                        });
                                    }
                                    desktop::windows_tray::TrayEvent::Exit => {
                                        controller_loop.update(cx, |c, cx| {
                                            c.begin_exit_check();
                                            cx.notify();
                                        });
                                    }
                                    desktop::windows_tray::TrayEvent::ShellRestart
                                    | desktop::windows_tray::TrayEvent::NoticeClosed => {}
                                }
                            }
                            if force_restore {
                                open_or_activate_main_window(&controller_loop, &window_loop, cx);
                                controller_loop.update(cx, |c, cx| {
                                    c.state.desktop_recovery_message =
                                        "Windows 트레이를 복구하지 못해 창을 다시 표시했습니다."
                                            .into();
                                    cx.notify();
                                });
                            }
                        }
                        #[cfg(not(windows))]
                        let _ = visible;

                        if !exit_pending && last_status_poll.elapsed() >= Duration::from_secs(3) {
                            last_status_poll = Instant::now();
                            controller_loop.update(cx, |c, _| c.request_desktop_status());
                        }
                    });

                    if should_break {
                        break;
                    }
                }
            })
            .detach();
        });
}
