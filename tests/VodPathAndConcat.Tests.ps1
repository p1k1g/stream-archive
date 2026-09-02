$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Core.ps1')
. (Join-Path $root 'backend/vod/modules/SOOP.Vod.Merge.ps1')

$decomposed = [string][char]0x1100 + [string][char]0x1161
$composed = [string][char]0xAC00
if ((Get-SafeVodFileName -Name $decomposed) -ne $composed) {
    throw 'VOD file names are not normalized to Unicode NFC.'
}

$korean = [string][char]0xD14C + [string][char]0xC2A4 + [string][char]0xD2B8
$path = "C:\$korean\owner's clip.mp4"
$line = ConvertTo-VodFfmpegConcatLine -Path $path
$quoteEscape = [string][char]0x27 + [string][char]0x5C + [string][char]0x27 + [string][char]0x27
if (-not $line.StartsWith("file '") -or -not $line.Contains($korean) -or -not $line.Contains($quoteEscape)) {
    throw 'FFmpeg concat escaping did not preserve Unicode or apostrophes.'
}
if ($line.Contains('C:\')) { throw 'FFmpeg concat path was not converted to forward slashes.' }

$tooLong = Join-Path ([IO.Path]::GetTempPath()) (('a' * 230) + '.mp4')
$rejected = $false
try { [void](Assert-VodFullPath -Path $tooLong) }
catch { $rejected = $true }
if (-not $rejected) { throw 'VOD full path safety limit was not enforced.' }

$script:VodRequest = [pscustomobject]@{ JobId = 'pipeline-test' }
$capturedEvent = @(Write-VodEvent -Type 'part_started' -Message 'event must not enter success pipeline')
if ($capturedEvent.Count -ne 0) { throw 'Structured VOD event polluted the PartFiles success pipeline.' }

$cleanupRoot = Join-Path ([IO.Path]::GetTempPath()) ('soop-vod-cleanup-' + [Guid]::NewGuid().ToString('N'))
$outputRoot = Join-Path $cleanupRoot 'output'
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
try {
    $partTarget = Join-Path $outputRoot 'part.mp4'
    [IO.File]::WriteAllText($partTarget, 'complete', [Text.Encoding]::ASCII)
    [IO.File]::WriteAllText($partTarget + '.part', 'partial', [Text.Encoding]::ASCII)
    [IO.File]::WriteAllText($partTarget + '.ytdl', 'state', [Text.Encoding]::ASCII)
    Register-VodOwnedOutputPath -JobDirectory $cleanupRoot -Path $partTarget
    $mergeTarget = Join-Path $outputRoot 'merge.mp4'
    [IO.File]::WriteAllText($mergeTarget, 'incomplete', [Text.Encoding]::ASCII)
    Register-VodOwnedOutputPath -JobDirectory $cleanupRoot -Path $mergeTarget -DeleteTargetOnCleanup
    Remove-VodIncompleteArtifacts -JobDirectory $cleanupRoot
    if (-not (Test-Path -LiteralPath $partTarget)) { throw 'VOD cancellation cleanup removed a completed PART.' }
    if (Test-Path -LiteralPath ($partTarget + '.part')) { throw 'VOD cancellation cleanup retained an mp4.part file.' }
    if (Test-Path -LiteralPath ($partTarget + '.ytdl')) { throw 'VOD cancellation cleanup retained a ytdl state file.' }
    if (Test-Path -LiteralPath $mergeTarget) { throw 'VOD cancellation cleanup retained an incomplete merge target.' }
}
finally { Remove-Item -LiteralPath $cleanupRoot -Recurse -Force -ErrorAction SilentlyContinue }

Write-Host 'VOD path and concat regression tests passed.'
