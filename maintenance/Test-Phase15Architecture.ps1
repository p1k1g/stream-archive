$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$app = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\app.js') -Raw -Encoding UTF8
$phase13 = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\phase13.js') -Raw -Encoding UTF8
$phase14 = Get-Content -LiteralPath (Join-Path $root 'rust-web\web\phase14.js') -Raw -Encoding UTF8
$platform = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\mod.rs') -Raw -Encoding UTF8
$platformSoop = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\soop\mod.rs') -Raw -Encoding UTF8
$support = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\support.rs') -Raw -Encoding UTF8

function Assert-Contains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) { throw $Message }
}

function Assert-NotContains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -match $Pattern) { throw $Message }
}

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

Assert-Contains $platform 'trait\s+PlatformProvider' 'Platform provider boundary is missing.'
Assert-Contains $platform 'mod\s+soop' 'SOOP provider module must be registered.'
Assert-Contains $platform 'fn\s+detect_vod_platform' 'Platform registry must own VOD platform detection.'
Assert-Contains $platformSoop 'fn\s+accepts_vod_url' 'SOOP provider must own SOOP VOD URL recognition.'
Assert-Contains $support 'provider\(platform\)' 'Channel lookup must route through the platform provider.'

Write-Host 'Phase 15/16 architecture regression checks passed.'
