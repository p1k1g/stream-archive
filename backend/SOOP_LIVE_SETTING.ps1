$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$IniPath = Join-Path $ScriptDir "SOOP_LIVE_SETTING.ini"
$ChannelPath = Join-Path $ScriptDir "SOOP_LIVE_CHANNELS.txt"

function Pause-Menu {
    Write-Host ""
    [void](Read-Host "Press Enter to continue")
}

function Read-Ini {
    param([string]$Path = $IniPath)

    $cfg = @{}

    if ([string]::IsNullOrWhiteSpace($Path)) {
        $Path = $IniPath
    }

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $cfg
    }

    $lines = @(Get-Content -LiteralPath $Path -Encoding UTF8)

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

        # Repair compatibility for older broken settings:
        # KEY=
        # VALUE
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

        $cfg[$key] = $value
    }

    return $cfg
}

function Get-Cfg {
    param(
        [hashtable]$Config,
        [string]$Key,
        [string]$Default = ""
    )

    $k = $Key.ToUpperInvariant()

    if ($Config.ContainsKey($k)) {
        return [string]$Config[$k]
    }

    return $Default
}

function Read-Keep {
    param(
        [string]$Label,
        [string]$Current,
        [switch]$Secret
    )

    if ($Secret) {
        if ([string]::IsNullOrWhiteSpace($Current)) {
            $display = "<not set>"
        }
        else {
            $display = "<configured>"
        }

        $value = Read-Host "$Label [$display] (Enter = keep)"
    }
    else {
        $value = Read-Host "$Label [$Current]"
    }

    if ([string]::IsNullOrEmpty($value)) {
        return $Current
    }

    return $value
}

function Write-Ini {
    param([hashtable]$Config)

    $lines = @(
        "# SOOP LIVE Downloader Settings - Cloudflare",
        "",
        "CHECK_INTERVAL=$($Config.CHECK_INTERVAL)",
        "CHANNEL_RELOAD_INTERVAL=$($Config.CHANNEL_RELOAD_INTERVAL)",
        "RECORD_RETRY_INTERVAL=$($Config.RECORD_RETRY_INTERVAL)",
        "RECORD_STALL_TIMEOUT=$($Config.RECORD_STALL_TIMEOUT)",
        "RECORD_MONITOR_INTERVAL=$($Config.RECORD_MONITOR_INTERVAL)",
        "WORKER_MAX_RETRY=$($Config.WORKER_MAX_RETRY)",
        "CONSOLE_REFRESH_INTERVAL=$($Config.CONSOLE_REFRESH_INTERVAL)",
        "MIN_FREE_SPACE_GB=$($Config.MIN_FREE_SPACE_GB)",
        "",
        "OUTPUT_DIR=$($Config.OUTPUT_DIR)",
        "QUALITY=$($Config.QUALITY)",
        "",
        "STREAMLINK_PATH=AUTO",
        "STREAMLINK_FALLBACK=$($Config.STREAMLINK_FALLBACK)",
        "",
        "SOOP_USERNAME=$($Config.SOOP_USERNAME)",
        "SOOP_PASSWORD=$($Config.SOOP_PASSWORD)",
        "SOOP_PURGE_CREDENTIALS=Y",
        "",
        "CLOUDFLARE_WORKER_URL=$($Config.CLOUDFLARE_WORKER_URL)",
        "CLOUDFLARE_API_KEY=$($Config.CLOUDFLARE_API_KEY)",
        "",
        "MASTER_QUALITY=auto",
        "",
        "LOG_ENABLED=$($Config.LOG_ENABLED)",
        "LOG_DIR=$($Config.LOG_DIR)",
        "LOG_RETENTION_DAYS=$($Config.LOG_RETENTION_DAYS)"
    )

    [IO.File]::WriteAllLines(
        $IniPath,
        $lines,
        (New-Object Text.UTF8Encoding($true))
    )
}

function Ensure-ChannelFile {
    if (Test-Path -LiteralPath $ChannelPath -PathType Leaf) {
        return
    }

    $lines = @(
        "# =========================================================",
        "# SOOP LIVE Channel List",
        "# =========================================================",
        "#",
        "# ENABLED|NAME|ACCOUNT|OUTDIR",
        "#",
        "# Y|Channel1|1004ysus|",
        "# Y|Channel2|account2|D:\SOOP_RECORD",
        "# N|Channel3|account3|",
        ""
    )

    [IO.File]::WriteAllLines(
        $ChannelPath,
        $lines,
        (New-Object Text.UTF8Encoding($true))
    )
}

