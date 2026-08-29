$ErrorActionPreference = 'Stop'

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("soop_literal_path_" + [Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    # '[' and ']' are valid Windows filename characters but wildcard metacharacters
    # to PowerShell path cmdlets. Keep them for the LiteralPath regression while
    # avoiding characters such as '|' that Windows forbids before path handling.
    # Build the Korean fixture from Unicode code points so this script remains
    # ASCII-only and parses identically in Windows PowerShell 5.1 without BOM.
    $specialFileName = -join ([char[]]@(
        0xB179,0xD654,0x005B,0xD14C,0xC2A4,0xD2B8,0x005D,
        0x003D,0xC81C,0xBAA9,0x002E,0x0074,0x0073
    ))
    if ($specialFileName.IndexOfAny([System.IO.Path]::GetInvalidFileNameChars()) -ge 0) {
        throw 'Literal-path fixture contains a Windows-invalid filename character.'
    }
    $specialPath = Join-Path $tempDir $specialFileName
    [System.IO.File]::WriteAllBytes($specialPath, [byte[]](1..32))

    if (-not (Test-Path -LiteralPath $specialPath -PathType Leaf)) {
        throw 'Literal-path recording file was not found.'
    }
    if ((Get-Item -LiteralPath $specialPath).Length -ne 32) {
        throw 'Literal-path recording size was not read correctly.'
    }

    $backendPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'backend\SOOP_LIVE.ps1'
    $source = (Get-Content -LiteralPath $backendPath -Raw -Encoding UTF8) + "`n" +
        ((Get-ChildItem -LiteralPath (Join-Path (Split-Path $backendPath -Parent) 'modules') -Filter '*.ps1' -File |
            ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 }) -join "`n")
    if ($source -notmatch 'Test-Path\s+-LiteralPath\s+\$Recording\.File') {
        throw 'Recording watchdog literal-path guard is missing.'
    }
    if ($source -match 'Test-Path\s+\$(?:Recording|rec)\.File') {
        throw 'Wildcard-sensitive recording Test-Path usage was reintroduced.'
    }
    if ($source -notmatch 'while\s*\(Test-Path\s+-LiteralPath\s+\$candidate\)') {
        throw 'Collision-safe output naming no longer uses a literal path.'
    }

    Write-Host 'Backend literal-path regression tests passed.'
}
finally {
    Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
