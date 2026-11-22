//! A hidden top-level window receives TaskbarCreated broadcasts. Message-only
//! windows do not. Wndproc only forwards presentation events, never core commands.
use crate::{
    desktop::TrayRecovery,
    notifications::{NoticeQueue, Summary},
};
use std::{
    cell::{Cell, RefCell},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME,
                NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIN_BALLOONHIDE, NIN_BALLOONTIMEOUT,
                NIN_BALLOONUSERCLICK, NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

const CALLBACK: u32 = WM_APP + 24;
const OPEN: usize = 1;
const EXIT: usize = 2;
const TOGGLE_WATCHER: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BalloonTarget {
    History,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Open,
    Exit,
    ToggleWatcher,
    ShellRestart,
    History,
    Live,
    NoticeClosed,
}

struct State {
    hwnd: HWND,
    icon: HICON,
    taskbar_created: u32,
    events: mpsc::Sender<TrayEvent>,
    status: RefCell<String>,
    watcher_running: Cell<bool>,
    notices: RefCell<NoticeQueue>,
    notice_visible: Cell<bool>,
    balloon_target: Cell<BalloonTarget>,
}

impl State {
    fn data(&self, tooltip: &str) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: CALLBACK,
            hIcon: self.icon,
            ..Default::default()
        };
        for (dest, src) in data.szTip.iter_mut().take(127).zip(tooltip.encode_utf16()) {
            *dest = src;
        }
        data
    }

    fn show_balloon(&self, title: &str, body: &str, warning: bool, target: BalloonTarget) -> bool {
        let mut data = self.data("");
        data.uFlags = NIF_INFO;
        data.dwInfoFlags =
            (if warning { NIIF_WARNING } else { NIIF_INFO }) | NIIF_RESPECT_QUIET_TIME;
        for (dest, src) in data
            .szInfoTitle
            .iter_mut()
            .take(63)
            .zip(title.encode_utf16())
        {
            *dest = src;
        }
        for (dest, src) in data.szInfo.iter_mut().take(255).zip(body.encode_utf16()) {
            *dest = src;
        }
        self.balloon_target.set(target);
        let shown = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data).as_bool() };
        self.notice_visible.set(shown);
        shown
    }

    fn show_notice(&self, summary: Summary) -> bool {
        let title = if summary.failed > 0 {
            "Stream Archive 다운로드 결과"
        } else {
            "Stream Archive 다운로드 완료"
        };
        let body = format!(
            "VOD 완료 {}건 · 실패 {}건\n클릭하면 History에서 결과를 확인합니다.",
            summary.completed, summary.failed
        );
        self.show_balloon(title, &body, summary.failed > 0, BalloonTarget::History)
    }

    fn clear_notice(&self) {
        self.notices.borrow_mut().clear();
        if self.notice_visible.replace(false) {
            let mut data = self.data("");
            data.uFlags = NIF_INFO;
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
    }

    fn ensure(&self, tooltip: &str) -> bool {
        let data = self.data(tooltip);
        // Verify the icon still exists, or re-add it after Explorer restarts.
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &data).as_bool()
                || Shell_NotifyIconW(NIM_ADD, &data).as_bool()
        }
    }
}

pub struct TrayHost {
    state: *mut State,
    receiver: mpsc::Receiver<TrayEvent>,
    recovery: RefCell<TrayRecovery>,
    last_ensure: Cell<Instant>,
}

impl TrayHost {
    fn state(&self) -> &State {
        // SAFETY: `self.state` is allocated with `Box::into_raw` in `TrayHost::new` and only
        // freed in `Drop` after `GWLP_USERDATA` is cleared and the hidden window is destroyed.
        unsafe { &*self.state }
    }

