(()=>{
'use strict';

const P14_STORAGE='streamArchiveNotificationsV1';
const P14_LEGACY='soopBrowserNotify';
const P14_DEFAULTS={enabled:false,liveStart:true,liveComplete:true,liveFailure:true,vodStart:true,vodComplete:true,vodFailure:true,vodCancel:true};
let p14Config=p14LoadConfig();
let p14LiveSeen=false;
let p14VodSeen=false;
const p14LiveStates=new Map();
const p14VodStates=new Map();
let p14Panel=null;
let p14TabButton=null;
let p14TrackingInstalled=false;

function p14LoadConfig(){
  let saved=null;
  try{saved=JSON.parse(localStorage.getItem(P14_STORAGE)||'null')}catch{}
  const legacy=localStorage.getItem(P14_LEGACY)==='Y';
  const cfg={...P14_DEFAULTS,...(saved&&typeof saved==='object'?saved:{})};
  if(!saved&&legacy)cfg.enabled=true;
  try{localStorage.setItem(P14_STORAGE,JSON.stringify(cfg));localStorage.setItem(P14_LEGACY,'N')}catch{}
  return cfg;
}
function p14SaveConfig(){try{localStorage.setItem(P14_STORAGE,JSON.stringify(p14Config));localStorage.setItem(P14_LEGACY,'N')}catch{}}
function p14Supported(){return 'Notification' in window}
function p14Permission(){return p14Supported()?Notification.permission:'unsupported'}
function p14PermissionInfo(){
  const state=p14Permission();
  if(state==='granted')return{label:'허용됨',detail:'이 브라우저에서 Stream Archive 알림을 표시할 수 있습니다.',klass:'ok'};
  if(state==='denied')return{label:'차단됨',detail:'브라우저 사이트 권한에서 알림을 허용해야 합니다.',klass:'bad'};
  if(state==='default')return{label:'권한 미설정',detail:'권한 요청을 눌러 브라우저 알림을 허용하세요.',klass:'warn'};
  return{label:'미지원',detail:'이 브라우저 또는 현재 접속 환경에서는 알림 API를 사용할 수 없습니다.',klass:'bad'};
}
function p14CanSend(key){return !!p14Config.enabled&&!!p14Config[key]&&p14Supported()&&Notification.permission==='granted'}
function p14Notify(key,title,body,tag){
  if(!p14CanSend(key))return false;
  try{new Notification(title,{body,tag});return true}catch(e){console.warn('Phase14 notification failed',e);return false}
}
function p14ForceTest(){
  if(!p14Supported()||Notification.permission!=='granted')return false;
  try{new Notification('Stream Archive 테스트',{body:'LIVE / VOD 브라우저 알림이 정상적으로 동작합니다.',tag:'stream-archive-phase14-test'});return true}catch(e){console.warn('Phase14 test notification failed',e);return false}
}
function p14Name(c){return c?.name||c?.account||'LIVE 채널'}
function p14TrackLive(watcher){
  const channels=watcher?.channels||[];
  if(!p14LiveSeen){channels.forEach(c=>p14LiveStates.set(c.account,c.status));p14LiveSeen=true;return}
  const next=new Map();
  for(const c of channels){
    const key=c.account||c.name||String(next.size);
    const prev=p14LiveStates.get(key);
    const now=c.status;
    next.set(key,now);
    if(prev===now)continue;
    if(now==='RECORDING'&&prev!=='RECORDING'){
      p14Notify('liveStart','LIVE 녹화 시작',`${p14Name(c)}${c.title?' · '+c.title:''}`,`stream-archive-live-start-${key}`);
      continue;
    }
    if(prev==='RECORDING'){
      if(['ERROR','LOW_DISK','STALLED','INTERRUPTED'].includes(now))p14Notify('liveFailure','LIVE 녹화 실패',`${p14Name(c)} · ${typeof statusText==='function'?statusText(now):now}`,`stream-archive-live-failure-${key}`);
      else p14Notify('liveComplete','LIVE 녹화 종료',`${p14Name(c)} · ${typeof statusText==='function'?statusText(now):now}`,`stream-archive-live-complete-${key}`);
      continue;
    }
    if(['ERROR','LOW_DISK','STALLED'].includes(now)&&prev)p14Notify('liveFailure','LIVE 감시 오류',`${p14Name(c)} · ${typeof statusText==='function'?statusText(now):now}`,`stream-archive-live-error-${key}`);
  }
  p14LiveStates.clear();next.forEach((v,k)=>p14LiveStates.set(k,v));
}
function p14VodLabel(item){return item?.title||item?.streamer||item?.vod_url||'VOD'}
function p14TrackVod(snapshot){
  const items=snapshot?.items||[];
  if(!p14VodSeen){items.forEach(i=>p14VodStates.set(i.id,i.state));p14VodSeen=true;return}
  const next=new Map();
  for(const item of items){
    const prev=p14VodStates.get(item.id);
    const now=item.state;
    next.set(item.id,now);
    if(prev===now)continue;
    const name=p14VodLabel(item);
    if(now==='RUNNING'&&prev!=='RUNNING')p14Notify('vodStart','VOD 다운로드 시작',name,`stream-archive-vod-start-${item.id}`);
    else if(now==='COMPLETED')p14Notify('vodComplete','VOD 다운로드 완료',name,`stream-archive-vod-complete-${item.id}`);
    else if(now==='FAILED'||now==='INTERRUPTED')p14Notify('vodFailure','VOD 다운로드 실패',`${name}${item.message?' · '+item.message:''}`,`stream-archive-vod-failure-${item.id}`);
    else if(now==='CANCELLED')p14Notify('vodCancel','VOD 다운로드 취소',name,`stream-archive-vod-cancel-${item.id}`);
  }
  p14VodStates.clear();next.forEach((v,k)=>p14VodStates.set(k,v));
}
function p14InstallTracking(){
  if(p14TrackingInstalled)return;
  const bus=window.StreamArchiveState;
  if(!bus?.subscribe){console.warn('Phase14 state bus unavailable');return}
  p14TrackingInstalled=true;
  bus.subscribe('status',d=>{try{p14TrackLive(d?.watcher)}catch(e){console.warn('Phase14 LIVE tracking failed',e)}},{replay:true});
  bus.subscribe('queue',snapshot=>{try{p14TrackVod(snapshot)}catch(e){console.warn('Phase14 VOD tracking failed',e)}},{replay:true});
}
function p14Option(key,title,desc){return `<div class="p14-notify-option"><div><strong>${title}</strong><small>${desc}</small></div><label class="p14-switch" title="${title}"><input type="checkbox" data-p14-key="${key}"></label></div>`}
function p14BuildPanel(){
  if(p14Panel)return p14Panel;
  const settingsPage=document.querySelector('[data-tab-page="settings"] section');
  if(!settingsPage)return null;
  p14Panel=document.createElement('div');
  p14Panel.id='p14NotificationPanel';p14Panel.className='p14-notification-panel';p14Panel.hidden=true;
  p14Panel.innerHTML=`
    <div class="p14-notification-head"><div><h3>브라우저 알림</h3><p>LIVE 녹화와 VOD 다운로드 상태 변화를 현재 브라우저에서 알려줍니다.</p></div></div>
    <div class="p14-permission-card">
      <div class="p14-permission-main"><span id="p14PermissionDot" class="p14-permission-dot"></span><div><strong id="p14PermissionState">확인 중</strong><small id="p14PermissionDetail">브라우저 알림 상태를 확인합니다.</small></div></div>
      <div class="p14-permission-actions"><button id="p14RequestPermission" type="button">권한 요청</button><button id="p14TestNotification" type="button" class="secondary">테스트 알림</button></div>
    </div>
    <div class="p14-master"><div><strong>브라우저 알림 사용</strong><small>끄면 아래 항목 설정은 유지되지만 실제 알림은 전송하지 않습니다.</small></div><label class="p14-switch"><input id="p14NotifyEnabled" type="checkbox"></label></div>
    <div class="p14-notify-grid">
      <div class="p14-notify-group"><h4>LIVE 녹화</h4>${p14Option('liveStart','녹화 시작','채널의 실제 녹화가 시작될 때 알립니다.')}${p14Option('liveComplete','녹화 종료','녹화 중이던 방송이 정상적으로 종료될 때 알립니다.')}${p14Option('liveFailure','실패 / 비정상 중단','오류, 디스크 부족, 정지 감지 등 비정상 상태를 알립니다.')}</div>
      <div class="p14-notify-group"><h4>VOD 다운로드</h4>${p14Option('vodStart','다운로드 시작','큐의 VOD 작업이 실제 다운로드를 시작할 때 알립니다.')}${p14Option('vodComplete','다운로드 완료','VOD 파일 저장이 정상 완료되면 알립니다.')}${p14Option('vodFailure','다운로드 실패','실패 또는 비정상 중단 상태를 알립니다.')}${p14Option('vodCancel','다운로드 취소','사용자가 작업을 취소해 CANCELLED 상태가 되면 알립니다.')}</div>
    </div>
    <p class="hint p14-local-note">알림 권한과 선택 항목은 이 브라우저에만 저장됩니다. 다른 PC·브라우저에서는 별도로 설정해야 합니다.</p>`;
  settingsPage.appendChild(p14Panel);
  p14Panel.querySelector('#p14NotifyEnabled').addEventListener('change',e=>{p14Config.enabled=e.target.checked;p14SaveConfig();p14RenderSettings()});
  p14Panel.querySelectorAll('[data-p14-key]').forEach(input=>input.addEventListener('change',e=>{p14Config[e.target.dataset.p14Key]=e.target.checked;p14SaveConfig()}));
  p14Panel.querySelector('#p14RequestPermission').addEventListener('click',p14RequestPermission);
  p14Panel.querySelector('#p14TestNotification').addEventListener('click',p14TestNotification);
  return p14Panel;
}
function p14RenderSettings(){
  if(!p14Panel)return;
  const info=p14PermissionInfo();
  const dot=p14Panel.querySelector('#p14PermissionDot');dot.className=`p14-permission-dot ${info.klass}`;
  p14Panel.querySelector('#p14PermissionState').textContent=info.label;
  p14Panel.querySelector('#p14PermissionDetail').textContent=info.detail;
  const master=p14Panel.querySelector('#p14NotifyEnabled');master.checked=!!p14Config.enabled;
  p14Panel.querySelectorAll('[data-p14-key]').forEach(input=>{input.checked=!!p14Config[input.dataset.p14Key]});
  const request=p14Panel.querySelector('#p14RequestPermission');
  request.disabled=!p14Supported()||Notification.permission==='granted';
  request.textContent=!p14Supported()?'알림 미지원':(Notification.permission==='granted'?'권한 허용됨':'권한 요청');
  const test=p14Panel.querySelector('#p14TestNotification');test.disabled=!p14Supported();
}
async function p14RequestPermission(){
  if(!p14Supported()){alert('이 브라우저는 Notification API를 지원하지 않습니다.');return}
  try{
    const permission=await Notification.requestPermission();
    if(permission==='granted'){p14Config.enabled=true;p14SaveConfig();toast('브라우저 알림 권한 허용됨')}
    else if(permission==='denied')alert('브라우저에서 알림이 차단되었습니다. 사이트 권한에서 직접 허용할 수 있습니다.');
  }catch(e){alert('알림 권한 요청 실패: '+e.message)}
  p14RenderSettings();
}
async function p14TestNotification(){
  if(!p14Supported()){alert('이 브라우저는 알림을 지원하지 않습니다.');return}
  if(Notification.permission!=='granted')await p14RequestPermission();
  if(Notification.permission==='granted'){const sent=p14ForceTest();toast(sent?'테스트 알림 전송':'테스트 알림 전송 실패')}
  p14RenderSettings();
}
function p14DeactivateSettings(){
  if(!p14Panel)return;
  p14Panel.hidden=true;
  const fields=document.getElementById('p8SettingsFields');if(fields)fields.hidden=false;
}
function p14ActivateSettings(persist=true){
  const panel=p14BuildPanel();if(!panel)return;
  ['p8SettingsFields','p8SecurityPanel','p12BackupPanel','p102AdvancedPanel'].forEach(id=>{const el=document.getElementById(id);if(el)el.hidden=true});
  panel.hidden=false;
  document.querySelectorAll('[data-settings-tab]').forEach(btn=>{const on=btn.dataset.settingsTab==='notifications';btn.classList.toggle('active',on);btn.setAttribute('aria-selected',on?'true':'false')});
  if(persist)localStorage.setItem('soopSettingsTab','notifications');
  p14RenderSettings();
}
function p14InstallSettingsTab(){
  const tabs=document.getElementById('settingsTabs');if(!tabs)return;
  if(!document.querySelector('[data-settings-tab="notifications"]')){
    p14TabButton=document.createElement('button');p14TabButton.type='button';p14TabButton.setAttribute('role','tab');p14TabButton.dataset.settingsTab='notifications';p14TabButton.setAttribute('aria-selected','false');p14TabButton.textContent='알림';
    const logs=tabs.querySelector('[data-settings-tab="logs"]');if(logs)logs.insertAdjacentElement('afterend',p14TabButton);else tabs.appendChild(p14TabButton);
  }else p14TabButton=document.querySelector('[data-settings-tab="notifications"]');
  p14BuildPanel();
  p14TabButton.addEventListener('click',e=>{e.preventDefault();p14ActivateSettings(true)});
  tabs.querySelectorAll('[data-settings-tab]:not([data-settings-tab="notifications"])').forEach(btn=>btn.addEventListener('click',()=>p14DeactivateSettings()));
  if(localStorage.getItem('soopSettingsTab')==='notifications')setTimeout(()=>p14ActivateSettings(false),0);else p14DeactivateSettings();
}
function p14RemoveLegacyVodUi(){
  document.getElementById('vodNotify')?.remove();
  const hint=document.querySelector('.p135-vod-queue > .hint');
  if(hint)hint.innerHTML='위의 <b>큐에 추가</b>를 누르면 여러 VOD를 저장해 두고 한 건씩 순서대로 다운로드합니다. 실패·취소·서버 재시작으로 중단된 작업은 재시도할 수 있습니다. 알림은 <b>설정 → 알림</b>에서 관리합니다.';
}
function p14Init(){p14RemoveLegacyVodUi();p14InstallSettingsTab();p14InstallTracking();window.addEventListener('focus',p14RenderSettings)}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',p14Init,{once:true});else p14Init();
})();

