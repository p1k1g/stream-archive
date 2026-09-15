param(
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [string]$ManifestPath = ".\rust-web\Cargo.toml"
)

$ErrorActionPreference = 'Stop'

$metadataJson = & cargo metadata --locked --no-deps --format-version 1 --manifest-path $ManifestPath
if ($LASTEXITCODE -ne 0) {
    throw "cargo metadata failed with exit code $LASTEXITCODE"
}

$metadata = $metadataJson | ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'stream-archive-server' } | Select-Object -First 1
if ($null -eq $package -or [string]::IsNullOrWhiteSpace([string]$package.version)) {
    throw 'Unable to resolve stream-archive-server package version from cargo metadata'
}

$commit = 'unknown'
if (Get-Command git -ErrorAction SilentlyContinue) {
    $candidate = (& git rev-parse --short=12 HEAD 2>$null | Select-Object -First 1)
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($candidate)) {
        $commit = $candidate.Trim()
    }
}

$parent = Split-Path -Parent $OutputPath
if ($parent) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$lines = [string[]]@(
    'product=Stream Archive',
    ('version=' + $package.version),
    ('commit=' + $commit),
    ('built_at=' + (Get-Date).ToString('o'))
)

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllLines($OutputPath, $lines, $utf8NoBom)

Write-Host "Release metadata written: version=$($package.version) commit=$commit"
