param(
    [string]$PackageRoot = ".\dist\stream-archive",
    [string]$OutputDir = ".\dist\release"
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$root = (Resolve-Path -LiteralPath $PackageRoot).Path
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDir).Path

$archName = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
    'X64' { 'x64' }
    'Arm64' { 'arm64' }
    default { throw "Unsupported Windows release architecture: $($_)" }
}

$archiveName = "stream-archive-windows-$archName.zip"
$archivePath = Join-Path $output $archiveName
$checksumPath = "$archivePath.sha256"
Remove-Item -LiteralPath $archivePath -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $checksumPath -Force -ErrorAction SilentlyContinue

$stream = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::CreateNew)
try {
    $zip = New-Object System.IO.Compression.ZipArchive(
        $stream,
        [System.IO.Compression.ZipArchiveMode]::Create,
        $false
    )
    try {
        foreach ($item in Get-ChildItem -LiteralPath $root -Recurse -Force) {
            $relative = $item.FullName.Substring($root.Length).TrimStart('\', '/').Replace('\', '/')
            if ($item.PSIsContainer) {
                if (-not [string]::IsNullOrWhiteSpace($relative)) {
                    [void]$zip.CreateEntry(($relative.TrimEnd('/') + '/'))
                }
                continue
            }
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip,
                $item.FullName,
                $relative,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    }
    finally {
        $zip.Dispose()
    }
}
finally {
    $stream.Dispose()
}

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
"$hash  $archiveName" | Set-Content -LiteralPath $checksumPath -Encoding ASCII

Write-Host "ARCHIVE=$archivePath"
Write-Host "ARCHIVE_CHECKSUM=$checksumPath"