    pub fn new() -> Result<Self, String> {
        let (events, receiver) = mpsc::channel();
        unsafe {
            let module = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("StreamArchiveTrayWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                lpszClassName: class,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class,
                w!("Stream Archive tray"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            let icon = match LoadImageW(
                Some(module.into()),
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                32,
                32,
                IMAGE_FLAGS(0),
            ) {
                Ok(handle) => HICON(handle.0),
                Err(error) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(error.to_string());
                }
            };
            let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
            if taskbar_created == 0 {
                let _ = DestroyIcon(icon);
                let _ = DestroyWindow(hwnd);
                return Err("TaskbarCreated message registration failed".into());
            }
            let state = Box::into_raw(Box::new(State {
                hwnd,
                icon,
                taskbar_created,
                status: RefCell::new("상태 확인 중".into()),
                watcher_running: Cell::new(false),
                notices: RefCell::new(NoticeQueue::default()),
                notice_visible: Cell::new(false),
                balloon_target: Cell::new(BalloonTarget::History),
                events,
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            let host = Self {
                state,
                receiver,
                recovery: RefCell::new(TrayRecovery::default()),
                last_ensure: Cell::new(Instant::now()),
            };
            if !host.state().ensure("Stream Archive") {
                return Err("Windows 트레이 아이콘 등록 실패".into());
            }
            Ok(host)
        }
    }

    pub fn update_status(&self, summary: &str) {
        *self.state().status.borrow_mut() = summary.to_string();
        let tooltip = format!("Stream Archive — {summary}");
        let _ = self.state().ensure(&tooltip);
    }

    pub fn set_watcher_running(&self, running: bool) {
        self.state().watcher_running.set(running);
    }

    pub fn show_alert(&self, title: &str, body: &str, warning: bool) {
        let _ = self
            .state()
            .show_balloon(title, body, warning, BalloonTarget::Live);
    }

    pub fn enqueue_notice(&self, completed: usize, failed: usize) {
        self.state().notices.borrow_mut().add(
            Summary {
                completed: i32::try_from(completed).unwrap_or(i32::MAX),
                failed: i32::try_from(failed).unwrap_or(i32::MAX),
            },
            Instant::now(),
        );
    }

    pub fn clear_notices(&self) {
        self.state().clear_notice();
    }

    /// Drains Win32 tray events and processes periodic balloon / Explorer-restart recovery.
    /// Returns `(events, force_restore_window)`.
    pub fn poll(&self, window_visible: bool) -> (Vec<TrayEvent>, bool) {
        let mut out = Vec::new();
        let mut saw_restart = false;
        while let Ok(ev) = self.receiver.try_recv() {
            match ev {
                TrayEvent::NoticeClosed => {
                    self.state().notice_visible.set(false);
                    self.state().notices.borrow_mut().closed();
                }
                TrayEvent::ShellRestart => {
                    saw_restart = true;
                }
                other => out.push(other),
            }
        }

        let now = Instant::now();
        let notice = self.state().notices.borrow_mut().take(now);
        if let Some(summary) = notice {
            let _ = self.state().show_notice(summary);
        }

        let mut force_restore = false;
        if saw_restart || now.duration_since(self.last_ensure.get()) >= Duration::from_secs(5) {
            self.last_ensure.set(now);
            let status = self.state().status.borrow().clone();
            let tooltip = format!("Stream Archive — {status}");
            let ok = self.state().ensure(&tooltip);
            if self
                .recovery
                .borrow_mut()
                .should_restore(ok, window_visible)
            {
                force_restore = true;
            }
        }

        (out, force_restore)
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        unsafe {
            let state = &*self.state;
            let hwnd = state.hwnd;
            let icon = state.icon;
            // Clear GWLP_USERDATA before Shell_NotifyIconW/DestroyWindow so re-entrant
            // window messages during teardown cannot observe a freeing State pointer.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = Shell_NotifyIconW(NIM_DELETE, &state.data(""));
            let _ = DestroyWindow(hwnd);
            let _ = DestroyIcon(icon);
            drop(Box::from_raw(self.state));
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    if !ptr.is_null() {
        let state = unsafe { &*ptr };
        if message == state.taskbar_created {
            let _ = state.events.send(TrayEvent::ShellRestart);
            return LRESULT(0);
        }
        if message == CALLBACK {
            match lparam.0 as u32 {
                NIN_BALLOONUSERCLICK => {
                    let event = match state.balloon_target.get() {
                        BalloonTarget::History => TrayEvent::History,
                        BalloonTarget::Live => TrayEvent::Live,
                    };
                    let _ = state.events.send(event);
                    let _ = state.events.send(TrayEvent::NoticeClosed);
                }
                NIN_BALLOONHIDE | NIN_BALLOONTIMEOUT => {
                    let _ = state.events.send(TrayEvent::NoticeClosed);
                }
                WM_LBUTTONUP => {
                    let _ = state.events.send(TrayEvent::Open);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => unsafe {
                    popup(state);
                },
                _ => {}
            }
            return LRESULT(0);
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

unsafe fn popup(state: &State) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Stream Archive 열기"));
        let watcher_label = if state.watcher_running.get() {
            w!("LIVE 감시 중지")
        } else {
            w!("LIVE 감시 시작")
        };
        let _ = AppendMenuW(menu, MF_STRING, TOGGLE_WATCHER, watcher_label);
        let label = HSTRING::from(state.status.borrow().as_str());
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 3, &label);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, EXIT, w!("종료"));
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_ok() {
            let _ = SetForegroundWindow(state.hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                state.hwnd,
                None,
            )
            .0 as usize;
            let _ = PostMessageW(Some(state.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            match command {
                OPEN => {
                    let _ = state.events.send(TrayEvent::Open);
                }
                TOGGLE_WATCHER => {
                    let _ = state.events.send(TrayEvent::ToggleWatcher);
                }
                EXIT => {
                    let _ = state.events.send(TrayEvent::Exit);
                }
                _ => {}
            }
        }
        let _ = DestroyMenu(menu);
    }
}