(()=>{
'use strict';

const P166_STATUS={IDLE:'대기',READY:'대기',QUEUED:'대기',STARTING:'준비 중',ANALYZING:'분석 중',DOWNLOADING:'다운로드 중',REFRESHING:'인증 갱신 중',RUNNING:'진행 중',MERGING:'병합 중',CANCELLING:'취소 중',COMPLETED:'완료',FAILED:'실패',INTERRUPTED:'비정상 종료',CANCELLED:'취소',STOPPED:'중지'};
const P166_REASON={
  'WATCHER EXIT':'감시 종료',
  'CHANNEL REMOVED':'채널 삭제',
  'CHANNEL DISABLED':'채널 비활성화',
  'USER CHANNEL STOP':'사용자 중지',
  'BROADCAST ENDED':'방송 종료',
  'BROADCAST CHANGED':'방송 변경',
  'LOW DISK SPACE':'디스크 공간 부족',
  'RECORD STALLED':'녹화 정지 감지',
  'NORMAL':'정상 종료'
};

function p166TranslateStatus(value){return P166_STATUS[String(value||'').trim()]||value}
function p166TranslateReason(value){
  const raw=String(value||'').trim();
  if(P166_REASON[raw])return P166_REASON[raw];
  const match=raw.match(/^RECORDER EXIT CODE=(.+)$/);
  if(match)return `녹화 프로세스 비정상 종료 (코드: ${match[1]})`;
  return value;
}
function p166PatchStatusText(){
  const base=window.statusText;
  if(typeof base==='function'&&!base.__phase166){
    const wrapped=function(value){return P166_STATUS[value]||base(value)};
    wrapped.__phase166=true;
    window.statusText=wrapped;
  }
}
function p166TranslateTextNodes(root,translator){
  if(!root)return;
  const walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT);
  const nodes=[];while(walker.nextNode())nodes.push(walker.currentNode);
  for(const node of nodes){
    const raw=node.nodeValue||'';const trimmed=raw.trim();if(!trimmed)continue;
    const translated=translator(trimmed);
    if(translated!==trimmed){const start=raw.indexOf(trimmed);node.nodeValue=raw.slice(0,start)+translated+raw.slice(start+trimmed.length)}
  }
}
function p166TranslateDynamicUi(){
  for(const id of ['p13QueueRows','p8VodHistory']){
    const body=document.getElementById(id);if(!body)continue;
    body.querySelectorAll('tr td:first-child').forEach(cell=>p166TranslateTextNodes(cell,p166TranslateStatus));
  }
  const live=document.getElementById('p8LiveHistory');
  if(live){
    live.querySelectorAll('tr td:first-child').forEach(cell=>p166TranslateTextNodes(cell,p166TranslateStatus));
    live.querySelectorAll('tr td:last-child').forEach(cell=>p166TranslateTextNodes(cell,p166TranslateReason));
  }
  const vodState=document.getElementById('vodState');
  if(vodState){const translated=p166TranslateStatus(vodState.textContent);if(translated!==vodState.textContent)vodState.textContent=translated}
  const watcher=document.getElementById('watcher');if(watcher){const raw=watcher.textContent.trim();if(raw==='RUNNING')watcher.textContent='감시 중';else if(raw==='STOPPED')watcher.textContent='중지'}
}
function p166ObserveDynamicUi(){
  const targets=['p13QueueRows','p8VodHistory','p8LiveHistory','vodState','watcher'].map(id=>document.getElementById(id)).filter(Boolean);
  const observer=new MutationObserver(()=>p166TranslateDynamicUi());
  targets.forEach(el=>observer.observe(el,{childList:true,subtree:true,characterData:true}));
  p166TranslateDynamicUi();
}
function p166PolishDashboard(){
  const card=document.querySelector('.p135-watcher-card');if(!card)return;
  const spans=[...card.querySelectorAll('.summary>span')];
  if(spans[3]){for(const node of spans[3].childNodes){if(node.nodeType===Node.TEXT_NODE&&node.nodeValue.includes('OFFLINE'))node.nodeValue=node.nodeValue.replace('OFFLINE','오프라인')}}
  if(spans[4]){for(const node of spans[4].childNodes){if(node.nodeType===Node.TEXT_NODE&&node.nodeValue.includes('ERROR'))node.nodeValue=node.nodeValue.replace('ERROR','오류')}}
}
function p166InstallChannelScroll(){document.getElementById('channels')?.closest('.table')?.classList.add('p166-channel-scroll')}
function p166InstallHistoryLayout(){
  const page=document.querySelector('[data-tab-page="history"]');if(!page||page.classList.contains('p166-history-layout'))return;
  const sections=[...page.children].filter(el=>el.tagName==='SECTION');const left=sections[0];if(!left)return;
  const headings=[...left.querySelectorAll(':scope > h3')];if(headings.length<2)return;
  const liveHeading=headings[0],vodHeading=headings[1];const liveTable=liveHeading.nextElementSibling,vodTable=vodHeading.nextElementSibling;
  if(!liveTable?.classList.contains('table')||!vodTable?.classList.contains('table'))return;
  page.classList.add('p166-history-layout');left.classList.add('p166-history-live');liveTable.classList.add('p166-history-scroll');
  const right=document.createElement('section');right.className='p166-history-vod';
  const title=document.createElement('div');title.className='title';title.innerHTML='<div><span class="p135-section-kicker">VOD HISTORY</span><h2>VOD 작업 이력</h2></div>';
  vodHeading.remove();vodTable.classList.add('p166-history-scroll');right.append(title,vodTable);
  const legacy=sections.find(section=>section.getAttribute('aria-hidden')==='true');
  if(legacy)page.insertBefore(right,legacy);else page.appendChild(right);
}
async function p166LoadBackendDiagnostic(){
  const target=document.getElementById('p166DiagBackend');if(!target)return;
  try{const d=await api('/api/status');target.textContent=d?.backend_dir||'-'}catch(e){target.textContent='조회 실패'}
}
function p166InstallBackendDiagnostic(){
  const panel=document.getElementById('p102AdvancedPanel');if(!panel||document.getElementById('p166DiagBackend'))return;
  const db=document.getElementById('diagDb')?.closest('p');if(!db)return;
  const line=document.createElement('p');line.className='mono p166-diag-line';line.innerHTML='Backend: <span id="p166DiagBackend">-</span>';db.insertAdjacentElement('afterend',line);
  document.getElementById('refreshDiagnostics')?.addEventListener('click',()=>setTimeout(p166LoadBackendDiagnostic,0));
  document.querySelector('[data-settings-tab="advanced"]')?.addEventListener('click',()=>setTimeout(p166LoadBackendDiagnostic,0));
  p166LoadBackendDiagnostic();
}
async function p166LoadBackupPath(){
  const input=document.getElementById('p166BackupDir'),note=document.getElementById('p166BackupDirNote');if(!input)return;
  try{
    const [settings,backups]=await Promise.all([api('/api/settings'),api('/api/backups')]);
    input.value=settings?.values?.BACKUP_DIR||'';input.placeholder=backups?.directory||'기본 백업 위치';input.dataset.resolved=backups?.directory||'';
    const editable=backups?.directory_editable!==false;
    input.disabled=!editable;document.querySelectorAll('[data-p166-backup-action]').forEach(btn=>btn.disabled=!editable);
    if(note)note.textContent=editable?'비워두면 프로그램 폴더 바깥의 기본 soop-recorder-backups 위치를 사용합니다. 위치 변경은 즉시 다음 백업/목록 조회부터 적용되며 기존 백업 파일은 자동 이동하지 않습니다.':'SOOP_BACKUP_DIR 환경변수가 설정되어 있어 UI에서 백업 위치를 변경할 수 없습니다.';
  }catch(e){if(note)note.textContent='백업 위치 조회 실패: '+e.message}
}
async function p166PickBackupPath(){
  const input=document.getElementById('p166BackupDir');if(!input)return;
  try{
    const result=await api('/api/local-picker',{method:'POST',body:JSON.stringify({kind:'folder',filter:'all',initial_path:input.value.trim()||input.dataset.resolved||''})});
    if(result&&!result.cancelled&&result.path)input.value=result.path;
  }catch(e){alert('폴더 선택 실패: '+e.message)}
}
async function p166SaveBackupPath(){
  const input=document.getElementById('p166BackupDir');if(!input)return;
  const value=input.value.trim();
  try{
    await api('/api/settings',{method:'PUT',body:JSON.stringify({BACKUP_DIR:value})});
    await p166LoadBackupPath();
    if(typeof window.p12LoadBackups==='function')await window.p12LoadBackups();
    toast(value?'백업 위치 변경 완료':'기본 백업 위치로 변경 완료');
  }catch(e){alert('백업 위치 저장 실패: '+e.message)}
}
function p166InstallBackupPath(){
  const panel=document.getElementById('p12BackupPanel');if(!panel||document.getElementById('p166BackupDir'))return;
  const summary=panel.querySelector('.summary');if(!summary)return;
  const card=document.createElement('div');card.className='p166-backup-path-card';card.innerHTML=`
    <label>백업 디렉토리<input id="p166BackupDir" type="text" placeholder="기본 백업 위치"></label>
    <div class="p166-backup-path-actions"><button type="button" class="secondary" data-p166-backup-action="pick">폴더 선택</button><button type="button" class="secondary" data-p166-backup-action="default">기본 위치</button><button type="button" data-p166-backup-action="save">적용</button></div>
    <p id="p166BackupDirNote" class="hint">백업 위치를 확인합니다.</p>`;
  summary.insertAdjacentElement('afterend',card);
  card.querySelector('[data-p166-backup-action="pick"]').addEventListener('click',p166PickBackupPath);
  card.querySelector('[data-p166-backup-action="default"]').addEventListener('click',()=>{card.querySelector('#p166BackupDir').value=''});
  card.querySelector('[data-p166-backup-action="save"]').addEventListener('click',p166SaveBackupPath);
  const oldHint=card.nextElementSibling;if(oldHint?.classList.contains('hint'))oldHint.textContent='백업 디렉토리는 UI에서 변경할 수 있습니다. SOOP_BACKUP_DIR 환경변수가 설정된 경우 환경변수 위치가 우선하며 UI 입력은 잠깁니다.';
  document.querySelector('[data-settings-tab="backup"]')?.addEventListener('click',()=>setTimeout(p166LoadBackupPath,0));
  document.getElementById('p12BackupRefresh')?.addEventListener('click',()=>setTimeout(p166LoadBackupPath,0));
  p166LoadBackupPath();
}
function p166Init(){
  p166PatchStatusText();p166PolishDashboard();p166InstallChannelScroll();p166InstallHistoryLayout();p166InstallBackendDiagnostic();p166InstallBackupPath();p166ObserveDynamicUi();
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',p166Init,{once:true});else p166Init();
})();

