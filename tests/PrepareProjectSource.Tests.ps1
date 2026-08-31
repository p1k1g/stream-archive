$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$prepare = Get-Content -LiteralPath (Join-Path $root 'PREPARE_PROJECT.ps1') -Raw -Encoding UTF8

if ($prepare -notmatch '\$previousErrorActionPreference\s*=\s*\$ErrorActionPreference') {
    throw 'Template probe does not preserve ErrorActionPreference.'
}
if ($prepare -notmatch '\$ErrorActionPreference\s*=\s*''Continue''') {
    throw 'Missing-template probe can still terminate under ErrorActionPreference Stop.'
}
if ($prepare -notmatch '\$templateProbeExitCode\s*=\s*\$LASTEXITCODE') {
    throw 'Template probe does not capture the native exit code.'
}
if ($prepare -notmatch '\$ErrorActionPreference\s*=\s*\$previousErrorActionPreference') {
    throw 'Template probe does not restore ErrorActionPreference.'
}
if ($prepare -notmatch 'dotnet\.exe new install Microsoft\.WindowsAppSDK\.WinUI\.CSharp\.Templates') {
    throw 'Official WinUI template installation fallback is missing.'
}

Write-Host 'Project preparation source invariants passed.'
