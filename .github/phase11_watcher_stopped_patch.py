from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

p='rust-web/src/native_watcher.rs'
s=read(p)
old='''    for state in states.values_mut() {
        stop_state_recording(state, "WATCHER EXIT", &recorder, &logs).await;
    }
    clear_all_stream_passwords();
    update_snapshot(&states, &snapshot).await;
    Ok(())
}
'''
new='''    for state in states.values_mut() {
        stop_state_recording(state, "WATCHER EXIT", &recorder, &logs).await;
    }
    mark_watcher_stopped(&mut states);
    clear_all_stream_passwords();
    update_snapshot(&states, &snapshot).await;
    Ok(())
}

fn mark_watcher_stopped(states: &mut HashMap<String, ChannelState>) {
    for state in states.values_mut() {
        if state.channel.enabled {
            state.status = "WATCHER_STOPPED".into();
            state.last_bno = None;
            state.suppressed_bno = None;
            state.detail = None;
        } else {
            state.status = "DISABLED".into();
        }
    }
}
'''
s=rep(s,old,new,'watcher exit state reset')
needle='''    #[test]
    fn channel_signature_is_case_insensitive_for_accounts() {
        let a = vec![Channel {
            enabled: true,
            name: "A".into(),
            account: "UserA".into(),
            outdir: "".into(),
        }];
        let b = vec![Channel {
            enabled: true,
            name: "A".into(),
            account: "usera".into(),
            outdir: "".into(),
        }];
        assert_eq!(channel_signature(&a), channel_signature(&b));
    }
'''
replacement=needle+'''
    #[test]
    fn watcher_exit_replaces_paused_state_and_clears_runtime_markers() {
        let enabled = Channel {
            enabled: true,
            name: "Live".into(),
            account: "live".into(),
            outdir: "".into(),
        };
        let disabled = Channel {
            enabled: false,
            name: "Disabled".into(),
            account: "disabled".into(),
            outdir: "".into(),
        };
        let mut states = HashMap::new();
        let mut live = ChannelState::new(enabled);
        live.status = "PAUSED".into();
        live.last_bno = Some("123".into());
        live.suppressed_bno = Some("123".into());
        live.detail = Some("old detail".into());
        states.insert("live".into(), live);
        states.insert("disabled".into(), ChannelState::new(disabled));

        mark_watcher_stopped(&mut states);

        let live = states.get("live").unwrap();
        assert_eq!(live.status, "WATCHER_STOPPED");
        assert!(live.last_bno.is_none());
        assert!(live.suppressed_bno.is_none());
        assert!(live.detail.is_none());
        assert_eq!(states.get("disabled").unwrap().status, "DISABLED");
    }
'''
s=rep(s,needle,replacement,'watcher stopped unit test')
write(p,s)