(()=>{
'use strict';

const P166R_ALIASES=new Map([
  ['완료',['COMPLETED']],['실패',['FAILED']],['비정상 종료',['INTERRUPTED']],['중단됨',['INTERRUPTED']],['취소',['CANCELLED']],
  ['중지',['STOPPED','WATCHER_STOPPED']],['감시 중지',['WATCHER_STOPPED']],['대기',['READY','QUEUED','IDLE']],
  ['진행 중',['RUNNING','STARTING','ANALYZING','DOWNLOADING','REFRESHING','MERGING','CANCELLING']],['진행중',['RUNNING','STARTING','ANALYZING','DOWNLOADING','REFRESHING','MERGING','CANCELLING']],
  ['준비 중',['STARTING']],['준비중',['STARTING']],['분석 중',['ANALYZING']],['분석중',['ANALYZING']],['다운로드 중',['DOWNLOADING']],['다운로드중',['DOWNLOADING']],
  ['인증 갱신 중',['REFRESHING']],['병합 중',['MERGING']],['병합중',['MERGING']],['취소 중',['CANCELLING']],['취소중',['CANCELLING']],
  ['녹화중',['RECORDING']],['녹화 중',['RECORDING']],['오프라인',['OFFLINE']],['오류',['ERROR']],['비활성',['DISABLED']],['디스크 부족',['LOW_DISK']],
  ['녹화 정지',['STALLED']],['인증 필요',['AUTH']],['비밀번호 필요',['PASSWORD_REQUIRED']],['현재방송 중지',['PAUSED']],['현재 방송 중지',['PAUSED']],
  ['확인중',['UNKNOWN']],['확인 중',['UNKNOWN']],['방송중',['LIVE']],['방송 중',['LIVE']],['COMPLETE',['COMPLETED']]
]);
let p166rHistory={live:[],vod:[]};

function p166rStatusAlias(raw){
  const value=String(raw||'').trim().replace(/\s+/g,' ');if(!value||value==='전체')return null;
  return P166R_ALIASES.get(value)||P166R_ALIASES.get(value.toUpperCase())||null;
}
function p166rHistoryValues(){
  return{
    q:document.getElementById('p8HistoryQ')?.value.trim()||'',
    status:document.getElementById('p8HistoryStatus')?.value.trim()||'',
    from:document.getElementById('p8HistoryFrom')?.value||'',
    to:document.getElementById('p8HistoryTo')?.value||'',
    limit:Math.min(500,Math.max(1,Number(document.getElementById('p8HistoryLimit')?.value)||100))
  };
}
function p166rRenderHistory(data){
  const live=document.getElementById('p8LiveHistory'),vod=document.getElementById('p8VodHistory');if(!live||!vod)return;
  live.replaceChildren();vod.replaceChildren();
  for(const item of data.live||[]){if(typeof liveHistoryRow==='function')live.appendChild(liveHistoryRow(item))}
  for(const item of data.vod||[]){if(typeof vodHistoryRow==='function')vod.appendChild(vodHistoryRow(item))}
  if(!(data.live||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="6" class="muted">조건에 맞는 LIVE 녹화 이력이 없습니다.</td>';live.appendChild(tr)}
  if(!(data.vod||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="6" class="muted">조건에 맞는 VOD 작업 이력이 없습니다.</td>';vod.appendChild(tr)}
  if(typeof p166TranslateDynamicUi==='function')p166TranslateDynamicUi();
}
async function p166rLoadHistory(){
  const values=p166rHistoryValues();const alias=p166rStatusAlias(values.status);const params=new URLSearchParams();
  if(values.q)params.set('q',values.q);if(values.from)params.set('from',values.from);if(values.to)params.set('to',values.to);
  if(alias){params.set('limit','500')}else{if(values.status)params.set('status',values.status.toUpperCase());params.set('limit',String(values.limit))}
  try{
    const data=await api('/api/history?'+params.toString());let live=[...(data?.live||[])],vod=[...(data?.vod||[])];
    if(alias){const allowed=new Set(alias);live=live.filter(item=>allowed.has(String(item.status||'').toUpperCase())).slice(0,values.limit);vod=vod.filter(item=>allowed.has(String(item.state||'').toUpperCase())).slice(0,values.limit)}
    p166rHistory={live,vod};
    if(typeof p8State==='object'&&p8State)p8State.history=p166rHistory;
    p166rRenderHistory(p166rHistory);
  }catch(e){toast('기록 조회 실패: '+e.message)}
}
function p166rClearHistory(){
  for(const id of ['p8HistoryQ','p8HistoryStatus','p8HistoryFrom','p8HistoryTo']){const el=document.getElementById(id);if(el)el.value=''}
  const limit=document.getElementById('p8HistoryLimit');if(limit)limit.value='100';p166rLoadHistory();
}
function p166rCsv(value){const text=String(value??'');return /[",\r\n]/.test(text)?`"${text.replace(/"/g,'""')}"`:text}
function p166rExportHistory(){
  const snapshot=(typeof p8State==='object'&&p8State?.history)?p8State.history:p166rHistory;
  const rows=[['구분','상태','이름','계정/스트리머','시작','길이/PART','크기','종료 사유/결과','파일/URL']];
  for(const x of snapshot.live||[])rows.push(['LIVE',statusText(x.status),x.channel_name,x.account,x.started_at,duration(x.duration_seconds),bytes(x.size_bytes),p166TranslateReason(x.reason||''),x.file_path||'']);
  for(const x of snapshot.vod||[])rows.push(['VOD',p166TranslateStatus(x.state),x.title||'',x.streamer||'',x.started_at||'',x.part_count||'', '',x.message||'',x.output_file||x.vod_url||'']);
  const blob=new Blob(['\uFEFF'+rows.map(row=>row.map(p166rCsv).join(',')).join('\r\n')],{type:'text/csv;charset=utf-8'});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download=`stream-archive-history-${new Date().toISOString().slice(0,10)}.csv`;document.body.appendChild(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),0);
}
function p166rBindButton(id,handler){
  const button=document.getElementById(id);if(!button)return;
  button.addEventListener('click',event=>{event.preventDefault();event.stopImmediatePropagation();Promise.resolve(handler()).catch(e=>alert(e.message))},true);
}
function p166rInstallHistorySearch(){
  const status=document.getElementById('p8HistoryStatus');if(status)status.placeholder='예: 완료 / 실패 / 녹화중 / 대기';
  p166rBindButton('p8ApplyHistory',p166rLoadHistory);p166rBindButton('p8ClearHistory',p166rClearHistory);p166rBindButton('p8ExportHistory',p166rExportHistory);
  ['p8HistoryQ','p8HistoryStatus','p8HistoryFrom','p8HistoryTo'].forEach(id=>document.getElementById(id)?.addEventListener('keydown',event=>{if(event.key==='Enter'){event.preventDefault();p166rLoadHistory()}}));
  p166rLoadHistory();
}
function p166rDriveLabel(path){
  const value=String(path||'');const win=value.match(/^([A-Za-z]:\\)/);if(win)return win[1];const unix=value.match(/^(\/[^/]+)?/);return unix?.[0]||'저장소';
}
function p166rStorageClass(status){return status==='OK'?'ok':(status==='WARN'?'warn':'bad')}
async function p166rLoadStorage(){
  const values=document.getElementById('p166StorageValues');if(!values)return;
  try{
    const data=await api('/api/storage');values.replaceChildren();
    for(const volume of data?.volumes||[]){const item=document.createElement('span');item.className=`p166-storage-volume ${p166rStorageClass(volume.status)}`;if(volume.error){item.textContent='저장공간 확인 실패';item.title=volume.error}else{const drive=p166rDriveLabel(volume.probe_path);item.innerHTML=`<b>${esc(drive)}</b><span>${esc(bytes(volume.free_bytes))} 남음 · 사용 ${Number(volume.used_percent||0).toFixed(1)}%</span>`}values.appendChild(item)}
    if(!values.children.length){const item=document.createElement('span');item.className='p166-storage-volume bad';item.textContent='저장공간 정보를 확인할 수 없습니다.';values.appendChild(item)}
  }catch(e){values.innerHTML='<span class="p166-storage-volume bad">저장공간 조회 실패</span>'}
}
function p166rInstallDashboardStorage(){
  const old=document.querySelector('.p135-storage-card');if(old){old.hidden=true;old.setAttribute('aria-hidden','true')}
  const card=document.querySelector('.p135-watcher-card');const summary=card?.querySelector('.summary');const actions=card?.querySelector('.p135-action-row');if(!card||!summary||!actions)return;
  let strip=document.getElementById('p166StorageStrip');if(!strip){strip=document.createElement('div');strip.id='p166StorageStrip';strip.className='p166-storage-strip';strip.innerHTML='<span class="p166-storage-strip-label">저장 공간</span><div id="p166StorageValues" class="p166-storage-strip-values"><span class="p166-storage-volume">확인 중</span></div>';actions.insertAdjacentElement('beforebegin',strip)}
  p166rLoadStorage();document.querySelector('[data-app-tab="dashboard"]')?.addEventListener('click',()=>setTimeout(p166rLoadStorage,0));setInterval(()=>{if(!document.hidden)p166rLoadStorage()},30000);
}
function p166rFixDiagnostics(){document.getElementById('diagDb')?.closest('p')?.classList.add('p166-diag-line')}
function p166rInit(){p166rFixDiagnostics();p166rInstallDashboardStorage();p166rInstallHistorySearch()}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',p166rInit,{once:true});else p166rInit();
})();
