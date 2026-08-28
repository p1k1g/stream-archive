param()

# ============================================================
# SOOP LIVE Watcher / Recorder
#
# Network routing
# ------------------------------------------------------------
# Channel page / live status:
#   DIRECT from local/Korea
#
# AID / global playlist URL:
#   Cloudflare Worker /soop/url via curl.exe
#
# m3u8 / .ts / .m4s / init / key:
#   Streamlink DIRECT from local/Korea
#
# SOOP auth:
#   local login -> selected auth cookies -> Worker POST
#
# Channel list:
#   hot reload while watcher is running
#
# Y -> N:
#   stops only that channel's Streamlink process tree
# ============================================================

$ErrorActionPreference = "Stop"

# Windows PowerShell 5.1 does not always auto-load System.Net.Http.
try {
    Add-Type -AssemblyName System.Net.Http -ErrorAction Stop
}
catch {
    throw "System.Net.Http assembly load failed: $($_.Exception.Message)"
}

# Prefer modern TLS for SOOP / Cloudflare HTTPS.
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor `
        [Net.SecurityProtocolType]::Tls12
}
catch {}


[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$ScriptDir = $PSScriptRoot.TrimEnd("\")
$SettingFile = Join-Path $ScriptDir "SOOP_LIVE_SETTING.ini"
$ChannelFile = Join-Path $ScriptDir "SOOP_LIVE_CHANNELS.txt"
$ControlDir = Join-Path $ScriptDir "control"
$scriptExitCode = 0

# Daily logging can be hot-toggled from SOOP_LIVE_SETTING.bat.
$script:LogEnabled = $false
$script:LogDir = Join-Path $ScriptDir "logs"
$script:LogRetentionDays = 30
$script:LastSettingWriteTime = [DateTime]::MinValue

# Duplicate watcher guard.
$script:WatcherMutex = $null
$script:WatcherMutexAcquired = $false

# Shared Worker circuit breaker. The watcher is single-threaded, so all live
# channels naturally share this endpoint-level failure state without locking.
$script:WorkerFailureCycles = 0
$script:WorkerCircuitLevel = 0
$script:WorkerCircuitOpenUntil = [DateTime]::MinValue

# Keyed by channel URL so NAME can be changed without losing identity.
$states = @{}

# ------------------------------------------------------------
# Helpers
# ------------------------------------------------------------

function Show-Line {
    Write-Host "========================================"
}

function Get-IniConfig {
    param([string]$Path)

    $config = @{}
    $lines = @(Get-Content -LiteralPath $Path -Encoding UTF8 -ErrorAction Stop)

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = ([string]$lines[$i]).Trim()

        if (
            [string]::IsNullOrWhiteSpace($line) -or
            $line.StartsWith("#") -or
            $line.StartsWith(";")
        ) {
            continue
        }

        $eq = $line.IndexOf("=")

        if ($eq -lt 1) {
            continue
        }

        $key = $line.Substring(0, $eq).Trim().ToUpperInvariant()
        $value = $line.Substring($eq + 1).Trim()

        if (
            [string]::IsNullOrWhiteSpace($value) -and
            ($i + 1) -lt $lines.Count
        ) {
            $next = ([string]$lines[$i + 1]).Trim()

            if (
                -not [string]::IsNullOrWhiteSpace($next) -and
                -not $next.StartsWith("#") -and
                -not $next.StartsWith(";") -and
                $next.IndexOf("=") -lt 0
            ) {
                $value = $next
                $i++
            }
        }

        $config[$key] = $value
    }

    return $config
}

function Resolve-ConfiguredPath {
    param(
        [string]$Path,
        [string]$DefaultPath
    )

    $value = $Path
    if ([string]::IsNullOrWhiteSpace($value)) {
        $value = $DefaultPath
    }

    if ([System.IO.Path]::IsPathRooted($value)) {
        return $value
    }

    return [System.IO.Path]::GetFullPath((Join-Path $ScriptDir $value))
}

function Write-LogMessage {
    param(
        [string]$Message,
        [string]$Level = "INFO",
        [switch]$Console
    )

    if ($Console) {
        Write-Host $Message
    }

    if (-not $script:LogEnabled) {
        return
    }

    try {
        if (-not (Test-Path $script:LogDir -PathType Container)) {
            New-Item -ItemType Directory -Path $script:LogDir -Force | Out-Null
        }

        $logFile = Join-Path $script:LogDir ((Get-Date).ToString("yyyy-MM-dd") + ".log")
        $line = "[{0}] [{1}] {2}" -f `
            (Get-Date -Format "yyyy-MM-dd HH:mm:ss"),
            $Level.ToUpperInvariant(),
            $Message

        Add-Content -LiteralPath $logFile -Value $line -Encoding UTF8
    }
    catch {
        # File logging must never stop the watcher.
    }
}

function Apply-LoggingConfig {
    param(
        [hashtable]$Config,
        [bool]$Initial = $false
    )

    $oldEnabled = $script:LogEnabled
    $script:LogEnabled = ConvertTo-BoolSetting `
        -Config $Config `
        -Name "LOG_ENABLED" `
        -Default $true

    $configuredDir = $Config["LOG_DIR"]
    $script:LogDir = Resolve-ConfiguredPath `
        -Path $configuredDir `
        -DefaultPath "logs"

    $script:LogRetentionDays = Get-IntSetting `
        -Config $Config `
        -Name "LOG_RETENTION_DAYS" `
        -Default 30

    if ($script:LogEnabled) {
        try {
            if (-not (Test-Path $script:LogDir -PathType Container)) {
                New-Item -ItemType Directory -Path $script:LogDir -Force | Out-Null
            }

            $cutoff = (Get-Date).AddDays(-$script:LogRetentionDays)
            Get-ChildItem -LiteralPath $script:LogDir -Filter "*.log" -File -ErrorAction SilentlyContinue |
                Where-Object { $_.LastWriteTime -lt $cutoff } |
                Remove-Item -Force -ErrorAction SilentlyContinue
        }
        catch {}
    }

    if (-not $Initial -and $oldEnabled -ne $script:LogEnabled) {
        if ($script:LogEnabled) {
            $msg = "Daily log ENABLED -> $script:LogDir"
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] $msg"
            Write-LogMessage $msg
        }
        else {
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] Daily log DISABLED"
        }
    }
}

