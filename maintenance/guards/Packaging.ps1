. (Join-Path $PSScriptRoot 'Common.ps1')

$root = $script:RuntimeContractsRoot
Push-Location $root
try {
    $unixBuild = Read-RepoFile 'BUILD_UNIX_PACKAGE.sh'
    $unixVerify = Read-RepoFile 'maintenance/Verify-UnixPackage.sh'
    $unixMetadata = Read-RepoFile 'maintenance/Write-ReleaseMetadata.sh'
    $windowsMetadata = Read-RepoFile 'maintenance/Write-ReleaseMetadata.ps1'
    $gitignore = Read-RepoFile '.gitignore'
    $windowsBuild = Read-RepoFile 'BUILD_PORTABLE.bat'
    $windowsVerify = Read-RepoFile 'maintenance/Verify-WindowsPackage.ps1'
    $windowsArchive = Read-RepoFile 'maintenance/New-WindowsReleaseArchive.ps1'
    $checkWorkflow = Read-RepoFile '.github/workflows/rust-runtime-check.yml'
    $releaseWorkflow = Read-RepoFile '.github/workflows/rust-runtime-release.yml'

    Assert-Match $windowsBuild 'StreamArchive\.exe' 'Windows portable package must retain the Native GUI.'
    Assert-Match $windowsBuild 'stream-archive-server\.exe' 'Windows portable package must retain the compatibility headless runtime.'
    Assert-Match $windowsVerify 'SHA256SUMS\.txt' 'Windows package verifier must validate package-local checksums.'
    Assert-Match $windowsVerify 'RequireCleanData' 'Official Windows release validation must support an empty-data contract.'
    Assert-Match $windowsArchive 'stream-archive-windows-' 'Windows release archive naming contract is missing.'
    Assert-Match $windowsArchive '\.sha256' 'Windows archive checksum generation is missing.'
    Assert-Match $windowsArchive 'Verify-WindowsPackage\.ps1' 'Windows archiver must invoke the canonical package verifier before opening a release ZIP.'
    Assert-Match $windowsArchive 'RequireCleanData' 'Windows archiver must reject preserved runtime data before release archiving.'

    Assert-Match $unixBuild 'cargo build --locked --release' 'Unix package build must use a locked release build.'
    Assert-Match $unixBuild 'bin/stream-archive-cli' 'Unix package must include stream-archive-cli.'
    Assert-Match $unixBuild 'bin/stream-archive-server' 'Unix package must include the compatibility headless runtime.'
    Assert-Match $unixBuild 'docs/UNIX_CLI\.md' 'Unix package must include Unix CLI documentation.'
    Assert-Match $unixBuild 'docs/OPERATIONS\.md' 'Unix package must include operations documentation.'
    Assert-Match $unixBuild 'THIRD_PARTY_NOTICES\.md' 'Unix package must include third-party notices.'
    Assert-Match $unixBuild 'LICENSE' 'Unix package must include the project license.'
    Assert-Match $unixBuild 'SHA256SUMS\.txt' 'Unix package-local checksum manifest is missing.'
    Assert-Match $unixBuild 'stream-archive-\$\{PLATFORM\}-\$\{ARCH\}\.tar\.gz' 'Unix release archive naming contract is missing.'
    Assert-Match $unixBuild 'ARCHIVE_CHECKSUM=' 'Unix archive checksum generation is missing.'
    Assert-Match $unixBuild 'Verify-UnixPackage\.sh' 'Unix package builder must call the reusable verifier.'

    Assert-Match $unixVerify 'shasum -a 256 -c SHA256SUMS\.txt' 'Unix verifier must validate package-local checksums.'
    Assert-Match $unixVerify 'Archive checksum mismatch' 'Unix verifier must validate archive checksum.'
    Assert-Match $unixVerify 'stream-archive-cli" version' 'Unix verifier must execute the packaged CLI.'
    Assert-Match $unixVerify 'stream-archive-cli" help' 'Unix verifier must execute packaged CLI help.'
    Assert-Match $unixVerify 'stream-archive-cli" init' 'Unix archive smoke must initialize from the extracted package.'
    Assert-Match $unixVerify 'status --json' 'Unix archive smoke must execute packaged status JSON.'
    Assert-Match $unixVerify 'mktemp -d "[^"]*\s[^"]*\.XXXXXX"' 'Unix archive smoke must exercise a whitespace path.'
    Assert-Match $unixVerify 'mktemp -d "[^"]*[^\x00-\x7F][^"]*\.XXXXXX"' 'Unix archive smoke must exercise a non-ASCII path.'
    Assert-Match $unixVerify 'Release package data directory must be empty' 'Unix verifier must reject bundled runtime data.'
    foreach ($tool in @('streamlink','yt-dlp','ffmpeg')) {
        Assert-Match $unixVerify $tool "Unix package verifier must reject bundled media tool: $tool"
    }

    Assert-Match $unixMetadata 'cargo metadata --locked --no-deps' 'Unix release metadata must source version from Cargo metadata.'
    Assert-Match $unixMetadata 'commit=\$COMMIT' 'Unix release metadata must include commit provenance.'
    Assert-Match $unixMetadata 'COMMIT="unknown"' 'Unix release metadata must support source archives without Git metadata.'
    Assert-Match $unixMetadata 'git status --porcelain --untracked-files=normal' 'Unix release metadata must detect dirty worktrees.'
    Assert-Match $unixMetadata '-dirty' 'Unix dirty release provenance marker is missing.'
    Assert-Match $windowsMetadata 'RepositoryRoot' 'Windows release metadata must support an explicit repository provenance root.'
    Assert-Match $windowsMetadata 'safe\.directory=\$resolvedRepositoryRoot' 'Windows release metadata must scope Git safe-directory trust to the explicit repository root.'
    Assert-Match $windowsMetadata 'git -c \$safeDirectoryArgument -C \$resolvedRepositoryRoot status --porcelain --untracked-files=normal' 'Windows release metadata must detect dirty worktrees from the explicit repository root.'
    Assert-Match $windowsMetadata '-dirty' 'Windows dirty release provenance marker is missing.'
    Assert-Match $windowsBuild '-RepositoryRoot "\."' 'Windows portable packaging must anchor release provenance to the repository checkout.'
    Assert-Match $gitignore '(?m)^dist/\r?$' 'Generated release staging must be ignored so clean packaging does not self-mark provenance dirty.'

    Assert-Match $checkWorkflow 'BUILD_UNIX_PACKAGE\.sh' 'PR CI must smoke the canonical Unix package builder.'
    Assert-Match $checkWorkflow 'Verify-WindowsPackage\.ps1' 'PR CI must use the reusable Windows package verifier.'
    Assert-Match $checkWorkflow 'New-WindowsReleaseArchive\.ps1' 'PR CI must verify the canonical Windows archive path.'
    Assert-Match $checkWorkflow 'Windows release archiver accepted non-empty runtime data' 'PR CI must regress the clean-data archive boundary.'
    Assert-Match $checkWorkflow 'dirty checkout metadata must include the -dirty provenance marker' 'PR CI must regress dirty release provenance.'
    Assert-Match $checkWorkflow '-RepositoryRoot "\$env:GITHUB_WORKSPACE"' 'Windows dirty provenance regression must use the explicit checkout root.'

    Assert-Match $releaseWorkflow 'workflow_dispatch' 'Release artifact workflow must remain manual.'
    Assert-Match $releaseWorkflow 'release-contracts:' 'Manual release artifacts must be gated by a release contract job.'
    Assert-Match $releaseWorkflow 'Test-RuntimeContracts\.ps1' 'Manual release workflow must enforce the canonical runtime/packaging contracts.'
    Assert-Match $releaseWorkflow 'needs: release-contracts' 'Artifact jobs must wait for release contract validation.'
    Assert-Match $releaseWorkflow 'windows-latest' 'Release artifact workflow must build Windows natively.'
    Assert-Match $releaseWorkflow 'ubuntu-latest' 'Release artifact workflow must build Linux natively.'
    Assert-Match $releaseWorkflow 'macos-latest' 'Release artifact workflow must build macOS natively.'
    Assert-Match $releaseWorkflow 'BUILD_UNIX_PACKAGE\.sh' 'Release workflow must reuse the Unix package builder.'
    Assert-Match $releaseWorkflow 'Verify-WindowsPackage\.ps1' 'Release workflow must reuse the Windows verifier.'
    Assert-Match $releaseWorkflow 'New-WindowsReleaseArchive\.ps1' 'Release workflow must reuse the Windows archiver.'
    Assert-Match $releaseWorkflow 'actions/upload-artifact@v4' 'Release workflow must upload verified workflow artifacts.'
    Assert-NotMatch $releaseWorkflow 'softprops/action-gh-release|gh\s+release|git\s+tag|create-release|upload-release-asset' 'Phase 23.6 must not publish a GitHub Release or create tags.'

    $runtimeMetadataJson = & cargo metadata --locked --no-deps --format-version 1 --manifest-path '.\rust-runtime\Cargo.toml'
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed for rust-runtime' }
    $runtimeMetadata = $runtimeMetadataJson | ConvertFrom-Json
    $runtimeVersion = ($runtimeMetadata.packages | Where-Object { $_.name -eq 'stream-archive-server' } | Select-Object -First 1).version

    $guiMetadataJson = & cargo metadata --locked --no-deps --format-version 1 --manifest-path '.\rust-gui\Cargo.toml'
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed for rust-gui' }
    $guiMetadata = $guiMetadataJson | ConvertFrom-Json
    $guiVersion = ($guiMetadata.packages | Where-Object { $_.name -eq 'stream-archive-gui' } | Select-Object -First 1).version

    if ([string]::IsNullOrWhiteSpace([string]$runtimeVersion) -or [string]::IsNullOrWhiteSpace([string]$guiVersion)) {
        throw 'Unable to resolve release versions from Cargo metadata.'
    }
    if ([string]$runtimeVersion -ne [string]$guiVersion) {
        throw "Release version mismatch: runtime=$runtimeVersion gui=$guiVersion"
    }

    Write-Host "Packaging contracts passed for release version $runtimeVersion."
}
finally {
    Pop-Location
}
