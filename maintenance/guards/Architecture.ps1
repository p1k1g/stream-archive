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
$store = Read-RepoFile 'rust-web/src/store.rs'
$main = Read-RepoFile 'rust-web/src/main.rs'



# Frontend state state guarantees remain prerequisites for platform expansion.
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

# SQLite is authoritative; INI/TXT files are compatibility mirrors emitted from it.
Assert-Match $main 'Store::open\(Store::default_path\(&backend_dir\)\)' 'Server startup must open the canonical SQLite store.'
Assert-Match $main 'store\.bootstrap_primary_once\(&backend_dir\)' 'Legacy config import must remain a one-time SQLite bootstrap.'
Assert-Match $main 'materialize_primary_files\(&store, &backend_dir\)' 'Compatibility mirrors must be materialized from SQLite.'
Assert-Match $store 'settings_cache: Arc<RwLock<BTreeMap<String, String>>>' 'Committed runtime settings cache is missing.'
Assert-RustTest $store 'runtime_config_cache_tracks_committed_writes' 'Committed settings-cache behavior test is missing.'
Assert-Match $store 'platform TEXT NOT NULL' 'Persistent history and queue schemas must retain platform identity.'
Assert-Match $queue 'lifecycle_lock: Arc<Mutex<\(\)>>' 'Queue must share the global serialized VOD lifecycle lock.'

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
# Common routing may contain provider URLs in tests, but must never implement
# provider authentication, signed-cookie, or direct provider endpoint mechanics.
Assert-NotMatch $platformVod 'CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php' 'Common VOD facade must not own provider network/auth implementation details.'
Assert-Match $platformSoopVod 'private_auth\.php' 'SOOP VOD provider must own SOOP authorization.'
Assert-Match $platformSoopVod 'vod\.sooplive\.com' 'SOOP VOD provider must own SOOP VOD endpoints.'

# Queue/orchestration may identify a provider and include routing fixtures in
# tests, but must not contain provider authentication/network implementation.
Assert-Match $queue 'detect_vod_platform' 'VOD queue must persist detected platform identity.'
Assert-NotMatch $queue 'CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php' 'VOD queue must remain provider-neutral.'

Write-Host 'Architecture contracts passed.'
