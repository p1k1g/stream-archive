. (Join-Path $PSScriptRoot 'Common.ps1')

$media = Read-RepoFile 'rust-runtime/src/media_process.rs'
$diagnostics = Read-RepoFile 'rust-runtime/src/diagnostics.rs'
$cli = Read-RepoFile 'rust-runtime/src/bin/stream-archive-cli.rs'

Assert-Match $media 'run_media_process' 'Shared media process runner is missing.'
Assert-Match $media 'spawn_owned\(&mut command\)' 'Shared media runner must reuse retained owned-process spawn.'
Assert-Match $media 'capture_bounded' 'Media output capture must remain bounded.'
Assert-Match $media 'DEFAULT_CAPTURE_LIMIT' 'Media output capture limit is missing.'
Assert-Match $media 'MediaCancellation' 'Media cancellation contract is missing.'
Assert-Match $media 'probe_tool_version' 'Media tool version probe is missing.'
Assert-Match $diagnostics 'collect_active_local_preflight' 'Opt-in active local preflight is missing.'
Assert-Match $diagnostics 'probe_tool_version' 'Active preflight must delegate tool execution to the shared probe.'
Assert-Match $cli '--active-tools' 'CLI must keep active media-tool probing explicit.'
Assert-RustTest $media 'spawn_preserves_argument_boundaries_and_unicode_output' 'Argument/Unicode media-process test is missing.'
Assert-RustTest $media 'large_concurrent_output_is_bounded_and_keeps_tail' 'Bounded output integration test is missing.'
Assert-RustTest $media 'timeout_terminates_owned_process_tree' 'Timeout ownership test is missing.'
Assert-RustTest $media 'timeout_does_not_wait_forever_for_detached_pipe_holder' 'Detached pipe-holder bounded-drain test is missing.'
Assert-RustTest $media 'cancellation_terminates_owned_tree_but_not_unrelated_process' 'Cancellation/unrelated-process test is missing.'
Assert-RustTest $media 'completed_process_wins_over_late_cancel_and_timeout' 'Late-cancellation terminal-result regression test is missing.'
Assert-RustTest $media 'version_probe_supports_all_media_tools' 'Version probe integration test is missing.'

Write-Host 'Media process contracts passed.'
