function Resolve-VodExecutable {
    param([string]$Configured, [string]$Local, [string]$Command)
    if (-not [string]::IsNullOrWhiteSpace($Configured) -and (Test-Path -LiteralPath $Configured -PathType Leaf)) { return [System.IO.Path]::GetFullPath($Configured) }
    if (Test-Path -LiteralPath $Local -PathType Leaf) { return [System.IO.Path]::GetFullPath($Local) }
    $found = Get-Command $Command -ErrorAction SilentlyContinue
    if ($null -ne $found) { return $found.Source }
    return $null
}

function Resolve-VodTools {
    param($Request, [string]$ScriptRoot)
    $settingsPath = Join-Path $ScriptRoot 'SOOP_VOD_SETTING.ini'
    $settings = @{}
    if (Test-Path -LiteralPath $settingsPath -PathType Leaf) {
        foreach ($line in Get-Content -LiteralPath $settingsPath -Encoding UTF8) {
            if ($line -match '^\s*([^#;][^=]*)=(.*)$') { $settings[$matches[1].Trim()] = $matches[2].Trim() }
        }
    }
    $requestedYtDlp = [string]$Request.YtDlpPath
    if (-not [string]::IsNullOrWhiteSpace($requestedYtDlp) -and -not (Test-Path -LiteralPath $requestedYtDlp -PathType Leaf)) {
        throw "설정한 yt-dlp 파일을 찾을 수 없습니다: $requestedYtDlp"
    }
    $ytConfiguration = if ([string]::IsNullOrWhiteSpace($requestedYtDlp)) { [string]$settings.YT_DLP_PATH } else { $requestedYtDlp }
    $yt = Resolve-VodExecutable -Configured $ytConfiguration -Local (Join-Path $ScriptRoot 'yt-dlp.exe') -Command 'yt-dlp.exe'
    if ([string]::IsNullOrWhiteSpace($yt)) { $yt = (Get-Command 'yt-dlp' -ErrorAction SilentlyContinue).Source }
    if ([string]::IsNullOrWhiteSpace($yt)) { throw 'yt-dlp를 찾을 수 없습니다.' }
    $requestedFfmpeg = [string]$Request.FfmpegPath
    if (-not [string]::IsNullOrWhiteSpace($requestedFfmpeg) -and -not (Test-Path -LiteralPath $requestedFfmpeg -PathType Leaf)) {
        throw "설정한 ffmpeg 파일을 찾을 수 없습니다: $requestedFfmpeg"
    }
    $ffConfiguration = if ([string]::IsNullOrWhiteSpace($requestedFfmpeg)) { [string]$settings.FFMPEG_PATH } else { $requestedFfmpeg }
    $ff = Resolve-VodExecutable -Configured $ffConfiguration -Local (Join-Path $ScriptRoot 'ffmpeg.exe') -Command 'ffmpeg.exe'
    if ([string]::IsNullOrWhiteSpace($ff)) { $command = Get-Command 'ffmpeg' -ErrorAction SilentlyContinue; if ($null -ne $command) { $ff = $command.Source } }
    return [pscustomobject]@{ YtDlp = $yt; Ffmpeg = $ff }
}

function Get-VodExternalErrorTail {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { return '' }
    try {
        $tail = @(Get-Content -LiteralPath $Path -Tail 20 -ErrorAction Stop)
        return Get-RedactedVodText -Text (($tail | ForEach-Object { [string]$_ }) -join ' | ')
    }
    catch { return '' }
}

