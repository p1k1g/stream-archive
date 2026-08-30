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

Write-Host 'Recorder exit classification tests passed.'
