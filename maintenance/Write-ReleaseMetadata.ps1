param(
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [string]$ManifestPath = ".\rust-runtime\Cargo.toml",
    [string]$RepositoryRoot = ""
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

# Git provenance is optional so GitHub source archives without .git still build.
# CI release/package jobs use GITHUB_SHA when a repository root is explicitly
# supplied. Local package builds fall back to Git and mark dirty worktrees.
$commit = 'unknown'
$resolvedRepositoryRoot = $null
if (-not [string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    if (-not (Test-Path -LiteralPath $RepositoryRoot -PathType Container)) {
        throw "RepositoryRoot does not exist: $RepositoryRoot"
    }
    $resolvedRepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
}

if ($null -ne $resolvedRepositoryRoot) {
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_SHA) -and $env:GITHUB_SHA -match '^[0-9a-fA-F]{40}$') {
        $commit = $env:GITHUB_SHA.Substring(0, 12).ToLowerInvariant()
    }
    elseif (Get-Command git -ErrorAction SilentlyContinue) {
        $previousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'SilentlyContinue'
            $candidate = (& git -C $resolvedRepositoryRoot rev-parse --short=12 HEAD 2>$null | Select-Object -First 1)
            $gitExitCode = $LASTEXITCODE
            $dirtyState = @()
            if ($gitExitCode -eq 0) {
                $dirtyState = @(& git -C $resolvedRepositoryRoot status --porcelain --untracked-files=normal 2>$null)
                $dirtyExitCode = $LASTEXITCODE
            }
            else {
                $dirtyExitCode = $gitExitCode
            }
        }
        finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }

        if ($gitExitCode -eq 0 -and -not [string]::IsNullOrWhiteSpace($candidate)) {
            $commit = $candidate.Trim()
            if ($dirtyExitCode -eq 0 -and $dirtyState.Count -gt 0) {
                $commit += '-dirty'
            }
        }
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
