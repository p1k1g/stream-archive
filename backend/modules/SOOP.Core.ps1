# SOOP LIVE backend module - dot-sourced by SOOP_LIVE.ps1

function Show-Line {
    Write-Host "========================================"
}

function Write-GuiEvent {
    param([string]$Type,[hashtable]$Data = @{})

    try {
        $payload = [ordered]@{
            version   = 1
            type      = $Type
            timestamp = (Get-Date).ToString("o")
        }
        foreach ($key in $Data.Keys) {
            $payload[$key] = $Data[$key]
        }
        Write-Host ("@@SOOP_EVENT@@" + ($payload | ConvertTo-Json -Compress -Depth 6))
    }
    catch {
        Write-LogMessage ("GUI EVENT SERIALIZE FAILED type={0} error={1}" -f $Type,$_.Exception.Message) -Level "WARN"
    }
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

    return (Resolve-ProtectedConfigSecrets -Config $config)
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
