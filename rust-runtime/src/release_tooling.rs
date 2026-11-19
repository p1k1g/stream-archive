//! Pure-Rust offline maintenance, release metadata, packaging, and archive verification tooling.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, Local, SecondsFormat, Utc};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::LazyLock,
    time::SystemTime,
};
use uuid::Uuid;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

static RE_HEX_40: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]{40}$").expect("valid regex"));
static RE_SHA256_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9a-fA-F]{64})\s+(.+)$").expect("valid regex"));
static RE_RUN_FORBIDDEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)launcher|RUN_WEB|127\.0\.0\.1|http").expect("valid regex"));

#[cfg(windows)]
fn ensure_windows_console_attached() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    // SAFETY: GetStdHandle and AttachConsole are Win32 console attachment calls
    // invoked once on startup when a GUI-subsystem binary handles CLI flags.
    unsafe {
        let out_missing = GetStdHandle(STD_OUTPUT_HANDLE).is_null();
        let err_missing = GetStdHandle(STD_ERROR_HANDLE).is_null();
        if out_missing && err_missing {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn ensure_windows_console_attached() {}

/// Handles Native GUI CLI maintenance and version flags before Slint startup.
/// Returns `true` when a CLI command ran and the process should exit normally.
pub fn handle_native_cli_args() -> bool {
    handle_cli_args_inner(true)
}

/// Handles shared release/packaging maintenance CLI flags without stealing `stream-archive-cli`'s
/// built-in `version` / `--version` output.
pub fn handle_release_tooling_args() -> bool {
    handle_cli_args_inner(false)
}

fn handle_cli_args_inner(handle_version: bool) -> bool {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(first) = args.first().map(String::as_str) else {
        return false;
    };

    if handle_version && matches!(first, "--version" | "-V") {
        ensure_windows_console_attached();
        println!("Stream Archive {}", env!("CARGO_PKG_VERSION"));
        return true;
    }

    let result = match first {
        "--backup" => {
            ensure_windows_console_attached();
            cmd_offline_backup(&args[1..])
        }
        "--restore" => {
            ensure_windows_console_attached();
            cmd_offline_restore(&args[1..])
        }
        "--write-release-metadata" => {
            ensure_windows_console_attached();
            cmd_write_release_metadata(&args[1..])
        }
        "--write-sha256sums" => {
            ensure_windows_console_attached();
            cmd_write_sha256sums(&args[1..])
        }
        "--verify-windows-package" => {
            ensure_windows_console_attached();
            cmd_verify_windows_package(&args[1..])
        }
        "--new-windows-release-archive" => {
            ensure_windows_console_attached();
            cmd_new_windows_release_archive(&args[1..])
        }
        "--copy-dir" => {
            ensure_windows_console_attached();
            cmd_copy_dir(&args[1..])
        }
        _ => return false,
    };

    if let Err(err) = result {
        eprintln!("[ERROR] {err:#}");
        std::process::exit(1);
    }
    true
}

fn normalize_flag(raw: &str) -> Option<String> {
    if !raw.starts_with('-') {
        return None;
    }
    let stripped = raw.trim_start_matches('-');
    if stripped.is_empty() {
        return None;
    }
    Some(
        stripped
            .chars()
            .filter(|ch| *ch != '-' && *ch != '_')
            .flat_map(char::to_lowercase)
            .collect(),
    )
}

fn next_flag_value(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .with_context(|| format!("missing value for {flag}"))
}

fn cmd_copy_dir(args: &[String]) -> Result<()> {
    if args.len() != 2 {
        bail!("usage: --copy-dir <source-dir> <dest-dir>");
    }
    copy_dir_recursive(Path::new(&args[0]), Path::new(&args[1]))
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        bail!("source directory does not exist: {}", src.display());
    }
    fs::create_dir_all(dst)
        .with_context(|| format!("failed to create directory {}", dst.display()))?;
    for entry in
        fs::read_dir(src).with_context(|| format!("failed to list directory {}", src.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to).with_context(|| {
                format!("failed to copy {} to {}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

fn resolve_default_app_root() -> Result<PathBuf> {
    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        if exe_dir.join("data").is_dir() || exe_dir.join("StreamArchive.exe").is_file() {
            return Ok(exe_dir.to_path_buf());
        }
        if exe_dir.file_name().and_then(|n| n.to_str()) == Some("maintenance")
            && let Some(parent) = exe_dir.parent()
        {
            return Ok(parent.to_path_buf());
        }
    }
    Ok(env::current_dir()?)
}

fn resolve_offline_data_dir(requested: Option<&str>) -> Result<PathBuf> {
    let candidate = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("STREAM_ARCHIVE_DATA_DIR")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
    let raw = match candidate {
        Some(path) => path,
        None => resolve_default_app_root()?.join("data"),
    };
    absolutize(&raw)
}

fn resolve_offline_backup_dir(requested: Option<&str>) -> Result<PathBuf> {
    let candidate = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("STREAM_ARCHIVE_BACKUP_DIR")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
    let raw = match candidate {
        Some(path) => path,
        None => {
            let app_root = resolve_default_app_root()?;
            let parent = app_root.parent().unwrap_or(&app_root);
            parent.join("stream-archive-backups")
        }
    };
    absolutize(&raw)
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn assert_sqlite_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("SQLite database not found: {}", path.display());
    }
    let mut file = File::open(path)
        .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
    let meta = file.metadata()?;
    if meta.len() < 16 {
        bail!("SQLite database is too small: {}", path.display());
    }
    let mut header = [0u8; 16];
    file.read_exact(&mut header)?;
    if &header != SQLITE_HEADER {
        bail!("Invalid SQLite header: {}", path.display());
    }
    Ok(())
}

fn assert_runtime_stopped(db_path: &Path, check_global_processes: bool) -> Result<()> {
    if db_path.parent().is_some_and(|p| p.exists())
        && crate::runtime_owner::runtime_owner_active(db_path)?
    {
        bail!(
            "Stream Archive is running. Close StreamArchive.exe and stop the optional headless runtime before backing up or restoring SQLite."
        );
    }

    #[cfg(windows)]
    {
        if check_global_processes && windows_runtime_process_running()? {
            bail!(
                "Stream Archive is running. Close StreamArchive.exe and stop the optional headless runtime before backing up or restoring SQLite."
            );
        }
    }

    #[cfg(not(windows))]
    let _ = check_global_processes;

    Ok(())
}

#[cfg(windows)]
fn windows_runtime_process_running() -> Result<bool> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };

    let current_pid = std::process::id();
    // SAFETY: ToolHelp snapshot enumeration of running processes only compares executable filenames.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Ok(false);
    }
    struct HandleGuard(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: snapshot handle was returned non-invalid by CreateToolhelp32Snapshot.
            unsafe { CloseHandle(self.0) };
        }
    }
    let _guard = HandleGuard(snapshot);

    // SAFETY: PROCESSENTRY32W is POD and dwSize is initialized before calling Process32FirstW.
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: snapshot and entry are valid.
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return Ok(false);
    }

    loop {
        if entry.th32ProcessID != current_pid {
            let len = entry
                .szExeFile
                .iter()
                .position(|&ch| ch == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case("StreamArchive.exe")
                || name.eq_ignore_ascii_case("stream-archive-gui.exe")
                || name.eq_ignore_ascii_case("stream-archive-server.exe")
            {
                return Ok(true);
            }
        }
        // SAFETY: snapshot and entry are valid for continuing enumeration.
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            break;
        }
    }
    Ok(false)
}

