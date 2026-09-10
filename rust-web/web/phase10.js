(()=>{
const SENTINEL='__SOOP_SESSION__';
const nativeFetch=window.fetch.bind(window);
let csrf=sessionStorage.getItem('soopCsrf')||'';
let authState=null;
let authPromise=null;
let overlay=null;
let authBar=null;

const previous=sessionStorage.getItem('soopToken')||'';
if(previous&&previous!==SENTINEL&&sessionStorage.getItem('soopRecoveryMode')!=='1'){
  sessionStorage.removeItem('soopToken');
}
if(!sessionStorage.getItem('soopToken'))sessionStorage.setItem('soopToken',SENTINEL);

function isRecoveryMode(){
  const value=sessionStorage.getItem('soopToken')||'';
  return value&&value!==SENTINEL;
}
function setSessionMode(){
  sessionStorage.setItem('soopToken',SENTINEL);
  sessionStorage.removeItem('soopRecoveryMode');
  sessionStorage.removeItem('soopCsrf');
  csrf='';authState=null;authPromise=null;
}
function setRecoveryMode(value){
  const token=String(value||'').trim();
  if(!token)return false;
  sessionStorage.setItem('soopToken',token);
  sessionStorage.setItem('soopRecoveryMode','1');
  sessionStorage.removeItem('soopCsrf');
  csrf='';authState=null;authPromise=null;
  return true;
}
function injectStyle(){
  if(document.getElementById('p10Style'))return;
  const style=document.createElement('style');
  style.id='p10Style';
  style.textContent=`
  .p10-overlay{position:fixed;inset:0;z-index:1000;background:rgba(2,6,12,.82);display:flex;align-items:center;justify-content:center;padding:18px;backdrop-filter:blur(5px)}
  .p10-overlay[hidden]{display:none!important}
  .p10-card{width:min(430px,100%);background:#131920;border:1px solid #35404d;border-radius:14px;padding:22px;box-shadow:0 20px 60px rgba(0,0,0,.45)}
  .p10-card h2{margin:0 0 8px;color:#edf2f7}.p10-card p{color:#9da9b7}.p10-card label{display:flex;flex-direction:column;gap:6px;margin:12px 0;color:#b8c3cf;font-size:13px}
  .p10-actions{display:flex;gap:8px;justify-content:flex-end;flex-wrap:wrap;margin-top:16px}.p10-error{min-height:20px;color:#ff8f9b;margin-top:8px;font-size:13px}
  header{column-gap:clamp(8px,.8vw,12px);padding-left:clamp(18px,2vw,28px);padding-right:clamp(18px,2vw,28px)}
  .p10-authbar{display:flex;gap:clamp(8px,.8vw,12px);align-items:center;flex-wrap:wrap;justify-content:flex-end;margin-left:auto}.p10-user{color:#9fe3b0;font-size:13px;white-space:nowrap}
  .p10-authbar button,#tokenBtn,.p135-theme-toggle{box-sizing:border-box;width:clamp(104px,7.5vw,118px);height:42px;min-width:0;padding:0 clamp(8px,.7vw,12px);font-size:12px;line-height:1;display:inline-flex;align-items:center;justify-content:center;white-space:nowrap;margin:0;flex:0 0 auto}
  #tokenBtn{margin-left:0}
  @media(max-width:700px){header{gap:clamp(6px,2vw,10px);padding-left:clamp(10px,4vw,17px);padding-right:clamp(10px,4vw,17px);flex-wrap:wrap}.p10-authbar{width:100%;display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:clamp(6px,2vw,10px);justify-content:stretch;margin-left:0;order:2}.p10-authbar .p10-user{grid-column:1/-1}.p10-authbar button{width:100%;min-width:0;padding:0 clamp(4px,1.6vw,8px)}#tokenBtn,.p135-theme-toggle{width:auto;min-width:0;max-width:none;flex:1 1 calc((100% - clamp(6px,2vw,10px))/2);padding:0 clamp(6px,2vw,10px)}#tokenBtn{order:3;margin-left:0}.p135-theme-toggle{order:4}}
  `;
  document.head.appendChild(style);
}
function jsonHeaders(extra={}){return {'Content-Type':'application/json',...extra}}
async function nativeJson(path,opt={}){
  const response=await nativeFetch(path,{credentials:'same-origin',...opt});
  const text=await response.text();
  let body=null;if(text){try{body=JSON.parse(text)}catch{body=text}}
  if(!response.ok){const error=new Error(typeof body==='string'?body:(body?.message||JSON.stringify(body)||`HTTP ${response.status}`));error.status=response.status;throw error}
  return body;
}
async function readStatus(){
  const state=await nativeJson('/api/auth/status');
  authState=state;
  if(state?.authenticated&&state.csrf_token){csrf=state.csrf_token;sessionStorage.setItem('soopCsrf',csrf)}
  else{csrf='';sessionStorage.removeItem('soopCsrf')}
  renderAuthBar();
  return state;
}
function makeOverlay(){
  injectStyle();
  if(overlay)return overlay;
  overlay=document.createElement('div');
  overlay.className='p10-overlay';
  overlay.hidden=true;
  document.body.appendChild(overlay);
  return overlay;
}
function errorText(card,message){const e=card.querySelector('.p10-error');if(e)e.textContent=message||''}
function recoveryButton(){
  const b=document.createElement('button');b.type='button';b.textContent='복구 토큰 사용';
  b.onclick=()=>{const value=prompt('SOOP 관리 복구 토큰을 입력하세요.');if(setRecoveryMode(value))location.reload()};
  return b;
}
function showSetup(){
  const root=makeOverlay();root.hidden=false;root.innerHTML=`<div class="p10-card"><h2>관리자 계정 만들기</h2><p>처음 한 번만 사용할 SOOP Recorder 관리 계정을 만듭니다.</p><form><label>아이디<input name="username" autocomplete="username" minlength="3" maxlength="32" required placeholder="admin"></label><label>비밀번호<input name="password" type="password" autocomplete="new-password" minlength="4" maxlength="128" required></label><label>비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="4" maxlength="128" required></label><p class="hint">비밀번호는 4자 이상이어야 합니다. 저장 시 단방향 PBKDF2-SHA256 해시로 보관됩니다.</p><div class="p10-error"></div><div class="p10-actions"><button type="submit">관리자 생성</button></div></form></div>`;
  const card=root.querySelector('.p10-card');card.querySelector('.p10-actions').prepend(recoveryButton());
  return new Promise(resolve=>{card.querySelector('form').onsubmit=async e=>{e.preventDefault();errorText(card,'');const f=new FormData(e.currentTarget);const password=String(f.get('password')||''),confirm=String(f.get('confirm')||'');if(password!==confirm){errorText(card,'비밀번호 확인이 일치하지 않습니다.');return}const submit=card.querySelector('button[type=submit]');submit.disabled=true;try{const result=await nativeJson('/api/auth/setup',{method:'POST',headers:jsonHeaders(),body:JSON.stringify({username:String(f.get('username')||'').trim(),password,password_confirm:confirm})});csrf=result.csrf_token||'';sessionStorage.setItem('soopCsrf',csrf);authState={configured:true,...result};root.hidden=true;renderAuthBar();resolve(authState)}catch(err){errorText(card,err.message)}finally{submit.disabled=false}}});
}
function showLogin(message=''){
  const root=makeOverlay();root.hidden=false;root.innerHTML=`<div class="p10-card"><h2>SOOP Recorder 로그인</h2><p>관리 화면을 사용하려면 로그인하세요.</p><form><label>아이디<input name="username" autocomplete="username" required></label><label>비밀번호<input name="password" type="password" autocomplete="current-password" required></label><div class="p10-error"></div><div class="p10-actions"><button type="submit">로그인</button></div></form></div>`;
  const card=root.querySelector('.p10-card');card.querySelector('.p10-actions').prepend(recoveryButton());errorText(card,message);
  return new Promise(resolve=>{card.querySelector('form').onsubmit=async e=>{e.preventDefault();errorText(card,'');const f=new FormData(e.currentTarget);const submit=card.querySelector('button[type=submit]');submit.disabled=true;try{const result=await nativeJson('/api/auth/login',{method:'POST',headers:jsonHeaders(),body:JSON.stringify({username:String(f.get('username')||'').trim(),password:String(f.get('password')||'')})});csrf=result.csrf_token||'';sessionStorage.setItem('soopCsrf',csrf);authState={configured:true,...result};root.hidden=true;renderAuthBar();resolve(authState)}catch(err){errorText(card,err.status===429?'로그인 실패가 너무 많습니다. 잠시 후 다시 시도하세요.':err.message)}finally{submit.disabled=false}}});
}
async function ensureAuth(force=false){
  if(isRecoveryMode())return {recovery:true};
  if(!force&&authState?.authenticated&&csrf)return authState;
  if(authPromise&&!force)return authPromise;
  authPromise=(async()=>{let state;try{state=await readStatus()}catch(err){throw new Error('인증 상태 확인 실패: '+err.message)}if(state.authenticated)return state;if(!state.configured)return await showSetup();return await showLogin()})();
  try{return await authPromise}finally{authPromise=null}
}
async function sessionRequest(input,init,retry=true){
  await ensureAuth();
  const headers=new Headers(init?.headers||{});headers.delete('Authorization');if(csrf)headers.set('X-CSRF-Token',csrf);
  const response=await nativeFetch(input,{...init,headers,credentials:'same-origin'});
  let authFailure=response.status===401;
  if(response.status===403){try{authFailure=(await response.clone().text()).includes('invalid CSRF token')}catch{}}
  if(authFailure&&retry){authState=null;csrf='';sessionStorage.removeItem('soopCsrf');await ensureAuth(true);return sessionRequest(input,init,false)}
  return response;
}
window.fetch=async function(input,init={}){
  const headers=new Headers(init.headers||{});const auth=headers.get('Authorization')||'';
  if(auth===`Bearer ${SENTINEL}`)return sessionRequest(input,{...init,headers});
  return nativeFetch(input,init);
};
async function logout(all=false){
  try{await ensureAuth();const path=all?'/api/auth/logout-all':'/api/auth/logout';await nativeJson(path,{method:'POST',headers:{'X-CSRF-Token':csrf}})}catch(err){alert('로그아웃 실패: '+err.message);return}authState=null;csrf='';sessionStorage.removeItem('soopCsrf');renderAuthBar();await ensureAuth(true)}
function showPasswordChange(){
  const root=makeOverlay();root.hidden=false;root.innerHTML=`<div class="p10-card"><h2>비밀번호 변경</h2><form><label>현재 비밀번호<input name="current" type="password" autocomplete="current-password" required></label><label>새 비밀번호<input name="next" type="password" autocomplete="new-password" minlength="4" maxlength="128" required></label><label>새 비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="4" maxlength="128" required></label><div class="p10-error"></div><div class="p10-actions"><button type="button" class="cancel">취소</button><button type="submit">변경</button></div></form></div>`;
  const card=root.querySelector('.p10-card');card.querySelector('.cancel').onclick=()=>root.hidden=true;card.querySelector('form').onsubmit=async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const next=String(f.get('next')||''),confirm=String(f.get('confirm')||'');if(next!==confirm){errorText(card,'새 비밀번호 확인이 일치하지 않습니다.');return}const submit=card.querySelector('button[type=submit]');submit.disabled=true;try{const result=await nativeJson('/api/auth/change-password',{method:'POST',headers:jsonHeaders({'X-CSRF-Token':csrf}),body:JSON.stringify({current_password:String(f.get('current')||''),new_password:next,new_password_confirm:confirm})});csrf=result.csrf_token||'';sessionStorage.setItem('soopCsrf',csrf);authState={configured:true,...result};root.hidden=true;renderAuthBar();alert('비밀번호를 변경했습니다. 다른 브라우저의 기존 세션은 모두 로그아웃되었습니다.')}catch(err){errorText(card,err.message)}finally{submit.disabled=false}};
}
function renderAuthBar(){
  if(!document.body)return;injectStyle();const header=document.querySelector('header');if(!header)return;
  if(!authBar){authBar=document.createElement('div');authBar.className='p10-authbar';const tokenButton=document.getElementById('tokenBtn');if(tokenButton&&tokenButton.parentElement===header)header.insertBefore(authBar,tokenButton);else header.appendChild(authBar)}
  authBar.replaceChildren();
  if(isRecoveryMode()){
    const label=document.createElement('span');label.className='p10-user';label.textContent='복구 토큰 모드';const back=document.createElement('button');back.type='button';back.textContent='ID/PW 사용';back.onclick=()=>{setSessionMode();location.reload()};authBar.append(label,back);return;
  }
  if(authState?.authenticated){
    if(authState.local_bypass){const label=document.createElement('span');label.className='p10-user';label.textContent='로컬 접속 · 로그인 생략';authBar.append(label);return}
    const label=document.createElement('span');label.className='p10-user';label.textContent=`${authState.username} 로그인`;const pw=document.createElement('button');pw.type='button';pw.textContent='비밀번호 변경';pw.onclick=showPasswordChange;const out=document.createElement('button');out.type='button';out.textContent='로그아웃';out.onclick=()=>logout(false);const all=document.createElement('button');all.type='button';all.textContent='전체 로그아웃';all.onclick=()=>{if(confirm('모든 브라우저의 로그인 세션을 종료할까요?'))logout(true)};authBar.append(label,pw,out,all);
  }else{const label=document.createElement('span');label.className='muted';label.textContent='로그인 필요';authBar.append(label)}
}
function installRecoveryButton(){
  const button=document.getElementById('tokenBtn');if(!button)return;button.textContent='복구 토큰';button.title='ID/PW 로그인이 불가능할 때만 관리 Bearer 토큰을 사용합니다.';button.addEventListener('click',e=>{e.preventDefault();e.stopImmediatePropagation();const value=prompt('SOOP 관리 복구 토큰을 입력하세요.');if(setRecoveryMode(value))location.reload()},true);
}
function init(){injectStyle();installRecoveryButton();renderAuthBar();if(!isRecoveryMode())ensureAuth().catch(err=>{makeOverlay().hidden=false;showLogin(err.message)})}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init,{once:true});else init();
})();