function Update-HotConfig {
    try {
        $item = Get-Item -LiteralPath $SettingFile -ErrorAction Stop
        if ($item.LastWriteTimeUtc -eq $script:LastSettingWriteTime) {
            return
        }

        $script:LastSettingWriteTime = $item.LastWriteTimeUtc
        $newConfig = Get-IniConfig $SettingFile
        $changes = @()

        # Runtime values
        $v = Get-IntSetting $newConfig "CHECK_INTERVAL" $script:checkInterval
        if ($v -ne $script:checkInterval) { $changes += "CHECK_INTERVAL: $($script:checkInterval) -> $v"; $script:checkInterval = $v }

        $v = Get-IntSetting $newConfig "CHANNEL_RELOAD_INTERVAL" $script:reloadInterval
        if ($v -ne $script:reloadInterval) { $changes += "CHANNEL_RELOAD_INTERVAL: $($script:reloadInterval) -> $v"; $script:reloadInterval = $v }

        $v = Get-IntSetting $newConfig "RECORD_RETRY_INTERVAL" $script:retryInterval
        if ($v -ne $script:retryInterval) { $changes += "RECORD_RETRY_INTERVAL: $($script:retryInterval) -> $v"; $script:retryInterval = $v }

        $v = Get-IntSetting $newConfig "RECORD_STALL_TIMEOUT" $script:stallTimeout
        if ($v -ne $script:stallTimeout) { $changes += "RECORD_STALL_TIMEOUT: $($script:stallTimeout) -> $v"; $script:stallTimeout = $v }

        $v = Get-IntSetting $newConfig "RECORD_MONITOR_INTERVAL" $script:monitorInterval
        if ($v -ne $script:monitorInterval) { $changes += "RECORD_MONITOR_INTERVAL: $($script:monitorInterval) -> $v"; $script:monitorInterval = $v }

        $v = Get-IntSetting $newConfig "WORKER_MAX_RETRY" $script:workerMaxRetry
        if ($v -ne $script:workerMaxRetry) { $changes += "WORKER_MAX_RETRY: $($script:workerMaxRetry) -> $v"; $script:workerMaxRetry = $v }

        $v = Get-IntSetting $newConfig "CONSOLE_REFRESH_INTERVAL" $script:consoleRefreshInterval
        if ($v -ne $script:consoleRefreshInterval) { $changes += "CONSOLE_REFRESH_INTERVAL: $($script:consoleRefreshInterval) -> $v"; $script:consoleRefreshInterval = $v }

        $b = ConvertTo-BoolSetting -Config $newConfig -Name "CONSOLE_AUTO_FORMAT" -Default $script:consoleAutoFormat
        if ($b -ne $script:consoleAutoFormat) {
            $changes += "CONSOLE_AUTO_FORMAT: $($script:consoleAutoFormat) -> $b"
            $script:consoleAutoFormat = $b
        }

        $b = ConvertTo-BoolSetting -Config $newConfig -Name "CONSOLE_COLOR" -Default $script:consoleColor
        if ($b -ne $script:consoleColor) {
            $changes += "CONSOLE_COLOR: $($script:consoleColor) -> $b"
            $script:consoleColor = $b
        }

        $b = ConvertTo-BoolSetting -Config $newConfig -Name "CONSOLE_SHOW_PATH" -Default $script:consoleShowPath
        if ($b -ne $script:consoleShowPath) {
            $changes += "CONSOLE_SHOW_PATH: $($script:consoleShowPath) -> $b"
            $script:consoleShowPath = $b
        }

        $tmpMin = 0.0
        if (
            $newConfig.ContainsKey("MIN_FREE_SPACE_GB") -and
            [double]::TryParse(
                $newConfig["MIN_FREE_SPACE_GB"],
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$tmpMin
            ) -and
            $tmpMin -ge 0.1 -and
            $tmpMin -le 1000000 -and
            $tmpMin -ne $script:minFreeSpaceGB
        ) {
            $changes += "MIN_FREE_SPACE_GB: $($script:minFreeSpaceGB) -> $tmpMin"
            $script:minFreeSpaceGB = $tmpMin
        }

        # Applies to newly started recordings only.
        $vText = [string]$newConfig["QUALITY"]
        if (
            -not [string]::IsNullOrWhiteSpace($vText) -and
            $vText -ne $script:streamQuality
        ) {
            $changes += "QUALITY: $($script:streamQuality) -> $vText [NEXT RECORD]"
            $script:streamQuality = $vText
        }

        $vText = ([string]$newConfig["FILE_NAME_PATTERN"]).ToUpperInvariant()
        if (
            $vText -in @("LEGACY", "TITLE_NUMBER", "TIME_TITLE", "BJ_TITLE") -and
            $vText -ne $script:fileNamePattern
        ) {
            $changes += "FILE_NAME_PATTERN: $($script:fileNamePattern) -> $vText [NEXT RECORD]"
            $script:fileNamePattern = $vText
        }

        # Cloudflare Worker - next request uses the new values.
        $vText = [string]$newConfig["CLOUDFLARE_WORKER_URL"]
        if (
            -not [string]::IsNullOrWhiteSpace($vText) -and
            $vText -ne $script:workerUrl
        ) {
            $changes += "CLOUDFLARE_WORKER_URL: changed"
            $script:workerUrl = $vText
            Reset-WorkerCircuit
        }

        $vText = [string]$newConfig["CLOUDFLARE_API_KEY"]
        if (
            -not [string]::IsNullOrWhiteSpace($vText) -and
            $vText -ne $script:workerApiKey
        ) {
            $changes += "CLOUDFLARE_API_KEY: changed"
            $script:workerApiKey = $vText
            Reset-WorkerCircuit
        }

        # SOOP login - preserve active recordings, refresh auth session.
        $newUser = [string]$newConfig["SOOP_USERNAME"]
        $newPass = [string]$newConfig["SOOP_PASSWORD"]
        $loginChanged = (
            $newUser -ne $script:soopUsername -or
            $newPass -ne $script:soopPassword
        )

        $newPurge = ConvertTo-BoolSetting `
            -Config $newConfig `
            -Name "SOOP_PURGE_CREDENTIALS" `
            -Default $script:purgeCredentials

        if ($newPurge -ne $script:purgeCredentials) {
            $changes += "SOOP_PURGE_CREDENTIALS: $($script:purgeCredentials) -> $newPurge"
            $script:purgeCredentials = $newPurge
        }

        if ($loginChanged) {
            $script:soopUsername = $newUser
            $script:soopPassword = $newPass
            $changes += "SOOP LOGIN: credentials changed"

            try {
                Initialize-SoopLogin `
                    -Username $script:soopUsername `
                    -Password $script:soopPassword | Out-Null

                foreach ($key in @($states.Keys)) {
                    if ($null -eq $states[$key].Recording) {
                        $states[$key].NextCheck = Get-Date
                    }
                }
            }
            catch {
                Write-LogMessage (
                    "SOOP HOT LOGIN FAILED: {0}" -f $_.Exception.Message
                ) -Level "WARN"
            }
        }

        # LOG_ENABLED / LOG_DIR / LOG_RETENTION_DAYS
        Apply-LoggingConfig -Config $newConfig -Initial $false

        foreach ($change in $changes) {
            Write-Host (
                "[{0}] CONFIG RELOAD - {1}" -f `
                (Get-Date -Format "HH:mm:ss"),
                $change
            )
            Write-LogMessage ("CONFIG RELOAD - " + $change)
        }
    }
    catch {
        Write-LogMessage (
            "CONFIG HOT RELOAD FAILED: {0}" -f $_.Exception.Message
        ) -Level "WARN"
    }
}

function Initialize-WatcherMutex {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($ScriptDir.ToLowerInvariant())
    $sha = [System.Security.Cryptography.SHA256]::Create()

    try {
        $hashBytes = $sha.ComputeHash($bytes)
    }
    finally {
        $sha.Dispose()
    }

    $hash = ([BitConverter]::ToString($hashBytes)).Replace("-", "").Substring(0, 16)
    $mutexName = "Local\SOOP_LIVE_WATCHER_$hash"

    $script:WatcherMutex = New-Object System.Threading.Mutex($false, $mutexName)

    try {
        $script:WatcherMutexAcquired = $script:WatcherMutex.WaitOne(0, $false)
    }
    catch [System.Threading.AbandonedMutexException] {
        $script:WatcherMutexAcquired = $true
    }

    if (-not $script:WatcherMutexAcquired) {
        throw "SOOP LIVE Watcher가 이미 실행 중입니다. 중복 실행을 종료합니다."
    }
}

function Format-BytesHuman {
    param([Int64]$Bytes)

    if ($Bytes -ge 1TB) { return ("{0:N2} TB" -f ($Bytes / 1TB)) }
    if ($Bytes -ge 1GB) { return ("{0:N2} GB" -f ($Bytes / 1GB)) }
    if ($Bytes -ge 1MB) { return ("{0:N2} MB" -f ($Bytes / 1MB)) }
    if ($Bytes -ge 1KB) { return ("{0:N2} KB" -f ($Bytes / 1KB)) }
    return "$Bytes B"
}