fn sha256_file_hex(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn cmd_offline_backup(args: &[String]) -> Result<()> {
    let mut data_dir: Option<String> = None;
    let mut backup_dir: Option<String> = None;
    let mut keep: usize = 10;
    let mut retention_days: i64 = 30;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("datadir") => data_dir = Some(next_flag_value(args, &mut index, arg)?),
            Some("backupdir") => backup_dir = Some(next_flag_value(args, &mut index, arg)?),
            Some("keep") => {
                keep = next_flag_value(args, &mut index, arg)?
                    .parse()
                    .context("invalid --keep value")?;
            }
            Some("retentiondays") => {
                retention_days = next_flag_value(args, &mut index, arg)?
                    .parse()
                    .context("invalid --retention-days value")?;
            }
            Some(other) => bail!("unknown --backup option: --{other}"),
            None => bail!("unexpected argument for --backup: {arg}"),
        }
        index += 1;
    }

    let data_root = resolve_offline_data_dir(data_dir.as_deref())?;
    let backup_root = resolve_offline_backup_dir(backup_dir.as_deref())?;
    let (backup_path, hash) = offline_backup(&data_root, &backup_root, keep, retention_days)?;
    println!("Backup complete: {}", backup_path.display());
    println!("SHA256: {hash}");
    Ok(())
}

pub fn offline_backup(
    data_root: &Path,
    backup_root: &Path,
    keep: usize,
    retention_days: i64,
) -> Result<(PathBuf, String)> {
    offline_backup_inner(data_root, backup_root, keep, retention_days, true)
}

fn offline_backup_inner(
    data_root: &Path,
    backup_root: &Path,
    keep: usize,
    retention_days: i64,
    check_global_processes: bool,
) -> Result<(PathBuf, String)> {
    let db_path = data_root.join("stream-archive.db");
    assert_runtime_stopped(&db_path, check_global_processes)?;
    assert_sqlite_file(&db_path)?;

    fs::create_dir_all(backup_root).with_context(|| {
        format!(
            "failed to create backup directory {}",
            backup_root.display()
        )
    })?;

    let stamp = Local::now().format("%Y%m%d_%H%M%S");
    let mut backup_path = backup_root.join(format!("stream_archive_manual_{stamp}.db"));
    if backup_path.exists() {
        let millis = Local::now().timestamp_subsec_millis();
        backup_path = backup_root.join(format!("stream_archive_manual_{stamp}_{millis:03}.db"));
    }

    fs::copy(&db_path, &backup_path).with_context(|| {
        format!(
            "failed to copy {} to {}",
            db_path.display(),
            backup_path.display()
        )
    })?;
    if let Err(err) = assert_sqlite_file(&backup_path) {
        let _ = fs::remove_file(&backup_path);
        return Err(err);
    }

    let hash = sha256_file_hex(&backup_path)?;
    let size_bytes = fs::metadata(&backup_path)?.len();
    let meta = serde_json::json!({
        "created_at": Local::now().to_rfc3339(),
        "source": db_path.display().to_string(),
        "sha256": hash,
        "size_bytes": size_bytes,
        "kind": "manual",
        "version": 1,
    });
    let meta_path = PathBuf::from(format!("{}.json", backup_path.display()));
    fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)
        .with_context(|| format!("failed to write backup metadata {}", meta_path.display()))?;

    prune_owned_backups(backup_root, keep, retention_days)?;
    Ok((backup_path, hash))
}

fn owned_backup_entries(backup_root: &Path) -> Result<Vec<(PathBuf, SystemTime)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backup_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.starts_with("stream_archive_") && name.ends_with(".db")) {
            continue;
        }
        let meta_path = PathBuf::from(format!("{}.json", path.display()));
        if !meta_path.is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push((path, modified));
    }
    entries.sort_by_key(|a| std::cmp::Reverse(a.1));
    Ok(entries)
}

fn remove_backup_pair(db_path: &Path) {
    let _ = fs::remove_file(db_path);
    let meta_path = PathBuf::from(format!("{}.json", db_path.display()));
    if meta_path.exists() {
        let _ = fs::remove_file(meta_path);
    }
}

fn prune_owned_backups(backup_root: &Path, keep: usize, retention_days: i64) -> Result<()> {
    if keep > 0 {
        let entries = owned_backup_entries(backup_root)?;
        for (path, _) in entries.into_iter().skip(keep) {
            remove_backup_pair(&path);
        }
    }
    if retention_days > 0 {
        let cutoff = Utc::now() - ChronoDuration::days(retention_days);
        for (path, modified) in owned_backup_entries(backup_root)? {
            let modified_dt: DateTime<Utc> = modified.into();
            if modified_dt < cutoff {
                remove_backup_pair(&path);
            }
        }
    }
    Ok(())
}

fn cmd_offline_restore(args: &[String]) -> Result<()> {
    let mut backup_file: Option<String> = None;
    let mut data_dir: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("backupfile") => backup_file = Some(next_flag_value(args, &mut index, arg)?),
            Some("datadir") => data_dir = Some(next_flag_value(args, &mut index, arg)?),
            Some(other) => bail!("unknown --restore option: --{other}"),
            None if backup_file.is_none() => backup_file = Some(arg.clone()),
            None => bail!("unexpected argument for --restore: {arg}"),
        }
        index += 1;
    }

    let backup_file =
        backup_file.context("usage: --restore --backup-file <path> [--data-dir <dir>]")?;
    let source = absolutize(Path::new(&backup_file))?;
    let data_root = resolve_offline_data_dir(data_dir.as_deref())?;
    let safety_dir = resolve_offline_backup_dir(None)?;
    let target = offline_restore(&source, &data_root, Some(&safety_dir))?;
    println!("Restore complete: {}", target.display());
    println!("Start Stream Archive and verify settings, channels, and history.");
    Ok(())
}

pub fn offline_restore(
    source: &Path,
    data_root: &Path,
    safety_backup_dir: Option<&Path>,
) -> Result<PathBuf> {
    offline_restore_inner(source, data_root, safety_backup_dir, true)
}

fn offline_restore_inner(
    source: &Path,
    data_root: &Path,
    safety_backup_dir: Option<&Path>,
    check_global_processes: bool,
) -> Result<PathBuf> {
    let target = data_root.join("stream-archive.db");
    assert_runtime_stopped(&target, check_global_processes)?;
    assert_sqlite_file(source)?;

    let metadata_path = PathBuf::from(format!("{}.json", source.display()));
    if metadata_path.is_file() {
        let raw = fs::read_to_string(&metadata_path).with_context(|| {
            format!("failed to read backup metadata {}", metadata_path.display())
        })?;
        let metadata: serde_json::Value = serde_json::from_str(&raw).with_context(|| {
            format!(
                "failed to parse backup metadata {}",
                metadata_path.display()
            )
        })?;
        if let Some(expected) = metadata
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let actual = sha256_file_hex(source)?;
            if !actual.eq_ignore_ascii_case(expected) {
                bail!("Backup SHA256 does not match its metadata file.");
            }
        }
    }

    fs::create_dir_all(data_root)
        .with_context(|| format!("failed to create data directory {}", data_root.display()))?;

    if target.is_file() {
        let stamp = Local::now().format("%Y%m%d_%H%M%S");
        let safety_dir = match safety_backup_dir {
            Some(dir) => dir.to_path_buf(),
            None => resolve_offline_backup_dir(None)?,
        };
        fs::create_dir_all(&safety_dir)?;
        let safety = safety_dir.join(format!("stream_archive_pre_restore_{stamp}.db"));
        fs::copy(&target, &safety).with_context(|| {
            format!(
                "failed to create pre-restore safety backup {}",
                safety.display()
            )
        })?;
        println!("Current database safety copy: {}", safety.display());
    }

    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", target.display(), suffix));
        if sidecar.exists() {
            fs::remove_file(&sidecar).with_context(|| {
                format!("failed to remove SQLite sidecar {}", sidecar.display())
            })?;
        }
    }

    let temp = PathBuf::from(format!("{}.restore.tmp", target.display()));
    fs::copy(source, &temp)
        .with_context(|| format!("failed to stage restore copy {}", temp.display()))?;
    if let Err(err) = assert_sqlite_file(&temp) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    fs::rename(&temp, &target).with_context(|| {
        format!(
            "failed to replace database {} with {}",
            target.display(),
            temp.display()
        )
    })?;
    Ok(target)
}

