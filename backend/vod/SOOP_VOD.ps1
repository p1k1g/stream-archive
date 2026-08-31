param(
    [Parameter(Mandatory = $true)]
    [string]$RequestFile
)

$ErrorActionPreference = 'Stop'
$script:VodExitCode = 0
$script:VodRequest = $null
$script:VodJobDirectory = $null

$moduleRoot = Join-Path $PSScriptRoot 'modules'
$requiredModules = @('SOOP.Vod.Core.ps1', 'SOOP.Vod.Auth.ps1', 'SOOP.Vod.Download.ps1', 'SOOP.Vod.Merge.ps1')
foreach ($module in $requiredModules) {
    $modulePath = Join-Path $moduleRoot $module
    if (-not (Test-Path -LiteralPath $modulePath -PathType Leaf)) { throw "VOD module missing: $module" }
    . $modulePath
}

try {
    if (-not (Test-Path -LiteralPath $RequestFile -PathType Leaf)) { throw 'VOD request file was not found.' }
    $script:VodJobDirectory = Split-Path -Parent ([System.IO.Path]::GetFullPath($RequestFile))
    $script:VodRequest = Get-Content -LiteralPath $RequestFile -Raw -Encoding UTF8 | ConvertFrom-Json
    Test-VodRequest -Request $script:VodRequest
    Write-VodEvent -Type 'analysis_started' -Message 'VOD 분석 중…'

    $tools = Resolve-VodTools -ScriptRoot $PSScriptRoot
    $cookie = Initialize-VodCookie -Request $script:VodRequest -JobDirectory $script:VodJobDirectory -YtDlp $tools.YtDlp
    $metadata = Get-VodMetadata -Request $script:VodRequest -YtDlp $tools.YtDlp -CookieFile $cookie.Path
    $parts = Resolve-VodParts -RequestedParts @($script:VodRequest.Parts) -PartCount $metadata.Entries.Count
    Write-VodEvent -Type 'metadata_ready' -Message ("{0} · {1}개 PART" -f $metadata.Title, $metadata.Entries.Count) -Title $metadata.Title -Streamer $metadata.Streamer -PartCount $metadata.Entries.Count

    $downloaded = Invoke-VodDownloads -Request $script:VodRequest -Metadata $metadata -SelectedParts $parts -Tools $tools -Cookie $cookie -JobDirectory $script:VodJobDirectory
    $finalFile = $downloaded[0]
    if ($downloaded.Count -gt 1 -and [bool]$script:VodRequest.Merge) {
        $finalFile = Merge-VodParts -Request $script:VodRequest -Metadata $metadata -PartFiles $downloaded -Ffmpeg $tools.Ffmpeg -JobDirectory $script:VodJobDirectory
    }
    Write-VodEvent -Type 'completed' -Message 'VOD 다운로드가 완료되었습니다.' -OutputFile $finalFile -Percent 100
}
catch {
    $script:VodExitCode = 1
    $message = Get-RedactedVodText -Text $_.Exception.Message
    Write-VodEvent -Type 'failed' -Message $message
    Write-Error $message
}
finally {
    if ($null -ne $script:VodRequest) { Remove-VodTemporarySecrets -JobDirectory $script:VodJobDirectory }
}
exit $script:VodExitCode
