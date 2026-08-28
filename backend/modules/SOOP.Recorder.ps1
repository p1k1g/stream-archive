# SOOP LIVE backend module - dot-sourced by SOOP_LIVE.ps1

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

function Get-RecorderExitReason {
    param($ExitCode)

    # A completed Process can occasionally lose its native exit-code handle
    # before PowerShell reads ExitCode. An unavailable value previously became
    # the misleading string "RECORDER EXIT CODE=" and left a GUI alert even
    # after a complete recording. Known non-zero codes remain actionable.
    if ($null -eq $ExitCode -or [string]::IsNullOrWhiteSpace([string]$ExitCode)) {
        return "NORMAL"
    }

    $numericCode = 0
    if ([int]::TryParse([string]$ExitCode,[ref]$numericCode) -and $numericCode -eq 0) {
        return "NORMAL"
    }
    return "RECORDER EXIT CODE=" + [string]$ExitCode
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

    Write-GuiEvent -Type "recording_finished" -Data @{
        account = $State.Channel.Account
        name = $State.Name
        duration = (Format-Duration $duration)
        durationSeconds = [Math]::Max(0,[Math]::Floor($duration.TotalSeconds))
        size = (Format-BytesHuman $size)
        sizeBytes = $size
        reason = $Reason
        file = $Recording.File
        diagnosticFile = [string]$Recording.DiagnosticFile
    }

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

    while (Test-Path -LiteralPath $candidate) {
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

function Protect-RecorderDiagnosticText {
    param([string]$Text)
    if ([string]::IsNullOrWhiteSpace($Text)) { return "" }

    $safe = $Text
    $safe = $safe -replace '(?im)^.*(?:SOOP_PASSWORD|WORKER_API_KEY|API_KEY|AUTHORIZATION|COOKIE)\s*[:=].*$', '<redacted sensitive header>'
    $safe = $safe -replace '(?i)(SOOP_PASSWORD|WORKER_API_KEY|API_KEY|AUTHORIZATION|COOKIE)\s*[:=]\s*[^\s;]+', '$1=<redacted>'
    $safe = $safe -replace '(?i)(Bearer|Basic)\s+[A-Za-z0-9+/=_\-.]+', '$1 <redacted>'
    $safe = $safe -replace '(?i)([?&](?:aid|token|key|apikey|api_key|worker_api_key|password|passwd)=)[^&\s]+', '$1<redacted>'
    return $safe
}

function Save-RecorderErrorDiagnostic {
    param($State,$Recording,[string]$Reason)

    if ($null -eq $Recording -or [string]::IsNullOrWhiteSpace([string]$Recording.StderrFile)) {
        return ""
    }

    try {
        if (-not (Test-Path -LiteralPath $Recording.StderrFile -PathType Leaf)) {
            return ""
        }

        $tail = @(Get-Content -LiteralPath $Recording.StderrFile -Tail 50 -Encoding UTF8 -ErrorAction Stop)
        if ($tail.Count -eq 0) { return "" }

        New-Item -ItemType Directory -Path $RecorderDiagnosticDir -Force | Out-Null
        $account = Get-SafeFileName -Name ([string]$State.Channel.Account) -MaxLength 40
        $reasonName = Get-SafeFileName -Name $Reason -MaxLength 40
        $diagnosticPath = Join-Path $RecorderDiagnosticDir (
            "{0}_{1}_{2}.log" -f (Get-Date -Format "yyyyMMdd_HHmmss"),$account,$reasonName
        )
        $header = @(
            "SOOP LIVE recorder diagnostic"
            "Time: $((Get-Date).ToString('o'))"
            "Account: $($State.Channel.Account)"
            "Channel: $($State.Name)"
            "Reason: $Reason"
            "Output: $($Recording.File)"
            "--- stderr tail (last 50 lines) ---"
        )
        $content = Protect-RecorderDiagnosticText (($header + $tail) -join [Environment]::NewLine)
        Set-Content -LiteralPath $diagnosticPath -Value $content -Encoding UTF8 -ErrorAction Stop

        @(Get-ChildItem -LiteralPath $RecorderDiagnosticDir -Filter "*.log" -File -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -Skip 20) |
            Remove-Item -Force -ErrorAction SilentlyContinue

        Write-LogMessage ("RECORDER DIAGNOSTIC SAVED channel={0} file={1}" -f $State.Name,$diagnosticPath) -Level "WARN"
        return $diagnosticPath
    }
    catch {
        Write-LogMessage ("RECORDER DIAGNOSTIC SAVE FAILED channel={0} error={1}" -f $State.Name,$_.Exception.Message) -Level "WARN"
        return ""
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

    Write-GuiEvent -Type "recording_started" -Data @{
        account = $Channel.Account
        name = $Channel.Name
        title = $LiveInfo.Title
        bno = [string]$LiveInfo.Bno
        file = $outputFile
        quality = $streamInfo.Quality
    }

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
        DiagnosticFile     = ""
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
    if ($Reason -eq "RECORD STALLED") {
        $rec.DiagnosticFile = Save-RecorderErrorDiagnostic -State $state -Recording $rec -Reason $Reason
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
            try {
                [void]$rec.Process.WaitForExit(1000)
                $rec.Process.Refresh()
                $exitCode = $rec.Process.ExitCode
            }
            catch {}
            $exitReason = Get-RecorderExitReason -ExitCode $exitCode

            Write-Host (
                "[{0}] RECORDER EXIT {1} PID={2} CODE={3}" -f `
                $now.ToString("HH:mm:ss"),
                $state.Name,
                $rec.PID,
                $exitCode
            )

            if ($exitReason -ne "NORMAL") {
                $rec.DiagnosticFile = Save-RecorderErrorDiagnostic `
                    -State $state `
                    -Recording $rec `
                    -Reason $exitReason
            }

            Write-RecordingSummary `
                -State $state `
                -Recording $rec `
                -Reason $exitReason

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
            Write-GuiEvent -Type "low_disk" -Data @{
                account = $state.Channel.Account
                name = $state.Name
                drive = $disk.Root
                freeGB = $disk.FreeGB
                limitGB = $MinFreeSpaceGB
                detail = ("Free={0} GB / Limit={1} GB" -f $disk.FreeGB,$MinFreeSpaceGB)
            }
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
            Write-GuiEvent -Type "recording_stalled" -Data @{
                account = $state.Channel.Account
                name = $state.Name
                file = $rec.File
                noGrowthSeconds = $StallTimeout
                detail = ("{0}s no growth" -f $StallTimeout)
            }
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
            elseif($action -eq "RECHECK"){
                $state.NextCheck=Get-Date
                Write-LogMessage ("CHANNEL RECHECK REQUESTED channel={0} account={1}" -f $state.Name,$account)
                Write-Host ("[{0}] {1} [account={2}] : CHANNEL RECHECK REQUESTED" -f (Get-Date -Format "HH:mm:ss"),$state.Name,$account)
            }
        }catch{
            Write-LogMessage ("CONTROL COMMAND FAILED file={0} error={1}" -f $file.Name,$_.Exception.Message) -Level "WARN"
        }finally{
            try{if(Test-Path -LiteralPath $claimed){Remove-Item -LiteralPath $claimed -Force -ErrorAction SilentlyContinue}}catch{}
        }
    }
}
