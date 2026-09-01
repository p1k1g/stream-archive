# SOOP LIVE backend module - dot-sourced by SOOP_LIVE.ps1

function Test-WorkerSoopAuthExpired {
    param([string]$Text)

    if ([string]::IsNullOrWhiteSpace($Text)) {
        return $false
    }

    return ($Text -match '(?i)(AUTH[_ -]?(EXPIRED|REQUIRED)|LOGIN[_ -]?REQUIRED|SOOP[_ -]?AUTH|COOKIE[_ -]?(EXPIRED|REQUIRED)|AGE[_ -]?RESTRICT|ADULT[_ -]?AUTH|로그인|인증.*만료)')
}

function Get-CompactExternalError {
    param([string]$Text,[int]$MaxLength = 300)
    if ([string]::IsNullOrWhiteSpace($Text)) { return "empty response" }
    $value = $Text -replace '(?is)<[^>]+>', ' '
    $value = $value -replace '\s+', ' '
    $value = $value.Trim()
    if ($value.Length -gt $MaxLength) { $value = $value.Substring(0,$MaxLength) + "..." }
    return $value
}

function Reset-WorkerCircuit {
    $script:WorkerFailureCycles = 0
    $script:WorkerCircuitLevel = 0
    $script:WorkerCircuitOpenUntil = [DateTime]::MinValue
}

function Open-WorkerCircuit {
    $script:WorkerFailureCycles++
    if ($script:WorkerFailureCycles -lt 2) { return }
    $cooldowns = @(30,60,120,300)
    $index = [Math]::Min($script:WorkerCircuitLevel,$cooldowns.Count - 1)
    $seconds = $cooldowns[$index]
    $script:WorkerCircuitLevel = [Math]::Min($script:WorkerCircuitLevel + 1,$cooldowns.Count - 1)
    $script:WorkerCircuitOpenUntil = (Get-Date).AddSeconds($seconds)
    Write-LogMessage "WORKER CIRCUIT OPEN cooldown=${seconds}s until=$($script:WorkerCircuitOpenUntil.ToString('o'))" -Level "WARN"
}

function Get-ChannelId {
    param([string]$AccountOrUrl)

    $value = ([string]$AccountOrUrl).Trim()

    if ([string]::IsNullOrWhiteSpace($value)) {
        return $null
    }

    if (
        $value -match '^https?://play\.(?:sooplive\.com|sooplive\.co\.kr|afreecatv\.com)/(?<id>\w+)'
    ) {
        return $matches["id"]
    }

    if ($value -match '^[A-Za-z0-9_]+$') {
        return $value
    }

    return $null
}

function Get-SoopChannelList {
    param([string]$Path)

    $channels = [System.Collections.Generic.List[object]]::new()

    try {
        $lines = Get-Content $Path -Encoding UTF8 -ErrorAction Stop

        foreach ($raw in $lines) {
            $line = $raw.Trim()

            if (
                [string]::IsNullOrWhiteSpace($line) -or
                $line.StartsWith("#")
            ) {
                continue
            }

            $parts = $line.Split("|")

            if ($parts.Count -lt 3) {
                Write-Host "[WARN] Invalid channel line: $line"
                continue
            }

            $enabledText = $parts[0].Trim().ToUpperInvariant()
            $name = $parts[1].Trim()
            $accountValue = $parts[2].Trim()
            $outDir = ""

            if ($parts.Count -ge 4) {
                $outDir = $parts[3].Trim()
            }

            $channelId = Get-ChannelId $accountValue

            if ([string]::IsNullOrWhiteSpace($channelId)) {
                Write-Host "[WARN] Invalid SOOP ACCOUNT/URL: $accountValue"
                continue
            }

            # NAME remains in the legacy channel-file schema for compatibility,
            # but account ID is the stable identity. Raw account-only rows use
            # the account as a safe display fallback until a nickname is known.
            if ([string]::IsNullOrWhiteSpace($name)) {
                $name = $channelId
            }

            $channelUrl = "https://play.sooplive.com/$channelId"

            $channels.Add([PSCustomObject]@{
                Enabled   = ($enabledText -eq "Y")
                Name      = $name
                Account   = $channelId
                Url       = $channelUrl
                OutDir    = $outDir
                ChannelId = $channelId
            })
        }

        return $channels.ToArray()
    }
    catch {
        Write-Host "[WARN] Channel list read failed; previous list will be kept."
        return $null
    }
}

