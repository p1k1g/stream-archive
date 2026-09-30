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
    $guiBuild = Read-RepoFile 'rust-gui/build.rs'
    $guiManifest = Read-RepoFile 'rust-gui/Cargo.toml'
    $checkWorkflow = Read-RepoFile '.github/workflows/rust-runtime-check.yml'
    $releaseWorkflow = Read-RepoFile '.github/workflows/rust-runtime-release.yml'

    Assert-Match $windowsBuild 'StreamArchive\.exe' 'Windows portable package must retain the Native GUI.'
    Assert-NotMatch $windowsBuild 'stream-archive-server\.exe|RUN_HEADLESS\.bat' 'Windows portable package must not expose the optional headless surface.'
    Assert-Match $windowsVerify "'stream-archive-server\.exe'" 'Windows verifier must explicitly reject the removed headless binary.'
    Assert-Match $windowsVerify "'RUN_HEADLESS\.bat'" 'Windows verifier must explicitly reject the removed headless launcher.'
    Assert-Match $windowsVerify "'stream-archive-icon\.png'" 'Windows verifier must reject a standalone branding PNG runtime dependency.'
    Assert-Match $windowsVerify "'stream-archive\.ico'" 'Windows verifier must reject a standalone ICO runtime dependency.'
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
    Assert-Match $unixVerify 'assert_cli_output_contains[^\r\n]*stream-archive-cli[^\r\n]*version' 'Unix verifier must execute the packaged CLI version check through captured output.'
    Assert-Match $unixVerify 'stream-archive-cli" help' 'Unix verifier must execute packaged CLI help.'
    Assert-Match $unixVerify 'assert_cli_output_contains' 'Unix verifier must capture CLI output before substring assertions.'
    $unixVerifyPipeScan = $unixVerify -replace '\\\r?\n\s*', ' '
    $unsafeCliDirectPipe = 'stream-archive-cli[^\r\n|]*\|(?!\|)'
    $unsafeCliVariablePipe = '"?\$(?:\{cli\}|cli)"?[^\r\n|]*\|(?!\|)'
    $unsafeCliHelperPipe = '(?m)^\s*(?:assert_cli_output_contains|assert_output_dir)\b[^\r\n|]*\|(?!\|)'
    Assert-NotMatch $unixVerifyPipeScan $unsafeCliDirectPipe 'Unix verifier must not pipe packaged Rust CLI stdout directly to any downstream command. Logical OR (||) remains allowed; capture CLI output before downstream assertions instead.'
    Assert-NotMatch $unixVerifyPipeScan $unsafeCliVariablePipe 'Unix verifier helpers must not pipe a CLI invoked indirectly through $cli/${cli}; capture the CLI output first.'
    Assert-NotMatch $unixVerifyPipeScan $unsafeCliHelperPipe 'Unix verifier must not pipe helper calls that proxy packaged CLI stdout to downstream consumers.'
    Assert-Match $unixVerify 'settings_json="\$\("\$cli" settings show --json\)"' 'assert_output_dir must capture CLI JSON before parsing it.'
    Assert-Match $unixVerify 'printf ''%s\\n'' "\$settings_json" \| python3 -c' 'assert_output_dir must parse captured JSON rather than piping the Rust CLI process directly.'
    if ('"$cli" "$@" | grep -q "$expected"' -notmatch $unsafeCliVariablePipe) {
        throw 'Packaging pipeline guard regression: indirect $cli pipeline was not detected.'
    }
    if ('assert_cli_output_contains "$cli" x status | grep -q x' -notmatch $unsafeCliHelperPipe) {
        throw 'Packaging pipeline guard regression: CLI helper pipeline was not detected.'
    }
    if ('"$cli" status || fail "status failed"' -match $unsafeCliVariablePipe) {
        throw 'Packaging pipeline guard regression: logical OR was misclassified as a pipeline.'
    }
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
    Assert-Match $unixMetadata 'if DIRTY_STATE="\$\(git status --porcelain --untracked-files=normal 2>/dev/null\)"; then' 'Unix release metadata must require git status to succeed before attributing a commit.'
    Assert-Match $unixMetadata 'COMMIT="\$CANDIDATE"' 'Unix release metadata must assign commit provenance only after the dirty-state check succeeds.'
    Assert-Match $unixMetadata '-dirty' 'Unix dirty release provenance marker is missing.'
    Assert-Match $windowsMetadata 'RepositoryRoot' 'Windows release metadata must support an explicit repository provenance root.'
    Assert-Match $windowsMetadata 'Get-GitHeadCommitFromMetadata' 'Windows release metadata must resolve HEAD from the explicit root Git metadata.'
    Assert-Match $windowsMetadata 'commondir' 'Windows release metadata must resolve linked-worktree refs through the shared Git commondir.'
    Assert-Match $windowsMetadata 'GITHUB_WORKSPACE' 'Windows CI release provenance must bind the explicit root to the workflow checkout.'
    Assert-Match $windowsMetadata '\$gitHeadCommit -ne \$env:GITHUB_SHA\.ToLowerInvariant\(\)' 'Windows release metadata must reject a GitHub workspace whose HEAD does not match GITHUB_SHA.'
    Assert-Match $windowsMetadata '\$provenanceCommit -match' 'Windows release metadata must gate commit attribution on validated repository provenance.'
    Assert-Match $windowsMetadata 'safe\.directory=\$resolvedRepositoryRoot' 'Windows Git status checks must scope safe-directory trust to the explicit repository root.'
    Assert-Match $windowsMetadata 'git -c \$safeDirectoryArgument -C \$resolvedRepositoryRoot status --porcelain --untracked-files=normal' 'Local Windows release metadata must detect dirty worktrees from the validated repository root.'
    Assert-Match $windowsMetadata '-dirty' 'Local Windows dirty release provenance marker is missing.'
    Assert-Match $windowsBuild '-RepositoryRoot "\."' 'Windows portable packaging must anchor release provenance to the repository checkout.'
    Assert-NotMatch $windowsBuild 'Set-WindowsExecutableIcon\.ps1|GeneratedIconPath' 'Windows portable packaging must not mutate PE icon resources after linking.'
    Assert-NotMatch $guiBuild 'generate_windows_icon|image::' 'Windows build must not regenerate icon pixels at build time.'
    Assert-Match $guiBuild 'assets/stream-archive-icon\.png' 'Slint branding must track the canonical PNG source.'
    Assert-Match $guiBuild 'assets/stream-archive\.ico' 'Windows resource build must use the canonical checked-in ICO asset.'
    Assert-Match $guiBuild 'winresource::WindowsResource' 'Windows icon must be linked through the standard Windows resource compiler path.'
    Assert-Match $guiBuild 'set_icon\("assets/stream-archive\.ico"\)' 'Windows resource build must attach the canonical ICO to the executable.'
    Assert-NotMatch $guiManifest '(?m)^image\s*=' 'Windows icon build must not require image decoding/generation dependencies.'
    Assert-Match $guiManifest 'winresource\s*=\s*"=0\.1\.31"' 'Windows resource compiler build dependency must remain locked.'
    Assert-Match $windowsVerify 'Assert-EmbeddedApplicationIcon' 'Windows package verification must reject cropped or blank embedded application icons.'
    Assert-Match $gitignore '(?m)^dist/\r?$' 'Generated release staging must be ignored so clean packaging does not self-mark provenance dirty.'

    Assert-Match $checkWorkflow '(?s)pull_request:\s+paths:.*?\.gitignore' 'PR packaging checks must trigger when the root .gitignore changes.'
    Assert-Match $checkWorkflow '(?s)push:.*?paths:.*?\.gitignore' 'Main-branch packaging checks must trigger when the root .gitignore changes.'
    Assert-Match $checkWorkflow 'BUILD_UNIX_PACKAGE\.sh' 'PR CI must smoke the canonical Unix package builder.'
    Assert-Match $checkWorkflow 'Verify-WindowsPackage\.ps1' 'PR CI must use the reusable Windows package verifier.'
    Assert-Match $checkWorkflow 'New-WindowsReleaseArchive\.ps1' 'PR CI must verify the canonical Windows archive path.'
    Assert-Match $checkWorkflow 'Windows release archiver accepted non-empty runtime data' 'PR CI must regress the clean-data archive boundary.'
    Assert-Match $checkWorkflow 'RELEASE_INFO_DIRTY\.txt' 'Unix PR CI must regress dirty release provenance.'
    Assert-Match $checkWorkflow 'RELEASE_INFO_STATUS_FAILURE\.txt' 'Unix PR CI must regress git-status provenance failures.'
    Assert-Match $checkWorkflow 'exit 42' 'Unix PR CI must force git status to fail while leaving rev-parse available.'
    Assert-Match $checkWorkflow 'grep -qx ''commit=unknown'' "\$status_failure"' 'Unix PR CI must retain unknown provenance when git status fails.'
    Assert-Match $checkWorkflow 'explicit non-Git RepositoryRoot must use commit=unknown' 'Windows PR CI must reject ambient GITHUB_SHA provenance for an explicit non-Git source root.'
    Assert-Match $checkWorkflow '-RepositoryRoot \$scratch' 'Windows source-archive regression must pass an explicit non-Git repository root.'
    Assert-Match $checkWorkflow 'checkout metadata must match GITHUB_SHA' 'Windows PR CI must verify checkout provenance against the workflow commit.'
    Assert-Match $checkWorkflow 'trusted GitHub checkout dirty metadata must include the -dirty provenance marker' 'Windows PR CI must regress dirty provenance for the actual GitHub workspace.'
    Assert-Match $checkWorkflow 'phase23-workspace-dirty-probe\.tmp' 'Windows PR CI must dirty the actual GitHub workspace when testing trusted checkout provenance.'
    Assert-Match $checkWorkflow 'git worktree add -b' 'Windows PR CI must exercise a branch-attached linked Git worktree.'
    Assert-Match $checkWorkflow 'linked worktree metadata must resolve commondir refs' 'Windows PR CI must regress linked-worktree commit provenance.'
    Assert-Match $checkWorkflow 'linked worktree dirty metadata must include the -dirty provenance marker' 'Windows PR CI must regress dirty linked-worktree provenance.'
    Assert-Match $checkWorkflow '-RepositoryRoot "\$env:GITHUB_WORKSPACE"' 'Windows checkout provenance regression must use the explicit repository root.'

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
