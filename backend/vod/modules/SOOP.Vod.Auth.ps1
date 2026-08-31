function Read-VodLoginConfig {
    param([string]$BackendRoot)
    $settingsPath = Join-Path $BackendRoot 'SOOP_LIVE_SETTING.ini'
    if (-not (Test-Path -LiteralPath $settingsPath -PathType Leaf)) {
        throw 'SOOP LIVE 설정 파일을 찾을 수 없습니다.'
    }
    $config = @{}
    foreach ($rawLine in Get-Content -LiteralPath $settingsPath -Encoding UTF8) {
        $line = [string]$rawLine
        if ($line -match '^\s*([^#;][^=]*)=(.*)$') {
            $config[$matches[1].Trim()] = $matches[2].Trim()
        }
    }
    $securityModule = Join-Path $BackendRoot 'modules\SOOP.Security.ps1'
    if (-not (Test-Path -LiteralPath $securityModule -PathType Leaf)) {
        throw 'SOOP DPAPI 보안 모듈을 찾을 수 없습니다.'
    }
    if (-not (Get-Command Resolve-ProtectedConfigSecrets -ErrorAction SilentlyContinue)) {
        . $securityModule
    }
    $resolved = Resolve-ProtectedConfigSecrets -Config $config
    return $resolved
}

function Export-VodNetscapeCookies {
    param(
        [System.Net.CookieContainer]$CookieContainer,
        [string]$Destination
    )
    $knownUris = @(
        [Uri]'https://sooplive.com/',
        [Uri]'https://www.sooplive.com/',
        [Uri]'https://login.sooplive.com/',
        [Uri]'https://login.sooplive.com/app/',
        [Uri]'https://vod.sooplive.com/',
        [Uri]'https://vod.sooplive.com/player/',
        [Uri]'https://live.sooplive.com/',
        [Uri]'https://live.sooplive.com/api/',
        [Uri]'https://play.sooplive.com/'
    )
    $seen = @{}
    $lines = New-Object 'System.Collections.Generic.List[string]'
    $lines.Add('# Netscape HTTP Cookie File')
    $lines.Add('# Generated for one SOOP VOD job; do not reuse or share.')
    foreach ($uri in $knownUris) {
        foreach ($cookie in $CookieContainer.GetCookies($uri)) {
            $domain = [string]$cookie.Domain
            if ([string]::IsNullOrWhiteSpace($domain)) { $domain = $uri.Host }
            $normalizedDomain = $domain.TrimStart('.').ToLowerInvariant()
            if ($normalizedDomain -ne 'sooplive.com' -and -not $normalizedDomain.EndsWith('.sooplive.com')) { continue }
            $path = if ([string]::IsNullOrWhiteSpace([string]$cookie.Path)) { '/' } else { [string]$cookie.Path }
            $key = '{0}|{1}|{2}' -f $normalizedDomain, $path, $cookie.Name
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = $true
            $includeSubdomains = if ($domain.StartsWith('.')) { 'TRUE' } else { 'FALSE' }
            $secure = if ($cookie.Secure) { 'TRUE' } else { 'FALSE' }
            $expires = [int64]0
            if ($cookie.Expires -ne [DateTime]::MinValue) {
                $expires = [DateTimeOffset]::new($cookie.Expires.ToUniversalTime()).ToUnixTimeSeconds()
            }
            $name = ([string]$cookie.Name) -replace '[\t\r\n]', ''
            $value = ([string]$cookie.Value) -replace '[\t\r\n]', ''
            $lines.Add(("{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}" -f $domain, $includeSubdomains, $path, $secure, $expires, $name, $value))
        }
    }
    if ($lines.Count -le 2) { throw 'SOOP 로그인 Cookie를 발급받지 못했습니다.' }
    [System.IO.File]::WriteAllLines($Destination, $lines, [System.Text.UTF8Encoding]::new($false))
    return ($lines.Count - 2)
}

