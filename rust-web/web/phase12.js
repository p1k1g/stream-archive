(()=>{
function fmtDate(v){if(!v)return'-';try{return new Date(v).toLocaleString()}catch{return v}}
function kindText(v){return({manual:'수동',auto:'자동',pre_restore:'복원 전 안전백업',legacy:'기존 백업'})[v]||v||'-'}
function integrityText(v){return({OK:'정상',NO_METADATA:'메타데이터 없음',HASH_MISMATCH:'해시 불일치',INVALID_SQLITE:'DB 손상'})[v]||v||'-'}
function integrityClass(v){return v==='OK'?'ok':(v==='NO_METADATA'?'warn':'bad')}
async function loadBackups(){
  const body=$('p12BackupRows');if(!body)return;
  try{
    const d=await api('/api/backups');
    $('p12BackupDir').textContent=d.directory||'-';
    const p=d.policy||{};$('p12BackupPolicy').textContent=p.enabled?`자동 · ${p.interval_hours}시간 · 최대 ${p.keep_count||'무제한'}개 · ${p.retention_days||'기간 제한 없음'}일`:'자동 백업 꺼짐';
    body.replaceChildren();
    for(const b of d.backups||[]){
      const tr=document.createElement('tr');
      const canRestore=b.integrity==='OK';
      tr.innerHTML=`<td>${fmtDate(b.created_at)}</td><td>${kindText(b.kind)}</td><td class="mono smallcell">${esc(b.file_name)}</td><td>${bytes(b.size_bytes)}</td><td class="mono smallcell">${esc((b.sha256||'').slice(0,16))}${b.sha256?'…':''}</td><td class="${integrityClass(b.integrity)}"><b>${integrityText(b.integrity)}</b></td><td><button class="mini restore" ${canRestore?'':'disabled'}>복원</button></td>`;
      const btn=tr.querySelector('.restore');if(btn&&!btn.disabled)btn.onclick=()=>restoreBackup(b.file_name);
      body.appendChild(tr);
    }
    if(!(d.backups||[]).length){const tr=document.createElement('tr');tr.innerHTML='<td colspan="7" class="muted">아직 생성된 백업이 없습니다.</td>';body.appendChild(tr)}
  }catch(e){body.innerHTML=`<tr><td colspan="7" class="bad">${esc(e.message)}</td></tr>`}
}
async function createBackup(){
  const btn=$('p12BackupNow');btn.disabled=true;
  try{const b=await api('/api/backups',{method:'POST'});toast(`백업 완료 · ${b.file_name}`);await loadBackups()}catch(e){alert('백업 실패: '+e.message)}finally{btn.disabled=false}
}
async function restoreBackup(file){
  if(!confirm(`${file} 백업으로 SQLite DB를 복원할까요?\n\nWatcher와 VOD가 중지되어 있어야 합니다.\n복원 직전에 현재 DB 안전백업을 자동 생성합니다.\n외부 로그인 세션은 복원 후 모두 종료됩니다.`))return;
  try{
    const d=await api('/api/backups/'+encodeURIComponent(file)+'/restore',{method:'POST'});
    alert(`복원 완료\n안전백업: ${d.safety_backup?.file_name||'-'}\n\n화면을 새로고침합니다.`);
    location.reload();
  }catch(e){alert('복원 실패: '+e.message)}
}
function init(){const refresh=$('p12BackupRefresh'),now=$('p12BackupNow');if(refresh)refresh.onclick=loadBackups;if(now)now.onclick=createBackup}
window.p12LoadBackups=loadBackups;
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init,{once:true});else init();
})();
