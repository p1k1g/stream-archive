param(
    [Parameter(Mandatory = $true)]
    [string]$BackupFile,
    [string]$DataDir = $env:STREAM_ARCHIVE_DATA_DIR
)

$ErrorActionPreference = 'Stop'

function Resolve-DataDir {
    param([string]$Requested)
    if (-not [string]::IsNullOrWhiteSpace($Requested)) {
        return [System.IO.Path]::GetFullPath($Requested)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\data'))
}

function Assert-RuntimeStopped {
    $native = Get-Process -Name 'StreamArchive', 'stream-archive-gui' -ErrorAction SilentlyContinue
    $server = Get-Process -Name 'stream-archive-server' -ErrorAction SilentlyContinue
    if ($native -or $server) {
        throw 'Stream Archive is running. Close StreamArchive.exe and stop the optional headless runtime before restoring SQLite.'
    }
}

function Assert-SqliteFile {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "SQLite database not found: $Path"
    }
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 16) {
        throw "SQLite database is too small: $Path"
    }
    $header = [System.Text.Encoding]::ASCII.GetString($bytes, 0, 16)
    if ($header -ne "SQLite format 3`0") {
        throw "Invalid SQLite header: $Path"
    }
}

Assert-RuntimeStopped
$source = [System.IO.Path]::GetFullPath($BackupFile)
Assert-SqliteFile $source

$metadataPath = "$source.json"
if (Test-Path -LiteralPath $metadataPath) {
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    if ($metadata.sha256) {
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $source).Hash.ToLowerInvariant()
        if ($actual -ne ([string]$metadata.sha256).ToLowerInvariant()) {
            throw 'Backup SHA256 does not match its metadata file.'
        }
    }
}

$dataRoot = Resolve-DataDir $DataDir
New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
$target = Join-Path $dataRoot 'stream-archive.db'

if (Test-Path -LiteralPath $target) {
    $stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
    $appRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $safetyDir = Join-Path (Split-Path $appRoot -Parent) 'stream-archive-backups'
    New-Item -ItemType Directory -Force -Path $safetyDir | Out-Null
    $safety = Join-Path $safetyDir "stream_archive_pre_restore_$stamp.db"
    Copy-Item -LiteralPath $target -Destination $safety -Force
    Write-Host "Current database safety copy: $safety"
}

foreach ($suffix in @('-wal', '-shm')) {
    $sidecar = "$target$suffix"
    if (Test-Path -LiteralPath $sidecar) {
        Remove-Item -LiteralPath $sidecar -Force
    }
}

$temp = "$target.restore.tmp"
Copy-Item -LiteralPath $source -Destination $temp -Force
Assert-SqliteFile $temp
Move-Item -LiteralPath $temp -Destination $target -Force

Write-Host "Restore complete: $target"
Write-Host 'Start Stream Archive and verify settings, channels, and history.'
