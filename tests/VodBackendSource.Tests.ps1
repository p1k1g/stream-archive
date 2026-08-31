$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$entry = Join-Path $root 'backend/vod/SOOP_VOD.ps1'
$modules = @(
    'SOOP.Vod.Core.ps1', 'SOOP.Vod.Auth.ps1',
    'SOOP.Vod.Download.ps1', 'SOOP.Vod.Merge.ps1'
)
if (-not (Test-Path -LiteralPath $entry -PathType Leaf)) { throw 'VOD backend entry script is missing.' }
foreach ($module in $modules) {
    $path = Join-Path $root "backend/vod/modules/$module"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "VOD module is missing: $module" }
}
$vodPowerShellFiles = @($entry) + @($modules | ForEach-Object { Join-Path $root "backend/vod/modules/$_" })
foreach ($path in $vodPowerShellFiles) {
    $bytes = [IO.File]::ReadAllBytes($path)
    if ($bytes.Length -lt 3 -or $bytes[0] -ne 0xEF -or $bytes[1] -ne 0xBB -or $bytes[2] -ne 0xBF) {
        throw "VOD PowerShell file must use UTF-8 BOM for Windows PowerShell 5.1: $path"
    }
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -ne 0) {
        throw "VOD PowerShell parser error: $path - $($parseErrors[0].Message)"
    }
}
$core = Get-Content -LiteralPath (Join-Path $root 'backend/vod/modules/SOOP.Vod.Core.ps1') -Raw
$auth = Get-Content -LiteralPath (Join-Path $root 'backend/vod/modules/SOOP.Vod.Auth.ps1') -Raw
$download = Get-Content -LiteralPath (Join-Path $root 'backend/vod/modules/SOOP.Vod.Download.ps1') -Raw
if ($core -notmatch 'Get-CollisionSafeVodPath' -or $core -notmatch 'Test-Path -LiteralPath') { throw 'VOD collision/literal-path protection is missing.' }
if ($core -notmatch 'Get-RedactedVodText' -or $auth -match 'Write-(Host|Output)\s+\$response') { throw 'VOD authentication output is not safely redacted.' }
if ($download -notmatch '--abort-on-unavailable-fragments' -or $download -notmatch '--continue') { throw 'VOD resume/fragment safeguards are missing.' }
if ($auth -notmatch '\$mode -eq ''SOOP_LOGIN''' -or
    $auth -notmatch 'Resolve-ProtectedConfigSecrets' -or
    $auth -notmatch 'LoginAction\.php' -or
    $auth -notmatch 'Export-VodNetscapeCookies' -or
    $auth -notmatch "@\('cookies\.txt', 'concat\.txt'\)") {
    throw 'Stored SOOP login, Netscape export, or temporary secret cleanup is missing.'
}
if ($download -notmatch 'Renew-VodBaseCookie' -or
    ([regex]::Matches($download, 'Refresh-VodAuthorization')).Count -lt 1 -or
    $download -notmatch 'for \(\$attempt') {
    throw 'Short-lived subscription authorization is not refreshed inside the retry loop.'
}
if ((Get-Content -LiteralPath (Join-Path $root 'backend/SOOP_LIVE.ps1') -Raw) -match 'SOOP_VOD|SOOP\.Vod') { throw 'LIVE bootstrap must not load VOD modules.' }
$models = Get-Content -LiteralPath (Join-Path $root 'overlay/VodModels.cs') -Raw
if ($models -match 'SoopPassword|SOOP_PASSWORD|Dpapi|AuthCookie') { throw 'VOD request model must not contain credentials or cookie values.' }
$vodUi = Get-Content -LiteralPath (Join-Path $root 'overlay/MainWindow.Vod.cs') -Raw
if ($vodUi -notmatch 'cookieMode == "SOOP_LOGIN"' -or
    $vodUi -notmatch 'cookieFile = ""' -or
    $vodUi -notmatch 'browserName = ""') {
    throw 'Stored-login request does not clear fallback cookie/browser fields.'
}
$service = Get-Content -LiteralPath (Join-Path $root 'overlay/VodProcessService.cs') -Raw
if ($service -notmatch '"/PID"' -or $service -notmatch 'Kill\(entireProcessTree: true\)' -or $service -match 'taskkill.+/IM') { throw 'VOD exact process-tree ownership guard is missing.' }
Write-Host 'VOD backend source invariants passed.'
