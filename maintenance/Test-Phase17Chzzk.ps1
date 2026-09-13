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
$platformLive = Read-RepoFile 'rust-web/src/platform/live.rs'
$chzzk = Read-RepoFile 'rust-web/src/platform/chzzk/mod.rs'
$auth = Read-RepoFile 'rust-web/src/platform/chzzk/auth.rs'
$live = Read-RepoFile 'rust-web/src/platform/chzzk/live.rs'
$recorder = Read-RepoFile 'rust-web/src/recorder.rs'
$watcher = Read-RepoFile 'rust-web/src/native_watcher.rs'
$primary = Read-RepoFile 'rust-web/src/primary_config.rs'
$backend = Read-RepoFile 'rust-web/src/backend.rs'
$app = Read-RepoFile 'rust-web/web/app.js'
$phase8 = Read-RepoFile 'rust-web/web/phase8.js'
$phase14 = Read-RepoFile 'rust-web/web/phase14.js'

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

# AuthRequired recovery must stay behind the platform-neutral facade.
Assert-Match $platformLive 'pub async fn recover_auth' 'LIVE facade auth recovery entrypoint is missing.'
Assert-Match $platformLive 'Self::Chzzk\(session\)\s*=>\s*session\.recover_auth\(\)' 'CHZZK auth recovery is not dispatched through the LIVE facade.'
Assert-Match $live 'pub fn recover_auth\(&self\)' 'CHZZK provider-owned auth recovery guidance is missing.'
Assert-Match $watcher 'recover_auth_required\(session, config\)' 'Watcher does not use provider-neutral auth recovery.'
Assert-Match $watcher '\.recover_auth\(&config\.soop_username, &config\.soop_password\)' 'Watcher auth recovery is not routed through LiveSession.'
Assert-NotMatch $watcher 'CHZZK 제한 방송 인증' 'Provider-specific CHZZK auth policy leaked into the common watcher.'

# Streamlink plugin boundary / secret-safe cookie transport.
Assert-Match $recorder 'StreamInput::PluginUrl' 'Recorder does not accept provider plugin URLs.'
Assert-Match $recorder '"--can-handle-url"' 'Streamlink plugin preflight is missing.'
Assert-Match $recorder '"--http-cookies-file"' 'Streamlink cookie-file transport is missing.'
Assert-Match $recorder 'Netscape HTTP Cookie File' 'Netscape cookie-file generation is missing.'
Assert-Match $recorder 'COOKIE_FILE_EXPIRES_UNIX' 'CHZZK Netscape cookie entries must use a non-expired timestamp.'
Assert-Match $recorder '4_102_444_800' 'CHZZK Netscape cookie expiry must stay in the future.'
Assert-NotMatch $recorder '--http-cookie\s+NID_' 'CHZZK cookie values must not be exposed as direct process arguments.'
Assert-Match $recorder 'struct CookieFile\(PathBuf\)' 'Temporary CHZZK authentication cookie is not ownership-guarded.'
Assert-Match $recorder 'impl Drop for CookieFile' 'Temporary CHZZK authentication cookie cleanup guard is missing.'
Assert-Match $recorder 'let timestamp_player = if start_at_zero[\s\S]*?resolve_timestamp_rebase_ffmpeg[\s\S]*?let cookie_file = match cookies' 'Fallible FFmpeg/player setup must finish before plaintext cookie creation.'
Assert-Match $recorder 'cookie_file_guard_removes_plaintext_temp_file_on_drop' 'Plaintext CHZZK cookie cleanup regression test is missing.'

# CHZZK fMP4 must be remuxed live to a zero-based fragmented MP4 timeline.
# The CHZZK Streamlink HLS worker still owns segment fetching; FFmpeg is only
# the player/output sink, so this remains a live stream-copy and not a post job.
Assert-Match $platformLive 'start_at_zero:\s*bool' 'Plugin stream timestamp policy is missing from StreamInput.'
Assert-Match $live 'start_at_zero:\s*true' 'CHZZK LIVE does not request zero-based recording timestamps.'
Assert-Match $recorder 'resolve_timestamp_rebase_ffmpeg' 'Recorder cannot locate FFmpeg for CHZZK timestamp rebasing.'
Assert-Match $recorder '\.arg\("--player"\)' 'CHZZK timestamp path does not route Streamlink output through an FFmpeg player.'
Assert-Match $recorder '\.arg\("--player-args"\)' 'CHZZK timestamp path does not configure FFmpeg player arguments.'
Assert-Match $recorder '-copyts -start_at_zero' 'CHZZK FFmpeg stream-copy does not rebase copied timestamps to zero.'
Assert-Match $recorder 'frag_keyframe\+empty_moov\+default_base_moof' 'CHZZK output is not written as an in-progress-safe fragmented MP4.'
Assert-Match $recorder 'if let Some\(\(ffmpeg, player_args\)\) = timestamp_player[\s\S]*?--player[\s\S]*?else[\s\S]*?--output' 'SOOP direct output and CHZZK FFmpeg-player output are not separated.'
Assert-NotMatch $recorder '"--ffmpeg-start-at-zero"' 'The ineffective Streamlink mux-only --ffmpeg-start-at-zero path must not return.'
Assert-Match $recorder 'timestamp_rebase_player_args_keep_copyts_but_shift_to_zero_and_fragment_mp4' 'CHZZK timestamp-remux regression test is missing.'
Assert-Match $recorder 'finds_ffmpeg_from_official_streamlink_windows_layout' 'Bundled Streamlink FFmpeg discovery regression test is missing.'
Assert-Match $live 'assert!\(start_at_zero\)' 'CHZZK timestamp policy regression coverage is missing.'

