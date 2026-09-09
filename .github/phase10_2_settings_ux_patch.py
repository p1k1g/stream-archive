from pathlib import Path
import re


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def replace_once(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# ---------------- phase8.js: Korean setting metadata / grouping / diagnostics integration ----------------
p='rust-web/web/phase8.js'
s=read(p)
old="const p8SettingGroup={CHECK_INTERVAL:'watcher',CHANNEL_RELOAD_INTERVAL:'watcher',RECORD_RETRY_INTERVAL:'watcher',RECORD_STALL_TIMEOUT:'watcher',RECORD_MONITOR_INTERVAL:'watcher',WORKER_MAX_RETRY:'watcher',CONSOLE_REFRESH_INTERVAL:'alerts',CONSOLE_AUTO_FORMAT:'alerts',CHANNEL_NAME_WIDTH:'alerts',CONSOLE_COLOR:'alerts',CONSOLE_SHOW_PATH:'alerts',GUI_NOTIFY_RECORD_START:'alerts',GUI_NOTIFY_RECORD_FINISH:'alerts',GUI_NOTIFY_WARNING:'alerts',MIN_FREE_SPACE_GB:'general',OUTPUT_DIR:'general',QUALITY:'general',FILE_NAME_PATTERN:'general',STREAMLINK_PATH:'tools',STREAMLINK_FALLBACK:'tools',YT_DLP_PATH:'tools',FFMPEG_PATH:'tools',SOOP_USERNAME:'general',MASTER_QUALITY:'general',CLOUDFLARE_WORKER_URL:'general',LOG_ENABLED:'logs',LOG_DIR:'logs',LOG_RETENTION_DAYS:'logs'};"
new="const p8SettingGroup={CHECK_INTERVAL:'watcher',CHANNEL_RELOAD_INTERVAL:'watcher',RECORD_RETRY_INTERVAL:'watcher',RECORD_STALL_TIMEOUT:'watcher',RECORD_MONITOR_INTERVAL:'watcher',WORKER_MAX_RETRY:'watcher',CONSOLE_REFRESH_INTERVAL:'advanced',CONSOLE_AUTO_FORMAT:'advanced',CHANNEL_NAME_WIDTH:'advanced',CONSOLE_COLOR:'advanced',CONSOLE_SHOW_PATH:'advanced',GUI_NOTIFY_RECORD_START:'advanced',GUI_NOTIFY_RECORD_FINISH:'advanced',GUI_NOTIFY_WARNING:'advanced',MIN_FREE_SPACE_GB:'general',OUTPUT_DIR:'general',QUALITY:'general',FILE_NAME_PATTERN:'general',STREAMLINK_PATH:'tools',STREAMLINK_FALLBACK:'tools',YT_DLP_PATH:'tools',FFMPEG_PATH:'tools',SOOP_USERNAME:'soop',MASTER_QUALITY:'advanced',CLOUDFLARE_WORKER_URL:'advanced',LOG_ENABLED:'logs',LOG_DIR:'logs',LOG_RETENTION_DAYS:'logs'};"
s=replace_once(s,old,new,'setting group')
anchor="const p8PathKeys=new Set(['OUTPUT_DIR','LOG_DIR','STREAMLINK_PATH','STREAMLINK_FALLBACK','YT_DLP_PATH','FFMPEG_PATH']);"
meta=r'''const p8PathKeys=new Set(['OUTPUT_DIR','LOG_DIR','STREAMLINK_PATH','STREAMLINK_FALLBACK','YT_DLP_PATH','FFMPEG_PATH']);
const p102SettingMeta={
CHECK_INTERVAL:{label:'방송 확인 주기',desc:'채널의 방송 시작·종료 여부를 확인하는 간격입니다.',unit:'초',recommended:'30'},
CHANNEL_RELOAD_INTERVAL:{label:'채널 목록 갱신 주기',desc:'SQLite 채널 목록 변경을 Watcher에 반영하는 간격입니다.',unit:'초',recommended:'2'},
RECORD_RETRY_INTERVAL:{label:'녹화 재시도 간격',desc:'녹화 시작 실패 후 다시 시도하기까지 기다리는 시간입니다.',unit:'초',recommended:'5'},
RECORD_STALL_TIMEOUT:{label:'녹화 정지 감지 시간',desc:'파일 크기가 늘지 않을 때 비정상 정지로 판단하는 시간입니다. 방송 종료 감지의 예비 안전장치로도 사용됩니다.',unit:'초',recommended:'90'},
RECORD_MONITOR_INTERVAL:{label:'녹화 상태 확인 주기',desc:'녹화 프로세스와 파일 크기·디스크 상태를 확인하는 주기입니다.',unit:'초',recommended:'5'},
WORKER_MAX_RETRY:{label:'스트림 조회 최대 재시도',desc:'Cloudflare Worker에서 HLS 주소 조회가 실패했을 때 재시도할 횟수입니다.',unit:'회',recommended:'3'},
MIN_FREE_SPACE_GB:{label:'최소 여유 디스크',desc:'남은 공간이 이 값 이하가 되면 새 녹화를 막거나 진행 중 녹화를 정리합니다.',unit:'GB',recommended:'20'},
OUTPUT_DIR:{label:'기본 녹화 폴더',desc:'채널별 출력 폴더를 따로 지정하지 않았을 때 LIVE 파일이 저장되는 기본 위치입니다.'},
QUALITY:{label:'녹화 화질',desc:'Streamlink에 전달할 기본 녹화 화질입니다.',recommended:'best'},
FILE_NAME_PATTERN:{label:'파일 이름 형식',desc:'LIVE 녹화 파일 이름을 만드는 규칙입니다.',recommended:'LEGACY'},
STREAMLINK_PATH:{label:'Streamlink 경로',desc:'LIVE 녹화에 사용하는 streamlink.exe 경로입니다. AUTO면 자동 탐지합니다.',recommended:'AUTO'},
STREAMLINK_FALLBACK:{label:'Streamlink 대체 경로',desc:'기본 Streamlink 경로를 찾지 못했을 때 확인할 보조 경로입니다.',recommended:'AUTO'},
YT_DLP_PATH:{label:'yt-dlp 경로',desc:'VOD 분석·다운로드에 사용하는 yt-dlp 실행 파일 경로입니다.',recommended:'AUTO 또는 실행 파일 경로'},
FFMPEG_PATH:{label:'FFmpeg 경로',desc:'VOD PART 병합과 미디어 처리에 사용하는 ffmpeg 실행 파일 경로입니다.',recommended:'AUTO 또는 실행 파일 경로'},
SOOP_USERNAME:{label:'SOOP 아이디',desc:'로그인이 필요한 LIVE/VOD 기능에서 사용할 SOOP 계정 아이디입니다.'},
LOG_ENABLED:{label:'로그 파일 저장',desc:'런타임 로그를 파일에도 저장할지 선택합니다.',recommended:'켜기'},
LOG_DIR:{label:'로그 저장 폴더',desc:'로그 파일을 저장할 폴더입니다.',recommended:'.\\logs'},
LOG_RETENTION_DAYS:{label:'로그 보관 기간',desc:'오래된 로그 파일을 보관할 기간입니다. 0은 정리하지 않는 설정으로 사용할 수 있습니다.',unit:'일',recommended:'30'},
MASTER_QUALITY:{label:'마스터 화질 요청값',desc:'Worker에 전달하는 마스터 화질 관련 고급 값입니다. 특별한 이유가 없으면 변경하지 마세요.',recommended:'auto'},
CLOUDFLARE_WORKER_URL:{label:'Cloudflare Worker 주소',desc:'SOOP 스트림 주소를 조회하는 Worker HTTPS 엔드포인트입니다.'},
CONSOLE_REFRESH_INTERVAL:{label:'콘솔 새로고침 주기',desc:'레거시/호환 콘솔 표시를 갱신하는 간격입니다.',unit:'초'},
CONSOLE_AUTO_FORMAT:{label:'콘솔 자동 정렬',desc:'레거시/호환 콘솔 출력의 자동 형식 정리를 사용합니다.'},
CHANNEL_NAME_WIDTH:{label:'채널 이름 표시 폭',desc:'레거시/호환 콘솔에서 채널 이름을 표시할 폭입니다.',recommended:'AUTO'},
CONSOLE_COLOR:{label:'콘솔 색상 사용',desc:'레거시/호환 콘솔에서 상태 색상을 표시합니다.'},
CONSOLE_SHOW_PATH:{label:'콘솔 파일 경로 표시',desc:'레거시/호환 콘솔에 녹화 파일 경로를 표시합니다.'},
GUI_NOTIFY_RECORD_START:{label:'녹화 시작 알림',desc:'녹화 시작 알림 사용 여부를 저장하는 호환 설정입니다.'},
GUI_NOTIFY_RECORD_FINISH:{label:'녹화 종료 알림',desc:'녹화 종료 알림 사용 여부를 저장하는 호환 설정입니다.'},
GUI_NOTIFY_WARNING:{label:'경고 알림',desc:'오류·경고 알림 사용 여부를 저장하는 호환 설정입니다.'}
};'''
s=replace_once(s,anchor,meta,'settings metadata')

old="function p8SetAppTab(name,persist=true){const valid=['dashboard','channels','vod','history','settings','diagnostics'];if(!valid.includes(name))name='dashboard';p8State.appTab=name;document.querySelectorAll('[data-tab-page]').forEach(el=>el.hidden=el.dataset.tabPage!==name);document.querySelectorAll('[data-app-tab]').forEach(btn=>{const on=btn.dataset.appTab===name;btn.classList.toggle('active',on);btn.setAttribute('aria-selected',on?'true':'false')});if(persist)localStorage.setItem('soopAppTab',name);if(location.hash!=='#'+name)history.replaceState(null,'','#'+name);if(name==='settings')setTimeout(()=>p8EnsureSettingsUX(),0);window.scrollTo({top:0,behavior:'instant'})}"
new="function p8SetAppTab(name,persist=true){const valid=['dashboard','channels','vod','history','settings'];if(!valid.includes(name))name='dashboard';p8State.appTab=name;document.querySelectorAll('[data-tab-page]').forEach(el=>el.hidden=el.dataset.tabPage!==name);document.querySelectorAll('[data-app-tab]').forEach(btn=>{const on=btn.dataset.appTab===name;btn.classList.toggle('active',on);btn.setAttribute('aria-selected',on?'true':'false')});if(persist)localStorage.setItem('soopAppTab',name);if(location.hash!=='#'+name)history.replaceState(null,'','#'+name);if(name==='settings')setTimeout(()=>p8EnsureSettingsUX(),0);window.scrollTo({top:0,behavior:'instant'})}"
s=replace_once(s,old,new,'app tabs')
old="function p8ApplySettingsVisibility(name){const security=name==='security';const fields=$('p8SettingsFields'),sec=$('p8SecurityPanel');if(fields)fields.hidden=security;if(sec)sec.hidden=!security;let visible=0;document.querySelectorAll('#settings label[data-setting-group]').forEach(label=>{const on=label.dataset.settingGroup===name;label.classList.toggle('p8-hidden',security||!on);if(on&&!security)visible++});const empty=$('p8SettingsEmpty');if(empty)empty.hidden=security||visible>0}"
new="function p8ApplySettingsVisibility(name){const sec=$('p8SecurityPanel'),advanced=$('p102AdvancedPanel');if(sec)sec.hidden=name!=='soop';if(advanced)advanced.hidden=name!=='advanced';let visible=0;document.querySelectorAll('#settings label[data-setting-group]').forEach(label=>{const on=label.dataset.settingGroup===name;label.classList.toggle('p8-hidden',!on);if(on)visible++});const empty=$('p8SettingsEmpty');if(empty)empty.hidden=visible>0||name==='soop'||name==='advanced'}"
s=replace_once(s,old,new,'settings visibility')
old="function p8SetSettingsTab(name,persist=true){const valid=['general','watcher','tools','logs','alerts','security'];if(!valid.includes(name))name='general';p8State.settingsTab=name;document.querySelectorAll('[data-settings-tab]').forEach(btn=>{const on=btn.dataset.settingsTab===name;btn.classList.toggle('active',on);btn.setAttribute('aria-selected',on?'true':'false')});if(persist)localStorage.setItem('soopSettingsTab',name);if(name!=='security'&&!p8HasGroupedSettings()){p8EnsureSettingsUX(true).then(()=>p8ApplySettingsVisibility(name)).catch(e=>toast('설정 UI 로드 실패: '+e.message));return}p8ApplySettingsVisibility(name)}"
new="function p8SetSettingsTab(name,persist=true){const valid=['general','watcher','tools','soop','logs','advanced'];if(!valid.includes(name))name='general';p8State.settingsTab=name;document.querySelectorAll('[data-settings-tab]').forEach(btn=>{const on=btn.dataset.settingsTab===name;btn.classList.toggle('active',on);btn.setAttribute('aria-selected',on?'true':'false')});if(persist)localStorage.setItem('soopSettingsTab',name);if(!p8HasGroupedSettings()){p8EnsureSettingsUX(true).then(()=>p8ApplySettingsVisibility(name)).catch(e=>toast('설정 UI 로드 실패: '+e.message));return}p8ApplySettingsVisibility(name)}"
s=replace_once(s,old,new,'settings tabs')
old="function p8InitTabs(){document.querySelectorAll('[data-app-tab]').forEach(btn=>btn.onclick=()=>p8SetAppTab(btn.dataset.appTab));document.querySelectorAll('[data-settings-tab]').forEach(btn=>btn.onclick=()=>p8SetSettingsTab(btn.dataset.settingsTab));const hash=(location.hash||'').replace(/^#/,'');const initial=['dashboard','channels','vod','history','settings','diagnostics'].includes(hash)?hash:(localStorage.getItem('soopAppTab')||'dashboard');p8SetAppTab(initial,false);p8SetSettingsTab(localStorage.getItem('soopSettingsTab')||'general',false);window.addEventListener('hashchange',()=>{const h=location.hash.replace(/^#/,'');if(['dashboard','channels','vod','history','settings','diagnostics'].includes(h))p8SetAppTab(h,false)})}"
new="function p8InitTabs(){document.querySelectorAll('[data-app-tab]').forEach(btn=>btn.onclick=()=>p8SetAppTab(btn.dataset.appTab));document.querySelectorAll('[data-settings-tab]').forEach(btn=>btn.onclick=()=>p8SetSettingsTab(btn.dataset.settingsTab));const hash=(location.hash||'').replace(/^#/,'');const migratedDiag=hash==='diagnostics'||localStorage.getItem('soopAppTab')==='diagnostics';const initial=migratedDiag?'settings':(['dashboard','channels','vod','history','settings'].includes(hash)?hash:(localStorage.getItem('soopAppTab')||'dashboard'));p8SetAppTab(initial,false);p8SetSettingsTab(migratedDiag?'advanced':(localStorage.getItem('soopSettingsTab')||'general'),false);window.addEventListener('hashchange',()=>{const h=location.hash.replace(/^#/,'');if(h==='diagnostics'){p8SetAppTab('settings',false);p8SetSettingsTab('advanced');return}if(['dashboard','channels','vod','history','settings'].includes(h))p8SetAppTab(h,false)})}"
s=replace_once(s,old,new,'tab init')
old="async function p8LoadSettingsUX(){if(p8State.settingsRendering)return;p8State.settingsRendering=true;try{const [d,v]=await Promise.all([api('/api/settings'),api('/api/vod/tool-settings')]);const merged={...d.values,...v};const box=$('settings');if(!box)return;box.replaceChildren();for(const key of p8SettingKeys){const label=document.createElement('label');label.dataset.settingGroup=p8SettingGroup[key]||'general';const title=document.createElement('span');title.textContent=key;label.appendChild(title);label.appendChild(p8SettingControl(key,merged[key]??''));if(p8PathKeys.has(key)){const note=document.createElement('span');note.className='hint';note.textContent='현재는 직접 입력 또는 AUTO를 사용합니다. 파일/폴더 선택 버튼은 다음 로컬 picker 단계에서 추가합니다.';label.appendChild(note)}box.appendChild(label)}p8ApplySettingsVisibility(p8State.settingsTab)}finally{p8State.settingsRendering=false}}"
new="async function p8LoadSettingsUX(){if(p8State.settingsRendering)return;p8State.settingsRendering=true;try{const [d,v]=await Promise.all([api('/api/settings'),api('/api/vod/tool-settings')]);const merged={...d.values,...v};const box=$('settings');if(!box)return;box.replaceChildren();for(const key of p8SettingKeys){const meta=p102SettingMeta[key]||{label:key,desc:''};const label=document.createElement('label');label.className='setting-field';label.dataset.settingGroup=p8SettingGroup[key]||'general';const title=document.createElement('span');title.className='setting-title';const name=document.createElement('strong');name.textContent=meta.label||key;const raw=document.createElement('code');raw.className='setting-key';raw.textContent=key;title.append(name,raw);label.appendChild(title);label.appendChild(p8SettingControl(key,merged[key]??''));const help=document.createElement('span');help.className='hint setting-help';help.textContent=[meta.desc,meta.unit?`단위: ${meta.unit}`:'',meta.recommended?`권장: ${meta.recommended}`:''].filter(Boolean).join(' · ');label.appendChild(help);box.appendChild(label)}p8ApplySettingsVisibility(p8State.settingsTab)}finally{p8State.settingsRendering=false}}"
s=replace_once(s,old,new,'settings render')
write(p,s)

# ---------------- index.html: five top tabs, categorized settings, diagnostics under Advanced ----------------
p='rust-web/web/index.html'
s=read(p)
s=s.replace('Rust Web Phase 10 · ID/PW Session Auth','Rust Web Phase 10.2 · 쉬운 설정 UI',1)
s=s.replace('  <button type="button" role="tab" data-app-tab="diagnostics" aria-selected="false">진단</button>\n','',1)
old='''    <button type="button" role="tab" data-settings-tab="general" class="active" aria-selected="true">일반</button>\n    <button type="button" role="tab" data-settings-tab="watcher" aria-selected="false">Watcher</button>\n    <button type="button" role="tab" data-settings-tab="tools" aria-selected="false">도구</button>\n    <button type="button" role="tab" data-settings-tab="logs" aria-selected="false">로그</button>\n    <button type="button" role="tab" data-settings-tab="alerts" aria-selected="false">알림</button>\n    <button type="button" role="tab" data-settings-tab="security" aria-selected="false">보안</button>'''
new='''    <button type="button" role="tab" data-settings-tab="general" class="active" aria-selected="true">기본</button>\n    <button type="button" role="tab" data-settings-tab="watcher" aria-selected="false">방송 / 녹화</button>\n    <button type="button" role="tab" data-settings-tab="tools" aria-selected="false">외부 프로그램</button>\n    <button type="button" role="tab" data-settings-tab="soop" aria-selected="false">SOOP 인증</button>\n    <button type="button" role="tab" data-settings-tab="logs" aria-selected="false">로그</button>\n    <button type="button" role="tab" data-settings-tab="advanced" aria-selected="false">고급</button>'''
s=replace_once(s,old,new,'settings tab html')
s=s.replace('<div class="title inner-title"><h3>보안 설정</h3><button id="saveSecrets">암호화 저장</button></div>','<div class="title inner-title"><h3>SOOP / Worker 인증</h3><button id="saveSecrets">암호화 저장</button></div>',1)
s=s.replace('<div class="grid"><label>SOOP_PASSWORD <span id="soopPasswordState" class="hint">확인 중</span><input id="soopPassword" type="password" autocomplete="new-password" placeholder="빈칸이면 기존 값 유지"></label><label>CLOUDFLARE_API_KEY <span id="cloudflareKeyState" class="hint">확인 중</span><input id="cloudflareKey" type="password" autocomplete="new-password" placeholder="빈칸이면 기존 값 유지"></label></div>','<div class="grid"><label><span class="setting-title"><strong>SOOP 비밀번호</strong><code class="setting-key">SOOP_PASSWORD</code></span><span id="soopPasswordState" class="hint">확인 중</span><input id="soopPassword" type="password" autocomplete="new-password" placeholder="빈칸이면 기존 값 유지"></label><label><span class="setting-title"><strong>Worker API Key</strong><code class="setting-key">CLOUDFLARE_API_KEY</code></span><span id="cloudflareKeyState" class="hint">확인 중</span><input id="cloudflareKey" type="password" autocomplete="new-password" placeholder="빈칸이면 기존 값 유지"></label></div>',1)
advanced='''\n  <div id="p102AdvancedPanel" hidden>\n    <div class="title inner-title"><h3>서버 / 진단 정보</h3><button id="refreshDiagnostics">새로고침</button></div>\n    <div class="summary"><span>수신 주소 <b id="diagBind">-</b></span><span>Watcher 설정 원본 <b id="diagSource">-</b></span><span>로컬 전용 <b id="diagLoopback">-</b></span></div>\n    <p class="mono">SQLite DB: <span id="diagDb">-</span></p>\n    <p id="diagRemote" class="hint">진단 중...</p>\n    <div class="summary"><span>Streamlink <b id="diagStreamlink">-</b></span><span>yt-dlp <b id="diagYtdlp">-</b></span><span>FFmpeg <b id="diagFfmpeg">-</b></span></div>\n    <p class="hint">일반 사용자는 이 영역을 변경할 필요가 없습니다. 기본 서버는 127.0.0.1:8787 로컬 전용이며, 외부 접속은 Caddy/HTTPS 구성을 권장합니다.</p>\n  </div>'''
s=replace_once(s,'  </div>\n</section>\n</div>\n\n<div class="tab-page" data-tab-page="diagnostics" hidden>',advanced+'\n  </div>\n</section>\n</div>\n\n<div class="tab-page" data-tab-page="diagnostics" hidden>','advanced insertion')
# Remove old standalone diagnostics page completely.
s=re.sub(r'\n<div class="tab-page" data-tab-page="diagnostics" hidden>\n<section>.*?</section>\n</div>\n(?=</main>)','\n',s,flags=re.S)
# Cache-bust web assets for the new UI.
s=s.replace('p10-auth1','p10-2')
write(p,s)

# ---------------- phase10.js: auth actions then recovery token at far right ----------------
p='rust-web/web/phase10.js'
s=read(p)
s=replace_once(s,'.p10-authbar{display:flex;gap:8px;align-items:center;flex-wrap:wrap;justify-content:flex-end}.p10-user{color:#9fe3b0;font-size:13px}.p10-authbar button{padding:7px 10px;font-size:12px}', '.p10-authbar{display:flex;gap:8px;align-items:center;flex-wrap:wrap;justify-content:flex-end;margin-left:auto}.p10-user{color:#9fe3b0;font-size:13px}.p10-authbar button{padding:7px 10px;font-size:12px}#tokenBtn{flex:0 0 auto}', 'authbar css')
s=replace_once(s,'@media(max-width:700px){header{gap:12px;flex-wrap:wrap}.p10-authbar{width:100%;justify-content:flex-start}}','@media(max-width:700px){header{gap:12px;flex-wrap:wrap}.p10-authbar{width:100%;justify-content:flex-start;margin-left:0;order:2}#tokenBtn{order:3;margin-left:auto}}','authbar mobile css')
s=replace_once(s,"if(!authBar){authBar=document.createElement('div');authBar.className='p10-authbar';header.appendChild(authBar)}","if(!authBar){authBar=document.createElement('div');authBar.className='p10-authbar';const tokenButton=document.getElementById('tokenBtn');if(tokenButton&&tokenButton.parentElement===header)header.insertBefore(authBar,tokenButton);else header.appendChild(authBar)}",'authbar DOM order')
write(p,s)

# ---------------- style.css: setting cards / readable metadata ----------------
p='rust-web/web/style.css'
s=read(p)
append='''.setting-field{background:#0f151c;border:1px solid #283442;border-radius:10px;padding:12px;gap:8px!important}.setting-title{display:flex;align-items:center;justify-content:space-between;gap:10px;color:#edf2f7}.setting-title strong{font-size:14px}.setting-key{font-family:Consolas,monospace;font-size:10px;font-weight:400;color:#7f8c9b;background:#0a0f14;border:1px solid #27313c;border-radius:5px;padding:2px 5px;white-space:nowrap}.setting-help{font-size:11px;line-height:1.45}.inner-title h3{margin:0}#p102AdvancedPanel,#p8SecurityPanel{margin-top:16px;padding-top:16px;border-top:1px solid #28303a}@media(max-width:700px){.setting-title{align-items:flex-start;flex-direction:column;gap:4px}.setting-key{white-space:normal;word-break:break-all}}'''
if '.setting-field{' not in s:
    s += append
write(p,s)

# Static sanity checks before CI compilation/package tests.
idx=read('rust-web/web/index.html')
p8=read('rust-web/web/phase8.js')
p10=read('rust-web/web/phase10.js')
assert 'data-app-tab="diagnostics"' not in idx
assert 'data-tab-page="diagnostics"' not in idx
assert 'data-settings-tab="advanced"' in idx
assert 'data-settings-tab="soop"' in idx
assert '방송 확인 주기' in p8 and '최소 여유 디스크' in p8 and 'Streamlink 경로' in p8
assert "header.insertBefore(authBar,tokenButton)" in p10
assert 'id="p102AdvancedPanel"' in idx and 'id="diagStreamlink"' in idx
print('Phase 10.2 web patch sanity: PASS')