function Get-ChannelRows {
    Ensure-ChannelFile

    $all = @(Get-Content -LiteralPath $ChannelPath -Encoding UTF8)

    $rows = @()

    for ($i = 0; $i -lt $all.Count; $i++) {
        $line = ([string]$all[$i]).Trim()

        if (
            [string]::IsNullOrWhiteSpace($line) -or
            $line.StartsWith("#")
        ) {
            continue
        }

        $parts = $line.Split("|")

        $rows += [PSCustomObject]@{
            FileIndex = $i
            Raw       = $all[$i]
            Enabled   = if ($parts.Count -gt 0) { $parts[0].Trim() } else { "" }
            Name      = if ($parts.Count -gt 1) { $parts[1].Trim() } else { "" }
            Account   = if ($parts.Count -gt 2) { $parts[2].Trim() } else { "" }
            OutDir    = if ($parts.Count -gt 3) { $parts[3].Trim() } else { "" }
        }
    }

    return $rows
}

function Show-Channels {
    $rows = @(Get-ChannelRows)

    if ($rows.Count -eq 0) {
        Write-Host " No channels configured."
        return
    }

    Write-Host (" {0,-3} {1,-8} {2,-20} {3,-20} {4}" -f `
        "No", "Enabled", "Name", "Account", "OUTDIR")
    Write-Host " --- -------- -------------------- -------------------- -------------------------"

    for ($i = 0; $i -lt $rows.Count; $i++) {
        $r = $rows[$i]

        Write-Host (" {0,-3} {1,-8} {2,-20} {3,-20} {4}" -f `
            ($i + 1),
            $r.Enabled,
            $r.Name,
            $r.Account,
            $r.OutDir)
    }
}

function Save-SettingsMenu {
    param(
        [hashtable]$Cfg,
        [string]$Message = "Settings saved successfully"
    )

    Write-Ini $Cfg
    Write-Host ""
    Write-Host "========================================"
    Write-Host " $Message"
    Write-Host "========================================"
    Write-Host $IniPath
    Pause-Menu
}

function Edit-AllSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath

    Write-Host ""
    Write-Host "========================================"
    Write-Host " All Settings"
    Write-Host "========================================"
    Write-Host ""
    Write-Host "Configure all settings (recommended for first setup)."
    Write-Host "Press Enter to keep the current value."
    Write-Host ""

    $cfg.OUTPUT_DIR = Read-Keep "Default Output Directory [RESTART]" $cfg.OUTPUT_DIR
    $cfg.CHECK_INTERVAL = Read-Keep "Live Check Interval sec [HOT]" $cfg.CHECK_INTERVAL
    $cfg.CHANNEL_RELOAD_INTERVAL = Read-Keep "Channel List Reload sec [HOT]" $cfg.CHANNEL_RELOAD_INTERVAL
    $cfg.RECORD_RETRY_INTERVAL = Read-Keep "Recorder Retry sec [HOT]" $cfg.RECORD_RETRY_INTERVAL
    $cfg.RECORD_STALL_TIMEOUT = Read-Keep "No File Growth Timeout sec [HOT]" $cfg.RECORD_STALL_TIMEOUT
    $cfg.RECORD_MONITOR_INTERVAL = Read-Keep "Recorder Monitor Interval sec [HOT]" $cfg.RECORD_MONITOR_INTERVAL
    $cfg.WORKER_MAX_RETRY = Read-Keep "Worker Max Retry [HOT]" $cfg.WORKER_MAX_RETRY
    $cfg.CONSOLE_REFRESH_INTERVAL = Read-Keep "Console Refresh Interval sec [HOT]" $cfg.CONSOLE_REFRESH_INTERVAL
    $cfg.MIN_FREE_SPACE_GB = Read-Keep "Minimum Free Disk Space GB [HOT]" $cfg.MIN_FREE_SPACE_GB
    $cfg.QUALITY = Read-Keep "Streamlink Quality [NEXT RECORD]" $cfg.QUALITY
    $cfg.STREAMLINK_PATH = Read-Keep "Streamlink Path [RESTART]" $cfg.STREAMLINK_PATH
    $cfg.STREAMLINK_FALLBACK = Read-Keep "Streamlink Fallback EXE [RESTART]" $cfg.STREAMLINK_FALLBACK

    Write-Host ""
    $cfg.SOOP_USERNAME = Read-Keep "SOOP Username [HOT]" $cfg.SOOP_USERNAME
    $cfg.SOOP_PASSWORD = Read-Keep "SOOP Password [HOT]" $cfg.SOOP_PASSWORD -Secret
    $cfg.SOOP_PURGE_CREDENTIALS = Read-Keep "SOOP Purge Credentials Y/N [HOT]" $cfg.SOOP_PURGE_CREDENTIALS

    Write-Host ""
    $cfg.CLOUDFLARE_WORKER_URL = Read-Keep "Cloudflare Worker URL [HOT]" $cfg.CLOUDFLARE_WORKER_URL
    $cfg.CLOUDFLARE_API_KEY = Read-Keep "Cloudflare API Key [HOT]" $cfg.CLOUDFLARE_API_KEY -Secret

    Write-Host ""
    $cfg.LOG_ENABLED = Read-Keep "Daily Log Y/N [HOT]" $cfg.LOG_ENABLED
    $cfg.LOG_DIR = Read-Keep "Daily Log Directory [HOT]" $cfg.LOG_DIR
    $cfg.LOG_RETENTION_DAYS = Read-Keep "Log Retention Days [HOT]" $cfg.LOG_RETENTION_DAYS

    Save-SettingsMenu -Cfg $cfg
}

