function Initialize-VodCookie {
    param($Request, [string]$JobDirectory, [string]$YtDlp)
    $mode = ([string]$Request.CookieMode).ToUpperInvariant()
    $temporary = Join-Path $JobDirectory 'cookies.txt'
    if ($mode -eq 'FILE') {
        $source = [string]$Request.CookieFile
        if ([string]::IsNullOrWhiteSpace($source) -or -not (Test-Path -LiteralPath $source -PathType Leaf)) { throw '사용 가능한 Cookie 파일이 없습니다.' }
        Copy-Item -LiteralPath $source -Destination $temporary -Force
    }
    elseif ($mode -eq 'BROWSER') {
        $browser = [string]$Request.BrowserName
        if ([string]::IsNullOrWhiteSpace($browser)) { throw '브라우저 이름이 비어 있습니다.' }
        & $YtDlp '--cookies-from-browser' $browser '--cookies' $temporary '--skip-download' ([string]$Request.VodUrl) 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $temporary -PathType Leaf)) { throw '브라우저 Cookie 추출에 실패했습니다.' }
    }
    else { throw "지원하지 않는 Cookie 모드입니다: $mode" }
    return [pscustomobject]@{ Path = $temporary; Mode = $mode }
}

function Refresh-VodAuthorization {
    param($Request, [string]$CookieFile, [string]$StreamerId, [string]$Url, [int]$Attempt)
    Write-VodEvent -Type 'auth_refreshing' -Message ("VOD 인증 갱신 중 ({0}/{1})" -f $Attempt, [int]$Request.MaxRetries)
    $response = & curl.exe '-sS' '-b' $CookieFile '-c' $CookieFile '-e' ([string]$Request.VodUrl) '-H' 'Origin: https://vod.sooplive.com' '--data-urlencode' 'type=vod' '--data-urlencode' ("strm_id=$StreamerId") '--data-urlencode' ("title_no=" + ([regex]::Match([string]$Request.VodUrl, '/player/(\d+)').Groups[1].Value)) '--data-urlencode' ("url=$Url") 'https://live.sooplive.com/api/private_auth.php' 2>&1
    return ($LASTEXITCODE -eq 0 -and (($response -join "`n") -match '"result"\s*:\s*1'))
}

function Remove-VodTemporarySecrets {
    param([string]$JobDirectory)
    if ([string]::IsNullOrWhiteSpace($JobDirectory)) { return }
    foreach ($name in @('cookies.txt', 'concat.txt')) {
        $path = Join-Path $JobDirectory $name
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue }
    }
}