function Format-Duration {
    param([TimeSpan]$Duration)

    if ($null -eq $Duration) {
        return "00:00:00"
    }

    # Avoid TimeSpan.ToString(custom-format) because Windows PowerShell 5.1
    # can throw FormatException depending on the runtime/escape handling.
    $totalHours = [Math]::Floor($Duration.TotalHours)
    $minutes = $Duration.Minutes
    $seconds = $Duration.Seconds

    return (
        "{0:00}:{1:00}:{2:00}" -f `
        $totalHours,
        $minutes,
        $seconds
    )
}

function Get-RecordingOutputSize {
    param($Recording)

    if ($null -eq $Recording -or [string]::IsNullOrWhiteSpace([string]$Recording.File)) {
        return [int64]0
    }

    try {
        # Recording titles can legitimately contain wildcard characters such as
        # '[' and ']'. -LiteralPath is required or PowerShell interprets the
        # generated filename as a wildcard and incorrectly reports zero bytes.
        if (Test-Path -LiteralPath $Recording.File -PathType Leaf) {
            return [int64](Get-Item -LiteralPath $Recording.File -ErrorAction Stop).Length
        }
    }
    catch {}

    return [int64]0
}

function Write-RecordingSummary {
    param(
        $State,
        $Recording,
        [string]$Reason
    )

    if ($null -eq $Recording) {
        return
    }

    $endedAt = Get-Date
    $duration = $endedAt - $Recording.StartedAt
    $size = Get-RecordingOutputSize -Recording $Recording

    # Machine-correlatable lifecycle event. Keep channel/account/reason/file on
    # one line so concurrent recorder output cannot leave the GUI with a stale
    # REC card after a recorder exits or a channel is removed.
    Write-Host (
        "[{0}] {1} [account={2}] : RECORD FINISHED | duration={3} | size={4} | reason={5} | file={6}" -f `
        (Get-Date -Format "HH:mm:ss"),
        $State.Name,
        $State.Channel.Account,
        (Format-Duration $duration),
        (Format-BytesHuman $size),
        $Reason,
        $Recording.File
    )

    Write-Host ""
    Show-Line
    Write-Host " RECORD FINISHED"
    Show-Line
    Write-Host "Channel  : $($State.Name)"
    Write-Host "Duration : $(Format-Duration $duration)"
    Write-Host "Size     : $(Format-BytesHuman $size)"
    Write-Host "Reason   : $Reason"
    Write-Host "File     : $($Recording.File)"
    Write-Host ""

    Write-LogMessage (
        "RECORD FINISHED channel={0} duration={1} size={2} reason={3} file={4}" -f `
        $State.Name,
        (Format-Duration $duration),
        (Format-BytesHuman $size),
        $Reason,
        $Recording.File
    )
}

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

function Get-SafeFileName {
    param(
        [string]$Name,
        [int]$MaxLength = 80
    )

    if ($null -eq $Name) {
        return "UNKNOWN"
    }

    $safe = $Name -replace '[\x00-\x1F]', ''
    $safe = $safe -replace '[\\/:*?"<>|]', '_'
    $safe = $safe -replace '\s+', ' '
    $safe = $safe.Trim().TrimEnd(".")

    if ([string]::IsNullOrWhiteSpace($safe)) {
        return "UNKNOWN"
    }

    if ($MaxLength -gt 0 -and $safe.Length -gt $MaxLength) {
        $safe = $safe.Substring(0, $MaxLength).Trim().TrimEnd(".")
    }

    if ($safe -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$') {
        $safe = "_" + $safe
    }

    return $safe
}

function Get-IntSetting {
    param(
        [hashtable]$Config,
        [string]$Name,
        [int]$Default
    )

    # Prevent absurd-but-valid Int32 values from creating multi-day sleeps,
    # retry storms, or effectively endless loops.
    $min = 1
    $max = [int]::MaxValue

    switch ($Name.ToUpperInvariant()) {
        "CHECK_INTERVAL"            { $min = 1;  $max = 3600 }
        "CHANNEL_RELOAD_INTERVAL"   { $min = 1;  $max = 300 }
        "RECORD_RETRY_INTERVAL"     { $min = 1;  $max = 600 }
        "RECORD_STALL_TIMEOUT"      { $min = 10; $max = 3600 }
        "RECORD_MONITOR_INTERVAL"   { $min = 1;  $max = 300 }
        "WORKER_MAX_RETRY"          { $min = 1;  $max = 10 }
        "CONSOLE_REFRESH_INTERVAL"  { $min = 1;  $max = 60 }
        "LOG_RETENTION_DAYS"        { $min = 1;  $max = 3650 }
    }

    $value = 0

    if (
        $Config.ContainsKey($Name) -and
        [int]::TryParse($Config[$Name], [ref]$value) -and
        $value -ge $min -and
        $value -le $max
    ) {
        return $value
    }

    return $Default
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


function ConvertTo-BoolSetting {
    param(
        [hashtable]$Config,
        [string]$Name,
        [bool]$Default
    )

    if (-not $Config.ContainsKey($Name)) {
        return $Default
    }

    $value = ([string]$Config[$Name]).Trim().ToUpperInvariant()

    if ($value -in @("Y", "YES", "TRUE", "1", "ON")) {
        return $true
    }

    if ($value -in @("N", "NO", "FALSE", "0", "OFF")) {
        return $false
    }

    return $Default
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

function Resolve-Streamlink {
    param([hashtable]$Config)

    $localCandidates = @(
        (Join-Path $ScriptDir "streamlink.exe"),
        "C:\Program Files\Streamlink\bin\streamlink.exe",
        "C:\Program Files\Streamlink\streamlink.exe"
    )

    foreach ($candidate in $localCandidates) {
        if (Test-Path $candidate -PathType Leaf) {
            return $candidate
        }
    }

    $fallback = $Config["STREAMLINK_FALLBACK"]

    if (
        -not [string]::IsNullOrWhiteSpace($fallback) -and
        (Test-Path $fallback -PathType Leaf)
    ) {
        return $fallback
    }

    $cmd = Get-Command streamlink.exe -ErrorAction SilentlyContinue

    if ($cmd) {
        return $cmd.Source
    }

    throw "streamlink.exe를 찾을 수 없습니다."
}


function Get-FreeSpaceInfo {
    param([string]$Path)

    try {
        $fullPath = [System.IO.Path]::GetFullPath($Path)
        $root = [System.IO.Path]::GetPathRoot($fullPath)

        if ([string]::IsNullOrWhiteSpace($root)) {
            return $null
        }

        $drive = New-Object System.IO.DriveInfo($root)

        if (-not $drive.IsReady) {
            return $null
        }

        return [PSCustomObject]@{
            Root   = $drive.Name.TrimEnd("\")
            FreeGB = [Math]::Round($drive.AvailableFreeSpace / 1GB, 2)
        }
    }
    catch {
        return $null
    }
}

function Test-LowDiskSpace {
    param(
        [string]$Path,
        [double]$MinFreeGB
    )

    $info = Get-FreeSpaceInfo -Path $Path

    if ($null -eq $info) {
        return [PSCustomObject]@{
            IsLow     = $false
            IsUnknown = $true
            Root      = "UNKNOWN"
            FreeGB    = $null
        }
    }

    return [PSCustomObject]@{
        IsLow     = ($info.FreeGB -lt $MinFreeGB)
        IsUnknown = $false
        Root      = $info.Root
        FreeGB    = $info.FreeGB
    }
}

function Get-ChannelOutputDirectory {
    param(
        $Channel,
        [string]$DefaultOutputDir
    )

    $base = $DefaultOutputDir

    if (-not [string]::IsNullOrWhiteSpace($Channel.OutDir)) {
        $base = $Channel.OutDir
    }

    $safeName = Get-SafeFileName $Channel.Name
    $dir = Join-Path $base $safeName

    if (-not (Test-Path $dir -PathType Container)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    return $dir
}

function Get-UniqueOutputFile {
    param(
        [string]$Directory,
        [string]$ChannelName,
        [string]$Title,
        [string]$Pattern = "LEGACY"
    )

    $safeChannel = Get-SafeFileName -Name $ChannelName -MaxLength 60
    $safeTitle = Get-SafeFileName -Name $Title -MaxLength 90
    $now = Get-Date
    $date = $now.ToString("yyMMdd")
    $time = $now.ToString("HHmmss")

    switch ($Pattern.ToUpperInvariant()) {
        "TITLE_NUMBER" {
            $baseName = "{0}_{1}" -f $date, $safeTitle
            $number = 1

            while ($number -le 9999) {
                $candidate = Join-Path $Directory (
                    "{0}_{1:D2}_{2}.ts" -f $baseName, $number, $safeChannel
                )

                if (-not (Test-Path $candidate)) {
                    return $candidate
                }
                $number++
            }

            throw "동일 날짜/제목 출력 파일 충돌이 너무 많습니다: $baseName"
        }

        "TIME_TITLE" {
            $baseName = "{0}_{1}_{2}_{3}" -f $date, $time, $safeTitle, $safeChannel
        }

        "BJ_TITLE" {
            $baseName = "{0}_{1}_{2}" -f $date, $safeChannel, $safeTitle
        }

        default {
            $baseName = "{0}_{1}_{2}" -f $date, $time, $safeChannel
        }
    }

    $candidate = Join-Path $Directory ($baseName + ".ts")
    $number = 2

    while (Test-Path $candidate) {
        if ($number -gt 9999) {
            throw "동일 출력 파일 충돌이 너무 많습니다: $baseName"
        }

        $candidate = Join-Path $Directory ("{0}_{1:D2}.ts" -f $baseName, $number)
        $number++
    }

    return $candidate
}

function Get-NextMemoryLogTime {
    param([DateTime]$From = (Get-Date))

    $next = Get-Date `
        -Year $From.Year `
        -Month $From.Month `
        -Day $From.Day `
        -Hour $From.Hour `
        -Minute 0 `
        -Second 0

    return $next.AddHours(1)
}

function Write-WatcherMemoryLog {
    if (-not $script:LogEnabled) {
        return
    }

    try {
        $proc = [System.Diagnostics.Process]::GetCurrentProcess()
        $memoryMB = [Math]::Round($proc.WorkingSet64 / 1MB, 1)

        Write-LogMessage (
            "WATCHER MEMORY : {0} MB" -f $memoryMB
        )
    }
    catch {
        Write-LogMessage (
            "WATCHER MEMORY CHECK FAILED : {0}" -f $_.Exception.Message
        ) -Level "WARN"
    }
}

function Get-LatestStreamlinkProgress {
    param($Recording)

    if ($null -eq $Recording) {
        return "[download] Waiting..."
    }

    $now = Get-Date
    # Update-RecordingStates already samples this file on the same watcher
    # loop. Reuse that value and touch the file only for the first sample.
    $size = [int64]$Recording.LastSize

    try {
        if ($size -le 0 -and (Test-Path -LiteralPath $Recording.File -PathType Leaf)) {
            $size = (Get-Item -LiteralPath $Recording.File -ErrorAction Stop).Length
        }
    }
    catch {}

    if ($size -le 0) {
        return "[download] Waiting for output file..."
    }

    $elapsed = $now - $Recording.StartedAt

    # Calculate current transfer rate from the previous dashboard sample.
    $rateBytes = 0.0
    $sampleSeconds = ($now - $Recording.DashboardLastAt).TotalSeconds

    if ($sampleSeconds -gt 0) {
        $delta = $size - [int64]$Recording.DashboardLastSize

        if ($delta -ge 0) {
            $rateBytes = $delta / $sampleSeconds
        }
    }

    # First sample has no previous interval. Use average rate since start.
    if ($rateBytes -le 0 -and $elapsed.TotalSeconds -gt 0) {
        $rateBytes = $size / $elapsed.TotalSeconds
    }

    $Recording.DashboardLastSize = $size
    $Recording.DashboardLastAt = $now

    $rateText = Format-BytesHuman ([int64]$rateBytes)

    if ($script:consoleShowPath) {
        return (
            "[download] Written {0} to {1} ({2} @ {3}/s)" -f `
            (Format-BytesHuman $size),
            $Recording.File,
            (Format-Duration $elapsed),
            $rateText
        )
    }

    return (
        "[download] Written {0} ({1} @ {2}/s)" -f `
        (Format-BytesHuman $size),
        (Format-Duration $elapsed),
        $rateText
    )
}


function Remove-StaleRecorderConsoleFiles {
    param([int]$OlderThanDays = 7)

    try {
        $cutoff = (Get-Date).AddDays(-$OlderThanDays)
        Get-ChildItem `
            -LiteralPath ([System.IO.Path]::GetTempPath()) `
            -Filter "soop_streamlink_*.log" `
            -File `
            -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $cutoff } |
            Remove-Item -Force -ErrorAction SilentlyContinue
    }
    catch {
        # Temp cleanup must never stop the watcher.
    }
}

function Remove-RecorderConsoleFiles {
    param($Recording)

    if ($null -eq $Recording) {
        return
    }

    foreach ($path in @($Recording.StdoutFile, $Recording.StderrFile)) {
        if ([string]::IsNullOrWhiteSpace($path)) {
            continue
        }

        try {
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        }
        catch {}
    }
}

function Show-ConsoleDashboard {
    param(
        [hashtable]$States,
        [DateTime]$Now
    )

    $offlineStates = [System.Collections.Generic.List[object]]::new()
    $recordingStates = [System.Collections.Generic.List[object]]::new()

    foreach ($key in @($States.Keys)) {
        $state = $States[$key]

        if (-not $state.Enabled) {
            continue
        }

        if ($null -ne $state.Recording) {
            $recordingStates.Add($state)
            continue
        }

        if ($state.LowDisk) {
            $offlineStates.Add([PSCustomObject]@{
                Name   = $state.Name
                Account = $state.Channel.Account
                Status = "LOW DISK"
            })
            continue
        }

        if ($state.LastStatus -eq "OFFLINE") {
            $offlineStates.Add([PSCustomObject]@{
                Name   = $state.Name
                Account = $state.Channel.Account
                Status = "OFFLINE"
            })
        }
        elseif ($state.LastStatus -eq "ERROR") {
            $offlineStates.Add([PSCustomObject]@{
                Name   = $state.Name
                Account = $state.Channel.Account
                Status = "CHECK ERROR"
            })
        }
    }

    if ($offlineStates.Count -eq 0 -and $recordingStates.Count -eq 0) {
        return
    }

    if ($script:consoleAutoFormat) {
        $offlineView = @($offlineStates | Sort-Object Name)
        $recordingView = @($recordingStates | Sort-Object Name)
    }
    else {
        $offlineView = @($offlineStates)
        $recordingView = @($recordingStates)
    }

    Write-Host ""

    foreach ($item in $offlineView) {
        if ($script:consoleColor -and $item.Status -eq "LOW DISK") {
            Write-Host (
                "[{0}] {1} [account={2}] : {3}" -f `
                $Now.ToString("HH:mm:ss"),
                $item.Name,
                $item.Account,
                $item.Status
            ) -ForegroundColor Yellow
        }
        else {
            Write-Host (
                "[{0}] {1} [account={2}] : {3}" -f `
                $Now.ToString("HH:mm:ss"),
                $item.Name,
                $item.Account,
                $item.Status
            )
        }
    }

    if ($script:consoleAutoFormat) {
        Write-Host ""
        Write-Host ("-" * 120)
        if ($script:consoleColor) {
            Write-Host " RECORDING" -ForegroundColor Green
        }
        else {
            Write-Host " RECORDING"
        }
        Write-Host ("-" * 120)
    }

    if ($recordingView.Count -eq 0) {
        if ($script:consoleAutoFormat) {
            Write-Host "(none)"
        }
    }
    else {
        foreach ($state in $recordingView) {
            $progress = Get-LatestStreamlinkProgress `
                -Recording $state.Recording

            # Always include the channel name on the SAME progress line.
            # Multiple recorder processes can interleave their output, so a
            # preceding "[Channel]" header is not a safe GUI correlation key.
            Write-Host (
                "[{0}] {1} [account={2}] : RECORDING | {3}" -f `
                $Now.ToString("HH:mm:ss"),
                $state.Name,
                $state.Channel.Account,
                $progress
            )
        }
    }

    Write-Host ""
}

function Start-ChannelRecording {
    param(
        $Channel,
        $LiveInfo,
        [string]$StreamlinkExe,
        [string]$DefaultOutputDir,
        [string]$StreamQuality,
        [string]$MasterQuality,
        [string]$WorkerUrl,
        [string]$WorkerApiKey,
        [hashtable]$AuthCookies,
        [double]$MinFreeSpaceGB,
        [string]$SoopUsername,
        [string]$SoopPassword,
        [int]$WorkerMaxRetry
    )

    try {
        $streamInfo = Get-CloudflarePlaylistUrl `
            -Channel $Channel `
            -LiveInfo $LiveInfo `
            -MasterQuality $MasterQuality `
            -WorkerUrl $WorkerUrl `
            -WorkerApiKey $WorkerApiKey `
            -AuthCookies $AuthCookies `
            -MaxRetry $WorkerMaxRetry
    }
    catch {
        if (
            $_.Exception.Message.StartsWith("[SOOP_AUTH_EXPIRED]") -and
            -not [string]::IsNullOrWhiteSpace($SoopUsername) -and
            -not [string]::IsNullOrWhiteSpace($SoopPassword)
        ) {
            $msg = "Worker reported SOOP auth expiry; refreshing login channel=$($Channel.Name)"
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] $msg"
            Write-LogMessage $msg -Level "WARN"

            Initialize-SoopLogin `
                -Username $SoopUsername `
                -Password $SoopPassword | Out-Null

            $streamInfo = Get-CloudflarePlaylistUrl `
                -Channel $Channel `
                -LiveInfo $LiveInfo `
                -MasterQuality $MasterQuality `
                -WorkerUrl $WorkerUrl `
                -WorkerApiKey $WorkerApiKey `
                -AuthCookies $script:SoopAuthCookies `
                -MaxRetry $WorkerMaxRetry
        }
        else {
            throw
        }
    }

    $safeName = Get-SafeFileName $Channel.Name

    $outputDir = Get-ChannelOutputDirectory `
        -Channel $Channel `
        -DefaultOutputDir $DefaultOutputDir

    $disk = Test-LowDiskSpace `
        -Path $outputDir `
        -MinFreeGB $MinFreeSpaceGB

    if ($disk.IsUnknown) {
        throw ("DISK SPACE CHECK FAILED - Path={0}" -f $outputDir)
    }

    if ($disk.IsLow) {
        throw ("LOW DISK SPACE - {0} Free={1}GB Limit={2}GB" -f `
            $disk.Root, $disk.FreeGB, $MinFreeSpaceGB)
    }

    $outputFile = Get-UniqueOutputFile `
        -Directory $outputDir `
        -ChannelName $Channel.Name `
        -Title $LiveInfo.Title `
        -Pattern $script:fileNamePattern

    Write-Host ""
    Show-Line
    Write-Host " LIVE DETECTED / RECORD START"
    Show-Line
    Write-Host ""
    Write-Host "Channel : $($Channel.Name)"
    Write-Host "Account : $($Channel.Account)"
    Write-Host "BJ      : $($LiveInfo.BjNick)"
    Write-Host "BNO     : $($LiveInfo.Bno)"
    Write-Host "Title   : $($LiveInfo.Title)"
    Write-Host "HLS     : $($streamInfo.Quality)"
    if (-not [string]::IsNullOrWhiteSpace($streamInfo.Cdn)) {
        Write-Host "CDN     : $($streamInfo.Cdn)"
    }
    if (-not [string]::IsNullOrWhiteSpace($streamInfo.Host)) {
        Write-Host "Host    : $($streamInfo.Host)"
    }
    Write-Host "Route   : Cloudflare AID -> DIRECT DOWNLOAD"
    Write-Host "Quality : $StreamQuality"
    Write-Host "Output  : $outputFile"
    Write-Host ""

    Write-LogMessage (
        "RECORD START channel={0} account={1} bno={2} title={3} hls={4} cdn={5} host={6} file={7}" -f `
        $Channel.Name,
        $Channel.Account,
        $LiveInfo.Bno,
        $LiveInfo.Title,
        $streamInfo.Quality,
        $streamInfo.Cdn,
        $streamInfo.Host,
        $outputFile
    )

    # Only AID/global playlist URL issuance goes through Cloudflare.
    # The returned global playlist URL is opened directly by Streamlink
    # from the local/Korean connection.
    $streamUrl = $streamInfo.PlaylistUrl
    if (-not $streamUrl.StartsWith("hls://", [System.StringComparison]::OrdinalIgnoreCase)) {
        $streamUrl = "hls://$streamUrl"
    }

    $args = @(
        "`"$streamUrl`"",
        "`"$StreamQuality`"",
        "--output", "`"$outputFile`"",
        "--force",
        "--hls-live-edge", "3",
        "--stream-segment-threads", "3"
    )

    $argString = $args -join " "

    $consoleBase = Join-Path `
        ([System.IO.Path]::GetTempPath()) `
        ("soop_streamlink_{0}_{1}" -f $PID, [Guid]::NewGuid().ToString("N"))

    $stdoutFile = $consoleBase + ".stdout.log"
    $stderrFile = $consoleBase + ".stderr.log"

    # Important: Streamlink never writes directly to this watcher console.
    # Each recorder gets its own output files.
    $proc = Start-Process `
        -FilePath $StreamlinkExe `
        -ArgumentList $argString `
        -WorkingDirectory $outputDir `
        -NoNewWindow `
        -RedirectStandardOutput $stdoutFile `
        -RedirectStandardError $stderrFile `
        -PassThru

    return [PSCustomObject]@{
        Process    = $proc
        PID        = $proc.Id
        Bno        = $LiveInfo.Bno
        File       = $outputFile
        StartedAt  = Get-Date
        LastStatus  = "RECORDING"
        LastSize      = [int64]0
        LastGrowthAt  = Get-Date
        LastMonitorAt = Get-Date
        OutputDir     = $outputDir
        StdoutFile         = $stdoutFile
        StderrFile         = $stderrFile
        DashboardLastSize  = [int64]0
        DashboardLastAt    = Get-Date
    }
}

function Stop-RecorderProcessTree {
    param([System.Diagnostics.Process]$Process,[int]$ProcessId)
    try{$Process.Refresh();if($Process.HasExited){return $true}}catch{return $true}
    $taskkillOk=$false
    try{& taskkill.exe /PID $ProcessId /T /F *> $null;$taskkillOk=($LASTEXITCODE -eq 0)}catch{$taskkillOk=$false}
    Start-Sleep -Milliseconds 200
    try{$Process.Refresh();if($Process.HasExited){return $true}}catch{return $true}
    try{$Process.Kill($true);if($Process.WaitForExit(5000)){return $true}}catch{}
    try{$Process.Refresh();return $Process.HasExited}catch{return $taskkillOk}
}

function Stop-ChannelRecording {
    param([string]$Key,[string]$Reason)
    if(-not $states.ContainsKey($Key)){return $true}
    $state=$states[$Key]
    if($null -eq $state.Recording){return $true}
    $rec=$state.Recording
    try{$rec.Process.Refresh()}catch{}
    if(-not $rec.Process.HasExited){
        Write-Host ("[{0}] STOP {1} (PID {2}) - {3}" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$rec.PID,$Reason)
        $stopped=Stop-RecorderProcessTree -Process $rec.Process -ProcessId $rec.PID
        if(-not $stopped){
            Write-LogMessage ("PROCESS STOP FAILED channel={0} pid={1} reason={2}" -f $state.Name,$rec.PID,$Reason) -Level "ERROR"
            return $false
        }
    }
    Write-RecordingSummary -State $state -Recording $rec -Reason $Reason
    Remove-RecorderConsoleFiles -Recording $rec
    $state.Recording=$null
    $state.NextCheck=Get-Date
    return $true
}

function Update-RecordingStates {
    param(
        [int]$RetryInterval,
        [int]$StallTimeout,
        [int]$MonitorInterval,
        [double]$MinFreeSpaceGB
    )

    foreach ($key in @($states.Keys)) {
        $state = $states[$key]

        if ($null -eq $state.Recording) {
            continue
        }

        $rec = $state.Recording
        $now = Get-Date

        try {
            $rec.Process.Refresh()
        }
        catch {}

        if ($rec.Process.HasExited) {
            $exitCode = $null
            try { $exitCode = $rec.Process.ExitCode } catch {}

            Write-Host (
                "[{0}] RECORDER EXIT {1} PID={2} CODE={3}" -f `
                $now.ToString("HH:mm:ss"),
                $state.Name,
                $rec.PID,
                $exitCode
            )

            Write-RecordingSummary `
                -State $state `
                -Recording $rec `
                -Reason ("RECORDER EXIT CODE=" + $exitCode)

            Remove-RecorderConsoleFiles -Recording $rec

            $state.Recording = $null
            $state.NextCheck = Get-Date
            continue
        }

        if (($now - $rec.LastMonitorAt).TotalSeconds -lt $MonitorInterval) {
            continue
        }

        $rec.LastMonitorAt = $now

        $disk = Test-LowDiskSpace `
            -Path $rec.OutputDir `
            -MinFreeGB $MinFreeSpaceGB

        if ($disk.IsUnknown) {
            Write-Host (
                "[{0}] {1} [account={2}] : DISK SPACE UNKNOWN - recording continues" -f `
                $now.ToString("HH:mm:ss"),
                $state.Name,
                $state.Channel.Account
            )
            Write-LogMessage ("DISK UNKNOWN channel={0} path={1}" -f $state.Name,$rec.OutputDir) -Level "WARN"
        }

        if ($disk.IsLow) {
            Write-Host ""
            Write-Host (
                "[{0}] {1} [account={2}] : LOW DISK SPACE" -f `
                $now.ToString("HH:mm:ss"),
                $state.Name,
                $state.Channel.Account
            )
            Write-Host "Drive   : $($disk.Root)"
            Write-Host "Free    : $($disk.FreeGB) GB"
            Write-Host "Limit   : $MinFreeSpaceGB GB"
            Write-Host "Action  : Recording stopped"
            Write-LogMessage ("LOW DISK channel={0} drive={1} free={2}GB limit={3}GB" -f $state.Name,$disk.Root,$disk.FreeGB,$MinFreeSpaceGB) -Level "WARN"

            Stop-ChannelRecording -Key $key -Reason "LOW DISK SPACE"

            $state.LowDisk = $true
            $state.LastStatus = "LOW_DISK"
            $state.NextDiskCheck = $now.AddSeconds(30)
            continue
        }

        $size = Get-RecordingOutputSize -Recording $rec

        if ($size -gt $rec.LastSize) {
            $rec.LastSize = $size
            $rec.LastGrowthAt = $now
            continue
        }

        if (($now - $rec.LastGrowthAt).TotalSeconds -ge $StallTimeout) {
            Write-LogMessage ("RECORD STALLED channel={0} no_growth={1}s" -f $state.Name,$StallTimeout) -Level "WARN"
            Write-Host (
                "[{0}] {1} : RECORD STALLED ({2}s no growth)" -f `
                $now.ToString("HH:mm:ss"),
                $state.Name,
                $StallTimeout
            )

            Stop-ChannelRecording -Key $key -Reason "RECORD STALLED"
            $state.NextCheck = Get-Date
        }
    }
}


