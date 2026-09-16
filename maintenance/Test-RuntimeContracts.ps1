$ErrorActionPreference = 'Stop'
$guardRoot = Join-Path $PSScriptRoot 'guards'
$contracts = @(
    'Architecture.ps1',
    'Providers.ps1',
    'ProcessLifecycle.ps1',
    'StorageOwnership.ps1',
    'Security.ps1',
    'ToolDiscovery.ps1',
    'ReleaseSafety.ps1'
)

foreach ($contract in $contracts) {
    Write-Host "Running runtime contract: $contract"
    & (Join-Path $guardRoot $contract)
}
Write-Host 'All runtime contracts passed.'
