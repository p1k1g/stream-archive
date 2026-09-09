from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# Cargo dependency for ReceiverStream.
p='rust-web/Cargo.toml'
s=read(p)
s=rep(s,'tokio = { version = "1", features = ["full"] }\n','tokio = { version = "1", features = ["full"] }\ntokio-stream = "0.1"\n','tokio-stream dep')
write(p,s)

# LogBuffer doubles as a lightweight realtime wake-up bus.
p='rust-web/src/backend.rs'
s=read(p)
s=rep(s,'use tokio::sync::RwLock;','use tokio::sync::{RwLock, broadcast};','backend broadcast import')
s=rep(s,'''pub struct LogBuffer {
    inner: Arc<RwLock<VecDeque<String>>>,
}''','''pub struct LogBuffer {
    inner: Arc<RwLock<VecDeque<String>>>,
    events: broadcast::Sender<()>,
}''','LogBuffer fields')
s=rep(s,'''    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_CAPACITY))),
        }
    }''','''    pub fn new() -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_CAPACITY))),
            events,
        }
    }''','LogBuffer new')
s=rep(s,'''    pub async fn push(&self, line: impl Into<String>) {
        let mut logs = self.inner.write().await;
        logs.push_back(line.into());
        while logs.len() > LOG_CAPACITY {
            logs.pop_front();
        }
    }

    pub async fn tail(&self, max_lines: usize) -> Vec<String> {''','''    pub async fn push(&self, line: impl Into<String>) {
        let mut logs = self.inner.write().await;
        logs.push_back(line.into());
        while logs.len() > LOG_CAPACITY {
            logs.pop_front();
        }
        drop(logs);
        let _ = self.events.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }

    pub async fn tail(&self, max_lines: usize) -> Vec<String> {''','LogBuffer push subscribe')
write(p,s)

# New SSE endpoint.
write('rust-web/src/realtime.rs', r'''use crate::{ApiResult, AppState, backend::LogBuffer, internal_error};
use axum::{
    extract::State,
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};
use tokio_stream::wrappers::ReceiverStream;

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const LOG_LINES: usize = 160;

fn authorize_stream(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(());
    }
    // Native EventSource cannot attach the recovery Bearer header. Session-cookie
    // clients use SSE; recovery-token mode intentionally remains on REST polling.
    state.auth.authorize_session(headers)
}

async fn snapshot_event(state: &AppState) -> Event {
    let (watcher, watcher_error) = match state.watcher.status().await {
        Ok(status) => (status, None),
        Err(err) => (Default::default(), Some(err.to_string())),
    };
    let vod = state.vod.status().await;
    let logs = state.logs.tail(LOG_LINES).await;
    let payload = json!({
        "phase": "phase11-realtime-sse",
        "status": {
            "watcher": watcher,
            "backend_dir": state.backend_dir.display().to_string(),
            "bind": state.bind,
            "phase": "phase11-realtime-sse"
        },
        "vod": vod,
        "logs": {"lines": logs},
        "watcher_error": watcher_error
    });
    Event::default().event("snapshot").data(payload.to_string())
}

pub(crate) async fn api_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Sse<ReceiverStream<Result<Event, Infallible>>>> {
    authorize_stream(&headers, &state)?;

    let mut log_events = state.logs.subscribe();
    let (sender, receiver) = mpsc::channel(8);
    tokio::spawn(async move {
        let mut tick = interval(SNAPSHOT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                event = log_events.recv() => match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            if sender.send(Ok(snapshot_event(&state).await)).await.is_err() {
                break;
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(receiver)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("phase11-keep-alive"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_buffer_wakes_realtime_subscribers() {
        let logs = LogBuffer::new();
        let mut receiver = logs.subscribe();
        logs.push("realtime-test").await;
        assert!(receiver.recv().await.is_ok());
        assert_eq!(logs.tail(1).await, vec!["realtime-test"]);
    }
}
''')

# Wire route and Phase marker.
p='rust-web/src/main.rs'
s=read(p)
s=rep(s,'mod recorder;\n','mod recorder;\nmod realtime;\n','realtime module')
s=rep(s,'.route("/api/status", get(api_status))\n','.route("/api/status", get(api_status))\n        .route("/api/events", get(realtime::api_events))\n','events route')
s=s.replace('[SERVER] Phase 10 session auth ready;','[SERVER] Phase 11 realtime SSE ready;')
s=s.replace('// Phase 10 keeps the SQLite-direct runtime/native picker and adds browser ID/password sessions.','// Phase 11 keeps REST compatibility and adds one authenticated SSE stream for realtime UI updates.')
s=s.replace('SOOP Rust Web - Phase 10','SOOP Rust Web - Phase 11')
s=s.replace('"phase10-session-auth"','"phase11-realtime-sse"')
s=rep(s,'    println!("Session : HttpOnly/SameSite cookie + CSRF; Secure cookie through HTTPS proxy");\n','    println!("Session : HttpOnly/SameSite cookie + CSRF; Secure cookie through HTTPS proxy");\n    println!("Realtime: SSE snapshot stream + automatic REST polling fallback");\n','console realtime line')
write(p,s)

