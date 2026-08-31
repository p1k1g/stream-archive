function Resolve-VodExecutable {
    param([string]$Configured, [string]$Local, [string]$Command)
    if (-not [string]::IsNullOrWhiteSpace($Configured) -and (Test-Path -LiteralPath $Configured -PathType Leaf)) { return [System.IO.Path]::GetFullPath($Configured) }
    if (Test-Path -LiteralPath $Local -PathType Leaf) { return [System.IO.Path]::GetFullPath($Local) }
    $found = Get-Command $Command -ErrorAction SilentlyContinue
    if ($null -ne $found) { return $found.Source }
    return $null
}

function Resolve-VodTools {
    param([string]$ScriptRoot)
    $settingsPath = Join-Path $ScriptRoot 'SOOP_VOD_SETTING.ini'
    $settings = @{}
    if (Test-Path -LiteralPath $settingsPath -PathType Leaf) {
        foreach ($line in Get-Content -LiteralPath $settingsPath -Encoding UTF8) {
            if ($line -match '^\s*([^#;][^=]*)=(.*)$') { $settings[$matches[1].Trim()] = $matches[2].Trim() }
        }
    }
    $yt = Resolve-VodExecutable -Configured $settings.YT_DLP_PATH -Local (Join-Path $ScriptRoot 'yt-dlp.exe') -Command 'yt-dlp.exe'
    if ([string]::IsNullOrWhiteSpace($yt)) { $yt = (Get-Command 'yt-dlp' -ErrorAction SilentlyContinue).Source }
    if ([string]::IsNullOrWhiteSpace($yt)) { throw 'yt-dlp를 찾을 수 없습니다.' }
    $ff = Resolve-VodExecutable -Configured $settings.FFMPEG_PATH -Local (Join-Path $ScriptRoot 'ffmpeg.exe') -Command 'ffmpeg.exe'
    if ([string]::IsNullOrWhiteSpace($ff)) { $command = Get-Command 'ffmpeg' -ErrorAction SilentlyContinue; if ($null -ne $command) { $ff = $command.Source } }
    return [pscustomobject]@{ YtDlp = $yt; Ffmpeg = $ff }
}

function Get-VodMetadata {
    param($Request, [string]$YtDlp, [string]$CookieFile)
    $json = & $YtDlp '--cookies' $CookieFile '--flat-playlist' '--dump-single-json' '--no-warnings' ([string]$Request.VodUrl) 2>&1
    if ($LASTEXITCODE -ne 0) { throw ('VOD 분석 실패: ' + (Get-RedactedVodText (($json | Select-Object -Last 5) -join ' '))) }
    try { $info = ($json -join [Environment]::NewLine) | ConvertFrom-Json }
    catch { throw 'VOD JSON 파싱에 실패했습니다.' }
    $entries = @($info.entries)
    if ($entries.Count -eq 0) { throw 'VOD PART를 찾지 못했습니다.' }
    $streamer = if ([string]::IsNullOrWhiteSpace([string]$info.uploader)) { [string]$info.uploader_id } else { [string]$info.uploader }
    $date = [string]$info.upload_date
    if ($date -notmatch '^\d{8}$') { $date = Get-Date -Format 'yyyyMMdd' }
    return [pscustomobject]@{ Title = [string]$info.title; Streamer = $streamer; StreamerId = [string]$info.uploader_id; Date = $date.Substring(2, 6); Entries = $entries }
}

function Invoke-VodDownloads {
    param($Request, $Metadata, [int[]]$SelectedParts, $Tools, $Cookie, [string]$JobDirectory)
    $directory = [System.IO.Path]::GetFullPath([string]$Request.OutputDirectory)
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    $streamer = Get-SafeVodFileName $Metadata.Streamer
    $files = @()
    foreach ($part in $SelectedParts) {
        $entry = $Metadata.Entries[$part - 1]
        $url = [string]$entry.url
        if ([string]::IsNullOrWhiteSpace($url)) { throw "PART $part URL이 없습니다." }
        $base = '{0}_{1}_{2:D2}' -f $Metadata.Date, $streamer, $part
        $path = Get-CollisionSafeVodPath -Directory $directory -BaseName $base -Extension '.mp4'
        Write-VodEvent -Type 'part_started' -Message ("PART {0}/{1} 다운로드 중…" -f $part, $Metadata.Entries.Count) -Part $part -PartCount $Metadata.Entries.Count
        $complete = $false
        for ($attempt = 1; $attempt -le [int]$Request.MaxRetries; $attempt++) {
            # Subscription VOD authorization values are deliberately short-lived.
            # private_auth.php is called for every attempt. On a retry, rebuild
            # the independent SOOP/browser base session first so an expired login
            # session cannot prevent issuance of a fresh CloudFront cookie.
            if ($attempt -gt 1) {
                Renew-VodBaseCookie -Request $Request -Cookie $Cookie -Attempt $attempt
            }
            if (-not (Refresh-VodAuthorization -Request $Request -CookieFile $Cookie.Path -StreamerId $Metadata.StreamerId -Url $url -Attempt $attempt)) {
                Write-VodEvent -Type 'auth_retrying' -Message ("구독 VOD 인증 재시도 ({0}/{1})" -f $attempt, [int]$Request.MaxRetries) -Part $part
                Start-Sleep -Seconds ([Math]::Min(16, [Math]::Pow(2, $attempt - 1))); continue
            }
            $args = @('--cookies', $Cookie.Path, '--referer', [string]$Request.VodUrl, '--continue', '--fragment-retries', '2', '--retries', '2', '--abort-on-unavailable-fragments', '--no-overwrites', '--merge-output-format', 'mp4', '--newline', '-o', $path)
            if (-not [string]::IsNullOrWhiteSpace([string]$Tools.Ffmpeg)) { $args += @('--ffmpeg-location', [string]$Tools.Ffmpeg) }
            $args += $url
            & $Tools.YtDlp @args 2>&1 | ForEach-Object {
                if ($_ -match '(?<percent>\d+(?:\.\d+)?)%') { Write-VodEvent -Type 'part_progress' -Message ("PART {0}: {1}%" -f $part, $matches.percent) -Part $part -PartCount $Metadata.Entries.Count -Percent ([double]$matches.percent) }
            }
            if ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $path -PathType Leaf)) { $complete = $true; break }
            Write-VodEvent -Type 'part_retrying' -Message ("PART {0} 재시도 ({1}/{2})" -f $part, $attempt, [int]$Request.MaxRetries) -Part $part
            Start-Sleep -Seconds ([Math]::Min(16, [Math]::Pow(2, $attempt - 1)))
        }
        if (-not $complete) { throw "PART $part 다운로드에 실패했습니다." }
        $files += $path
    }
    return @($files)
}