p='rust-web/web/app.js'
s=read(p)
s=rep(s,
"const labels={UNKNOWN:'확인중',OFFLINE:'오프라인',RECORDING:'녹화중',PAUSED:'현재방송 중지',DISABLED:'비활성',ERROR:'오류',LOW_DISK:'디스크 부족',STALLED:'녹화 정지',AUTH:'인증 필요',PASSWORD_REQUIRED:'비밀번호 필요',LIVE:'방송중',COMPLETED:'완료',STOPPED:'중지',FAILED:'실패',INTERRUPTED:'비정상 종료',CANCELLED:'취소'};",
"const labels={UNKNOWN:'확인중',OFFLINE:'오프라인',RECORDING:'녹화중',PAUSED:'현재방송 중지',WATCHER_STOPPED:'Watcher 중지',DISABLED:'비활성',ERROR:'오류',LOW_DISK:'디스크 부족',STALLED:'녹화 정지',AUTH:'인증 필요',PASSWORD_REQUIRED:'비밀번호 필요',LIVE:'방송중',COMPLETED:'완료',STOPPED:'중지',FAILED:'실패',INTERRUPTED:'비정상 종료',CANCELLED:'취소'};",
'watcher stopped label')
old="function runtimeRow(c){const tr=document.createElement('tr');const detail=[c.title,c.file,c.detail].filter(Boolean).map(esc).join('<br>');const klass=c.status==='RECORDING'?'ok':(c.status==='OFFLINE'||c.status==='DISABLED'?'muted':(c.status==='PAUSED'||c.status==='PASSWORD_REQUIRED'?'warn':(c.status==='ERROR'||c.status==='LOW_DISK'||c.status==='STALLED'?'bad':'')));const stopButton=c.status==='PAUSED'?'<button class=\"mini resume\">재개</button>':(c.status==='RECORDING'?'<button class=\"mini danger stopOne\">현재 방송 중지</button>':(c.status==='PASSWORD_REQUIRED'?'<button class=\"mini streamPassword\">비밀번호 입력</button>':''));tr.innerHTML=`<td class=\"${klass}\"><b>${esc(statusText(c.status))}</b></td><td>${esc(c.name)}</td><td class=\"mono\">${esc(c.account)}</td><td class=\"smallcell\">${detail||'-'}</td><td>${bytes(c.size_bytes)}</td><td><button class=\"mini recheck\">재확인</button> ${stopButton}</td>`;tr.querySelector('.recheck').onclick=()=>channelAction(c.account,'recheck');const r=tr.querySelector('.resume');if(r)r.onclick=()=>channelAction(c.account,'resume');const s=tr.querySelector('.stopOne');if(s)s.onclick=()=>channelAction(c.account,'stop');const p=tr.querySelector('.streamPassword');if(p)p.onclick=()=>channelPassword(c.account);return tr}"
new="function runtimeRow(c,watcherRunning=true){const tr=document.createElement('tr');const detail=[c.title,c.file,c.detail].filter(Boolean).map(esc).join('<br>');const klass=c.status==='RECORDING'?'ok':(c.status==='OFFLINE'||c.status==='DISABLED'||c.status==='WATCHER_STOPPED'?'muted':(c.status==='PAUSED'||c.status==='PASSWORD_REQUIRED'?'warn':(c.status==='ERROR'||c.status==='LOW_DISK'||c.status==='STALLED'?'bad':'')));const stopButton=c.status==='PAUSED'?'<button class=\"mini resume\">재개</button>':(c.status==='RECORDING'?'<button class=\"mini danger stopOne\">현재 방송 중지</button>':(c.status==='PASSWORD_REQUIRED'?'<button class=\"mini streamPassword\">비밀번호 입력</button>':''));const controls=watcherRunning?`<button class=\"mini recheck\">재확인</button> ${stopButton}`:'-';tr.innerHTML=`<td class=\"${klass}\"><b>${esc(statusText(c.status))}</b></td><td>${esc(c.name)}</td><td class=\"mono\">${esc(c.account)}</td><td class=\"smallcell\">${detail||'-'}</td><td>${bytes(c.size_bytes)}</td><td>${controls}</td>`;const q=tr.querySelector('.recheck');if(q)q.onclick=()=>channelAction(c.account,'recheck');const r=tr.querySelector('.resume');if(r)r.onclick=()=>channelAction(c.account,'resume');const s=tr.querySelector('.stopOne');if(s)s.onclick=()=>channelAction(c.account,'stop');const p=tr.querySelector('.streamPassword');if(p)p.onclick=()=>channelPassword(c.account);return tr}"
s=rep(s,old,new,'runtime row watcher controls')
old="function renderStatus(d){const w=d?.watcher||{};$('watcher').textContent=w.running?'RUNNING':'STOPPED';$('watcher').className=w.running?'ok':'bad';$('engine').textContent=w.engine||'-';$('channelCount').textContent=w.channel_count??0;$('recCount').textContent=w.recording_count??0;$('offlineCount').textContent=w.offline_count??0;$('errorCount').textContent=w.error_count??0;$('backend').textContent=d?.backend_dir||'-';$('start').disabled=!!w.running;$('stop').disabled=!w.running;const body=$('runtime');body.replaceChildren();(w.channels||[]).forEach(c=>body.appendChild(runtimeRow(c)))}"
new="function renderStatus(d){const w=d?.watcher||{};$('watcher').textContent=w.running?'RUNNING':'STOPPED';$('watcher').className=w.running?'ok':'bad';$('engine').textContent=w.engine||'-';$('channelCount').textContent=w.channel_count??0;$('recCount').textContent=w.recording_count??0;$('offlineCount').textContent=w.offline_count??0;$('errorCount').textContent=w.error_count??0;$('backend').textContent=d?.backend_dir||'-';$('start').disabled=!!w.running;$('stop').disabled=!w.running;const body=$('runtime');body.replaceChildren();(w.channels||[]).forEach(c=>body.appendChild(runtimeRow(c,!!w.running)))}"
s=rep(s,old,new,'render status watcher controls')
write(p,s)

assert 'WATCHER_STOPPED' in read('rust-web/src/native_watcher.rs')
assert "WATCHER_STOPPED:'Watcher 중지'" in read('rust-web/web/app.js')
assert 'runtimeRow(c,!!w.running)' in read('rust-web/web/app.js')
print('Phase 11 watcher stopped patch sanity: PASS')
