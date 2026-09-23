. (Join-Path $PSScriptRoot 'Common.ps1')

$recorder = Read-RepoFile 'rust-runtime/src/recorder.rs'
$soopVod = Read-RepoFile 'rust-runtime/src/platform/soop/vod.rs'
$chzzkVod = Read-RepoFile 'rust-runtime/src/platform/chzzk/vod.rs'
$media = Read-RepoFile 'rust-runtime/src/media_process.rs'
$fixture = Read-RepoFile 'rust-runtime/tests/fixtures/media_tool_fixture.rs'
$support = Read-RepoFile 'rust-runtime/src/test_support.rs'

Assert-Match $recorder 'mod provider_e2e' 'LIVE provider E2E module is missing.'
Assert-RustTest $recorder 'soop_live_provider_e2e_preserves_unicode_argument_contract' 'SOOP LIVE provider invocation E2E test is missing.'
Assert-RustTest $recorder 'chzzk_live_provider_e2e_preserves_cookie_player_and_quality_contract' 'CHZZK LIVE provider invocation E2E test is missing.'
Assert-RustTest $recorder 'live_provider_e2e_maps_nonzero_and_spawn_failure' 'LIVE provider failure-mapping E2E test is missing.'
Assert-RustTest $recorder 'live_provider_cancel_cleans_owned_descendant_not_unrelated_process' 'LIVE provider owned-tree cancellation E2E test is missing.'

Assert-Match $soopVod 'mod provider_e2e' 'SOOP VOD provider E2E module is missing.'
Assert-RustTest $soopVod 'soop_vod_provider_e2e_metadata_and_download_contract' 'SOOP VOD invocation E2E test is missing.'
Assert-RustTest $soopVod 'soop_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout' 'SOOP VOD result-mapping E2E test is missing.'
Assert-RustTest $soopVod 'soop_vod_provider_e2e_cancel_cleans_owned_tree_not_unrelated_process' 'SOOP VOD owned-tree cancellation E2E test is missing.'

Assert-Match $chzzkVod 'mod provider_e2e' 'CHZZK VOD provider E2E module is missing.'
Assert-RustTest $chzzkVod 'chzzk_vod_provider_e2e_streamlink_ffmpeg_contract_and_unicode_output' 'CHZZK VOD Streamlink/FFmpeg invocation E2E test is missing.'
Assert-RustTest $chzzkVod 'chzzk_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout' 'CHZZK VOD result-mapping E2E test is missing.'
Assert-RustTest $chzzkVod 'chzzk_vod_provider_e2e_cancel_cleans_streamlink_descendant' 'CHZZK VOD cancellation E2E test is missing.'

Assert-Match $media 'run_media_process_with_atomic_cancel' 'Provider capture bridge to the shared media runner is missing.'
Assert-Match $soopVod 'run_media_process_with_atomic_cancel' 'SOOP VOD capture path is not connected to the shared media runner.'
Assert-Match $chzzkVod 'run_media_process_with_atomic_cancel' 'CHZZK VOD capture path is not connected to the shared media runner.'
Assert-Match $recorder 'spawn_owned\(&mut command\)' 'LIVE streaming must retain long-lived owned-process semantics.'
Assert-Match $chzzkVod 'spawn_owned\(&mut streamlink_command\)' 'CHZZK VOD streaming pipe must retain Streamlink ownership.'
Assert-Match $chzzkVod 'spawn_owned\(&mut ffmpeg_command\)' 'CHZZK VOD streaming pipe must retain FFmpeg ownership.'

Assert-Match $fixture 'enum ProviderTool' 'Provider-capable Rust media fixture is missing.'
Assert-Match $fixture 'record_provider_invocation' 'Provider fixture invocation recording is missing.'
Assert-Match $fixture '"run-spawn-child"' 'Provider fixture owned-descendant scenario is missing.'
Assert-Match $fixture 'STREAM_ARCHIVE_FIXTURE_GENERIC' 'Provider fixture unrelated-process mode is missing.'
Assert-Match $support 'Stream Archive provider 한글' 'Provider E2E Unicode temporary-path coverage is missing.'
Assert-Match $support 'invocations\.log' 'Provider E2E invocation-log contract is missing.'
Assert-NotMatch $fixture 'reqwest|https://sooplive\.com|https://chzzk\.naver\.com' 'Provider executable fixture must not perform provider network I/O.'

Write-Host 'Provider E2E contracts passed.'