fn cmd_write_release_metadata(args: &[String]) -> Result<()> {
    let mut output_path: Option<String> = None;
    let mut manifest_path = String::from(r".\rust-runtime\Cargo.toml");
    let mut repository_root = String::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("outputpath") => output_path = Some(next_flag_value(args, &mut index, arg)?),
            Some("manifestpath") => manifest_path = next_flag_value(args, &mut index, arg)?,
            Some("repositoryroot") => repository_root = next_flag_value(args, &mut index, arg)?,
            Some(other) => bail!("unknown --write-release-metadata option: --{other}"),
            None => bail!("unexpected argument for --write-release-metadata: {arg}"),
        }
        index += 1;
    }

    let output_path =
        output_path.context("usage: --write-release-metadata --output-path <path>")?;
    let repo_root_opt =
        (!repository_root.trim().is_empty()).then_some(Path::new(repository_root.trim()));
    let (version, commit) = write_release_metadata(
        Path::new(&output_path),
        Path::new(&manifest_path),
        repo_root_opt,
    )?;
    println!("Release metadata written: version={version} commit={commit}");
    Ok(())
}

pub fn resolve_package_version(manifest_path: &Path, package_name: &str) -> Result<String> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest_path)
        .output();
    if let Ok(output) = output {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cargo metadata failed ({}): {stderr}", output.status);
        }
        let doc: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("invalid cargo metadata JSON")?;
        if let Some(version) = doc
            .get("packages")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|pkg| pkg.get("name").and_then(|n| n.as_str()) == Some(package_name))
            })
            .and_then(|pkg| pkg.get("version"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(version.trim().to_string());
        }
        bail!("Unable to resolve {package_name} package version from cargo metadata");
    }

    // Fallback: parse [package] version directly from Cargo.toml when cargo isn't in PATH.
    let toml = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
    let re = Regex::new(r#"(?m)^version\s*=\s*"([^"]+)""#).expect("valid regex");
    re.captures(&toml)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
        .filter(|s| !s.is_empty())
        .with_context(|| {
            format!(
                "Unable to resolve {package_name} package version from {}",
                manifest_path.display()
            )
        })
}

fn resolve_git_dir_entry(base: &Path, raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    let path = PathBuf::from(trimmed);
    let joined = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    fs::canonicalize(&joined).or_else(|_| Ok(joined))
}

pub fn git_head_commit_from_metadata(root: &Path) -> Option<String> {
    let git_entry = root.join(".git");
    let git_dir = if git_entry.is_dir() {
        fs::canonicalize(&git_entry).unwrap_or(git_entry)
    } else if git_entry.is_file() {
        let first_line = fs::read_to_string(&git_entry)
            .ok()?
            .lines()
            .next()?
            .trim()
            .to_string();
        let value = first_line.strip_prefix("gitdir:")?.trim();
        resolve_git_dir_entry(root, value).ok()?
    } else {
        return None;
    };

    let common_dir = {
        let commondir_path = git_dir.join("commondir");
        if commondir_path.is_file() {
            let value = fs::read_to_string(&commondir_path)
                .ok()
                .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
                .filter(|s| !s.is_empty());
            match value {
                Some(rel) => {
                    resolve_git_dir_entry(&git_dir, &rel).unwrap_or_else(|_| git_dir.clone())
                }
                None => git_dir.clone(),
            }
        } else {
            git_dir.clone()
        }
    };

    let head_path = git_dir.join("HEAD");
    let head = fs::read_to_string(&head_path)
        .ok()?
        .lines()
        .next()?
        .trim()
        .to_string();
    if RE_HEX_40.is_match(&head) {
        return Some(head.to_ascii_lowercase());
    }
    let ref_name = head.strip_prefix("ref:")?.trim();
    let mut ref_roots = vec![git_dir.clone()];
    if common_dir != git_dir {
        ref_roots.push(common_dir.clone());
    }
    for ref_root in ref_roots {
        let mut ref_path = ref_root;
        for segment in ref_name.split('/') {
            ref_path.push(segment);
        }
        if ref_path.is_file()
            && let Ok(contents) = fs::read_to_string(&ref_path)
            && let Some(line) = contents.lines().next()
        {
            let trimmed = line.trim();
            if RE_HEX_40.is_match(trimmed) {
                return Some(trimmed.to_ascii_lowercase());
            }
        }
    }

    let packed_refs = common_dir.join("packed-refs");
    if packed_refs.is_file()
        && let Ok(contents) = fs::read_to_string(&packed_refs)
    {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some((sha, name)) = line.split_once(char::is_whitespace)
                && RE_HEX_40.is_match(sha)
                && name.trim() == ref_name
            {
                return Some(sha.to_ascii_lowercase());
            }
        }
    }

    None
}

fn strip_windows_verbatim_prefix(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    }
}

pub fn write_release_metadata(
    output_path: &Path,
    manifest_path: &Path,
    repository_root: Option<&Path>,
) -> Result<(String, String)> {
    let version = resolve_package_version(manifest_path, "stream-archive-server")?;

    let mut commit = String::from("unknown");
    let resolved_root = match repository_root {
        Some(root) => {
            if !root.is_dir() {
                bail!("RepositoryRoot does not exist: {}", root.display());
            }
            Some(fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()))
        }
        None => None,
    };

    if let Some(root) = resolved_root.as_ref() {
        let git_head_commit = git_head_commit_from_metadata(root);
        let mut provenance_commit = git_head_commit
            .as_deref()
            .filter(|sha| RE_HEX_40.is_match(sha))
            .map(str::to_string);

        if env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true")
            && let Some(workspace) = env::var("GITHUB_WORKSPACE")
                .ok()
                .filter(|s| !s.trim().is_empty())
        {
            let workspace_path = PathBuf::from(workspace);
            if workspace_path.is_dir() {
                let resolved_workspace =
                    fs::canonicalize(&workspace_path).unwrap_or(workspace_path);
                if resolved_workspace == *root {
                    let sha_ok = env::var("GITHUB_SHA")
                        .ok()
                        .filter(|sha| RE_HEX_40.is_match(sha.trim()))
                        .is_some_and(|sha| {
                            git_head_commit
                                .as_deref()
                                .is_some_and(|head| head == sha.trim().to_ascii_lowercase())
                        });
                    if !sha_ok {
                        provenance_commit = None;
                    }
                }
            }
        }

        if let Some(provenance) = provenance_commit {
            let root_for_git = strip_windows_verbatim_prefix(root);
            let safe_dir_arg = format!("safe.directory={root_for_git}");
            if let Ok(output) = Command::new("git")
                .args([
                    "-c",
                    &safe_dir_arg,
                    "-C",
                    &root_for_git,
                    "status",
                    "--porcelain",
                    "--untracked-files=normal",
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                && output.status.success()
            {
                commit = provenance[..12].to_string();
                let dirty = String::from_utf8_lossy(&output.stdout);
                if dirty.lines().any(|line| !line.is_empty()) {
                    commit.push_str("-dirty");
                }
            }
        }
    }

    if let Some(parent) = output_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    let built_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let content = format!(
        "product=Stream Archive\nversion={version}\ncommit={commit}\nbuilt_at={built_at}\n"
    );
    fs::write(output_path, content.as_bytes())
        .with_context(|| format!("failed to write release metadata {}", output_path.display()))?;

    Ok((version, commit))
}

fn cmd_write_sha256sums(args: &[String]) -> Result<()> {
    let mut root: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("root") => root = Some(next_flag_value(args, &mut index, arg)?),
            Some(other) => bail!("unknown --write-sha256sums option: --{other}"),
            None => files.push(arg.clone()),
        }
        index += 1;
    }

    let root = root.context("usage: --write-sha256sums --root <dir> [files...]")?;
    if files.is_empty() {
        files.push("StreamArchive.exe".to_string());
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    write_sha256sums(Path::new(&root), &refs)
}