function New-VodSoopLoginCookie {
    param([string]$BackendRoot, [string]$Destination)
    $config = Read-VodLoginConfig -BackendRoot $BackendRoot
    $username = [string]$config['SOOP_USERNAME']
    $password = [string]$config['SOOP_PASSWORD']
    $client = $null
    try {
        if ([string]::IsNullOrWhiteSpace($username) -or [string]::IsNullOrWhiteSpace($password)) {
            throw '설정 탭에 SOOP 아이디와 비밀번호를 먼저 저장해 주세요.'
        }

        Add-Type -AssemblyName System.Net.Http -ErrorAction SilentlyContinue
        $container = New-Object System.Net.CookieContainer
        $handler = New-Object System.Net.Http.HttpClientHandler
        $handler.CookieContainer = $container
        $handler.UseCookies = $true
        $handler.UseProxy = $false
        $handler.AutomaticDecompression = [System.Net.DecompressionMethods]::GZip -bor [System.Net.DecompressionMethods]::Deflate
        $client = New-Object System.Net.Http.HttpClient($handler)
        $client.Timeout = [TimeSpan]::FromSeconds(15)
        $client.DefaultRequestHeaders.UserAgent.ParseAdd('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36')
        $loginData = @{
            szWork = 'login'; szType = 'json'; szUid = $username; szPassword = $password
            isSaveId = 'true'; isSavePw = 'false'; isSaveJoin = 'false'; isLoginRetain = 'Y'
        }
        $pairs = New-Object 'System.Collections.Generic.List[System.Collections.Generic.KeyValuePair[string,string]]'
        foreach ($key in $loginData.Keys) {
            $pairs.Add((New-Object 'System.Collections.Generic.KeyValuePair[string,string]' -ArgumentList ([string]$key), ([string]$loginData[$key])))
        }
        $form = New-Object System.Net.Http.FormUrlEncodedContent -ArgumentList (, $pairs)
        try {
            $request = New-Object System.Net.Http.HttpRequestMessage(
                [System.Net.Http.HttpMethod]::Post,
                [Uri]'https://login.sooplive.com/app/LoginAction.php'
            )
            $request.Headers.Referrer = [Uri]'https://www.sooplive.com/'
            $request.Content = $form
            try {
                $response = $client.SendAsync($request).GetAwaiter().GetResult()
                try {
                    $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                    $response.EnsureSuccessStatusCode() | Out-Null
                    $json = $text | ConvertFrom-Json
                    if ([int]$json.RESULT -ne 1) { throw "SOOP 로그인 실패 (RESULT=$($json.RESULT))" }
                }
                finally { if ($null -ne $response) { $response.Dispose() } }
            }
            finally { $request.Dispose() }
        }
        finally { $form.Dispose() }

        $verifyRequest = New-Object System.Net.Http.HttpRequestMessage(
            [System.Net.Http.HttpMethod]::Get,
            [Uri]'https://afevent2.sooplive.com/api/get_private_info.php'
        )
        $verifyRequest.Headers.Referrer = [Uri]'https://www.sooplive.com/'
        try {
            $verifyResponse = $client.SendAsync($verifyRequest).GetAwaiter().GetResult()
            try {
                $verifyText = $verifyResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                $verifyResponse.EnsureSuccessStatusCode() | Out-Null
                $verifyJson = $verifyText | ConvertFrom-Json
                if ([string]::IsNullOrWhiteSpace([string]$verifyJson.CHANNEL.LOGIN_ID)) { throw 'SOOP 로그인 검증에 실패했습니다.' }
            }
            finally { if ($null -ne $verifyResponse) { $verifyResponse.Dispose() } }
        }
        finally { $verifyRequest.Dispose() }

        $count = Export-VodNetscapeCookies -CookieContainer $container -Destination $Destination
        Write-VodEvent -Type 'auth_session_ready' -Message ("저장된 SOOP 로그인 세션 준비 완료 (Cookie {0}개)" -f $count)
    }
    finally {
        $password = $null
        $config['SOOP_PASSWORD'] = $null
        if ($null -ne $client) { $client.Dispose() }
    }
}

function Initialize-VodCookie {
    param($Request, [string]$JobDirectory, [string]$YtDlp, [string]$BackendRoot)
    $mode = ([string]$Request.CookieMode).ToUpperInvariant()
    $temporary = Join-Path $JobDirectory 'cookies.txt'
    if ($mode -eq 'SOOP_LOGIN') {
        New-VodSoopLoginCookie -BackendRoot $BackendRoot -Destination $temporary
    }
    elseif ($mode -eq 'FILE') {
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
    return [pscustomobject]@{ Path = $temporary; Mode = $mode; BackendRoot = $BackendRoot; JobDirectory = $JobDirectory; YtDlp = $YtDlp }
}

function Renew-VodBaseCookie {
    param($Request, $Cookie, [int]$Attempt)
    if ($Cookie.Mode -eq 'SOOP_LOGIN') {
        Write-VodEvent -Type 'auth_session_refreshing' -Message ("SOOP 로그인 세션 재발급 중 ({0}/{1})" -f $Attempt, [int]$Request.MaxRetries)
        New-VodSoopLoginCookie -BackendRoot $Cookie.BackendRoot -Destination $Cookie.Path
    }
    elseif ($Cookie.Mode -eq 'BROWSER') {
        Write-VodEvent -Type 'auth_session_refreshing' -Message ("브라우저 Cookie 다시 가져오는 중 ({0}/{1})" -f $Attempt, [int]$Request.MaxRetries)
        & $Cookie.YtDlp '--cookies-from-browser' ([string]$Request.BrowserName) '--cookies' $Cookie.Path '--skip-download' ([string]$Request.VodUrl) 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw '브라우저 Cookie 재발급에 실패했습니다.' }
    }
}

function Refresh-VodAuthorization {
    param($Request, [string]$CookieFile, [string]$StreamerId, [string]$Url, [int]$Attempt)
    Write-VodEvent -Type 'auth_refreshing' -Message ("구독 VOD 단기 인증 Cookie 발급 중 ({0}/{1})" -f $Attempt, [int]$Request.MaxRetries)
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
