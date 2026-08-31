$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$backendDir = Join-Path $root 'backend'
$backend = (Get-Content -LiteralPath (Join-Path $backendDir 'SOOP_LIVE.ps1') -Raw -Encoding UTF8) + "`n" +
    ((Get-ChildItem -LiteralPath (Join-Path $backendDir 'modules') -Filter '*.ps1' -File |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 }) -join "`n")
$main = (Get-ChildItem -LiteralPath (Join-Path $root 'overlay') -Filter 'MainWindow*.cs' -File |
    ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 }) -join "`n"
$settings = Get-Content -LiteralPath (Join-Path $root 'overlay\SettingsFix36.cs') -Raw -Encoding UTF8
$features = Get-Content -LiteralPath (Join-Path $root 'overlay\UserFeaturesFix51.cs') -Raw -Encoding UTF8
$recentStore = Get-Content -LiteralPath (Join-Path $root 'overlay\RecentRecordingStore.cs') -Raw -Encoding UTF8
$diagnostics = Get-Content -LiteralPath (Join-Path $root 'overlay\DiagnosticInfoService.cs') -Raw -Encoding UTF8
$models = Get-Content -LiteralPath (Join-Path $root 'overlay\Models.cs') -Raw -Encoding UTF8
$design = Get-Content -LiteralPath (Join-Path $root 'overlay\DesignTokens.cs') -Raw -Encoding UTF8
$channelSync = Get-Content -LiteralPath (Join-Path $root 'overlay\ChannelCollectionSynchronizer.cs') -Raw -Encoding UTF8
$channelPerfTest = Get-Content -LiteralPath (Join-Path $root 'tests\ChannelCollectionSynchronizerRegression.cs') -Raw -Encoding UTF8
$uiPreferences = Get-Content -LiteralPath (Join-Path $root 'overlay\UiPreferences.cs') -Raw -Encoding UTF8
$recentRecordingStore = Get-Content -LiteralPath (Join-Path $root 'overlay\RecentRecordingStore.cs') -Raw -Encoding UTF8
$pipelineDefense = Get-Content -LiteralPath (Join-Path $root 'overlay\EventPipelineDefense.cs') -Raw -Encoding UTF8
$pipelineSoak = Get-Content -LiteralPath (Join-Path $root 'tests\EventPipelineSoakRegression.cs') -Raw -Encoding UTF8

