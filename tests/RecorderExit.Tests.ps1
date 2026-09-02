$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$recorderModule = Join-Path $root 'backend\modules\SOOP.Recorder.ps1'
. $recorderModule

if ((Get-RecorderExitReason -ExitCode $null) -ne 'NORMAL') {
    throw 'Unavailable recorder exit code must be classified as normal.'
}
if ((Get-RecorderExitReason -ExitCode '') -ne 'NORMAL') {
    throw 'Blank recorder exit code must be classified as normal.'
}
if ((Get-RecorderExitReason -ExitCode 0) -ne 'NORMAL') {
    throw 'Recorder exit code 0 must be classified as normal.'
}
if ((Get-RecorderExitReason -ExitCode 1) -ne 'RECORDER EXIT CODE=1') {
    throw 'Known non-zero recorder exit code must remain actionable.'
}

$heart = [string][char]0x2665
$safeChannel = Get-SafeChannelFileName -Name ("salt[$heart]:name")
if ($safeChannel -ne 'salt[_]_name' -or $safeChannel.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0) {
    throw 'Channel/BJ filename sanitization did not replace symbols or invalid Windows characters.'
}

Write-Host 'Recorder exit classification tests passed.'
