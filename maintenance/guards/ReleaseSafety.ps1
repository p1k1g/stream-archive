. (Join-Path $PSScriptRoot 'Common.ps1')

$root = $script:RuntimeContractsRoot
Push-Location $root
try {
    $tracked = (& git ls-files) | Where-Object {
        $_ -and $_ -ne 'maintenance/guards/ReleaseSafety.ps1'
    }
    if ($LASTEXITCODE -ne 0) { throw 'git ls-files failed' }

    $textExtensions = @('.rs','.js','.css','.html','.md','.ps1','.bat','.cmd','.yml','.yaml','.toml','.ini','.txt','.json','.example')
    $rules = @(
        @{ Name = 'GitHub classic token'; Pattern = 'ghp_[A-Za-z0-9]{30,}' },
        @{ Name = 'GitHub fine-grained token'; Pattern = 'github_pat_[A-Za-z0-9_]{40,}' },
        @{ Name = 'OpenAI-style API key'; Pattern = 'sk-[A-Za-z0-9_-]{20,}' },
        @{ Name = 'AWS access key'; Pattern = 'AKIA[0-9A-Z]{16}' },
        @{ Name = 'Private key material'; Pattern = '-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----' },
        @{ Name = 'Local developer user path'; Pattern = '(?i)C:\\Users\\pokga(?:\\|/)' },
        @{ Name = 'Non-empty SOOP password assignment'; Pattern = '(?im)^[ \t]*SOOP_PASSWORD[ \t]*=[ \t]*(?!<[^>\r\n]+>[ \t]*$)(?!CHANGE_ME[ \t]*$)(?!YOUR_[A-Z0-9_]+[ \t]*$)\S[^\r\n]*$' },
        @{ Name = 'Non-empty Cloudflare API key assignment'; Pattern = '(?im)^[ \t]*CLOUDFLARE_API_KEY[ \t]*=[ \t]*(?!<[^>\r\n]+>[ \t]*$)(?!CHANGE_ME[ \t]*$)(?!YOUR_[A-Z0-9_]+[ \t]*$)\S[^\r\n]*$' },
        @{ Name = 'Non-empty CHZZK NID_AUT assignment'; Pattern = '(?im)^[ \t]*(?:CHZZK_)?NID_AUT[ \t]*=[ \t]*(?!<[^>\r\n]+>[ \t]*$)(?!CHANGE_ME[ \t]*$)(?!YOUR_[A-Z0-9_]+[ \t]*$)\S[^\r\n]*$' },
        @{ Name = 'Non-empty CHZZK NID_SES assignment'; Pattern = '(?im)^[ \t]*(?:CHZZK_)?NID_SES[ \t]*=[ \t]*(?!<[^>\r\n]+>[ \t]*$)(?!CHANGE_ME[ \t]*$)(?!YOUR_[A-Z0-9_]+[ \t]*$)\S[^\r\n]*$' },
        @{ Name = 'Non-empty management token assignment'; Pattern = '(?im)^[ \t]*STREAM_ARCHIVE_TOKEN[ \t]*=[ \t]*(?!<[^>\r\n]+>[ \t]*$)(?!CHANGE_ME[ \t]*$)(?!YOUR_[A-Z0-9_]+[ \t]*$)\S[^\r\n]*$' },
        @{ Name = 'Discord webhook'; Pattern = 'https://(?:canary\.|ptb\.)?discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9._-]+' },
        @{ Name = 'Teams legacy webhook'; Pattern = 'https://[^\s]+\.webhook\.office\.com/webhookb2/[A-Za-z0-9@/_-]+' }
    )

    $violations = New-Object System.Collections.Generic.List[string]
    foreach ($relative in $tracked) {
        $extension = [IO.Path]::GetExtension($relative).ToLowerInvariant()
        if ($textExtensions -notcontains $extension -and [IO.Path]::GetFileName($relative) -notin @('.gitignore','.gitattributes')) { continue }
        $path = Join-Path $root $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
        try {
            $text = Get-Content -LiteralPath $path -Raw -Encoding UTF8
        } catch {
            continue
        }
        foreach ($rule in $rules) {
            if ($text -match $rule.Pattern) {
                $violations.Add("Current tree - $($rule.Name): $relative")
            }
        }
    }

    # Public visibility exposes reachable Git history, not only the current tree.
    # Scan historical patch content without printing matching secret values.
    $historyLines = & git log --all --no-color --format= --patch --unified=0 -- . ':(exclude)maintenance/guards/ReleaseSafety.ps1'
    if ($LASTEXITCODE -ne 0) { throw 'git history scan failed' }
    $historyText = (($historyLines | ForEach-Object {
        if ($_ -match '^[+-](?![+-])') { $_.Substring(1) } else { $_ }
    }) -join "`n")
    foreach ($rule in $rules) {
        if ($historyText -match $rule.Pattern) {
            $violations.Add("Git history - $($rule.Name)")
        }
    }
    $historyText = $null
    $historyLines = $null

    if ($violations.Count -gt 0) {
        $violations | Sort-Object -Unique | ForEach-Object { Write-Error $_ }
        throw "Public release safety scan found $($violations.Count) potential secret/privacy issue(s). Rewrite or remove affected history before making the repository public."
    }

    $workflow = Read-RepoFile '.github/workflows/rust-runtime-check.yml'
    $releaseWorkflow = Read-RepoFile '.github/workflows/rust-runtime-release.yml'
    $package = Read-RepoFile 'BUILD_PORTABLE.bat'
    $gitignore = Read-RepoFile '.gitignore'
    $runDev = Read-RepoFile 'RUN_DEV.bat'
    $releaseMetadata = Read-RepoFile 'maintenance/Write-ReleaseMetadata.ps1'
    $readme = Read-RepoFile 'README.md'
    $agents = Read-RepoFile 'AGENTS.md'
    $contributing = Read-RepoFile 'CONTRIBUTING.md'
    $unixCli = Read-RepoFile 'docs/UNIX_CLI.md'
    $manifest = Read-RepoFile 'rust-runtime/Cargo.toml'
    $guiManifest = Read-RepoFile 'rust-gui/Cargo.toml'

    foreach ($activeRuntimePath in @(
        @{ Name = 'PR workflow'; Text = $workflow },
        @{ Name = 'portable build'; Text = $package },
        @{ Name = 'developer runner'; Text = $runDev },
        @{ Name = 'release metadata'; Text = $releaseMetadata },
        @{ Name = 'README'; Text = $readme },
        @{ Name = 'AGENTS'; Text = $agents },
        @{ Name = 'CONTRIBUTING'; Text = $contributing },
        @{ Name = 'Unix CLI guide'; Text = $unixCli },
        @{ Name = 'GUI manifest'; Text = $guiManifest }
    )) {
        Assert-NotMatch $activeRuntimePath.Text 'rust-web[\\/]' "Active runtime path must use rust-runtime, not rust-web: $($activeRuntimePath.Name)"
    }
    if (Test-Path -LiteralPath (Join-Path $root '.github/workflows/rust-web-check.yml') -PathType Leaf) { throw 'Retired rust-web check workflow filename must not return.' }
    if (Test-Path -LiteralPath (Join-Path $root '.github/workflows/rust-web-release.yml') -PathType Leaf) { throw 'Retired rust-web release workflow filename must not return.' }
    Assert-Match $releaseWorkflow 'workflow_dispatch' 'Canonical runtime release workflow must retain manual workflow_dispatch.'
    Assert-Match $gitignore '(?m)^rust-runtime/target/\r?$' 'Git ignore must cover the canonical rust-runtime Cargo target directory.'
    Assert-NotMatch $gitignore '(?m)^rust-web/target/\r?    Assert-Match $workflow 'maintenance/Test-RuntimeContracts\.ps1' 'CI must call the single runtime-contract entry point.'
    Assert-NotMatch $workflow 'Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety' 'CI must not call superseded guard entry points.'
    Assert-NotMatch $workflow 'runs-on:\s*\[?self-hosted' 'Public CI must not depend on a private self-hosted runner.'
    Assert-Match $workflow 'fetch-depth:\s*0' 'Public-release CI must fetch full history for the history safety scan.'
    foreach ($trigger in @(
        'rust-runtime/\*\*',
        'rust-gui/\*\*',
        'RUN_DEV\.bat',
        'BUILD_PORTABLE\.bat',
        'maintenance/\*\*',
        'docs/\*\*'
    )) {
        Assert-Match $workflow $trigger "Runtime workflow path coverage is missing: $trigger"
    }
    Assert-NotMatch $workflow 'BUILD_RELEASE\.bat' 'Runtime workflow must not reference the retired BUILD_RELEASE.bat wrapper.'
    if (Test-Path -LiteralPath (Join-Path $root 'BUILD_RELEASE.bat') -PathType Leaf) {
        throw 'Retired BUILD_RELEASE.bat wrapper must not exist; use BUILD_PORTABLE.bat or Cargo directly.'
    }
    Assert-Match $workflow 'BUILD_PORTABLE\.bat' 'Portable package smoke step is missing.'
    Assert-Match $workflow 'Verify portable package' 'Portable package verification step is missing.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-runtime\\Cargo\.toml"' 'Portable package must perform the locked shared/headless runtime release build directly.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-gui\\Cargo\.toml"' 'Portable package must perform the locked native GUI release build directly.'
    Assert-Match $package 'StreamArchive\.exe' 'Portable package must include the native Stream Archive GUI.'
    Assert-Match $package 'stream-archive-server\.exe' 'Portable package must retain the compatible headless runtime binary.'
    Assert-Match $package '(?s)>"%OUT%\\RUN\.bat".*?StreamArchive\.exe' 'RUN.bat generation must make the native GUI the default entry point.'
    Assert-Match $package '(?s)>"%OUT%\\RUN_HEADLESS\.bat".*?stream-archive-server\.exe' 'RUN_HEADLESS.bat must launch the compatible headless runtime.'
    Assert-NotMatch $package 'stream-archive-launcher\.exe|RUN_WEB\.bat|RUN_SERVER_CONSOLE\.bat|Caddyfile\.example|REVERSE_PROXY\.md|LOCAL_LAUNCHER\.md' 'Retired Web launcher/proxy package artifacts must not return.'
    Assert-Match $package 'dist\\stream-archive' 'Portable package output must use the Stream Archive namespace.'
    Assert-Match $package 'docs\\OPERATIONS\.md' 'Portable package must include current operations guidance.'
    Assert-Match $package 'THIRD_PARTY_NOTICES\.md' 'Portable package must include third-party notices.'
    Assert-Match $package 'LICENSE' 'Portable package must include the project license.'
    Assert-Match $manifest 'name\s*=\s*"stream-archive-server"' 'Cargo package must keep the compatible shared/headless runtime name.'
    Assert-NotMatch $manifest '(?m)^\s*(?:axum|tokio-stream|tower-http|tracing|tracing-subscriber)\s*=' 'Retired Web-only direct dependencies must not return to rust-runtime.'
    Assert-Match $manifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Cargo package must declare AGPL-3.0-or-later.'
    Assert-Match $guiManifest 'name\s*=\s*"stream-archive-gui"' 'Native GUI Cargo package must use the Stream Archive namespace.'
    Assert-Match $guiManifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Native GUI Cargo package must declare AGPL-3.0-or-later.'
    Assert-NotMatch $package 'soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web' 'Portable packaging must not reintroduce generic legacy app names.'
    Write-Host "Release safety contracts passed across $($tracked.Count) tracked files and reachable Git history."
}
finally {
    Pop-Location
}
 'Git ignore must not retain the retired rust-web Cargo target directory.'
