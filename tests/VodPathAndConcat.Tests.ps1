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

Write-Host 'VOD path and concat regression tests passed.'
