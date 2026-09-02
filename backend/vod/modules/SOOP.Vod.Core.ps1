function Write-VodEvent {
    param([string]$Type, [string]$Message = '', [string]$Title = '', [string]$Streamer = '', [int]$Part = 0, [int]$PartCount = 0, [double]$Percent = 0, [string]$OutputFile = '', [string[]]$Qualities = @())
    $event = [ordered]@{ version = 1; type = $Type; jobId = [string]$script:VodRequest.JobId; timestamp = [DateTimeOffset]::Now.ToString('o'); message = $Message; title = $Title; streamer = $Streamer; part = $Part; partCount = $PartCount; percent = $Percent; outputFile = $OutputFile; qualities = @($Qualities) }
    # Write directly to stdout instead of the success pipeline. Invoke-VodDownloads
    # is assigned to $downloaded by the entry script; pipeline events would become
    # fake PartFiles and later be handed to ffmpeg as file names.
    [Console]::Out.WriteLine('@@SOOP_VOD_EVENT@@' + ($event | ConvertTo-Json -Compress -Depth 5))
}

function Register-VodOwnedOutputPath {
    param([string]$JobDirectory, [string]$Path, [switch]$DeleteTargetOnCleanup)
    $registry = Join-Path $JobDirectory 'owned-output-paths.txt'
    $kind = if ($DeleteTargetOnCleanup) { 'DELETE' } else { 'KEEP' }
    [System.IO.File]::AppendAllLines($registry, [string[]]@($kind + '|' + [System.IO.Path]::GetFullPath($Path)), [System.Text.UTF8Encoding]::new($false))
}

function Complete-VodOwnedOutputPath {
    param([string]$JobDirectory, [string]$Path)
    $registry = Join-Path $JobDirectory 'owned-output-paths.txt'
    if (-not (Test-Path -LiteralPath $registry -PathType Leaf)) { return }
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $remaining = @([System.IO.File]::ReadAllLines($registry, [System.Text.Encoding]::UTF8) | Where-Object { $_ -ne ('DELETE|' + $fullPath) })
    [System.IO.File]::WriteAllLines($registry, [string[]]$remaining, [System.Text.UTF8Encoding]::new($false))
}

function Remove-VodIncompleteArtifacts {
    param([string]$JobDirectory)
    $registry = Join-Path $JobDirectory 'owned-output-paths.txt'
    if (-not (Test-Path -LiteralPath $registry -PathType Leaf)) { return }
    foreach ($record in [System.IO.File]::ReadAllLines($registry, [System.Text.Encoding]::UTF8)) {
        if ([string]::IsNullOrWhiteSpace($record)) { continue }
        $separator = $record.IndexOf('|')
        if ($separator -le 0 -or $separator -ge ($record.Length - 1)) { continue }
        $kind = $record.Substring(0, $separator)
        if ($kind -ne 'KEEP' -and $kind -ne 'DELETE') { continue }
        $target = $record.Substring($separator + 1)
        $directory = Split-Path -Parent $target
        $leaf = Split-Path -Leaf $target
        if ([string]::IsNullOrWhiteSpace($directory) -or -not (Test-Path -LiteralPath $directory -PathType Container)) { continue }
        $artifactPrefixes = @($leaf + '.part', $leaf + '.ytdl', $leaf + '.temp')
        # Remove the standard yt-dlp names explicitly. This is the reliable path
        # on Windows PowerShell 5.1 even when provider enumeration behaves
        # differently for a directory containing a recently closed subprocess.
        foreach ($suffix in @('.part', '.ytdl', '.temp')) {
            $candidate = $target + $suffix
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                Remove-Item -LiteralPath $candidate -Force -ErrorAction SilentlyContinue
            }
        }
        $artifactPaths = @()
        try { $artifactPaths = @([System.IO.Directory]::EnumerateFiles($directory)) }
        catch { }
        foreach ($artifactPath in $artifactPaths) {
            $artifactName = [System.IO.Path]::GetFileName($artifactPath)
            $removeArtifact = $false
            foreach ($prefix in $artifactPrefixes) {
                if ($artifactName.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                    $removeArtifact = $true
                    break
                }
            }
            if ($removeArtifact) {
                Remove-Item -LiteralPath $artifactPath -Force -ErrorAction SilentlyContinue
            }
        }
        if ($kind -eq 'DELETE' -and (Test-Path -LiteralPath $target -PathType Leaf)) {
            Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
        }
    }
}