Assert-Match $gitignore '(?m)^backend/\.stream-archive/\r?    Assert-Match $workflow 'maintenance/Test-RuntimeContracts\.ps1' 'CI must call the single runtime-contract entry point.'
    Assert-NotMatch $workflow 'Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety' 'CI must not call superseded guard entry points.'
    Assert-NotMatch $workflow 'runs-on:\s*\[?self-hosted' 'Public CI must not depend on a private self-hosted runner.'
    Assert-Match $workflow 'fetch-depth:\s*0' 'Public-release CI must fetch full history for the history safety scan.'
    foreach ($trigger in @(
        'rust-runtime/\*\*',
        'rust-gui/\*\*',
        'RUN_DEV\.bat',
        'BUILD_PORTABLE\.bat',
        'maintenance/\*\*',
        'docs/\*\*'
    )) {
        Assert-Match $workflow $trigger "Runtime workflow path coverage is missing: $trigger"
    }
    Assert-NotMatch $workflow 'BUILD_RELEASE\.bat' 'Runtime workflow must not reference the retired BUILD_RELEASE.bat wrapper.'
    if (Test-Path -LiteralPath (Join-Path $root 'BUILD_RELEASE.bat') -PathType Leaf) {
        throw 'Retired BUILD_RELEASE.bat wrapper must not exist; use BUILD_PORTABLE.bat or Cargo directly.'
    }
    Assert-Match $workflow 'BUILD_PORTABLE\.bat' 'Portable package smoke step is missing.'
    Assert-Match $workflow 'Verify portable package' 'Portable package verification step is missing.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-runtime\\Cargo\.toml"' 'Portable package must perform the locked shared/headless runtime release build directly.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-gui\\Cargo\.toml"' 'Portable package must perform the locked native GUI release build directly.'
    Assert-Match $package 'StreamArchive\.exe' 'Portable package must include the native Stream Archive GUI.'
    Assert-Match $package 'stream-archive-server\.exe' 'Portable package must retain the compatible headless runtime binary.'
    Assert-Match $package '(?s)>"%OUT%\\RUN\.bat".*?StreamArchive\.exe' 'RUN.bat generation must make the native GUI the default entry point.'
    Assert-Match $package '(?s)>"%OUT%\\RUN_HEADLESS\.bat".*?stream-archive-server\.exe' 'RUN_HEADLESS.bat must launch the compatible headless runtime.'
    Assert-NotMatch $package 'stream-archive-launcher\.exe|RUN_WEB\.bat|RUN_SERVER_CONSOLE\.bat|Caddyfile\.example|REVERSE_PROXY\.md|LOCAL_LAUNCHER\.md' 'Retired Web launcher/proxy package artifacts must not return.'
    Assert-Match $package 'dist\\stream-archive' 'Portable package output must use the Stream Archive namespace.'
    Assert-Match $package 'docs\\OPERATIONS\.md' 'Portable package must include current operations guidance.'
    Assert-Match $package 'THIRD_PARTY_NOTICES\.md' 'Portable package must include third-party notices.'
    Assert-Match $package 'LICENSE' 'Portable package must include the project license.'
    Assert-Match $manifest 'name\s*=\s*"stream-archive-server"' 'Cargo package must keep the compatible shared/headless runtime name.'
    Assert-NotMatch $manifest '(?m)^\s*(?:axum|tokio-stream|tower-http|tracing|tracing-subscriber)\s*=' 'Retired Web-only direct dependencies must not return to rust-runtime.'
    Assert-Match $manifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Cargo package must declare AGPL-3.0-or-later.'
    Assert-Match $guiManifest 'name\s*=\s*"stream-archive-gui"' 'Native GUI Cargo package must use the Stream Archive namespace.'
    Assert-Match $guiManifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Native GUI Cargo package must declare AGPL-3.0-or-later.'
    Assert-NotMatch $package 'soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web' 'Portable packaging must not reintroduce generic legacy app names.'
    Write-Host "Release safety contracts passed across $($tracked.Count) tracked files and reachable Git history."
}
finally {
    Pop-Location
}
 'Git ignore must cover the canonical runtime-private transient state directory.'
