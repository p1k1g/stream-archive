#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::{
    env,
    ffi::OsStr,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    thread,
    time::Duration,
};

#[cfg(windows)]
use windows_sys::Win32::UI::{
    Shell::ShellExecuteW,
    WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW, SW_SHOWNORMAL},
};

#[cfg(windows)]
const APP_MARKER: &str = "<title>Stream Archive</title>";

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Probe {
    Closed,
    StreamArchive,
    Other,
}

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn local_endpoint() -> (SocketAddr, String) {
    let bind = env::var("STREAM_ARCHIVE_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let port = bind
        .rsplit_once(':')
        .and_then(|(_, value)| value.parse::<u16>().ok())
        .unwrap_or(8787);
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("valid loopback endpoint");
    (addr, format!("http://127.0.0.1:{port}/"))
}

#[cfg(windows)]
fn probe(addr: SocketAddr) -> Probe {
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) else {
        return Probe::Closed;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return Probe::Other;
    }
    let mut response = String::new();
    let _ = stream.take(32 * 1024).read_to_string(&mut response);
    if response.contains(APP_MARKER) || response.contains("Stream Archive") {
        Probe::StreamArchive
    } else {
        Probe::Other
    }
}

#[cfg(windows)]
fn shell_open(file: &OsStr, directory: Option<&Path>) -> Result<(), String> {
    let operation = wide("open");
    let file = wide(file);
    let directory_wide = directory.map(wide);
    let directory_ptr = directory_wide
        .as_ref()
        .map_or(null(), |value| value.as_ptr());
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            null(),
            directory_ptr,
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        Err(format!(
            "Windows ShellExecute failed with code {}",
            result as isize
        ))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn show_error(message: &str) {
    let title = wide("Stream Archive");
    let message = wide(message);
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(windows)]
fn executable_dir() -> Result<PathBuf, String> {
    let exe = env::current_exe()
        .map_err(|err| format!("실행 파일 위치를 확인하지 못했습니다.\n{err}"))?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "실행 파일 폴더를 확인하지 못했습니다.".to_string())
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    let (addr, url) = local_endpoint();
    match probe(addr) {
        Probe::StreamArchive => {
            shell_open(OsStr::new(&url), None)?;
            return Ok(());
        }
        Probe::Other => {
            return Err(format!(
                "127.0.0.1:{} 포트를 다른 프로그램이 사용 중입니다.\nStream Archive를 시작할 수 없습니다.",
                addr.port()
            ));
        }
        Probe::Closed => {}
    }

    let dir = executable_dir()?;
    let server = dir.join("stream-archive-server.exe");
    if !server.is_file() {
        return Err(format!(
            "stream-archive-server.exe를 찾지 못했습니다.\n{}",
            server.display()
        ));
    }

    // Keep the server console visible. Users can still stop it with Ctrl+C,
    // preserving the server's graceful owned LIVE/VOD cleanup behavior.
    shell_open(server.as_os_str(), Some(&dir))?;

    for _ in 0..150 {
        thread::sleep(Duration::from_millis(100));
        if probe(addr) == Probe::StreamArchive {
            shell_open(OsStr::new(&url), None)?;
            return Ok(());
        }
    }

    Err(format!(
        "Stream Archive 서버가 15초 안에 준비되지 않았습니다.\n서버 콘솔의 오류 메시지를 확인하세요.\n접속 주소: {url}"
    ))
}

#[cfg(windows)]
fn main() {
    if let Err(err) = run() {
        show_error(&err);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("stream-archive-launcher is only supported on Windows.");
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_to_loopback() {
        unsafe { env::remove_var("STREAM_ARCHIVE_BIND") };
        let (addr, url) = local_endpoint();
        assert_eq!(addr.to_string(), "127.0.0.1:8787");
        assert_eq!(url, "http://127.0.0.1:8787/");
    }
}
