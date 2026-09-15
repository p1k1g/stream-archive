$ErrorActionPreference = 'Stop'

function Read-Raw([string]$Path) {
    Get-Content -LiteralPath $Path -Raw -Encoding UTF8
}

function Write-Utf8NoBom([string]$Path, [string]$Text) {
    $full = (Resolve-Path -LiteralPath $Path).Path
    [System.IO.File]::WriteAllText($full, $Text, [System.Text.UTF8Encoding]::new($false))
}

function Replace-Once([string]$Text, [string]$Pattern, [string]$Replacement, [string]$Label) {
    $regex = [regex]::new($Pattern, [System.Text.RegularExpressions.RegexOptions]::Singleline)
    $count = $regex.Matches($Text).Count
    if ($count -ne 1) { throw "$Label expected one match, found $count" }
    $regex.Replace($Text, $Replacement, 1)
}

# ---------------- Frontend initialization ----------------
$appPath = 'rust-web/web/app.js'
$app = Read-Raw $appPath

# Remove code that belonged only to the deleted INI/TXT import/legacy-history DOM.
$app = [regex]::Replace($app, '(?m)^const importableLiveSettingKeys=.*\r?\n', '')
$app = [regex]::Replace($app, '(?m)^const hiddenImportKeys=.*\r?\n', '')
$app = [regex]::Replace($app, '(?m)^function normalizedLines\(text\).*\r?\n', '')
$app = Replace-Once $app 'function parseChannelBackup[\s\S]*?(?=async function loadSettings\(\))' '' 'legacy channel import block'
$app = Replace-Once $app 'function parseSettingsImport[\s\S]*?(?=function setSecretState\()' '' 'legacy settings import block'
$app = Replace-Once $app 'function liveHistoryRow[\s\S]*?(?=function renderLogs\()' '' 'legacy history block'

$authUi = @'
async function testSoopAuth(){const btn=$('testSoopAuth');if(btn)btn.disabled=true;try{const result=await api('/api/secrets/test/soop',{method:'POST'});toast(`SOOP 인증 정상 · ${result.login_id||'login OK'} · Worker ${result.worker_status||'OK'}`)}catch(e){alert(e.message)}finally{if(btn)btn.disabled=false}}
function installSoopAuthTest(){if($('testSoopAuth'))return;const save=$('saveSecrets');if(!save)return;const btn=document.createElement('button');btn.id='testSoopAuth';btn.type='button';btn.className='secondary';btn.textContent='인증 테스트';btn.onclick=()=>testSoopAuth();save.insertAdjacentElement('beforebegin',btn)}
'@
$app = Replace-Once $app '(?m)^(async function saveSecrets\(\).*)\r?\n(?=async function saveChzzkSecrets)' ('$1' + "`n" + $authUi.TrimEnd() + "`n") 'SOOP auth test UI anchor'
$app = $app.Replace('연령 제한 LIVE와 추후 CHZZK VOD에서 공통으로 사용하는 NAVER 로그인 쿠키입니다.', '연령 제한 LIVE와 CHZZK VOD에서 공통으로 사용하는 NAVER 로그인 쿠키입니다.')
$app = Replace-Once $app '(?m)^async function action\(a\).*\r?$' "async function action(a){try{await api('/api/watcher/'+a,{method:'POST'});toast('Watcher '+(a==='start'?'시작':'중지'));status();logs()}catch(e){alert(e.message)}}" 'watcher action'

$bindings = @'
$('start').onclick=()=>action('start');$('stop').onclick=()=>action('stop');$('refreshStatus').onclick=status;$('refreshDiagnostics').onclick=diagnostics;$('add').onclick=()=>$('channels').appendChild(row());$('saveChannels').onclick=()=>saveChannels().catch(e=>alert(e.message));$('saveSettings').onclick=()=>saveSettings().catch(e=>alert(e.message));$('saveSecrets').onclick=()=>saveSecrets().catch(e=>alert(e.message));$('refreshLogs').onclick=logs;$('tokenBtn').onclick=()=>{sessionStorage.removeItem('soopToken');token='';location.reload()};$('vodAnalyze').onclick=vodAnalyze;$('vodDownload').onclick=vodDownload;$('vodCancel').onclick=vodCancel;
'@
$app = Replace-Once $app '(?m)^\$\(''start''\)\.onclick=.*\r?$' $bindings.TrimEnd() 'base UI bindings'
$startup = "(async()=>{getToken();installSoopAuthTest();await Promise.all([status(),diagnostics(),loadChannels(),loadSettings(),loadSecrets(),vodStatus(),logs()]);startRealtime();setTimeout(installChzzkSettings,120)})().catch(e=>{startFallbackPolling();alert(e.message)});window.addEventListener('beforeunload',()=>{if(realtimeSource)realtimeSource.close()});"
$app = Replace-Once $app '(?m)^\(async\(\)=>\{getToken\(\);await Promise\.all\(\[.*\r?$' $startup 'base UI startup'
Write-Utf8NoBom $appPath $app