pub fn write_sha256sums(root: &Path, relative_files: &[&str]) -> Result<()> {
    let mut out = String::new();
    for rel in relative_files {
        let path = root.join(rel);
        let hash = sha256_file_hex(&path)?;
        out.push_str(&format!("{hash}  {rel}\n"));
    }
    fs::write(root.join("SHA256SUMS.txt"), out.as_bytes()).context("failed to write SHA256SUMS.txt")
}

fn collect_zip_items(
    root: &Path,
    current: &Path,
    dirs: &mut Vec<String>,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(current)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .context("zip entry escaped root")?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !rel.is_empty() {
                dirs.push(format!("{rel}/"));
            }
            collect_zip_items(root, &path, dirs, files)?;
        } else if file_type.is_file() {
            files.push((rel, path));
        }
    }
    Ok(())
}

struct ZipCdEntry {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    external_attrs: u32,
    local_header_offset: u32,
}

struct ZipEntrySpec<'a> {
    name: &'a str,
    method: u16,
    crc: u32,
    uncompressed_size: u32,
    payload: &'a [u8],
    external_attrs: u32,
}

fn write_zip_entry(out: &mut File, offset: &mut u64, spec: ZipEntrySpec<'_>) -> Result<ZipCdEntry> {
    let name_bytes = spec.name.as_bytes();
    let name_len = u16::try_from(name_bytes.len()).context("zip path too long")?;
    let compressed_size = u32::try_from(spec.payload.len()).context("zip entry exceeds 4GiB")?;
    let local_offset = u32::try_from(*offset).context("zip archive exceeds 4GiB")?;

    let dos_time: u16 = 0;
    let dos_date: u16 = 0x5421; // 2022-01-01
    let flags: u16 = 0x0800; // UTF-8 filename

    let mut header = Vec::with_capacity(30 + name_bytes.len());
    header.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    header.extend_from_slice(&20u16.to_le_bytes());
    header.extend_from_slice(&flags.to_le_bytes());
    header.extend_from_slice(&spec.method.to_le_bytes());
    header.extend_from_slice(&dos_time.to_le_bytes());
    header.extend_from_slice(&dos_date.to_le_bytes());
    header.extend_from_slice(&spec.crc.to_le_bytes());
    header.extend_from_slice(&compressed_size.to_le_bytes());
    header.extend_from_slice(&spec.uncompressed_size.to_le_bytes());
    header.extend_from_slice(&name_len.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(name_bytes);

    out.write_all(&header)?;
    out.write_all(spec.payload)?;
    *offset += header.len() as u64 + spec.payload.len() as u64;

    Ok(ZipCdEntry {
        name: name_bytes.to_vec(),
        method: spec.method,
        crc: spec.crc,
        compressed_size,
        uncompressed_size: spec.uncompressed_size,
        external_attrs: spec.external_attrs,
        local_header_offset: local_offset,
    })
}

pub fn create_zip_archive(root_dir: &Path, archive_path: &Path) -> Result<()> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    collect_zip_items(root_dir, root_dir, &mut dirs, &mut files)?;

    if let Some(parent) = archive_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut out = File::create(archive_path)
        .with_context(|| format!("failed to create zip archive {}", archive_path.display()))?;
    let mut offset = 0u64;
    let mut cd_entries = Vec::with_capacity(dirs.len() + files.len());

    for dir_name in dirs {
        cd_entries.push(write_zip_entry(
            &mut out,
            &mut offset,
            ZipEntrySpec {
                name: &dir_name,
                method: 0,
                crc: 0,
                uncompressed_size: 0,
                payload: &[],
                external_attrs: 0x10,
            },
        )?);
    }

    for (rel, path) in files {
        let raw = fs::read(&path)
            .with_context(|| format!("failed to read {} for archive", path.display()))?;
        let uncompressed_size = u32::try_from(raw.len()).context("zip input exceeds 4GiB")?;
        let crc = crc32fast::hash(&raw);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&raw)?;
        let compressed = encoder.finish()?;
        cd_entries.push(write_zip_entry(
            &mut out,
            &mut offset,
            ZipEntrySpec {
                name: &rel,
                method: 8,
                crc,
                uncompressed_size,
                payload: &compressed,
                external_attrs: 0x20,
            },
        )?);
    }

    let cd_offset = u32::try_from(offset).context("zip archive exceeds 4GiB")?;
    let mut cd_buf = Vec::new();
    for entry in &cd_entries {
        let name_len = entry.name.len() as u16;
        cd_buf.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd_buf.extend_from_slice(&20u16.to_le_bytes());
        cd_buf.extend_from_slice(&20u16.to_le_bytes());
        cd_buf.extend_from_slice(&0x0800u16.to_le_bytes());
        cd_buf.extend_from_slice(&entry.method.to_le_bytes());
        cd_buf.extend_from_slice(&0u16.to_le_bytes());
        cd_buf.extend_from_slice(&0x5421u16.to_le_bytes());
        cd_buf.extend_from_slice(&entry.crc.to_le_bytes());
        cd_buf.extend_from_slice(&entry.compressed_size.to_le_bytes());
        cd_buf.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        cd_buf.extend_from_slice(&name_len.to_le_bytes());
        cd_buf.extend_from_slice(&0u16.to_le_bytes());
        cd_buf.extend_from_slice(&0u16.to_le_bytes());
        cd_buf.extend_from_slice(&0u16.to_le_bytes());
        cd_buf.extend_from_slice(&0u16.to_le_bytes());
        cd_buf.extend_from_slice(&entry.external_attrs.to_le_bytes());
        cd_buf.extend_from_slice(&entry.local_header_offset.to_le_bytes());
        cd_buf.extend_from_slice(&entry.name);
    }
    out.write_all(&cd_buf)?;

    let cd_size = u32::try_from(cd_buf.len()).context("central directory too large")?;
    let total_entries = u16::try_from(cd_entries.len()).context("too many zip entries")?;
    let mut eocd = [0u8; 22];
    eocd[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
    eocd[8..10].copy_from_slice(&total_entries.to_le_bytes());
    eocd[10..12].copy_from_slice(&total_entries.to_le_bytes());
    eocd[12..16].copy_from_slice(&cd_size.to_le_bytes());
    eocd[16..20].copy_from_slice(&cd_offset.to_le_bytes());
    out.write_all(&eocd)?;
    out.flush()?;
    Ok(())
}

