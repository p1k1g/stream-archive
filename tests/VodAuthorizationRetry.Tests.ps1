$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Core.ps1')
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Auth.ps1')
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Download.ps1')

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('soop-vod-retry-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $fakeYtDlp = Join-Path $tempRoot 'fake-yt-dlp.cmd'
    [IO.File]::WriteAllText($fakeYtDlp, "@echo ERROR: HTTP Error 403: Forbidden 1>&2`r`n@exit /b 1`r`n", [Text.Encoding]::ASCII)
    $cookiePath = Join-Path $tempRoot 'cookies.txt'
    [IO.File]::WriteAllText($cookiePath, "# Netscape HTTP Cookie File`r`n.sooplive.com`tTRUE`t/`tTRUE`t0`tAuthTicket`tvalue`r`n", [Text.UTF8Encoding]::new($false))

    $script:metadataRefreshes = 0
    $script:baseRenewals = 0
    $script:authorizationRefreshes = 0
    function Get-VodMetadata {
        [void]($script:metadataRefreshes++)
        return [pscustomobject]@{
            StreamerId = 'account';
            Entries = @([pscustomobject]@{ url = 'https://vod.sooplive.com/refreshed.m3u8' })
        }
    }
    function Renew-VodBaseCookie { [void]($script:baseRenewals++) }
    function Refresh-VodAuthorization { [void]($script:authorizationRefreshes++); return $true }
    function Test-VodManifestAuthorization { return $true }
    function Start-Sleep { }

    $request = [pscustomobject]@{
        JobId = 'retry-test'; VodUrl = 'https://vod.sooplive.com/player/1';
        OutputDirectory = $tempRoot; MaxRetries = 2
    }
    $script:VodRequest = $request
    $metadata = [pscustomobject]@{
        Streamer = 'tester'; StreamerId = 'account'; Date = '260831';
        Entries = @([pscustomobject]@{ url = 'https://vod.sooplive.com/expired.m3u8' })
    }
    $tools = [pscustomobject]@{ YtDlp = $fakeYtDlp; Ffmpeg = '' }
    $cookie = [pscustomobject]@{ Path = $cookiePath; Mode = 'FILE' }
    $failed = $false
    $failureMessage = ''
    try { [void](Invoke-VodDownloads -Request $request -Metadata $metadata -SelectedParts @(1) -Tools $tools -Cookie $cookie -JobDirectory $tempRoot) }
    catch { $failed = $true; $failureMessage = $_.Exception.Message }

    if (-not $failed -or $failureMessage -notmatch '403' -or $script:baseRenewals -ne 1 -or $script:metadataRefreshes -ne 2 -or $script:authorizationRefreshes -ne 2) {
        throw ("A 403 retry did not renew every layer or retain its diagnostic. failed={0}, base={1}, metadata={2}, authorization={3}, message={4}" -f $failed, $script:baseRenewals, $script:metadataRefreshes, $script:authorizationRefreshes, $failureMessage)
    }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
# The fake native downloader intentionally exits with 1. GitHub Actions'
# PowerShell wrapper propagates a lingering LASTEXITCODE even after every
# assertion succeeds, so clear only that expected native test result.
$global:LASTEXITCODE = 0
Write-Host 'VOD authorization retry regression tests passed.'
