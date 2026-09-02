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
    param([string]$BackendRoot, [string]$Destination, [string]$VodUrl)
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

        # A browser-backed cookie file has already visited the VOD player.
        # Do the same for the isolated credential login before exporting its
        # CookieContainer so player-scoped session cookies are not omitted.
        $vodUri = $null
        if (-not [Uri]::TryCreate($VodUrl, [UriKind]::Absolute, [ref]$vodUri) -or
            $vodUri.Scheme -ne 'https' -or
            -not $vodUri.Host.EndsWith('sooplive.com', [StringComparison]::OrdinalIgnoreCase)) {
            throw '로그인 세션을 준비할 VOD URL이 올바르지 않습니다.'
        }
        $vodRequest = New-Object System.Net.Http.HttpRequestMessage(
            [System.Net.Http.HttpMethod]::Get,
            $vodUri
        )
        $vodRequest.Headers.Referrer = [Uri]'https://vod.sooplive.com/'
        try {
            $vodResponse = $client.SendAsync($vodRequest).GetAwaiter().GetResult()
            try {
                $vodResponse.EnsureSuccessStatusCode() | Out-Null
                [void]$vodResponse.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
            }
            finally { if ($null -ne $vodResponse) { $vodResponse.Dispose() } }
        }
        finally { $vodRequest.Dispose() }

        $count = Export-VodNetscapeCookies -CookieContainer $container -Destination $Destination
        Write-VodEvent -Type 'auth_session_ready' -Message ("저장된 SOOP 로그인 세션 준비 완료 (Cookie {0}개)" -f $count)
    }
    finally {
        $password = $null
        $config['SOOP_PASSWORD'] = $null
        if ($null -ne $client) { $client.Dispose() }
    }
}

function Test-VodNetscapeCookieFile {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'VOD Cookie 파일이 생성되지 않았습니다.' }
    $dataLine = Get-Content -LiteralPath $Path -ErrorAction Stop |
        Where-Object {
            $line = ([string]$_).Trim()
            $line.Length -gt 0 -and (-not $line.StartsWith('#') -or $line.StartsWith('#HttpOnly_'))
        } |
        Select-Object -First 1
    $fields = @(([string]$dataLine) -split "`t", 7)
    if ([string]::IsNullOrWhiteSpace([string]$dataLine) -or $fields.Count -ne 7) {
        throw 'Cookie 파일이 Netscape 형식이 아닙니다. 브라우저 확장이나 yt-dlp로 내보낸 cookies.txt를 사용해 주세요.'
    }
}

function Get-VodCookieCapabilities {
    param([string]$Path)
    $names = @{}
    foreach ($line in [System.IO.File]::ReadAllLines($Path, [System.Text.Encoding]::UTF8)) {
        if ([string]::IsNullOrWhiteSpace($line) -or ($line.StartsWith('#') -and -not $line.StartsWith('#HttpOnly_'))) { continue }
        $fields = @($line -split "`t", 7)
        if ($fields.Count -eq 7) { $names[$fields[5]] = $true }
    }
    $hasSigned = $names.ContainsKey('CloudFront-Key-Pair-Id') -and $names.ContainsKey('CloudFront-Policy') -and $names.ContainsKey('CloudFront-Signature')
    $hasLogin = @('AuthTicket', 'BbsTicket', 'UserTicket', 'RDB', 'PdboxTicket') | Where-Object { $names.ContainsKey($_) } | Select-Object -First 1
    return [pscustomobject]@{ HasCloudFrontAuthorization = [bool]$hasSigned; HasSoopLoginCookies = ($null -ne $hasLogin) }
}

function Get-VodCloudFrontCookieValues {
    param([string]$Path)
    $requiredNames = @('CloudFront-Key-Pair-Id', 'CloudFront-Policy', 'CloudFront-Signature')
    $values = @{}
    foreach ($line in [System.IO.File]::ReadAllLines($Path, [System.Text.Encoding]::UTF8)) {
        if ([string]::IsNullOrWhiteSpace($line) -or ($line.StartsWith('#') -and -not $line.StartsWith('#HttpOnly_'))) { continue }
        $fields = @($line -split "`t", 7)
        if ($fields.Count -eq 7 -and $requiredNames -contains $fields[5]) { $values[$fields[5]] = [string]$fields[6] }
    }
    foreach ($name in $requiredNames) {
        if (-not $values.ContainsKey($name) -or [string]::IsNullOrWhiteSpace([string]$values[$name])) { return $null }
    }
    return $values
}

