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