# Browser: split rendering from REST fetch, add SSE lifecycle, remove steady polling.
p='rust-web/web/app.js'
s=read(p)
anchor="const labels={UNKNOWN:'확인중',OFFLINE:'오프라인',RECORDING:'녹화중',PAUSED:'현재방송 중지',DISABLED:'비활성',ERROR:'오류',LOW_DISK:'디스크 부족',STALLED:'녹화 정지',AUTH:'인증 필요',PASSWORD_REQUIRED:'비밀번호 필요',LIVE:'방송중',COMPLETED:'완료',STOPPED:'중지',FAILED:'실패',INTERRUPTED:'비정상 종료',CANCELLED:'취소'};"
s=rep(s,anchor,anchor+"\nconst SESSION_SENTINEL='__SOOP_SESSION__';let realtimeSource=null,realtimeConnected=false;let fallbackTimers=[];",'realtime vars')
old="async function status(){try{const d=await api('/api/status');const w=d.watcher;$('watcher').textContent=w.running?'RUNNING':'STOPPED';$('watcher').className=w.running?'ok':'bad';$('engine').textContent=w.engine||'-';$('channelCount').textContent=w.channel_count??0;$('recCount').textContent=w.recording_count??0;$('offlineCount').textContent=w.offline_count??0;$('errorCount').textContent=w.error_count??0;$('backend').textContent=d.backend_dir;$('start').disabled=w.running;$('stop').disabled=!w.running;const body=$('runtime');body.replaceChildren();(w.channels||[]).forEach(c=>body.appendChild(runtimeRow(c)))}catch(e){$('watcher').textContent='ERROR'}}"
new="function renderStatus(d){const w=d?.watcher||{};$('watcher').textContent=w.running?'RUNNING':'STOPPED';$('watcher').className=w.running?'ok':'bad';$('engine').textContent=w.engine||'-';$('channelCount').textContent=w.channel_count??0;$('recCount').textContent=w.recording_count??0;$('offlineCount').textContent=w.offline_count??0;$('errorCount').textContent=w.error_count??0;$('backend').textContent=d?.backend_dir||'-';$('start').disabled=!!w.running;$('stop').disabled=!w.running;const body=$('runtime');body.replaceChildren();(w.channels||[]).forEach(c=>body.appendChild(runtimeRow(c)))}\nasync function status(){try{renderStatus(await api('/api/status'))}catch(e){$('watcher').textContent='ERROR'}}"
s=rep(s,old,new,'status renderer')
old="async function vodStatus(){try{const s=await api('/api/vod/status');$('vodState').textContent=s.state||'-';$('vodPart').textContent=`${s.current_part||0}/${s.part_count||0}`;$('vodPercent').textContent=`${Number(s.percent||0).toFixed(1)}%`;$('vodMessage').textContent=s.message||'';$('vodOutputFile').textContent=s.output_file?`완료 파일: ${s.output_file}`:'';$('vodAnalyze').disabled=!!s.running;$('vodDownload').disabled=!!s.running;$('vodCancel').disabled=!s.running;if(s.analysis){$('vodMeta').textContent=`${s.analysis.title} · ${s.analysis.streamer} · ${s.analysis.part_count} PART`;const q=$('vodQuality');const current=q.value;q.replaceChildren();(s.analysis.qualities||[]).forEach(o=>{const op=document.createElement('option');op.value=o.value;op.textContent=o.label;q.appendChild(op)});if([...q.options].some(o=>o.value===current))q.value=current;const parts=(s.analysis.parts||[]).map(p=>`P${p.part}:${p.duration_seconds||0}s`).join(' / ');if(parts)$('vodMeta').textContent+=` · ${parts}`}}catch(e){}}"
new="function renderVodStatus(s){s=s||{};$('vodState').textContent=s.state||'-';$('vodPart').textContent=`${s.current_part||0}/${s.part_count||0}`;$('vodPercent').textContent=`${Number(s.percent||0).toFixed(1)}%`;$('vodMessage').textContent=s.message||'';$('vodOutputFile').textContent=s.output_file?`완료 파일: ${s.output_file}`:'';$('vodAnalyze').disabled=!!s.running;$('vodDownload').disabled=!!s.running;$('vodCancel').disabled=!s.running;if(s.analysis){$('vodMeta').textContent=`${s.analysis.title} · ${s.analysis.streamer} · ${s.analysis.part_count} PART`;const q=$('vodQuality');const current=q.value;q.replaceChildren();(s.analysis.qualities||[]).forEach(o=>{const op=document.createElement('option');op.value=o.value;op.textContent=o.label;q.appendChild(op)});if([...q.options].some(o=>o.value===current))q.value=current;const parts=(s.analysis.parts||[]).map(p=>`P${p.part}:${p.duration_seconds||0}s`).join(' / ');if(parts)$('vodMeta').textContent+=` · ${parts}`}}\nasync function vodStatus(){try{renderVodStatus(await api('/api/vod/status'))}catch(e){}}"
s=rep(s,old,new,'vod renderer')
old="async function logs(){try{const d=await api('/api/logs');const p=$('logs');p.textContent=d.lines.join('\\n')||'(로그 없음)';p.scrollTop=p.scrollHeight}catch(e){}}"
new="function renderLogs(d){const p=$('logs');p.textContent=(d?.lines||[]).join('\\n')||'(로그 없음)';p.scrollTop=p.scrollHeight}\nasync function logs(){try{renderLogs(await api('/api/logs'))}catch(e){}}"
s=rep(s,old,new,'logs renderer')
marker='async function action(a){try{await api(\'/api/watcher/\'+a,{method:\'POST\'});toast(\'Watcher \'+(a===\'start\'?\'시작\':\'중지\'));status();logs();legacyHistory()}catch(e){alert(e.message)}}'
insert=r'''function setRealtimeState(text,klass='muted'){const el=$('realtimeState');if(!el)return;el.textContent=' · '+text;el.className=klass}
function stopFallbackPolling(){fallbackTimers.forEach(clearInterval);fallbackTimers=[]}
function startFallbackPolling(){if(fallbackTimers.length)return;realtimeConnected=false;fallbackTimers=[setInterval(status,2500),setInterval(vodStatus,2000),setInterval(logs,2500)]}
function applyRealtimeSnapshot(data){if(data?.status)renderStatus(data.status);if(data?.vod)renderVodStatus(data.vod);if(data?.logs)renderLogs(data.logs)}
function startRealtime(){
  if(token&&token!==SESSION_SENTINEL){setRealtimeState('복구 토큰 · 폴링','warn');startFallbackPolling();return}
  if(realtimeSource)realtimeSource.close();
  setRealtimeState('실시간 연결 중','muted');
  const source=new EventSource('/api/events');realtimeSource=source;
  source.addEventListener('snapshot',event=>{try{applyRealtimeSnapshot(JSON.parse(event.data))}catch(e){console.warn('SSE snapshot parse failed',e)}});
  source.onopen=()=>{realtimeConnected=true;stopFallbackPolling();setRealtimeState('실시간 연결','ok')};
  source.onerror=()=>{realtimeConnected=false;setRealtimeState('SSE 재연결 중 · 폴링 대체','warn');startFallbackPolling()};
}
'''+marker
s=rep(s,marker,insert,'realtime browser functions')
old="(async()=>{getToken();await Promise.all([status(),diagnostics(),loadChannels(),loadSettings(),loadSecrets(),vodStatus(),legacyHistory(),logs()])})().catch(e=>alert(e.message));setInterval(status,2000);setInterval(vodStatus,1500);setInterval(legacyHistory,5000);setInterval(logs,2000);"
new="(async()=>{getToken();await Promise.all([status(),diagnostics(),loadChannels(),loadSettings(),loadSecrets(),vodStatus(),legacyHistory(),logs()]);startRealtime()})().catch(e=>{startFallbackPolling();alert(e.message)});window.addEventListener('beforeunload',()=>{if(realtimeSource)realtimeSource.close()});"
s=rep(s,old,new,'polling replacement')
write(p,s)

# Header connection indicator + cache bust.
p='rust-web/web/index.html'
s=read(p)
s=rep(s,'<small>Rust Web Phase 10.2 · 쉬운 설정 UI</small>','<small>Rust Web Phase 11 · 실시간 UI<span id="realtimeState" class="muted" aria-live="polite"> · 연결 준비</span></small>','phase header')
s=s.replace('v=p10-2b','v=p11-sse1').replace('v=p10-2','v=p11-sse1')
write(p,s)

# Sanity: no steady status/vod/log polling remains; fallback only.
app=read('rust-web/web/app.js')
main=read('rust-web/src/main.rs')
assert 'new EventSource(\'/api/events\')' in app
assert 'phase11-realtime-sse' in main
assert '.route("/api/events", get(realtime::api_events))' in main
assert 'setInterval(status,2000)' not in app
assert 'setInterval(vodStatus,1500)' not in app
assert Path('rust-web/src/realtime.rs').is_file()
print('Phase 11 patch sanity: PASS')