function Process-ControlCommands {
    param([hashtable]$States,[hashtable]$StateKeyByAccount)
    if(-not (Test-Path -LiteralPath $ControlDir -PathType Container)){return}
    foreach($file in @(Get-ChildItem -LiteralPath $ControlDir -Filter "*.cmd" -File -ErrorAction SilentlyContinue | Sort-Object Name)){
        $claimed=Join-Path $ControlDir ($file.BaseName+".processing")
        try{
            Move-Item -LiteralPath $file.FullName -Destination $claimed -ErrorAction Stop
            $line=(Get-Content -LiteralPath $claimed -Raw -Encoding UTF8 -ErrorAction Stop).Trim()
            if([string]::IsNullOrWhiteSpace($line)){continue}
            $parts=$line.Split('|',2);if($parts.Count -lt 2){continue}
            $action=$parts[0].Trim().ToUpperInvariant();$account=$parts[1].Trim()
            $targetKey=$null
            $accountKey=$account.ToLowerInvariant()
            if($StateKeyByAccount.ContainsKey($accountKey)){$targetKey=$StateKeyByAccount[$accountKey]}
            if($null -eq $targetKey){Write-LogMessage "CONTROL CHANNEL NOT FOUND account=$account" -Level "WARN";continue}
            $state=$States[$targetKey]
            if($action -eq "STOP_ONCE"){
                $state.SuppressedBno=$state.LastBno
                Write-LogMessage ("CHANNEL STOP REQUESTED channel={0} account={1} bno={2}" -f $state.Name,$account,$state.SuppressedBno)
                Write-Host ("[{0}] {1} [account={2}] : CHANNEL STOP REQUESTED BNO={3}" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$account,$state.SuppressedBno)
                $stopped=$true
                if($null -ne $state.Recording){
                    $stopped=Stop-ChannelRecording -Key $targetKey -Reason "USER CHANNEL STOP"
                }
                if($stopped){
                    Write-LogMessage ("CHANNEL STOP COMPLETED channel={0} account={1} bno={2}" -f $state.Name,$account,$state.SuppressedBno)
                    Write-Host ("[{0}] {1} [account={2}] : CHANNEL STOP COMPLETED BNO={3}" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$account,$state.SuppressedBno)
                }
                else{
                    $state.SuppressedBno=$null
                    Write-LogMessage ("CHANNEL STOP FAILED channel={0} account={1}" -f $state.Name,$account) -Level "ERROR"
                    Write-Host ("[{0}] {1} [account={2}] : CHANNEL STOP FAILED - recorder process did not exit" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$account)
                }
            }
            elseif($action -eq "RESUME_ONCE"){
                $resumeBno=$state.SuppressedBno
                $state.SuppressedBno=$null
                $state.NextCheck=Get-Date
                Write-LogMessage ("CHANNEL RESUME REQUESTED channel={0} account={1} bno={2}" -f $state.Name,$account,$resumeBno)
                Write-Host ("[{0}] {1} [account={2}] : CHANNEL RESUME REQUESTED BNO={3}" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$account,$resumeBno)
            }
        }catch{
            Write-LogMessage ("CONTROL COMMAND FAILED file={0} error={1}" -f $file.Name,$_.Exception.Message) -Level "WARN"
        }finally{
            try{if(Test-Path -LiteralPath $claimed){Remove-Item -LiteralPath $claimed -Force -ErrorAction SilentlyContinue}}catch{}
        }
    }
}

