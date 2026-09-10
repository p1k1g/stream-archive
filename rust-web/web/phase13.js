(()=>{
'use strict';
labels.QUEUED='대기';labels.RUNNING='진행중';labels.STARTING='시작중';labels.CANCELLING='취소중';
labels.INTERRUPTED='중단됨';

const p135ThemeKey='streamArchiveTheme';
const p135ThemeOrder=['system','light','dark'];
let p135ThemeMedia=null;
function p135StoredTheme(){const v=localStorage.getItem(p135ThemeKey);return p135ThemeOrder.includes(v)?v:'system'}
function p135ResolvedTheme(pref=p135StoredTheme()){return pref==='system'?(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):pref}
function p135ThemeLabel(pref=p135StoredTheme()){return pref==='dark'?'🌙 다크':pref==='light'?'☀️ 라이트':'◐ 시스템'}
function p135ApplyTheme(pref=p135StoredTheme(),persist=false){if(!p135ThemeOrder.includes(pref))pref='system';if(persist)localStorage.setItem(p135ThemeKey,pref);document.documentElement.dataset.theme=p135ResolvedTheme(pref);document.documentElement.dataset.themePreference=pref;const btn=document.getElementById('p135ThemeToggle');if(btn){btn.textContent=p135ThemeLabel(pref);btn.title=`테마: ${pref==='system'?'시스템 설정 사용':pref==='light'?'라이트':'다크'}`;btn.setAttribute('aria-label',btn.title)}const select=document.getElementById('p135ThemeSelect');if(select)select.value=pref}
function p135CycleTheme(){const current=p135StoredTheme();const next=p135ThemeOrder[(p135ThemeOrder.indexOf(current)+1)%p135ThemeOrder.length];p135ApplyTheme(next,true);toast(`테마: ${next==='system'?'시스템':next==='light'?'라이트':'다크'}`)}
function p135InstallThemeUi(){p135ApplyTheme();const header=document.querySelector('.p135-workspace > header');if(header&&!document.getElementById('p135ThemeToggle')){const btn=document.createElement('button');btn.id='p135ThemeToggle';btn.type='button';btn.className='secondary p135-theme-toggle';btn.addEventListener('click',p135CycleTheme);const token=document.getElementById('tokenBtn');if(token&&token.parentElement===header)token.insertAdjacentElement('afterend',btn);else header.appendChild(btn)}const settingsPage=document.querySelector('[data-tab-page="settings"] section');const settingsTabs=document.getElementById('settingsTabs');if(settingsPage&&settingsTabs&&!document.getElementById('p135ThemePreference')){const box=document.createElement('div');box.id='p135ThemePreference';box.className='p135-theme-preference';box.innerHTML='<div><strong>화면 테마</strong><small>이 브라우저에만 저장됩니다. 시스템을 선택하면 Windows/브라우저 테마를 자동으로 따릅니다.</small></div><label>테마<select id="p135ThemeSelect"><option value="system">시스템 설정</option><option value="light">라이트</option><option value="dark">다크</option></select></label>';settingsTabs.insertAdjacentElement('afterend',box);box.querySelector('#p135ThemeSelect')?.addEventListener('change',e=>p135ApplyTheme(e.target.value,true))}p135ApplyTheme();p135ThemeMedia=matchMedia('(prefers-color-scheme: dark)');const sync=()=>{if(p135StoredTheme()==='system')p135ApplyTheme('system')};if(p135ThemeMedia.addEventListener)p135ThemeMedia.addEventListener('change',sync);else p135ThemeMedia.addListener?.(sync)}
function p135InstallHeaderActions(){const header=document.querySelector('.p135-workspace > header');const auth=header?.querySelector('.p10-authbar');const token=document.getElementById('tokenBtn');const theme=document.getElementById('p135ThemeToggle');if(!header||!auth||!token||!theme)return;let group=header.querySelector('.p135-header-actions');if(!group){group=document.createElement('div');group.className='p135-header-actions';header.appendChild(group)}group.append(auth,token,theme)}
function p135InstallResponsiveTuning(){if(document.getElementById('p135ResponsiveTuning'))return;const style=document.createElement('style');style.id='p135ResponsiveTuning';style.textContent=`
.p135-header-actions{display:flex;align-items:center;justify-content:flex-end;gap:clamp(8px,.8vw,12px);margin-left:auto;min-width:0}
.p135-header-actions .p10-authbar{margin-left:0!important;gap:clamp(8px,.8vw,12px)!important;flex-wrap:nowrap!important;width:auto!important}
.p135-header-actions .p10-authbar button,.p135-header-actions>#tokenBtn,.p135-header-actions>.p135-theme-toggle{width:clamp(104px,7.5vw,118px)!important;min-width:0!important;height:42px!important;margin:0!important;flex:0 0 auto!important;padding-inline:clamp(8px,.8vw,12px)!important}
.p135-vod-input{padding:clamp(16px,1.15vw,20px)}
.p135-vod-input .grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));column-gap:clamp(10px,1vw,16px);row-gap:clamp(10px,.85vw,14px)}
.p135-vod-input .grid>label{min-width:0;margin:0}
.p135-vod-input .grid>label:first-child,.p135-vod-input .grid>label:nth-child(2),.p135-vod-input .grid>label:last-child{grid-column:1/-1}
.p135-vod-input .grid input,.p135-vod-input .grid select{min-height:40px}
.p135-vod-input .summary{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:clamp(8px,.85vw,12px);margin-top:clamp(10px,.9vw,14px)}
.p135-vod-input .summary>span{min-width:0;margin:0}
@media(max-width:900px){
  header{display:grid!important;grid-template-columns:minmax(205px,230px) minmax(0,1fr);align-items:center!important;gap:clamp(8px,1.2vw,14px)!important;padding:clamp(10px,1.8vw,14px) clamp(12px,2vw,20px)!important}
  header>div:first-child{min-width:0}
  .p135-header-actions{display:grid;grid-template-columns:max-content repeat(5,minmax(0,1fr));gap:clamp(4px,.7vw,8px);width:100%;margin-left:0;min-width:0}
  .p135-header-actions .p10-authbar{display:contents!important}
  .p135-header-actions .p10-user{font-size:clamp(10px,1.35vw,12px)!important;white-space:nowrap;align-self:center;margin:0}
  .p135-header-actions .p10-authbar button,.p135-header-actions>#tokenBtn,.p135-header-actions>.p135-theme-toggle{width:100%!important;min-width:0!important;height:40px!important;padding-inline:clamp(4px,.8vw,8px)!important;font-size:clamp(10px,1.3vw,12px)!important;order:initial!important}
  .table{max-width:100%;overflow-x:auto;-webkit-overflow-scrolling:touch;overscroll-behavior-inline:contain}
  .table table{table-layout:auto}
  .table th{white-space:nowrap;word-break:normal}
  .table td{word-break:normal}
  .p135-storage-card table{min-width:700px}
  .p135-storage-card th:nth-child(1),.p135-storage-card td:nth-child(1){width:72px;min-width:72px;white-space:nowrap}
  .p135-storage-card th:nth-child(2),.p135-storage-card td:nth-child(2){width:140px;min-width:140px;white-space:nowrap}
  .p135-storage-card th:nth-child(3),.p135-storage-card td:nth-child(3){min-width:320px;white-space:nowrap;word-break:normal;overflow-wrap:normal}
  .p135-storage-card th:nth-child(4),.p135-storage-card td:nth-child(4){width:160px;min-width:160px;white-space:nowrap}
  .p135-runtime-card table{min-width:760px}
  .p135-runtime-card th:first-child,.p135-runtime-card td:first-child{min-width:76px;white-space:nowrap}
  .p135-vod-queue table{min-width:780px}
  .p135-vod-queue #p13QueueRows td:nth-child(1){min-width:78px;white-space:nowrap}
  .p135-vod-queue #p13QueueRows td:nth-child(2){min-width:250px}
  .p135-vod-queue #p13QueueRows td:nth-child(3){min-width:64px;white-space:nowrap}
  .p135-vod-queue #p13QueueRows td:nth-child(4){min-width:110px;white-space:nowrap}
  .p135-vod-input,.p135-vod-queue{padding:clamp(14px,2vw,18px)}
  .p135-vod-input .grid{grid-template-columns:repeat(2,minmax(0,1fr));gap:clamp(10px,1.4vw,14px)}
  .p135-vod-input .grid>label:first-child,.p135-vod-input .grid>label:nth-child(2),.p135-vod-input .grid>label:last-child{grid-column:1/-1}
  .p135-vod-input .summary,.p135-vod-queue .summary{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:clamp(8px,1.2vw,12px)}
  .p135-vod-input .summary>span,.p135-vod-queue .summary>span{margin:0;min-width:0}
  .p135-vod-input .summary>span:nth-child(3),.p135-vod-queue .summary>span:nth-child(3){grid-column:1/-1}
}
@media(max-width:620px){
  header{display:flex!important;flex-direction:column;align-items:stretch!important;padding:clamp(11px,3vw,16px) clamp(10px,3.5vw,17px)!important;gap:clamp(8px,2vw,12px)!important}
  header>div:first-child{width:100%}
  .p135-header-actions{display:grid;grid-template-columns:repeat(6,minmax(0,1fr));gap:clamp(6px,2vw,10px);width:100%}
  .p135-header-actions .p10-user{grid-column:1/-1}
  .p135-header-actions .p10-authbar button{grid-column:span 2;width:100%!important}
  .p135-header-actions>#tokenBtn,.p135-header-actions>.p135-theme-toggle{grid-column:span 3;width:100%!important}
  .p135-header-actions .p10-authbar button,.p135-header-actions>#tokenBtn,.p135-header-actions>.p135-theme-toggle{height:clamp(38px,10vw,42px)!important;padding-inline:clamp(4px,2vw,10px)!important;font-size:clamp(10px,3vw,12px)!important}
  main{padding:clamp(14px,4vw,20px) clamp(10px,3.5vw,16px) clamp(28px,7vw,36px)!important}
  .tab-page{gap:clamp(12px,3.5vw,18px)!important}
  section{padding:clamp(13px,3.8vw,16px)!important}
  .p135-vod-input .title,.p135-vod-queue .title{flex-direction:column;align-items:stretch;gap:clamp(8px,2.5vw,12px)}
  .p135-vod-input .title>div:last-child{width:100%;display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:clamp(6px,2vw,10px)}
  .p135-vod-queue .title>div:last-child{width:100%;display:grid;grid-template-columns:minmax(0,1.55fr) minmax(96px,1fr);gap:clamp(6px,2vw,10px)}
  .p135-vod-input .title>div:last-child button,.p135-vod-queue .title>div:last-child button{width:100%;min-width:0;margin:0;padding-inline:clamp(5px,2vw,10px);min-height:40px}
  .p135-vod-input .grid{grid-template-columns:1fr}
  .p135-vod-input .grid>label:first-child,.p135-vod-input .grid>label:nth-child(2),.p135-vod-input .grid>label:last-child{grid-column:auto}
  .p135-vod-input .summary,.p135-vod-queue .summary{gap:clamp(7px,2vw,10px)}
  .p135-storage-card table{min-width:650px}
}
@media(max-width:520px){
  .app-tabs{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:clamp(1px,.8vw,3px);overflow:visible;padding-bottom:4px}
  .app-tabs button{width:100%;min-width:0;justify-content:center;gap:clamp(1px,.7vw,3px);padding:7px 1px;font-size:clamp(9px,2.7vw,10px);white-space:nowrap}
  .p135-nav-icon{width:17px;height:17px;min-width:17px;border-radius:5px;font-size:10px}
  .p135-watcher-card .summary{grid-template-columns:repeat(2,minmax(0,1fr));gap:clamp(7px,2vw,9px)}
  .p135-watcher-card .summary>span{min-height:64px;padding:8px 10px}
  .p135-watcher-card .summary>span:first-child{grid-column:1/-1;min-height:66px}
  .p135-watcher-card .summary>span b{font-size:20px}
}
`;document.head.appendChild(style)}

function p13QueueControls(item){if(['RUNNING','STARTING','CANCELLING','QUEUED'].includes(item.state))return `<button class="mini danger p13-cancel" data-id="${esc(item.id)}">취소</button>`;const retry=['FAILED','CANCELLED','INTERRUPTED'].includes(item.state)?`<button class="mini p13-retry" data-id="${esc(item.id)}">재시도</button>`:'';return `${retry}<button class="mini danger p13-remove" data-id="${esc(item.id)}">삭제</button>`}
function p13QueueRow(item){const tr=document.createElement('tr');const label=[item.title,item.streamer].filter(Boolean).map(esc).join('<br>')||`<span class="mono">${esc(item.vod_url)}</span>`;const progress=item.state==='RUNNING'?`${Number(item.percent||0).toFixed(1)}% · ${item.current_part||0}/${item.part_count||0}`:'-';const result=[item.message,item.output_file].filter(Boolean).map(esc).join('<br>')||'-';const klass=item.state==='COMPLETED'?'ok':(['FAILED','INTERRUPTED'].includes(item.state)?'bad':(['QUEUED','CANCELLED'].includes(item.state)?'muted':'warn'));tr.innerHTML=`<td class="${klass}"><b>${esc(statusText(item.state))}</b></td><td class="smallcell">${label}</td><td>${item.attempts||0}</td><td>${esc(progress)}</td><td class="smallcell">${result}</td><td>${p13QueueControls(item)}</td>`;tr.querySelector('.p13-cancel')?.addEventListener('click',()=>p13QueueAction(item.id,'cancel'));tr.querySelector('.p13-retry')?.addEventListener('click',()=>p13QueueAction(item.id,'retry'));tr.querySelector('.p13-remove')?.addEventListener('click',()=>p13Remove(item.id));return tr}
function p13RenderQueue(snapshot){snapshot=snapshot||{items:[],queued_count:0};$('p13QueueWaiting').textContent=snapshot.queued_count??0;$('p13QueueActive').textContent=snapshot.active_id?'1':'0';const body=$('p13QueueRows');if(!body)return;body.replaceChildren();(snapshot.items||[]).forEach(item=>body.appendChild(p13QueueRow(item)));if(!(snapshot.items||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="6" class="muted">VOD 다운로드 큐가 비어 있습니다.</td>';body.appendChild(tr)}window.StreamArchiveState?.publish('queue',snapshot)}
async function p13LoadQueue(){try{p13RenderQueue(await api('/api/vod/queue'))}catch(e){console.warn('VOD queue refresh failed',e)}}
async function p13Enqueue(){try{const req={...vodBase(),parts:parseParts($('vodParts').value),quality:$('vodQuality').value||'best',merge:$('vodMerge').value==='Y'};await api('/api/vod/queue',{method:'POST',body:JSON.stringify(req)});toast('VOD 다운로드 큐에 추가');await p13LoadQueue()}catch(e){alert(e.message)}}
async function p13QueueAction(id,action){try{await api(`/api/vod/queue/${encodeURIComponent(id)}/${action}`,{method:'POST'});toast(action==='retry'?'재시도 대기열에 추가':'취소 요청');await p13LoadQueue()}catch(e){alert(e.message)}}
async function p13Remove(id){if(!confirm('이 큐 기록을 삭제할까요? 다운로드된 파일은 삭제하지 않습니다.'))return;try{await api(`/api/vod/queue/${encodeURIComponent(id)}`,{method:'DELETE'});toast('큐 기록 삭제');await p13LoadQueue()}catch(e){alert(e.message)}}

const download=$('vodDownload');if(download){download.textContent='큐에 추가';download.disabled=false;download.dataset.queueMode='1';download.onclick=p13Enqueue}
const cancel=$('vodCancel');if(cancel)cancel.textContent='현재 작업 취소';
$('p13QueueRefresh')?.addEventListener('click',p13LoadQueue);
p135InstallThemeUi();
p135InstallHeaderActions();
p135InstallResponsiveTuning();
window.StreamArchiveState?.subscribe('snapshot',data=>{if(data?.queue)p13RenderQueue(data.queue)},{replay:true});
p13LoadQueue();
setInterval(()=>{if(!realtimeConnected)p13LoadQueue()},3000);
})();