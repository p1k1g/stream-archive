. (Join-Path $PSScriptRoot 'Common.ps1')

$cli = Read-RepoFile 'rust-runtime/src/bin/stream-archive-cli.rs'
$unixCli = Read-RepoFile 'rust-runtime/src/unix_cli.rs'
$headless = Read-RepoFile 'rust-runtime/src/headless.rs'
$server = Read-RepoFile 'rust-runtime/src/main.rs'
$store = Read-RepoFile 'rust-runtime/src/store.rs'
$queue = Read-RepoFile 'rust-runtime/src/queue_service.rs'
$runtimeOwner = Read-RepoFile 'rust-runtime/src/runtime_owner.rs'
$unixControl = Read-RepoFile 'rust-runtime/src/unix_control.rs'
$unixTest = Read-RepoFile 'rust-runtime/tests/unix_cli.rs'

foreach ($command in @(
    '"status"', '"settings"', '"providers"', '"channels"', '"watcher"',
    '"vod"', '"queue"', '"history"', '"backup"', '"storage"', '"logs"'
)) {
    Assert-Match $cli ([regex]::Escape($command)) "Unix CLI routing is missing command $command."
}

Assert-Match $cli 'run_management' 'Unix management commands must route through the shared management boundary.'
Assert-Match $unixCli 'StreamArchiveCore' 'Unix CLI management must use StreamArchiveCore.'
Assert-Match $unixCli 'open_observer' 'One-shot Unix management must use the non-recovering observer core.'
Assert-Match $store 'pub fn open_observer' 'Observer store open boundary is missing.'
Assert-Match $queue 'pub fn new_observer' 'Observer Queue construction must not perform startup recovery.'
Assert-Match $runtimeOwner 'try_lock_exclusive' 'Cross-process runtime owner lock is missing.'
Assert-Match $runtimeOwner 'RuntimeOwnerGuard' 'Canonical runtime owner guard is missing.'
Assert-Match $unixCli 'update_environment_settings' 'Unix settings writes must use StreamArchiveCore validation.'
Assert-Match $unixCli 'update_provider_configuration' 'Unix provider writes must use the shared protected-secret boundary.'
Assert-Match $unixCli 'update_channels' 'Unix channel writes must use the shared core.'
Assert-Match $unixCli 'analyze_vod' 'Unix VOD analyze must use the shared core.'
Assert-Match $unixCli 'download_vod' 'Unix VOD download must use the shared core.'
Assert-Match $unixCli 'enqueue_vod' 'Unix Queue writes must use the shared core.'
Assert-Match $unixCli 'restore_backup' 'Unix backup restore must use shared lifecycle safety checks.'
Assert-Match $unixCli 'runtime_logs' 'Unix logs command must use the shared core.'
Assert-Match $unixCli 'providers secret <KEY> --stdin' 'Provider secret CLI must document stdin-only transport.'
Assert-Match $unixCli 'read_secret_stdin' 'Provider secret CLI stdin boundary is missing.'
Assert-NotMatch $unixCli '--password\s+\S+' 'Unix CLI must not accept provider passwords as ordinary argv values.'
Assert-NotMatch $unixCli '--nid-aut\s+\S+' 'Unix CLI must not accept CHZZK secrets as ordinary argv values.'
Assert-NotMatch $unixCli 'reqwest|TcpListener|axum' 'Unix management surface must not add a provider/Web control plane.'
Assert-NotMatch $unixCli 'std::process::Command|tokio::process::Command' 'Daily-use Unix management commands must not directly spawn provider tools.'

Assert-Match $unixControl 'UnixListener' 'Unix runtime control must use a local Unix-domain socket.'
Assert-Match $unixControl '0o600' 'Unix runtime control socket must be owner-only.'
Assert-Match $unixControl '"channel.password"' 'Protected-stream passwords must route to the running watcher owner.'
Assert-Match $unixControl 'core\.channel_password' 'Runtime control must deliver protected-stream passwords to the owner core.'
Assert-Match $unixControl 'core\.cancel_vod' 'Runtime control must deliver VOD cancellation to the owner core.'
Assert-NotMatch $unixControl 'TcpListener|axum|Router::new' 'Runtime control must not reintroduce an HTTP/Web control plane.'

Assert-Match $headless 'SignalKind::terminate' 'Unix headless runtime must handle SIGTERM.'
Assert-Match $headless 'core\.shutdown\(\)\.await' 'Headless signal path must call shared core shutdown.'
Assert-Match $server 'run_headless' 'Compatibility server must use the shared headless lifecycle.'
Assert-Match $cli 'run_serve' 'CLI serve must use the shared headless lifecycle.'

Assert-Match $unixTest 'CARGO_BIN_EXE_stream-archive-cli' 'Unix CLI integration must execute the real CLI binary.'
Assert-Match $unixTest 'CARGO_BIN_EXE_stream-archive-server' 'Unix compatibility-server integration is missing.'
Assert-Match $unixTest 'const SIGTERM: i32 = 15' 'Unix CLI lifecycle smoke must send real SIGTERM.'
Assert-Match $unixTest 'unrelated headless runtime must survive' 'Unix signal regression must protect unrelated processes.'
Assert-Match $unixTest 'one_shot_cli_observes_running_owner_without_recovering_active_rows' 'One-shot observer regression test is missing.'
Assert-Match $unixTest 'runtime_owner_active' 'Unix CLI integration must verify the running-owner status contract.'
Assert-Match $unixTest 'cannot restore while another Stream Archive runtime owns' 'Restore must be blocked while another runtime owns the canonical database.'
Assert-Match $unixCli 'ensure_vod_success\(&final_status\)' 'VOD analyze/download terminal failures must propagate a non-zero CLI result.'
Assert-Match $unixTest 'tempfile::Builder::new\(\)[\s\S]*?\.prefix\("[^"]*\s[^"]*"\)' 'Unix CLI integration must cover whitespace temporary paths.'
Assert-Match $unixTest 'tempfile::Builder::new\(\)[\s\S]*?\.prefix\("[^"]*[^\x00-\x7F][^"]*"\)' 'Unix CLI integration must cover non-ASCII Unicode temporary paths.'

Write-Host 'Unix CLI completion contracts passed.'