function Edit-RuntimeSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath
    Write-Host ""
    Write-Host "========================================"
    Write-Host " Runtime / Hot Reload Settings"
    Write-Host "========================================"
    Write-Host ""

    $cfg.CHECK_INTERVAL = Read-Keep "Live Check Interval sec [HOT]" $cfg.CHECK_INTERVAL
    $cfg.CHANNEL_RELOAD_INTERVAL = Read-Keep "Channel List Reload sec [HOT]" $cfg.CHANNEL_RELOAD_INTERVAL
    $cfg.RECORD_RETRY_INTERVAL = Read-Keep "Recorder Retry sec [HOT]" $cfg.RECORD_RETRY_INTERVAL
    $cfg.RECORD_STALL_TIMEOUT = Read-Keep "No File Growth Timeout sec [HOT]" $cfg.RECORD_STALL_TIMEOUT
    $cfg.RECORD_MONITOR_INTERVAL = Read-Keep "Recorder Monitor Interval sec [HOT]" $cfg.RECORD_MONITOR_INTERVAL
    $cfg.WORKER_MAX_RETRY = Read-Keep "Worker Max Retry [HOT]" $cfg.WORKER_MAX_RETRY
    $cfg.CONSOLE_REFRESH_INTERVAL = Read-Keep "Console Refresh Interval sec [HOT]" $cfg.CONSOLE_REFRESH_INTERVAL
    $cfg.MIN_FREE_SPACE_GB = Read-Keep "Minimum Free Disk Space GB [HOT]" $cfg.MIN_FREE_SPACE_GB

    Save-SettingsMenu -Cfg $cfg -Message "Hot Reload Settings saved"
}

function Edit-RecordingSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath
    Write-Host ""
    Write-Host "========================================"
    Write-Host " Recording Settings"
    Write-Host "========================================"
    Write-Host ""

    $cfg.OUTPUT_DIR = Read-Keep "Default Output Directory [RESTART]" $cfg.OUTPUT_DIR
    $cfg.QUALITY = Read-Keep "Streamlink Quality [NEXT RECORD]" $cfg.QUALITY
    $cfg.STREAMLINK_PATH = Read-Keep "Streamlink Path [RESTART]" $cfg.STREAMLINK_PATH
    $cfg.STREAMLINK_FALLBACK = Read-Keep "Streamlink Fallback EXE [RESTART]" $cfg.STREAMLINK_FALLBACK

    Save-SettingsMenu -Cfg $cfg
}

function Edit-SoopLoginSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath
    Write-Host ""
    Write-Host "========================================"
    Write-Host " SOOP Login Settings"
    Write-Host "========================================"
    Write-Host ""
    Write-Host "Changes are hot reloaded. Active recordings are not restarted."
    Write-Host ""

    $cfg.SOOP_USERNAME = Read-Keep "SOOP Username [HOT]" $cfg.SOOP_USERNAME
    $cfg.SOOP_PASSWORD = Read-Keep "SOOP Password [HOT]" $cfg.SOOP_PASSWORD -Secret
    $cfg.SOOP_PURGE_CREDENTIALS = Read-Keep "SOOP Purge Credentials Y/N [HOT]" $cfg.SOOP_PURGE_CREDENTIALS

    Save-SettingsMenu -Cfg $cfg
}

function Edit-WorkerSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath
    Write-Host ""
    Write-Host "========================================"
    Write-Host " Cloudflare Worker Settings"
    Write-Host "========================================"
    Write-Host ""
    Write-Host "Changes are used from the next Worker request."
    Write-Host ""

    $cfg.CLOUDFLARE_WORKER_URL = Read-Keep "Cloudflare Worker URL [HOT]" $cfg.CLOUDFLARE_WORKER_URL
    $cfg.CLOUDFLARE_API_KEY = Read-Keep "Cloudflare API Key [HOT]" $cfg.CLOUDFLARE_API_KEY -Secret

    Save-SettingsMenu -Cfg $cfg
}

