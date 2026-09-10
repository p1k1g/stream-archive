$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$app = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\app.js') -Raw -Encoding UTF8
$phase13 = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\phase13.js') -Raw -Encoding UTF8
$phase14 = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\phase14.js') -Raw -Encoding UTF8
$platform = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\mod.rs') -Raw -Encoding UTF8
$platformLive = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\live.rs') -Raw -Encoding UTF8
$platformVod = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\vod.rs') -Raw -Encoding UTF8
$platformSoop = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\soop\mod.rs') -Raw -Encoding UTF8
$platformSoopLive = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\soop\live.rs') -Raw -Encoding UTF8
$platformSoopVod = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\soop\vod.rs') -Raw -Encoding UTF8
$support = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\support.rs') -Raw -Encoding UTF8
$watcher = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\native_watcher.rs') -Raw -Encoding UTF8
$vodFacade = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\vod.rs') -Raw -Encoding UTF8
$queue = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\vod_queue.rs') -Raw -Encoding UTF8

function Assert-Contains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) { throw $Message }
}

function Assert-NotContains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -match $Pattern) { throw $Message }
}

# Phase 15 frontend state guarantees remain prerequisites for platform expansion.
Assert-Contains $app 'window\.StreamArchiveState\s*=\s*StreamArchiveState' 'app.js must expose the shared StreamArchiveState bus.'
Assert-Contains $app "StreamArchiveState\.publish\('status'" 'status must publish through StreamArchiveState.'
Assert-Contains $app "StreamArchiveState\.publish\('vod'" 'VOD status must publish through StreamArchiveState.'
Assert-Contains $app "StreamArchiveState\.publish\('snapshot'" 'SSE snapshots must publish through StreamArchiveState.'
Assert-Contains $phase13 "StreamArchiveState\?\.publish\('queue'" 'Phase13 queue must publish queue state through the shared bus.'
Assert-Contains $phase13 "StreamArchiveState\?\.subscribe\('snapshot'" 'Phase13 queue must consume SSE snapshots through the shared bus.'
Assert-NotContains $phase13 'renderStatus\s*=\s*function' 'Phase13 must not wrap renderStatus.'
Assert-NotContains $phase13 'renderVodStatus\s*=\s*function' 'Phase13 must not wrap renderVodStatus.'
Assert-NotContains $phase13 'applyRealtimeSnapshot\s*=\s*function' 'Phase13 must not wrap applyRealtimeSnapshot.'
Assert-NotContains $phase13 'p13Notify\s*\(' 'Legacy Phase13 browser notification code must stay removed.'
Assert-Contains $phase14 "bus\.subscribe\('status'" 'Phase14 notifications must subscribe to status state.'
Assert-Contains $phase14 "bus\.subscribe\('queue'" 'Phase14 notifications must subscribe to queue state.'
Assert-NotContains $phase14 'window\.renderStatus\s*=' 'Phase14 must not replace renderStatus.'
Assert-NotContains $phase14 'window\.api\s*=' 'Phase14 must not replace api.'
Assert-NotContains $phase14 'window\.applyRealtimeSnapshot\s*=' 'Phase14 must not replace applyRealtimeSnapshot.'
Assert-NotContains $phase14 '__p14Wrapped' 'Legacy Phase14 wrapper markers must stay removed.'

# Phase 16 provider registry/facades.
Assert-Contains $platform 'pub\s+mod\s+live' 'Platform LIVE facade must be registered.'
Assert-Contains $platform 'pub\s+mod\s+vod' 'Platform VOD facade must be registered.'
Assert-Contains $platform 'pub\s+mod\s+soop' 'SOOP provider module must be registered.'
Assert-Contains $platform 'trait\s+PlatformProvider' 'Platform provider boundary is missing.'
Assert-Contains $platform 'fn\s+detect_vod_platform' 'Platform registry must own VOD platform detection.'
Assert-Contains $platformSoop 'pub\s+mod\s+live' 'SOOP LIVE provider must be registered.'
Assert-Contains $platformSoop 'pub\s+mod\s+vod' 'SOOP VOD provider must be registered.'
Assert-Contains $platformSoop 'fn\s+accepts_vod_url' 'SOOP provider must own SOOP VOD URL recognition.'
Assert-Contains $platformLive 'enum\s+LiveSession' 'Common LIVE session facade is missing.'
Assert-Contains $platformVod 'pub\s+struct\s+VodManager' 'Common VOD manager facade is missing.'
Assert-Contains $support 'provider\(platform\)' 'Channel lookup must route through the platform provider.'

# Provider-specific network/auth details must stay below the platform boundary.
Assert-NotContains $watcher 'https?://[^\s"'']*sooplive\.com|player_live_api\.php|LoginAction\.php' 'Native watcher must not contain direct SOOP network endpoints.'
Assert-Contains $platformSoopLive 'player_live_api\.php' 'SOOP LIVE provider must own SOOP live discovery endpoints.'
Assert-NotContains $vodFacade 'sooplive\.com|CloudFront|yt-dlp|ffmpeg|taskkill\.exe' 'Root VOD facade must stay platform/process neutral.'
# The common dispatcher may contain provider URLs in routing tests, but it must
# never own provider authentication, signed-cookie, or HTTP endpoint mechanics.
Assert-NotContains $platformVod 'CloudFront|private_auth\.php|LoginAction\.php|player_live_api\.php' 'Common VOD facade must not own provider network/auth implementation details.'
Assert-Contains $platformSoopVod 'private_auth\.php' 'SOOP VOD provider must own SOOP authorization.'
Assert-Contains $platformSoopVod 'vod\.sooplive\.com' 'SOOP VOD provider must own SOOP VOD endpoints.'

# Queue/orchestration may identify a provider but must not implement provider networking/auth.
Assert-Contains $queue 'detect_vod_platform' 'VOD queue must persist detected platform identity.'
Assert-NotContains $queue 'sooplive\.com|CloudFront|private_auth\.php|LoginAction\.php' 'VOD queue must remain provider-neutral.'

Write-Host 'Phase 15/16 architecture regression checks passed.'