function Get-VodCloudFrontPolicyResource {
    param([string]$CookieFile)
    $values = Get-VodCloudFrontCookieValues -Path $CookieFile
    if ($null -eq $values) { return '' }
    try {
        # CloudFront uses its URL-safe substitutions: +=-, ==_, /=~.
        $encoded = ([string]$values['CloudFront-Policy']).Replace('-', '+').Replace('_', '=').Replace('~', '/')
        while (($encoded.Length % 4) -ne 0) { $encoded += '=' }
        $policyText = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded))
        $policy = $policyText | ConvertFrom-Json
        $statement = @($policy.Statement) | Select-Object -First 1
        $resource = [string]$statement.Resource
        $resourceUri = $null
        if ([Uri]::TryCreate($resource, [UriKind]::Absolute, [ref]$resourceUri) -and $resourceUri.Scheme -eq 'https') { return $resource }
    }
    catch { }
    return ''
}

function New-VodCloudFrontCurlConfig {
    param([string]$CookieFile, [string]$Destination)
    $values = Get-VodCloudFrontCookieValues -Path $CookieFile
    if ($null -eq $values) { throw 'CloudFront 인증 Cookie 3종을 읽을 수 없습니다.' }
    $header = 'Cookie: CloudFront-Key-Pair-Id={0}; CloudFront-Policy={1}; CloudFront-Signature={2}' -f
        $values['CloudFront-Key-Pair-Id'], $values['CloudFront-Policy'], $values['CloudFront-Signature']
    $escaped = $header.Replace('\', '\\').Replace('"', '\"')
    [System.IO.File]::WriteAllText($Destination, ('header = "{0}"' -f $escaped), [System.Text.UTF8Encoding]::new($false))
}

function Write-VodCloudFrontCookieValues {
    param($Values, [string]$CookieFile, [string]$ResourceUrl)
    $resourceUri = $null
    if ($null -eq $Values -or
        -not [Uri]::TryCreate($ResourceUrl, [UriKind]::Absolute, [ref]$resourceUri) -or
        $resourceUri.Scheme -ne 'https') { return 0 }
    foreach ($name in @('CloudFront-Key-Pair-Id', 'CloudFront-Policy', 'CloudFront-Signature')) {
        if ([string]::IsNullOrWhiteSpace([string]$Values[$name])) { return 0 }
    }
    Remove-VodCloudFrontCookies -CookieFile $CookieFile
    $lines = New-Object 'System.Collections.Generic.List[string]'
    foreach ($line in [System.IO.File]::ReadAllLines($CookieFile, [System.Text.Encoding]::UTF8)) { [void]$lines.Add($line) }
    foreach ($name in @('CloudFront-Key-Pair-Id', 'CloudFront-Policy', 'CloudFront-Signature')) {
        [void]$lines.Add(("{0}`tFALSE`t/`tTRUE`t0`t{1}`t{2}" -f $resourceUri.Host.ToLowerInvariant(), $name, $Values[$name]))
    }
    [System.IO.File]::WriteAllLines($CookieFile, [string[]]$lines, [System.Text.UTF8Encoding]::new($false))
    return 3
}

function Import-VodCloudFrontSetCookieHeaders {
    param([string]$HeaderFile, [string]$CookieFile, [string]$ResourceUrl)
    if (-not (Test-Path -LiteralPath $HeaderFile -PathType Leaf)) { return 0 }
    $values = @{}
    foreach ($line in [System.IO.File]::ReadAllLines($HeaderFile, [System.Text.Encoding]::UTF8)) {
        $match = [regex]::Match([string]$line, '(?i)^\s*Set-Cookie:\s*(?<name>CloudFront-(?:Key-Pair-Id|Policy|Signature))=(?<value>[^;\r\n]+)')
        if ($match.Success) { $values[$match.Groups['name'].Value] = $match.Groups['value'].Value.Trim('"') }
    }
    if ($values.Count -lt 3) { return 0 }
    return Write-VodCloudFrontCookieValues -Values $values -CookieFile $CookieFile -ResourceUrl $ResourceUrl
}

function Import-VodCloudFrontJsonResponse {
    param([string]$JsonText, [string]$CookieFile, [string]$ResourceUrl)
    if ([string]::IsNullOrWhiteSpace($JsonText)) { return 0 }
    $patterns = [ordered]@{
        'CloudFront-Key-Pair-Id' = '"(?:CloudFront[-_])?(?:Key[-_]?Pair[-_]?I[Dd]|Key)"\s*:\s*"(?<value>(?:\\.|[^"\\])*)"'
        'CloudFront-Policy' = '"(?:CloudFront[-_])?Policy"\s*:\s*"(?<value>(?:\\.|[^"\\])*)"'
        'CloudFront-Signature' = '"(?:CloudFront[-_])?Signature"\s*:\s*"(?<value>(?:\\.|[^"\\])*)"'
    }
    $values = @{}
    foreach ($name in $patterns.Keys) {
        $match = [regex]::Match($JsonText, [string]$patterns[$name], [Text.RegularExpressions.RegexOptions]::IgnoreCase)
        if (-not $match.Success) { return 0 }
        try { $values[$name] = ('"' + $match.Groups['value'].Value + '"') | ConvertFrom-Json }
        catch { return 0 }
    }
    return Write-VodCloudFrontCookieValues -Values $values -CookieFile $CookieFile -ResourceUrl $ResourceUrl
}

function Repair-VodCloudFrontCookieScope {
    param([string]$CookieFile, [string]$ResourceUrl)
    $resourceUri = $null
    if (-not [Uri]::TryCreate($ResourceUrl, [UriKind]::Absolute, [ref]$resourceUri) -or $resourceUri.Scheme -ne 'https') {
        throw 'CloudFront Cookie 범위를 설정할 manifest URL이 올바르지 않습니다.'
    }
    $lines = @([System.IO.File]::ReadAllLines($CookieFile, [System.Text.Encoding]::UTF8))
    $signedNames = @('CloudFront-Policy', 'CloudFront-Signature', 'CloudFront-Key-Pair-Id', 'CloudFront-Expires')
    $requiredNames = @('CloudFront-Key-Pair-Id', 'CloudFront-Policy', 'CloudFront-Signature')
    $latest = @{}
    $preserved = New-Object 'System.Collections.Generic.List[string]'
    foreach ($line in $lines) {
        if ([string]::IsNullOrWhiteSpace($line) -or ($line.StartsWith('#') -and -not $line.StartsWith('#HttpOnly_'))) {
            $preserved.Add($line)
            continue
        }
        $fields = @($line -split "`t", 7)
        if ($fields.Count -eq 7 -and $signedNames -contains $fields[5]) {
            # curl sends every domain-matching cookie with the same name. Keeping
            # an old host alias alongside a newly issued parent-domain cookie can
            # therefore send two Policy/Signature values and CloudFront rejects
            # the request. Retain only the last value curl wrote for each name.
            $latest[$fields[5]] = $fields
            continue
        }
        $preserved.Add($line)
    }
    foreach ($name in $requiredNames) {
        if (-not $latest.ContainsKey($name) -or [string]::IsNullOrWhiteSpace([string]$latest[$name][6])) {
            throw "private_auth 응답에 $name Cookie가 없습니다."
        }
    }
    $manifestHost = $resourceUri.Host.ToLowerInvariant()
    foreach ($name in $signedNames) {
        if (-not $latest.ContainsKey($name)) { continue }
        $fields = $latest[$name]
        $preserved.Add(("{0}`tFALSE`t/`tTRUE`t{1}`t{2}`t{3}" -f $manifestHost, $fields[4], $name, $fields[6]))
    }
    [System.IO.File]::WriteAllLines($CookieFile, [string[]]$preserved, [System.Text.UTF8Encoding]::new($false))
    return @($latest.Keys).Count
}

function Remove-VodCloudFrontCookies {
    param([string]$CookieFile)
    $signedNames = @('CloudFront-Policy', 'CloudFront-Signature', 'CloudFront-Key-Pair-Id', 'CloudFront-Expires')
    $preserved = @([System.IO.File]::ReadAllLines($CookieFile, [System.Text.Encoding]::UTF8) | Where-Object {
        $line = [string]$_
        $fields = @($line -split "`t", 7)
        $fields.Count -ne 7 -or $signedNames -notcontains $fields[5]
    })
    [System.IO.File]::WriteAllLines($CookieFile, [string[]]$preserved, [System.Text.UTF8Encoding]::new($false))
}

function Get-VodManifestQualityOptions {
    param([string]$Path)
    $result = @('best|최고 화질 (자동)')
    $heights = @([System.IO.File]::ReadAllLines($Path, [System.Text.Encoding]::UTF8) | ForEach-Object {
        $match = [regex]::Match([string]$_, 'RESOLUTION=\d+x(?<height>\d+)')
        if ($match.Success) { [int]$match.Groups['height'].Value }
    } | Sort-Object -Descending -Unique)
    foreach ($height in $heights) { $result += "best[height<=$height]|${height}p" }
    return @($result)
}

function Test-VodManifestAuthorization {
    param($Request, [string]$CookieFile, [string]$Url, [string]$ProbeFile)
    Remove-Item -LiteralPath $ProbeFile -Force -ErrorAction SilentlyContinue
    $curlConfig = $ProbeFile + '.curl-config'
    New-VodCloudFrontCurlConfig -CookieFile $CookieFile -Destination $curlConfig
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $statusOutput = & curl.exe '-sS' '-L' '--config' $curlConfig '-A' 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36' '-e' ([string]$Request.VodUrl) '-H' 'Origin: https://vod.sooplive.com' '-o' $ProbeFile '-w' '%{http_code}' $Url 2>&1
        $probeExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
        Remove-Item -LiteralPath $curlConfig -Force -ErrorAction SilentlyContinue
    }
    $statusText = ($statusOutput -join ' ').Trim()
    $statusMatch = [regex]::Match($statusText, '(\d{3})\s*$')
    $httpStatus = if ($statusMatch.Success) { [int]$statusMatch.Groups[1].Value } else { 0 }
    $script:LastVodQualities = @('best|최고 화질 (자동)')
    if ($probeExitCode -eq 0 -and $httpStatus -ge 200 -and $httpStatus -lt 300) {
        $script:LastVodQualities = @(Get-VodManifestQualityOptions -Path $ProbeFile)
        Remove-Item -LiteralPath $ProbeFile -Force -ErrorAction SilentlyContinue
        return $true
    }
    Remove-Item -LiteralPath $ProbeFile -Force -ErrorAction SilentlyContinue
    $manifestHost = ([Uri]$Url).Host
    $script:LastVodAuthError = "manifest authorization check failed: HTTP $httpStatus, curl $probeExitCode, host=$manifestHost"
    return $false
}