function Get-VodMetadata {
    param($Request, [string]$YtDlp, [string]$CookieFile, [string]$JobDirectory)
    $stderrFile = Join-Path $JobDirectory 'yt-dlp-metadata.stderr.log'
    $metadataFile = Join-Path $JobDirectory 'yt-dlp-metadata.json'
    try {
        $previousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $json = @(& $YtDlp '--cookies' $CookieFile '--flat-playlist' '--dump-single-json' '--no-warnings' ([string]$Request.VodUrl) 2> $stderrFile)
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        $errorTail = Get-VodExternalErrorTail -Path $stderrFile
        if ($exitCode -ne 0) {
            if ([string]::IsNullOrWhiteSpace($errorTail)) { $errorTail = "yt-dlp exit code $exitCode" }
            throw "VOD 분석 실패: $errorTail"
        }
        $jsonText = $json -join [Environment]::NewLine
        [System.IO.File]::WriteAllText($metadataFile, $jsonText, [System.Text.UTF8Encoding]::new($false))
        try {
            $metadataText = [System.IO.File]::ReadAllText($metadataFile, [System.Text.Encoding]::UTF8)
            $info = $metadataText | ConvertFrom-Json
        }
        catch {
            $detail = if ([string]::IsNullOrWhiteSpace($errorTail)) { $_.Exception.Message } else { $errorTail }
            throw "VOD JSON 파싱 실패: $(Get-RedactedVodText -Text $detail)"
        }
    }
    finally {
        Remove-Item -LiteralPath $stderrFile -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $metadataFile -Force -ErrorAction SilentlyContinue
    }
    $entries = @($info.entries)
    if ($entries.Count -eq 0) { throw 'VOD PART를 찾지 못했습니다.' }
    $streamerId = [string]$info.uploader_id
    if ([string]::IsNullOrWhiteSpace($streamerId)) { $streamerId = [string]$entries[0].uploader_id }
    if ([string]::IsNullOrWhiteSpace($streamerId)) { throw 'VOD BJ ID를 찾지 못해 구독 인증을 요청할 수 없습니다.' }
    $streamer = [string]$info.uploader
    if ([string]::IsNullOrWhiteSpace($streamer)) { $streamer = [string]$entries[0].uploader }
    if ([string]::IsNullOrWhiteSpace($streamer)) { $streamer = $streamerId }
    $date = [string]$info.upload_date
    if ([string]::IsNullOrWhiteSpace($date)) { $date = [string]$entries[0].upload_date }
    if ([string]::IsNullOrWhiteSpace($date) -and [string]$entries[0].id -match '^(\d{8})_') { $date = $matches[1] }
    if ($date -notmatch '^\d{8}$') { $date = Get-Date -Format 'yyyyMMdd' }
    $title = [string]$info.title
    if ([string]::IsNullOrWhiteSpace($title)) { $title = [string]$entries[0].title }
    return [pscustomobject]@{ Title = $title; Streamer = $streamer; StreamerId = $streamerId; Date = $date.Substring(2, 6); Entries = $entries }
}