# LIVE files must keep their real platform container extension and collision scan.
Assert-Match $platform 'pub const fn live_output_extension' 'Platform LIVE output extension mapping is missing.'
Assert-Match $platform 'Self::Soop\s*=>\s*"ts"' 'SOOP LIVE output must remain .ts.'
Assert-Match $platform 'Self::Chzzk\s*=>\s*"mp4"' 'CHZZK LIVE output must use .mp4.'
Assert-Match $recorder 'output_file_for_platform\(output_file, platform\)' 'Recorder does not apply the platform LIVE output extension.'
Assert-Match $recorder 'uses_platform_specific_live_output_extension_without_overwriting_existing_file' 'Platform output-extension regression test is missing.'
Assert-Match $watcher 'fn unique_output_file\([\s\S]*?platform: PlatformId' 'Filename generation does not receive the LIVE platform.'
Assert-Match $watcher 'let extension = platform\.live_output_extension\(\)' 'Filename collision scanning is not platform-extension aware.'
Assert-Match $watcher 'TITLE_NUMBER[\s\S]*?\{extension\}' 'TITLE_NUMBER collisions are not checked with the platform extension.'
Assert-Match $watcher 'title_number_collision_scans_with_platform_extension' 'Platform-aware TITLE_NUMBER collision regression test is missing.'

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

# Browser LIVE notification state must use the same composite identity.
Assert-Match $phase14 'function p14LiveKey\(c,index=0\)' 'LIVE notification composite-key helper is missing.'
Assert-Match $phase14 'c\?\.platform\|\|''SOOP''' 'LIVE notification key is not namespaced by platform.'
Assert-Match $phase14 'p14LiveStates\.set\(p14LiveKey\(' 'Initial LIVE notification state is still keyed only by account.'
Assert-Match $phase14 'const key=p14LiveKey\(' 'LIVE notification transitions are still keyed only by account.'

# Legacy channel-file migration must preserve LF, CRLF, and lone-CR compatibility.
Assert-Match $backend '\.replace\(''\\r'', "\\n"\)' 'Legacy lone-CR channel line ending normalization is missing.'
Assert-Match $backend 'fn parses_legacy_channel_line_endings\(\)' 'Legacy channel line-ending regression test is missing.'
Assert-Match $backend 'for separator in \["\\n", "\\r\\n", "\\r"\]' 'Legacy channel parser test does not cover LF/CRLF/lone-CR.'

# API-side validation and user-facing platform selection must remain connected.
Assert-Match $primary 'provider\(channel\.platform\)\s*\.validate_account' 'Server-side platform channel validation is missing.'
Assert-Match $app '<option value="CHZZK">CHZZK</option>' 'Channel UI CHZZK selector is missing.'
Assert-Match $app 'saveChzzkSecrets' 'CHZZK settings save flow is missing.'
Assert-Match $app 'CHZZK_NID_AUT' 'CHZZK NID_AUT settings UI binding is missing.'
Assert-Match $app 'CHZZK_NID_SES' 'CHZZK NID_SES settings UI binding is missing.'
Assert-Match $app '\[\$\{platform\}\]' 'Runtime platform label rendering is missing.'
Assert-Match $phase8 'function p8LiveRow\(x\).*x\.platform' 'Visible LIVE history renderer does not show the platform label.'

# Export and settings-tab transitions must preserve multiplatform UX.
Assert-Match $phase8 '\[''type'',''platform'',''status''' 'History CSV export is missing the platform column.'
Assert-Match $phase8 '\[''LIVE'',String\(x\.platform\|\|''SOOP''\)\.toUpperCase\(\)' 'LIVE CSV rows do not preserve platform identity.'
Assert-Match $app 'function deactivateChzzkSettings\(destination=''''\)' 'CHZZK settings cleanup does not inspect the destination tab.'
Assert-Match $app 'destination===''notifications''' 'Leaving CHZZK for Notifications does not preserve notification panel isolation.'
Assert-Match $app 'deactivateChzzkSettings\(tab\.dataset\.settingsTab\|\|''''\)' 'CHZZK settings tab listeners do not pass their destination identity.'

Write-Host 'Phase 17 CHZZK regression checks passed.'