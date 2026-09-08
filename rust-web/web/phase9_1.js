(()=>{
const folderSettings=new Set(['OUTPUT_DIR','LOG_DIR']);
const fileSettings=new Map([
  ['STREAMLINK_PATH','exe'],
  ['STREAMLINK_FALLBACK','exe'],
  ['YT_DLP_PATH','exe'],
  ['FFMPEG_PATH','exe']
]);
const directLocal=['127.0.0.1','localhost','::1','[::1]'].includes(String(location.hostname||'').toLowerCase());
let refreshQueued=false;

function p91InjectStyle(){
  if(document.getElementById('p91PickerStyle'))return;
  const style=document.createElement('style');
  style.id='p91PickerStyle';
  style.textContent='.p91-picker-control{display:flex;gap:6px;align-items:center}.p91-picker-control input{min-width:0;flex:1}.p91-picker-control .mini{white-space:nowrap;flex:0 0 auto}';
  document.head.appendChild(style);
}

async function p91Pick(input,kind,filter='all'){
  if(!directLocal){
    alert('파일/폴더 선택 창은 127.0.0.1 또는 localhost로 직접 접속한 브라우저에서만 사용할 수 있습니다.\n외부/Caddy 접속에서는 경로를 직접 입력하세요.');
    return;
  }
  const raw=String(input.value||'').trim();
  const initial=raw&&raw.toUpperCase()!=='AUTO'?raw:'';
  const button=input.parentElement?.querySelector('.p91-picker-button');
  if(button){button.disabled=true;button.textContent='선택 중...'}
  try{
    const result=await api('/api/local-picker',{method:'POST',body:JSON.stringify({kind,filter,initial_path:initial})});
    if(result&&!result.cancelled&&result.path){
      input.value=result.path;
      input.dispatchEvent(new Event('input',{bubbles:true}));
      input.dispatchEvent(new Event('change',{bubbles:true}));
      toast(kind==='folder'?'폴더 선택 완료':'파일 선택 완료');
    }
  }catch(e){
    alert('경로 선택 실패: '+e.message);
  }finally{
    if(button){
      button.disabled=!directLocal;
      button.textContent=kind==='folder'?'폴더 선택':'파일 선택';
    }
  }
}

function p91Decorate(input,kind,filter='all'){
  if(!input||input.dataset.p91Picker==='1')return;
  input.dataset.p91Picker='1';
  const parent=input.parentNode;
  if(!parent)return;
  const wrap=document.createElement('div');
  wrap.className='p91-picker-control';
  parent.insertBefore(wrap,input);
  wrap.appendChild(input);
  const button=document.createElement('button');
  button.type='button';
  button.className='mini p91-picker-button';
  button.textContent=kind==='folder'?'폴더 선택':'파일 선택';
  button.disabled=!directLocal;
  button.title=directLocal?'Windows 선택 창 열기':'127.0.0.1 / localhost 직접 접속에서만 사용 가능';
  button.onclick=()=>p91Pick(input,kind,filter);
  wrap.appendChild(button);
}

function p91DecorateSettings(){
  document.querySelectorAll('#settings input[data-key]').forEach(input=>{
    const key=input.dataset.key;
    if(folderSettings.has(key))p91Decorate(input,'folder');
    else if(fileSettings.has(key))p91Decorate(input,'file',fileSettings.get(key));
  });
  document.querySelectorAll('#settings .hint').forEach(note=>{
    if(note.dataset.p91Hint==='1')return;
    if(note.textContent.includes('다음 로컬 picker 단계에서 추가합니다')){
      note.dataset.p91Hint='1';
      note.textContent='직접 입력/AUTO 또는 로컬 선택 버튼을 사용할 수 있습니다. 선택 버튼은 127.0.0.1/localhost 직접 접속에서만 활성화됩니다.';
    }
  });
}

function p91DecorateChannels(){
  document.querySelectorAll('#channels input.od').forEach(input=>p91Decorate(input,'folder'));
}

function p91DecorateVod(){
  p91Decorate(document.getElementById('vodOutput'),'folder');
  p91Decorate(document.getElementById('vodCookieFile'),'file','cookie');
}

function p91Run(){
  refreshQueued=false;
  p91InjectStyle();
  p91DecorateSettings();
  p91DecorateChannels();
  p91DecorateVod();
}

function p91Queue(){
  if(refreshQueued)return;
  refreshQueued=true;
  queueMicrotask(p91Run);
}

const observer=new MutationObserver(p91Queue);
if(document.body)observer.observe(document.body,{childList:true,subtree:true});
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',()=>{
  observer.observe(document.body,{childList:true,subtree:true});
  p91Run();
},{once:true});
else p91Run();
})();