function ConvertFrom-CodePoints([int[]]$CodePoints) {
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

if ($backend -notmatch 'elseif\s*\(\$action\s+-eq\s+"RECHECK"\)') {
    throw 'Per-account RECHECK command handling is missing.'
}
if ($recentStore -notmatch 'void\s+AtomicReplace\(string\s+content\)' -or
    $recentStore -notmatch 'AtomicReplace\(content\)' -or
    $recentStore -notmatch 'File\.WriteAllText\(temporary,\s*content' -or
    $recentStore -notmatch 'JsonDocument\.Parse\(File\.ReadAllText\(temporary' -or
    $recentStore -notmatch 'File\.Replace\(temporary,\s*path' -or
    $recentStore -notmatch 'File\.Move\(temporary,\s*path') {
    throw 'Recent recording history is not using atomic replacement.'
}
if ($diagnostics -notmatch 'TakeLast\(50\)') {
    throw 'Diagnostic copy is not bounded to the latest 50 GUI events.'
}
if ($diagnostics -notmatch 'worker_api_key\|password\|passwd') {
    throw 'Diagnostic token redaction guard is missing.'
}
if ($settings -notmatch 'settingsUiTransitionDepth\s*==\s*0') {
    throw 'Advanced-settings transition dirty guard is missing.'
}
if ($main -notmatch 'RefreshChannelNamesPreviewFix51_Click') {
    throw 'Channel-name refresh preview action is not wired.'
}
if ($models -notmatch 'CardBackground' -or $models -notmatch 'StateForeground' -or
    $main -notmatch 'Background="\{Binding CardBackground\}"' -or
    $main -notmatch 'Foreground="\{Binding StateForeground\}"') {
    throw 'Enabled/disabled channel visual distinction is missing.'
}
$applyLabel = ConvertFrom-CodePoints @(0xBCC0,0xACBD,0x20,0xC0AC,0xD56D,0x20,0xBC18,0xC601)
$confirmLabel = ConvertFrom-CodePoints @(0xD655,0xC778)
$cancelLabel = ConvertFrom-CodePoints @(0xCDE8,0xC18C)
$applyPattern = 'PrimaryButtonText\s*=\s*"' + [regex]::Escape($applyLabel) + '"'
$confirmPattern = 'SecondaryButtonText\s*=\s*"' + [regex]::Escape($confirmLabel) + '"'
$cancelPattern = 'CloseButtonText\s*=\s*"' + [regex]::Escape($cancelLabel) + '"'
if ($features -notmatch $applyPattern -or
    $features -notmatch $confirmPattern -or
    $features -notmatch $cancelPattern) {
    throw 'Channel-name preview apply/confirm/cancel actions are not distinct.'
}
if ($main -notmatch 'NavigationItemFix39\("[^"]+",\s*"recent",\s*Symbol\.Video\)') {
    throw 'Recent-recordings navigation icon is not distinct from the log document icon.'
}
if ($settings -notmatch 'DispatcherQueue\.CreateTimer\(\)' -or
    $settings -notmatch 'settingsUiTransitionDepth\s*=\s*Math\.Max') {
    throw 'Advanced-settings deferred layout dirty guard is missing.'
}
if ($design -notmatch 'class\s+DesignTokens' -or
    $design -notmatch 'AccessibilitySettings' -or
    $design -notmatch 'AddAccelerator') {
    throw 'Design tokens, high-contrast semantics, or keyboard accelerators are missing.'
}
if ($main -notmatch 'new\s+CommandBar' -or
    $main -notmatch 'BuildChannelTemplate\(bool\s+compact\)' -or
    $features -notmatch 'BuildRecentRecordingTemplateFix51\(bool\s+compact\)') {
    throw 'Responsive channel/recent templates or command surfaces are missing.'
}
if ($main -match 'VisibleChannelItems\.Clear\(\)' -or
    $main -notmatch 'ScheduleChannelFilterRefresh' -or
    $main -notmatch 'TimeSpan\.FromMilliseconds\(250\)' -or
    $main -notmatch 'CaptureChannelScrollAnchor' -or
    $main -notmatch 'RestoreChannelScrollAnchor') {
    throw 'Debounced channel filtering or selection/scroll preservation is missing.'
}
if ($channelSync -notmatch 'ObservableCollection<T>' -or
    $channelSync -notmatch '\.Move\(' -or
    $channelPerfTest -notmatch '10_000' -or
    $channelPerfTest -notmatch 'TotalChanges\s*==\s*0') {
    throw 'Minimal channel collection synchronization or large-list regression coverage is missing.'
}
if ($settings -notmatch 'CaptureSettingsSnapshotFix59' -or
    $settings -notmatch 'savedSettingsSnapshot' -or
    $settings -notmatch 'SettingsSnapshot\.Create') {
    throw 'Snapshot-based settings dirty tracking is missing.'
}
if ($uiPreferences -notmatch 'LocalApplicationData' -or
    $uiPreferences -notmatch 'File\.Replace' -or
    $uiPreferences -notmatch 'LastView' -or
    $uiPreferences -notmatch 'WindowWidth' -or
    $uiPreferences -notmatch 'UiDensity') {
    throw 'Atomic LocalAppData UI state persistence is incomplete.'
}
if ($recentRecordingStore -notmatch 'pendingEntries' -or
    $recentRecordingStore -notmatch 'Task\.Run\(ProcessPendingSavesAsync\)' -or
    $recentRecordingStore -notmatch 'Task\.Delay\(250\)' -or
    $recentRecordingStore -notmatch 'FlushAsync') {
    throw 'Background-coalesced recent recording persistence is missing.'
}
if ($pipelineDefense -notmatch 'BoundedConcurrentQueue<T>' -or
    $pipelineDefense -notmatch 'WarningDeduplicator' -or
    $pipelineDefense -notmatch 'DriveSpaceCache' -or
    $pipelineDefense -notmatch 'ProgressSnapshot' -or
    $main -notmatch 'MaxQueuedPriorityEvents\s*=\s*512' -or
    $main -match 'latestProgressByChannel\.ToArray\(\)') {
    throw 'Event pipeline bounds, deduplication, caching, or allocation guards are missing.'
}
if ($pipelineSoak -notmatch '24\s*\*\s*60\s*\*\s*60' -or
    $pipelineSoak -notmatch '100_000' -or
    $pipelineSoak -notmatch 'progress\.Count\s*==\s*64') {
    throw 'Long-running multi-channel event pipeline soak coverage is missing.'
}

Write-Host 'User feature source invariants passed.'