Assert-Match $gitignore '(?m)^backend/\.rust-web/\r?    Assert-Match $workflow 'maintenance/Test-RuntimeContracts\.ps1' 'CI must call the single runtime-contract entry point.'
    Assert-NotMatch $workflow 'Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety' 'CI must not call superseded guard entry points.'
    Assert-NotMatch $workflow 'runs-on:\s*\[?self-hosted' 'Public CI must not depend on a private self-hosted runner.'
    Assert-Match $workflow 'fetch-depth:\s*0' 'Public-release CI must fetch full history for the history safety scan.'
    foreach ($trigger in @(
        'rust-runtime/\*\*',
        'rust-gui/\*\*',
        'RUN_DEV\.bat',
        'BUILD_PORTABLE\.bat',
        'maintenance/\*\*',
        'docs/\*\*'
    )) {
        Assert-Match $workflow $trigger "Runtime workflow path coverage is missing: $trigger"
    }
    Assert-NotMatch $workflow 'BUILD_RELEASE\.bat' 'Runtime workflow must not reference the retired BUILD_RELEASE.bat wrapper.'
    if (Test-Path -LiteralPath (Join-Path $root 'BUILD_RELEASE.bat') -PathType Leaf) {
        throw 'Retired BUILD_RELEASE.bat wrapper must not exist; use BUILD_PORTABLE.bat or Cargo directly.'
    }
    Assert-Match $workflow 'BUILD_PORTABLE\.bat' 'Portable package smoke step is missing.'
    Assert-Match $workflow 'Verify portable package' 'Portable package verification step is missing.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-runtime\\Cargo\.toml"' 'Portable package must perform the locked shared/headless runtime release build directly.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-gui\\Cargo\.toml"' 'Portable package must perform the locked native GUI release build directly.'
    Assert-Match $package 'StreamArchive\.exe' 'Portable package must include the native Stream Archive GUI.'
    Assert-Match $package 'stream-archive-server\.exe' 'Portable package must retain the compatible headless runtime binary.'
    Assert-Match $package '(?s)>"%OUT%\\RUN\.bat".*?StreamArchive\.exe' 'RUN.bat generation must make the native GUI the default entry point.'
    Assert-Match $package '(?s)>"%OUT%\\RUN_HEADLESS\.bat".*?stream-archive-server\.exe' 'RUN_HEADLESS.bat must launch the compatible headless runtime.'
    Assert-NotMatch $package 'stream-archive-launcher\.exe|RUN_WEB\.bat|RUN_SERVER_CONSOLE\.bat|Caddyfile\.example|REVERSE_PROXY\.md|LOCAL_LAUNCHER\.md' 'Retired Web launcher/proxy package artifacts must not return.'
    Assert-Match $package 'dist\\stream-archive' 'Portable package output must use the Stream Archive namespace.'
    Assert-Match $package 'docs\\OPERATIONS\.md' 'Portable package must include current operations guidance.'
    Assert-Match $package 'THIRD_PARTY_NOTICES\.md' 'Portable package must include third-party notices.'
    Assert-Match $package 'LICENSE' 'Portable package must include the project license.'
    Assert-Match $manifest 'name\s*=\s*"stream-archive-server"' 'Cargo package must keep the compatible shared/headless runtime name.'
    Assert-NotMatch $manifest '(?m)^\s*(?:axum|tokio-stream|tower-http|tracing|tracing-subscriber)\s*=' 'Retired Web-only direct dependencies must not return to rust-runtime.'
    Assert-Match $manifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Cargo package must declare AGPL-3.0-or-later.'
    Assert-Match $guiManifest 'name\s*=\s*"stream-archive-gui"' 'Native GUI Cargo package must use the Stream Archive namespace.'
    Assert-Match $guiManifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Native GUI Cargo package must declare AGPL-3.0-or-later.'
    Assert-NotMatch $package 'soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web' 'Portable packaging must not reintroduce generic legacy app names.'
    Write-Host "Release safety contracts passed across $($tracked.Count) tracked files and reachable Git history."
}
finally {
    Pop-Location
}
 'Legacy CHZZK temp state must remain ignored while bounded stale cleanup compatibility is retained.'
    Assert-Match $workflow 'maintenance/Test-RuntimeContracts\.ps1' 'CI must call the single runtime-contract entry point.'
    Assert-NotMatch $workflow 'Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety' 'CI must not call superseded guard entry points.'
    Assert-NotMatch $workflow 'runs-on:\s*\[?self-hosted' 'Public CI must not depend on a private self-hosted runner.'
    Assert-Match $workflow 'fetch-depth:\s*0' 'Public-release CI must fetch full history for the history safety scan.'
    foreach ($trigger in @(
        'rust-runtime/\*\*',
        'rust-gui/\*\*',
        'RUN_DEV\.bat',
        'BUILD_PORTABLE\.bat',
        'maintenance/\*\*',
        'docs/\*\*'
    )) {
        Assert-Match $workflow $trigger "Runtime workflow path coverage is missing: $trigger"
    }
    Assert-NotMatch $workflow 'BUILD_RELEASE\.bat' 'Runtime workflow must not reference the retired BUILD_RELEASE.bat wrapper.'
    if (Test-Path -LiteralPath (Join-Path $root 'BUILD_RELEASE.bat') -PathType Leaf) {
        throw 'Retired BUILD_RELEASE.bat wrapper must not exist; use BUILD_PORTABLE.bat or Cargo directly.'
    }
    Assert-Match $workflow 'BUILD_PORTABLE\.bat' 'Portable package smoke step is missing.'
    Assert-Match $workflow 'Verify portable package' 'Portable package verification step is missing.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-runtime\\Cargo\.toml"' 'Portable package must perform the locked shared/headless runtime release build directly.'
    Assert-Match $package 'cargo build --locked --release --manifest-path "\.\\rust-gui\\Cargo\.toml"' 'Portable package must perform the locked native GUI release build directly.'
    Assert-Match $package 'StreamArchive\.exe' 'Portable package must include the native Stream Archive GUI.'
    Assert-Match $package 'stream-archive-server\.exe' 'Portable package must retain the compatible headless runtime binary.'
    Assert-Match $package '(?s)>"%OUT%\\RUN\.bat".*?StreamArchive\.exe' 'RUN.bat generation must make the native GUI the default entry point.'
    Assert-Match $package '(?s)>"%OUT%\\RUN_HEADLESS\.bat".*?stream-archive-server\.exe' 'RUN_HEADLESS.bat must launch the compatible headless runtime.'
    Assert-NotMatch $package 'stream-archive-launcher\.exe|RUN_WEB\.bat|RUN_SERVER_CONSOLE\.bat|Caddyfile\.example|REVERSE_PROXY\.md|LOCAL_LAUNCHER\.md' 'Retired Web launcher/proxy package artifacts must not return.'
    Assert-Match $package 'dist\\stream-archive' 'Portable package output must use the Stream Archive namespace.'
    Assert-Match $package 'docs\\OPERATIONS\.md' 'Portable package must include current operations guidance.'
    Assert-Match $package 'THIRD_PARTY_NOTICES\.md' 'Portable package must include third-party notices.'
    Assert-Match $package 'LICENSE' 'Portable package must include the project license.'
    Assert-Match $manifest 'name\s*=\s*"stream-archive-server"' 'Cargo package must keep the compatible shared/headless runtime name.'
    Assert-NotMatch $manifest '(?m)^\s*(?:axum|tokio-stream|tower-http|tracing|tracing-subscriber)\s*=' 'Retired Web-only direct dependencies must not return to rust-runtime.'
    Assert-Match $manifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Cargo package must declare AGPL-3.0-or-later.'
    Assert-Match $guiManifest 'name\s*=\s*"stream-archive-gui"' 'Native GUI Cargo package must use the Stream Archive namespace.'
    Assert-Match $guiManifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Native GUI Cargo package must declare AGPL-3.0-or-later.'
    Assert-NotMatch $package 'soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web' 'Portable packaging must not reintroduce generic legacy app names.'
    Write-Host "Release safety contracts passed across $($tracked.Count) tracked files and reachable Git history."
}
finally {
    Pop-Location
}