# ---------------- SOOP authentication test + FFmpeg diagnostics ----------------
$mainPath = 'rust-web/src/main.rs'
$main = Read-Raw $mainPath
$main = $main.Replace('use security::protect_secret;', 'use security::{protect_secret, unprotect_secret};')
$main = $main.Replace('use support::resolve_channel_name;', "use support::{`n    platform::{PlatformId, live::LiveSession},`n    resolve_channel_name,`n};")
$route = '.route("/api/secrets", get(api_secrets).put(api_update_secrets))'
if (-not $main.Contains('/api/secrets/test/soop')) {
    if (-not $main.Contains($route)) { throw 'SOOP secret route anchor missing' }
    $main = $main.Replace($route, $route + "`n        .route(\"/api/secrets/test/soop\", post(api_test_soop_auth))")
}

if ($main -notmatch 'async fn api_test_soop_auth') {
$testFn = @'
async fn api_test_soop_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let settings = state.store.live_settings_with_secrets().map_err(internal_error)?;
    let username = settings.get("SOOP_USERNAME").cloned().unwrap_or_default();
    let password = unprotect_secret(
        settings.get("SOOP_PASSWORD").map(String::as_str).unwrap_or(""),
        "SOOP_PASSWORD",
    )
    .map_err(internal_error)?;
    let worker_url = settings.get("CLOUDFLARE_WORKER_URL").cloned().unwrap_or_default();
    let worker_key = unprotect_secret(
        settings.get("CLOUDFLARE_API_KEY").map(String::as_str).unwrap_or(""),
        "CLOUDFLARE_API_KEY",
    )
    .map_err(internal_error)?;

    for (label, value) in [
        ("SOOP 아이디", username.as_str()),
        ("SOOP 비밀번호", password.as_str()),
        ("Worker URL", worker_url.as_str()),
        ("Worker API Key", worker_key.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, format!("{label}를 먼저 설정해 주세요.")));
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .no_proxy()
        .http1_only()
        .build()
        .map_err(internal_error)?;
    let mut session = LiveSession::new(PlatformId::Soop, client.clone()).map_err(internal_error)?;
    let login_id = session
        .login(&username, &password)
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("SOOP 로그인 테스트 실패: {err:#}")))?;

    let response = client
        .post(&worker_url)
        .header("X-API-Key", &worker_key)
        .json(&json!({}))
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("Worker 연결 테스트 실패: {err:#}")))?;
    let worker_status = response.status();
    if worker_status == reqwest::StatusCode::UNAUTHORIZED
        || worker_status == reqwest::StatusCode::FORBIDDEN
    {
        return Err((StatusCode::BAD_REQUEST, "Worker API Key 인증에 실패했습니다.".into()));
    }
    if worker_status != reqwest::StatusCode::BAD_REQUEST && !worker_status.is_success() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Worker endpoint 테스트 실패: HTTP {worker_status}"),
        ));
    }

    state.logs.push(format!("[AUTH] SOOP credential test passed login_id={login_id}")).await;
    Ok(Json(json!({"login_id": login_id, "worker_status": "OK"})))
}

'@
    $main = Replace-Once $main '(async fn api_update_secrets\([\s\S]*?\r?\n\}\r?\n\r?\n)(?=async fn api_channels\()' ('$1' + $testFn) 'SOOP auth endpoint insertion'
}

$oldDiag = '&[state.backend_dir.join("vod").join("ffmpeg.exe")],'
$newDiag = @'
&[
            state.backend_dir.join("vod").join("ffmpeg.exe"),
            PathBuf::from(r"C:\Program Files\Streamlink\ffmpeg\ffmpeg.exe"),
        ],