function Invoke-VodDownloads {
    param($Request, $Metadata, [int[]]$SelectedParts, $Tools, $Cookie, [string]$JobDirectory)
    $directory = [System.IO.Path]::GetFullPath([string]$Request.OutputDirectory).Normalize([System.Text.NormalizationForm]::FormC)
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
        $lastFailureDetail = ''
        $streamerId = [string]$Metadata.StreamerId
        for ($attempt = 1; $attempt -le [int]$Request.MaxRetries; $attempt++) {
            # Subscription VOD authorization values are deliberately short-lived.
            # private_auth.php is called for every attempt. On a retry, rebuild
            # the independent SOOP/browser base session first so an expired login
            # session cannot prevent issuance of a fresh CloudFront cookie.
            if ($attempt -gt 1) {
                Renew-VodBaseCookie -Request $Request -Cookie $Cookie -Attempt $attempt
            }
            # Refresh metadata before every PART attempt, not only after a 403.
            # A later selected PART may start hours after the initial analysis,
            # by which time its manifest URL can already be expired.
            Write-VodEvent -Type 'metadata_refreshing' -Message ("PART {0} 최신 VOD URL 분석 중 ({1}/{2})" -f $part, $attempt, [int]$Request.MaxRetries) -Part $part
            $refreshedMetadata = Get-VodMetadata -Request $Request -YtDlp $Tools.YtDlp -CookieFile $Cookie.Path -JobDirectory $JobDirectory
            if ($part -gt $refreshedMetadata.Entries.Count) { throw "새 VOD 정보에서 PART $part 를 찾지 못했습니다." }
            $url = [string]$refreshedMetadata.Entries[$part - 1].url
            $streamerId = [string]$refreshedMetadata.StreamerId
            if ([string]::IsNullOrWhiteSpace($url)) { throw "새 VOD 정보에 PART $part URL이 없습니다." }
            $authorized = Refresh-VodAuthorization -Request $Request -CookieFile $Cookie.Path -StreamerId $streamerId -Url $url -Attempt $attempt
            if ($authorized) {
                $probeFile = Join-Path $JobDirectory ("manifest-probe-{0:D4}-{1:D2}.m3u8" -f $part, $attempt)
                $authorized = Test-VodManifestAuthorization -Request $Request -CookieFile $Cookie.Path -Url $url -ProbeFile $probeFile
            }
            if (-not $authorized) {
                $authDetail = if ([string]::IsNullOrWhiteSpace([string]$script:LastVodAuthError)) { 'private_auth 응답이 인증 성공을 반환하지 않았습니다.' } else { [string]$script:LastVodAuthError }
                $lastFailureDetail = $authDetail
                Write-VodEvent -Type 'auth_retrying' -Message ("구독 VOD 인증 재시도 ({0}/{1}) · {2}" -f $attempt, [int]$Request.MaxRetries, $authDetail) -Part $part
                Start-Sleep -Seconds ([Math]::Min(16, [Math]::Pow(2, $attempt - 1))); continue
            }
            $args = @('--cookies', $Cookie.Path, '--referer', [string]$Request.VodUrl, '--add-header', 'Origin:https://vod.sooplive.com', '--user-agent', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36', '--continue', '--fragment-retries', '1', '--retries', '1', '--abort-on-unavailable-fragments', '--no-overwrites', '--merge-output-format', 'mp4', '--newline', '-o', $path)
            if (-not [string]::IsNullOrWhiteSpace([string]$Tools.Ffmpeg)) { $args += @('--ffmpeg-location', [string]$Tools.Ffmpeg) }
            $args += $url
            $stderrFile = Join-Path $JobDirectory ("yt-dlp-part-{0:D4}-attempt-{1:D2}.stderr.log" -f $part, $attempt)
            try {
                $previousErrorActionPreference = $ErrorActionPreference
                try {
                    # Windows PowerShell 5.1 can promote native stderr to a
                    # terminating NativeCommandError when the script preference
                    # is Stop. Keep stderr redirected and classify the exit code
                    # ourselves so a 403 reaches the authorization retry path.
                    $ErrorActionPreference = 'Continue'
                    & $Tools.YtDlp @args 2> $stderrFile | ForEach-Object {
                        if ($_ -match '(?<percent>\d+(?:\.\d+)?)%') {
                            $percent = 0.0
                            if ([double]::TryParse(
                                $matches.percent,
                                [Globalization.NumberStyles]::Float,
                                [Globalization.CultureInfo]::InvariantCulture,
                                [ref]$percent
                            )) {
                                Write-VodEvent -Type 'part_progress' -Message ("PART {0}: {1}%" -f $part, $matches.percent) -Part $part -PartCount $Metadata.Entries.Count -Percent $percent
                            }
                        }
                    }
                    $downloadExitCode = $LASTEXITCODE
                }
                finally {
                    $ErrorActionPreference = $previousErrorActionPreference
                }
                if ($downloadExitCode -eq 0 -and (Test-Path -LiteralPath $path -PathType Leaf)) { $complete = $true; break }
                $errorTail = Get-VodExternalErrorTail -Path $stderrFile
                if ([string]::IsNullOrWhiteSpace($errorTail)) { $errorTail = "yt-dlp exit code $downloadExitCode" }
                $lastFailureDetail = $errorTail
                $retryType = if ($errorTail -match '(?i)(HTTP Error 403|Forbidden)') { 'authorization_expired' } else { 'part_retrying' }
                $retryMessage = if ($retryType -eq 'authorization_expired') {
                    "PART $part 단기 인증 만료(403) · 로그인 세션, VOD URL, 인증 Cookie를 새로 발급합니다."
                }
                else { "PART $part 재시도 ($attempt/$([int]$Request.MaxRetries)) · $errorTail" }
                Write-VodEvent -Type $retryType -Message $retryMessage -Part $part
            }
            finally {
                Remove-Item -LiteralPath $stderrFile -Force -ErrorAction SilentlyContinue
            }
            Start-Sleep -Seconds ([Math]::Min(16, [Math]::Pow(2, $attempt - 1)))
        }
        if (-not $complete) {
            if ([string]::IsNullOrWhiteSpace($lastFailureDetail)) { $lastFailureDetail = '상세 오류를 확인하지 못했습니다.' }
            throw "PART $part 다운로드 실패: $lastFailureDetail"
        }
        $files += $path
    }
    return @($files)
}