fn read_le_u16(data: &[u8], offset: usize) -> Result<u16> {
    let slice: [u8; 2] = data
        .get(offset..offset + 2)
        .context("truncated zip header")?
        .try_into()?;
    Ok(u16::from_le_bytes(slice))
}

fn read_le_u32(data: &[u8], offset: usize) -> Result<u32> {
    let slice: [u8; 4] = data
        .get(offset..offset + 4)
        .context("truncated zip header")?
        .try_into()?;
    Ok(u32::from_le_bytes(slice))
}

fn safe_archive_rel_path(raw: &str) -> Result<PathBuf> {
    let mut clean = PathBuf::new();
    for segment in raw.split(['/', '\\']).filter(|s| !s.is_empty()) {
        if segment == "." || segment == ".." {
            bail!("unsafe zip entry path: {raw}");
        }
        let candidate = Path::new(segment);
        if candidate
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("unsafe zip entry path: {raw}");
        }
        clean.push(segment);
    }
    Ok(clean)
}

pub fn extract_zip_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let bytes = fs::read(archive_path)
        .with_context(|| format!("failed to read zip archive {}", archive_path.display()))?;
    if bytes.len() < 22 {
        bail!(
            "Corrupted or truncated zip archive: {}",
            archive_path.display()
        );
    }
    let min_eocd_pos = bytes.len().saturating_sub(22 + u16::MAX as usize);
    let eocd_pos = (min_eocd_pos..=bytes.len() - 22)
        .rev()
        .find(|&pos| {
            read_le_u32(&bytes, pos).ok() == Some(0x0605_4b50)
                && usize::from(read_le_u16(&bytes, pos + 20).unwrap_or(u16::MAX))
                    == bytes.len() - pos - 22
        })
        .context("Invalid or corrupted zip End of Central Directory")?;

    let entry_count = usize::from(read_le_u16(&bytes, eocd_pos + 10)?);
    let cd_size = read_le_u32(&bytes, eocd_pos + 12)? as usize;
    let cd_offset = read_le_u32(&bytes, eocd_pos + 16)? as usize;
    if cd_offset + cd_size != eocd_pos {
        bail!("Corrupted zip central directory bounds");
    }

    fs::create_dir_all(dest_dir)?;
    let mut cursor = cd_offset;
    for _ in 0..entry_count {
        if cursor + 46 > eocd_pos || read_le_u32(&bytes, cursor)? != 0x0201_4b50 {
            bail!("Corrupted zip central directory header");
        }
        let method = read_le_u16(&bytes, cursor + 10)?;
        let expected_crc = read_le_u32(&bytes, cursor + 16)?;
        let comp_size = read_le_u32(&bytes, cursor + 20)? as usize;
        let uncomp_size = read_le_u32(&bytes, cursor + 24)? as usize;
        let name_len = usize::from(read_le_u16(&bytes, cursor + 28)?);
        let extra_len = usize::from(read_le_u16(&bytes, cursor + 30)?);
        let comment_len = usize::from(read_le_u16(&bytes, cursor + 32)?);
        let local_offset = read_le_u32(&bytes, cursor + 42)? as usize;

        let name_bytes = bytes
            .get(cursor + 46..cursor + 46 + name_len)
            .context("truncated zip entry filename")?;
        let raw_name = std::str::from_utf8(name_bytes).context("non-UTF8 zip entry name")?;
        let is_dir = raw_name.ends_with('/') || raw_name.ends_with('\\');
        let rel_path = safe_archive_rel_path(raw_name)?;
        cursor += 46 + name_len + extra_len + comment_len;

        if local_offset + 30 > cd_offset || read_le_u32(&bytes, local_offset)? != 0x0403_4b50 {
            bail!("Corrupted zip local file header for {raw_name}");
        }
        let local_name_len = usize::from(read_le_u16(&bytes, local_offset + 26)?);
        let local_extra_len = usize::from(read_le_u16(&bytes, local_offset + 28)?);
        let data_start = local_offset + 30 + local_name_len + local_extra_len;
        let payload = bytes
            .get(data_start..data_start + comp_size)
            .context("truncated zip entry payload")?;

        if rel_path.as_os_str().is_empty() {
            continue;
        }
        let target = dest_dir.join(&rel_path);
        if is_dir {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let decoded = match method {
            0 => payload.to_vec(),
            8 => {
                let mut decoder = DeflateDecoder::new(payload);
                let mut out = Vec::with_capacity(uncomp_size);
                decoder.read_to_end(&mut out)?;
                out
            }
            other => bail!("unsupported zip compression method {other} for {raw_name}"),
        };
        if decoded.len() != uncomp_size || crc32fast::hash(&decoded) != expected_crc {
            bail!("zip CRC/length mismatch for {raw_name}");
        }
        fs::write(&target, &decoded)?;
    }

    Ok(())
}

fn cmd_verify_windows_package(args: &[String]) -> Result<()> {
    let mut root = String::from(r".\dist\stream-archive");
    let mut require_clean_data = false;
    let mut archive_path: Option<String> = None;
    let mut archive_checksum_path: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("root") => root = next_flag_value(args, &mut index, arg)?,
            Some("requirecleandata") => require_clean_data = true,
            Some("archivepath") => archive_path = Some(next_flag_value(args, &mut index, arg)?),
            Some("archivechecksumpath") => {
                archive_checksum_path = Some(next_flag_value(args, &mut index, arg)?)
            }
            Some(other) => bail!("unknown --verify-windows-package option: --{other}"),
            None => bail!("unexpected argument for --verify-windows-package: {arg}"),
        }
        index += 1;
    }

    verify_windows_package(
        Path::new(&root),
        require_clean_data,
        archive_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(Path::new),
        archive_checksum_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(Path::new),
    )
}

pub fn verify_windows_package(
    root: &Path,
    require_clean_data: bool,
    archive_path: Option<&Path>,
    archive_checksum_path: Option<&Path>,
) -> Result<()> {
    verify_windows_package_tree(root, require_clean_data)?;

    if let Some(archive_path) = archive_path {
        if !archive_path.is_file() {
            bail!("Archive does not exist: {}", archive_path.display());
        }
        let default_checksum = PathBuf::from(format!("{}.sha256", archive_path.display()));
        let checksum_path = archive_checksum_path.unwrap_or(&default_checksum);
        if !checksum_path.is_file() {
            bail!(
                "Archive checksum does not exist: {}",
                checksum_path.display()
            );
        }

        let first_line = fs::read_to_string(checksum_path)?
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        let caps = RE_SHA256_LINE
            .captures(&first_line)
            .context("Archive checksum format is invalid")?;
        let expected_hash = caps[1].to_ascii_lowercase();
        let actual_hash = sha256_file_hex(archive_path)?;
        if expected_hash != actual_hash {
            bail!("Archive checksum mismatch");
        }

        let temp_root = env::var("RUNNER_TEMP")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir);
        let scratch = temp_root.join(format!(
            "Stream Archive Release 테스트-{}",
            Uuid::new_v4().simple()
        ));
        let verify_result = (|| -> Result<()> {
            extract_zip_archive(archive_path, &scratch)?;
            verify_windows_package_tree(&scratch, true)
        })();
        let _ = fs::remove_dir_all(&scratch);
        verify_result?;
        println!("Windows archive verified: {}", archive_path.display());
    }

    Ok(())
}

