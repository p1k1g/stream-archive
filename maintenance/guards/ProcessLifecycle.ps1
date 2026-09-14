. (Join-Path $PSScriptRoot 'Common.ps1')

$recorder = Read-RepoFile 'rust-web/src/recorder.rs'
$vodFacade = Read-RepoFile 'rust-web/src/vod.rs'
$soopVod = Read-RepoFile 'rust-web/src/platform/soop/vod.rs'
$chzzkVod = Read-RepoFile 'rust-web/src/platform/chzzk/vod.rs'
$runtime = Read-RepoFile 'rust-web/src/platform_runtime.rs'
$allRuntime = $recorder + $soopVod + $chzzkVod + $runtime

Assert-Match $recorder '\.kill_on_drop\(true\)' 'LIVE Streamlink child must retain kill-on-drop fallback.'
Assert-NotMatch $vodFacade 'taskkill\.exe|yt-dlp|ffmpeg|sooplive\.com' 'Root VOD facade must remain provider/process neutral.'
Assert-Match $chzzkVod 'terminate_owned\(&mut streamlink_child\)\.await' 'CHZZK VOD setup/failure paths must terminate owned Streamlink.'
Assert-Match $chzzkVod 'terminate_owned\(&mut ffmpeg_child\)\.await' 'CHZZK VOD cancellation must terminate owned FFmpeg.'
Assert-Match $chzzkVod 'abort_reader_tasks\(reader_tasks\)\.await' 'CHZZK VOD failure/cancel must reap line readers.'
Assert-Match $chzzkVod 'join_reader_tasks\(reader_tasks\)\.await' 'CHZZK VOD success must reap line readers.'
Assert-Match $chzzkVod 'mpsc::channel::<String>\(256\)' 'External-tool line transport must remain bounded.'
Assert-Match $runtime 'async fn terminate_owned\(child: &mut Child\)' 'Common owned-process termination primitive is missing.'
Assert-RustTest $runtime 'owned_child_is_terminated_and_reaped' 'Owned-child termination/reaping behavior test is missing.'
Assert-Match $runtime 'taskkill\.exe' 'Windows owned-tree termination implementation is missing.'
Assert-Match $runtime '\.arg\("/PID"\)' 'Windows termination must target the owned PID.'
Assert-Match $runtime '\.arg\("/T"\)' 'Windows termination must include descendants.'
Assert-Match $runtime '\.arg\("/F"\)' 'Windows termination must force cleanup when required.'
Assert-NotMatch $allRuntime '(?i)taskkill(?:\.exe)?[^\r\n]*(?:/IM|\.arg\("/IM"\))' 'Process-name-wide taskkill /IM is forbidden.'
Assert-NotMatch ($recorder + $soopVod + $chzzkVod) 'taskkill\.exe' 'Provider/runtime callers must use the common process boundary.'

$stopCalls = [regex]::Matches($soopVod, 'stop_child\(&mut child\)\.await;').Count
if ($stopCalls -lt 2) { throw "SOOP VOD cancellation must cover progress and capture children; found $stopCalls call(s)." }
Assert-Match $soopVod 'if !exit\.success\(\)' 'SOOP capture must reject non-zero exits.'
Assert-Match $soopVod 'if exit\.success\(\)' 'SOOP progress must distinguish successful exits.'
Write-Host 'Process lifecycle contracts passed.'
