//! Windows presentation only; runtime state and shutdown stay in the core.
#[cfg(windows)]
pub mod windows_tray;

#[cfg(windows)]
pub fn extract_hwnd(window: &gpui_kit::Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    if let RawWindowHandle::Win32(win32) = handle.as_raw() {
        Some(windows::Win32::Foundation::HWND(win32.hwnd.get() as *mut _))
    } else {
        None
    }
}

#[cfg(windows)]
pub fn apply_native_window_icon(window: &gpui_kit::Window) {
    use windows::{
        Win32::{
            Foundation::{LPARAM, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SendMessageW, WM_SETICON,
            },
        },
        core::PCWSTR,
    };
    let Some(hwnd) = extract_hwnd(window) else {
        return;
    };
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        if let Ok(small) = LoadImageW(
            Some(module.into()),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            16,
            16,
            LR_SHARED,
        ) {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(small.0 as isize)),
            );
        }
        if let Ok(big) = LoadImageW(
            Some(module.into()),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            32,
            32,
            LR_SHARED,
        ) {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(big.0 as isize)),
            );
        }
    }
}

#[cfg(not(windows))]
pub fn apply_native_window_icon(_window: &gpui_kit::Window) {}

#[cfg(windows)]
pub fn confirm_exit(owner: Option<windows::Win32::Foundation::HWND>) -> bool {
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
        },
        core::w,
    };
    // The core thread continues working while this owned UI dialog is open.
    unsafe {
        MessageBoxW(
            owner,
            w!(
                "녹화·다운로드·채널 감시 또는 대기 중인 Queue가 있습니다.\n이 앱의 작업을 정리하고 종료할까요?"
            ),
            w!("Stream Archive 종료"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}

#[cfg(not(windows))]
pub fn confirm_exit(_owner: Option<()>) -> bool {
    true
}

#[cfg(windows)]
#[allow(dead_code)]
pub fn notify(owner: Option<windows::Win32::Foundation::HWND>, message: &str) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW},
        core::{HSTRING, w},
    };
    let message = HSTRING::from(message);
    unsafe {
        MessageBoxW(
            owner,
            &message,
            w!("Stream Archive"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

// Loaded settings work must not make X/Alt+F4 silently stop responding.
pub fn close_request_blocked(
    settings_loaded: bool,
    settings_busy: bool,
    close_choice_busy: bool,
    dialog_visible: bool,
    exit_pending: bool,
) -> bool {
    (!settings_loaded && settings_busy) || close_choice_busy || dialog_visible || exit_pending
}

// Explorer can briefly reject icon registration while restarting. Only restore
// a hidden window after repeated failures; a visible window needs no fallback.
#[cfg(any(windows, test))]
#[derive(Default)]
pub struct TrayRecovery {
    failures: u8,
}

#[cfg(any(windows, test))]
impl TrayRecovery {
    pub fn should_restore(&mut self, registered: bool, visible: bool) -> bool {
        if registered || visible {
            self.failures = 0;
            return false;
        }
        self.failures = self.failures.saturating_add(1);
        self.failures >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::{TrayRecovery, close_request_blocked};

    #[test]
    fn transient_tray_failure_does_not_restore_hidden_window() {
        let mut recovery = TrayRecovery::default();
        assert!(!recovery.should_restore(false, false));
        assert!(!recovery.should_restore(false, false));
        assert!(!recovery.should_restore(true, false));
        assert!(!recovery.should_restore(false, false));
        assert!(!recovery.should_restore(false, false));
        assert!(recovery.should_restore(false, false));
    }

    #[test]
    fn visible_window_resets_tray_failure_streak() {
        let mut recovery = TrayRecovery::default();
        for _ in 0..3 {
            recovery.should_restore(false, false);
        }
        assert!(!recovery.should_restore(false, true));
        assert!(!recovery.should_restore(false, false));
    }

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