function Test-VodRequest {
    param($Request)
    if ([int]$Request.Version -ne 1) { throw 'Unsupported VOD request version.' }
    if ([string]::IsNullOrWhiteSpace([string]$Request.JobId)) { throw 'VOD job ID is missing.' }
    $uri = $null
    if (-not [Uri]::TryCreate([string]$Request.VodUrl, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne 'https' -or -not $uri.Host.EndsWith('sooplive.com', [StringComparison]::OrdinalIgnoreCase) -or $uri.AbsolutePath -notmatch '/player/\d+') { throw '올바른 SOOP VOD HTTPS URL이 아닙니다.' }
    if ([string]::IsNullOrWhiteSpace([string]$Request.OutputDirectory)) { throw 'VOD 출력 폴더가 비어 있습니다.' }
    if ([int]$Request.MaxRetries -lt 1 -or [int]$Request.MaxRetries -gt 20) { throw 'MAX_RETRY must be between 1 and 20.' }
    $quality = [string]$Request.Quality
    if (-not [string]::IsNullOrWhiteSpace($quality) -and $quality -notmatch '^best(?:\[height<=\d+\])?$') {
        throw '지원하지 않는 VOD 화질 선택입니다.'
    }
}

function Resolve-VodParts {
    param([object[]]$RequestedParts, [int]$PartCount)
    if ($PartCount -lt 1) { throw 'VOD PART를 찾지 못했습니다.' }
    if ($RequestedParts.Count -eq 0) { return @(1..$PartCount) }
    $result = @($RequestedParts | ForEach-Object { [int]$_ } | Sort-Object -Unique)
    foreach ($part in $result) { if ($part -lt 1 -or $part -gt $PartCount) { throw "PART는 1~$PartCount 범위여야 합니다: $part" } }
    return $result
}

function Get-SafeVodFileName {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name)) { return 'UNKNOWN' }
    $normalized = $Name.Normalize([System.Text.NormalizationForm]::FormC)
    $safe = [Regex]::Replace($normalized, '[\x00-\x1f\\/:*?"<>|]', '_').Trim().TrimEnd('.', ' ')
    if ($safe -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)') { $safe = '_' + $safe }
    if ($safe.Length -gt 100) { $safe = $safe.Substring(0, 100).TrimEnd('.', ' ') }
    if ([string]::IsNullOrWhiteSpace($safe)) { return 'UNKNOWN' }
    return $safe
}

function Get-CollisionSafeVodPath {
    param([string]$Directory, [string]$BaseName, [string]$Extension)
    $candidate = Assert-VodFullPath -Path (Join-Path $Directory ($BaseName + $Extension))
    if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    for ($suffix = 1; $suffix -le 9999; $suffix++) {
        $candidate = Assert-VodFullPath -Path (Join-Path $Directory ("{0}_{1:D3}{2}" -f $BaseName, $suffix, $Extension))
        if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    }
    throw '충돌 없는 VOD 출력 파일명을 만들지 못했습니다.'
}

function Assert-VodFullPath {
    param([string]$Path, [int]$MaximumLength = 240)
    $fullPath = [System.IO.Path]::GetFullPath($Path).Normalize([System.Text.NormalizationForm]::FormC)
    if ($fullPath.Length -gt $MaximumLength) {
        throw "VOD 전체 출력 경로가 안전 제한을 초과했습니다 ($($fullPath.Length)/$MaximumLength). 출력 폴더 또는 파일명을 줄여 주세요."
    }
    return $fullPath
}

function Get-RedactedVodText {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return '' }
    $safe = $Text -replace '(?i)(cookie|token|signature|policy|key-pair-id|authorization)(\s*[:=]\s*)[^\s;&]+', '$1$2***'
    return $safe -replace '(?i)(https?://[^?\s]+)\?[^\s]+', '$1?[REDACTED]'
}
