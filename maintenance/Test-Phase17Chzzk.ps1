$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent

function Read-RepoFile([string]$relativePath) {
    $path = Join-Path $root $relativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Missing required Phase 17 file: $relativePath"
    }
    return Get-Content -LiteralPath $path -Raw -Encoding UTF8
}

function Assert-Match([string]$text, [string]$pattern, [string]$message) {
    if ($text -notmatch $pattern) { throw $message }
}

function Assert-NotMatch([string]$text, [string]$pattern, [string]$message) {
    if ($text -match $pattern) { throw $message }
}

$platform = Read-RepoFile 'rust-web/src/platform/mod.rs'
$chzzk = Read-RepoFile 'rust-web/src/platform/chzzk/mod.rs'
$auth = Read-RepoFile 'rust-web/src/platform/chzzk/auth.rs'
$live = Read-RepoFile 'rust-web/src/platform/chzzk/live.rs'
$recorder = Read-RepoFile 'rust-web/src/recorder.rs'
$watcher = Read-RepoFile 'rust-web/src/native_watcher.rs'
$primary = Read-RepoFile 'rust-web/src/primary_config.rs'
$backend = Read-RepoFile 'rust-web/src/backend.rs'
$app = Read-RepoFile 'rust-web/web/app.js'
$phase8 = Read-RepoFile 'rust-web/web/phase8.js'

# Provider registration / Phase 17 scope boundary.
Assert-Match $platform 'Chzzk' 'PlatformId::Chzzk registration is missing.'
Assert-Match $platform 'PlatformId::Chzzk\s*=>\s*&chzzk::CHZZK' 'CHZZK provider dispatch is missing.'
Assert-Match $chzzk 'live:\s*true' 'CHZZK LIVE capability must remain enabled.'
Assert-Match $chzzk 'vod:\s*false' 'CHZZK VOD must remain disabled until Phase 18.'
Assert-Match $chzzk 'account\.len\(\)\s*!=\s*32' 'CHZZK channel ID length validation is missing.'
Assert-Match $chzzk 'is_ascii_hexdigit' 'CHZZK channel ID hex validation is missing.'

# Authentication must stay encrypted-at-rest and hot-loadable.
Assert-Match $backend '"CHZZK_NID_AUT"' 'CHZZK_NID_AUT is not registered as a hidden setting.'
Assert-Match $backend '"CHZZK_NID_SES"' 'CHZZK_NID_SES is not registered as a hidden setting.'
Assert-Match $auth 'unprotect_secret' 'CHZZK auth does not decrypt through the common secret boundary.'
Assert-Match $auth 'pub fn load\(\)' 'CHZZK auth hot-load entrypoint is missing.'
Assert-Match $auth 'ChzzkAuthState::Missing' 'Missing CHZZK auth state coverage is missing.'
Assert-Match $auth 'ChzzkAuthState::Partial' 'Partial CHZZK auth state coverage is missing.'
Assert-Match $auth 'ChzzkAuthState::Configured' 'Configured CHZZK auth state coverage is missing.'

# LIVE discovery / restriction handling.
Assert-Match $live 'service/v2/channels/\{channel_id\}/live-detail' 'CHZZK live-detail endpoint is missing.'
Assert-Match $live 'membershipBenefitType' 'CHZZK membership-only restriction detection is missing.'
Assert-Match $live 'let requires_auth = adult \|\| membership_only' 'CHZZK restricted playback flag is missing.'
Assert-Match $live 'if !playback_available' 'CHZZK playback availability handling is missing.'
Assert-Match $live 'return auth_failure\(auth\)' 'Restricted playback auth handling is missing.'
Assert-Match $live 'ChzzkAuthState::Configured' 'Expired or unauthorized CHZZK auth path is missing.'
Assert-Match $live 'cookies:\s*if live\.requires_auth' 'Public CHZZK LIVE must not always forward stored cookies.'

# Streamlink plugin boundary / secret-safe cookie transport.
Assert-Match $recorder 'StreamInput::PluginUrl' 'Recorder does not accept provider plugin URLs.'
Assert-Match $recorder '"--can-handle-url"' 'Streamlink plugin preflight is missing.'
Assert-Match $recorder '"--http-cookies-file"' 'Streamlink cookie-file transport is missing.'
Assert-Match $recorder 'Netscape HTTP Cookie File' 'Netscape cookie-file generation is missing.'
Assert-Match $recorder 'COOKIE_FILE_EXPIRES_UNIX' 'CHZZK Netscape cookie entries must use a non-expired timestamp.'
Assert-Match $recorder '4_102_444_800' 'CHZZK Netscape cookie expiry must stay in the future.'
Assert-NotMatch $recorder '--http-cookie\s+NID_' 'CHZZK cookie values must not be exposed as direct process arguments.'

# CHZZK-only operation must not depend on SOOP Worker credentials.
Assert-Match $watcher 'fn channels_require_soop' 'SOOP credential gating helper is missing.'
Assert-Match $watcher 'require_soop:\s*bool' 'WatcherConfig does not receive the SOOP requirement boundary.'
Assert-Match $watcher 'CLOUDFLARE_API_KEY is empty while an enabled SOOP channel exists' 'SOOP Worker validation is not scoped to enabled SOOP channels.'
Assert-Match $watcher 'channels_require_soop\(&channels\)' 'Watcher startup does not derive SOOP credential requirements from channels.'

# Mixed-platform commands must use the composite platform/account identity.
Assert-Match $watcher 'fn scoped_channel_target' 'Watcher scoped channel-target parser is missing.'
Assert-Match $watcher 'channel_key\(platform, account\)' 'Watcher commands are not resolved through the composite platform/account key.'
Assert-Match $watcher 'scoped_channel_command_targets_composite_identity' 'Composite channel-command regression test is missing.'
Assert-Match $app 'function channelTarget\(platform,account\)' 'Runtime controls do not construct a platform-scoped channel target.'
Assert-Match $app 'channelAction\(platformId,c\.account' 'Runtime action buttons are not passing the platform.'
Assert-Match $app 'channelPassword\(platformId,c\.account\)' 'Runtime password control is not passing the platform.'

# API-side validation and user-facing platform selection must remain connected.
Assert-Match $primary 'provider\(channel\.platform\)\s*\.validate_account' 'Server-side platform channel validation is missing.'
Assert-Match $app '<option value="CHZZK">CHZZK</option>' 'Channel UI CHZZK selector is missing.'
Assert-Match $app 'saveChzzkSecrets' 'CHZZK settings save flow is missing.'
Assert-Match $app 'CHZZK_NID_AUT' 'CHZZK NID_AUT settings UI binding is missing.'
Assert-Match $app 'CHZZK_NID_SES' 'CHZZK NID_SES settings UI binding is missing.'
Assert-Match $app '\[\$\{platform\}\]' 'Runtime platform label rendering is missing.'
Assert-Match $phase8 'function p8LiveRow\(x\).*x\.platform' 'Visible LIVE history renderer does not show the platform label.'

Write-Host 'Phase 17 CHZZK regression checks passed.'
