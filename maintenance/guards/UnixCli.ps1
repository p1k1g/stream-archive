. (Join-Path $PSScriptRoot 'Common.ps1')

$cli = Read-RepoFile 'rust-runtime/src/bin/stream-archive-cli.rs'
$unixCli = Read-RepoFile 'rust-runtime/src/unix_cli.rs'
$headless = Read-RepoFile 'rust-runtime/src/headless.rs'
$server = Read-RepoFile 'rust-runtime/src/main.rs'
$unixTest = Read-RepoFile 'rust-runtime/tests/unix_cli.rs'

foreach ($command in @(
    '"status"', '"settings"', '"providers"', '"channels"', '"watcher"',
    '"vod"', '"queue"', '"history"', '"backup"', '"storage"', '"logs"'
)) {
    Assert-Match $cli ([regex]::Escape($command)) "Unix CLI routing is missing command $command."
}

Assert-Match $cli 'run_management' 'Unix management commands must route through the shared management boundary.'
Assert-Match $unixCli 'StreamArchiveCore' 'Unix CLI management must use StreamArchiveCore.'
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

Assert-Match $headless 'SignalKind::terminate' 'Unix headless runtime must handle SIGTERM.'
Assert-Match $headless 'core\.shutdown\(\)\.await' 'Headless signal path must call shared core shutdown.'
Assert-Match $server 'run_headless' 'Compatibility server must use the shared headless lifecycle.'
Assert-Match $cli 'run_serve' 'CLI serve must use the shared headless lifecycle.'

Assert-Match $unixTest 'CARGO_BIN_EXE_stream-archive-cli' 'Unix CLI integration must execute the real CLI binary.'
Assert-Match $unixTest 'CARGO_BIN_EXE_stream-archive-server' 'Unix compatibility-server integration is missing.'
Assert-Match $unixTest 'const SIGTERM: i32 = 15' 'Unix CLI lifecycle smoke must send real SIGTERM.'
Assert-Match $unixTest 'unrelated headless runtime must survive' 'Unix signal regression must protect unrelated processes.'
Assert-Match $unixTest 'Stream Archive CLI 테스트' 'Unix CLI integration must cover Unicode/whitespace paths.'

Write-Host 'Unix CLI completion contracts passed.'