# ------------------------------------------------------------
# Main
# ------------------------------------------------------------

try {
    if (-not (Test-Path $SettingFile -PathType Leaf)) {
        throw "SOOP_LIVE_SETTING.ini 파일이 없습니다."
    }

    if (-not (Test-Path $ChannelFile -PathType Leaf)) {
        throw "SOOP_LIVE_CHANNELS.txt 파일이 없습니다."
    }


    $config = Get-IniConfig $SettingFile

    Apply-LoggingConfig -Config $config -Initial $true
    try {
        $script:LastSettingWriteTime = (Get-Item -LiteralPath $SettingFile -ErrorAction Stop).LastWriteTimeUtc
    }
    catch {}

    Initialize-WatcherMutex
    New-Item -ItemType Directory -Path $ControlDir -Force | Out-Null
    Get-ChildItem -LiteralPath $ControlDir -Filter "*.processing" -File -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    Write-LogMessage "Watcher started PID=$PID path=$ScriptDir"
    Remove-StaleRecorderConsoleFiles -OlderThanDays 7

    $checkInterval = Get-IntSetting $config "CHECK_INTERVAL" 30
    $reloadInterval = Get-IntSetting $config "CHANNEL_RELOAD_INTERVAL" 2
    $retryInterval = Get-IntSetting $config "RECORD_RETRY_INTERVAL" 5
    $stallTimeout = Get-IntSetting $config "RECORD_STALL_TIMEOUT" 90
    $monitorInterval = Get-IntSetting $config "RECORD_MONITOR_INTERVAL" 5
    $workerMaxRetry = Get-IntSetting $config "WORKER_MAX_RETRY" 3
    $consoleRefreshInterval = Get-IntSetting $config "CONSOLE_REFRESH_INTERVAL" 5

    $consoleAutoFormat = ConvertTo-BoolSetting -Config $config -Name "CONSOLE_AUTO_FORMAT" -Default $true
    $consoleColor = ConvertTo-BoolSetting -Config $config -Name "CONSOLE_COLOR" -Default $true
    $consoleShowPath = ConvertTo-BoolSetting -Config $config -Name "CONSOLE_SHOW_PATH" -Default $false

    $minFreeSpaceGB = 20.0
    $parsedMinFree = 0.0
    if (
        $config.ContainsKey("MIN_FREE_SPACE_GB") -and
        [double]::TryParse(
            $config["MIN_FREE_SPACE_GB"],
            [Globalization.NumberStyles]::Float,
            [Globalization.CultureInfo]::InvariantCulture,
            [ref]$parsedMinFree
        ) -and
        $parsedMinFree -ge 0.1 -and
        $parsedMinFree -le 1000000
    ) {
        $minFreeSpaceGB = $parsedMinFree
    }

    $defaultOutputDir = $config["OUTPUT_DIR"]
    if ([string]::IsNullOrWhiteSpace($defaultOutputDir)) {
        $defaultOutputDir = "C:\SOOP_LIVE"
    }

    $streamQuality = $config["QUALITY"]
    if ([string]::IsNullOrWhiteSpace($streamQuality)) {
        $streamQuality = "best"
    }

    $fileNamePattern = ([string]$config["FILE_NAME_PATTERN"]).ToUpperInvariant()
    if ($fileNamePattern -notin @("LEGACY", "TITLE_NUMBER", "TIME_TITLE", "BJ_TITLE")) {
        $fileNamePattern = "LEGACY"
    }

    $masterQuality = $config["MASTER_QUALITY"]
    if ([string]::IsNullOrWhiteSpace($masterQuality)) {
        $masterQuality = "auto"
    }

    $workerUrl = $config["CLOUDFLARE_WORKER_URL"]
    $workerApiKey = $config["CLOUDFLARE_API_KEY"]

    $soopUsername = $config["SOOP_USERNAME"]
    $soopPassword = $config["SOOP_PASSWORD"]
    $purgeCredentials = ConvertTo-BoolSetting `
        -Config $config `
        -Name "SOOP_PURGE_CREDENTIALS" `
        -Default $true

    if ([string]::IsNullOrWhiteSpace($workerUrl)) {
        throw "CLOUDFLARE_WORKER_URL 설정이 없습니다."
    }

    if ($workerUrl -notmatch '^https://') {
        throw "CLOUDFLARE_WORKER_URL은 https:// URL이어야 합니다."
    }

    if ([string]::IsNullOrWhiteSpace($workerApiKey)) {
        throw "CLOUDFLARE_API_KEY 설정이 없습니다."
    }

    # Runtime values are script-scoped so Update-HotConfig can modify them.
    $script:checkInterval = $checkInterval
    $script:reloadInterval = $reloadInterval
    $script:retryInterval = $retryInterval
    $script:stallTimeout = $stallTimeout
    $script:monitorInterval = $monitorInterval
    $script:workerMaxRetry = $workerMaxRetry
    $script:consoleRefreshInterval = $consoleRefreshInterval
    $script:consoleAutoFormat = $consoleAutoFormat
    $script:consoleColor = $consoleColor
    $script:consoleShowPath = $consoleShowPath
    $script:minFreeSpaceGB = $minFreeSpaceGB
    $script:streamQuality = $streamQuality
    $script:fileNamePattern = $fileNamePattern
    $script:workerUrl = $workerUrl
    $script:workerApiKey = $workerApiKey
    $script:soopUsername = $soopUsername
    $script:soopPassword = $soopPassword
    $script:purgeCredentials = $purgeCredentials

    $streamlinkExe = Resolve-Streamlink $config

    if (-not (Test-Path $defaultOutputDir -PathType Container)) {
        New-Item `
            -ItemType Directory `
            -Path $defaultOutputDir `
            -Force |
        Out-Null
    }

    $startupChannels = Get-SoopChannelList $ChannelFile
    $startupChannel = @(
        $startupChannels |
        Where-Object { $_.Enabled } |
        Select-Object -First 1
    )

    if (
        $purgeCredentials -and
        $startupChannel.Count -gt 0 -and
        -not [string]::IsNullOrWhiteSpace($soopUsername) -and
        -not [string]::IsNullOrWhiteSpace($soopPassword)
    ) {
        Invoke-StreamlinkCredentialPurge `
            -StreamlinkExe $streamlinkExe `
            -ChannelUrl $startupChannel[0].Url `
            -Username $soopUsername `
            -Password $soopPassword
    }

    try {
        Initialize-SoopLogin `
            -Username $soopUsername `
            -Password $soopPassword | Out-Null
    }
    catch {
        # Login is optional for ordinary broadcasts. A temporary SOOP/network
        # failure must not terminate the entire watcher during bootstrap.
        $script:SoopAuthCookies = @{}
        $initialLoginError = $_.Exception.Message
        Write-Host (
            "[AUTH] SOOP initial login failed; continuing without login - {0}" -f `
            $initialLoginError
        )
        Write-LogMessage (
            "SOOP INITIAL LOGIN FAILED; watcher continues without login error={0}" -f `
            $initialLoginError
        ) -Level "WARN"
    }

    Write-Host ""
    Show-Line
    Write-Host " SOOP LIVE WATCHER"
    Show-Line
    Write-Host ""
    Write-Host "Check      : $checkInterval sec"
    Write-Host "Hot Reload : $reloadInterval sec"
    Write-Host "Output     : $defaultOutputDir"
    Write-Host "Streamlink : $streamlinkExe"
    Write-Host "Cloudflare : $workerUrl"
    Write-Host "AID/URL    : CLOUDFLARE WORKER (master HLS)"
    Write-Host "Media      : DIRECT"
    Write-Host "Stall      : $stallTimeout sec"
    Write-Host "Disk Limit : $minFreeSpaceGB GB"
    Write-Host "SOOP Auth  : $(if ($script:SoopAuthCookies.Count -gt 0) { 'OK' } else { 'NONE' })"
    Write-Host "Worker Try : $workerMaxRetry"
    Write-Host "Console    : $consoleRefreshInterval sec refresh"
    Write-Host "Daily Log  : $(if ($script:LogEnabled) { 'ON' } else { 'OFF' })"
    if ($script:LogEnabled) { Write-Host "Log Dir    : $script:LogDir" }
    Write-Host ""

    $currentChannels = @()
    $lastChannelWriteTimeUtc = $null
    $stateKeyByAccount = @{}
    $nextHotConfigCheck = Get-Date
    $nextReload = Get-Date
    $nextDashboard = Get-Date

    # Memory usage is written to the daily log at every top of the hour.
    # It is skipped entirely while LOG_ENABLED=N.
    $nextMemoryLog = Get-NextMemoryLogTime

    while ($true) {
        $now = Get-Date

        if ($now -ge $nextHotConfigCheck) {
            Update-HotConfig
            $nextHotConfigCheck = $now.AddSeconds(1)
        }
        Process-ControlCommands -States $states -StateKeyByAccount $stateKeyByAccount

        # Refresh loop-local references after hot reload.
        $checkInterval = $script:checkInterval
        $reloadInterval = $script:reloadInterval
        $retryInterval = $script:retryInterval
        $stallTimeout = $script:stallTimeout
        $monitorInterval = $script:monitorInterval
        $workerMaxRetry = $script:workerMaxRetry
        $consoleRefreshInterval = $script:consoleRefreshInterval
        $minFreeSpaceGB = $script:minFreeSpaceGB
        $streamQuality = $script:streamQuality
        $workerUrl = $script:workerUrl
        $workerApiKey = $script:workerApiKey
        $soopUsername = $script:soopUsername
        $soopPassword = $script:soopPassword
        $purgeCredentials = $script:purgeCredentials

        # ----------------------------------------------------
        # Hourly watcher memory log
        # ----------------------------------------------------
        if ($now -ge $nextMemoryLog) {
            if ($script:LogEnabled) {
                Write-WatcherMemoryLog
            }

            # Always advance the timer even while logging is disabled.
            # If logging is turned back on, the next top-of-hour entry resumes.
            $nextMemoryLog = Get-NextMemoryLogTime -From $now
        }

        # ----------------------------------------------------
        # Hot reload channel list
        # ----------------------------------------------------
        if ($now -ge $nextReload) {
            $newChannels = $currentChannels
            try {
                $channelItem = Get-Item -LiteralPath $ChannelFile -ErrorAction Stop
                if ($null -eq $lastChannelWriteTimeUtc -or
                    $channelItem.LastWriteTimeUtc -ne $lastChannelWriteTimeUtc) {
                    $loadedChannels = Get-SoopChannelList $ChannelFile
                    if ($null -ne $loadedChannels) {
                        $currentChannels = @($loadedChannels)
                        $newChannels = $currentChannels
                        $lastChannelWriteTimeUtc = $channelItem.LastWriteTimeUtc
                    }
                    else {
                        $newChannels = $null
                    }
                }
            }
            catch {
                Write-Host "[WARN] Channel file metadata check failed; previous list will be kept."
                $newChannels = $null
            }

            if ($null -ne $newChannels) {
                $newByUrl = @{}
                $stateKeyByAccount.Clear()

                foreach ($ch in $newChannels) {
                    $newByUrl[$ch.Url] = $ch
                    $accountIndexKey = ([string]$ch.Account).ToLowerInvariant()
                    $stateKeyByAccount[$accountIndexKey] = $ch.Url

                    if (-not $states.ContainsKey($ch.Url)) {
                        $states[$ch.Url] = [PSCustomObject]@{
                            Name      = $ch.Name
                            Enabled   = $ch.Enabled
                            Channel   = $ch
                            Recording = $null
                            LastBno       = $null
                            SuppressedBno = $null
                            NextCheck     = Get-Date
                            LowDisk       = $false
                            NextDiskCheck = Get-Date
                            LastStatus    = "UNKNOWN"
                        }
                    }
                    else {
                        $state = $states[$ch.Url]
                        $wasEnabled = $state.Enabled

                        $state.Name = $ch.Name
                        $state.Enabled = $ch.Enabled
                        $state.Channel = $ch

                        # Y -> N: stop only this recording.
                        # If termination fails, keep the in-memory state enabled
                        # so the next hot-reload pass retries instead of silently
                        # forgetting a still-running recorder process.
                        if ($wasEnabled -and -not $ch.Enabled) {
                            $stopped = Stop-ChannelRecording `
                                -Key $ch.Url `
                                -Reason "CHANNEL DISABLED"

                            if (-not $stopped) {
                                $state.Enabled = $true
                                Write-LogMessage (
                                    "CHANNEL DISABLE FAILED channel={0}; recorder is still alive; retry on next reload" -f `
                                    $state.Name
                                ) -Level "ERROR"

                                Write-Host (
                                    "[ERROR] {0} : disable requested but recorder stop failed; will retry" -f `
                                    $state.Name
                                )
                            }
                            else {
                                Write-Host (
                                    "[{0}] {1} [account={2}] : CHANNEL DISABLED" -f `
                                    (Get-Date -Format "HH:mm:ss"),
                                    $state.Name,
                                    $state.Channel.Account
                                )
                            }
                        }

                        # N -> Y: check immediately, even if same BNO.
                        if (-not $wasEnabled -and $ch.Enabled) {
                            $state.NextCheck = Get-Date
                        }
                    }
                }

                # Deleted row = stop watching + stop this channel only.
                # Never remove the state until its owned recorder is confirmed
                # stopped. Keeping the state lets the next reload retry.
                foreach ($key in @($states.Keys)) {
                    if (-not $newByUrl.ContainsKey($key)) {
                        $removedState = $states[$key]
                        $stopped = Stop-ChannelRecording `
                            -Key $key `
                            -Reason "CHANNEL REMOVED"

                        if ($stopped) {
                            Write-Host (
                                "[{0}] {1} [account={2}] : CHANNEL REMOVED" -f `
                                (Get-Date -Format "HH:mm:ss"),
                                $removedState.Name,
                                $removedState.Channel.Account
                            )
                            $states.Remove($key)
                        }
                        else {
                            $states[$key].Enabled = $true
                            Write-LogMessage (
                                "CHANNEL REMOVE FAILED channel={0}; recorder is still alive; state retained for retry" -f `
                                $states[$key].Name
                            ) -Level "ERROR"

                            Write-Host (
                                "[ERROR] {0} : remove requested but recorder stop failed; state retained for retry" -f `
                                $states[$key].Name
                            )
                        }
                    }
                }

                $currentChannels = $newChannels
            }

            $nextReload = (Get-Date).AddSeconds($reloadInterval)
        }

        # ----------------------------------------------------
        # Check recorder process exits
        # ----------------------------------------------------
        Update-RecordingStates `
            -RetryInterval $retryInterval `
            -StallTimeout $stallTimeout `
            -MonitorInterval $monitorInterval `
            -MinFreeSpaceGB $minFreeSpaceGB

        # ----------------------------------------------------
        # Live status checks
        # ----------------------------------------------------
        foreach ($key in @($states.Keys)) {
            $state = $states[$key]

            if (-not $state.Enabled) {
                continue
            }

            if ($state.LowDisk) {
                if ((Get-Date) -ge $state.NextDiskCheck) {
                    $outputDirForDisk = Get-ChannelOutputDirectory `
                        -Channel $state.Channel `
                        -DefaultOutputDir $defaultOutputDir

                    $disk = Test-LowDiskSpace `
                        -Path $outputDirForDisk `
                        -MinFreeGB $minFreeSpaceGB

                    if (-not $disk.IsLow) {
                        Write-Host (
                            "[{0}] {1} [account={2}] : DISK SPACE OK ({3} {4} GB free)" -f `
                            (Get-Date -Format "HH:mm:ss"),
                            $state.Name,
                            $state.Channel.Account,
                            $disk.Root,
                            $disk.FreeGB
                        )

                        Write-LogMessage ("DISK SPACE OK channel={0} drive={1} free={2}GB" -f $state.Name,$disk.Root,$disk.FreeGB)
                        $state.LowDisk = $false
                        $state.LastStatus = "UNKNOWN"
                        $state.NextCheck = Get-Date
                    }
                    else {
                        Write-Host (
                            "[{0}] {1} [account={2}] : LOW DISK ({3} {4} GB / limit {5} GB)" -f `
                            (Get-Date -Format "HH:mm:ss"),
                            $state.Name,
                            $state.Channel.Account,
                            $disk.Root,
                            $disk.FreeGB,
                            $minFreeSpaceGB
                        )

                        $state.NextDiskCheck = (Get-Date).AddSeconds(30)
                    }
                }

                continue
            }

            if ($null -ne $state.Recording) {
                continue
            }

            if ((Get-Date) -lt $state.NextCheck) {
                continue
            }

            $channel = $state.Channel
            $live = Get-SoopLiveInfo $channel

            if (-not [string]::IsNullOrWhiteSpace($live.Error)) {
                Write-Host (
                    "[{0}] {1} [account={2}] : CHECK ERROR - {3}" -f `
                    (Get-Date -Format "HH:mm:ss"),
                    $channel.Name,
                    $channel.Account,
                    $live.Error
                )

                $state.LastStatus = "ERROR"
                $state.NextCheck = (Get-Date).AddSeconds($checkInterval)
                continue
            }

            if ($live.AuthRequired) {
                Write-Host (
                    "[{0}] {1} [account={2}] : LOGIN REQUIRED - refreshing SOOP session" -f `
                    (Get-Date -Format "HH:mm:ss"),
                    $channel.Name,
                    $channel.Account
                )

                try {
                    Initialize-SoopLogin `
                        -Username $soopUsername `
                        -Password $soopPassword | Out-Null

                    $state.NextCheck = Get-Date
                }
                catch {
                    Write-Host (
                        "[{0}] {1} : LOGIN FAILED - {2}" -f `
                        (Get-Date -Format "HH:mm:ss"),
                        $channel.Name,
                        $_.Exception.Message
                    )

                    $state.NextCheck = (Get-Date).AddSeconds($checkInterval)
                }

                continue
            }

            if (-not $live.IsLive) {
$state.LastStatus = "OFFLINE"
                $state.LastBno = $null
                $state.NextCheck = (Get-Date).AddSeconds($checkInterval)
                continue
            }

            # If profile lookup previously failed and ACCOUNT was stored as the
            # fallback NAME, promote the live API nickname for this runtime.
            # Custom/persisted names are never overwritten automatically.
            if (
                [string]::Equals(
                    [string]$channel.Name,
                    [string]$channel.Account,
                    [StringComparison]::OrdinalIgnoreCase
                ) -and
                -not [string]::IsNullOrWhiteSpace([string]$live.BjNick)
            ) {
                $channel.Name = [string]$live.BjNick
                $state.Name = [string]$live.BjNick
            }

            try {
                if (-not [string]::IsNullOrWhiteSpace([string]$state.SuppressedBno)) {
                    if ([string]$live.Bno -eq [string]$state.SuppressedBno) {
                        $state.NextCheck = (Get-Date).AddSeconds($checkInterval)
                        continue
                    }
                    $state.SuppressedBno = $null
                }

                $recording = Start-ChannelRecording `
                    -Channel $channel `
                    -LiveInfo $live `
                    -StreamlinkExe $streamlinkExe `
                    -DefaultOutputDir $defaultOutputDir `
                    -StreamQuality $streamQuality `
                    -MasterQuality $masterQuality `
                    -WorkerUrl $workerUrl `
                    -WorkerApiKey $workerApiKey `
                    -AuthCookies $script:SoopAuthCookies `
                    -MinFreeSpaceGB $minFreeSpaceGB `
                    -SoopUsername $soopUsername `
                    -SoopPassword $soopPassword `
                    -WorkerMaxRetry $workerMaxRetry

                $state.Recording = $recording
                $state.LastStatus = "RECORDING"
                $state.LastBno = $live.Bno
                $state.NextCheck = (Get-Date).AddSeconds($checkInterval)
            }
            catch {
                if ($_.Exception.Message -match '^\[WORKER_CIRCUIT_OPEN\]\s+seconds=(?<seconds>\d+)\s+until=(?<until>\S+)') {
                    $cooldownSeconds = [int]$Matches['seconds']
                    Write-Host (
                        "[{0}] {1} [account={2}] : WORKER COOLDOWN - seconds={3} until={4}" -f `
                        (Get-Date -Format "HH:mm:ss"),
                        $channel.Name,
                        $channel.Account,
                        $cooldownSeconds,
                        $Matches['until']
                    )
                    $state.NextCheck = (Get-Date).AddSeconds($cooldownSeconds)
                    continue
                }
                Write-LogMessage ("RECORD START FAILED channel={0} error={1}" -f $channel.Name,$_.Exception.Message) -Level "ERROR"
                Write-Host (
                    "[{0}] {1} [account={2}] : RECORD START FAILED - {3}" -f `
                    (Get-Date -Format "HH:mm:ss"),
                    $channel.Name,
                    $channel.Account,
                    $_.Exception.Message
                )

                $state.NextCheck = (Get-Date).AddSeconds($retryInterval)
            }
        }


        if ((Get-Date) -ge $nextDashboard) {
            Show-ConsoleDashboard `
                -States $states `
                -Now (Get-Date)

            $nextDashboard = (Get-Date).AddSeconds($consoleRefreshInterval)
        }

        Start-Sleep -Milliseconds 500
    }
}
catch {
    $scriptExitCode = 1
    Write-LogMessage ("WATCHER ERROR: " + $_.Exception.Message) -Level "ERROR"

    Write-Host ""
    Show-Line
    Write-Host " ERROR"
    Show-Line
    Write-Host ""
    Write-Host $_.Exception.Message
}
finally {
    # Stop only processes started by this watcher.
    foreach ($key in @($states.Keys)) {
        Stop-ChannelRecording `
            -Key $key `
            -Reason "WATCHER EXIT"
    }

    Write-LogMessage "Watcher stopped exitCode=$scriptExitCode"

    if ($HttpClient) {
        try {
            $HttpClient.Dispose()
        }
        catch {}
    }

    if ($script:WatcherMutex) {
        if ($script:WatcherMutexAcquired) {
            try { $script:WatcherMutex.ReleaseMutex() } catch {}
        }
        try { $script:WatcherMutex.Dispose() } catch {}
    }
}

exit $scriptExitCode
