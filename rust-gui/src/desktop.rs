//! Windows presentation only; runtime state and shutdown stay in the core.
#[cfg(windows)]
mod windows_tray;
#[cfg(windows)]
pub use windows_tray::bind;

#[cfg(windows)]
fn owner(ui: &crate::MainWindow) -> Option<windows::Win32::Foundation::HWND> {
    use slint::ComponentHandle;
    use slint::winit_030::{
        WinitWindowAccessor,
        winit::raw_window_handle::{HasWindowHandle, RawWindowHandle},
    };
    let mut result = None;
    ui.window().with_winit_window(|window| {
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(handle) = handle.as_raw()
        {
            result = Some(windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _));
        }
    });
    result
}

#[cfg(windows)]
pub fn confirm_exit(ui: &crate::MainWindow) -> bool {
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
        },
        core::w,
    };
    // The core thread continues working while this owned UI dialog is open.
    unsafe {
        MessageBoxW(
            owner(ui),
            w!(
                "녹화·다운로드·채널 감시 또는 대기 중인 Queue가 있습니다.\n이 앱의 작업을 정리하고 종료할까요?"
            ),
            w!("Stream Archive 종료"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}

#[cfg(windows)]
pub fn notify(ui: &crate::MainWindow, message: &str) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW},
        core::{HSTRING, w},
    };
    let message = HSTRING::from(message);
    unsafe {
        MessageBoxW(
            owner(ui),
            &message,
            w!("Stream Archive"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(not(windows))]
pub fn confirm_exit(_ui: &crate::MainWindow) -> bool {
    false
}
#[cfg(not(windows))]
pub fn notify(_ui: &crate::MainWindow, message: &str) {
    eprintln!("{message}");
}

// Loaded settings work must not make X/Alt+F4 silently stop responding.
#[cfg(any(windows, test))]
fn close_request_blocked(
    settings_loaded: bool,
    settings_busy: bool,
    close_choice_busy: bool,
    dialog_visible: bool,
    exit_pending: bool,
) -> bool {
    (!settings_loaded && settings_busy) || close_choice_busy || dialog_visible || exit_pending
}

#[cfg(test)]
mod tests {
    use super::close_request_blocked;

    #[test]
    fn loaded_settings_work_does_not_block_close() {
        assert!(!close_request_blocked(true, true, false, false, false));
        assert!(!close_request_blocked(true, false, false, false, false));
    }

    #[test]
    fn initial_loading_and_active_close_transactions_block_reentry() {
        assert!(close_request_blocked(false, true, false, false, false));
        assert!(close_request_blocked(true, false, true, false, false));
        assert!(close_request_blocked(true, false, false, true, false));
        assert!(close_request_blocked(true, false, false, false, true));
    }
}
