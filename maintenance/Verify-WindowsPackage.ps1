param(
    [string]$Root = ".\dist\stream-archive",
    [switch]$RequireCleanData,
    [string]$ArchivePath = "",
    [string]$ArchiveChecksumPath = ""
)

$ErrorActionPreference = 'Stop'

function Assert-Leaf {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Missing package file: $Path"
    }
}

function Assert-Directory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "Missing package directory: $Path"
    }
}

function Test-PackageTree {
    param(
        [string]$PackageRoot,
        [bool]$CleanData
    )

    $resolved = (Resolve-Path -LiteralPath $PackageRoot).Path
    $required = @(
        'StreamArchive.exe',
        'stream-archive-server.exe',
        'RUN.bat',
        'RUN_HEADLESS.bat',
        'BACKUP_DATA.bat',
        'RESTORE_DATA.bat',
        'LICENSE',
        'THIRD_PARTY_NOTICES.md',
        'RELEASE_INFO.txt',
        'SHA256SUMS.txt',
        'maintenance\Backup-StreamArchiveData.ps1',
        'maintenance\Restore-StreamArchiveData.ps1',
        'docs\OPERATIONS.md'
    )
    foreach ($relative in $required) {
        Assert-Leaf (Join-Path $resolved $relative)
    }

    foreach ($relative in @('backend', 'backend\vod', 'data', 'maintenance', 'docs')) {
        Assert-Directory (Join-Path $resolved $relative)
    }

    foreach ($relative in @(
        'stream-archive-launcher.exe',
        'RUN_WEB.bat',
        'RUN_SERVER_CONSOLE.bat',
        'Caddyfile.example',
        'docs\REVERSE_PROXY.md',
        'docs\LOCAL_LAUNCHER.md'
    )) {
        $path = Join-Path $resolved $relative
        if (Test-Path -LiteralPath $path) {
            throw "Retired Web package file returned: $relative"
        }
    }

    $forbiddenNames = @(
        'streamlink.exe',
        'yt-dlp.exe',
        'ffmpeg.exe',
        'SOOP_LIVE_SETTING.ini',
        'SOOP_LIVE_CHANNELS.txt',
        'SOOP_VOD_SETTING.ini'
    )
    $forbidden = Get-ChildItem -LiteralPath $resolved -Recurse -Force -File |
        Where-Object {
            $_.Name -in $forbiddenNames -or
            $_.Name -like '*.stream-archive.claim' -or
            $_.Name -like '*.log'
        } |
        Select-Object -First 1
    if ($null -ne $forbidden) {
        throw "Forbidden bundled/runtime file: $($forbidden.FullName)"
    }

    if ($CleanData) {
        $dataRoot = Join-Path $resolved 'data'
        $dataEntry = Get-ChildItem -LiteralPath $dataRoot -Force | Select-Object -First 1
        if ($null -ne $dataEntry) {
            throw "Official package data directory must be empty: $($dataEntry.FullName)"
        }
        $database = Get-ChildItem -LiteralPath $resolved -Recurse -Force -File |
            Where-Object { $_.Name -match '\.db(?:-wal|-shm)?$' } |
            Select-Object -First 1
        if ($null -ne $database) {
            throw "Official package must not contain a runtime database: $($database.FullName)"
        }
    }

    $expected = @{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $resolved 'SHA256SUMS.txt')) {
        if ($line -match '^([0-9a-fA-F]{64})\s+(.+)$') {
            $expected[$matches[2].Trim()] = $matches[1].ToLowerInvariant()
        }
    }
    foreach ($name in @('StreamArchive.exe', 'stream-archive-server.exe')) {
        if (-not $expected.ContainsKey($name)) {
            throw "checksum missing: $name"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolved $name)).Hash.ToLowerInvariant()
        if ($expected[$name] -ne $actual) {
            throw "$name checksum mismatch"
        }
    }

    $run = Get-Content -LiteralPath (Join-Path $resolved 'RUN.bat') -Raw
    if ($run -notmatch 'StreamArchive\.exe') {
        throw 'RUN.bat does not use StreamArchive.exe'
    }
    if ($run -match 'launcher|RUN_WEB|127\.0\.0\.1|http') {
        throw 'RUN.bat must remain native-only'
    }

    $headless = Get-Content -LiteralPath (Join-Path $resolved 'RUN_HEADLESS.bat') -Raw
    if ($headless -notmatch 'stream-archive-server\.exe') {
        throw 'RUN_HEADLESS.bat does not use the compatibility headless runtime'
    }

    $releaseInfo = Get-Content -LiteralPath (Join-Path $resolved 'RELEASE_INFO.txt') -Raw
    if ($releaseInfo -notmatch '(?m)^product=Stream Archive\s*$') {
        throw 'release metadata product missing'
    }
    if ($releaseInfo -notmatch '(?m)^version=\S+\s*$') {
        throw 'release metadata version missing'
    }

    $license = Get-Content -LiteralPath (Join-Path $resolved 'LICENSE') -Raw
    if ($license -notmatch 'GNU AFFERO GENERAL PUBLIC LICENSE') {
        throw 'AGPL license text missing from package'
    }

    Write-Host "Windows package tree verified: $resolved"
}

Test-PackageTree -PackageRoot $Root -CleanData $RequireCleanData.IsPresent

if (-not [string]::IsNullOrWhiteSpace($ArchivePath)) {
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        throw "Archive does not exist: $ArchivePath"
    }
    if ([string]::IsNullOrWhiteSpace($ArchiveChecksumPath)) {
        $ArchiveChecksumPath = "$ArchivePath.sha256"
    }
    if (-not (Test-Path -LiteralPath $ArchiveChecksumPath -PathType Leaf)) {
        throw "Archive checksum does not exist: $ArchiveChecksumPath"
    }

    $checksumLine = Get-Content -LiteralPath $ArchiveChecksumPath | Select-Object -First 1
    if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+(.+)$') {
        throw 'Archive checksum format is invalid'
    }
    $expectedArchiveHash = $matches[1].ToLowerInvariant()
    $actualArchiveHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
    if ($expectedArchiveHash -ne $actualArchiveHash) {
        throw 'Archive checksum mismatch'
    }

    $scratch = Join-Path $env:RUNNER_TEMP ("Stream Archive Release 테스트-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch -Force | Out-Null
    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $scratch -Force
        Test-PackageTree -PackageRoot $scratch -CleanData $true
    }
    finally {
        Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Windows archive verified: $ArchivePath"
}
