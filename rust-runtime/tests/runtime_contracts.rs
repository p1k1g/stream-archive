//! Cross-platform runtime, architecture, provider, security, packaging, and release contracts.
//! Replaces the retired `maintenance/Test-RuntimeContracts.ps1` and `maintenance/guards/*.ps1` suite.

use regex::Regex;
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust-runtime lives directly under repo root")
        .to_path_buf()
}

fn read_repo_file(relative: &str) -> String {
    let path = repo_root().join(relative.replace('\\', "/"));
    assert!(
        path.is_file(),
        "Missing required runtime contract file: {relative} ({})",
        path.display()
    );
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("Failed to read {relative}: {err}"))
}

fn read_dir_files_joined(relative_dir: &str, ext: &str) -> String {
    let root = repo_root().join(relative_dir.replace('\\', "/"));
    let mut files = Vec::new();
    collect_files_with_ext(&root, ext, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_files_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_ext(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

fn assert_match(text: &str, pattern: &str, message: &str) {
    let re = Regex::new(pattern).unwrap_or_else(|err| panic!("invalid regex `{pattern}`: {err}"));
    assert!(re.is_match(text), "{message} (pattern: {pattern})");
}

fn assert_not_match(text: &str, pattern: &str, message: &str) {
    let re = Regex::new(pattern).unwrap_or_else(|err| panic!("invalid regex `{pattern}`: {err}"));
    assert!(!re.is_match(text), "{message} (pattern: {pattern})");
}

fn assert_rust_test(text: &str, test_name: &str, message: &str) {
    let pattern = format!(r"fn\s+{}\s*\(", regex::escape(test_name));
    assert_match(text, &pattern, message);
}

fn cargo_package_version(manifest_relative: &str, package_name: &str) -> String {
    let manifest = repo_root().join(manifest_relative);
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .unwrap_or_else(|err| panic!("cargo metadata failed for {manifest_relative}: {err}"));
    assert!(
        output.status.success(),
        "cargo metadata failed for {manifest_relative}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid cargo metadata JSON");
    json["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|pkg| pkg["name"].as_str() == Some(package_name))
        .and_then(|pkg| pkg["version"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("package {package_name} not found in {manifest_relative}"))
}

#[test]
fn architecture_contracts() {
    let platform = read_repo_file("rust-runtime/src/platform/mod.rs");
    let platform_live = read_repo_file("rust-runtime/src/platform/live.rs");
    let platform_vod = read_repo_file("rust-runtime/src/platform/vod.rs");
    let platform_soop = read_repo_file("rust-runtime/src/platform/soop/mod.rs");
    let platform_soop_live = read_repo_file("rust-runtime/src/platform/soop/live.rs");
    let platform_soop_vod = read_repo_file("rust-runtime/src/platform/soop/vod.rs");
    let support = read_repo_file("rust-runtime/src/support.rs");
    let backend = read_repo_file("rust-runtime/src/backend.rs");
    let watcher = read_repo_file("rust-runtime/src/native_watcher.rs");
    let vod_facade = read_repo_file("rust-runtime/src/vod.rs");
    let queue_service = read_repo_file("rust-runtime/src/queue_service.rs");
    let backup_service = read_repo_file("rust-runtime/src/backup_service.rs");
    let storage_service = read_repo_file("rust-runtime/src/storage_service.rs");
    let store = read_repo_file("rust-runtime/src/store.rs");
    let main = read_repo_file("rust-runtime/src/main.rs");
    let headless = read_repo_file("rust-runtime/src/headless.rs");
    let lib = read_repo_file("rust-runtime/src/lib.rs");
    let core = read_repo_file("rust-runtime/src/app_core.rs");
    let diagnostics = read_repo_file("rust-runtime/src/diagnostics.rs");
    let cli = read_repo_file("rust-runtime/src/bin/stream-archive-cli.rs");
    let gui_manifest = read_repo_file("rust-gui/Cargo.toml");
    let gui_build = read_repo_file("rust-gui/build.rs");
    let gui_native_shell = read_repo_file("rust-gui/src/native_shell.rs");
    let gui_state = read_repo_file("rust-gui/src/state.rs");
    let gui_ui = read_dir_files_joined("rust-gui/src/ui", "rs");
    let gui_settings_ui = read_repo_file("rust-gui/src/ui/settings_view.rs");
    let gui_diagnostics_ui = read_repo_file("rust-gui/src/ui/diagnostics_view.rs");
    let gui_sources = read_dir_files_joined("rust-gui/src", "rs");
    let main_and_headless = format!("{main}\n{headless}");

    assert_match(
        &main,
        r"run_headless\(",
        "Compatibility server entrypoint must delegate to the shared headless lifecycle.",
    );
    assert_match(
        &headless,
        r"StreamArchiveCore::open\(&backend_dir\)",
        "Headless runtime must bootstrap through StreamArchiveCore.",
    );
    assert_match(
        &headless,
        r"core\.spawn_vod_history_sync\(\)",
        "Headless runtime must keep VOD history synchronization.",
    );
    assert_match(
        &headless,
        r"core\.spawn_auto_backup\(\)",
        "Headless runtime must keep automatic backup scheduling.",
    );
    assert_match(
        &headless,
        r"core\.spawn_queue_worker\(\)",
        "Headless runtime must keep the persistent Queue worker.",
    );
    assert_match(
        &headless,
        r"core\.shutdown\(\)\.await",
        "Headless runtime must shut down through shared owned-runtime cleanup.",
    );
    assert_not_match(
        &main_and_headless,
        r"\baxum::|Router::new|TcpListener::bind|include_str!\(\x22\.\./web|STREAM_ARCHIVE_BIND|STREAM_ARCHIVE_TOKEN",
        "Headless runtime must stay free of retired Web presentation/listener state.",
    );

    assert_match(
        &core,
        r"Store::migrate_legacy_database\(&db_path\)",
        "Shared core startup must perform only the bounded legacy database filename migration.",
    );
    assert_match(
        &core,
        r"Store::open\(db_path\)",
        "Shared core startup must open the canonical SQLite store.",
    );
    assert_match(
        &store,
        r#"const DATABASE_FILE:\s*&str\s*=\s*"stream-archive\.db""#,
        "Canonical database filename must use the Stream Archive namespace.",
    );
    assert_match(
        &store,
        r"STREAM_ARCHIVE_DATA_DIR",
        "Data directory environment override must use the Stream Archive namespace.",
    );
    assert_not_match(
        &main_and_headless,
        r"bootstrap_primary_once|materialize_primary_files|SOOP_LIVE_SETTING\.ini|SOOP_LIVE_CHANNELS\.txt|SOOP_VOD_SETTING\.ini",
        "Runtime must not import or emit legacy INI/TXT mirrors.",
    );
    assert_not_match(
        &store,
        r"legacy-import-live|legacy-import-vod|read_safe_settings|read_channels|read_hidden_settings",
        "SQLite store must not depend on legacy config-file import helpers.",
    );
    assert_match(
        &store,
        r"settings_cache: Arc<RwLock<BTreeMap<String, String>>>",
        "Committed runtime settings cache is missing.",
    );
    assert_rust_test(
        &store,
        "runtime_config_cache_tracks_committed_writes",
        "Committed settings-cache behavior test is missing.",
    );
    assert_match(
        &store,
        r"platform TEXT NOT NULL",
        "Persistent history and queue schemas must retain platform identity.",
    );
    assert_match(
        &queue_service,
        r"lifecycle_lock: Arc<Mutex<\(\)>>",
        "Queue must share the global serialized VOD lifecycle lock.",
    );

    for (pat, msg) in [
        (
            r"pub\s+mod\s+app_core",
            "Shared library must expose the Phase 21 application core.",
        ),
        (
            r"pub\s+mod\s+store",
            "Shared library must expose canonical SQLite persistence.",
        ),
        (
            r"pub\s+mod\s+backup_service",
            "Shared library must expose the reusable backup service.",
        ),
        (
            r"pub\s+mod\s+storage_service",
            "Shared library must expose reusable storage diagnostics.",
        ),
        (
            r"pub\s+mod\s+native_watcher",
            "Shared library must expose the native watcher boundary.",
        ),
        (
            r"pub\s+mod\s+vod",
            "Shared library must expose the VOD facade.",
        ),
        (
            r"pub\s+mod\s+release_tooling",
            "Shared library must expose pure-Rust release tooling.",
        ),
    ] {
        assert_match(&lib, pat, msg);
    }

    assert_match(
        &core,
        r"pub\s+struct\s+StreamArchiveCore",
        "Phase 21 shared application facade is missing.",
    );
    assert_match(
        &core,
        r"Store::default_path\(&backend_dir\)",
        "Shared core must open the canonical SQLite path.",
    );
    assert_match(
        &core,
        r"store::init_global\(store\.clone\(\)\)",
        "Shared core must initialize the canonical global store.",
    );
    assert_match(
        &core,
        r"crate::environment_settings::validate_updates\(updates\)",
        "Shared core environment settings writes must preserve validation.",
    );
    assert_match(
        &core,
        r"validate_setting_updates\(settings\)",
        "Shared core provider settings writes must preserve validation.",
    );
    assert_match(
        &core,
        r"validate_secret_updates\(secrets\)",
        "Shared core provider secret writes must preserve security validation.",
    );
    assert_match(
        &core,
        r"protect_secret_with_cleanup_intent\(value,",
        "Shared core provider secret writes must use the native protection boundary.",
    );
    assert_match(
        &core,
        r"retain_secret_cleanup",
        "Provider native secret creation must retain cleanup intent.",
    );
    assert_match(
        &core,
        r"sync_settings_retiring_secrets",
        "Provider secret transitions must atomically retire native references.",
    );
    assert_match(
        &core,
        r"validate_channels\(channels\)",
        "Shared core channel writes must preserve provider validation.",
    );
    assert_match(
        &core,
        r"apply_vod_tool_defaults",
        "Shared core VOD operations must preserve canonical media-tool defaults.",
    );
    assert_match(
        &core,
        r"lifecycle_lock: Arc<Mutex<\(\)>>",
        "Shared core must serialize VOD lifecycle operations.",
    );
    assert_not_match(
        &core,
        r"pub\s+fn\s+(?:watcher|vod|queue|backups|config_write_lock|lifecycle_lock)\s*\(",
        "Dead manager/lock compatibility getters must not return to the public core API.",
    );
    assert_not_match(
        &core,
        r"pub\s+async\s+fn\s+(?:update_settings|update_secrets|update_vod_tool_settings)\s*\(",
        "Dead generic configuration compatibility facades must not return to the public core API.",
    );
    assert_not_match(
        &core,
        r"pub\s+fn\s+storage_check\s*\(",
        "Unused single-path storage compatibility facade must not return.",
    );
    assert_match(
        &core,
        r"pub\s+async\s+fn\s+shutdown",
        "Shared core must expose owned-runtime shutdown.",
    );
    assert_not_match(
        &core,
        r"(?m)^\s*use\s+(?:axum|http)(?:::|\s*\{)|\baxum::|\bhttp::(?:HeaderMap|StatusCode)\b",
        "Shared core must stay independent from Axum/HTTP presentation concerns.",
    );
    assert_rust_test(
        &core,
        "assembled_core_keeps_one_canonical_store_backend_and_queue",
        "Shared core canonical-store/queue regression test is missing.",
    );
    assert_match(
        &core,
        r"pub\s+async\s+fn\s+backup_snapshot",
        "Shared core native backup snapshot service is missing.",
    );
    assert_match(
        &core,
        r"pub\s+async\s+fn\s+restore_backup",
        "Shared core native restore service is missing.",
    );
    assert_match(
        &core,
        r"pub\s+fn\s+storage_snapshot",
        "Shared core native storage snapshot service is missing.",
    );
    assert_match(
        &storage_service,
        r"MIN_FREE_SPACE_GB",
        "Shared storage diagnostics must use the canonical free-space threshold.",
    );
    assert_match(
        &storage_service,
        r"channel\.outdir",
        "Shared storage diagnostics must include channel-specific output directories.",
    );
    assert_match(
        &core,
        r"storage_service::snapshot\(&self\.store\)",
        "Shared core storage diagnostics must delegate to the storage service.",
    );
    assert_match(
        &core,
        r"pub\s+async\s+fn\s+runtime_logs",
        "Shared core bounded runtime log service is missing.",
    );
    assert_match(
        &backup_service,
        r"pub\s+struct\s+BackupManager",
        "Reusable backup manager is missing from backup_service.rs.",
    );
    assert_match(
        &backup_service,
        r"pre_restore",
        "Restore safety backup behavior is missing from shared backup service.",
    );
    assert_not_match(
        &backup_service,
        r"\baxum::|HeaderMap|StatusCode|\bState\s*\(",
        "Shared backup service must stay independent from Axum/Web presentation.",
    );

    assert_match(
        &gui_manifest,
        r#"name\s*=\s*"stream-archive-gui""#,
        "GPUI desktop crate is missing.",
    );
    assert_match(
        &gui_manifest,
        r"gpui-kit\s*=\s*\{",
        "GPUI runtime dependency is missing.",
    );
    assert_not_match(
        &gui_manifest,
        r"(?m)^\s*slint(?:-build)?\s*=",
        "Retired Slint dependencies must not return to rust-gui.",
    );
    assert_match(
        &gui_manifest,
        r#"stream-archive-server\s*=\s*\{\s*path\s*=\s*"\.\./rust-runtime""#,
        "GPUI desktop must depend on the shared Rust library directly.",
    );
    assert_match(
        &gui_sources,
        r"StreamArchiveCore",
        "GPUI bootstrap must use the shared application core.",
    );
    assert_match(
        &gui_sources,
        r"resolve_backend_dir",
        "GPUI bootstrap must reuse canonical backend resolution.",
    );
    assert_match(
        &backend,
        r"(?s)env::current_exe\(\).*?candidates\.push\(exe_dir\.join\(.backend.\)\).*?env::current_dir\(\)",
        "Portable backend resolution must prefer the executable-adjacent backend before the working directory.",
    );
    assert_match(
        &gui_sources,
        r"bind_core_snapshot",
        "GPUI shell must bind runtime state from the shared core.",
    );
    assert_not_match(
        &gui_sources,
        r"\breqwest::|\baxum::|https?://127\.0\.0\.1|https?://localhost|rusqlite::|Command::new|taskkill|pkill|killall",
        "GPUI shell must not bypass shared core through HTTP, SQLite, or direct process control.",
    );
    assert_match(
        &gui_state,
        r"pub\s+struct\s+UiState",
        "GPUI shell state boundary is missing.",
    );
    assert_match(
        &gui_sources,
        r"pub\s+fn\s+refresh_requested\s*\(",
        "GPUI runtime refresh action is missing.",
    );
    for page in [
        "LIVE",
        "Channels",
        "VOD",
        "Queue",
        "History",
        "Settings",
        "Diagnostics",
    ] {
        assert_match(
            &gui_state,
            &regex::escape(&format!(r#""{page}""#)),
            &format!("GPUI navigation page missing: {page}"),
        );
    }
    assert_not_match(
        &gui_state,
        r#"Self::Maintenance\b|"Maintenance""#,
        "Maintenance must stay nested under Settings.",
    );
    assert_not_match(
        &gui_state,
        r#"Self::Dashboard\b|"Dashboard""#,
        "Dashboard must stay removed.",
    );
    assert_match(
        &gui_state,
        r"pub\s+enum\s+SettingsView\s*\{[^}]*General[^}]*Manage",
        "Settings must own an internal General/Manage view boundary.",
    );
    assert_match(
        &gui_settings_ui,
        r"SettingsView::Manage",
        "Settings must expose the nested management view.",
    );
    assert_match(
        &gui_settings_ui,
        r"render_backup_manage_section\(",
        "Backup/restore must remain reachable inside Settings > Manage.",
    );
    assert_match(
        &gui_state,
        r"active_page:\s*Page::Live",
        "Configured native startup must default to the LIVE page.",
    );
    assert_match(
        &gui_ui,
        r#"include_bytes!\("\.\./\.\./assets/stream-archive-icon\.png"\)"#,
        "GPUI branding image must be embedded into the executable.",
    );
    assert_not_match(
        &gui_build,
        r"slint_build",
        "Retired Slint build step must not return.",
    );
    assert_not_match(
        &gui_ui,
        r"Phase 21 Slint native shell",
        "Public LIVE UI must not expose the old development subtitle.",
    );
    assert_match(
        &gui_ui,
        r"live_open_folder\(",
        "LIVE recording cards must expose the save-folder action through the Native controller.",
    );
    assert_match(
        &gui_native_shell,
        r"ShellExecuteW",
        "Windows LIVE folder opening must use the Win32 Shell API boundary.",
    );
    assert_not_match(
        &gui_native_shell,
        r"(?i)cmd(?:\.exe)?|/c|Command::new|powershell",
        "LIVE folder opening must not assemble or spawn a command shell.",
    );

    assert_match(
        &platform,
        r"pub\s+mod\s+live",
        "Platform LIVE facade must be registered.",
    );
    assert_match(
        &platform,
        r"pub\s+mod\s+vod",
        "Platform VOD facade must be registered.",
    );
    assert_match(
        &platform,
        r"pub\s+mod\s+soop",
        "SOOP provider module must be registered.",
    );
    assert_match(
        &platform,
        r"trait\s+PlatformProvider",
        "Platform provider boundary is missing.",
    );
    assert_match(
        &platform,
        r"fn\s+detect_vod_platform",
        "Platform registry must own VOD platform detection.",
    );
    assert_match(
        &platform_soop,
        r"pub\s+mod\s+live",
        "SOOP LIVE provider must be registered.",
    );
    assert_match(
        &platform_soop,
        r"pub\s+mod\s+vod",
        "SOOP VOD provider must be registered.",
    );
    assert_match(
        &platform_soop,
        r"fn\s+accepts_vod_url",
        "SOOP provider must own SOOP VOD URL recognition.",
    );
    assert_match(
        &platform_live,
        r"enum\s+LiveSession",
        "Common LIVE session facade is missing.",
    );
    assert_match(
        &platform_vod,
        r"pub\s+struct\s+VodManager",
        "Common VOD manager facade is missing.",
    );
    assert_match(
        &support,
        r"provider\(platform\)",
        "Channel lookup must route through the platform provider.",
    );

    assert_not_match(
        &watcher,
        r#"https?://[^\s"']*sooplive\.com|player_live_api\.php|LoginAction\.php"#,
        "Native watcher must not contain direct SOOP network endpoints.",
    );
    assert_match(
        &platform_soop_live,
        r"player_live_api\.php",
        "SOOP LIVE provider must own SOOP live discovery endpoints.",
    );
    assert_not_match(
        &vod_facade,
        r"sooplive\.com|CloudFront|yt-dlp|ffmpeg|taskkill\.exe",
        "Root VOD facade must stay platform/process neutral.",
    );
    assert_not_match(
        &platform_vod,
        r"CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php",
        "Common VOD facade must not own provider network/auth implementation details.",
    );
    assert_match(
        &platform_soop_vod,
        r"private_auth\.php",
        "SOOP VOD provider must own SOOP authorization.",
    );
    assert_match(
        &platform_soop_vod,
        r"vod\.sooplive\.com",
        "SOOP VOD provider must own SOOP VOD endpoints.",
    );
    assert_match(
        &queue_service,
        r"detect_vod_platform",
        "VOD queue must persist detected platform identity.",
    );
    assert_not_match(
        &queue_service,
        r"CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php",
        "VOD queue must remain provider-neutral.",
    );

    assert_match(
        &core,
        r"pub\s+async\s+fn\s+update_environment_settings",
        "Native settings must persist through shared core.",
    );
    assert_match(
        &core,
        r"pub\s+fn\s+diagnostics",
        "Structured diagnostics service is missing.",
    );
    assert_match(
        &core,
        r"pub\s+async\s+fn\s+active_local_diagnostics",
        "Shared core must expose explicit active local diagnostics.",
    );
    assert_match(
        &gui_sources,
        r"core\.active_local_diagnostics\(",
        "Native diagnostics refresh must re-run shared active local tool probes.",
    );
    assert_match(
        &core,
        r"pub\s+fn\s+is_first_run_unconfigured",
        "Shared core must expose first-run routing state.",
    );
    assert_match(
        &gui_sources,
        r"core\.is_first_run_unconfigured\(",
        "Native startup must determine first-run routing through the shared core.",
    );
    assert_match(
        &gui_sources,
        r"core\.update_environment_settings",
        "GUI settings bypass shared service.",
    );
    assert_match(
        &gui_sources,
        r"core\.diagnostics\(",
        "GUI diagnostics must consume shared diagnostics.",
    );
    assert_match(
        &diagnostics,
        r"pub\s+enum\s+DiagnosticRequirement",
        "Shared preflight must model Required/Optional/Informational checks.",
    );
    assert_match(
        &diagnostics,
        r"pub\s+struct\s+DiagnosticsSummary",
        "Shared preflight summary model is missing.",
    );
    assert_match(
        &diagnostics,
        r#""database\.integrity""#,
        "Shared preflight must retain stable database integrity check identity.",
    );
    assert_match(
        &diagnostics,
        r#""provider\.soop""#,
        "Shared preflight must retain stable SOOP readiness identity.",
    );
    assert_match(
        &diagnostics,
        r#""provider\.chzzk""#,
        "Shared preflight must retain stable CHZZK readiness identity.",
    );
    assert_match(
        &diagnostics,
        r"PRAGMA quick_check",
        "Shared preflight must perform read-only SQLite integrity checking.",
    );
    assert_match(
        &diagnostics,
        r"blocking_errors\s*==\s*0",
        "Runtime usability must depend on blocking required errors, not warnings.",
    );
    assert_not_match(
        &diagnostics,
        r"\breqwest::|\breqwest\b|Client::new|Command::new|unprotect_secret",
        "Shared preflight must remain local/read-only.",
    );
    assert_match(
        &cli,
        r"collect_read_only_preflight",
        "CLI doctor must load diagnostics through the shared read-only preflight service.",
    );
    assert_not_match(
        &cli,
        r"load_preflight_settings|backup_policy_from_settings|backup_directory_from_settings",
        "CLI doctor must not duplicate preflight interpretation.",
    );
    assert_match(
        &cli,
        r"doctor \[--json\]",
        "CLI doctor must retain documented JSON automation mode.",
    );
    assert_not_match(
        &gui_sources,
        r"std::fs::write|fs::write|Connection::open|std::process|tokio::process",
        "GUI must not write runtime files or own child processes.",
    );
    assert_match(
        &gui_diagnostics_ui,
        r"MaintenanceSection::Diagnostics",
        "Diagnostics view must retain the read-only Diagnostics section.",
    );
    assert_match(
        &gui_diagnostics_ui,
        r"MaintenanceSection::Logs",
        "Diagnostics view must retain the Runtime Logs section.",
    );
    assert_match(
        &gui_diagnostics_ui,
        r"maintenance_diagnostic_rows",
        "Diagnostics view must render shared diagnostic rows.",
    );
    assert_match(
        &gui_sources,
        r"core\.backup_snapshot\(",
        "Native Maintenance must load backups through StreamArchiveCore.",
    );
    assert_match(
        &gui_sources,
        r"core\.restore_backup\(",
        "Native Maintenance restore must use StreamArchiveCore.",
    );
    assert_match(
        &gui_sources,
        r"core\.runtime_logs\(",
        "Native log viewer must use the bounded shared LogBuffer service.",
    );
    assert_match(
        &gui_sources,
        r"core\.storage_snapshot\(",
        "Native storage diagnostics must use StreamArchiveCore.",
    );
    let (general_settings_ui, manage_settings_ui) = gui_settings_ui
        .split_once("fn render_backup_manage_section")
        .expect("Settings view must keep General and Manage backup sections separated.");
    assert_not_match(
        general_settings_ui,
        r"save_backup_policy_inputs|backup_dir_input",
        "General settings must not duplicate backup policy actions or fields.",
    );
    assert_match(
        &gui_settings_ui,
        r"Phase 21\.9: Settings > Manage owns backup policy and operations\.",
        "Nested Settings management must own backup policy presentation.",
    );
    assert_match(
        manage_settings_ui,
        r"save_backup_policy_inputs\(",
        "Nested Settings management must save backup policy through AppController.",
    );
    assert_match(
        manage_settings_ui,
        r"backup_dir_input",
        "Nested Settings management must bind the shared backup directory policy.",
    );
    assert_match(
        manage_settings_ui,
        r"backup_create\(",
        "Nested Settings management must retain operational backup creation.",
    );
    assert_match(
        manage_settings_ui,
        r"request_restore\(",
        "Nested Settings management must retain operational restore actions.",
    );
    assert_not_match(
        &gui_sources,
        r"\bSha256::new\b|\bsha2::Digest\b|\bread_to_end\s*\(|\bstd::io::copy\s*\(",
        "GPUI presentation must not implement backup hashing/copying.",
    );
}

#[test]
fn provider_contracts() {
    let platform = read_repo_file("rust-runtime/src/platform/mod.rs");
    let platform_live = read_repo_file("rust-runtime/src/platform/live.rs");
    let chzzk = read_repo_file("rust-runtime/src/platform/chzzk/mod.rs");
    let auth = read_repo_file("rust-runtime/src/platform/chzzk/auth.rs");
    let live = read_repo_file("rust-runtime/src/platform/chzzk/live.rs");
    let recorder = read_repo_file("rust-runtime/src/recorder.rs");
    let watcher = read_repo_file("rust-runtime/src/native_watcher.rs");
    let primary = read_repo_file("rust-runtime/src/primary_config.rs");
    let backend = read_repo_file("rust-runtime/src/backend.rs");
    let soop_vod = read_repo_file("rust-runtime/src/platform/soop/vod.rs");
    let core = read_repo_file("rust-runtime/src/app_core.rs");
    let kick = read_repo_file("rust-runtime/src/platform/kick/mod.rs");
    let kick_live = read_repo_file("rust-runtime/src/platform/kick/live.rs");
    let kick_vod = read_repo_file("rust-runtime/src/platform/kick/vod.rs");
    let gui_sources = read_dir_files_joined("rust-gui/src", "rs");
    let gui_ui = read_dir_files_joined("rust-gui/src/ui", "rs");

    assert_match(
        &platform,
        r"Chzzk",
        "PlatformId::Chzzk registration is missing.",
    );
    assert_match(
        &platform,
        r"PlatformId::Chzzk\s*=>\s*&chzzk::CHZZK",
        "CHZZK provider dispatch is missing.",
    );
    assert_match(
        &chzzk,
        r"live:\s*true",
        "CHZZK LIVE capability must remain enabled.",
    );
    assert_match(
        &chzzk,
        r"vod:\s*(?:false|true)",
        "CHZZK provider VOD capability field is missing.",
    );
    assert_match(
        &soop_vod,
        r"tools\.yt_dlp",
        "SOOP VOD must remain yt-dlp based.",
    );
    assert_match(
        &chzzk,
        r"account\.len\(\)\s*!=\s*32",
        "CHZZK channel ID length validation is missing.",
    );
    assert_match(
        &chzzk,
        r"is_ascii_hexdigit",
        "CHZZK channel ID hex validation is missing.",
    );

    assert_match(
        &backend,
        r#""CHZZK_NID_AUT""#,
        "CHZZK_NID_AUT is not registered as a hidden setting.",
    );
    assert_match(
        &backend,
        r#""CHZZK_NID_SES""#,
        "CHZZK_NID_SES is not registered as a hidden setting.",
    );
    assert_match(
        &auth,
        r"read_provider_settings",
        "CHZZK auth does not decrypt through the common secret boundary.",
    );
    assert_match(
        &auth,
        r"pub fn load\(\)",
        "CHZZK auth hot-load entrypoint is missing.",
    );
    assert_match(
        &auth,
        r"ChzzkAuthState::Missing",
        "Missing CHZZK auth state coverage is missing.",
    );
    assert_match(
        &auth,
        r"ChzzkAuthState::Partial",
        "Partial CHZZK auth state coverage is missing.",
    );
    assert_match(
        &auth,
        r"ChzzkAuthState::Configured",
        "Configured CHZZK auth state coverage is missing.",
    );

    assert_match(
        &live,
        r"service/v2/channels/\{channel_id\}/live-detail",
        "CHZZK live-detail endpoint is missing.",
    );
    assert_match(
        &live,
        r"membershipBenefitType",
        "CHZZK membership-only restriction detection is missing.",
    );
    assert_match(
        &live,
        r"let requires_auth = adult \|\| membership_only",
        "CHZZK restricted playback flag is missing.",
    );
    assert_match(
        &live,
        r"if !playback_available",
        "CHZZK playback availability handling is missing.",
    );
    assert_match(
        &live,
        r"return auth_failure\(auth\)",
        "Restricted playback auth handling is missing.",
    );
    assert_match(
        &live,
        r"ChzzkAuthState::Configured",
        "Expired or unauthorized CHZZK auth path is missing.",
    );
    assert_match(
        &live,
        r"cookies:\s*if live\.requires_auth",
        "Public CHZZK LIVE must not always forward stored cookies.",
    );

    assert_match(
        &platform_live,
        r"pub async fn recover_auth",
        "LIVE facade auth recovery entrypoint is missing.",
    );
    assert_match(
        &platform_live,
        r"Self::Chzzk\(session\)\s*=>\s*session\.recover_auth\(\)",
        "CHZZK auth recovery is not dispatched through LIVE facade.",
    );
    assert_match(
        &live,
        r"pub fn recover_auth\(&self\)",
        "CHZZK provider-owned auth recovery guidance is missing.",
    );
    assert_match(
        &watcher,
        r"recover_auth_required\(session, config\)",
        "Watcher does not use provider-neutral auth recovery.",
    );
    assert_match(
        &watcher,
        r"\.recover_auth\(&config\.soop_username, &config\.soop_password\)",
        "Watcher auth recovery is not routed through LiveSession.",
    );
    assert_not_match(
        &watcher,
        r"CHZZK 제한 방송 인증",
        "Provider-specific CHZZK auth policy leaked into the common watcher.",
    );

    assert_match(
        &recorder,
        r"StreamInput::PluginUrl",
        "Recorder does not accept provider plugin URLs.",
    );
    assert_match(
        &recorder,
        r#""--can-handle-url""#,
        "Streamlink plugin preflight is missing.",
    );
    assert_match(
        &recorder,
        r#""--http-cookies-file""#,
        "Streamlink cookie-file transport is missing.",
    );
    assert_match(
        &recorder,
        r"Netscape HTTP Cookie File",
        "Netscape cookie-file generation is missing.",
    );
    assert_match(
        &recorder,
        r"COOKIE_FILE_EXPIRES_UNIX",
        "CHZZK Netscape cookie entries must use a non-expired timestamp.",
    );
    assert_match(
        &recorder,
        r"4_102_444_800",
        "CHZZK Netscape cookie expiry must stay in the future.",
    );
    assert_not_match(
        &recorder,
        r"--http-cookie\s+NID_",
        "CHZZK cookie values must not be exposed as direct process arguments.",
    );
    assert_match(
        &recorder,
        r"struct CookieFile\(PathBuf\)",
        "Temporary CHZZK authentication cookie is not ownership-guarded.",
    );
    assert_match(
        &recorder,
        r"impl Drop for CookieFile",
        "Temporary CHZZK authentication cookie cleanup guard is missing.",
    );
    assert_match(
        &recorder,
        r"(?s)let timestamp_player = if start_at_zero.*?resolve_timestamp_rebase_ffmpeg.*?let cookie_file = match cookies",
        "Fallible FFmpeg/player setup must finish before plaintext cookie creation.",
    );
    assert_match(
        &recorder,
        r"cookie_file_guard_removes_plaintext_temp_file_on_drop",
        "Plaintext CHZZK cookie cleanup regression test is missing.",
    );

    assert_match(
        &platform_live,
        r"start_at_zero:\s*bool",
        "Plugin stream timestamp policy is missing from StreamInput.",
    );
    assert_match(
        &live,
        r"start_at_zero:\s*true",
        "CHZZK LIVE does not request zero-based recording timestamps.",
    );
    assert_match(
        &recorder,
        r"resolve_timestamp_rebase_ffmpeg",
        "Recorder cannot locate FFmpeg for CHZZK live remux.",
    );
    assert_match(
        &recorder,
        r#"\.arg\("--player"\)"#,
        "CHZZK timestamp path does not route Streamlink output through FFmpeg.",
    );
    assert_match(
        &recorder,
        r#"\.arg\("--player-args"\)"#,
        "CHZZK timestamp path does not configure FFmpeg player arguments.",
    );
    assert_match(
        &recorder,
        r"-fflags \+genpts\+discardcorrupt",
        "CHZZK FFmpeg input does not regenerate timestamps.",
    );
    assert_not_match(
        &recorder,
        r"-bsf:v h264_mp4toannexb",
        "CHZZK MPEG-TS must rely on FFmpeg mpegts auto_bsf so both H.264 and HEVC streams copy cleanly.",
    );
    assert_match(
        &recorder,
        r"-f mpegts",
        "CHZZK output is not a genuine MPEG-TS container.",
    );
    assert_match(
        &recorder,
        r"-mpegts_flags resend_headers",
        "CHZZK MPEG-TS does not resend headers.",
    );
    assert_match(
        &recorder,
        r"-mpegts_copyts 0",
        "CHZZK MPEG-TS must not preserve the source broadcast clock.",
    );
    assert_match(
        &recorder,
        r"-avoid_negative_ts make_zero",
        "CHZZK MPEG-TS timestamps are not rebased to zero.",
    );
    assert_match(
        &recorder,
        r"-avioflags direct",
        "CHZZK MPEG-TS live output direct I/O policy is missing.",
    );
    assert_not_match(
        &recorder,
        r"frag_keyframe\+empty_moov\+default_base_moof",
        "CHZZK LIVE must not regress to fragmented MP4 output.",
    );
    assert_match(
        &recorder,
        r"(?s)if let Some\(\(ffmpeg, player_args\)\) = timestamp_player.*?--player.*?else.*?--output",
        "SOOP direct output and CHZZK FFmpeg-player output are not separated.",
    );
    assert_not_match(
        &recorder,
        r#""--ffmpeg-start-at-zero""#,
        "Ineffective --ffmpeg-start-at-zero path must not return.",
    );
    assert_match(
        &recorder,
        r"timestamp_rebase_player_args_write_zero_based_mpegts_stream_copy",
        "CHZZK MPEG-TS remux regression test is missing.",
    );
    assert_match(
        &recorder,
        r"finds_ffmpeg_from_official_streamlink_windows_layout",
        "Bundled Streamlink FFmpeg discovery regression test is missing.",
    );
    assert_match(
        &live,
        r"assert!\(start_at_zero\)",
        "CHZZK timestamp policy regression coverage is missing.",
    );

    assert_match(
        &platform,
        r"pub const fn live_output_extension",
        "Platform LIVE output extension mapping is missing.",
    );
    assert_match(
        &platform,
        r#"Self::Soop\s*=>\s*"ts""#,
        "SOOP LIVE output must remain .ts.",
    );
    assert_match(
        &platform,
        r#"Self::Chzzk\s*=>\s*"ts""#,
        "CHZZK LIVE genuine MPEG-TS output must use .ts.",
    );
    assert_match(
        &recorder,
        r"output_file_for_platform\(output_file, platform\)",
        "Recorder does not apply the platform LIVE output extension.",
    );
    assert_match(
        &recorder,
        r"uses_platform_specific_live_output_extension_without_overwriting_existing_file",
        "Platform output-extension regression test is missing.",
    );
    assert_match(
        &watcher,
        r"(?s)fn unique_output_file\(.*?platform: PlatformId",
        "Filename generation does not receive the LIVE platform.",
    );
    assert_match(
        &watcher,
        r"let extension = platform\.live_output_extension\(\)",
        "Filename collision scanning is not platform-extension aware.",
    );
    assert_match(
        &watcher,
        r"(?s)TITLE_NUMBER.*?\{extension\}",
        "TITLE_NUMBER collisions are not checked with the platform extension.",
    );
    assert_match(
        &watcher,
        r"title_number_collision_scans_with_platform_extension",
        "Platform-aware TITLE_NUMBER collision regression test is missing.",
    );

    assert_match(
        &watcher,
        r"fn channels_require_soop",
        "SOOP credential gating helper is missing.",
    );
    assert_match(
        &watcher,
        r"require_soop:\s*bool",
        "WatcherConfig does not receive the SOOP requirement boundary.",
    );
    assert_match(
        &watcher,
        r"CLOUDFLARE_API_KEY is empty while an enabled SOOP channel exists",
        "SOOP Worker validation is not scoped to enabled SOOP channels.",
    );
    assert_match(
        &watcher,
        r"channels_require_soop\(&channels\)",
        "Watcher startup does not derive SOOP credential requirements from channels.",
    );

    assert_match(
        &watcher,
        r"fn scoped_channel_target",
        "Watcher scoped channel-target parser is missing.",
    );
    assert_match(
        &watcher,
        r"channel_key\(platform, account\)",
        "Watcher commands are not resolved through composite platform/account key.",
    );
    assert_match(
        &watcher,
        r"scoped_channel_command_targets_composite_identity",
        "Composite channel-command regression test is missing.",
    );
    assert_match(
        &gui_sources,
        r"pub fn scoped_target\(platform: PlatformId, account: &str\)",
        "Native LIVE controls must construct a platform-scoped channel target.",
    );
    assert_match(
        &gui_sources,
        r"scoped_target\(channel\.platform, &channel\.account\)",
        "Native LIVE rows must use the composite platform/account target.",
    );

    assert_match(
        &primary,
        r"provider\(channel\.platform\)\s*\.validate_account",
        "Shared platform channel validation is missing.",
    );
    assert_match(
        &core,
        r"pub async fn update_provider_configuration",
        "Shared provider configuration service is missing.",
    );
    assert_match(
        &core,
        r"NATIVE_PROVIDER_SECRET_KEYS",
        "Shared provider secret allowlist is missing.",
    );
    assert_match(
        &core,
        r#""CHZZK_NID_AUT""#,
        "CHZZK NID_AUT must remain a protected provider secret.",
    );
    assert_match(
        &core,
        r#""CHZZK_NID_SES""#,
        "CHZZK NID_SES must remain a protected provider secret.",
    );
    assert_match(
        &core,
        r"pub async fn test_soop_auth",
        "Shared SOOP authentication test service is missing.",
    );
    assert_match(
        &gui_sources,
        r"core\.update_provider_configuration",
        "Native Settings must save provider configuration through StreamArchiveCore.",
    );
    assert_match(
        &gui_sources,
        r"Request::ProviderTestSoop",
        "Native Settings must retain the SOOP authentication test action.",
    );
    assert_match(
        &gui_sources,
        r"core\.test_soop_auth\(\)",
        "Native SOOP authentication test must execute through StreamArchiveCore.",
    );
    assert_match(
        &gui_sources,
        r#""SOOP"\s*=>\s*PlatformId::Soop"#,
        "Native channel editor must explicitly select SOOP.",
    );
    assert_match(
        &gui_sources,
        r#""CHZZK"\s*=>\s*PlatformId::Chzzk"#,
        "Native channel editor must explicitly select CHZZK.",
    );
    assert_match(
        &gui_sources,
        r"pub\s+fn\s+channels_select_platform\(",
        "Native platform selector controller action is missing.",
    );
    assert_match(
        &gui_sources,
        r"\.select_platform\(index,\s*platform\)",
        "Native platform choice must reach the channel draft.",
    );
    assert_match(
        &gui_ui,
        r#"\["SOOP",\s*"CHZZK",\s*"KICK"\]"#,
        "Native platform selector must expose only supported platforms.",
    );
    assert_match(
        &gui_ui,
        r"channels_select_platform\(idx,",
        "Native platform selector must submit the selected platform.",
    );
    assert_match(
        &gui_sources,
        r"fn selecting_current_or_unknown_platform_does_not_modify_channel",
        "Explicit platform selection regression coverage is missing.",
    );
    assert_match(
        &gui_sources,
        r"prov_chzzk_aut",
        "Native Settings must retain CHZZK NID_AUT input.",
    );
    assert_match(
        &gui_sources,
        r"prov_chzzk_ses",
        "Native Settings must retain CHZZK NID_SES input.",
    );
    assert_match(
        &gui_ui,
        r"platform",
        "Native Queue/History presentation must retain platform identity.",
    );
    assert_not_match(
        &gui_sources,
        r"\baxum::|https?://127\.0\.0\.1|https?://localhost",
        "Native provider controls must not depend on the retired Web adapter.",
    );
    assert_match(
        &soop_vod,
        r"C:\\Program Files\\Streamlink\\ffmpeg\\ffmpeg\.exe",
        "SOOP VOD AUTO FFmpeg does not include Streamlink bundled FFmpeg.",
    );

    assert_match(
        &platform,
        r"PlatformId::Kick\s*=>\s*&kick::KICK",
        "KICK provider dispatch is missing.",
    );
    assert_match(
        &platform,
        r#"Self::Kick\s*=>\s*"ts""#,
        "KICK LIVE output must remain .ts.",
    );
    assert_match(
        &kick,
        r"live:\s*true",
        "KICK LIVE capability must remain enabled.",
    );
    assert_match(
        &kick,
        r"vod:\s*true",
        "KICK VOD capability must be enabled.",
    );
    assert_match(
        &kick_vod,
        r"playback_url/vod",
        "KICK VOD must use the authenticated playback response.",
    );
    assert_match(
        &kick_vod,
        r"spawn_owned",
        "KICK FFmpeg must retain owned-process lifecycle.",
    );
    assert_match(
        &backend,
        r#""KICK_SESSION_TOKEN""#,
        "KICK token must be a hidden secret.",
    );
    assert_not_match(
        &kick_vod,
        r"Command::new\([^)]*yt.dlp",
        "KICK direct MP4 must not use yt-dlp page extraction.",
    );
    assert_match(
        &kick,
        r"https://kick\.com/api/v2/channels/",
        "KICK lookup must use its canonical channel endpoint.",
    );
    assert_match(
        &kick_live,
        r"StreamInput::PluginUrl",
        "KICK recording must use the shared Streamlink plugin boundary.",
    );
    assert_match(
        &kick_live,
        r"cookies:\s*Vec::new\(\)",
        "Public KICK LIVE must not forward provider cookies.",
    );
    assert_match(
        &kick_live,
        r"start_at_zero:\s*false",
        "KICK must not inherit CHZZK-specific timestamp remux.",
    );
    assert_match(
        &kick_live,
        r"StatusCode::UNAUTHORIZED \| StatusCode::FORBIDDEN",
        "KICK access failures must be handled explicitly.",
    );
    assert_match(
        &kick_live,
        r"StatusCode::TOO_MANY_REQUESTS",
        "KICK rate limits must be handled explicitly.",
    );
    assert_match(
        &kick_live,
        r"offline_requires_an_explicit_null_on_a_matching_channel",
        "KICK offline/identity regression coverage is missing.",
    );
    assert_match(
        &kick_live,
        r"access_errors_are_not_offline_and_image_hosts_are_restricted",
        "KICK access error and thumbnail host coverage is missing.",
    );
    assert_match(
        &recorder,
        r"kick_live_plugin_records_without_cookies_or_chzzk_remux",
        "KICK recorder contract coverage is missing.",
    );
    assert_match(
        &recorder,
        r"kick_live_cancel_preserves_unrelated_process",
        "KICK owned-process cancellation coverage is missing.",
    );
    assert_match(
        &watcher,
        r"kick_session_and_scoped_commands_do_not_require_soop_credentials",
        "KICK credential independence coverage is missing.",
    );
    assert_match(
        &gui_sources,
        r#""KICK"\s*=>\s*PlatformId::Kick"#,
        "Native channel editor must explicitly select KICK.",
    );
    assert_match(
        &gui_sources,
        r"kick_selection_retains_channel_fields",
        "KICK channel draft regression coverage is missing.",
    );
}

#[test]
fn provider_e2e_contracts() {
    let recorder = read_repo_file("rust-runtime/src/recorder.rs");
    let soop_vod = read_repo_file("rust-runtime/src/platform/soop/vod.rs");
    let chzzk_vod = read_repo_file("rust-runtime/src/platform/chzzk/vod.rs");
    let media = read_repo_file("rust-runtime/src/media_process.rs");
    let fixture = read_repo_file("rust-runtime/tests/fixtures/media_tool_fixture.rs");
    let support = read_repo_file("rust-runtime/src/test_support.rs");
    let watcher = read_repo_file("rust-runtime/src/native_watcher.rs");

    assert_match(
        &recorder,
        r"mod provider_e2e",
        "LIVE provider E2E module is missing.",
    );
    assert_rust_test(
        &watcher,
        "live_configured_streamlink_resolution_uses_explicit_tool_path",
        "LIVE configured Streamlink resolution test is missing.",
    );
    assert_rust_test(
        &recorder,
        "soop_live_provider_e2e_preserves_unicode_argument_contract",
        "SOOP LIVE provider invocation E2E test is missing.",
    );
    assert_rust_test(
        &recorder,
        "chzzk_live_provider_e2e_preserves_cookie_player_and_quality_contract",
        "CHZZK LIVE provider invocation E2E test is missing.",
    );
    assert_rust_test(
        &recorder,
        "live_provider_e2e_maps_nonzero_and_spawn_failure",
        "LIVE provider failure-mapping E2E test is missing.",
    );
    assert_rust_test(
        &recorder,
        "live_provider_cancel_cleans_owned_descendant_not_unrelated_process",
        "LIVE provider owned-tree cancellation E2E test is missing.",
    );

    assert_match(
        &soop_vod,
        r"mod provider_e2e",
        "SOOP VOD provider E2E module is missing.",
    );
    assert_rust_test(
        &soop_vod,
        "soop_vod_provider_e2e_runs_full_manager_pipeline_without_internet",
        "SOOP VOD invocation E2E test is missing.",
    );
    assert_rust_test(
        &soop_vod,
        "soop_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout",
        "SOOP VOD result-mapping E2E test is missing.",
    );
    assert_rust_test(
        &soop_vod,
        "soop_vod_provider_e2e_cancel_cleans_owned_tree_not_unrelated_process",
        "SOOP VOD owned-tree cancellation E2E test is missing.",
    );

    assert_match(
        &chzzk_vod,
        r"mod provider_e2e",
        "CHZZK VOD provider E2E module is missing.",
    );
    assert_rust_test(
        &chzzk_vod,
        "chzzk_vod_provider_e2e_runs_prepared_download_to_completed_status",
        "CHZZK VOD Streamlink/FFmpeg invocation E2E test is missing.",
    );
    assert_rust_test(
        &chzzk_vod,
        "chzzk_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout",
        "CHZZK VOD result-mapping E2E test is missing.",
    );
    assert_rust_test(
        &chzzk_vod,
        "chzzk_vod_provider_e2e_cancel_cleans_streamlink_descendant",
        "CHZZK VOD cancellation E2E test is missing.",
    );

    assert_match(
        &media,
        r"run_media_process_with_atomic_cancel",
        "Provider capture bridge to the shared media runner is missing.",
    );
    assert_match(
        &media,
        r"without_timeout",
        "Cancellation-only media-process support is missing.",
    );
    assert_match(
        &soop_vod,
        r"run_capture_without_timeout\(ffmpeg",
        "SOOP FFmpeg merge must remain cancellation-only.",
    );
    assert_rust_test(
        &soop_vod,
        "soop_vod_merge_is_cancel_only_and_removes_partial_target_on_failure",
        "SOOP FFmpeg merge cleanup regression test is missing.",
    );
    assert_match(
        &soop_vod,
        r"run_media_process_with_atomic_cancel",
        "SOOP VOD capture path is not connected to shared media runner.",
    );
    assert_match(
        &chzzk_vod,
        r"run_media_process_with_atomic_cancel",
        "CHZZK VOD capture path is not connected to shared media runner.",
    );
    assert_match(
        &recorder,
        r"spawn_owned\(&mut command\)",
        "LIVE streaming must retain long-lived owned-process semantics.",
    );
    assert_match(
        &chzzk_vod,
        r"spawn_owned\(&mut streamlink_command\)",
        "CHZZK VOD streaming pipe must retain Streamlink ownership.",
    );
    assert_match(
        &chzzk_vod,
        r"spawn_owned\(&mut ffmpeg_command\)",
        "CHZZK VOD streaming pipe must retain FFmpeg ownership.",
    );

    assert_match(
        &fixture,
        r"enum ProviderTool",
        "Provider-capable Rust media fixture is missing.",
    );
    assert_match(
        &fixture,
        r"record_provider_invocation",
        "Provider fixture invocation recording is missing.",
    );
    assert_match(
        &fixture,
        r"write_all\(record\.as_bytes\(\)\)",
        "Provider fixture invocation records must append atomically.",
    );
    assert_match(
        &fixture,
        r#""run-spawn-child""#,
        "Provider fixture owned-descendant scenario is missing.",
    );
    assert_match(
        &fixture,
        r"STREAM_ARCHIVE_FIXTURE_GENERIC",
        "Provider fixture unrelated-process mode is missing.",
    );
    assert_match(
        &support,
        r#"(?s)tempfile::Builder::new\(\).*?\.prefix\("[^"]*[^\x00-\x7F][^"]*"\)"#,
        "Provider E2E Unicode temporary-path coverage is missing.",
    );
    assert_match(
        &support,
        r"invocations\.log",
        "Provider E2E invocation-log contract is missing.",
    );
    assert_not_match(
        &fixture,
        r"reqwest|https://sooplive\.com|https://chzzk\.naver\.com",
        "Provider executable fixture must not perform provider network I/O.",
    );
}

#[test]
fn unix_cli_contracts() {
    let cli = read_repo_file("rust-runtime/src/bin/stream-archive-cli.rs");
    let unix_cli = read_repo_file("rust-runtime/src/unix_cli.rs");
    let headless = read_repo_file("rust-runtime/src/headless.rs");
    let server = read_repo_file("rust-runtime/src/main.rs");
    let store = read_repo_file("rust-runtime/src/store.rs");
    let queue = read_repo_file("rust-runtime/src/queue_service.rs");
    let runtime_owner = read_repo_file("rust-runtime/src/runtime_owner.rs");
    let unix_control = read_repo_file("rust-runtime/src/unix_control.rs");
    let unix_test = read_repo_file("rust-runtime/tests/unix_cli.rs");

    for command in [
        "\"status\"",
        "\"settings\"",
        "\"providers\"",
        "\"channels\"",
        "\"watcher\"",
        "\"vod\"",
        "\"queue\"",
        "\"history\"",
        "\"backup\"",
        "\"storage\"",
        "\"logs\"",
    ] {
        assert_match(
            &cli,
            &regex::escape(command),
            &format!("Unix CLI routing is missing command {command}."),
        );
    }

    assert_match(
        &cli,
        r"run_management",
        "Unix management commands must route through the shared management boundary.",
    );
    assert_match(
        &unix_cli,
        r"StreamArchiveCore",
        "Unix CLI management must use StreamArchiveCore.",
    );
    assert_match(
        &unix_cli,
        r"open_observer",
        "One-shot Unix management must use the non-recovering observer core.",
    );
    assert_match(
        &store,
        r"pub fn open_observer",
        "Observer store open boundary is missing.",
    );
    assert_match(
        &queue,
        r"pub fn new_observer",
        "Observer Queue construction must not perform startup recovery.",
    );
    assert_match(
        &runtime_owner,
        r"try_lock_exclusive",
        "Cross-process runtime owner lock is missing.",
    );
    assert_match(
        &runtime_owner,
        r"RuntimeOwnerGuard",
        "Canonical runtime owner guard is missing.",
    );
    assert_match(
        &unix_cli,
        r"update_environment_settings",
        "Unix settings writes must use StreamArchiveCore validation.",
    );
    assert_match(
        &unix_cli,
        r"update_provider_configuration",
        "Unix provider writes must use protected-secret boundary.",
    );
    assert_match(
        &unix_cli,
        r"add_channel",
        "Unix channel add must use shared core atomic mutation.",
    );
    assert_match(
        &unix_cli,
        r"remove_channel",
        "Unix channel remove must use shared core atomic mutation.",
    );
    assert_match(
        &unix_cli,
        r"set_channel_enabled",
        "Unix channel enable/disable must use shared core atomic mutation.",
    );
    assert_match(
        &store,
        r"pub fn insert_channel",
        "Atomic SQLite channel insert boundary is missing.",
    );
    assert_match(
        &store,
        r"pub fn delete_channel",
        "Atomic SQLite channel delete boundary is missing.",
    );
    assert_match(
        &store,
        r"pub fn set_channel_enabled",
        "Atomic SQLite channel enable/disable boundary is missing.",
    );
    assert_match(
        &unix_cli,
        r"analyze_vod",
        "Unix VOD analyze must use the shared core.",
    );
    assert_match(
        &unix_cli,
        r"download_vod",
        "Unix VOD download must use the shared core.",
    );
    assert_match(
        &unix_cli,
        r"enqueue_vod",
        "Unix Queue writes must use the shared core.",
    );
    assert_match(
        &unix_cli,
        r"restore_backup",
        "Unix backup restore must use shared lifecycle safety checks.",
    );
    assert_match(
        &unix_cli,
        r"runtime_logs",
        "Unix logs command must use the shared core.",
    );
    assert_match(
        &unix_cli,
        r"providers secret <KEY> --stdin",
        "Provider secret CLI must document stdin-only transport.",
    );
    assert_match(
        &unix_cli,
        r"read_secret_stdin",
        "Provider secret CLI stdin boundary is missing.",
    );
    assert_not_match(
        &unix_cli,
        r"--password\s+\S+",
        "Unix CLI must not accept provider passwords as ordinary argv values.",
    );
    assert_not_match(
        &unix_cli,
        r"--nid-aut\s+\S+",
        "Unix CLI must not accept CHZZK secrets as ordinary argv values.",
    );
    assert_not_match(
        &unix_cli,
        r"reqwest|TcpListener|axum",
        "Unix management surface must not add a provider/Web control plane.",
    );
    assert_not_match(
        &unix_cli,
        r"std::process::Command|tokio::process::Command",
        "Daily-use Unix management commands must not directly spawn provider tools.",
    );

    assert_match(
        &unix_control,
        r"UnixListener",
        "Unix runtime control must use a local Unix-domain socket.",
    );
    assert_match(
        &unix_control,
        r"0o600",
        "Unix runtime control socket must be owner-only.",
    );
    assert_match(
        &unix_control,
        r#""channel\.password""#,
        "Protected-stream passwords must route to the running watcher owner.",
    );
    assert_match(
        &unix_control,
        r"core\.channel_password",
        "Runtime control must deliver protected-stream passwords to owner core.",
    );
    assert_match(
        &unix_control,
        r"core\.cancel_vod",
        "Runtime control must deliver VOD cancellation to owner core.",
    );
    assert_match(
        &unix_control,
        r#""queue\.cancel""#,
        "Active Queue cancellation must route through runtime owner.",
    );
    assert_match(
        &unix_control,
        r"core\.cancel_queue_item",
        "Runtime control must execute Queue cancellation on owner core.",
    );
    assert_not_match(
        &unix_control,
        r"TcpListener|axum|Router::new",
        "Runtime control must not reintroduce HTTP/Web control plane.",
    );

    assert_match(
        &headless,
        r"SignalKind::terminate",
        "Unix headless runtime must handle SIGTERM.",
    );
    assert_match(
        &headless,
        r"core\.shutdown\(\)\.await",
        "Headless signal path must call shared core shutdown.",
    );
    assert_match(
        &server,
        r"run_headless",
        "Compatibility server must use shared headless lifecycle.",
    );
    assert_match(
        &cli,
        r"run_serve",
        "CLI serve must use shared headless lifecycle.",
    );

    assert_match(
        &unix_test,
        r"CARGO_BIN_EXE_stream-archive-cli",
        "Unix CLI integration must execute the real CLI binary.",
    );
    assert_match(
        &unix_test,
        r"CARGO_BIN_EXE_stream-archive-server",
        "Unix compatibility-server integration is missing.",
    );
    assert_match(
        &unix_test,
        r"const SIGTERM: i32 = 15",
        "Unix CLI lifecycle smoke must send real SIGTERM.",
    );
    assert_match(
        &unix_test,
        r"unrelated headless runtime must survive",
        "Unix signal regression must protect unrelated processes.",
    );
    assert_match(
        &unix_test,
        r"one_shot_cli_observes_running_owner_without_recovering_active_rows",
        "One-shot observer regression test is missing.",
    );
    assert_match(
        &unix_test,
        r"runtime_owner_active",
        "Unix CLI integration must verify running-owner status contract.",
    );
    assert_match(
        &unix_test,
        r"cannot restore while another Stream Archive runtime owns",
        "Restore must be blocked while another runtime owns DB.",
    );
    assert_match(
        &unix_test,
        r"queue_cancel_reaches_active_runtime_owner",
        "Active Queue cancellation owner-routing regression test is missing.",
    );
    assert_match(
        &store,
        r"atomic_channel_inserts_preserve_changes_from_stale_observers",
        "Atomic channel writer regression test is missing.",
    );
    assert_match(
        &unix_cli,
        r"ensure_vod_success\(&final_status\)",
        "VOD analyze/download terminal failures must propagate non-zero CLI result.",
    );
    assert_match(
        &unix_test,
        r#"(?s)tempfile::Builder::new\(\).*?\.prefix\("[^"]*\s[^"]*"\)"#,
        "Unix CLI integration must cover whitespace temporary paths.",
    );
    assert_match(
        &unix_test,
        r#"(?s)tempfile::Builder::new\(\).*?\.prefix\("[^"]*[^\x00-\x7F][^"]*"\)"#,
        "Unix CLI integration must cover non-ASCII Unicode temporary paths.",
    );
}

#[test]
fn process_lifecycle_contracts() {
    let recorder = read_repo_file("rust-runtime/src/recorder.rs");
    let watcher = read_repo_file("rust-runtime/src/native_watcher.rs");
    let vod_facade = read_repo_file("rust-runtime/src/vod.rs");
    let soop_vod = read_repo_file("rust-runtime/src/platform/soop/vod.rs");
    let chzzk_vod = read_repo_file("rust-runtime/src/platform/chzzk/vod.rs");
    let runtime = read_repo_file("rust-runtime/src/platform_runtime.rs");
    let unix_runtime = read_repo_file("rust-runtime/src/platform_runtime_unix.rs");
    let all_runtime = format!("{recorder}\n{soop_vod}\n{chzzk_vod}\n{runtime}\n{unix_runtime}");
    let callers = format!("{recorder}\n{soop_vod}\n{chzzk_vod}");

    assert_match(
        &recorder,
        r"\.kill_on_drop\(true\)",
        "LIVE Streamlink child must retain kill-on-drop fallback.",
    );
    assert_not_match(
        &vod_facade,
        r"taskkill\.exe|yt-dlp|ffmpeg|sooplive\.com",
        "Root VOD facade must remain provider/process neutral.",
    );
    assert_match(
        &chzzk_vod,
        r"spawn_owned\(&mut streamlink_command\)",
        "CHZZK VOD Streamlink must enter retained ownership before execution.",
    );
    assert_match(
        &chzzk_vod,
        r"spawn_owned\(&mut ffmpeg_command\)",
        "CHZZK VOD FFmpeg must enter retained ownership before execution.",
    );
    assert_match(
        &chzzk_vod,
        r"streamlink_tree\.terminate\(&mut streamlink_child\)",
        "CHZZK VOD cancellation must terminate retained Streamlink ownership.",
    );
    assert_match(
        &chzzk_vod,
        r"ffmpeg_tree\.terminate\(&mut ffmpeg_child\)",
        "CHZZK VOD cancellation must terminate retained FFmpeg ownership.",
    );
    assert_match(
        &chzzk_vod,
        r"run_media_process_with_atomic_cancel",
        "CHZZK VOD one-shot capture commands must use shared media-process runner.",
    );
    assert_not_match(
        &chzzk_vod,
        r"terminate_owned\(&mut (?:streamlink_child|ffmpeg_child|child)\)",
        "CHZZK VOD must not fall back to compatibility tree recapture.",
    );
    assert_match(
        &chzzk_vod,
        r"abort_reader_tasks\(reader_tasks\)\.await",
        "CHZZK VOD cancellation must reap line readers.",
    );
    assert_match(
        &chzzk_vod,
        r"drain_failed_reader_tasks\(",
        "CHZZK VOD process failures must drain bounded stderr readers.",
    );
    assert_match(
        &chzzk_vod,
        r"join_reader_tasks\(tasks\)\.await",
        "CHZZK VOD reader tasks must be joined after draining.",
    );
    assert_match(
        &chzzk_vod,
        r"mpsc::channel::<String>\(256\)",
        "External-tool line transport must remain bounded.",
    );
    assert_match(
        &runtime,
        r"async fn spawn_owned\(command: &mut Command\)",
        "Retained owned-process spawn primitive is missing.",
    );
    assert_match(
        &runtime,
        r"async fn terminate_owned\(child: &mut Child\)",
        "Common owned-process termination primitive is missing.",
    );
    assert_match(
        &runtime,
        r"struct OwnedProcessTree",
        "Durable retained owned-tree type is missing.",
    );
    assert_match(
        &runtime,
        r"JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
        "Windows retained process tree must use kill-on-close Job ownership.",
    );
    assert_match(
        &runtime,
        r"AssignProcessToJobObject",
        "Windows retained process tree must assign owned members to the Job.",
    );
    assert_match(
        &runtime,
        r"TerminateJobObject",
        "Windows retained process tree must have an authoritative tree-termination primitive.",
    );
    assert_match(
        &runtime,
        r"QueryInformationJobObject",
        "Retained Job cleanup must query the Job directly.",
    );
    assert_match(
        &runtime,
        r"ActiveProcesses",
        "Retained Job cleanup must verify no Job members remain.",
    );
    assert_match(
        &runtime,
        r"self\.job\.active_process_count\(\)",
        "Retained Job cleanup must loop on authoritative Job membership.",
    );
    assert_not_match(
        &runtime,
        r"self\.root_pid",
        "Retained process ownership must not reuse a historical root PID after spawn.",
    );

    assert_match(
        &runtime,
        r"CREATE_SUSPENDED",
        "Windows retained LIVE spawn must create the child suspended.",
    );
    assert_match(
        &runtime,
        r"CREATE_NO_WINDOW",
        "Windows retained media-tool spawn must suppress console windows.",
    );
    assert_match(
        &runtime,
        r"creation_flags\(CREATE_SUSPENDED\s*\|\s*CREATE_NO_WINDOW\)",
        "Windows retained LIVE spawn must apply suspended, no-console creation flags.",
    );
    assert_match(
        &runtime,
        r"fn configure_background_command\(command: &mut Command\)",
        "Short-lived native helper commands need a no-console Windows boundary.",
    );
    assert_match(
        &runtime,
        r"configure_no_window\(command\)",
        "Short-lived native helper commands must delegate to no-console boundary.",
    );
    assert_match(
        &recorder,
        r"configure_background_command\(&mut can_handle\)",
        "Streamlink URL preflight must run without a visible console window.",
    );
    assert_match(
        &recorder,
        r"configure_background_command\(&mut help\)",
        "Streamlink capability preflight must run without a visible console window.",
    );
    assert_match(
        &runtime,
        r"configure_no_window\(&mut taskkill\)",
        "Windows taskkill fallback must not flash a console window.",
    );
    assert_match(
        &runtime,
        r"configure_std_no_window\(&mut command\)",
        "Windows ACL helper must not flash a console window.",
    );
    assert_match(
        &runtime,
        r"capture_suspended_child\(&child\)",
        "Windows retained LIVE spawn must capture exact suspended Child handle.",
    );
    assert_match(
        &runtime,
        r"assign_handle\(root_handle\)",
        "Windows Job capture must assign the exact spawned root handle.",
    );
    assert_match(
        &runtime,
        r"resume_child_threads\(&child\)",
        "Windows retained LIVE spawn must resume only after Job ownership.",
    );
    assert_match(
        &runtime,
        r"TH32CS_SNAPTHREAD",
        "Suspended child resume must enumerate child threads explicitly.",
    );
    assert_match(
        &runtime,
        r"ResumeThread",
        "Suspended child must be resumed after exact Job assignment.",
    );
    assert_match(
        &runtime,
        r"\.raw_handle\(\)",
        "Windows retained ownership must use exact spawned Child handle.",
    );
    assert_match(
        &recorder,
        r"spawn_owned\(&mut command\)\.await",
        "LIVE recorder must use the suspended owned-spawn boundary.",
    );
    assert_not_match(
        &recorder,
        r"OwnedProcessTree::capture\(&child\)",
        "LIVE recorder must not reconstruct retained ownership after normal spawn.",
    );
    assert_not_match(
        &recorder,
        r"OwnedProcessTree::capture\(pid\)",
        "LIVE recorder must never establish retained ownership from reusable numeric PID alone.",
    );

    assert_match(
        &runtime,
        r"platform_runtime_unix\.rs",
        "Unix retained ownership module is not connected.",
    );
    assert_match(
        &runtime,
        r"unix_group::configure_process_group\(command\)",
        "Unix owned spawn must configure an isolated process group.",
    );
    assert_match(
        &runtime,
        r"unix_group::OwnedProcessGroup::from_spawned_child\(&child\)",
        "Unix owned spawn must retain group identity.",
    );
    assert_match(
        &unix_runtime,
        r"\.process_group\(0\)",
        "Unix child must become a process-group leader before exec.",
    );
    assert_match(
        &unix_runtime,
        r"posix_getpgid\(pid\)",
        "Unix compatibility capture must verify existing process-group identity.",
    );
    assert_match(
        &unix_runtime,
        r"actual != expected",
        "Unix compatibility capture must reject non-leader children.",
    );
    assert_match(
        &unix_runtime,
        r"posix_kill\(-self\.pgid, signal\)",
        "Unix termination must target only the retained process group.",
    );
    assert_match(
        &unix_runtime,
        r"self\.signal_group\(SIGTERM\)",
        "Unix retained cleanup must request graceful termination first.",
    );
    assert_match(
        &unix_runtime,
        r"self\.signal_group\(SIGKILL\)",
        "Unix retained cleanup must escalate to SIGKILL when group remains.",
    );
    assert_match(
        &unix_runtime,
        r"impl Drop for OwnedProcessGroup",
        "Unix retained ownership must fail closed on drop.",
    );
    assert_not_match(
        &unix_runtime,
        r"pkill|killall",
        "Unix retained cleanup must not use process-name-wide termination.",
    );

    assert_match(
        &runtime,
        r"capture_running_child\(child: &Child\)",
        "Compatibility running-child capture boundary is missing.",
    );
    assert_match(
        &runtime,
        r"failed to assign exact running root pid=.*before snapshot",
        "Running-child capture must assign exact root before snapshots.",
    );
    assert_match(
        &runtime,
        r"snapshot_root_is_current",
        "Compatibility snapshots must bind snapshot root to original process identity.",
    );
    assert_match(
        &runtime,
        r"snapshot\.present\.contains\(&root\.pid\)",
        "Compatibility snapshots must require original root PID present.",
    );
    assert_match(
        &runtime,
        r"current_root == Some\(root\)",
        "Compatibility snapshots must reject absent or reused root identities.",
    );
    assert_match(
        &runtime,
        r"compatibility snapshot root identity is absent or changed",
        "Absent/reused compatibility roots must fail capture.",
    );
    assert_match(
        &runtime,
        r"cutoff_creation_time",
        "ToolHelp descendant absorption must carry a snapshot creation-time cutoff.",
    );
    assert_match(
        &runtime,
        r"identity\.creation_time <= snapshot\.cutoff_creation_time",
        "Snapshot descendants must be creation-time bound.",
    );
    assert_match(
        &runtime,
        r"snapshot-bound pid=",
        "Snapshot descendant assignment must use verified handle/identity.",
    );
    assert_match(
        &runtime,
        r"GetProcessTimes",
        "Windows process identity must include creation time.",
    );
    assert_match(
        &runtime,
        r"struct ProcessIdentity",
        "Windows snapshot assignments must carry stable process identity.",
    );
    assert_match(
        &runtime,
        r"CreateToolhelp32Snapshot",
        "Windows compatibility capture must discover already-existing descendants.",
    );
    assert_match(
        &runtime,
        r"Process32FirstW failed",
        "ToolHelp process enumeration must propagate first-entry failures.",
    );
    assert_match(
        &runtime,
        r"Process32NextW failed",
        "ToolHelp process enumeration must distinguish end-of-list from failures.",
    );
    assert_match(
        &runtime,
        r"Thread32First failed",
        "ToolHelp thread enumeration must propagate first-entry failures.",
    );
    assert_match(
        &runtime,
        r"Thread32Next failed",
        "ToolHelp thread enumeration must distinguish end-of-list from failures.",
    );
    assert_match(
        &runtime,
        r"ERROR_NO_MORE_FILES",
        "ToolHelp enumeration may only treat ERROR_NO_MORE_FILES as end-of-list.",
    );

    assert_match(
        &recorder,
        r"owned_tree:\s*OwnedProcessTree",
        "Recording must retain durable process-tree ownership.",
    );
    assert_match(
        &recorder,
        r"rec\.owned_tree\s*\.terminate_now\(\)",
        "Root-exit polling must enforce retained-tree cleanup.",
    );
    assert_match(
        &recorder,
        r"rec\.owned_tree\s*\.terminate\(&mut rec\.child\)",
        "Recorder stop must always terminate through retained tree ownership.",
    );
    assert_not_match(
        &recorder,
        r"if rec\.child\.try_wait\(\)\?\.is_none\(\)\s*\{\s*return terminate_owned_checked",
        "Recorder stop must not bypass retained-tree cleanup.",
    );
    assert_match(
        &watcher,
        r"state\.recording\s*=\s*Some\(rec\)",
        "Watcher must retain Recording owner on unexpected cleanup error.",
    );
    assert_match(
        &watcher,
        r"channel removal deferred while recorder cleanup is retained",
        "Removed channels must remain tracked if recorder cleanup fails.",
    );
    assert_rust_test(
        &runtime,
        "owned_child_is_terminated_and_reaped",
        "Owned-child termination/reaping behavior test is missing.",
    );
    assert_rust_test(
        &runtime,
        "unix_owned_spawn_retains_descendant_after_root_exit",
        "Unix process-group descendant ownership test is missing.",
    );
    assert_rust_test(
        &runtime,
        "windows_owned_spawn_retains_immediate_descendant",
        "Windows suspended-spawn descendant inheritance test is missing.",
    );
    assert_match(
        &runtime,
        r"snapshot_cutoff_rejects_newer_identity",
        "Windows snapshot PID-reuse cutoff regression check is missing.",
    );
    assert_match(
        &runtime,
        r"snapshot_root_guard_rejects_absent_or_reused_identity",
        "Windows compatibility snapshot root-identity regression check is missing.",
    );
    assert_match(
        &runtime,
        r"taskkill\.exe",
        "Windows exact-PID tree fallback for non-retained callers is missing.",
    );
    assert_match(
        &runtime,
        r#"\.arg\("/PID"\)"#,
        "Windows non-retained termination fallback must target owned PID.",
    );
    assert_match(
        &runtime,
        r#"\.arg\("/T"\)"#,
        "Windows non-retained termination fallback must include descendants.",
    );
    assert_match(
        &runtime,
        r#"\.arg\("/F"\)"#,
        "Windows non-retained termination fallback must force cleanup when required.",
    );
    assert_not_match(
        &all_runtime,
        r#"(?i)taskkill(?:\.exe)?[^\r\n]*(?:/IM|\.arg\("/IM"\))"#,
        "Process-name-wide taskkill /IM is forbidden.",
    );
    assert_not_match(
        &all_runtime,
        r"(?i)(?:pkill|killall)[^\r\n]*",
        "Process-name-wide Unix termination is forbidden.",
    );
    assert_not_match(
        &callers,
        r"taskkill\.exe",
        "Provider/runtime callers must use the common process boundary.",
    );

    let soop_owned_spawns = Regex::new(r"spawn_owned\(&mut command\)")
        .unwrap()
        .find_iter(&soop_vod)
        .count();
    assert!(
        soop_owned_spawns >= 1,
        "SOOP VOD streaming progress must use retained owned spawn; found {soop_owned_spawns}"
    );
    assert_match(
        &soop_vod,
        r"run_media_process_with_atomic_cancel",
        "SOOP VOD one-shot capture commands must use shared media-process runner.",
    );
    assert_match(
        &soop_vod,
        r"owned_tree\.terminate\(&mut child\)\.await",
        "SOOP VOD cancellation must terminate through retained ownership.",
    );
    assert_match(
        &soop_vod,
        r"owned_tree\.terminate_now\(\)",
        "SOOP VOD root-exit handling must clean retained descendants.",
    );
    assert_not_match(
        &soop_vod,
        r"terminate_owned\(&mut child\)",
        "SOOP VOD must not use compatibility child-only termination.",
    );
    assert_not_match(
        &soop_vod,
        r"\.kill_on_drop\(false\)",
        "SOOP VOD external tools must retain direct-child drop fallback.",
    );
    assert_match(
        &soop_vod,
        r"MediaProcessOutcome::ProcessFailure",
        "SOOP capture must map shared-runner non-zero process failures.",
    );
    assert_match(
        &soop_vod,
        r"if exit\.success\(\)",
        "SOOP progress must distinguish successful exits.",
    );
}

#[test]
fn media_process_contracts() {
    let media = read_repo_file("rust-runtime/src/media_process.rs");
    let diagnostics = read_repo_file("rust-runtime/src/diagnostics.rs");
    let cli = read_repo_file("rust-runtime/src/bin/stream-archive-cli.rs");

    assert_match(
        &media,
        r"run_media_process",
        "Shared media process runner is missing.",
    );
    assert_match(
        &media,
        r"spawn_owned\(&mut command\)",
        "Shared media runner must reuse retained owned-process spawn.",
    );
    assert_match(
        &media,
        r"capture_bounded",
        "Media output capture must remain bounded.",
    );
    assert_match(
        &media,
        r"DEFAULT_CAPTURE_LIMIT",
        "Media output capture limit is missing.",
    );
    assert_match(
        &media,
        r"MediaCancellation",
        "Media cancellation contract is missing.",
    );
    assert_match(
        &media,
        r"without_timeout",
        "Shared media runner cancellation-only mode is missing.",
    );
    assert_rust_test(
        &media,
        "without_timeout_allows_completion_past_configured_deadline",
        "Cancellation-only media-process regression test is missing.",
    );
    assert_match(
        &media,
        r"probe_tool_version",
        "Media tool version probe is missing.",
    );
    assert_match(
        &diagnostics,
        r"collect_active_local_preflight",
        "Opt-in active local preflight is missing.",
    );
    assert_match(
        &diagnostics,
        r"probe_tool_version",
        "Active preflight must delegate tool execution to shared probe.",
    );
    assert_match(
        &cli,
        r"--active-tools",
        "CLI must keep active media-tool probing explicit.",
    );
    assert_rust_test(
        &media,
        "spawn_preserves_argument_boundaries_and_unicode_output",
        "Argument/Unicode media-process test is missing.",
    );
    assert_rust_test(
        &media,
        "large_concurrent_output_is_bounded_and_keeps_tail",
        "Bounded output integration test is missing.",
    );
    assert_rust_test(
        &media,
        "timeout_terminates_owned_process_tree",
        "Timeout ownership test is missing.",
    );
    assert_rust_test(
        &media,
        "timeout_does_not_wait_forever_for_detached_pipe_holder",
        "Detached pipe-holder bounded-drain test is missing.",
    );
    assert_rust_test(
        &media,
        "cancellation_terminates_owned_tree_but_not_unrelated_process",
        "Cancellation/unrelated-process test is missing.",
    );
    assert_rust_test(
        &media,
        "completed_process_wins_over_late_cancel_and_timeout",
        "Late-cancellation terminal-result regression test is missing.",
    );
    assert_rust_test(
        &media,
        "version_probe_supports_all_media_tools",
        "Version probe integration test is missing.",
    );
}

#[test]
fn storage_ownership_contracts() {
    let platform = read_repo_file("rust-runtime/src/platform/mod.rs");
    let chzzk = read_repo_file("rust-runtime/src/platform/chzzk/mod.rs");
    let vod_facade = read_repo_file("rust-runtime/src/platform/vod.rs");
    let chzzk_vod = read_repo_file("rust-runtime/src/platform/chzzk/vod.rs");
    let queue_service = read_repo_file("rust-runtime/src/queue_service.rs");
    let auth = read_repo_file("rust-runtime/src/platform/chzzk/auth.rs");
    let recorder = read_repo_file("rust-runtime/src/recorder.rs");
    let main = read_repo_file("rust-runtime/src/main.rs");
    let headless = read_repo_file("rust-runtime/src/headless.rs");
    let core = read_repo_file("rust-runtime/src/app_core.rs");
    let runtime = read_repo_file("rust-runtime/src/platform_runtime.rs");

    assert_match(
        &chzzk,
        r"pub mod vod;",
        "CHZZK VOD provider module is not registered.",
    );
    assert_match(
        &chzzk,
        r"vod:\s*true",
        "CHZZK VOD capability is not enabled.",
    );
    assert_match(
        &chzzk,
        r"chzzk\.naver\.com",
        "CHZZK provider does not validate the VOD host.",
    );
    assert_match(
        &chzzk,
        r#"parts\[0\] == "video""#,
        "CHZZK /video/<id> URL recognition is missing.",
    );
    assert_match(
        &platform,
        r"detects_soop_and_chzzk_vod_urls",
        "Platform VOD routing regression coverage is missing.",
    );

    assert_match(
        &vod_facade,
        r"chzzk:\s*chzzk::vod::VodManager",
        "Common VOD facade does not own a CHZZK provider manager.",
    );
    assert_match(
        &vod_facade,
        r"PlatformId::Chzzk\s*=>\s*self\.chzzk\.analyze",
        "CHZZK analyze is not routed through common VOD facade.",
    );
    assert_match(
        &vod_facade,
        r"PlatformId::Chzzk\s*=>\s*self\.chzzk\.download",
        "CHZZK download is not routed through common VOD facade.",
    );
    assert_match(
        &vod_facade,
        r"PlatformId::Chzzk\s*=>\s*chzzk::vod::validate_download_request",
        "CHZZK queue validation is not provider-routed.",
    );
    assert_match(
        &vod_facade,
        r"lifecycle:\s*Mutex<\(\)>",
        "Common VOD facade does not serialize provider starts.",
    );
    assert_match(
        &vod_facade,
        r"ensure_idle\(\)\.await\?",
        "Cross-provider VOD start guard is missing.",
    );
    assert_match(
        &vod_facade,
        r"running_provider\(\)",
        "VOD status/cancel cannot recover the actually running provider.",
    );
    assert_not_match(
        &queue_service,
        r"api\.chzzk\.naver\.com|NID_AUT|NID_SES",
        "Provider-specific CHZZK network/auth logic leaked into Queue.",
    );

    assert_match(
        &chzzk_vod,
        r"ChzzkAuth::load\(\)",
        "CHZZK VOD does not reuse encrypted CHZZK auth store.",
    );
    assert_match(
        &chzzk_vod,
        r"chzzk-cookies\.txt",
        "CHZZK VOD temporary Netscape cookie transport is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"JobDirGuard",
        "CHZZK VOD temporary job/cookie cleanup guard is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"cleanup_stale_job_dirs",
        "Stale CHZZK VOD job directory scavenging is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"backend\.join\("\.stream-archive"\)\.join\("vod"\)"#,
        "CHZZK VOD must create new temp jobs under .stream-archive.",
    );
    assert_match(
        &chzzk_vod,
        r"fn legacy_vod_job_root",
        "Bounded legacy CHZZK temp cleanup boundary is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"backend\.join\("\.rust-web"\)\.join\("vod"\)"#,
        "Legacy CHZZK temp namespace must remain cleanup-only.",
    );
    assert_match(
        &chzzk_vod,
        r"cleanup_stale_job_dirs_in_root\(&legacy_vod_job_root\(backend\)\)",
        "Legacy CHZZK temp jobs must be scavenged during upgrade.",
    );
    assert_match(
        &chzzk_vod,
        r"restrict_private_dir",
        "CHZZK VOD temporary directory permission hardening is missing.",
    );
    assert_match(
        &runtime,
        r"icacls\.exe",
        "Windows current-user-only CHZZK temp ACL setup is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"NID_\(\?:AUT\|SES\)",
        "CHZZK VOD secret redaction coverage is missing.",
    );
    assert_not_match(
        &chzzk_vod,
        r"NID_AUT=.*--|NID_SES=.*--",
        "CHZZK cookies must not be put directly on process arguments.",
    );
    assert_match(
        &auth,
        r"CHZZK_NID_AUT",
        "Shared CHZZK auth key disappeared.",
    );
    assert_match(
        &auth,
        r"CHZZK_NID_SES",
        "Shared CHZZK auth key disappeared.",
    );

    assert_match(
        &main,
        r"run_headless\(",
        "Compatibility server entrypoint must delegate to shared headless lifecycle.",
    );
    assert_match(
        &headless,
        r"StreamArchiveCore::open\(&backend_dir\)",
        "Headless runtime must initialize services through StreamArchiveCore.",
    );
    assert_match(
        &core,
        r"VodManager::new\(backend_dir\.clone\(\), logs\.clone\(\)\)",
        "Shared core must construct the canonical VOD manager.",
    );
    assert_not_match(
        &format!("{main}\n{headless}"),
        r"TcpListener::bind|STREAM_ARCHIVE_BIND",
        "Retired Web listener ownership must not return.",
    );

    assert_match(
        &chzzk_vod,
        r"use fs2::FileExt;",
        "CHZZK VOD per-job OS locking is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"JOB_LOCK_FILE_NAME:\s*&str\s*=\s*"owner\.lock""#,
        "CHZZK VOD ownership lock filename is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"lock\.lock_exclusive\(\)",
        "CHZZK VOD job does not acquire an exclusive ownership lock.",
    );
    assert_match(
        &chzzk_vod,
        r"try_lock_exclusive\(\)",
        "Stale cleanup does not probe ownership locks.",
    );
    assert_match(
        &chzzk_vod,
        r"FileExt::unlock\(&self\.lock\)",
        "CHZZK VOD job guard does not release ownership lock.",
    );
    assert_match(
        &chzzk_vod,
        r"name == COOKIE_FILE_NAME \|\| name == JOB_LOCK_FILE_NAME",
        "Retry/cancel cleanup must preserve cookie and lock files.",
    );
    assert_match(
        &chzzk_vod,
        r"active_job_lock_survives_scavenging_until_release",
        "Active-owner stale-cleanup regression test is missing.",
    );

    assert_match(
        &chzzk_vod,
        r"struct ChzzkTools",
        "CHZZK Streamlink tool bundle is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"api\.chzzk\.naver\.com/service/v3/videos/",
        "CHZZK VOD metadata is not read from the CHZZK API.",
    );
    assert_not_match(
        &chzzk_vod,
        r"(?s)duration_seconds.*?/ 1000;",
        "CHZZK API duration is already seconds and must not be divided by 1000.",
    );
    assert_match(
        &chzzk_vod,
        r"chzzk_api_metadata_preserves_seconds",
        "CHZZK API seconds regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"MEDIA_FILE_NAME:\s*&str\s*=\s*"media\.ts""#,
        "CHZZK VOD short MPEG-TS staging filename is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"claim_collision_path\((?:&)?output_dir,\s*&base,\s*"ts"\)"#,
        "CHZZK VOD final output is not MPEG-TS.",
    );
    assert_match(
        &chzzk_vod,
        r"staging_output\s*=\s*job_dir\.join\(MEDIA_FILE_NAME\)",
        "CHZZK VOD does not write into its private short staging path.",
    );
    assert_match(
        &chzzk_vod,
        r"finalize_output\(&staged_file,\s*&destination,\s*cancel\)",
        "CHZZK VOD staging output is not finalized through claimed destination.",
    );
    assert_match(
        &chzzk_vod,
        r"cleanup_job_media",
        "CHZZK VOD retry/cancel cleanup is not scoped to owned job directory.",
    );
    assert_match(
        &chzzk_vod,
        r"resolve_streamlink_tool",
        "CHZZK VOD Streamlink resolver is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"STREAMLINK_PATH",
        "CHZZK VOD does not honor configured Streamlink path.",
    );
    assert_match(
        &chzzk_vod,
        r#""--json""#,
        "CHZZK VOD Streamlink quality analysis is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#""--http-cookies-file""#,
        "CHZZK VOD Streamlink cookie transport is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#""--ffmpeg-ffmpeg""#,
        "CHZZK VOD does not provide FFmpeg to Streamlink.",
    );
    assert_match(
        &chzzk_vod,
        r#""--ffmpeg-fout""#,
        "CHZZK VOD does not force Streamlink DASH mux output format.",
    );
    assert_match(
        &chzzk_vod,
        r#""mpegts""#,
        "CHZZK VOD Streamlink DASH mux is not forced to MPEG-TS.",
    );
    assert_match(
        &chzzk_vod,
        r#""--stdout""#,
        "CHZZK VOD Streamlink MPEG-TS pipe is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#""-progress""#,
        "CHZZK VOD owned FFmpeg progress output is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#""pipe:1""#,
        "CHZZK VOD FFmpeg progress pipe is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"ffmpeg_progress_seconds",
        "CHZZK VOD FFmpeg media progress parser is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"ffmpeg_media_progress_drives_chzzk_percent_and_time",
        "CHZZK VOD progress regression test is missing.",
    );
    assert_match(
        &recorder,
        r"pub\(crate\) fn timestamp_rebase_player_args",
        "CHZZK LIVE MPEG-TS player helper disappeared.",
    );
    assert_match(
        &recorder,
        r"-f mpegts",
        "Shared CHZZK player pipeline no longer emits MPEG-TS.",
    );
    assert_match(
        &recorder,
        r"-c copy",
        "Shared CHZZK player pipeline is no longer stream-copy only.",
    );
    assert_match(
        &chzzk_vod,
        r"streamlink_json_builds_quality_options",
        "CHZZK Streamlink quality regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"maps_legacy_height_quality_to_streamlink_selector",
        "CHZZK legacy quality mapping regression test is missing.",
    );
    assert_not_match(
        &chzzk_vod,
        r#""--dump-single-json"|"--merge-output-format"|run_ffmpeg_fallback|is_source_url_extractor_bug|tools\.yt_dlp"#,
        "Legacy CHZZK yt-dlp/direct-DASH fallback code returned.",
    );
    assert_match(
        &runtime,
        r"taskkill\.exe",
        "Windows owned-process cancellation path is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"spawn_owned\(&mut ffmpeg_command\)",
        "Downstream FFmpeg must establish retained ownership.",
    );
    assert_match(
        &chzzk_vod,
        r"streamlink_tree\.terminate\(&mut streamlink_child\)",
        "Streamlink retained tree must be terminated on downstream setup failures.",
    );
    assert_match(
        &runtime,
        r#"\.arg\("/PID"\)"#,
        "CHZZK VOD cancellation is not PID scoped.",
    );
    assert_match(
        &runtime,
        r#"\.arg\("/T"\)"#,
        "CHZZK VOD cancellation does not include owned child tree.",
    );
    assert_not_match(
        &format!("{chzzk_vod}\n{runtime}"),
        r"(?i)taskkill[^\r\n]*/IM",
        "CHZZK VOD must never kill processes by image name.",
    );

    assert_match(
        &chzzk_vod,
        r#"DESTINATION_CLAIM_SUFFIX:\s*&str\s*=\s*"\.stream-archive\.claim""#,
        "Destination claim sidecar is missing.",
    );
    assert_match(
        &chzzk_vod,
        r#"FINALIZING_SUFFIX:\s*&str\s*=\s*"\.stream-archive\.finalizing""#,
        "Exact app-owned finalizing suffix is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"hide_destination_claim",
        "Windows destination claim sidecars must be hidden.",
    );
    assert_match(
        &chzzk_vod,
        r"FILE_ATTRIBUTE_HIDDEN",
        "Windows destination claim sidecars must use hidden attribute.",
    );
    assert_match(
        &chzzk_vod,
        r"claim_collision_path",
        "Atomic destination claim helper is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"lock\.try_lock_exclusive\(\)",
        "Destination filename claim is not protected by OS lock.",
    );
    assert_match(
        &chzzk_vod,
        r"reusable lock anchor",
        "Destination claim sidecars must remain reusable lock anchors.",
    );
    assert_not_match(
        &chzzk_vod,
        r"remove_file\(&self\.claim_path\)",
        "Destination claim teardown must not unlink reusable lock anchor.",
    );
    assert_match(
        &chzzk_vod,
        r"finalizing_path",
        "Destination-side atomic publication temp path is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"COPY_BUFFER_SIZE",
        "Cancellable bounded copy buffer is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"cancel\.load\(Ordering::SeqCst\)",
        "Cross-volume publication does not observe cancellation.",
    );
    assert_match(
        &chzzk_vod,
        r"sync_all\(\)",
        "Destination-side copied media is not synced before publication.",
    );
    assert_match(
        &chzzk_vod,
        r"publish_by_copy",
        "Cross-volume atomic publication helper is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"fs::hard_link\(",
        "Final publication must use atomic no-replace primitive.",
    );
    assert_match(
        &chzzk_vod,
        r"PublishOutcome::Collision",
        "Late external destination collisions must be detected.",
    );
    assert_not_match(
        &chzzk_vod,
        r"fs::copy\(source,\s*target\)",
        "CHZZK VOD must never copy directly into final pathname.",
    );
    let platform_vod = read_repo_file("rust-runtime/src/platform/vod.rs");
    assert_match(
        &chzzk_vod,
        r"ProviderVodRunner",
        "CHZZK VOD must delegate job lifecycle to shared ProviderVodRunner.",
    );
    assert_match(
        &platform_vod,
        r"should_mark_cancelled",
        "Late cancellation completion precedence helper is missing.",
    );
    assert_match(
        &platform_vod,
        r#"state != "COMPLETED""#,
        "Successfully published file must remain COMPLETED after late cancel.",
    );
    assert_match(
        &chzzk_vod,
        r"concurrent_destination_claims_choose_distinct_collision_paths",
        "Concurrent destination claim test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"destination_claim_sidecar_is_reused_after_release",
        "Reusable destination-claim sidecar test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"late_external_collision_is_not_clobbered_and_retargets",
        "Late external collision test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"cancelled_copy_publish_keeps_final_unpublished",
        "Cancelled destination publication test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"completed_state_wins_over_late_cancellation",
        "COMPLETED-versus-late-cancel test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"stale_finalizing_is_reclaimed_under_destination_claim",
        "Stale finalizing reclamation test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"atomic_copy_publish_keeps_partial_data_out_of_final_name",
        "Atomic publication success test is missing.",
    );

    assert_match(
        &chzzk_vod,
        r"Client::builder\(\)",
        "CHZZK video-detail API client must use an explicit builder.",
    );
    assert_match(
        &chzzk_vod,
        r"connect_timeout\(CHZZK_API_CONNECT_TIMEOUT\)",
        "CHZZK video-detail API connect timeout is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"\.timeout\(CHZZK_API_TOTAL_TIMEOUT\)",
        "CHZZK video-detail API total timeout is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"tokio::select!",
        "CHZZK video-detail API lookup is not cancellation-raced.",
    );
    assert_match(
        &chzzk_vod,
        r"wait_for_cancel\(cancel\)",
        "CHZZK video-detail API cancellation waiter is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"chzzk_api_timeouts_are_bounded",
        "CHZZK API timeout regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"cancellation_waiter_observes_atomic_flag",
        "CHZZK API cancellation regression test is missing.",
    );

    assert_match(
        &chzzk_vod,
        r"part_count:\s*1",
        "CHZZK VOD analysis must expose one logical part.",
    );
    assert_match(
        &chzzk_vod,
        r"current\.platform = PlatformId::Chzzk",
        "CHZZK VOD runtime status does not preserve platform identity.",
    );
    assert_match(
        &chzzk_vod,
        r"recognizes_only_chzzk_video_urls",
        "CHZZK VOD URL regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"validates_single_part_download_contract",
        "CHZZK queue request regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"stale_chzzk_job_dirs_are_scavenged_only",
        "CHZZK stale-job cleanup regression test is missing.",
    );
    assert_rust_test(
        &chzzk_vod,
        "job_root_uses_canonical_runtime_namespace",
        "Canonical CHZZK temp runtime namespace regression test is missing.",
    );
    assert_rust_test(
        &chzzk_vod,
        "legacy_stale_chzzk_job_dirs_are_scavenged_without_reuse",
        "Legacy CHZZK temp cleanup compatibility test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"cleanup_is_scoped_to_unique_job_directory",
        "CHZZK owned-temp cleanup regression test is missing.",
    );
    assert_match(
        &chzzk_vod,
        r"streamlink_staging_name_is_short_and_title_independent",
        "CHZZK Windows long-path regression test is missing.",
    );
}

#[test]
fn security_contracts() {
    let backend = read_repo_file("rust-runtime/src/backend.rs");
    let security = read_repo_file("rust-runtime/src/security.rs");
    let auth = read_repo_file("rust-runtime/src/platform/chzzk/auth.rs");
    let chzzk_vod = read_repo_file("rust-runtime/src/platform/chzzk/vod.rs");
    let recorder = read_repo_file("rust-runtime/src/recorder.rs");
    let runtime = read_repo_file("rust-runtime/src/platform_runtime.rs");
    let watcher = read_repo_file("rust-runtime/src/native_watcher.rs");

    assert_match(
        &backend,
        r#""CHZZK_NID_AUT""#,
        "NID_AUT must remain a hidden setting.",
    );
    assert_match(
        &backend,
        r#""CHZZK_NID_SES""#,
        "NID_SES must remain a hidden setting.",
    );
    assert_match(
        &auth,
        r"read_provider_settings",
        "CHZZK auth must use common encrypted secret boundary.",
    );
    assert_match(
        &security,
        r"read_provider_settings_with\(store, keys, secret_keys, unprotect_secret\)",
        "Provider snapshots must decrypt through native protection boundary.",
    );
    assert_match(
        &security,
        r"(?s)fn with_provider_settings<T>\(.*?let _guard = kick_secret_read_guard.*?store\.refresh_config_cache\(\).*?read\(store\.settings_for_keys\(keys\)\?\)",
        "Provider lookup must refresh SQLite and hold shared native-secret lock.",
    );
    assert_match(
        &security,
        r"CryptProtectData",
        "Windows secret protection must remain DPAPI-backed.",
    );
    assert_match(
        &security,
        r"DPAPI_PREFIX",
        "Windows DPAPI payload prefix must remain explicit.",
    );
    assert_match(
        &security,
        r"NATIVE_SECRET_PREFIX",
        "Unix native secret references must use explicit versioned prefix.",
    );
    assert_match(
        &security,
        r"native-secret:v1:",
        "Unix native secret reference format must remain versioned.",
    );
    assert_match(
        &security,
        r#"Command::new\("secret-tool"\)"#,
        "Linux secret storage must use Secret Service helper boundary.",
    );
    assert_match(
        &security,
        r"\.stdin\(Stdio::piped\(\)\)",
        "Linux Secret Service writes must pass secret data through stdin.",
    );
    assert_match(
        &security,
        r#"const APPLICATION_ATTRIBUTE: &str = "stream-archive""#,
        "Linux Secret Service entries must remain Stream Archive scoped.",
    );
    assert_match(
        &security,
        r#""application""#,
        "Linux Secret Service lookup/store must use application attribute.",
    );
    assert_match(
        &security,
        r"SecKeychainAddGenericPassword",
        "macOS secret storage must use Keychain Services.",
    );
    assert_match(
        &security,
        r"SecKeychainFindGenericPassword",
        "macOS secret lookup must use Keychain Services.",
    );
    assert_match(
        &security,
        r"io\.github\.p1k1g\.stream-archive",
        "macOS Keychain entries must remain app-scoped.",
    );
    assert_not_match(
        &security,
        r"\.arg\(value\)|\.args\([^\)]*value",
        "Linux secret values must never be appended to helper argv.",
    );
    assert_match(
        &security,
        r"New writes never intentionally fall back to plaintext",
        "Secret writes must fail closed.",
    );
    assert_match(
        &chzzk_vod,
        r"ChzzkAuth::load\(\)",
        "CHZZK VOD must reuse encrypted CHZZK auth.",
    );
    assert_match(
        &chzzk_vod,
        r"chzzk-cookies\.txt",
        "CHZZK VOD must use temporary cookie-file transport.",
    );
    assert_match(
        &chzzk_vod,
        r"restrict_private_dir\(&preparing\)",
        "CHZZK VOD must invoke platform private-directory boundary.",
    );
    assert_match(
        &runtime,
        r"icacls\.exe",
        "Windows private-directory implementation must use ACL tooling.",
    );
    assert_match(
        &runtime,
        r"/inheritance:r",
        "Windows private directories must remove inherited permissions.",
    );
    assert_match(
        &runtime,
        r"\(OI\)\(CI\)F",
        "Windows private directories must grant current owner required access.",
    );
    assert_match(
        &runtime,
        r"from_mode\(0o700\)",
        "Unix private directories must use mode 0700.",
    );
    assert_not_match(
        &format!("{chzzk_vod}\n{recorder}"),
        r"--http-cookie\s+NID_|NID_AUT=.*--|NID_SES=.*--",
        "CHZZK secrets must not appear directly in process arguments.",
    );
    assert_match(
        &chzzk_vod,
        r"NID_\(\?:AUT\|SES\)",
        "CHZZK VOD logs/errors must redact Naver cookie values.",
    );
    assert_match(
        &security,
        r"(?s)fn read_provider_settings_with\(.*?with_provider_settings\(store, keys,.*?unprotect\(reference, key\)",
        "Provider decryption must occur within locked snapshot callback.",
    );
    assert_match(
        &watcher,
        r"(?s)fn load_if_changed\(.*?with_provider_settings\(db, &keys,.*?Self::from_values",
        "SOOP watcher config decryption must use locked fresh snapshot.",
    );
    assert_not_match(
        &watcher,
        r"live_settings_with_secrets\(\)",
        "Watcher must not load native references from stale unlocked cache.",
    );
}

#[test]
fn tool_discovery_contracts() {
    let lib = read_repo_file("rust-runtime/src/lib.rs");
    let tools = read_repo_file("rust-runtime/src/tool_discovery.rs");
    let cli = read_repo_file("rust-runtime/src/bin/stream-archive-cli.rs");
    let unix_cli = read_repo_file("rust-runtime/src/unix_cli.rs");
    let diagnostics = read_repo_file("rust-runtime/src/diagnostics.rs");
    let headless = read_repo_file("rust-runtime/src/headless.rs");
    let server = read_repo_file("rust-runtime/src/main.rs");

    assert_match(
        &lib,
        r"pub mod tool_discovery;",
        "Shared library boundary must export tool discovery.",
    );
    assert_match(
        &tools,
        r"ToolKind::Streamlink",
        "Tool discovery must include Streamlink.",
    );
    assert_match(
        &tools,
        r"ToolKind::YtDlp",
        "Tool discovery must include yt-dlp.",
    );
    assert_match(
        &tools,
        r"ToolKind::Ffmpeg",
        "Tool discovery must include FFmpeg.",
    );
    assert_match(
        &tools,
        r"env::split_paths",
        "Tool discovery must search PATH portably.",
    );
    assert_match(
        &tools,
        r#""streamlink""#,
        "Unix Streamlink binary name must remain supported.",
    );
    assert_match(
        &tools,
        r#""yt-dlp""#,
        "Unix yt-dlp binary name must remain supported.",
    );
    assert_match(
        &tools,
        r#""ffmpeg""#,
        "Unix FFmpeg binary name must remain supported.",
    );
    assert_match(
        &tools,
        r"/opt/homebrew/bin",
        "macOS Homebrew tool discovery must remain supported.",
    );
    assert_match(
        &tools,
        r"\.local.*bin",
        "Unix user-local tool discovery must remain supported.",
    );
    assert_match(
        &tools,
        r"pub fn resolve_all_tools",
        "Shared tool discovery must own configured multi-tool resolution.",
    );
    assert_rust_test(
        &tools,
        "bundled_tools_are_rediscovered_after_they_appear",
        "Bundled yt-dlp/FFmpeg discovery must refresh after files appear.",
    );
    assert_match(
        &cli,
        r"resolve_all_tools",
        "Bootstrap CLI must reuse shared multi-tool resolution.",
    );
    assert_match(
        &unix_cli,
        r"resolve_all_tools",
        "Daily-use CLI must reuse shared multi-tool resolution.",
    );
    assert_match(
        &diagnostics,
        r"resolve_all_tools",
        "Diagnostics must reuse shared multi-tool resolution.",
    );
    assert_not_match(
        &format!("{cli}\n{unix_cli}"),
        r"fn\s+resolve_all\s*\(",
        "Presentation CLIs must not duplicate tool-resolution helpers.",
    );

    assert_match(
        &cli,
        r#""init" => command_init"#,
        "CLI init command is missing.",
    );
    assert_match(
        &cli,
        r#""doctor" => command_doctor"#,
        "CLI doctor command is missing.",
    );
    assert_match(
        &cli,
        r#""tools" => command_tools"#,
        "CLI tools command is missing.",
    );
    assert_match(
        &cli,
        r#""serve" => command_serve"#,
        "CLI serve command is missing.",
    );
    assert_match(
        &cli,
        r"cli-tool-discovery",
        "CLI-discovered tool settings must retain explicit SQLite source.",
    );
    assert_match(
        &cli,
        r"run_serve",
        "CLI serve --watch must delegate to shared headless lifecycle.",
    );
    assert_match(
        &headless,
        r"core\.start_watcher\(\)\.await",
        "Shared headless lifecycle must start canonical watcher when requested.",
    );
    assert_match(
        &server,
        r"STREAM_ARCHIVE_START_WATCHER",
        "Compatibility server must retain watcher startup flag.",
    );
    assert_match(
        &cli,
        r"STREAMLINK_PATH",
        "CLI must configure canonical Streamlink setting.",
    );
    assert_match(
        &cli,
        r"YT_DLP_PATH",
        "CLI must configure canonical yt-dlp setting.",
    );
    assert_match(
        &cli,
        r"FFMPEG_PATH",
        "CLI must configure canonical FFmpeg setting.",
    );
    assert_not_match(
        &cli,
        r"(?i)powershell|cmd\.exe|System\.Windows\.Forms",
        "Unix/headless CLI must not depend on Windows shell or GUI tooling.",
    );
}

fn unix_verify_has_unsafe_cli_pipe(text: &str) -> bool {
    let joined = Regex::new(r"\\\r?\n\s*").unwrap().replace_all(text, " ");
    let without_or = joined.replace("||", " __OR__ ");
    let patterns = [
        r"stream-archive-cli[^\r\n|]*\|",
        r#""?\$(?:\{cli\}|cli)"?[^\r\n|]*\|"#,
        r"(?m)^\s*(?:assert_cli_output_contains|assert_output_dir)\b[^\r\n|]*\|",
    ];
    patterns
        .iter()
        .any(|pat| Regex::new(pat).unwrap().is_match(&without_or))
}

#[test]
fn packaging_contracts() {
    let unix_build = read_repo_file("BUILD_UNIX_PACKAGE.sh");
    let unix_verify = read_repo_file("maintenance/Verify-UnixPackage.sh");
    let unix_metadata = read_repo_file("maintenance/Write-ReleaseMetadata.sh");
    let release_tooling = read_repo_file("rust-runtime/src/release_tooling.rs");
    let gitignore = read_repo_file(".gitignore");
    let windows_build = read_repo_file("BUILD_PORTABLE.bat");
    let gui_build = read_repo_file("rust-gui/build.rs");
    let gui_manifest = read_repo_file("rust-gui/Cargo.toml");
    let check_workflow = read_repo_file(".github/workflows/rust-runtime-check.yml");
    let release_workflow = read_repo_file(".github/workflows/rust-runtime-release.yml");

    // Ensure all PowerShell (.ps1) scripts have been retired from the repository.
    let ps1_output = Command::new("git")
        .args(["ls-files", "*.ps1"])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files *.ps1");
    assert!(ps1_output.status.success(), "git ls-files *.ps1 failed");
    let ps1_files = String::from_utf8_lossy(&ps1_output.stdout);
    assert!(
        ps1_files.trim().is_empty(),
        "PowerShell (.ps1) scripts must remain migrated to Rust; found:\n{ps1_files}"
    );
    assert_not_match(
        &windows_build,
        r"(?i)\bpowershell\b|\.ps1\b",
        "BUILD_PORTABLE.bat must use Rust release tooling instead of PowerShell.",
    );
    assert_not_match(
        &check_workflow,
        r"(?i)shell:\s*powershell|\.ps1\b",
        "PR CI workflow must use Rust tooling instead of PowerShell.",
    );
    assert_not_match(
        &release_workflow,
        r"(?i)shell:\s*powershell|\.ps1\b",
        "Release workflow must use Rust tooling instead of PowerShell.",
    );

    assert_match(
        &windows_build,
        r"StreamArchive\.exe",
        "Windows portable package must retain the Native GUI.",
    );
    assert_not_match(
        &windows_build,
        r"stream-archive-server\.exe|RUN_HEADLESS\.bat",
        "Windows portable package must not expose headless surface.",
    );
    assert_match(
        &windows_build,
        r"--backup",
        "BACKUP_DATA.bat must invoke StreamArchive.exe --backup.",
    );
    assert_match(
        &windows_build,
        r"--restore",
        "RESTORE_DATA.bat must invoke StreamArchive.exe --restore.",
    );
    assert_match(
        &windows_build,
        r"--write-release-metadata",
        "BUILD_PORTABLE.bat must write RELEASE_INFO.txt via Rust tooling.",
    );
    assert_match(
        &windows_build,
        r"--write-sha256sums",
        "BUILD_PORTABLE.bat must write SHA256SUMS.txt via Rust tooling.",
    );
    assert_match(
        &windows_build,
        r"--repository-root",
        "Windows portable packaging must anchor release provenance to repository checkout.",
    );
    assert_not_match(
        &windows_build,
        r"Set-WindowsExecutableIcon\.ps1|GeneratedIconPath",
        "Windows portable packaging must not mutate PE icon resources after linking.",
    );

    assert_match(
        &release_tooling,
        r#""stream-archive-server\.exe""#,
        "Windows verifier must reject removed headless binary.",
    );
    assert_match(
        &release_tooling,
        r#""RUN_HEADLESS\.bat""#,
        "Windows verifier must reject removed headless launcher.",
    );
    assert_match(
        &release_tooling,
        r#""stream-archive-icon\.png""#,
        "Windows verifier must reject standalone branding PNG.",
    );
    assert_match(
        &release_tooling,
        r#""stream-archive\.ico""#,
        "Windows verifier must reject standalone ICO.",
    );
    assert_match(
        &release_tooling,
        r"SHA256SUMS\.txt",
        "Windows package verifier must validate package-local checksums.",
    );
    assert_match(
        &release_tooling,
        r"require_clean_data",
        "Official Windows release validation must support empty-data contract.",
    );
    assert_match(
        &release_tooling,
        r"stream-archive-windows-",
        "Windows release archive naming contract is missing.",
    );
    assert_match(
        &release_tooling,
        r"\.sha256",
        "Windows archive checksum generation is missing.",
    );
    assert_match(
        &release_tooling,
        r"verify_windows_package\(&resolved_root, true",
        "Windows archiver must invoke clean-data verifier before opening ZIP.",
    );
    assert_match(
        &release_tooling,
        r"assert_embedded_windows_resources",
        "Windows package verifier must validate embedded PE icon and version resources.",
    );
    assert_match(
        &release_tooling,
        r"git_head_commit_from_metadata",
        "Release metadata must resolve HEAD from explicit root Git metadata.",
    );
    assert_match(
        &release_tooling,
        r"commondir",
        "Release metadata must resolve linked-worktree refs through commondir.",
    );
    assert_match(
        &release_tooling,
        r"GITHUB_WORKSPACE",
        "CI release provenance must bind explicit root to workflow checkout.",
    );
    assert_match(
        &release_tooling,
        r"GITHUB_SHA",
        "CI release provenance must validate GITHUB_SHA against HEAD.",
    );
    assert_match(
        &release_tooling,
        r"safe\.directory=",
        "Git status checks must scope safe.directory to explicit repository root.",
    );
    assert_match(
        &release_tooling,
        r"-dirty",
        "Dirty release provenance marker is missing.",
    );

    assert_match(
        &unix_build,
        r"cargo build --locked --release",
        "Unix package build must use locked release build.",
    );
    assert_match(
        &unix_build,
        r"bin/stream-archive-cli",
        "Unix package must include stream-archive-cli.",
    );
    assert_match(
        &unix_build,
        r"bin/stream-archive-server",
        "Unix package must include compatibility headless runtime.",
    );
    assert_match(
        &unix_build,
        r"docs/UNIX_CLI\.md",
        "Unix package must include Unix CLI documentation.",
    );
    assert_match(
        &unix_build,
        r"docs/OPERATIONS\.md",
        "Unix package must include operations documentation.",
    );
    assert_match(
        &unix_build,
        r"THIRD_PARTY_NOTICES\.md",
        "Unix package must include third-party notices.",
    );
    assert_match(
        &unix_build,
        r"LICENSE",
        "Unix package must include project license.",
    );
    assert_match(
        &unix_build,
        r"SHA256SUMS\.txt",
        "Unix package-local checksum manifest is missing.",
    );
    assert_match(
        &unix_build,
        r"stream-archive-\$\{PLATFORM\}-\$\{ARCH\}\.tar\.gz",
        "Unix release archive naming contract is missing.",
    );
    assert_match(
        &unix_build,
        r"ARCHIVE_CHECKSUM=",
        "Unix archive checksum generation is missing.",
    );
    assert_match(
        &unix_build,
        r"Verify-UnixPackage\.sh",
        "Unix package builder must call reusable verifier.",
    );

    assert_match(
        &unix_verify,
        r"shasum -a 256 -c SHA256SUMS\.txt",
        "Unix verifier must validate package-local checksums.",
    );
    assert_match(
        &unix_verify,
        r"Archive checksum mismatch",
        "Unix verifier must validate archive checksum.",
    );
    assert_match(
        &unix_verify,
        r"assert_cli_output_contains[^\r\n]*stream-archive-cli[^\r\n]*version",
        "Unix verifier must execute packaged CLI version check through captured output.",
    );
    assert_match(
        &unix_verify,
        r#"stream-archive-cli" help"#,
        "Unix verifier must execute packaged CLI help.",
    );
    assert!(
        !unix_verify_has_unsafe_cli_pipe(&unix_verify),
        "Unix verifier must not pipe packaged Rust CLI stdout directly to a downstream command."
    );
    assert!(unix_verify_has_unsafe_cli_pipe(
        r#""$cli" "$@" | grep -q "$expected""#
    ));
    assert!(unix_verify_has_unsafe_cli_pipe(
        r#"assert_cli_output_contains "$cli" x status | grep -q x"#
    ));
    assert!(!unix_verify_has_unsafe_cli_pipe(
        r#""$cli" status || fail "status failed""#
    ));
    assert_match(
        &unix_verify,
        r#"settings_json="\$\("\$cli" settings show --json\)""#,
        "assert_output_dir must capture CLI JSON before parsing.",
    );
    assert_match(
        &unix_verify,
        r"printf '%s\\n' \x22\$settings_json\x22 \| python3 -c",
        "assert_output_dir must parse captured JSON rather than piping Rust CLI.",
    );
    assert_match(
        &unix_verify,
        r#"stream-archive-cli" init"#,
        "Unix archive smoke must initialize from extracted package.",
    );
    assert_match(
        &unix_verify,
        r"status --json",
        "Unix archive smoke must execute packaged status JSON.",
    );
    assert_match(
        &unix_verify,
        r#"mktemp -d "[^"]*\s[^"]*\.XXXXXX""#,
        "Unix archive smoke must exercise whitespace path.",
    );
    assert_match(
        &unix_verify,
        r#"mktemp -d "[^"]*[^\x00-\x7F][^"]*\.XXXXXX""#,
        "Unix archive smoke must exercise non-ASCII path.",
    );
    assert_match(
        &unix_verify,
        r"Release package data directory must be empty",
        "Unix verifier must reject bundled runtime data.",
    );
    for tool in ["streamlink", "yt-dlp", "ffmpeg"] {
        assert_match(
            &unix_verify,
            tool,
            &format!("Unix package verifier must reject bundled media tool: {tool}"),
        );
    }

    assert_match(
        &unix_metadata,
        r"cargo metadata --locked --no-deps",
        "Unix release metadata must source version from Cargo metadata.",
    );
    assert_match(
        &unix_metadata,
        r"commit=\$COMMIT",
        "Unix release metadata must include commit provenance.",
    );
    assert_match(
        &unix_metadata,
        r#"COMMIT="unknown""#,
        "Unix release metadata must support source archives without Git metadata.",
    );
    assert_match(
        &unix_metadata,
        r#"if DIRTY_STATE="\$\(git status --porcelain --untracked-files=normal 2>/dev/null\)"; then"#,
        "Unix release metadata must require git status to succeed before attributing a commit.",
    );
    assert_match(
        &unix_metadata,
        r#"COMMIT="\$CANDIDATE""#,
        "Unix release metadata must assign commit provenance only after dirty-state check succeeds.",
    );
    assert_match(
        &unix_metadata,
        r"-dirty",
        "Unix dirty release provenance marker is missing.",
    );

    assert_match(
        &gui_build,
        r"generate_windows_icon",
        "Windows icon must be generated from canonical validated PNG during Rust build.",
    );
    assert_match(
        &gui_build,
        r"16, 24, 32, 48, 64, 128, 256",
        "Windows icon generation must retain all canonical icon sizes.",
    );
    assert_match(
        &gui_build,
        r"CARGO_MANIFEST_DIR",
        "Windows icon generation must resolve source from manifest directory.",
    );
    assert_match(
        &gui_build,
        r"stream-archive-icon\.png",
        "Windows resource build must use canonical branding PNG source.",
    );
    assert_match(
        &gui_build,
        r"OUT_DIR",
        "Generated ICO must stay in Cargo OUT_DIR.",
    );
    assert_match(
        &gui_build,
        r"winresource::WindowsResource",
        "Windows icon must be linked through Windows resource compiler.",
    );
    assert_match(
        &gui_build,
        r"set_icon\(icon_path\)",
        "Windows resource build must attach generated ICO.",
    );
    assert_match(
        &gui_manifest,
        r"image\s*=.*0\.25\.10",
        "Windows icon generation must use locked image build dependency.",
    );
    assert_match(
        &gui_manifest,
        r#"winresource\s*=\s*"=0\.1\.31""#,
        "Windows resource compiler build dependency must remain locked.",
    );
    assert_match(
        &gitignore,
        r"(?m)^dist/\r?$",
        "Generated release staging must be ignored.",
    );

    assert_match(
        &check_workflow,
        r"(?s)pull_request:\s+paths:.*?\.gitignore",
        "PR packaging checks must trigger on .gitignore.",
    );
    assert_match(
        &check_workflow,
        r"(?s)push:.*?paths:.*?\.gitignore",
        "Main-branch packaging checks must trigger on .gitignore.",
    );
    assert_match(
        &check_workflow,
        r"BUILD_UNIX_PACKAGE\.sh",
        "PR CI must smoke canonical Unix package builder.",
    );
    assert_match(
        &check_workflow,
        r"--verify-windows-package",
        "PR CI must use Rust Windows package verifier.",
    );
    assert_match(
        &check_workflow,
        r"--new-windows-release-archive",
        "PR CI must use Rust Windows release archiver.",
    );
    assert_match(
        &check_workflow,
        r"Windows release archiver accepted non-empty runtime data",
        "PR CI must regress clean-data archive boundary.",
    );
    assert_match(
        &check_workflow,
        r"RELEASE_INFO_DIRTY\.txt",
        "Unix PR CI must regress dirty release provenance.",
    );
    assert_match(
        &check_workflow,
        r"RELEASE_INFO_STATUS_FAILURE\.txt",
        "Unix PR CI must regress git-status provenance failures.",
    );
    assert_match(
        &check_workflow,
        r"exit 42",
        "Unix PR CI must force git status failure while leaving rev-parse available.",
    );
    assert_match(
        &check_workflow,
        r"grep -qx 'commit=unknown' \x22\$status_failure\x22",
        "Unix PR CI must retain unknown provenance on git status failure.",
    );
    assert_match(
        &check_workflow,
        r"explicit non-Git RepositoryRoot must use commit=unknown",
        "Windows PR CI must reject ambient GITHUB_SHA provenance for non-Git root.",
    );
    assert_match(
        &check_workflow,
        r"checkout metadata must match GITHUB_SHA",
        "Windows PR CI must verify checkout provenance against GITHUB_SHA.",
    );
    assert_match(
        &check_workflow,
        r"trusted GitHub checkout dirty metadata must include the -dirty provenance marker",
        "Windows PR CI must regress dirty provenance for GitHub workspace.",
    );
    assert_match(
        &check_workflow,
        r"git worktree add -b",
        "Windows PR CI must exercise a branch-attached linked Git worktree.",
    );
    assert_match(
        &check_workflow,
        r"linked worktree metadata must resolve commondir refs",
        "Windows PR CI must regress linked-worktree commit provenance.",
    );
    assert_match(
        &check_workflow,
        r"linked worktree dirty metadata must include the -dirty provenance marker",
        "Windows PR CI must regress dirty linked-worktree provenance.",
    );

    assert_match(
        &release_workflow,
        r"workflow_dispatch",
        "Release artifact workflow must remain manual.",
    );
    assert_match(
        &release_workflow,
        r"release-contracts:",
        "Manual release artifacts must be gated by release-contracts.",
    );
    assert_match(
        &release_workflow,
        r"--test runtime_contracts",
        "Manual release workflow must enforce runtime contracts via Cargo.",
    );
    assert_match(
        &release_workflow,
        r"needs: release-contracts",
        "Artifact jobs must wait for release contract validation.",
    );
    assert_match(
        &release_workflow,
        r"windows-latest",
        "Release workflow must build Windows natively.",
    );
    assert_match(
        &release_workflow,
        r"ubuntu-latest",
        "Release workflow must build Linux natively.",
    );
    assert_match(
        &release_workflow,
        r"macos-latest",
        "Release workflow must build macOS natively.",
    );
    assert_match(
        &release_workflow,
        r"BUILD_UNIX_PACKAGE\.sh",
        "Release workflow must reuse Unix package builder.",
    );
    assert_match(
        &release_workflow,
        r"--verify-windows-package",
        "Release workflow must reuse Rust Windows verifier.",
    );
    assert_match(
        &release_workflow,
        r"--new-windows-release-archive",
        "Release workflow must reuse Rust Windows archiver.",
    );
    assert_match(
        &release_workflow,
        r"actions/upload-artifact@v4",
        "Release workflow must upload verified workflow artifacts.",
    );
    assert_not_match(
        &release_workflow,
        r"softprops/action-gh-release|gh\s+release|git\s+tag|create-release|upload-release-asset",
        "Release workflow must not publish a GitHub Release or create tags.",
    );

    // Ensure workflows read version dynamically rather than hardcoding a literal release version.
    assert_not_match(
        &check_workflow,
        r"version['\x22]?\s*==\s*['\x22]\d+\.\d+\.\d+['\x22]|version=\d+\.\d+\.\d+|ProductVersion=\d+\.\d+\.\d+",
        "PR check workflow must resolve release version dynamically from Cargo.toml.",
    );
    assert_not_match(
        &release_workflow,
        r"version['\x22]?\s*==\s*['\x22]\d+\.\d+\.\d+['\x22]|version=\d+\.\d+\.\d+|ProductVersion=\d+\.\d+\.\d+",
        "Release workflow must resolve release version dynamically from Cargo.toml.",
    );

    let runtime_version = cargo_package_version("rust-runtime/Cargo.toml", "stream-archive-server");
    let gui_version = cargo_package_version("rust-gui/Cargo.toml", "stream-archive-gui");
    assert!(!runtime_version.is_empty() && !gui_version.is_empty());
    assert_eq!(
        runtime_version, gui_version,
        "Release version mismatch between runtime and gui"
    );
}

#[test]
fn release_candidate_contracts() {
    let phase = read_repo_file("docs/PHASE23_7_RELEASE_CANDIDATE_FINAL_QA.md");
    let unix_verify = read_repo_file("maintenance/Verify-UnixPackage.sh");
    let release_tooling = read_repo_file("rust-runtime/src/release_tooling.rs");
    let check_workflow = read_repo_file(".github/workflows/rust-runtime-check.yml");
    let release_workflow = read_repo_file(".github/workflows/rust-runtime-release.yml");
    let readme = read_repo_file("README.md");

    let runtime_version = cargo_package_version("rust-runtime/Cargo.toml", "stream-archive-server");
    let gui_version = cargo_package_version("rust-gui/Cargo.toml", "stream-archive-gui");
    assert_eq!(runtime_version, gui_version, "RC version mismatch");
    assert_match(
        &runtime_version,
        r"^\d+\.\d+\.\d+$",
        "Release version must use X.Y.Z semver.",
    );

    let notes_relative = format!(
        "docs/RELEASE_NOTES_{}.md",
        runtime_version.replace('.', "_")
    );
    let release_notes = read_repo_file(&notes_relative);

    for artifact in [
        r"stream-archive-windows-x64\.zip",
        r"stream-archive-linux-x64\.tar\.gz",
        r"stream-archive-macos-arm64\.tar\.gz",
    ] {
        assert_match(
            &phase,
            artifact,
            &format!("RC checklist missing artifact matrix entry: {artifact}"),
        );
    }

    assert_match(
        &phase,
        r"Automated RC validation",
        "RC checklist must separate automated validation.",
    );
    assert_match(
        &phase,
        r"Manual RC validation",
        "RC checklist must separate manual validation.",
    );
    assert_match(
        &phase,
        r"MANUAL TEST REQUIRED",
        "RC checklist must keep real-session checks explicit.",
    );
    assert_match(
        &phase,
        r"Release blockers",
        "RC checklist must identify release blockers.",
    );
    assert_match(
        &phase,
        r"Public release readiness",
        "RC checklist must state public-release readiness.",
    );
    assert_match(
        &phase,
        r"(?i)cross-version upgrade",
        "RC checklist must keep cross-version upgrade scope explicit.",
    );
    assert_match(
        &phase,
        r"(?i)arbitrary schema downgrade",
        "RC checklist must not imply arbitrary schema downgrade support.",
    );

    assert_match(
        &release_notes,
        &regex::escape(&runtime_version),
        "Release notes must identify current Cargo version.",
    );
    assert_match(
        &release_notes,
        r"(?i)Windows Native",
        "Release notes must describe Windows Native surface.",
    );
    assert_match(
        &release_notes,
        r"Linux",
        "Release notes must describe Linux support.",
    );
    assert_match(
        &release_notes,
        r"macOS",
        "Release notes must describe macOS support.",
    );
    assert_match(
        &release_notes,
        r"Streamlink",
        "Release notes must document external media-tool dependencies.",
    );
    assert_match(
        &release_notes,
        r"(?i)upgrade",
        "Release notes must include upgrade guidance.",
    );
    assert_match(
        &readme,
        &regex::escape(&format!("v{runtime_version}")),
        "README must reference current release version.",
    );
    assert_match(
        &readme,
        &regex::escape(&notes_relative),
        "README must link to current release notes.",
    );

    assert_match(
        &unix_verify,
        r"backup create --json",
        "Unix RC archive smoke must create a managed backup.",
    );
    assert_match(
        &unix_verify,
        r"backup restore",
        "Unix RC archive smoke must restore a managed backup.",
    );
    assert_match(
        &unix_verify,
        r"replacement package",
        "Unix RC archive smoke must re-extract a replacement package.",
    );
    assert_match(
        &unix_verify,
        r"rc-fixture",
        "Unix RC archive smoke must verify persisted channel data.",
    );
    assert_match(
        &unix_verify,
        r"assert_output_dir",
        "Unix RC archive smoke must verify exact OUTPUT_DIR.",
    );
    assert_match(
        &unix_verify,
        r"OUTPUT_DIR mismatch",
        "Unix RC setting persistence assertion must fail explicitly.",
    );
    assert_match(
        &unix_verify,
        r#"runtime_data="\$scratch/runtime data"#,
        "Unix RC archive smoke must retain whitespace runtime-data path.",
    );
    assert_match(
        &release_tooling,
        r"Archive checksum mismatch",
        "Windows verifier must reject archive checksum mismatches.",
    );

    assert_match(
        &check_workflow,
        r"Reject corrupted Unix release archive",
        "PR CI must reject corrupted Unix RC archives.",
    );
    assert_match(
        &check_workflow,
        r"Corrupted Unix release archive was accepted",
        "Unix corruption regression must fail closed.",
    );
    assert_match(
        &check_workflow,
        r"Windows offline backup restore smoke",
        "PR CI must exercise packaged Windows offline backup/restore.",
    );
    assert_match(
        &check_workflow,
        r"Windows restore accepted a backup whose SHA256 no longer matched metadata",
        "Windows restore corruption regression is missing.",
    );
    assert_match(
        &check_workflow,
        r"Reject corrupted Windows release archive",
        "PR CI must reject corrupted Windows RC archives.",
    );
    assert_match(
        &check_workflow,
        r"Corrupted Windows release archive was accepted",
        "Windows corruption regression must fail closed.",
    );

    assert_match(
        &release_workflow,
        r"workflow_dispatch",
        "RC artifact workflow must remain manually dispatched.",
    );
    assert_match(
        &release_workflow,
        r"permissions:\s*\r?\n\s*contents:\s*read",
        "RC artifact workflow must retain read-only contents permission.",
    );
    assert_match(
        &release_workflow,
        r"windows-latest",
        "RC artifact workflow must build Windows natively.",
    );
    assert_match(
        &release_workflow,
        r"ubuntu-latest",
        "RC artifact workflow must build Linux natively.",
    );
    assert_match(
        &release_workflow,
        r"macos-latest",
        "RC artifact workflow must build macOS natively.",
    );
    assert_match(
        &release_workflow,
        r"BUILD_UNIX_PACKAGE\.sh",
        "RC artifact workflow must reuse canonical Unix package builder.",
    );
    assert_match(
        &release_workflow,
        r"--new-windows-release-archive",
        "RC artifact workflow must reuse canonical Rust Windows archiver.",
    );
    assert_not_match(
        &release_workflow,
        r"softprops/action-gh-release|gh\s+release|git\s+tag|create-release|upload-release-asset|cargo\s+publish",
        "Release workflow must not publish releases or tags.",
    );
}

struct SecretRule {
    name: &'static str,
    regex: Regex,
}

fn secret_rules() -> Vec<SecretRule> {
    vec![
        SecretRule {
            name: "GitHub classic token",
            regex: Regex::new(r"ghp_[A-Za-z0-9]{30,}").unwrap(),
        },
        SecretRule {
            name: "GitHub fine-grained token",
            regex: Regex::new(r"github_pat_[A-Za-z0-9_]{40,}").unwrap(),
        },
        SecretRule {
            name: "OpenAI-style API key",
            regex: Regex::new(r"sk-[A-Za-z0-9_-]{20,}").unwrap(),
        },
        SecretRule {
            name: "AWS access key",
            regex: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
        },
        SecretRule {
            name: "Private key material",
            regex: Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----").unwrap(),
        },
        SecretRule {
            name: "Local developer user path",
            regex: Regex::new(r"(?i)C:\\Users\\pokga(?:\\|/)").unwrap(),
        },
        SecretRule {
            name: "Discord webhook",
            regex: Regex::new(
                r"https://(?:canary\.|ptb\.)?discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9._-]+",
            )
            .unwrap(),
        },
        SecretRule {
            name: "Teams legacy webhook",
            regex: Regex::new(r"https://[^\s]+\.webhook\.office\.com/webhookb2/[A-Za-z0-9@/_-]+")
                .unwrap(),
        },
    ]
}

fn secret_assignment_regex() -> Regex {
    Regex::new(r"(?i)^[ \t]*(SOOP_PASSWORD|CLOUDFLARE_API_KEY|(?:CHZZK_)?NID_AUT|(?:CHZZK_)?NID_SES|STREAM_ARCHIVE_TOKEN)[ \t]*=[ \t]*(\S[^\r\n]*)$").unwrap()
}

fn is_placeholder_secret(value: &str) -> bool {
    let trimmed = value.trim();
    (trimmed.starts_with('<') && trimmed.ends_with('>'))
        || trimmed.eq_ignore_ascii_case("CHANGE_ME")
        || (trimmed.starts_with("YOUR_")
            && trimmed
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_'))
}

fn scan_line_for_secrets(
    line: &str,
    rules: &[SecretRule],
    assignment_re: &Regex,
    context: &str,
    violations: &mut BTreeSet<String>,
) {
    for rule in rules {
        if rule.regex.is_match(line) {
            violations.insert(format!("{context} - {}", rule.name));
        }
    }
    if let Some(caps) = assignment_re.captures(line)
        && !is_placeholder_secret(&caps[2])
    {
        violations.insert(format!("{context} - Non-empty {} assignment", &caps[1]));
    }
}

#[test]
fn release_safety_contracts() {
    let root = repo_root();
    let rules = secret_rules();
    let assignment_re = secret_assignment_regex();
    let mut violations = BTreeSet::new();

    let ls_output = Command::new("git")
        .args(["ls-files"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    assert!(ls_output.status.success(), "git ls-files failed");
    let text_exts: BTreeSet<&str> = [
        "rs", "js", "css", "html", "md", "ps1", "bat", "cmd", "yml", "yaml", "toml", "ini", "txt",
        "json", "example", "sh",
    ]
    .into_iter()
    .collect();

    for relative in String::from_utf8_lossy(&ls_output.stdout).lines() {
        if relative.is_empty()
            || relative == "maintenance/guards/ReleaseSafety.ps1"
            || relative == "rust-runtime/tests/runtime_contracts.rs"
        {
            continue;
        }
        let path = root.join(relative);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !text_exts.contains(ext.as_str()) && !matches!(fname, ".gitignore" | ".gitattributes") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let ctx = format!("Current tree ({relative})");
        for line in text.lines() {
            scan_line_for_secrets(line, &rules, &assignment_re, &ctx, &mut violations);
        }
    }

    // Stream Git patch history line-by-line to keep memory usage bounded while covering all commits.
    let mut child = Command::new("git")
        .args([
            "log",
            "--all",
            "--no-color",
            "--format=",
            "--patch",
            "--unified=0",
            "--",
            ".",
            ":(exclude)maintenance/guards/ReleaseSafety.ps1",
            ":(exclude)rust-runtime/tests/runtime_contracts.rs",
        ])
        .current_dir(&root)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn git log history scan");
    let stdout = child.stdout.take().expect("git log stdout");
    let mut reader = BufReader::new(stdout);
    let mut line_buf = Vec::new();
    loop {
        line_buf.clear();
        let read = reader
            .read_until(b'\n', &mut line_buf)
            .expect("read git log");
        if read == 0 {
            break;
        }
        let raw = String::from_utf8_lossy(&line_buf);
        let stripped = raw.trim_end_matches(['\r', '\n']);
        let content = if (stripped.starts_with('+') && !stripped.starts_with("+++"))
            || (stripped.starts_with('-') && !stripped.starts_with("---"))
        {
            &stripped[1..]
        } else {
            stripped
        };
        scan_line_for_secrets(
            content,
            &rules,
            &assignment_re,
            "Git history",
            &mut violations,
        );
    }
    let status = child.wait().expect("wait git log");
    assert!(status.success(), "git history scan failed");
    assert!(
        violations.is_empty(),
        "Public release safety scan found {} issue(s):\n{}",
        violations.len(),
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );

    let workflow = read_repo_file(".github/workflows/rust-runtime-check.yml");
    let release_workflow = read_repo_file(".github/workflows/rust-runtime-release.yml");
    let package = read_repo_file("BUILD_PORTABLE.bat");
    let gitignore = read_repo_file(".gitignore");
    let run_dev = read_repo_file("RUN_DEV.bat");
    let release_tooling = read_repo_file("rust-runtime/src/release_tooling.rs");
    let unix_package = read_repo_file("BUILD_UNIX_PACKAGE.sh");
    let unix_package_verifier = read_repo_file("maintenance/Verify-UnixPackage.sh");
    let readme = read_repo_file("README.md");
    let agents = read_repo_file("AGENTS.md");
    let contributing = read_repo_file("CONTRIBUTING.md");
    let unix_cli = read_repo_file("docs/UNIX_CLI.md");
    let manifest = read_repo_file("rust-runtime/Cargo.toml");
    let gui_manifest = read_repo_file("rust-gui/Cargo.toml");

    for (name, text) in [
        ("PR workflow", &workflow),
        ("portable build", &package),
        ("developer runner", &run_dev),
        ("release tooling", &release_tooling),
        ("Unix package builder", &unix_package),
        ("Unix package verifier", &unix_package_verifier),
        ("README", &readme),
        ("AGENTS", &agents),
        ("CONTRIBUTING", &contributing),
        ("Unix CLI guide", &unix_cli),
        ("GUI manifest", &gui_manifest),
    ] {
        assert_not_match(
            text,
            r"rust-web[\\/]",
            &format!("Active runtime path must use rust-runtime, not rust-web: {name}"),
        );
    }

    assert!(
        !root.join(".github/workflows/rust-web-check.yml").is_file(),
        "Retired rust-web check workflow must not return."
    );
    assert!(
        !root
            .join(".github/workflows/rust-web-release.yml")
            .is_file(),
        "Retired rust-web release workflow must not return."
    );
    assert_match(
        &release_workflow,
        r"workflow_dispatch",
        "Canonical runtime release workflow must retain manual workflow_dispatch.",
    );
    assert_not_match(
        &release_workflow,
        r"softprops/action-gh-release|gh\s+release|git\s+tag|create-release|upload-release-asset",
        "Release workflow must not publish releases or tags.",
    );
    assert_match(
        &gitignore,
        r"(?m)^rust-runtime/target/\r?$",
        "Git ignore must cover rust-runtime/target/.",
    );
    assert_not_match(
        &gitignore,
        r"(?m)^rust-web/target/\r?$",
        "Git ignore must not retain rust-web/target/.",
    );
    assert_match(
        &gitignore,
        r"(?m)^backend/\.stream-archive/\r?$",
        "Git ignore must cover backend/.stream-archive/.",
    );
    assert_match(
        &gitignore,
        r"(?m)^backend/\.rust-web/\r?$",
        "Legacy CHZZK temp state must remain ignored while cleanup compatibility is retained.",
    );
    assert_match(
        &workflow,
        r"--test runtime_contracts",
        "CI must call the single Rust runtime-contract entry point.",
    );
    assert_not_match(
        &workflow,
        r"Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety",
        "CI must not call superseded guard entry points.",
    );
    assert_not_match(
        &workflow,
        r"runs-on:\s*\[?self-hosted",
        "Public CI must not depend on a private self-hosted runner.",
    );
    assert_match(
        &workflow,
        r"fetch-depth:\s*0",
        "Public-release CI must fetch full history for the history safety scan.",
    );
    for trigger in [
        r"rust-runtime/\*\*",
        r"rust-gui/\*\*",
        r"RUN_DEV\.bat",
        r"BUILD_PORTABLE\.bat",
        r"BUILD_UNIX_PACKAGE\.sh",
        r"maintenance/\*\*",
        r"docs/\*\*",
    ] {
        assert_match(
            &workflow,
            trigger,
            &format!("Runtime workflow path coverage is missing: {trigger}"),
        );
    }
    assert_not_match(
        &workflow,
        r"BUILD_RELEASE\.bat",
        "Runtime workflow must not reference retired BUILD_RELEASE.bat.",
    );
    assert!(
        !root.join("BUILD_RELEASE.bat").is_file(),
        "Retired BUILD_RELEASE.bat wrapper must not exist."
    );
    assert_match(
        &workflow,
        r"BUILD_PORTABLE\.bat",
        "Portable package smoke step is missing.",
    );
    assert_match(
        &workflow,
        r"--verify-windows-package",
        "Reusable Windows portable package verification step is missing.",
    );
    assert_not_match(
        &package,
        r#"cargo build --locked --release --manifest-path "\.\\rust-runtime\\Cargo\.toml""#,
        "Windows portable packaging must not build the standalone headless server binary.",
    );
    assert_match(
        &package,
        r#"cargo build --locked --release --manifest-path "\.\\rust-gui\\Cargo\.toml""#,
        "Portable package must perform the locked native GUI release build directly.",
    );
    assert_match(
        &package,
        r"StreamArchive\.exe",
        "Portable package must include native Stream Archive GUI.",
    );
    assert_not_match(
        &package,
        r"stream-archive-server\.exe|RUN_HEADLESS\.bat",
        "Windows portable package must be Native-only.",
    );
    assert_match(
        &package,
        r#"(?s)>"%OUT%\\RUN\.bat".*?StreamArchive\.exe"#,
        "RUN.bat generation must make native GUI default.",
    );
    assert_not_match(
        &package,
        r"stream-archive-launcher\.exe|RUN_WEB\.bat|RUN_SERVER_CONSOLE\.bat|Caddyfile\.example|REVERSE_PROXY\.md|LOCAL_LAUNCHER\.md",
        "Retired Web launcher/proxy package artifacts must not return.",
    );
    assert_match(
        &package,
        r"dist\\stream-archive",
        "Portable package output must use Stream Archive namespace.",
    );
    assert_match(
        &package,
        r"docs\\OPERATIONS\.md",
        "Portable package must include current operations guidance.",
    );
    assert_match(
        &package,
        r"THIRD_PARTY_NOTICES\.md",
        "Portable package must include third-party notices.",
    );
    assert_match(
        &package,
        r"LICENSE",
        "Portable package must include project license.",
    );
    assert_match(
        &manifest,
        r#"name\s*=\s*"stream-archive-server""#,
        "Cargo package must keep compatible shared/headless runtime name.",
    );
    assert_not_match(
        &manifest,
        r"(?m)^\s*(?:axum|tokio-stream|tower-http|tracing|tracing-subscriber)\s*=",
        "Retired Web-only direct dependencies must not return.",
    );
    assert_match(
        &manifest,
        r#"license\s*=\s*"AGPL-3\.0-or-later""#,
        "Cargo package must declare AGPL-3.0-or-later.",
    );
    assert_match(
        &gui_manifest,
        r#"name\s*=\s*"stream-archive-gui""#,
        "Native GUI Cargo package must use Stream Archive namespace.",
    );
    assert_match(
        &gui_manifest,
        r#"license\s*=\s*"AGPL-3\.0-or-later""#,
        "Native GUI Cargo package must declare AGPL-3.0-or-later.",
    );
    assert_not_match(
        &package,
        r"soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web",
        "Portable packaging must not reintroduce generic legacy app names.",
    );
}
