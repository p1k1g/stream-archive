param(
    [string]$DataDir = $env:STREAM_ARCHIVE_DATA_DIR,
    [string]$BackupDir = "",
    [int]$Keep = 10,
    [int]$RetentionDays = 30
)

$ErrorActionPreference = 'Stop'

function Resolve-DataDir {
    param([string]$Requested)
    if (-not [string]::IsNullOrWhiteSpace($Requested)) {
        return [System.IO.Path]::GetFullPath($Requested)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\data'))
}

function Assert-ServerStopped {
    $running = Get-Process -Name 'stream-archive-server' -ErrorAction SilentlyContinue
    if ($running) {
        throw 'Stream Archive server is running. Stop it with Ctrl+C before backing up SQLite.'
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

Assert-ServerStopped
$dataRoot = Resolve-DataDir $DataDir
$dbPath = Join-Path $dataRoot 'stream-archive.db'
Assert-SqliteFile $dbPath

if ([string]::IsNullOrWhiteSpace($BackupDir)) {
    $appRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $BackupDir = Join-Path (Split-Path $appRoot -Parent) 'stream-archive-backups'
}
$backupRoot = [System.IO.Path]::GetFullPath($BackupDir)
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null

$stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
$backupPath = Join-Path $backupRoot "stream_archive_manual_$stamp.db"
Copy-Item -LiteralPath $dbPath -Destination $backupPath -Force
Assert-SqliteFile $backupPath

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $backupPath).Hash.ToLowerInvariant()
$meta = [ordered]@{
    created_at = (Get-Date).ToString('o')
    source = $dbPath
    sha256 = $hash
    size_bytes = (Get-Item -LiteralPath $backupPath).Length
    kind = 'manual'
    version = 1
}
$meta | ConvertTo-Json | Set-Content -LiteralPath "$backupPath.json" -Encoding UTF8

function Get-OwnedBackupFiles {
    Get-ChildItem -LiteralPath $backupRoot -Filter 'stream_archive_*.db' -File | Where-Object {
        Test-Path -LiteralPath "$($_.FullName).json" -PathType Leaf
    }
}

if ($Keep -gt 0) {
    $old = Get-OwnedBackupFiles |
        Sort-Object LastWriteTime -Descending |
        Select-Object -Skip $Keep
    foreach ($item in $old) {
        Remove-Item -LiteralPath $item.FullName -Force
        $metaPath = "$($item.FullName).json"
        if (Test-Path -LiteralPath $metaPath) {
            Remove-Item -LiteralPath $metaPath -Force
        }
    }
}

if ($RetentionDays -gt 0) {
    $cutoff = (Get-Date).AddDays(-$RetentionDays)
    $aged = Get-OwnedBackupFiles | Where-Object { $_.LastWriteTime -lt $cutoff }
    foreach ($item in $aged) {
        Remove-Item -LiteralPath $item.FullName -Force
        $metaPath = "$($item.FullName).json"
        if (Test-Path -LiteralPath $metaPath) { Remove-Item -LiteralPath $metaPath -Force }
    }
}

Write-Host "Backup complete: $backupPath"
Write-Host "SHA256: $hash"
