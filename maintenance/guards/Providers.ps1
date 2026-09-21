. (Join-Path $PSScriptRoot 'Common.ps1')

$platform = Read-RepoFile 'rust-web/src/platform/mod.rs'
$platformLive = Read-RepoFile 'rust-web/src/platform/live.rs'
$chzzk = Read-RepoFile 'rust-web/src/platform/chzzk/mod.rs'
$auth = Read-RepoFile 'rust-web/src/platform/chzzk/auth.rs'
$live = Read-RepoFile 'rust-web/src/platform/chzzk/live.rs'
$recorder = Read-RepoFile 'rust-web/src/recorder.rs'
$watcher = Read-RepoFile 'rust-web/src/native_watcher.rs'
$primary = Read-RepoFile 'rust-web/src/primary_config.rs'
$backend = Read-RepoFile 'rust-web/src/backend.rs'
$soopVod = Read-RepoFile 'rust-web/src/platform/soop/vod.rs'
$core = Read-RepoFile 'rust-web/src/app_core.rs'
$guiSources = (Get-ChildItem (Join-Path $script:RuntimeContractsRoot 'rust-gui/src') -Filter '*.rs' -Recurse | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
$guiUi = (Get-ChildItem (Join-Path $script:RuntimeContractsRoot 'rust-gui/ui') -Filter '*.slint' -Recurse | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"

# Provider registration / CHZZK LIVE boundary. CHZZK VOD may enable VOD separately.
Assert-Match $platform 'Chzzk' 'PlatformId::Chzzk registration is missing.'
Assert-Match $platform 'PlatformId::Chzzk\s*=>\s*&chzzk::CHZZK' 'CHZZK provider dispatch is missing.'
Assert-Match $chzzk 'live:\s*true' 'CHZZK LIVE capability must remain enabled.'
Assert-Match $chzzk 'vod:\s*(?:false|true)' 'CHZZK provider VOD capability field is missing.'
Assert-Match $soopVod 'tools\.yt_dlp' 'SOOP VOD must remain yt-dlp based.'
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

# CHZZK fMP4 input must be remuxed live into genuine zero-based MPEG-TS.
# Streamlink keeps ownership of CHZZK HLS segment fetching; FFmpeg is only the
# player/output sink, so this remains a live stream-copy with no post-recording pass.
Assert-Match $platformLive 'start_at_zero:\s*bool' 'Plugin stream timestamp policy is missing from StreamInput.'
Assert-Match $live 'start_at_zero:\s*true' 'CHZZK LIVE does not request zero-based recording timestamps.'
Assert-Match $recorder 'resolve_timestamp_rebase_ffmpeg' 'Recorder cannot locate FFmpeg for CHZZK live remux.'
Assert-Match $recorder '\.arg\("--player"\)' 'CHZZK timestamp path does not route Streamlink output through an FFmpeg player.'
Assert-Match $recorder '\.arg\("--player-args"\)' 'CHZZK timestamp path does not configure FFmpeg player arguments.'
Assert-Match $recorder '-fflags \+genpts\+discardcorrupt' 'CHZZK FFmpeg input does not regenerate timestamps / discard corrupt packets.'
Assert-Match $recorder '-bsf:v h264_mp4toannexb' 'CHZZK H.264 is not converted to Annex-B for MPEG-TS.'
Assert-Match $recorder '-f mpegts' 'CHZZK output is not a genuine MPEG-TS container.'
Assert-Match $recorder '-mpegts_flags resend_headers' 'CHZZK MPEG-TS does not resend headers for robust seeking/playback.'
Assert-Match $recorder '-mpegts_copyts 0' 'CHZZK MPEG-TS must not preserve the source broadcast clock.'
Assert-Match $recorder '-avoid_negative_ts make_zero' 'CHZZK MPEG-TS timestamps are not rebased to zero.'
Assert-Match $recorder '-avioflags direct' 'CHZZK MPEG-TS live output direct I/O policy is missing.'
Assert-NotMatch $recorder 'frag_keyframe\+empty_moov\+default_base_moof' 'CHZZK LIVE must not regress to fragmented MP4 output.'
Assert-Match $recorder 'if let Some\(\(ffmpeg, player_args\)\) = timestamp_player[\s\S]*?--player[\s\S]*?else[\s\S]*?--output' 'SOOP direct output and CHZZK FFmpeg-player output are not separated.'
Assert-NotMatch $recorder '"--ffmpeg-start-at-zero"' 'The ineffective Streamlink mux-only --ffmpeg-start-at-zero path must not return.'
Assert-Match $recorder 'timestamp_rebase_player_args_write_zero_based_mpegts_stream_copy' 'CHZZK MPEG-TS remux regression test is missing.'
Assert-Match $recorder 'finds_ffmpeg_from_official_streamlink_windows_layout' 'Bundled Streamlink FFmpeg discovery regression test is missing.'
Assert-Match $live 'assert!\(start_at_zero\)' 'CHZZK timestamp policy regression coverage is missing.'

# LIVE files must keep their real platform container extension and collision scan.
Assert-Match $platform 'pub const fn live_output_extension' 'Platform LIVE output extension mapping is missing.'
Assert-Match $platform 'Self::Soop\s*=>\s*"ts"' 'SOOP LIVE output must remain .ts.'
Assert-Match $platform 'Self::Chzzk\s*=>\s*"ts"' 'CHZZK LIVE genuine MPEG-TS output must use .ts.'
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
Assert-Match $guiSources 'pub fn scoped_target\(platform: PlatformId, account: &str\)' 'Native LIVE controls must construct a platform-scoped channel target.'
Assert-Match $guiSources 'scoped_target\(channel\.platform, &channel\.account\)' 'Native LIVE rows must use the composite platform/account target.'

# Native provider configuration must stay routed through the shared core.
Assert-Match $primary 'provider\(channel\.platform\)\s*\.validate_account' 'Shared platform channel validation is missing.'
Assert-Match $core 'pub async fn update_provider_configuration' 'Shared provider configuration service is missing.'
Assert-Match $core 'NATIVE_PROVIDER_SECRET_KEYS' 'Shared provider secret allowlist is missing.'
Assert-Match $core '"CHZZK_NID_AUT"' 'CHZZK NID_AUT must remain a protected provider secret.'
Assert-Match $core '"CHZZK_NID_SES"' 'CHZZK NID_SES must remain a protected provider secret.'
Assert-Match $core 'pub async fn test_soop_auth' 'Shared SOOP authentication test service is missing.'
Assert-Match $guiSources 'core\.update_provider_configuration' 'Native Settings must save provider configuration through StreamArchiveCore.'
Assert-Match $guiSources 'Request::ProviderTestSoop' 'Native Settings must retain the SOOP authentication test action.'
Assert-Match $guiSources 'core\.test_soop_auth\(\)' 'Native SOOP authentication test must execute through StreamArchiveCore.'
Assert-Match $guiSources 'PlatformId::Soop\s*=>\s*PlatformId::Chzzk' 'Native channel editor must retain SOOP/CHZZK platform selection.'
Assert-Match $guiSources 'get_chzzk_nid_aut_draft' 'Native Settings must retain CHZZK NID_AUT input.'
Assert-Match $guiSources 'get_chzzk_nid_ses_draft' 'Native Settings must retain CHZZK NID_SES input.'
Assert-Match $guiUi 'platform' 'Native Queue/History presentation must retain platform identity.'
Assert-NotMatch $guiSources '\baxum::|https?://127\.0\.0\.1|https?://localhost' 'Native provider controls must not depend on the retired Web adapter.'

# Shared diagnostics/tool discovery must retain the official Streamlink FFmpeg candidate.
Assert-Match $soopVod 'C:\\Program Files\\Streamlink\\ffmpeg\\ffmpeg\.exe' 'SOOP VOD AUTO FFmpeg does not include Streamlink bundled FFmpeg.'

Write-Host 'Provider contracts passed.'
