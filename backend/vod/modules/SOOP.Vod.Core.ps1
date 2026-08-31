function Write-VodEvent {
    param([string]$Type, [string]$Message = '', [string]$Title = '', [string]$Streamer = '', [int]$Part = 0, [int]$PartCount = 0, [double]$Percent = 0, [string]$OutputFile = '')
    $event = [ordered]@{ version = 1; type = $Type; jobId = [string]$script:VodRequest.JobId; timestamp = [DateTimeOffset]::Now.ToString('o'); message = $Message; title = $Title; streamer = $Streamer; part = $Part; partCount = $PartCount; percent = $Percent; outputFile = $OutputFile }
    Write-Output ('@@SOOP_VOD_EVENT@@' + ($event | ConvertTo-Json -Compress -Depth 5))
}

function Test-VodRequest {
    param($Request)
    if ([int]$Request.Version -ne 1) { throw 'Unsupported VOD request version.' }
    if ([string]::IsNullOrWhiteSpace([string]$Request.JobId)) { throw 'VOD job ID is missing.' }
    $uri = $null
    if (-not [Uri]::TryCreate([string]$Request.VodUrl, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne 'https' -or -not $uri.Host.EndsWith('sooplive.com', [StringComparison]::OrdinalIgnoreCase) -or $uri.AbsolutePath -notmatch '/player/\d+') { throw '올바른 SOOP VOD HTTPS URL이 아닙니다.' }
    if ([string]::IsNullOrWhiteSpace([string]$Request.OutputDirectory)) { throw 'VOD 출력 폴더가 비어 있습니다.' }
    if ([int]$Request.MaxRetries -lt 1 -or [int]$Request.MaxRetries -gt 20) { throw 'MAX_RETRY must be between 1 and 20.' }
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
    $safe = [Regex]::Replace($Name, '[\x00-\x1f\\/:*?"<>|]', '_').Trim().TrimEnd('.', ' ')
    if ($safe -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)') { $safe = '_' + $safe }
    if ($safe.Length -gt 100) { $safe = $safe.Substring(0, 100).TrimEnd('.', ' ') }
    if ([string]::IsNullOrWhiteSpace($safe)) { return 'UNKNOWN' }
    return $safe
}

function Get-CollisionSafeVodPath {
    param([string]$Directory, [string]$BaseName, [string]$Extension)
    $candidate = Join-Path $Directory ($BaseName + $Extension)
    if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    for ($suffix = 1; $suffix -le 9999; $suffix++) {
        $candidate = Join-Path $Directory ("{0}_{1:D3}{2}" -f $BaseName, $suffix, $Extension)
        if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    }
    throw '충돌 없는 VOD 출력 파일명을 만들지 못했습니다.'
}

function Get-RedactedVodText {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return '' }
    $safe = $Text -replace '(?i)(cookie|token|signature|policy|key-pair-id|authorization)(\s*[:=]\s*)[^\s;&]+', '$1$2***'
    return $safe -replace '(?i)(https?://[^?\s]+)\?[^\s]+', '$1?[REDACTED]'
}