function Edit-LogSettings {
    Clear-Host
    $cfg = Read-Ini $IniPath
    Write-Host ""
    Write-Host "========================================"
    Write-Host " Log Settings"
    Write-Host "========================================"
    Write-Host ""
    Write-Host "Changes are hot reloaded."
    Write-Host "Changing LOG_DIR moves subsequent log entries to the new directory."
    Write-Host ""

    $cfg.LOG_ENABLED = Read-Keep "Daily Log Y/N [HOT]" $cfg.LOG_ENABLED
    $cfg.LOG_DIR = Read-Keep "Daily Log Directory [HOT]" $cfg.LOG_DIR
    $cfg.LOG_RETENTION_DAYS = Read-Keep "Log Retention Days [HOT]" $cfg.LOG_RETENTION_DAYS

    Save-SettingsMenu -Cfg $cfg
}

function Edit-IniDirectly {
    Start-Process notepad.exe -ArgumentList "`"$IniPath`"" -Wait
}

function General-Settings {
    while ($true) {
        Clear-Host
        Write-Host ""
        Write-Host "========================================"
        Write-Host " General Settings"
        Write-Host "========================================"
        Write-Host ""
        Write-Host " 1. All Settings"
        Write-Host "    Configure all settings (recommended for first setup)"
        Write-Host " 2. Runtime / Hot Reload Settings"
        Write-Host " 3. Recording Settings"
        Write-Host " 4. SOOP Login Settings"
        Write-Host " 5. Cloudflare Worker Settings"
        Write-Host " 6. Log Settings"
        Write-Host " 7. Edit INI Directly"
        Write-Host " 8. Back"
        Write-Host ""
        Write-Host " [HOT]         Applied while watcher is running"
        Write-Host " [NEXT RECORD] Applied to newly started recordings"
        Write-Host " [RESTART]     Watcher restart required"
        Write-Host ""

        $sel = Read-Host "Select"
        switch ($sel) {
            "1" { Edit-AllSettings }
            "2" { Edit-RuntimeSettings }
            "3" { Edit-RecordingSettings }
            "4" { Edit-SoopLoginSettings }
            "5" { Edit-WorkerSettings }
            "6" { Edit-LogSettings }
            "7" { Edit-IniDirectly }
            "8" { return }
            default {
                Write-Host ""
                Write-Host "Invalid selection."
                Start-Sleep -Seconds 1
            }
        }
    }
}

function Channel-Menu {
    while ($true) {
        Clear-Host
        Write-Host ""
        Write-Host "========================================"
        Write-Host " Edit Channel"
        Write-Host "========================================"
        Write-Host ""
        Show-Channels
        Write-Host ""
        Write-Host " 1. Add Channel"
        Write-Host " 2. Toggle Channel"
        Write-Host " 3. Remove Channel"
        Write-Host " 4. Edit Channel List"
        Write-Host " 5. Back"
        Write-Host ""

        $sel = Read-Host "Select"

        switch ($sel) {
            "1" {
                $enabled = Read-Host "Enable this channel? [Y/N] [Y]"
                if ([string]::IsNullOrWhiteSpace($enabled)) { $enabled = "Y" }
                $enabled = $enabled.Trim().ToUpperInvariant()

                if ($enabled -notin @("Y", "N")) {
                    Write-Host "ENABLED must be Y or N."
                    Pause-Menu
                    continue
                }

                $name = Read-Host "Display Name"
                if ([string]::IsNullOrWhiteSpace($name)) {
                    Write-Host "Display Name is required."
                    Pause-Menu
                    continue
                }

                $account = Read-Host "SOOP Account ID"
                if ([string]::IsNullOrWhiteSpace($account)) {
                    Write-Host "SOOP Account ID is required."
                    Pause-Menu
                    continue
                }

                $outdir = Read-Host "Custom OUTDIR [blank = default]"

                Add-Content -LiteralPath $ChannelPath `
                    -Value "$enabled|$name|$account|$outdir" `
                    -Encoding UTF8
            }

            "2" {
                $rows = @(Get-ChannelRows)
                if ($rows.Count -eq 0) {
                    Pause-Menu
                    continue
                }

                $rawNo = Read-Host "Channel number [0 = Cancel]"
                $no = 0

                if (-not [int]::TryParse($rawNo, [ref]$no) -or $no -eq 0) {
                    continue
                }

                if ($no -lt 1 -or $no -gt $rows.Count) {
                    Write-Host "Invalid channel number."
                    Pause-Menu
                    continue
                }

                $all = @(Get-Content -LiteralPath $ChannelPath -Encoding UTF8)
                $row = $rows[$no - 1]
                $parts = $all[$row.FileIndex].Split("|")

                if ($parts[0].Trim().ToUpperInvariant() -eq "Y") {
                    $parts[0] = "N"
                }
                else {
                    $parts[0] = "Y"
                }

                $all[$row.FileIndex] = $parts -join "|"

                [IO.File]::WriteAllLines(
                    $ChannelPath,
                    $all,
                    (New-Object Text.UTF8Encoding($true))
                )
            }

            "3" {
                $rows = @(Get-ChannelRows)
                if ($rows.Count -eq 0) {
                    Pause-Menu
                    continue
                }

                $rawNo = Read-Host "Channel number [0 = Cancel]"
                $no = 0

                if (-not [int]::TryParse($rawNo, [ref]$no) -or $no -eq 0) {
                    continue
                }

                if ($no -lt 1 -or $no -gt $rows.Count) {
                    Write-Host "Invalid channel number."
                    Pause-Menu
                    continue
                }

                $all = @(Get-Content -LiteralPath $ChannelPath -Encoding UTF8)
                $removeIndex = $rows[$no - 1].FileIndex

                $new = @()
                for ($i = 0; $i -lt $all.Count; $i++) {
                    if ($i -ne $removeIndex) {
                        $new += $all[$i]
                    }
                }

                [IO.File]::WriteAllLines(
                    $ChannelPath,
                    $new,
                    (New-Object Text.UTF8Encoding($true))
                )
            }

            "4" {
                Start-Process notepad.exe -ArgumentList "`"$ChannelPath`"" -Wait
            }

            "5" {
                return
            }
        }
    }
}

