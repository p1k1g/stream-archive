$ErrorActionPreference = 'Stop'

if (-not $script:RuntimeContractsRoot) {
    $script:RuntimeContractsRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
}

function Read-RepoFile([string]$RelativePath) {
    $path = Join-Path $script:RuntimeContractsRoot $RelativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Missing required runtime contract file: $RelativePath"
    }
    Get-Content -LiteralPath $path -Raw -Encoding UTF8
}

function Assert-Match([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) { throw $Message }
}

function Assert-NotMatch([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -match $Pattern) { throw $Message }
}

function Assert-RustTest([string]$Text, [string]$TestName, [string]$Message) {
    Assert-Match $Text ("fn\s+" + [regex]::Escape($TestName) + "\s*\(") $Message
}
