. (Join-Path $PSScriptRoot 'Common.ps1')

$root = $script:RuntimeContractsRoot
Push-Location $root
try {
    $phase = Read-RepoFile 'docs/PHASE23_7_RELEASE_CANDIDATE_FINAL_QA.md'
    $unixVerify = Read-RepoFile 'maintenance/Verify-UnixPackage.sh'
    $windowsVerify = Read-RepoFile 'maintenance/Verify-WindowsPackage.ps1'
    $checkWorkflow = Read-RepoFile '.github/workflows/rust-runtime-check.yml'
    $releaseWorkflow = Read-RepoFile '.github/workflows/rust-runtime-release.yml'

    $runtimeMetadataJson = & cargo metadata --locked --no-deps --format-version 1 --manifest-path '.\rust-runtime\Cargo.toml'
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed for rust-runtime' }
    $runtimeMetadata = $runtimeMetadataJson | ConvertFrom-Json
    $runtimeVersion = ($runtimeMetadata.packages | Where-Object { $_.name -eq 'stream-archive-server' } | Select-Object -First 1).version

    $guiMetadataJson = & cargo metadata --locked --no-deps --format-version 1 --manifest-path '.\rust-gui\Cargo.toml'
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed for rust-gui' }
    $guiMetadata = $guiMetadataJson | ConvertFrom-Json
    $guiVersion = ($guiMetadata.packages | Where-Object { $_.name -eq 'stream-archive-gui' } | Select-Object -First 1).version

    if ([string]::IsNullOrWhiteSpace([string]$runtimeVersion) -or [string]::IsNullOrWhiteSpace([string]$guiVersion)) {
        throw 'Unable to resolve RC versions from Cargo metadata.'
    }
    if ([string]$runtimeVersion -ne [string]$guiVersion) {
        throw "RC version mismatch: runtime=$runtimeVersion gui=$guiVersion"
    }

    $releaseNotesPath = 'docs/RELEASE_NOTES_' + ([string]$runtimeVersion).Replace('.', '_') + '.md'
    if (-not (Test-Path -LiteralPath $releaseNotesPath -PathType Leaf)) {
        throw "Release notes are missing for RC version ${runtimeVersion}: $releaseNotesPath"
    }
    $releaseNotes = Read-RepoFile $releaseNotesPath

    foreach ($artifact in @(
        'stream-archive-windows-x64\.zip',
        'stream-archive-linux-x64\.tar\.gz',
        'stream-archive-macos-arm64\.tar\.gz'
    )) {
        Assert-Match $phase $artifact "RC checklist must retain the verified artifact matrix entry: $artifact"
    }

    Assert-Match $phase 'Automated RC validation' 'RC checklist must separate automated validation.'
    Assert-Match $phase 'Manual RC validation' 'RC checklist must separate manual validation.'
    Assert-Match $phase 'MANUAL TEST REQUIRED' 'RC checklist must not present all real-session checks as automated.'
    Assert-Match $phase 'Release blockers' 'RC checklist must identify release blockers explicitly.'
    Assert-Match $phase 'Public release readiness' 'RC checklist must state public-release readiness explicitly.'
    Assert-Match $phase 'cross-version upgrade' 'RC checklist must keep cross-version upgrade scope explicit.'
    Assert-Match $phase 'arbitrary schema downgrade' 'RC checklist must not imply arbitrary schema downgrade support.'

    Assert-Match $releaseNotes ([regex]::Escape([string]$runtimeVersion)) 'Release notes must identify the current Cargo version.'
    Assert-Match $releaseNotes 'Windows Native' 'Release notes must describe the Windows Native surface.'
    Assert-Match $releaseNotes 'Linux' 'Release notes must describe Linux support.'
    Assert-Match $releaseNotes 'macOS' 'Release notes must describe macOS support.'
    Assert-Match $releaseNotes 'Streamlink' 'Release notes must document external media-tool dependencies.'
    Assert-Match $releaseNotes 'upgrade' 'Release notes must include upgrade guidance.'

    Assert-Match $unixVerify 'backup create --json' 'Unix RC archive smoke must create a managed backup from the extracted package.'
    Assert-Match $unixVerify 'backup restore' 'Unix RC archive smoke must restore a managed backup.'
    Assert-Match $unixVerify 'replacement package' 'Unix RC archive smoke must re-extract a replacement package.'
    Assert-Match $unixVerify 'rc-fixture' 'Unix RC archive smoke must verify persisted channel data across restore/replacement.'
    Assert-Match $unixVerify 'assert_output_dir' 'Unix RC archive smoke must verify the exact persisted OUTPUT_DIR value.'
    Assert-Match $unixVerify 'OUTPUT_DIR mismatch' 'Unix RC setting persistence assertion must fail with an explicit mismatch.'
    Assert-Match $unixVerify 'runtime_data="\$scratch/runtime data' 'Unix RC archive smoke must retain an external whitespace runtime-data path.'
    Assert-Match $windowsVerify 'Archive checksum mismatch' 'Windows verifier must reject archive checksum mismatches.'

    Assert-Match $checkWorkflow 'Reject corrupted Unix release archive' 'PR CI must reject corrupted Unix RC archives.'
    Assert-Match $checkWorkflow 'Corrupted Unix release archive was accepted' 'Unix corruption regression must fail closed.'
    Assert-Match $checkWorkflow 'Windows offline backup restore smoke' 'PR CI must exercise packaged Windows offline backup/restore scripts.'
    Assert-Match $checkWorkflow 'Windows restore accepted a backup whose SHA256 no longer matched metadata' 'Windows restore corruption regression is missing.'
    Assert-Match $checkWorkflow 'Reject corrupted Windows release archive' 'PR CI must reject corrupted Windows RC archives.'
    Assert-Match $checkWorkflow 'Corrupted Windows release archive was accepted' 'Windows corruption regression must fail closed.'

    Assert-Match $releaseWorkflow 'workflow_dispatch' 'RC artifact workflow must remain manually dispatched.'
    Assert-Match $releaseWorkflow 'permissions:\s*\r?\n\s*contents:\s*read' 'RC artifact workflow must retain read-only repository contents permission.'
    Assert-Match $releaseWorkflow 'windows-latest' 'RC artifact workflow must build Windows natively.'
    Assert-Match $releaseWorkflow 'ubuntu-latest' 'RC artifact workflow must build Linux natively.'
    Assert-Match $releaseWorkflow 'macos-latest' 'RC artifact workflow must build macOS natively.'
    Assert-Match $releaseWorkflow 'BUILD_UNIX_PACKAGE\.sh' 'RC artifact workflow must reuse the canonical Unix package builder.'
    Assert-Match $releaseWorkflow 'New-WindowsReleaseArchive\.ps1' 'RC artifact workflow must reuse the canonical Windows archiver.'
    Assert-NotMatch $releaseWorkflow 'softprops/action-gh-release|gh\s+release|git\s+tag|create-release|upload-release-asset|cargo\s+publish' 'Phase 23.7 must not publish a release, tag, or package registry artifact.'

    Write-Host "Release Candidate contracts passed for version $runtimeVersion."
}
finally {
    Pop-Location
}
