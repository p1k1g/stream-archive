//! Windows current-user startup (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) helper.

#[cfg(any(windows, test))]
use std::path::Path;

#[cfg(any(windows, test))]
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(any(windows, test))]
const VALUE_NAME: &str = "StreamArchive";

#[cfg(any(windows, test))]
pub(crate) fn format_startup_command(exe_path: &Path) -> Result<String, String> {
    let raw = exe_path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains(['\0', '\r', '\n', '"']) {
        return Err("실행 파일 경로가 올바르지 않습니다.".into());
    }
    Ok(format!("\"{trimmed}\""))
}

#[cfg(windows)]
pub fn is_enabled() -> Result<bool, String> {
    use windows::{
        Win32::{
            Foundation::ERROR_FILE_NOT_FOUND,
            System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
            },
        },
        core::HSTRING,
    };

    let subkey = HSTRING::from(RUN_SUBKEY);
    let value_name = HSTRING::from(VALUE_NAME);
    let mut hkey = HKEY::default();
    // SAFETY: valid HSTRING pointers, closed before returning on success.
    let open = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, Some(0), KEY_READ, &mut hkey) };
    if open == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if open.is_err() {
        return Err(format!(
            "시작프로그램 레지스트리 조회 실패 (code {})",
            open.0
        ));
    }

    let mut data_len: u32 = 0;
    // SAFETY: hkey is open; passing None buffer queries byte size only.
    let query =
        unsafe { RegQueryValueExW(hkey, &value_name, None, None, None, Some(&mut data_len)) };
    // SAFETY: paired with successful RegOpenKeyExW.
    let _ = unsafe { RegCloseKey(hkey) };

    if query == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if query.is_err() {
        return Err(format!("시작프로그램 값 조회 실패 (code {})", query.0));
    }
    // UTF-16 string with at least one character plus NUL terminator (> 2 bytes).
    Ok(data_len > 2)
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<bool, String> {
    use windows::{
        Win32::{
            Foundation::ERROR_FILE_NOT_FOUND,
            System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
                RegOpenKeyExW, RegSetValueExW,
            },
        },
        core::HSTRING,
    };

    let subkey = HSTRING::from(RUN_SUBKEY);
    let value_name = HSTRING::from(VALUE_NAME);
    let mut hkey = HKEY::default();
    // SAFETY: valid HSTRING pointer; handle closed before returning on success.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if open.is_err() {
        return Err(format!(
            "시작프로그램 레지스트리 열기 실패 (code {})",
            open.0
        ));
    }

    let result = if enabled {
        let exe = std::env::current_exe()
            .map_err(|err| format!("현재 실행 파일 경로 확인 실패: {err}"))?;
        let command = format_startup_command(&exe)?;
        let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
        // SAFETY: bytes references a valid NUL-terminated UTF-16 buffer for REG_SZ.
        let status = unsafe { RegSetValueExW(hkey, &value_name, Some(0), REG_SZ, Some(bytes)) };
        if status.is_err() {
            Err(format!("시작프로그램 등록 실패 (code {})", status.0))
        } else {
            Ok(true)
        }
    } else {
        // SAFETY: hkey is open with KEY_SET_VALUE.
        let status = unsafe { RegDeleteValueW(hkey, &value_name) };
        if status.is_ok() || status == ERROR_FILE_NOT_FOUND {
            Ok(false)
        } else {
            Err(format!("시작프로그램 해제 실패 (code {})", status.0))
        }
    };

    // SAFETY: paired with successful RegOpenKeyExW.
    let _ = unsafe { RegCloseKey(hkey) };
    result
}

#[cfg(not(windows))]
pub fn is_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> Result<bool, String> {
    Err("Windows 시작프로그램 등록은 Windows 환경에서만 지원됩니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_quoted_startup_command_and_rejects_control_chars() {
        let cmd = format_startup_command(Path::new(
            r"C:\Program Files\StreamArchive\StreamArchive.exe",
        ))
        .unwrap();
        assert_eq!(cmd, r#""C:\Program Files\StreamArchive\StreamArchive.exe""#);
        assert!(format_startup_command(Path::new("")).is_err());
        assert!(format_startup_command(Path::new("C:\\bad\"path\\app.exe")).is_err());
        assert!(format_startup_command(Path::new("C:\\bad\npath\\app.exe")).is_err());
    }
}