'@
if ($main.Contains($oldDiag)) { $main = $main.Replace($oldDiag, $newDiag.TrimEnd()) }
Write-Utf8NoBom $mainPath $main

# ---------------- CHZZK VOD retained process ownership ----------------
$vodPath = 'rust-web/src/platform/chzzk/vod.rs'
$vod = Read-Raw $vodPath
$vod = $vod.Replace('use crate::platform_runtime::{configure_utf8_cli, restrict_private_dir, terminate_owned};', 'use crate::platform_runtime::{configure_utf8_cli, restrict_private_dir, spawn_owned};')

# Streamlink and FFmpeg are created suspended and assigned to retained Job Objects
# before either process can launch descendants.
$vod = $vod.Replace('    let mut streamlink_child = streamlink_command`n        .spawn()`n        .with_context(|| format!("Streamlink 실행 실패: {}", streamlink.display()))?;', '    let (mut streamlink_child, mut streamlink_tree) = spawn_owned(&mut streamlink_command)`n        .await`n        .with_context(|| format!("Streamlink 실행 실패: {}", streamlink.display()))?;')
$vod = $vod.Replace('    let mut ffmpeg_child = match ffmpeg_command.spawn() {`n        Ok(child) => child,`n        Err(err) => {`n            terminate_owned(&mut streamlink_child).await;`n            return Err(err).with_context(|| format!("FFmpeg 실행 실패: {}", ffmpeg.display()));`n        }`n    };', '    let (mut ffmpeg_child, mut ffmpeg_tree) = match spawn_owned(&mut ffmpeg_command).await {`n        Ok(owned) => owned,`n        Err(err) => {`n            let _ = streamlink_tree.terminate(&mut streamlink_child).await;`n            return Err(err).with_context(|| format!("FFmpeg 실행 실패: {}", ffmpeg.display()));`n        }`n    };')
$vod = $vod.Replace('            terminate_owned(&mut streamlink_child).await;`n            bail!("Streamlink stdout unavailable");', '            let _ = streamlink_tree.terminate(&mut streamlink_child).await;`n            bail!("Streamlink stdout unavailable");')
$vod = $vod.Replace('            terminate_owned(&mut streamlink_child).await;`n            terminate_owned(&mut ffmpeg_child).await;`n            bail!("FFmpeg stdin unavailable");', '            let _ = streamlink_tree.terminate(&mut streamlink_child).await;`n            let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;`n            bail!("FFmpeg stdin unavailable");')
$vod = $vod.Replace('            terminate_owned(&mut streamlink_child).await;`n            terminate_owned(&mut ffmpeg_child).await;', '            let _ = streamlink_tree.terminate(&mut streamlink_child).await;`n            let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;')
$vod = $vod.Replace('                    terminate_owned(&mut ffmpeg_child).await;', '                    let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;')
$vod = $vod.Replace('                    terminate_owned(&mut streamlink_child).await;', '                    let _ = streamlink_tree.terminate(&mut streamlink_child).await;')

# Metadata/quality capture processes use the same retained ownership, so cancel
# during Analyze cannot wedge in the compatibility recapture loop either.
$vod = $vod.Replace('    let mut child = command`n        .args(args)`n        .stdin(Stdio::null())`n        .stdout(Stdio::piped())`n        .stderr(Stdio::piped())`n        .kill_on_drop(true)`n        .spawn()`n        .with_context(|| format!("{label} 프로세스 실행 실패: {}", program.display()))?;', '    command`n        .args(args)`n        .stdin(Stdio::null())`n        .stdout(Stdio::piped())`n        .stderr(Stdio::piped())`n        .kill_on_drop(true);`n    let (mut child, mut owned_tree) = spawn_owned(&mut command)`n        .await`n        .with_context(|| format!("{label} 프로세스 실행 실패: {}", program.display()))?;')
$vod = $vod.Replace('            terminate_owned(&mut child).await;`n            let _ = stdout_task.await;', '            let _ = owned_tree.terminate(&mut child).await;`n            let _ = stdout_task.await;')
Write-Utf8NoBom $vodPath $vod

