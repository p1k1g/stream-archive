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
$merge = Get-Content -LiteralPath (Join-Path $root 'backend/vod/modules/SOOP.Vod.Merge.ps1') -Raw
if ($auth -match '(?im)^\s*\$host\s*=') { throw 'VOD auth must not assign the read-only PowerShell Host automatic variable.' }
if ($core -notmatch 'Get-CollisionSafeVodPath' -or $core -notmatch 'Test-Path -LiteralPath') { throw 'VOD collision/literal-path protection is missing.' }
if ($core -notmatch 'Get-RedactedVodText' -or $auth -match 'Write-(Host|Output)\s+\$response') { throw 'VOD authentication output is not safely redacted.' }
if ($download -notmatch '--abort-on-unavailable-fragments' -or $download -notmatch '--continue') { throw 'VOD resume/fragment safeguards are missing.' }
if ($auth -notmatch '\$mode -eq ''SOOP_LOGIN''' -or
    $auth -notmatch 'Resolve-ProtectedConfigSecrets' -or
    $auth -notmatch 'LoginAction\.php' -or
    $auth -notmatch 'ReadAsByteArrayAsync' -or
    $auth -notmatch 'New-VodSoopLoginCookie.+VodUrl' -or
    $auth -notmatch 'Repair-VodCloudFrontCookieScope' -or
    $auth -notmatch 'Retain only the last value curl wrote' -or
    $auth -notmatch 'Get-VodCookieCapabilities' -or
    $auth -notmatch 'Get-VodCloudFrontPolicyResource' -or
    $auth -notmatch 'New-VodCloudFrontCurlConfig' -or
    $auth -notmatch 'Remove-VodCloudFrontCookies' -or
    $auth -notmatch 'Test-VodManifestAuthorization' -or
    $auth -notmatch '--config.+\$curlConfig' -or
    $auth -notmatch 'Get-VodManifestQualityOptions' -or
    $auth -notmatch 'manifest-probe-\*' -or
    $auth -notmatch 'Export-VodNetscapeCookies' -or
    $auth -notmatch "@\('cookies\.txt', 'concat\.txt'\)") {
    throw 'Stored SOOP login, Netscape export, or temporary secret cleanup is missing.'
}
if ($download -notmatch 'Renew-VodBaseCookie' -or
    ([regex]::Matches($download, 'Refresh-VodAuthorization')).Count -lt 1 -or
    $download -notmatch 'for \(\$attempt' -or
    ([regex]::Matches($download, 'Get-VodMetadata')).Count -lt 2 -or
    $download -notmatch 'authorization_expired' -or
    $download -notmatch 'manifest-probe-' -or
    $download -notmatch '--ignore-no-formats-error' -or
    $download -notmatch 'Get-VodAnalysisQualities' -or
    $download -notmatch 'usedExistingSignedCookie' -or
    $download -notmatch '\$Cookie\.PolicyResource' -or
    $download -notmatch 'Get-VodEntryManifestUrl' -or
    $download -notmatch 'Complete-VodManifestUrlsFromApi' -or
    $download -notmatch 'api\.m\.sooplive\.co\.kr/station/video/a/view' -or
    $download -notmatch '\$Request\.Quality' -or
    $download -notmatch 'Origin:https://vod\.sooplive\.com' -or
    $download -notmatch '\$ErrorActionPreference = ''Continue''' -or
    $download -notmatch '\$ErrorActionPreference = \$previousErrorActionPreference' -or
    $download -notmatch 'throw "PART \$part [^"]*: \$lastFailureDetail"') {
    throw 'Short-lived subscription authorization, metadata URL, or request headers are not refreshed inside the retry loop.'
}
$entrySource = Get-Content -LiteralPath $entry -Raw
if ($entrySource -notmatch '\$script:VodRequest\.AnalyzeOnly' -or $entrySource -notmatch '-Qualities \$qualities') {
    throw 'VOD analysis and download phases are not separated.'
}
$processService = Get-Content -LiteralPath (Join-Path $root 'overlay/VodProcessService.cs') -Raw
if ($processService -notmatch 'StandardOutputEncoding\s*=\s*new UTF8Encoding' -or
    $processService -notmatch 'StandardErrorEncoding\s*=\s*new UTF8Encoding' -or
    $processService -notmatch '\[Console\]::OutputEncoding' -or
    $processService -notmatch '\$OutputEncoding=') {
    throw 'VOD PowerShell UTF-8 process boundary is incomplete.'
}
if ($download -match '--dump-single-json''.*2>&1' -or
    $download -notmatch 'yt-dlp-metadata\.stderr\.log' -or
    $download -notmatch 'CultureInfo\]::InvariantCulture') {
    throw 'VOD metadata stderr separation or invariant progress parsing is missing.'
}
if ($download -notmatch '\$entries\[0\]\.uploader_id' -or
    $download -notmatch 'VOD BJ ID') {
    throw 'VOD metadata does not fall back to the first PART uploader ID required by private_auth.'
}
if ($download -notmatch '\$Request\.YtDlpPath' -or
    $download -notmatch 'yt-dlp-metadata\.json' -or
    $download -notmatch 'WriteAllText\(\$metadataFile' -or
    $core -notmatch 'NormalizationForm\]::FormC' -or
    $core -notmatch 'Assert-VodFullPath' -or
    $merge -notmatch 'ConvertTo-VodFfmpegConcatLine') {
    throw 'VOD tool path, UTF-8 metadata, Unicode, path-length, or concat safeguards are incomplete.'
}
if ((Get-Content -LiteralPath (Join-Path $root 'backend/SOOP_LIVE.ps1') -Raw) -match 'SOOP_VOD|SOOP\.Vod') { throw 'LIVE bootstrap must not load VOD modules.' }
$models = Get-Content -LiteralPath (Join-Path $root 'overlay/VodModels.cs') -Raw
if ($models -match 'SoopPassword|SOOP_PASSWORD|Dpapi|AuthCookie') { throw 'VOD request model must not contain credentials or cookie values.' }
if ($models -notmatch 'string YtDlpPath' -or $models -notmatch 'string FfmpegPath') {
    throw 'VOD request model does not carry the non-secret tool paths.'
}
$vodUi = Get-Content -LiteralPath (Join-Path $root 'overlay/MainWindow.Vod.cs') -Raw
if ($vodUi -notmatch 'cookieMode == "SOOP_LOGIN"' -or
    $vodUi -notmatch 'cookieFile = ""' -or
    $vodUi -notmatch 'browserName = ""') {
    throw 'Stored-login request does not clear fallback cookie/browser fields.'
}
if ($vodUi -notmatch 'FileOpenPicker' -or
    $vodUi -notmatch 'FileTypeFilter\.Add\("\.exe"\)' -or
    $vodUi -notmatch 'PickVodExecutableAsync') {
    throw 'VOD executable paths cannot be selected with the Windows file picker.'
}
if ($vodUi -notmatch 'ApplyVodAnalysis' -or $vodUi -notmatch 'VodQualityBox' -or $vodUi -notmatch 'analyzedVodPartCount') {
    throw 'VOD UI does not wait for analysis before PART and quality selection.'
}
$explorerSources = Get-ChildItem -LiteralPath (Join-Path $root 'overlay') -Filter '*.cs' |
    ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }
$explorerText = $explorerSources -join "`n"
if ($explorerText -match 'ProcessStartInfo\("explorer\.exe",' -or
    $explorerText -notmatch 'ArgumentList\.Add') {
    throw 'Explorer paths must be passed through ProcessStartInfo.ArgumentList.'
}
$service = $processService
if ($service -notmatch '"/PID"' -or $service -notmatch 'Kill\(entireProcessTree: true\)' -or $service -match 'taskkill.+/IM') { throw 'VOD exact process-tree ownership guard is missing.' }
Write-Host 'VOD backend source invariants passed.'
