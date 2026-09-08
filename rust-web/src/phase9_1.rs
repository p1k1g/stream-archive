use super::{authorize, AppState, ApiResult};
use axum::{
    extract::State,
    http::{header::HOST, HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::process::Command;

pub(crate) async fn api_local_picker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    if !direct_local_request_allowed(&state.bind, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "native picker is available only from a direct 127.0.0.1/localhost browser session".into(),
        ));
    }

    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("").trim();
    if !matches!(kind, "folder" | "file") {
        return Err((StatusCode::BAD_REQUEST, "kind must be folder or file".into()));
    }

    let filter = body.get("filter").and_then(Value::as_str).unwrap_or("all").trim();
    if !matches!(filter, "all" | "exe" | "cookie") {
        return Err((StatusCode::BAD_REQUEST, "filter must be all, exe or cookie".into()));
    }

    let mut initial = body
        .get("initial_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if initial.eq_ignore_ascii_case("AUTO") {
        initial.clear();
    }
    if initial.len() > 4096 || initial.contains('\0') {
        return Err((StatusCode::BAD_REQUEST, "initial_path is invalid or too long".into()));
    }

    let kind = kind.to_string();
    let filter = filter.to_string();
    let selected = tokio::task::spawn_blocking(move || run_native_picker(&kind, &filter, &initial))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("picker task failed: {e}")))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(match selected {
        Some(path) => json!({"cancelled": false, "path": path}),
        None => json!({"cancelled": true, "path": Value::Null}),
    }))
}

fn direct_local_request_allowed(bind: &str, headers: &HeaderMap) -> bool {
    let bind_lower = bind.to_ascii_lowercase();
    let bind_loopback = bind_lower.starts_with("127.0.0.1:")
        || bind_lower.starts_with("[::1]:")
        || bind_lower.starts_with("localhost:");
    if !bind_loopback {
        return false;
    }

    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let direct_host = host == "localhost"
        || host.starts_with("localhost:")
        || host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || host == "[::1]"
        || host.starts_with("[::1]:");
    if !direct_host {
        return false;
    }

    !["forwarded", "x-forwarded-for", "x-forwarded-host", "x-real-ip"]
        .into_iter()
        .any(|name| headers.contains_key(name))
}

#[cfg(windows)]
fn run_native_picker(kind: &str, filter: &str, initial: &str) -> Result<Option<String>, String> {
    let script = if kind == "folder" {
        FOLDER_PICKER_PS
    } else {
        FILE_PICKER_PS
    };

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-STA",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("SOOP_PICKER_INITIAL", initial)
        .env("SOOP_PICKER_FILTER", filter)
        .output()
        .map_err(|e| format!("failed to start Windows PowerShell picker: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("Windows picker exited with {}", output.status)
        } else {
            stderr
        });
    }

    let encoded = String::from_utf8(output.stdout)
        .map_err(|e| format!("picker output was not UTF-8: {e}"))?;
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Ok(None);
    }

    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("picker output decode failed: {e}"))?;
    let path = String::from_utf8(bytes)
        .map_err(|e| format!("picker path decode failed: {e}"))?
        .trim()
        .to_string();

    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path))
    }
}

#[cfg(not(windows))]
fn run_native_picker(_kind: &str, _filter: &str, _initial: &str) -> Result<Option<String>, String> {
    Err("native picker is available only on Windows".into())
}

#[cfg(windows)]
const FOLDER_PICKER_PS: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Application]::EnableVisualStyles()
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'SOOP Recorder - 폴더 선택'
$dialog.ShowNewFolderButton = $true
$initial = [Environment]::GetEnvironmentVariable('SOOP_PICKER_INITIAL')
if ($initial -and [IO.Directory]::Exists($initial)) {
    $dialog.SelectedPath = $initial
}
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($dialog.SelectedPath)
    [Console]::Out.Write([Convert]::ToBase64String($bytes))
}
$dialog.Dispose()
"#;

#[cfg(windows)]
const FILE_PICKER_PS: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Application]::EnableVisualStyles()
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
$mode = [Environment]::GetEnvironmentVariable('SOOP_PICKER_FILTER')
if ($mode -eq 'exe') {
    $dialog.Filter = 'Executable files (*.exe)|*.exe|All files (*.*)|*.*'
} elseif ($mode -eq 'cookie') {
    $dialog.Filter = 'Cookie/text files (*.txt;*.cookies)|*.txt;*.cookies|All files (*.*)|*.*'
} else {
    $dialog.Filter = 'All files (*.*)|*.*'
}
$initial = [Environment]::GetEnvironmentVariable('SOOP_PICKER_INITIAL')
if ($initial) {
    if ([IO.File]::Exists($initial)) {
        $dialog.InitialDirectory = [IO.Path]::GetDirectoryName($initial)
        $dialog.FileName = [IO.Path]::GetFileName($initial)
    } elseif ([IO.Directory]::Exists($initial)) {
        $dialog.InitialDirectory = $initial
    } else {
        $parent = [IO.Path]::GetDirectoryName($initial)
        if ($parent -and [IO.Directory]::Exists($parent)) {
            $dialog.InitialDirectory = $parent
        }
    }
}
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($dialog.FileName)
    [Console]::Out.Write([Convert]::ToBase64String($bytes))
}
$dialog.Dispose()
"#;

#[cfg(test)]
mod tests {
    use super::direct_local_request_allowed;
    use axum::http::{header::HOST, HeaderMap, HeaderValue};

    fn headers(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_str(host).unwrap());
        headers
    }

    #[test]
    fn allows_direct_ipv4_loopback() {
        assert!(direct_local_request_allowed("127.0.0.1:8787", &headers("127.0.0.1:8787")));
    }

    #[test]
    fn allows_direct_localhost() {
        assert!(direct_local_request_allowed("localhost:8787", &headers("localhost:8787")));
    }

    #[test]
    fn rejects_non_loopback_bind() {
        assert!(!direct_local_request_allowed("0.0.0.0:8787", &headers("127.0.0.1:8787")));
    }

    #[test]
    fn rejects_remote_host() {
        assert!(!direct_local_request_allowed("127.0.0.1:8787", &headers("recorder.example.com")));
    }

    #[test]
    fn rejects_reverse_proxy_headers() {
        let mut h = headers("127.0.0.1:8787");
        h.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.10"));
        assert!(!direct_local_request_allowed("127.0.0.1:8787", &h));
    }
}