function Initialize-VodCookie {
    param($Request, [string]$JobDirectory, [string]$YtDlp, [string]$BackendRoot)
    $mode = ([string]$Request.CookieMode).ToUpperInvariant()
    $temporary = Join-Path $JobDirectory 'cookies.txt'
    if ($mode -eq 'SOOP_LOGIN') {
        New-VodSoopLoginCookie -BackendRoot $BackendRoot -Destination $temporary -VodUrl ([string]$Request.VodUrl)
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
    Test-VodNetscapeCookieFile -Path $temporary
    if ($mode -eq 'SOOP_LOGIN') {
        # Visiting the player can leave an incomplete or page-scoped signed
        # triplet in the login CookieContainer. Stored-login mode must always
        # obtain a fresh authorization for the extracted PART URL instead of
        # mistaking those incidental cookies for a ready CDN session.
        Remove-VodCloudFrontCookies -CookieFile $temporary
    }
    $capabilities = Get-VodCookieCapabilities -Path $temporary
    $policyResource = ''
    if ($capabilities.HasCloudFrontAuthorization) {
        # FILE mode needs the CDN scope before yt-dlp metadata extraction. The
        # signed policy carries that resource even when yt-dlp cannot yet expose
        # entry.url because opening the protected m3u8 would return 403.
        $policyResource = Get-VodCloudFrontPolicyResource -CookieFile $temporary
        if (-not [string]::IsNullOrWhiteSpace($policyResource)) {
            [void](Repair-VodCloudFrontCookieScope -CookieFile $temporary -ResourceUrl $policyResource)
        }
    }
    return [pscustomobject]@{ Path = $temporary; Mode = $mode; BackendRoot = $BackendRoot; JobDirectory = $JobDirectory; YtDlp = $YtDlp; HasCloudFrontAuthorization = $capabilities.HasCloudFrontAuthorization; HasSoopLoginCookies = $capabilities.HasSoopLoginCookies; PolicyResource = $policyResource }
}

function Renew-VodBaseCookie {
    param($Request, $Cookie, [int]$Attempt)
    if ($Cookie.Mode -eq 'SOOP_LOGIN') {
        Write-VodEvent -Type 'auth_session_refreshing' -Message ("SOOP 로그인 세션 재발급 중 ({0}/{1})" -f $Attempt, [int]$Request.MaxRetries)
        New-VodSoopLoginCookie -BackendRoot $Cookie.BackendRoot -Destination $Cookie.Path -VodUrl ([string]$Request.VodUrl)
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
    $script:LastVodAuthError = ''
    # Never send an expired CloudFront triplet back to private_auth.php. More
    # importantly, this guarantees that curl's output jar contains only the
    # newly issued triplet, so an older exact-host alias cannot win by ordering.
    Remove-VodCloudFrontCookies -CookieFile $CookieFile
    $headerFile = Join-Path (Split-Path -Parent $CookieFile) ("private-auth-{0:D2}.headers" -f $Attempt)
    Remove-Item -LiteralPath $headerFile -Force -ErrorAction SilentlyContinue
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        try {
            $ErrorActionPreference = 'Continue'
            $response = & curl.exe '-sS' '-L' '-D' $headerFile '-b' $CookieFile '-c' $CookieFile '-A' 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36' '-e' ([string]$Request.VodUrl) '-H' 'Origin: https://vod.sooplive.com' '-H' 'Accept: application/json, text/plain, */*' '--data-urlencode' 'type=vod' '--data-urlencode' ("strm_id=$StreamerId") '--data-urlencode' ("title_no=" + ([regex]::Match([string]$Request.VodUrl, '/player/(\d+)').Groups[1].Value)) '--data-urlencode' ("url=$Url") 'https://live.sooplive.com/api/private_auth.php' 2>&1
            $curlExitCode = $LASTEXITCODE
        }
        finally { $ErrorActionPreference = $previousErrorActionPreference }
        $responseText = ($response -join ' ').Trim()
        $success = ($curlExitCode -eq 0 -and $responseText -match '"result"\s*:\s*1')
        if (-not $success) {
            $detail = Get-RedactedVodText -Text $responseText
            if ($detail.Length -gt 300) { $detail = $detail.Substring(0, 300) }
            if ([string]::IsNullOrWhiteSpace($detail)) { $detail = "curl exit code $curlExitCode" }
            $script:LastVodAuthError = $detail
        }
        if ($success) {
            $imported = Import-VodCloudFrontSetCookieHeaders -HeaderFile $headerFile -CookieFile $CookieFile -ResourceUrl $Url
            if ($imported -lt 3) {
                $imported = Import-VodCloudFrontJsonResponse -JsonText $responseText -CookieFile $CookieFile -ResourceUrl $Url
            }
            $cloudFrontValues = Get-VodCloudFrontCookieValues -Path $CookieFile
            if ($null -eq $cloudFrontValues) {
                $success = $false
                $script:LastVodAuthError = 'private_auth 성공 응답에 CloudFront Cookie 3종이 없습니다.'
            }
            else {
                Test-VodNetscapeCookieFile -Path $CookieFile
                [void](Repair-VodCloudFrontCookieScope -CookieFile $CookieFile -ResourceUrl $Url)
            }
        }
        return $success
    }
    finally { Remove-Item -LiteralPath $headerFile -Force -ErrorAction SilentlyContinue }
}

function Remove-VodTemporarySecrets {
    param([string]$JobDirectory)
    if ([string]::IsNullOrWhiteSpace($JobDirectory)) { return }
    foreach ($name in @('cookies.txt', 'concat.txt')) {
        $path = Join-Path $JobDirectory $name
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue }
    }
    Get-ChildItem -LiteralPath $JobDirectory -Filter 'manifest-probe-*' -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}