function New-DirectHttpClient {
    $script:SoopCookieContainer = New-Object System.Net.CookieContainer
    $handler = New-Object System.Net.Http.HttpClientHandler

    $handler.UseProxy = $false
    $handler.CookieContainer = $script:SoopCookieContainer
    $handler.UseCookies = $true
    $handler.AutomaticDecompression = `
        [System.Net.DecompressionMethods]::GZip -bor `
        [System.Net.DecompressionMethods]::Deflate

    $client = New-Object System.Net.Http.HttpClient($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(15)

    $client.DefaultRequestHeaders.UserAgent.ParseAdd(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36"
    )

    return $client
}

$HttpClient = New-DirectHttpClient

function Invoke-DirectGetText {
    param(
        [string]$Url,
        [string]$Referer = ""
    )

    $request = New-Object System.Net.Http.HttpRequestMessage(
        [System.Net.Http.HttpMethod]::Get,
        $Url
    )

    if (-not [string]::IsNullOrWhiteSpace($Referer)) {
        $request.Headers.Referrer = [Uri]$Referer
    }

    try {
        $response = $HttpClient.SendAsync($request).GetAwaiter().GetResult()
        $response.EnsureSuccessStatusCode() | Out-Null
        return $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    }
    finally {
        $request.Dispose()
        if ($response) {
            $response.Dispose()
        }
    }
}

function Invoke-DirectPostForm {
    param(
        [string]$Url,
        [string]$Referer,
        [hashtable]$Data
    )

    $pairs = New-Object 'System.Collections.Generic.List[System.Collections.Generic.KeyValuePair[string,string]]'

    foreach ($key in $Data.Keys) {
        $value = ""

        if ($null -ne $Data[$key]) {
            $value = [string]$Data[$key]
        }

        $pairs.Add(
            (New-Object 'System.Collections.Generic.KeyValuePair[string,string]' `
                -ArgumentList ([string]$key), $value)
        )
    }

    $content = New-Object System.Net.Http.FormUrlEncodedContent `
        -ArgumentList (, $pairs)

    try {
        $request = New-Object System.Net.Http.HttpRequestMessage(
            [System.Net.Http.HttpMethod]::Post,
            [Uri]$Url
        )

        try {
            $request.Content = $content

            if (-not [string]::IsNullOrWhiteSpace($Referer)) {
                $request.Headers.Referrer = [Uri]$Referer
            }

            $response = $HttpClient.SendAsync($request).GetAwaiter().GetResult()

            try {
                $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()

                if (-not $response.IsSuccessStatusCode) {
                    throw "HTTP $([int]$response.StatusCode) $($response.ReasonPhrase): $text"
                }

                return $text
            }
            finally {
                $response.Dispose()
            }
        }
        finally {
            $request.Dispose()
        }
    }
    finally {
        $content.Dispose()
    }
}

function Get-QualityScore {
    param($Preset)

    $label = [string]$Preset.label
    $name  = [string]$Preset.name

    if (
        $label -match '(?i)original|source|원본' -or
        $name -match '(?i)original|source'
    ) {
        return 999999
    }

    if ($label -match '(\d{3,4})') {
        return [int]$matches[1]
    }

    if ($name -match '(\d{3,4})') {
        return [int]$matches[1]
    }

    if ($name -eq "auto") {
        return 900000
    }

    return 0
}

function Select-InitialQuality {
    param(
        $ViewPreset,
        [string]$RequestedQuality
    )

    $presets = @($ViewPreset)

    if ($presets.Count -eq 0) {
        return $RequestedQuality
    }

    # Prefer auto for the initial master/multivariant HLS URL.
    if ($RequestedQuality -eq "auto") {
        $auto = $presets |
            Where-Object { $_.name -eq "auto" } |
            Select-Object -First 1

        if ($auto) {
            return [string]$auto.name
        }
    }

    # Explicit quality name/label.
    $match = $presets |
        Where-Object {
            $_.name -eq $RequestedQuality -or
            $_.label -eq $RequestedQuality
        } |
        Select-Object -First 1

    if ($match) {
        return [string]$match.name
    }

    # Fallback to highest advertised preset.
    $best = $presets |
        Where-Object { $_.name -ne "auto" } |
        Sort-Object `
            @{ Expression = { Get-QualityScore $_ }; Descending = $true } |
        Select-Object -First 1

    if ($best) {
        return [string]$best.name
    }

    return "auto"
}

function Invoke-StreamlinkCredentialPurge {
    param(
        [string]$StreamlinkExe,
        [string]$ChannelUrl,
        [string]$Username,
        [string]$Password
    )

    if (
        [string]::IsNullOrWhiteSpace($Username) -or
        [string]::IsNullOrWhiteSpace($Password)
    ) {
        return
    }

    Write-Host "[AUTH] Streamlink SOOP credential cache purge / re-auth request"

    $purgeArgs = @(
        $ChannelUrl,
        "best",
        "--soop-purge-credentials",
        "--soop-username", $Username,
        "--soop-password", $Password,
        "--loglevel", "error",
        "--stream-url"
    )

    try {
        & $StreamlinkExe @purgeArgs 2>$null | Out-Null
    }
    catch {
        # Offline channel / no stream is acceptable for this bootstrap step.
    }
}

function Initialize-SoopLogin {
    param(
        [string]$Username,
        [string]$Password
    )

    if (
        [string]::IsNullOrWhiteSpace($Username) -or
        [string]::IsNullOrWhiteSpace($Password)
    ) {
        $script:SoopAuthCookies = @{}
        Write-Host "[AUTH] SOOP login disabled (username/password not configured)"
        return $false
    }

    $loginText = Invoke-DirectPostForm `
        -Url "https://login.sooplive.com/app/LoginAction.php" `
        -Referer "https://www.sooplive.com/" `
        -Data @{
            szWork        = "login"
            szType        = "json"
            szUid         = $Username
            szPassword    = $Password
            isSaveId      = "true"
            isSavePw      = "false"
            isSaveJoin    = "false"
            isLoginRetain = "Y"
        }

    $loginJson = $loginText | ConvertFrom-Json

    if ([int]$loginJson.RESULT -ne 1) {
        throw "SOOP 로그인 실패 (RESULT=$($loginJson.RESULT))"
    }

    $authText = Invoke-DirectGetText `
        -Url "https://afevent2.sooplive.com/api/get_private_info.php" `
        -Referer "https://www.sooplive.com/"

    $authJson = $authText | ConvertFrom-Json
    $loginId = [string]$authJson.CHANNEL.LOGIN_ID

    if ([string]::IsNullOrWhiteSpace($loginId)) {
        throw "SOOP 로그인 검증 실패"
    }

    $script:SoopAuthCookies = Get-SoopWorkerCookies

    Write-Host "[AUTH] SOOP login OK : $loginId"
    Write-Host "[AUTH] Worker cookie set : $($script:SoopAuthCookies.Keys.Count) item(s)"

    return $true
}

function Get-SoopWorkerCookies {
    # Forward only auth-related SOOP cookies to the user's Worker.
    $allow = @(
        "AuthTicket",
        "BbsTicket",
        "UserTicket",
        "BbsSaveTicket",
        "RDB",
        "PdboxTicket",
        "PdboxBbs",
        "PdboxUser",
        "PdboxSaveTicket"
    )

    $result = @{}
    $uris = @(
        [Uri]"https://sooplive.com/",
        [Uri]"https://www.sooplive.com/",
        [Uri]"https://play.sooplive.com/",
        [Uri]"https://live.sooplive.com/"
    )

    foreach ($uri in $uris) {
        foreach ($cookie in $script:SoopCookieContainer.GetCookies($uri)) {
            if ($allow -contains $cookie.Name) {
                $result[$cookie.Name] = $cookie.Value
            }
        }
    }

    return $result
}

function ConvertTo-CookieHeader {
    param([hashtable]$Cookies)

    if ($null -eq $Cookies -or $Cookies.Count -eq 0) {
        return ""
    }

    return (
        $Cookies.GetEnumerator() |
        Sort-Object Name |
        ForEach-Object { "$($_.Name)=$($_.Value)" }
    ) -join "; "
}

function Get-SoopLiveInfo {
    param($Channel)

    $channelUrl = $Channel.Url
    $channelId = $Channel.ChannelId

    try {
        # 1) Channel page: DIRECT
        $html = Invoke-DirectGetText `
            -Url $channelUrl `
            -Referer "https://play.sooplive.com/"

        if ($html -notmatch 'window\.nBroadNo\s*=\s*(?<bno>\d+);') {
            return [PSCustomObject]@{
                IsLive = $false
                Error  = $null
            }
        }

        $bno = $matches["bno"]

        # 2) Live status API: DIRECT
        $apiUrl = "https://live.sooplive.com/afreeca/player_live_api.php"

        $liveText = Invoke-DirectPostForm `
            -Url $apiUrl `
            -Referer $channelUrl `
            -Data @{
                from_api    = "0"
                mode        = "landing"
                player_type = "html5"
                stream_type = "common"
                type        = "live"
                bid         = $channelId
                bno         = $bno
                pwd         = ""
            }

        $liveJson = $liveText | ConvertFrom-Json
        $ch = $liveJson.CHANNEL

        if ($null -eq $ch) {
            return [PSCustomObject]@{
                IsLive       = $false
                AuthRequired = $false
                Error        = $null
            }
        }

        if ([int]$ch.RESULT -eq -6) {
            return [PSCustomObject]@{
                IsLive       = $false
                AuthRequired = $true
                Error        = $null
            }
        }

        if (
            [int]$ch.RESULT -ne 1 -or
            [string]::IsNullOrWhiteSpace([string]$ch.BNO) -or
            [string]::IsNullOrWhiteSpace([string]$ch.RMD)
        ) {
            return [PSCustomObject]@{
                IsLive       = $false
                AuthRequired = $false
                Error        = $null
            }
        }

        return [PSCustomObject]@{
            IsLive     = $true
            Bno        = [string]$ch.BNO
            BjNick     = [string]$ch.BJNICK
            Title      = [string]$ch.TITLE
            Rmd        = [string]$ch.RMD
            Cdn        = [string]$ch.CDN
            Bpwd       = [string]$ch.BPWD
            ViewPreset   = @($ch.VIEWPRESET)
            AuthRequired = $false
            Error        = $null
        }
    }
    catch {
        return [PSCustomObject]@{
            IsLive = $false
            Error  = $_.Exception.Message
        }
    }
}

function Get-CloudflarePlaylistUrl {
    param(
        $Channel,
        $LiveInfo,
        [string]$MasterQuality,
        [string]$WorkerUrl,
        [string]$WorkerApiKey,
        [hashtable]$AuthCookies,
        [int]$MaxRetry = 3
    )

    $now = Get-Date
    if ($now -lt $script:WorkerCircuitOpenUntil) {
        $seconds = [Math]::Max(1,[Math]::Ceiling(($script:WorkerCircuitOpenUntil - $now).TotalSeconds))
        throw ("[WORKER_CIRCUIT_OPEN] seconds={0} until={1}" -f $seconds,$script:WorkerCircuitOpenUntil.ToString("o"))
    }

    if ([string]::IsNullOrWhiteSpace($WorkerUrl)) {
        throw "CLOUDFLARE_WORKER_URL 설정이 없습니다."
    }

    if ([string]::IsNullOrWhiteSpace($WorkerApiKey)) {
        throw "CLOUDFLARE_API_KEY 설정이 없습니다."
    }

    if ($MaxRetry -lt 1) {
        $MaxRetry = 1
    }

    # Worker always requests the SOOP master HLS playlist.
    # Final variant selection is handled locally by Streamlink QUALITY=best/etc.
    $quality = "master"

    $backoff = @(2, 5, 10)
    $lastError = "Unknown Worker error"

    for ($attempt = 1; $attempt -le $MaxRetry; $attempt++) {
        $cookieHeader = ConvertTo-CookieHeader -Cookies $AuthCookies

        $workerBody = @{
            # Exact fields consumed by worker.js
            account            = [string]$Channel.ChannelId
            bno                = [string]$LiveInfo.Bno
            rmd                = [string]$LiveInfo.Rmd
            quality            = "master"
            cq                 = "sd"
            password           = [string]$LiveInfo.Bpwd
            cookie             = $cookieHeader

            # Compatibility fields kept for older Worker revisions
            bid                = [string]$Channel.ChannelId
            bpwd               = [string]$LiveInfo.Bpwd
            channel_url        = [string]$Channel.Url
            cookies            = $AuthCookies
            soop_cookies       = $AuthCookies
            soop_cookie_header = $cookieHeader
        } | ConvertTo-Json -Compress -Depth 5

        if ($attempt -eq 1) {
            Write-LogMessage (
                "Worker request channel={0} account={1} bno={2} quality={3} authCookieCount={4}" -f `
                $Channel.Name,
                $Channel.ChannelId,
                $LiveInfo.Bno,
                $quality,
                $(if ($null -eq $AuthCookies) { 0 } else { $AuthCookies.Count })
            )
        }

        $statusMarker = "__SOOP_HTTP_STATUS__:"
        $tempBodyFile = Join-Path `
            ([System.IO.Path]::GetTempPath()) `
            ("soop_worker_{0}_{1}.json" -f $PID, [Guid]::NewGuid().ToString("N"))

        try {
            # Do not pass JSON directly as a native command-line argument.
            # Windows PowerShell 5.1/curl.exe quoting can strip embedded quotes
            # and make request.json() fail with "invalid_json".
            [System.IO.File]::WriteAllText(
                $tempBodyFile,
                $workerBody,
                (New-Object System.Text.UTF8Encoding($false))
            )

            $curlArgs = @(
                "--silent",
                "--show-error",
                "--location",
                "--request", "POST",
                "--header", "X-API-Key: $WorkerApiKey",
                "--header", "Content-Type: application/json",
                "--data-binary", "@$tempBodyFile",
                "--write-out", "`n$statusMarker%{http_code}",
                $WorkerUrl
            )

            $curlOutput = & curl.exe @curlArgs 2>&1
            $curlExitCode = $LASTEXITCODE
            $curlText = ($curlOutput -join [Environment]::NewLine).Trim()
        }
        finally {
            try {
                Remove-Item -LiteralPath $tempBodyFile -Force -ErrorAction SilentlyContinue
            }
            catch {}
        }

        $httpStatus = 0
        $responseBody = $curlText

        $markerIndex = $curlText.LastIndexOf($statusMarker)

        if ($markerIndex -ge 0) {
            $responseBody = $curlText.Substring(0, $markerIndex).Trim()
            $statusText = $curlText.Substring(
                $markerIndex + $statusMarker.Length
            ).Trim()

            [void][int]::TryParse(
                $statusText,
                [ref]$httpStatus
            )
        }

        if (Test-WorkerSoopAuthExpired $responseBody) {
            throw "[SOOP_AUTH_EXPIRED] $responseBody"
        }

        if ($curlExitCode -eq 0 -and $httpStatus -ge 200 -and $httpStatus -lt 300) {
            try {
                $workerJson = $responseBody | ConvertFrom-Json

                if ($workerJson.success) {
                    $playlistUrl = [string]$workerJson.playlist_url

                    if (-not [string]::IsNullOrWhiteSpace($playlistUrl)) {
                        if ($attempt -gt 1) {
                            Write-LogMessage "Worker recovered on attempt $attempt/$MaxRetry channel=$($Channel.Name)"
                        }

                        Reset-WorkerCircuit
                        return [PSCustomObject]@{
                            Quality     = if ($workerJson.quality) { [string]$workerJson.quality } else { $quality }
                            Cdn         = [string]$workerJson.cdn
                            Host        = [string]$workerJson.host
                            PlaylistUrl = $playlistUrl
                        }
                    }

                    $lastError = "Worker 응답에 playlist_url이 없습니다."
                }
                else {
                    $message = [string]$workerJson.error
                    if ([string]::IsNullOrWhiteSpace($message)) {
                        $message = [string]$workerJson.message
                    }
                    if ([string]::IsNullOrWhiteSpace($message)) {
                        $message = "success=false"
                    }

                    if (Test-WorkerSoopAuthExpired $message) {
                        throw "[SOOP_AUTH_EXPIRED] $message"
                    }

                    $lastError = "Cloudflare Worker URL 발급 실패: $(Get-CompactExternalError $message)"
                }
            }
            catch {
                if ($_.Exception.Message.StartsWith("[SOOP_AUTH_EXPIRED]")) {
                    throw
                }

                $lastError = "Cloudflare Worker JSON/응답 처리 실패 (HTTP $httpStatus): $(Get-CompactExternalError $responseBody)"
            }
        }
        else {
            if ($curlExitCode -ne 0) {
                $lastError = "Cloudflare Worker transport 실패 (curl=$curlExitCode): $(Get-CompactExternalError $responseBody)"
            }
            else {
                $lastError = "Cloudflare Worker HTTP $httpStatus : $(Get-CompactExternalError $responseBody)"
            }
        }

        if ($attempt -lt $MaxRetry) {
            $delayIndex = [Math]::Min($attempt - 1, $backoff.Count - 1)
            $delay = $backoff[$delayIndex]
            $msg = "Worker retry $attempt/$MaxRetry failed; retry in ${delay}s - $lastError"
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] $msg"
            Write-LogMessage $msg -Level "WARN"
            Start-Sleep -Seconds $delay
        }
    }

    Open-WorkerCircuit
    throw $lastError
}
