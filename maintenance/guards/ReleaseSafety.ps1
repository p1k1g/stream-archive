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

    $workflow = Read-RepoFile '.github/workflows/rust-web-check.yml'
    $package = Read-RepoFile 'BUILD_PORTABLE.bat'
    $manifest = Read-RepoFile 'rust-web/Cargo.toml'
    Assert-Match $workflow 'maintenance/Test-RuntimeContracts\.ps1' 'CI must call the single runtime-contract entry point.'
    Assert-NotMatch $workflow 'Test-Phase\d+|Test-ProcessLifecycle|Test-PublicReleaseSafety' 'CI must not call superseded guard entry points.'
    Assert-NotMatch $workflow 'runs-on:\s*\[?self-hosted' 'Public CI must not depend on a private self-hosted runner.'
    Assert-Match $workflow 'fetch-depth:\s*0' 'Public-release CI must fetch full history for the history safety scan.'
    foreach ($trigger in @(
        'rust-web/\*\*',
        'RUN_DEV\.bat',
        'BUILD_RELEASE\.bat',
        'BUILD_PORTABLE\.bat',
        'maintenance/\*\*',
        'docs/\*\*',
        'deploy/\*\*'
    )) {
        Assert-Match $workflow $trigger "Runtime workflow path coverage is missing: $trigger"
    }
    Assert-Match $workflow 'BUILD_PORTABLE\.bat' 'Portable package smoke step is missing.'
    Assert-Match $workflow 'Verify portable package' 'Portable package verification step is missing.'
    Assert-Match $package 'stream-archive-server\.exe' 'Portable package must include the Stream Archive server.'
    Assert-Match $package 'stream-archive-launcher\.exe' 'Portable package must include the native Stream Archive launcher.'
    Assert-Match $package 'dist\\stream-archive' 'Portable package output must use the Stream Archive namespace.'
    Assert-Match $package 'THIRD_PARTY_NOTICES\.md' 'Portable package must include third-party notices.'
    Assert-Match $package 'LICENSE' 'Portable package must include the project license.'
    Assert-Match $manifest 'name\s*=\s*"stream-archive-server"' 'Cargo package must use the Stream Archive namespace.'
    Assert-Match $manifest 'license\s*=\s*"AGPL-3\.0-or-later"' 'Cargo package must declare AGPL-3.0-or-later.'
    Assert-NotMatch $package 'soop-server|soop-launcher|soop-recorder|SOOP_NO_PAUSE|\.rust-web' 'Portable packaging must not reintroduce generic legacy app names.'
    Write-Host "Release safety contracts passed across $($tracked.Count) tracked files and reachable Git history."
}
finally {
    Pop-Location
}
