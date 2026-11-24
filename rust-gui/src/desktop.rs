//! Windows presentation only; runtime state and shutdown stay in the core.
pub mod windows_startup;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn parse_window_bounds(spec: &str) -> Option<SavedWindowBounds> {
    let parts: Vec<&str> = spec.trim().split(',').collect();
    if parts.len() != 5 {
        return None;
    }
    let x: i32 = parts[0].parse().ok()?;
    let y: i32 = parts[1].parse().ok()?;
    let width: u32 = parts[2].parse().ok()?;
    let height: u32 = parts[3].parse().ok()?;
    if !(1000..=7680).contains(&width) || !(650..=4320).contains(&height) {
        return None;
    }
    let maximized = match parts[4] {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    Some(SavedWindowBounds {
        x,
        y,
        width,
        height,
        maximized,
    })
}

pub fn format_window_bounds(bounds: SavedWindowBounds) -> String {
    format!(
        "{},{},{},{},{}",
        bounds.x,
        bounds.y,
        bounds.width.clamp(1000, 7680),
        bounds.height.clamp(650, 4320),
        u8::from(bounds.maximized)
    )
}

pub fn capture_window_bounds_spec(window: &gpui_kit::Window) -> Option<String> {
    let (bounds, maximized) = match window.window_bounds() {
        gpui_kit::WindowBounds::Windowed(bounds) => (bounds, false),
        gpui_kit::WindowBounds::Maximized(bounds) => (bounds, true),
        gpui_kit::WindowBounds::Fullscreen(bounds) => (bounds, true),
    };
    let x = f32::from(bounds.origin.x).round() as i32;
    let y = f32::from(bounds.origin.y).round() as i32;
    let width = f32::from(bounds.size.width).round();
    let height = f32::from(bounds.size.height).round();
    if !width.is_finite() || !height.is_finite() || width < 1000.0 || height < 650.0 {
        return None;
    }
    Some(format_window_bounds(SavedWindowBounds {
        x,
        y,
        width: (width as u32).clamp(1000, 7680),
        height: (height as u32).clamp(650, 4320),
        maximized,
    }))
}

#[cfg(windows)]
pub fn apply_window_bounds(window: &gpui_kit::Window, saved: SavedWindowBounds) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SW_MAXIMIZE, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow,
    };
    let Some(hwnd) = extract_hwnd(window) else {
        return;
    };
    let scale = window.scale_factor().max(0.5);
    let x = (saved.x as f32 * scale).round() as i32;
    let y = (saved.y as f32 * scale).round() as i32;
    let width = (saved.width as f32 * scale).round() as i32;
    let height = (saved.height as f32 * scale).round() as i32;
    // SAFETY: `hwnd` is a live top-level GPUI window handle on the UI thread.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        if saved.maximized {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
}

#[cfg(not(windows))]
pub fn apply_window_bounds(_window: &gpui_kit::Window, _saved: SavedWindowBounds) {}

pub fn resolve_restored_bounds(
    spec: Option<&str>,
    displays: &[DisplayRect],
) -> Option<SavedWindowBounds> {
    let mut saved = parse_window_bounds(spec?)?;
    if displays.is_empty() {
        return Some(saved);
    }
    let right = saved.x.saturating_add(saved.width as i32);
    let bottom = saved.y.saturating_add(saved.height as i32);
    let visible_on_display = displays.iter().any(|d| {
        let d_right = d.x.saturating_add(d.width);
        let d_bottom = d.y.saturating_add(d.height);
        let overlap_w = right.min(d_right) - saved.x.max(d.x);
        let overlap_h = bottom.min(d_bottom) - saved.y.max(d.y);
        let top_reachable = saved.y >= d.y - 40 && saved.y <= d_bottom - 80;
        overlap_w >= 160 && overlap_h >= 80 && top_reachable
    });
    if visible_on_display {
        return Some(saved);
    }
    let primary = displays[0];
    let max_w = (primary.width - 40).max(1000) as u32;
    let max_h = (primary.height - 60).max(650) as u32;
    saved.width = saved.width.min(max_w);
    saved.height = saved.height.min(max_h);
    saved.x = primary.x + ((primary.width - saved.width as i32) / 2).max(0);
    saved.y = primary.y + ((primary.height - saved.height as i32) / 2).max(0);
    Some(saved)
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
    use super::{
        DisplayRect, SavedWindowBounds, TrayRecovery, close_request_blocked, format_window_bounds,
        parse_window_bounds, resolve_restored_bounds,
    };

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

    #[test]
    fn window_bounds_parse_format_and_clamp_offscreen_monitors() {
        let saved = SavedWindowBounds {
            x: 120,
            y: 90,
            width: 1320,
            height: 820,
            maximized: true,
        };
        let encoded = format_window_bounds(saved);
        assert_eq!(encoded, "120,90,1320,820,1");
        assert_eq!(parse_window_bounds(&encoded), Some(saved));
        assert_eq!(parse_window_bounds("0,0,800,600,0"), None);
        assert_eq!(parse_window_bounds("bad"), None);

        let primary = DisplayRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let secondary = DisplayRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };

        // Visible on secondary monitor stays on secondary monitor.
        let on_second =
            resolve_restored_bounds(Some("-1600,100,1280,800,0"), &[primary, secondary]).unwrap();
        assert_eq!(on_second.x, -1600);
        assert_eq!(on_second.width, 1280);

        // When secondary monitor is detached, bounds are clamped onto primary monitor.
        let clamped = resolve_restored_bounds(Some("-1600,100,1280,800,0"), &[primary]).unwrap();
        assert!(clamped.x >= 0 && clamped.x < primary.width);
        assert!(clamped.y >= 0 && clamped.y < primary.height);
        assert_eq!(clamped.width, 1280);
        assert_eq!(clamped.height, 800);
    }
}