# ---------------- Contract guards ----------------
$processPath = 'maintenance/guards/ProcessLifecycle.ps1'
$process = Read-Raw $processPath
$process = $process.Replace("Assert-Match `$chzzkVod 'terminate_owned\\(&mut streamlink_child\\)\\.await' 'CHZZK VOD setup/failure paths must terminate owned Streamlink.'", "Assert-Match `$chzzkVod 'spawn_owned\\(&mut streamlink_command\\)' 'CHZZK VOD Streamlink must enter retained ownership before it executes.'")
$process = $process.Replace("Assert-Match `$chzzkVod 'terminate_owned\\(&mut ffmpeg_child\\)\\.await' 'CHZZK VOD cancellation must terminate owned FFmpeg.'", "Assert-Match `$chzzkVod 'spawn_owned\\(&mut ffmpeg_command\\)' 'CHZZK VOD FFmpeg must enter retained ownership before it executes.'`nAssert-Match `$chzzkVod 'streamlink_tree\\.terminate\\(&mut streamlink_child\\)' 'CHZZK VOD cancellation must terminate retained Streamlink ownership.'`nAssert-Match `$chzzkVod 'ffmpeg_tree\\.terminate\\(&mut ffmpeg_child\\)' 'CHZZK VOD cancellation must terminate retained FFmpeg ownership.'`nAssert-Match `$chzzkVod 'spawn_owned\\(&mut command\\)' 'CHZZK VOD analysis/capture commands must use retained ownership.'")
Write-Utf8NoBom $processPath $process

$storagePath = 'maintenance/guards/StorageOwnership.ps1'
$storage = Read-Raw $storagePath
$storage = [regex]::Replace($storage, "Assert-Match \$chzzkVod 'let mut ffmpeg_child = match ffmpeg_command\\.spawn\\\(\\\)'[^\r\n]*", "Assert-Match `$chzzkVod 'spawn_owned\\(&mut ffmpeg_command\\)' 'Downstream FFmpeg must establish retained ownership before execution.'")
$storage = [regex]::Replace($storage, "Assert-Match \$chzzkVod 'terminate_owned\\\(&mut streamlink_child\\\)\\.await;'[^\r\n]*", "Assert-Match `$chzzkVod 'streamlink_tree\\.terminate\\(&mut streamlink_child\\)' 'Streamlink retained tree is not explicitly terminated on downstream setup failures.'")
Write-Utf8NoBom $storagePath $storage

$providersPath = 'maintenance/guards/Providers.ps1'
$providers = Read-Raw $providersPath
if ($providers -notmatch "\$main = Read-RepoFile 'rust-web/src/main.rs'") {
    $providers = $providers.Replace("`$soopVod = Read-RepoFile 'rust-web/src/platform/soop/vod.rs'", "`$soopVod = Read-RepoFile 'rust-web/src/platform/soop/vod.rs'`n`$main = Read-RepoFile 'rust-web/src/main.rs'")
}
if ($providers -notmatch 'Phase 19\.5 UI cleanup regression') {
$providers += @'

# Phase 19.5 UI cleanup regression: removed DOM must never abort app.js startup.
Assert-NotMatch $app 'restoreChannels|channelBackupFile|importSettings|settingsImportFile|refreshHistory' 'Removed legacy DOM is still referenced by the base UI script.'
Assert-Match $app '\$\(''add''\)\.onclick' 'Channel Add binding is missing from the base UI.'
Assert-Match $app '\$\(''saveSecrets''\)\.onclick' 'SOOP secret-save binding is missing from the base UI.'
Assert-Match $app '\$\(''vodAnalyze''\)\.onclick=vodAnalyze' 'VOD Analyze binding is missing from the base UI.'
Assert-Match $app 'setTimeout\(installChzzkSettings,120\)' 'CHZZK authentication tab installation is missing.'
Assert-Match $app 'api\(''/api/secrets/test/soop''' 'SOOP authentication test UI is missing.'
Assert-Match $main '"/api/secrets/test/soop"' 'SOOP authentication test endpoint is missing.'
Assert-Match $main 'C:\\Program Files\\Streamlink\\ffmpeg\\ffmpeg\.exe' 'Diagnostics do not include Streamlink bundled FFmpeg.'
'@
}
Write-Utf8NoBom $providersPath $providers

Write-Host 'Focused UI/process regression fixes applied.'
