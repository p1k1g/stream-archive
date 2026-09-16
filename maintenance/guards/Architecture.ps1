. (Join-Path $PSScriptRoot 'Common.ps1')

$app = Read-RepoFile 'rust-web\web\app.js'
$phase13 = Read-RepoFile 'rust-web\web\phase13.js'
$phase14 = Read-RepoFile 'rust-web\web\phase14.js'
$platform = Read-RepoFile 'rust-web\src\platform\mod.rs'
$platformLive = Read-RepoFile 'rust-web\src\platform\live.rs'
$platformVod = Read-RepoFile 'rust-web\src\platform\vod.rs'
$platformSoop = Read-RepoFile 'rust-web\src\platform\soop\mod.rs'
$platformSoopLive = Read-RepoFile 'rust-web\src\platform\soop\live.rs'
$platformSoopVod = Read-RepoFile 'rust-web\src\platform\soop\vod.rs'
$support = Read-RepoFile 'rust-web\src\support.rs'
$watcher = Read-RepoFile 'rust-web\src\native_watcher.rs'
$vodFacade = Read-RepoFile 'rust-web\src\vod.rs'
$queue = Read-RepoFile 'rust-web\src\vod_queue.rs'
$store = Read-RepoFile 'rust-web\src\store.rs'
$main = Read-RepoFile 'rust-web\src\main.rs'
$lib = Read-RepoFile 'rust-web\src\lib.rs'
$core = Read-RepoFile 'rust-web\src\app_core.rs'
$guiManifest = Read-RepoFile 'rust-gui\Cargo.toml'
$guiMain = Read-RepoFile 'rust-gui\src\main.rs'
$guiUi = Read-RepoFile 'rust-gui\ui\app-window.slint'
$guiSources = (Get-ChildItem (Join-Path $script:RuntimeContractsRoot 'rust-gui/src') -Filter '*.rs' -Recurse | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"

# Frontend state guarantees remain prerequisites for platform expansion.
Assert-Match $app 'window\.StreamArchiveState\s*=\s*StreamArchiveState' 'app.js must expose the shared StreamArchiveState bus.'
Assert-Match $app "StreamArchiveState\.publish\('status'" 'status must publish through StreamArchiveState.'
Assert-Match $app "StreamArchiveState\.publish\('vod'" 'VOD status must publish through StreamArchiveState.'
Assert-Match $app "StreamArchiveState\.publish\('snapshot'" 'SSE snapshots must publish through StreamArchiveState.'
Assert-Match $phase13 "StreamArchiveState\?\.publish\('queue'" 'Queue UI queue must publish queue state through the shared bus.'
Assert-Match $phase13 "StreamArchiveState\?\.subscribe\('snapshot'" 'Queue UI queue must consume SSE snapshots through the shared bus.'
Assert-NotMatch $phase13 'renderStatus\s*=\s*function' 'Queue UI must not wrap renderStatus.'
Assert-NotMatch $phase13 'renderVodStatus\s*=\s*function' 'Queue UI must not wrap renderVodStatus.'
Assert-NotMatch $phase13 'applyRealtimeSnapshot\s*=\s*function' 'Queue UI must not wrap applyRealtimeSnapshot.'
Assert-NotMatch $phase13 'p13Notify\s*\(' 'Legacy Queue UI browser notification code must stay removed.'
Assert-Match $phase14 "bus\.subscribe\('status'" 'Notification UI notifications must subscribe to status state.'
Assert-Match $phase14 "bus\.subscribe\('queue'" 'Notification UI notifications must subscribe to queue state.'
Assert-NotMatch $phase14 'window\.renderStatus\s*=' 'Notification UI must not replace renderStatus.'
Assert-NotMatch $phase14 'window\.api\s*=' 'Notification UI must not replace api.'
Assert-NotMatch $phase14 'window\.applyRealtimeSnapshot\s*=' 'Notification UI must not replace applyRealtimeSnapshot.'
Assert-NotMatch $phase14 '__p14Wrapped' 'Legacy Notification UI wrapper markers must stay removed.'

# SQLite is the only runtime configuration/history authority.
Assert-Match $main 'Store::migrate_legacy_database\(&db_path\)' 'Startup must perform only the bounded legacy database filename migration.'
Assert-Match $main 'Store::open\(db_path\)' 'Server startup must open the canonical SQLite store.'
Assert-Match $store 'const DATABASE_FILE:\s*&str\s*=\s*"stream-archive\.db"' 'Canonical database filename must use the Stream Archive namespace.'
Assert-Match $store 'STREAM_ARCHIVE_DATA_DIR' 'Data directory environment override must use the Stream Archive namespace.'
Assert-NotMatch $main 'bootstrap_primary_once|materialize_primary_files|SOOP_LIVE_SETTING\.ini|SOOP_LIVE_CHANNELS\.txt|SOOP_VOD_SETTING\.ini' 'Runtime must not import or emit legacy INI/TXT mirrors.'
Assert-NotMatch $store 'legacy-import-live|legacy-import-vod|read_safe_settings|read_channels|read_hidden_settings' 'SQLite store must not depend on legacy config-file import helpers.'
Assert-Match $store 'settings_cache: Arc<RwLock<BTreeMap<String, String>>>' 'Committed runtime settings cache is missing.'
Assert-RustTest $store 'runtime_config_cache_tracks_committed_writes' 'Committed settings-cache behavior test is missing.'
Assert-Match $store 'platform TEXT NOT NULL' 'Persistent history and queue schemas must retain platform identity.'
Assert-Match $queue 'lifecycle_lock: Arc<Mutex<\(\)>>' 'Queue must share the global serialized VOD lifecycle lock.'

# Phase 21 shared service boundary. Presentation/authentication concerns must stay
# outside this facade so Slint and CLI callers can use Rust services directly.
Assert-Match $lib 'pub\s+mod\s+app_core' 'Shared library must expose the Phase 21 application core.'
Assert-Match $lib 'pub\s+mod\s+store' 'Shared library must expose canonical SQLite persistence.'
Assert-Match $lib 'pub\s+mod\s+native_watcher' 'Shared library must expose the native watcher boundary.'
Assert-Match $lib 'pub\s+mod\s+vod' 'Shared library must expose the VOD facade.'
Assert-Match $core 'pub\s+struct\s+StreamArchiveCore' 'Phase 21 shared application facade is missing.'
Assert-Match $core 'Store::default_path\(&backend_dir\)' 'Shared core must open the canonical SQLite path.'
Assert-Match $core 'store::init_global\(store\.clone\(\)\)' 'Shared core must initialize the canonical global store for existing runtime modules.'
Assert-Match $core 'validate_setting_updates\(updates\)' 'Shared core settings writes must preserve runtime validation.'
Assert-Match $core 'validate_secret_updates\(updates\)' 'Shared core secret writes must preserve security validation.'
Assert-Match $core 'protect_secret\(value\)' 'Shared core secret writes must use the native protection boundary.'
Assert-Match $core 'validate_channels\(channels\)' 'Shared core channel writes must preserve provider validation.'
Assert-Match $core 'apply_vod_tool_defaults' 'Shared core VOD operations must preserve canonical media-tool defaults.'
Assert-Match $core 'lifecycle_lock: Arc<Mutex<\(\)>>' 'Shared core must serialize VOD lifecycle operations.'
Assert-Match $core 'pub\s+async\s+fn\s+shutdown' 'Shared core must expose owned-runtime shutdown.'
# Match actual Axum/direct http crate dependencies, not similarly named transport types such as reqwest::StatusCode.
Assert-NotMatch $core '(?m)^\s*use\s+(?:axum|http)(?:::|\s*\{)|\baxum::|\bhttp::(?:HeaderMap|StatusCode)\b' 'Shared core must stay independent from Axum/HTTP presentation concerns.'
Assert-RustTest $core 'assembled_core_keeps_one_canonical_store_and_backend' 'Shared core canonical-store regression test is missing.'

# Phase 21.2 Windows Slint shell. The desktop UI is a presentation adapter over
# StreamArchiveCore, not another HTTP client, persistence authority, or process owner.
Assert-Match $guiManifest 'name\s*=\s*"stream-archive-gui"' 'Slint desktop crate is missing.'
Assert-Match $guiManifest 'slint\s*=\s*\{' 'Slint runtime dependency is missing.'
Assert-Match $guiManifest 'stream-archive-server\s*=\s*\{\s*path\s*=\s*"\.\./rust-web"' 'Slint desktop must depend on the shared Rust library directly.'
Assert-Match $guiSources 'StreamArchiveCore' 'Slint bootstrap must use the shared application core.'
Assert-Match $guiSources 'resolve_backend_dir' 'Slint bootstrap must reuse canonical backend resolution.'
Assert-Match $guiSources 'bind_core_snapshot' 'Slint shell must bind runtime state from the shared core.'
Assert-NotMatch $guiSources '\breqwest::|\baxum::|https?://127\.0\.0\.1|https?://localhost|rusqlite::|Command::new|taskkill|pkill|killall' 'Slint shell must not bypass shared core through HTTP, SQLite, or direct process control.'
Assert-Match $guiUi 'export\s+global\s+AppState' 'Slint shell state boundary is missing.'
Assert-Match $guiUi 'callback\s+refresh-requested' 'Slint runtime refresh callback is missing.'
foreach ($page in @('Dashboard', 'LIVE', 'VOD', 'Queue', 'History', 'Settings')) {
    Assert-Match $guiUi ([regex]::Escape("page: `"$page`"")) "Slint navigation page missing: $page"
}

# Provider registry/facades.
Assert-Match $platform 'pub\s+mod\s+live' 'Platform LIVE facade must be registered.'
Assert-Match $platform 'pub\s+mod\s+vod' 'Platform VOD facade must be registered.'
Assert-Match $platform 'pub\s+mod\s+soop' 'SOOP provider module must be registered.'
Assert-Match $platform 'trait\s+PlatformProvider' 'Platform provider boundary is missing.'
Assert-Match $platform 'fn\s+detect_vod_platform' 'Platform registry must own VOD platform detection.'
Assert-Match $platformSoop 'pub\s+mod\s+live' 'SOOP LIVE provider must be registered.'
Assert-Match $platformSoop 'pub\s+mod\s+vod' 'SOOP VOD provider must be registered.'
Assert-Match $platformSoop 'fn\s+accepts_vod_url' 'SOOP provider must own SOOP VOD URL recognition.'
Assert-Match $platformLive 'enum\s+LiveSession' 'Common LIVE session facade is missing.'
Assert-Match $platformVod 'pub\s+struct\s+VodManager' 'Common VOD manager facade is missing.'
Assert-Match $support 'provider\(platform\)' 'Channel lookup must route through the platform provider.'

# Provider-specific network/auth details must stay below the platform boundary.
Assert-NotMatch $watcher 'https?://[^\s"'']*sooplive\.com|player_live_api\.php|LoginAction\.php' 'Native watcher must not contain direct SOOP network endpoints.'
Assert-Match $platformSoopLive 'player_live_api\.php' 'SOOP LIVE provider must own SOOP live discovery endpoints.'
Assert-NotMatch $vodFacade 'sooplive\.com|CloudFront|yt-dlp|ffmpeg|taskkill\.exe' 'Root VOD facade must stay platform/process neutral.'
Assert-NotMatch $platformVod 'CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php' 'Common VOD facade must not own provider network/auth implementation details.'
Assert-Match $platformSoopVod 'private_auth\.php' 'SOOP VOD provider must own SOOP authorization.'
Assert-Match $platformSoopVod 'vod\.sooplive\.com' 'SOOP VOD provider must own SOOP VOD endpoints.'

# Queue/orchestration may identify a provider and include routing fixtures in tests,
# but must not contain provider authentication/network implementation.
Assert-Match $queue 'detect_vod_platform' 'VOD queue must persist detected platform identity.'
Assert-NotMatch $queue 'CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php' 'VOD queue must remain provider-neutral.'

Write-Host 'Architecture contracts passed.'

# Phase 21.3: inspect every GUI module, not only the small bootstrap.
Assert-Match $core 'pub\s+async\s+fn\s+update_environment_settings' 'Native settings must persist through shared core.'
Assert-Match $core 'pub\s+fn\s+diagnostics' 'Structured diagnostics service is missing.'
Assert-Match $guiSources 'core\.update_environment_settings' 'GUI settings bypass shared service.'
Assert-Match $guiSources 'core\.diagnostics\(' 'GUI diagnostics must consume shared diagnostics.'
Assert-NotMatch $guiSources 'std::fs::write|fs::write|Connection::open|std::process|tokio::process' 'GUI must not write runtime files or own child processes.'
Assert-Match $guiUi 'Diagnostics.*read-only' 'Read-only diagnostics must be distinguished from editable settings.'
