use std::path::{Path, PathBuf};

pub fn containing_directory(file_path: &str) -> Result<PathBuf, String> {
    let file_path = file_path.trim();
    if file_path.is_empty() {
        return Err("recording file path is empty".into());
    }
    let path = Path::new(file_path);
    if path.is_dir() {
        return Ok(path.to_path_buf());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "recording file path has no parent directory".to_string())?;
    if !parent.is_dir() {
        return Err(format!(
            "recording directory does not exist: {}",
            parent.display()
        ));
    }
    Ok(parent.to_path_buf())
}

#[cfg(windows)]
fn shell_execute_open(
    target: &windows::core::HSTRING,
    failure_context: &str,
) -> Result<(), String> {
    use windows::{
        Win32::{
            System::Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            },
            UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        },
        core::{PCWSTR, w},
    };

    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: paired only with a successful CoInitializeEx on this thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    // SAFETY: ShellExecuteW recommends STA COM initialization on worker threads.
    let com_hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let _com = ComGuard(com_hr.is_ok());
    // SAFETY: all strings live through the call, no ownership is transferred,
    // and ShellExecuteW only requests the OS shell to open an allowlisted target.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        return Err(format!("{failure_context} (code {code})"));
    }
    Ok(())
}

#[cfg(windows)]
pub fn open_containing_directory(file_path: &str) -> Result<(), String> {
    let directory = containing_directory(file_path)?;
    let directory = windows::core::HSTRING::from(directory.as_os_str());
    shell_execute_open(
        &directory,
        "Windows Shell failed to open the recording directory",
    )
}

#[cfg(not(windows))]
pub fn open_containing_directory(_file_path: &str) -> Result<(), String> {
    Err("opening a LIVE recording folder is supported only by the Windows Native UI".into())
}

#[cfg(windows)]
pub fn open_provider_url(platform: &str, account: &str) -> Result<(), String> {
    let url = crate::live_adapter::provider_live_url(platform, account)?;
    if !url.starts_with("https://") {
        return Err("안전하지 않은 방송 URL입니다.".into());
    }
    let target = windows::core::HSTRING::from(url.as_str());
    shell_execute_open(&target, "Windows Shell failed to open the live stream URL")
}

#[cfg(not(windows))]
pub fn open_provider_url(platform: &str, account: &str) -> Result<(), String> {
    let _ = crate::live_adapter::provider_live_url(platform, account)?;
    Err("opening a LIVE stream URL is supported only by the Windows Native UI".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containing_directory_preserves_spaces_and_unicode() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Stream Archive 저장 폴더-{nonce}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("방송 파일.ts");
        let resolved = containing_directory(file.to_string_lossy().as_ref()).unwrap();
        assert_eq!(resolved, directory);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn empty_or_missing_parent_is_rejected_without_shelling_out() {
        assert!(containing_directory("").is_err());
        let missing = std::env::temp_dir()
            .join("stream-archive-definitely-missing")
            .join("recording.ts");
        assert!(containing_directory(missing.to_string_lossy().as_ref()).is_err());
    }
}