fn verify_windows_package_tree(package_root: &Path, clean_data: bool) -> Result<()> {
    let resolved = fs::canonicalize(package_root)
        .with_context(|| format!("Missing package root: {}", package_root.display()))?;

    let required_files = [
        "StreamArchive.exe",
        "RUN.bat",
        "BACKUP_DATA.bat",
        "RESTORE_DATA.bat",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        "RELEASE_INFO.txt",
        "SHA256SUMS.txt",
        r"docs\OPERATIONS.md",
    ];
    for rel in required_files {
        let p = resolved.join(rel.replace('\\', "/"));
        if !p.is_file() {
            bail!("Missing package file: {}", p.display());
        }
    }

    for rel in ["backend", "backend/vod", "data", "docs"] {
        let p = resolved.join(rel);
        if !p.is_dir() {
            bail!("Missing package directory: {}", p.display());
        }
    }

    for rel in [
        "stream-archive-server.exe",
        "RUN_HEADLESS.bat",
        "stream-archive-icon.png",
        "stream-archive.ico",
        "stream-archive-launcher.exe",
        "RUN_WEB.bat",
        "RUN_SERVER_CONSOLE.bat",
        "Caddyfile.example",
        "docs/REVERSE_PROXY.md",
        "docs/LOCAL_LAUNCHER.md",
        "maintenance/Backup-StreamArchiveData.ps1",
        "maintenance/Restore-StreamArchiveData.ps1",
    ] {
        if resolved.join(rel).exists() {
            bail!("Forbidden Windows package file returned: {rel}");
        }
    }

    let forbidden_names: BTreeSet<&str> = [
        "streamlink.exe",
        "yt-dlp.exe",
        "ffmpeg.exe",
        "SOOP_LIVE_SETTING.ini",
        "SOOP_LIVE_CHANNELS.txt",
        "SOOP_VOD_SETTING.ini",
    ]
    .into_iter()
    .collect();
    scan_forbidden_package_files(&resolved, &forbidden_names, clean_data)?;

    if clean_data {
        let data_root = resolved.join("data");
        if let Some(first) = fs::read_dir(&data_root)?.next() {
            bail!(
                "Official package data directory must be empty: {}",
                first?.path().display()
            );
        }
    }

    let mut expected_sums = BTreeMap::new();
    for line in fs::read_to_string(resolved.join("SHA256SUMS.txt"))?.lines() {
        if let Some(caps) = RE_SHA256_LINE.captures(line) {
            expected_sums.insert(caps[2].trim().to_string(), caps[1].to_ascii_lowercase());
        }
    }
    let exe_path = resolved.join("StreamArchive.exe");
    let expected_exe_sum = expected_sums
        .get("StreamArchive.exe")
        .context("checksum missing: StreamArchive.exe")?;
    let actual_exe_sum = sha256_file_hex(&exe_path)?;
    if expected_exe_sum != &actual_exe_sum {
        bail!("StreamArchive.exe checksum mismatch");
    }

    #[cfg(windows)]
    assert_embedded_windows_resources(&exe_path)?;

    let run_bat = fs::read_to_string(resolved.join("RUN.bat"))?;
    if !run_bat.contains("StreamArchive.exe") {
        bail!("RUN.bat does not use StreamArchive.exe");
    }
    if RE_RUN_FORBIDDEN.is_match(&run_bat) {
        bail!("RUN.bat must remain native-only");
    }

    let backup_bat = fs::read_to_string(resolved.join("BACKUP_DATA.bat"))?;
    if !backup_bat.contains("StreamArchive.exe") || !backup_bat.contains("--backup") {
        bail!("BACKUP_DATA.bat must invoke StreamArchive.exe --backup");
    }
    let restore_bat = fs::read_to_string(resolved.join("RESTORE_DATA.bat"))?;
    if !restore_bat.contains("StreamArchive.exe") || !restore_bat.contains("--restore") {
        bail!("RESTORE_DATA.bat must invoke StreamArchive.exe --restore");
    }

    let release_info = fs::read_to_string(resolved.join("RELEASE_INFO.txt"))?;
    let has_product = release_info
        .lines()
        .any(|l| l.trim_start_matches('\u{feff}').trim() == "product=Stream Archive");
    if !has_product {
        bail!("release metadata product missing");
    }
    let metadata_version = release_info
        .lines()
        .find_map(|l| {
            l.trim_start_matches('\u{feff}')
                .trim()
                .strip_prefix("version=")
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("release metadata version missing")?;
    if metadata_version != env!("CARGO_PKG_VERSION") {
        bail!(
            "release metadata version mismatch: expected {}, got {metadata_version}",
            env!("CARGO_PKG_VERSION")
        );
    }

    let license = fs::read_to_string(resolved.join("LICENSE"))?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        bail!("AGPL license text missing from package");
    }

    println!("Windows package tree verified: {}", resolved.display());
    Ok(())
}

fn scan_forbidden_package_files(
    dir: &Path,
    forbidden_names: &BTreeSet<&str>,
    check_clean_db: bool,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            scan_forbidden_package_files(&path, forbidden_names, check_clean_db)?;
        } else if file_type.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if forbidden_names.contains(name)
                || name.ends_with(".stream-archive.claim")
                || name.ends_with(".log")
                || name.ends_with(".ps1")
            {
                bail!("Forbidden bundled/runtime file: {}", path.display());
            }
            if check_clean_db
                && (name.ends_with(".db") || name.ends_with(".db-wal") || name.ends_with(".db-shm"))
            {
                bail!(
                    "Official package must not contain a runtime database: {}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn assert_embedded_windows_resources(executable_path: &Path) -> Result<()> {
    use image::GenericImageView;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            EnumResourceNamesW, FindResourceW, LOAD_LIBRARY_AS_DATAFILE,
            LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW, LoadResource, LockResource,
            SizeofResource,
        },
    };

    const RT_ICON: usize = 3;
    const RT_GROUP_ICON: usize = 14;
    const RT_VERSION: usize = 16;

    let wide: Vec<u16> = OsStr::new(executable_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: wide is null-terminated UTF-16 and flags load the PE solely as a data/resource image.
    let module = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    if module.is_null() {
        bail!("LoadLibraryExW failed for {}", executable_path.display());
    }
    struct LibGuard(HMODULE);
    impl Drop for LibGuard {
        fn drop(&mut self) {
            // SAFETY: module handle was returned non-null by LoadLibraryExW.
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
    let _guard = LibGuard(module);

    enum ResId {
        Int(u16),
        Name(Vec<u16>),
    }

    unsafe extern "system" fn enum_group_proc(
        _module: HMODULE,
        _ty: *const u16,
        name: *const u16,
        lparam: isize,
    ) -> i32 {
        // SAFETY: lparam points to Vec<ResId> alive for the duration of EnumResourceNamesW.
        let list = unsafe { &mut *(lparam as *mut Vec<ResId>) };
        let raw = name as usize;
        if (raw >> 16) == 0 {
            list.push(ResId::Int(raw as u16));
        } else {
            let mut len = 0usize;
            // SAFETY: non-integer resource names passed by Win32 are null-terminated UTF-16 strings.
            while unsafe { *name.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: len bounded to null terminator.
            list.push(ResId::Name(
                unsafe { std::slice::from_raw_parts(name, len + 1) }.to_vec(),
            ));
        }
        1
    }

    let read_res = |name_ptr: *const u16, ty: usize| -> Result<Vec<u8>> {
        // SAFETY: module is valid, name_ptr and integer-encoded ty follow Win32 resource conventions.
        unsafe {
            let info = FindResourceW(module, name_ptr, ty as *const u16);
            if info.is_null() {
                bail!("FindResourceW failed for resource type {ty}");
            }
            let size = SizeofResource(module, info) as usize;
            if size == 0 {
                bail!("Embedded resource type {ty} is empty");
            }
            let loaded = LoadResource(module, info);
            if loaded.is_null() {
                bail!("LoadResource failed for resource type {ty}");
            }
            let locked = LockResource(loaded);
            if locked.is_null() {
                bail!("LockResource returned null for resource type {ty}");
            }
            Ok(std::slice::from_raw_parts(locked.cast::<u8>(), size).to_vec())
        }
    };

    let mut groups: Vec<ResId> = Vec::new();
    // SAFETY: EnumResourceNamesW completes synchronously before groups leaves scope.
    let ok = unsafe {
        EnumResourceNamesW(
            module,
            RT_GROUP_ICON as *const u16,
            Some(enum_group_proc),
            (&mut groups as *mut Vec<ResId>) as isize,
        )
    };
    if ok == 0 || groups.is_empty() {
        bail!("Executable has no RT_GROUP_ICON resources");
    }

    let expected_sizes: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let mut frames: Vec<(u32, Vec<u8>)> = Vec::new();
    for group_id in &groups {
        let group_bytes = match group_id {
            ResId::Int(id) => read_res(usize::from(*id) as *const u16, RT_GROUP_ICON)?,
            ResId::Name(wide_name) => read_res(wide_name.as_ptr(), RT_GROUP_ICON)?,
        };
        if group_bytes.len() < 6 {
            bail!("RT_GROUP_ICON header is truncated");
        }
        let reserved = read_le_u16(&group_bytes, 0)?;
        let res_type = read_le_u16(&group_bytes, 2)?;
        let count = usize::from(read_le_u16(&group_bytes, 4)?);
        if reserved != 0 || res_type != 1 || count == 0 {
            bail!("RT_GROUP_ICON header is invalid");
        }
        if group_bytes.len() < 6 + count * 14 {
            bail!("RT_GROUP_ICON directory is truncated");
        }
        for i in 0..count {
            let offset = 6 + i * 14;
            let width = if group_bytes[offset] == 0 {
                256u32
            } else {
                u32::from(group_bytes[offset])
            };
            let height = if group_bytes[offset + 1] == 0 {
                256u32
            } else {
                u32::from(group_bytes[offset + 1])
            };
            if group_bytes[offset + 3] != 0 {
                bail!("RT_GROUP_ICON entry reserved byte is non-zero");
            }
            if width != height {
                bail!("Embedded application icon frame is not square: {width}x{height}");
            }
            let bytes_in_res = read_le_u32(&group_bytes, offset + 8)? as usize;
            let resource_id = read_le_u16(&group_bytes, offset + 12)?;
            let image_bytes = read_res(usize::from(resource_id) as *const u16, RT_ICON)?;
            if image_bytes.len() != bytes_in_res {
                bail!(
                    "RT_ICON size mismatch for {width}px frame: group={bytes_in_res}, resource={}",
                    image_bytes.len()
                );
            }
            frames.push((width, image_bytes));
        }
    }

    if frames.len() != expected_sizes.len() {
        bail!(
            "StreamArchive.exe must embed exactly {} application icon frames; found {}",
            expected_sizes.len(),
            frames.len()
        );
    }
    frames.sort_by_key(|(width, _)| *width);
    let actual_sizes: Vec<u32> = frames.iter().map(|(width, _)| *width).collect();
    if actual_sizes != expected_sizes {
        bail!(
            "StreamArchive.exe embedded icon sizes are invalid. Expected {expected_sizes:?}, got {actual_sizes:?}"
        );
    }

    for (expected_size, image_bytes) in frames {
        const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
        if !image_bytes.starts_with(PNG_SIG) {
            bail!("StreamArchive.exe {expected_size}px RT_ICON resource is not a PNG frame");
        }
        let img = image::load_from_memory_with_format(&image_bytes, image::ImageFormat::Png)
            .with_context(|| {
                format!("StreamArchive.exe {expected_size}px embedded RT_ICON PNG failed to decode")
            })?;
        let (w, h) = img.dimensions();
        if w != expected_size || h != expected_size {
            bail!(
                "StreamArchive.exe embedded icon frame decoded at {w}x{h}; expected {expected_size}x{expected_size}"
            );
        }
        let rgba = img.to_rgba8();
        let mut lower_pixels = 0usize;
        let mut lower_signal = 0usize;
        for y in (h / 2)..h {
            for x in 0..w {
                let pixel = rgba.get_pixel(x, y);
                lower_pixels += 1;
                if pixel[3] > 0 && (pixel[0] < 240 || pixel[1] < 240 || pixel[2] < 240) {
                    lower_signal += 1;
                }
            }
        }
        if lower_pixels == 0 || (lower_signal as f64 / lower_pixels as f64) < 0.20 {
            bail!(
                "StreamArchive.exe {expected_size}px icon appears cropped/blank in the lower half: {lower_signal}/{lower_pixels}"
            );
        }
    }

    let version_bytes = read_res(std::ptr::without_provenance::<u16>(1), RT_VERSION)?;
    let product_version = extract_product_version_from_vs_versioninfo(&version_bytes)
        .context("StreamArchive.exe missing RT_VERSION ProductVersion")?;
    if product_version != env!("CARGO_PKG_VERSION") {
        bail!(
            "StreamArchive.exe ProductVersion mismatch: expected {}, got {product_version}",
            env!("CARGO_PKG_VERSION")
        );
    }

    Ok(())
}

#[cfg(any(windows, test))]
fn extract_product_version_from_vs_versioninfo(data: &[u8]) -> Option<String> {
    // VS_VERSIONINFO string values live in UTF-16LE String blocks where szKey is "ProductVersion\0"
    // followed by 4-byte alignment padding and the UTF-16LE null-terminated value string.
    let key_utf16: Vec<u8> = "ProductVersion\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let needle = key_utf16.as_slice();
    let pos = data.windows(needle.len()).position(|w| w == needle)?;
    // Each String entry header starts 6 bytes before szKey (wLength, wValueLength, wType).
    if pos < 6 {
        return None;
    }
    let node_start = pos - 6;
    let value_len_wchars = usize::from(read_le_u16(data, node_start + 2).ok()?);
    let after_key = pos + needle.len();
    let rel_from_node = after_key - node_start;
    let padded_rel = (rel_from_node + 3) & !3;
    let value_start = node_start + padded_rel;
    let value_byte_len = value_len_wchars.checked_mul(2)?;
    let value_bytes = data.get(value_start..value_start + value_byte_len)?;
    let words: Vec<u16> = value_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect();
    let decoded = String::from_utf16_lossy(&words);
    Some(decoded.trim_end_matches('\0').trim().to_string())
}

fn cmd_new_windows_release_archive(args: &[String]) -> Result<()> {
    let mut package_root = String::from(r".\dist\stream-archive");
    let mut output_dir = String::from(r".\dist\release");

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match normalize_flag(arg).as_deref() {
            Some("packageroot") => package_root = next_flag_value(args, &mut index, arg)?,
            Some("outputdir") => output_dir = next_flag_value(args, &mut index, arg)?,
            Some(other) => bail!("unknown --new-windows-release-archive option: --{other}"),
            None => bail!("unexpected argument for --new-windows-release-archive: {arg}"),
        }
        index += 1;
    }

    let (archive_path, checksum_path, bytes) =
        new_windows_release_archive(Path::new(&package_root), Path::new(&output_dir))?;
    println!("ARCHIVE={}", archive_path.display());
    println!("ARCHIVE_BYTES={bytes}");
    println!("ARCHIVE_CHECKSUM={}", checksum_path.display());
    Ok(())
}

pub fn new_windows_release_archive(
    package_root: &Path,
    output_dir: &Path,
) -> Result<(PathBuf, PathBuf, u64)> {
    let resolved_root = fs::canonicalize(package_root)
        .with_context(|| format!("Missing package root: {}", package_root.display()))?;
    // Validate clean-data package contract BEFORE creating the release ZIP so a failure
    // never leaves an official-looking archive on disk.
    verify_windows_package(&resolved_root, true, None, None)?;

    fs::create_dir_all(output_dir)?;
    let resolved_output = fs::canonicalize(output_dir)?;

    let arch_name = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => bail!("Unsupported Windows release architecture: {other}"),
    };
    let archive_name = format!("stream-archive-windows-{arch_name}.zip");
    let archive_path = resolved_output.join(&archive_name);
    let checksum_path = PathBuf::from(format!("{}.sha256", archive_path.display()));
    let _ = fs::remove_file(&archive_path);
    let _ = fs::remove_file(&checksum_path);

    create_zip_archive(&resolved_root, &archive_path)?;
    let hash = sha256_file_hex(&archive_path)?;
    fs::write(
        &checksum_path,
        format!("{hash}  {archive_name}\n").as_bytes(),
    )?;
    let bytes = fs::metadata(&archive_path)?.len();
    Ok((archive_path, checksum_path, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn offline_backup_and_restore_round_trip_and_reject_corrupted_backup() {
        let dir = tempfile::Builder::new()
            .prefix("Stream Archive RC backup 테스트-")
            .tempdir()
            .unwrap();
        let data_dir = dir.path().join("data 한글");
        let backup_dir = dir.path().join("backups 한글");
        fs::create_dir_all(&data_dir).unwrap();

        let db = data_dir.join("stream-archive.db");
        let mut bytes = vec![0u8; 4096];
        bytes[..16].copy_from_slice(SQLITE_HEADER);
        bytes[20] = 42;
        fs::write(&db, &bytes).unwrap();

        let (backup_path, backup_hash) =
            offline_backup_inner(&data_dir, &backup_dir, 5, 30, false).unwrap();
        assert!(backup_path.is_file());
        assert!(PathBuf::from(format!("{}.json", backup_path.display())).is_file());

        // Holding the per-database RuntimeOwnerGuard must block both offline backup and restore.
        {
            let _owner = crate::runtime_owner::RuntimeOwnerGuard::acquire(&db).unwrap();
            let backup_err = offline_backup_inner(&data_dir, &backup_dir, 5, 30, false)
                .unwrap_err()
                .to_string();
            assert!(
                backup_err.contains("Stream Archive is running"),
                "unexpected backup contention error: {backup_err}"
            );
            let restore_err =
                offline_restore_inner(&backup_path, &data_dir, Some(&backup_dir), false)
                    .unwrap_err()
                    .to_string();
            assert!(
                restore_err.contains("Stream Archive is running"),
                "unexpected restore contention error: {restore_err}"
            );
        }

        bytes[32] = 99;
        fs::write(&db, &bytes).unwrap();
        offline_restore_inner(&backup_path, &data_dir, Some(&backup_dir), false).unwrap();
        assert_eq!(sha256_file_hex(&db).unwrap(), backup_hash);

        let mut corrupted = fs::read(&backup_path).unwrap();
        corrupted.extend_from_slice(b"corruption");
        fs::write(&backup_path, &corrupted).unwrap();
        let err = offline_restore_inner(&backup_path, &data_dir, Some(&backup_dir), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("SHA256"), "unexpected error: {err}");
    }

    #[test]
    fn release_metadata_handles_non_git_clean_dirty_and_linked_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

        let no_git = dir.path().join("no-git");
        fs::create_dir_all(&no_git).unwrap();
        let meta_out = dir.path().join("RELEASE_INFO.txt");
        let (_, commit) = write_release_metadata(&meta_out, &manifest, Some(&no_git)).unwrap();
        assert_eq!(commit, "unknown");

        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let run_git = |cwd: &Path, args: &[&str]| -> bool {
            Command::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !run_git(&repo, &["init"]) {
            return;
        }
        assert!(run_git(&repo, &["config", "user.name", "Test"]));
        assert!(run_git(
            &repo,
            &["config", "user.email", "test@example.com"]
        ));
        fs::write(repo.join("tracked.txt"), b"hello\n").unwrap();
        assert!(run_git(&repo, &["add", "tracked.txt"]));
        assert!(run_git(&repo, &["commit", "-m", "init"]));

        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let expected = head.trim().to_ascii_lowercase();

        let (_, clean_commit) = write_release_metadata(&meta_out, &manifest, Some(&repo)).unwrap();
        assert_eq!(clean_commit, expected);

        fs::write(repo.join("dirty.tmp"), b"dirty").unwrap();
        let (_, dirty_commit) = write_release_metadata(&meta_out, &manifest, Some(&repo)).unwrap();
        assert_eq!(dirty_commit, format!("{expected}-dirty"));
        fs::remove_file(repo.join("dirty.tmp")).unwrap();

        let worktree = dir.path().join("worktree");
        assert!(run_git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "wt-branch",
                &worktree.display().to_string(),
                "HEAD"
            ]
        ));
        let (_, wt_commit) = write_release_metadata(&meta_out, &manifest, Some(&worktree)).unwrap();
        assert_eq!(wt_commit, expected);

        fs::write(worktree.join("dirty-wt.tmp"), b"dirty").unwrap();
        let (_, wt_dirty) = write_release_metadata(&meta_out, &manifest, Some(&worktree)).unwrap();
        assert_eq!(wt_dirty, format!("{expected}-dirty"));
    }

    #[test]
    fn zip_archive_round_trips_and_rejects_trailing_corruption() {
        let dir = tempfile::Builder::new()
            .prefix("Stream Archive ZIP 테스트-")
            .tempdir()
            .unwrap();
        let src = dir.path().join("pkg");
        let empty_dir = src.join("data");
        let nested_dir = src.join("docs");
        fs::create_dir_all(&empty_dir).unwrap();
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(src.join("RELEASE_INFO.txt"), b"product=Stream Archive\n").unwrap();
        fs::write(nested_dir.join("OPERATIONS.md"), b"# Ops\n").unwrap();

        let zip_path = dir.path().join("test.zip");
        create_zip_archive(&src, &zip_path).unwrap();

        let extracted = dir.path().join("extracted");
        extract_zip_archive(&zip_path, &extracted).unwrap();
        assert!(extracted.join("data").is_dir());
        assert_eq!(
            fs::read_to_string(extracted.join("docs/OPERATIONS.md")).unwrap(),
            "# Ops\n"
        );

        let mut bytes = fs::read(&zip_path).unwrap();
        bytes.extend_from_slice(b"trailing corruption");
        fs::write(&zip_path, &bytes).unwrap();
        assert!(extract_zip_archive(&zip_path, &dir.path().join("bad")).is_err());
    }

    #[test]
    fn extracts_product_version_from_vs_versioninfo_layout() {
        // Minimal String node layout: wLength(2) wValueLength(6 wchars) wType(1) "ProductVersion\0" pad "1.0.4\0"
        let mut buf = Vec::new();
        let value_wchars: u16 = 6; // "1.0.4\0"
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&value_wchars.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        for w in "ProductVersion\0".encode_utf16() {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        for w in "1.0.4\0".encode_utf16() {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(
            extract_product_version_from_vs_versioninfo(&buf).as_deref(),
            Some("1.0.4")
        );
    }
}
