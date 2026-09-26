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

function Get-GitHeadCommitFromMetadata {
    param([Parameter(Mandatory = $true)][string]$Root)

    $gitEntry = Join-Path $Root '.git'
    $gitDirectory = $null
    if (Test-Path -LiteralPath $gitEntry -PathType Container) {
        $gitDirectory = (Resolve-Path -LiteralPath $gitEntry).Path
    }
    elseif (Test-Path -LiteralPath $gitEntry -PathType Leaf) {
        $gitDirLine = (Get-Content -LiteralPath $gitEntry -TotalCount 1).Trim()
        if ($gitDirLine -match '^gitdir:\s*(.+)$') {
            $gitDirValue = $Matches[1].Trim()
            if ([System.IO.Path]::IsPathRooted($gitDirValue)) {
                $gitDirectory = [System.IO.Path]::GetFullPath($gitDirValue)
            }
            else {
                $gitDirectory = [System.IO.Path]::GetFullPath((Join-Path $Root $gitDirValue))
            }
        }
    }

    if ([string]::IsNullOrWhiteSpace($gitDirectory)) {
        return $null
    }

    # Linked worktrees keep HEAD under .git/worktrees/<name>, while branch refs
    # and packed-refs live in the shared directory referenced by commondir.
    $commonGitDirectory = $gitDirectory
    $commonDirPath = Join-Path $gitDirectory 'commondir'
    if (Test-Path -LiteralPath $commonDirPath -PathType Leaf) {
        $commonDirValue = (Get-Content -LiteralPath $commonDirPath -TotalCount 1).Trim()
        if (-not [string]::IsNullOrWhiteSpace($commonDirValue)) {
            if ([System.IO.Path]::IsPathRooted($commonDirValue)) {
                $commonGitDirectory = [System.IO.Path]::GetFullPath($commonDirValue)
            }
            else {
                $commonGitDirectory = [System.IO.Path]::GetFullPath((Join-Path $gitDirectory $commonDirValue))
            }
        }
    }

    $headPath = Join-Path $gitDirectory 'HEAD'
    if (-not (Test-Path -LiteralPath $headPath -PathType Leaf)) {
        return $null
    }

    $head = (Get-Content -LiteralPath $headPath -TotalCount 1).Trim()
    if ($head -match '^[0-9a-fA-F]{40}$') {
        return $head.ToLowerInvariant()
    }

    if ($head -match '^ref:\s*(.+)$') {
        $refName = $Matches[1].Trim()
        $refRelativePath = $refName -replace '/', [System.IO.Path]::DirectorySeparatorChar
        $refRoots = @($gitDirectory)
        if ($commonGitDirectory -ne $gitDirectory) {
            $refRoots += $commonGitDirectory
        }

        foreach ($refRoot in $refRoots) {
            $refPath = Join-Path $refRoot $refRelativePath
            if (Test-Path -LiteralPath $refPath -PathType Leaf) {
                $refValue = (Get-Content -LiteralPath $refPath -TotalCount 1).Trim()
                if ($refValue -match '^[0-9a-fA-F]{40}$') {
                    return $refValue.ToLowerInvariant()
                }
            }
        }

        $packedRefsPath = Join-Path $commonGitDirectory 'packed-refs'
        if (Test-Path -LiteralPath $packedRefsPath -PathType Leaf) {
            foreach ($line in Get-Content -LiteralPath $packedRefsPath) {
                if ($line -match '^([0-9a-fA-F]{40})\s+(.+)$' -and $Matches[2] -eq $refName) {
                    return $Matches[1].ToLowerInvariant()
                }
            }
        }
    }

    return $null
}

# Git provenance is optional so GitHub source archives without .git still build.
# In GitHub Actions, GITHUB_SHA is trusted only when RepositoryRoot is exactly
# GITHUB_WORKSPACE and the root's own .git/HEAD resolves to that SHA. Extracted
# source archives therefore cannot inherit provenance from the calling workflow.
$commit = 'unknown'
$resolvedRepositoryRoot = $null
if (-not [string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    if (-not (Test-Path -LiteralPath $RepositoryRoot -PathType Container)) {
        throw "RepositoryRoot does not exist: $RepositoryRoot"
    }
    $resolvedRepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
}

$gitHeadCommit = $null
if ($null -ne $resolvedRepositoryRoot) {
    $gitHeadCommit = Get-GitHeadCommitFromMetadata -Root $resolvedRepositoryRoot
}

$provenanceCommit = $null
if ($gitHeadCommit -match '^[0-9a-fA-F]{40}$') {
    $provenanceCommit = $gitHeadCommit
}

if (
    $null -ne $resolvedRepositoryRoot -and
    $env:GITHUB_ACTIONS -eq 'true' -and
    -not [string]::IsNullOrWhiteSpace($env:GITHUB_WORKSPACE) -and
    (Test-Path -LiteralPath $env:GITHUB_WORKSPACE -PathType Container)
) {
    $resolvedGitHubWorkspace = (Resolve-Path -LiteralPath $env:GITHUB_WORKSPACE).Path
    if ($resolvedGitHubWorkspace -eq $resolvedRepositoryRoot) {
        if (
            [string]::IsNullOrWhiteSpace($env:GITHUB_SHA) -or
            $env:GITHUB_SHA -notmatch '^[0-9a-fA-F]{40}$' -or
            $gitHeadCommit -ne $env:GITHUB_SHA.ToLowerInvariant()
        ) {
            $provenanceCommit = $null
        }
    }
}

# A validated commit is not enough by itself: always inspect the worktree state
# before attributing an artifact to that commit, including GITHUB_WORKSPACE.
if (
    $null -ne $resolvedRepositoryRoot -and
    $provenanceCommit -match '^[0-9a-fA-F]{40}$' -and
    (Get-Command git -ErrorAction SilentlyContinue)
) {
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'SilentlyContinue'
        $safeDirectoryArgument = "safe.directory=$resolvedRepositoryRoot"
        $dirtyState = @(& git -c $safeDirectoryArgument -C $resolvedRepositoryRoot status --porcelain --untracked-files=normal 2>$null)
        $dirtyExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    if ($dirtyExitCode -eq 0) {
        $commit = $provenanceCommit.Substring(0, 12)
        if ($dirtyState.Count -gt 0) {
            $commit += '-dirty'
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
