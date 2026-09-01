$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Auth.ps1')
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('soop-vod-auth-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $jar = New-Object System.Net.CookieContainer
    $cookie = New-Object System.Net.Cookie('AuthTicket', 'test-value', '/', '.sooplive.com')
    $cookie.Secure = $true
    $jar.Add($cookie)
    $path = Join-Path $tempRoot 'cookies.txt'
    $count = Export-VodNetscapeCookies -CookieContainer $jar -Destination $path
    if ($count -ne 1 -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw 'Netscape cookie export failed.'
    }
    $dataLine = Get-Content -LiteralPath $path -Encoding UTF8 | Where-Object { -not $_.StartsWith('#') } | Select-Object -First 1
    $fields = $dataLine -split "`t", 7
    if ($fields.Count -ne 7 -or $fields[0] -ne '.sooplive.com' -or $fields[5] -ne 'AuthTicket') {
        throw 'Netscape cookie fields are invalid.'
    }
    Add-Content -LiteralPath $path -Encoding UTF8 -Value @(
        "cdn.example.test`tFALSE`t/`tTRUE`t0`tCloudFront-Policy`tstale-policy",
        "cdn.example.test`tFALSE`t/`tTRUE`t0`tCloudFront-Signature`tstale-signature",
        "cdn.example.test`tFALSE`t/`tTRUE`t0`tCloudFront-Key-Pair-Id`tstale-key",
        ".sooplive.com`tTRUE`t/`tTRUE`t0`tCloudFront-Policy`tpolicy",
        ".sooplive.com`tTRUE`t/`tTRUE`t0`tCloudFront-Signature`tsignature",
        ".sooplive.com`tTRUE`t/`tTRUE`t0`tCloudFront-Key-Pair-Id`tkey"
    )
    $aliasCount = Repair-VodCloudFrontCookieScope -CookieFile $path -ResourceUrl 'https://cdn.example.test/master.m3u8'
    $aliased = Get-Content -LiteralPath $path -Encoding UTF8 | Where-Object { $_ -like "cdn.example.test`t*" }
    if ($aliasCount -ne 3 -or @($aliased).Count -ne 3) {
        throw 'CloudFront signed cookies were not scoped to the manifest host.'
    }
    $allSigned = Get-Content -LiteralPath $path -Encoding UTF8 | Where-Object { $_ -match "`tCloudFront-(?:Policy|Signature|Key-Pair-Id)`t" }
    if (@($allSigned).Count -ne 3 -or ($allSigned -join "`n") -match 'stale-') {
        throw 'Stale duplicate CloudFront cookies were retained.'
    }
    $master = Join-Path $tempRoot 'master.m3u8'
    [IO.File]::WriteAllLines($master, @(
        '#EXTM3U',
        '#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080',
        '1080.m3u8',
        '#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720',
        '720.m3u8'
    ), [Text.UTF8Encoding]::new($false))
    $qualities = @(Get-VodManifestQualityOptions -Path $master)
    if ($qualities.Count -ne 3 -or $qualities[1] -notmatch '1080' -or $qualities[2] -notmatch '720') {
        throw 'Master manifest qualities were not sorted or exposed.'
    }
    $request = [pscustomobject]@{ CookieMode = 'FILE'; CookieFile = $path; BrowserName = ''; VodUrl = 'https://vod.sooplive.com/player/1' }
    $jobRoot = Join-Path $tempRoot 'job'
    New-Item -ItemType Directory -Path $jobRoot -Force | Out-Null
    $copied = Initialize-VodCookie -Request $request -JobDirectory $jobRoot -YtDlp 'unused.exe' -BackendRoot $root
    if ($copied.Mode -ne 'FILE' -or -not (Test-Path -LiteralPath $copied.Path -PathType Leaf)) {
        throw 'FILE cookie mode did not accept a valid Netscape cookie file.'
    }
    if (-not $copied.HasCloudFrontAuthorization -or -not $copied.HasSoopLoginCookies) {
        throw 'Cookie capabilities did not recognize signed and login cookies.'
    }
    $invalid = Join-Path $tempRoot 'invalid.txt'
    [IO.File]::WriteAllText($invalid, 'name=value', [Text.UTF8Encoding]::new($false))
    $request.CookieFile = $invalid
    $rejected = $false
    try { [void](Initialize-VodCookie -Request $request -JobDirectory $jobRoot -YtDlp 'unused.exe' -BackendRoot $root) }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Malformed FILE cookie input was accepted.' }
    Remove-VodTemporarySecrets -JobDirectory $tempRoot
    if (Test-Path -LiteralPath $path) { throw 'Temporary cookie cleanup failed.' }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host 'VOD Netscape cookie regression tests passed.'
