$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$generated = Join-Path $root '.generated\SOOPLiveWinUI'
$overlay = Join-Path $root 'overlay'
$backend = Join-Path $root 'backend'
$project = Join-Path $generated 'SOOPLiveWinUI.csproj'

if (-not (Test-Path -LiteralPath $project -PathType Leaf)) {
    throw 'Generated WinUI project is missing. Run PREPARE_PROJECT.bat first.'
}

Get-ChildItem -LiteralPath $overlay -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $generated $_.Name) -Force
}

$assetSource = Join-Path $overlay 'Assets'
$assetTarget = Join-Path $generated 'Assets'
if (Test-Path -LiteralPath $assetSource -PathType Container) {
    New-Item -ItemType Directory -Path $assetTarget -Force | Out-Null
    Get-ChildItem -LiteralPath $assetSource -File | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $assetTarget $_.Name) -Force
    }
}

$backendTarget = Join-Path $generated 'backend'
New-Item -ItemType Directory -Path $backendTarget -Force | Out-Null
Get-ChildItem -LiteralPath $backend -Recurse -File | ForEach-Object {
    $relative = $_.FullName.Substring($backend.Length).TrimStart([char[]]"\/")
    $destination = Join-Path $backendTarget $relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $_.FullName -Destination $destination -Force
}

foreach ($runtimeFile in @(
    @{ Target = 'SOOP_LIVE_SETTING.ini'; Example = 'SOOP_LIVE_SETTING.example.ini' },
    @{ Target = 'SOOP_LIVE_CHANNELS.txt'; Example = 'SOOP_LIVE_CHANNELS.example.txt' }
)) {
    $targetPath = Join-Path $backendTarget $runtimeFile.Target
    $sourcePath = Join-Path $backend $runtimeFile.Target
    $examplePath = Join-Path $backendTarget $runtimeFile.Example
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf) -and
        (Test-Path -LiteralPath $examplePath -PathType Leaf)) {
        Copy-Item -LiteralPath $examplePath -Destination $targetPath -Force
    }
}

$versionText = Get-Content -LiteralPath (Join-Path $root 'VERSION.txt') -Raw
$match = [regex]::Match($versionText, '(?m)^Version:\s*(\S+)\s*$')
if (-not $match.Success) { throw 'VERSION.txt does not contain a Version value.' }
$version = $match.Groups[1].Value
[xml]$xml = Get-Content -LiteralPath $project -Raw
$propertyGroup = $xml.Project.PropertyGroup | Select-Object -First 1
$propertyGroup.Version = $version
$propertyGroup.InformationalVersion = $version
$xml.Save($project)

function Get-Sha256([string]$Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        try { return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '') }
        finally { $stream.Dispose() }
    }
    finally { $sha.Dispose() }
}

foreach ($source in Get-ChildItem -LiteralPath $overlay -File) {
    $target = Join-Path $generated $source.Name
    if ((Get-Sha256 $source.FullName) -ne (Get-Sha256 $target)) {
        throw "Overlay synchronization failed: $($source.Name)"
    }
}

foreach ($requiredBackendFile in @(
    'SOOP_LIVE.ps1',
    'modules\SOOP.Security.ps1',
    'modules\SOOP.Core.ps1',
    'modules\SOOP.Network.ps1',
    'modules\SOOP.Recorder.ps1'
)) {
    $source = Join-Path $backend $requiredBackendFile
    $target = Join-Path $backendTarget $requiredBackendFile
    if (-not (Test-Path -LiteralPath $target -PathType Leaf) -or
        (Get-Sha256 $source) -ne (Get-Sha256 $target)) {
        throw "Backend synchronization failed: $requiredBackendFile"
    }
}

Write-Host "[OK] Source synchronized: $version"
