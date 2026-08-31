# SOOP LIVE DPAPI secret compatibility module

$script:DpapiSecretPrefix = "dpapi:v1:"
$script:DpapiEntropy = [System.Text.Encoding]::UTF8.GetBytes("SOOPLiveDownloader:v1")

function Protect-DpapiSecret {
    param([string]$Value,[string]$SettingName)
    if ([string]::IsNullOrWhiteSpace($Value)) { return "" }
    $plain = [System.Text.Encoding]::UTF8.GetBytes($Value)
    try {
        Add-Type -AssemblyName System.Security -ErrorAction SilentlyContinue
        $encrypted = [System.Security.Cryptography.ProtectedData]::Protect(
            $plain,
            $script:DpapiEntropy,
            [System.Security.Cryptography.DataProtectionScope]::CurrentUser
        )
        return $script:DpapiSecretPrefix + [Convert]::ToBase64String($encrypted)
    }
    catch {
        throw "$SettingName DPAPI 보호에 실패했습니다. $($_.Exception.Message)"
    }
    finally {
        if ($null -ne $plain) { [Array]::Clear($plain,0,$plain.Length) }
        if ($null -ne $encrypted) { [Array]::Clear($encrypted,0,$encrypted.Length) }
    }
}

function Unprotect-DpapiSecret {
    param([string]$Value,[string]$SettingName)

    if ([string]::IsNullOrWhiteSpace($Value)) { return "" }
    if (-not $Value.StartsWith($script:DpapiSecretPrefix,[System.StringComparison]::OrdinalIgnoreCase)) {
        # Legacy plaintext is accepted in memory. The GUI migrates it to DPAPI
        # on the next explicit Settings save/import.
        return $Value
    }

    $encrypted = $null
    try {
        Add-Type -AssemblyName System.Security -ErrorAction SilentlyContinue
        $encoded = $Value.Substring($script:DpapiSecretPrefix.Length)
        $encrypted = [Convert]::FromBase64String($encoded)
        $plain = [System.Security.Cryptography.ProtectedData]::Unprotect(
            $encrypted,
            $script:DpapiEntropy,
            [System.Security.Cryptography.DataProtectionScope]::CurrentUser
        )
        try { return [System.Text.Encoding]::UTF8.GetString($plain) }
        finally { if ($null -ne $plain) { [Array]::Clear($plain,0,$plain.Length) } }
    }
    catch {
        throw "$SettingName DPAPI 자격증명을 해제하지 못했습니다. 암호화한 Windows 사용자로 실행하거나 GUI 설정에서 값을 다시 저장해 주세요. $($_.Exception.Message)"
    }
    finally {
        if ($null -ne $encrypted) { [Array]::Clear($encrypted,0,$encrypted.Length) }
    }
}

function Resolve-ProtectedConfigSecrets {
    param([hashtable]$Config)
    foreach ($name in @("SOOP_PASSWORD","CLOUDFLARE_API_KEY")) {
        if ($Config.ContainsKey($name)) {
            $Config[$name] = Unprotect-DpapiSecret -Value ([string]$Config[$name]) -SettingName $name
        }
    }
    return $Config
}

function Protect-IniSecretText {
    param([string]$Text)

    $options = [System.Text.RegularExpressions.RegexOptions]::Multiline -bor
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    return [regex]::Replace(
        $Text,
        '^(?<key>SOOP_PASSWORD|CLOUDFLARE_API_KEY)\s*=\s*(?<value>[^\r\n]*)(?<cr>\r?)$',
        [System.Text.RegularExpressions.MatchEvaluator]{
            param($match)
            $value = $match.Groups['value'].Value.Trim()
            $ending = $match.Groups['cr'].Value
            if ([string]::IsNullOrEmpty($value)) {
                return $match.Groups['key'].Value + '=' + $ending
            }
            $plain = Unprotect-DpapiSecret -Value $value -SettingName $match.Groups['key'].Value
            return $match.Groups['key'].Value + '=' +
                (Protect-DpapiSecret -Value $plain -SettingName $match.Groups['key'].Value) + $ending
        },
        $options
    )
}
