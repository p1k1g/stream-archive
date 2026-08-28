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
$RecorderDiagnosticDir = Join-Path $ScriptDir "logs\recorder-diagnostics"
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

# Feature modules contain function definitions only. Fail before watcher startup
# if a publish/synchronization step omitted any required backend module.
$BackendModuleDir = Join-Path $ScriptDir "modules"
foreach ($moduleName in @("SOOP.Security.ps1","SOOP.Core.ps1","SOOP.Network.ps1","SOOP.Recorder.ps1")) {
    $modulePath = Join-Path $BackendModuleDir $moduleName
    if (-not (Test-Path -LiteralPath $modulePath -PathType Leaf)) {
        throw "필수 backend 모듈이 없습니다: $modulePath"
    }
    . $modulePath
}

# ------------------------------------------------------------
# Main
# ------------------------------------------------------------

try {
    if (-not (Test-Path -LiteralPath $SettingFile -PathType Leaf)) {
        throw "SOOP_LIVE_SETTING.ini 파일이 없습니다."
    }

    if (-not (Test-Path -LiteralPath $ChannelFile -PathType Leaf)) {
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
                                Write-GuiEvent -Type "channel_disabled" -Data @{
                                    account = $state.Channel.Account
                                    name = $state.Name
                                    action = "disabled"
                                }
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
                            Write-GuiEvent -Type "channel_removed" -Data @{
                                account = $removedState.Channel.Account
                                name = $removedState.Name
                                action = "removed"
                            }
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
                    Write-GuiEvent -Type "worker_cooldown" -Data @{
                        account = $channel.Account
                        name = $channel.Name
                        cooldownSeconds = $cooldownSeconds
                        until = $Matches['until']
                        detail = ("seconds={0} until={1}" -f $cooldownSeconds,$Matches['until'])
                    }
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
