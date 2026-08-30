$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$backend = Join-Path $root 'backend'
$overlay = Join-Path $root 'overlay'
$mainPath = Join-Path $backend 'SOOP_LIVE.ps1'
$main = Get-Content -LiteralPath $mainPath -Raw -Encoding UTF8
$requiredModules = @(
    'SOOP.Security.ps1',
    'SOOP.Core.ps1',
    'SOOP.Network.ps1',
    'SOOP.Recorder.ps1'
)

foreach ($testScript in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -File) {
    $scriptText = [IO.File]::ReadAllText($testScript.FullName,[Text.Encoding]::UTF8)
    if ($scriptText.ToCharArray() | Where-Object { [int]$_ -gt 127 } | Select-Object -First 1) {
        throw "PowerShell 5.1-compatible source test contains non-ASCII text: $($testScript.Name)"
    }
}

if ([regex]::IsMatch($main,'(?m)^function\s+')) {
    throw 'SOOP_LIVE.ps1 must remain orchestration-only; function definitions belong in modules.'
}
foreach ($name in $requiredModules) {
    $path = Join-Path $backend (Join-Path 'modules' $name)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing backend module: $name" }
    if ($main -notmatch [regex]::Escape($name)) { throw "Main watcher does not require module: $name" }
}

$security = Get-Content -LiteralPath (Join-Path $backend 'modules\SOOP.Security.ps1') -Raw -Encoding UTF8
if ($security -notmatch 'dpapi:v1:' -or $security -notmatch 'ProtectedData.*Unprotect') {
    throw 'PowerShell DPAPI compatibility implementation is missing.'
}

$guiSecurity = Get-Content -LiteralPath (Join-Path $root 'overlay\SecretProtectionService.cs') -Raw -Encoding UTF8
$settings = Get-Content -LiteralPath (Join-Path $root 'overlay\SettingsFix36.cs') -Raw -Encoding UTF8
if ($guiSecurity -notmatch 'CryptProtectData' -or $guiSecurity -notmatch 'CryptUnprotectData' -or
    $settings -notmatch 'SecretProtectionService\.Protect' -or
    $settings -notmatch 'BackupProtectedSettingsFix52') {
    throw 'GUI DPAPI save/unprotect integration is missing.'
}

$cliSettings = Get-Content -LiteralPath (Join-Path $backend 'SOOP_LIVE_SETTING.ps1') -Raw -Encoding UTF8
if ($cliSettings -notmatch 'Protect-DpapiSecret' -or
    $cliSettings -notmatch 'Protect-IniSecretText' -or
    $security -notmatch 'function\s+Protect-IniSecretText') {
    throw 'CLI settings or legacy backup DPAPI migration is missing.'
}

$example = Get-Content -LiteralPath (Join-Path $backend 'SOOP_LIVE_SETTING.example.ini') -Raw -Encoding UTF8
# On CRLF files, `.` also consumes the carriage return before multiline `$`.
# Require an actual non-line-ending character after `=` so empty defaults do
# not become false positives on Windows PowerShell/GitHub Actions.
$credentialValuePattern = '(?m)^(SOOP_PASSWORD|CLOUDFLARE_API_KEY)=[^\r\n]+'
if ($example -match $credentialValuePattern) {
    throw 'Distributed example contains a credential value.'
}
if ("SOOP_PASSWORD=`r`nCLOUDFLARE_API_KEY=`r`n" -match $credentialValuePattern) {
    throw 'CRLF empty credential regression fixture was treated as populated.'
}
if ("SOOP_PASSWORD=not-empty`r`n" -notmatch $credentialValuePattern) {
    throw 'Populated credential regression fixture was not detected.'
}

$sync = Get-Content -LiteralPath (Join-Path $root 'SYNC_PROJECT.ps1') -Raw -Encoding UTF8
if ($sync -notmatch 'Get-ChildItem\s+-LiteralPath\s+\$backend\s+-Recurse') {
    throw 'Generated project synchronization does not recurse into backend modules.'
}

$windowCore = Get-Content -LiteralPath (Join-Path $overlay 'MainWindow.xaml.cs') -Encoding UTF8
if ($windowCore.Count -gt 1200) { throw 'MainWindow.xaml.cs role split regressed into a monolithic file.' }
foreach ($name in @(
    'MainWindow.Views.cs',
    'MainWindow.Lifecycle.cs',
    'MainWindow.BackendDashboard.cs',
    'MainWindow.Channels.cs',
    'MainWindow.ChannelParsing.cs',
    'MainWindow.RecordingActions.cs',
    'DesignTokens.cs',
    'RecentRecordingStore.cs',
    'DiagnosticInfoService.cs'
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $overlay $name) -PathType Leaf)) {
        throw "Missing separated UI responsibility: $name"
    }
}

Write-Host 'Security and backend module invariants passed.'
