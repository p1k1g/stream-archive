(()=>{
'use strict';
labels.QUEUED='대기';labels.RUNNING='진행중';labels.STARTING='시작중';labels.CANCELLING='취소중';
labels.INTERRUPTED='중단됨';

let p13QueueSeen=false;
const p13QueueStates=new Map();
let p13LiveSeen=false;
const p13LiveStates=new Map();

function p13NotifyEnabled(){return localStorage.getItem('soopBrowserNotify')==='Y'&&'Notification'in window&&Notification.permission==='granted'}
function p13Notify(title,body){if(!p13NotifyEnabled())return;try{new Notification(title,{body,tag:'soop-recorder'})}catch{}}
function p13UpdateNotifyButton(){const b=$('vodNotify');if(!b)return;const supported='Notification'in window;if(!supported){b.textContent='브라우저 알림 미지원';b.disabled=true;return}const on=p13NotifyEnabled();b.textContent=on?'브라우저 알림 켜짐':'브라우저 알림 켜기';b.classList.toggle('secondary',on)}
async function p13ToggleNotify(){if(!('Notification'in window)){alert('이 브라우저는 알림 API를 지원하지 않습니다.');return}if(p13NotifyEnabled()){localStorage.setItem('soopBrowserNotify','N');p13UpdateNotifyButton();toast('브라우저 알림 끔');return}const permission=await Notification.requestPermission();if(permission==='granted'){localStorage.setItem('soopBrowserNotify','Y');toast('브라우저 알림 켬')}else{localStorage.setItem('soopBrowserNotify','N');alert('브라우저에서 알림 권한을 허용해야 합니다.')}p13UpdateNotifyButton()}

function p13TrackLive(w){const channels=w?.channels||[];if(!p13LiveSeen){channels.forEach(c=>p13LiveStates.set(c.account,c.status));p13LiveSeen=true;return}const next=new Map();for(const c of channels){const prev=p13LiveStates.get(c.account);next.set(c.account,c.status);if(prev&&prev!=='RECORDING'&&c.status==='RECORDING')p13Notify('LIVE 녹화 시작',`${c.name} (${c.account})`);if(prev==='RECORDING'&&c.status!=='RECORDING')p13Notify('LIVE 녹화 종료',`${c.name} · ${statusText(c.status)}`)}p13LiveStates.clear();next.forEach((v,k)=>p13LiveStates.set(k,v))}
const p13BaseRenderStatus=renderStatus;
renderStatus=function(d){p13BaseRenderStatus(d);p13TrackLive(d?.watcher)};

function p13QueueControls(item){if(['RUNNING','STARTING','CANCELLING','QUEUED'].includes(item.state))return `<button class="mini danger p13-cancel" data-id="${esc(item.id)}">취소</button>`;const retry=['FAILED','CANCELLED','INTERRUPTED'].includes(item.state)?`<button class="mini p13-retry" data-id="${esc(item.id)}">재시도</button>`:'';return `${retry}<button class="mini danger p13-remove" data-id="${esc(item.id)}">삭제</button>`}
function p13QueueRow(item){const tr=document.createElement('tr');const label=[item.title,item.streamer].filter(Boolean).map(esc).join('<br>')||`<span class="mono">${esc(item.vod_url)}</span>`;const progress=item.state==='RUNNING'?`${Number(item.percent||0).toFixed(1)}% · ${item.current_part||0}/${item.part_count||0}`:'-';const result=[item.message,item.output_file].filter(Boolean).map(esc).join('<br>')||'-';const klass=item.state==='COMPLETED'?'ok':(['FAILED','INTERRUPTED'].includes(item.state)?'bad':(['QUEUED','CANCELLED'].includes(item.state)?'muted':'warn'));tr.innerHTML=`<td class="${klass}"><b>${esc(statusText(item.state))}</b></td><td class="smallcell">${label}</td><td>${item.attempts||0}</td><td>${esc(progress)}</td><td class="smallcell">${result}</td><td>${p13QueueControls(item)}</td>`;tr.querySelector('.p13-cancel')?.addEventListener('click',()=>p13QueueAction(item.id,'cancel'));tr.querySelector('.p13-retry')?.addEventListener('click',()=>p13QueueAction(item.id,'retry'));tr.querySelector('.p13-remove')?.addEventListener('click',()=>p13Remove(item.id));return tr}
function p13TrackQueue(snapshot){const items=snapshot?.items||[];if(!p13QueueSeen){items.forEach(i=>p13QueueStates.set(i.id,i.state));p13QueueSeen=true;return}const next=new Map();for(const item of items){const prev=p13QueueStates.get(item.id);next.set(item.id,item.state);if(prev&&prev!==item.state&&item.state==='COMPLETED')p13Notify('VOD 다운로드 완료',item.title||item.vod_url);if(prev&&prev!==item.state&&item.state==='FAILED')p13Notify('VOD 다운로드 실패',item.title||item.vod_url)}p13QueueStates.clear();next.forEach((v,k)=>p13QueueStates.set(k,v))}
function p13RenderQueue(snapshot){snapshot=snapshot||{items:[],queued_count:0};$('p13QueueWaiting').textContent=snapshot.queued_count??0;$('p13QueueActive').textContent=snapshot.active_id?'1':'0';const body=$('p13QueueRows');if(!body)return;body.replaceChildren();(snapshot.items||[]).forEach(item=>body.appendChild(p13QueueRow(item)));if(!(snapshot.items||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="6" class="muted">VOD 다운로드 큐가 비어 있습니다.</td>';body.appendChild(tr)}p13TrackQueue(snapshot)}
async function p13LoadQueue(){try{p13RenderQueue(await api('/api/vod/queue'))}catch(e){console.warn('VOD queue refresh failed',e)}}
async function p13Enqueue(){try{const req={...vodBase(),parts:parseParts($('vodParts').value),quality:$('vodQuality').value||'best',merge:$('vodMerge').value==='Y'};await api('/api/vod/queue',{method:'POST',body:JSON.stringify(req)});toast('VOD 다운로드 큐에 추가');await p13LoadQueue()}catch(e){alert(e.message)}}
async function p13QueueAction(id,action){try{await api(`/api/vod/queue/${encodeURIComponent(id)}/${action}`,{method:'POST'});toast(action==='retry'?'재시도 대기열에 추가':'취소 요청');await p13LoadQueue()}catch(e){alert(e.message)}}
async function p13Remove(id){if(!confirm('이 큐 기록을 삭제할까요? 다운로드된 파일은 삭제하지 않습니다.'))return;try{await api(`/api/vod/queue/${encodeURIComponent(id)}`,{method:'DELETE'});toast('큐 기록 삭제');await p13LoadQueue()}catch(e){alert(e.message)}}

const p13BaseRealtime=applyRealtimeSnapshot;
applyRealtimeSnapshot=function(data){p13BaseRealtime(data);if(data?.queue)p13RenderQueue(data.queue)};

const download=$('vodDownload');if(download){download.textContent='큐에 추가';download.onclick=p13Enqueue}
const cancel=$('vodCancel');if(cancel)cancel.textContent='현재 작업 취소';
$('p13QueueRefresh')?.addEventListener('click',p13LoadQueue);
$('vodNotify')?.addEventListener('click',p13ToggleNotify);
p13UpdateNotifyButton();
p13LoadQueue();
setInterval(()=>{if(!realtimeConnected)p13LoadQueue()},3000);
})();