function Toggle-DailyLog {
    $cfg = Read-Ini $IniPath

    if ($cfg.Count -eq 0) {
        Write-Host "SOOP_LIVE_SETTING.ini does not exist. Run General Settings first."
        Pause-Menu
        return
    }

    $current = Get-Cfg $cfg "LOG_ENABLED" "Y"

    if ($current.Trim().ToUpperInvariant() -eq "Y") {
        $newValue = "N"
    }
    else {
        $newValue = "Y"
    }

    $lines = @(Get-Content -LiteralPath $IniPath -Encoding UTF8)
    $found = $false

    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^\s*LOG_ENABLED\s*=') {
            $lines[$i] = "LOG_ENABLED=$newValue"
            $found = $true
            break
        }
    }

    if (-not $found) {
        $lines += "LOG_ENABLED=$newValue"
    }

    [IO.File]::WriteAllLines(
        $IniPath,
        $lines,
        (New-Object Text.UTF8Encoding($true))
    )

    $display = if ($newValue -eq "Y") { "ON" } else { "OFF" }

    Write-Host ""
    Write-Host "Daily Log changed to: $display"
    Write-Host "Running watcher will apply this automatically."
    Pause-Menu
}

Ensure-ChannelFile

try {
    while ($true) {
        $cfg = Read-Ini $IniPath
        $logState = Get-Cfg $cfg "LOG_ENABLED" "Y"
        $logDisplay = if ($logState.Trim().ToUpperInvariant() -eq "Y") { "ON" } else { "OFF" }

        Clear-Host
        Write-Host ""
        Write-Host "========================================"
        Write-Host " SOOP LIVE Settings - Cloudflare"
        Write-Host "========================================"
        Write-Host ""
        Write-Host " 1. General Settings"
        Write-Host " 2. Edit Channel"
        Write-Host " 3. Toggle Daily Log"
        Write-Host " 4. Open Folder"
        Write-Host " 5. Exit"
        Write-Host ""
        Write-Host " Daily Log: $logDisplay"
        Write-Host ""

        $sel = Read-Host "Select"

        switch ($sel) {
            "1" { General-Settings }
            "2" { Channel-Menu }
            "3" { Toggle-DailyLog }
            "4" { Start-Process explorer.exe -ArgumentList "`"$ScriptDir`"" }
            "5" { break }
        }

        if ($sel -eq "5") {
            break
        }
    }
}
catch {
    Write-Host ""
    Write-Host "========================================"
    Write-Host " SETTINGS ERROR"
    Write-Host "========================================"
    Write-Host ""
    Write-Host $_.Exception.Message
    Write-Host ""
    Write-Host $_.ScriptStackTrace
    Write-Host ""
    Pause-Menu
    exit 1
}
