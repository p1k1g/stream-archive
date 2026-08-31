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
    Remove-VodTemporarySecrets -JobDirectory $tempRoot
    if (Test-Path -LiteralPath $path) { throw 'Temporary cookie cleanup failed.' }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host 'VOD Netscape cookie regression tests passed.'
