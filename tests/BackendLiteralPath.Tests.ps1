$ErrorActionPreference = 'Stop'

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("soop_literal_path_" + [Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    $specialPath = Join-Path $tempDir '녹화`[테스트`]|=제목.ts'
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
