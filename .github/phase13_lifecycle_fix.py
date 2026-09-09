from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

p='rust-web/src/vod.rs'
s=read(p)
old='''    pub async fn status(&self) -> VodJobStatus {
        let finished = {
            self.runtime
                .lock()
                .await
                .task
                .as_ref()
                .is_some_and(|t| t.is_finished())
        };
        if finished {
            self.runtime.lock().await.task.take();
        }
        self.status.read().await.clone()
    }
'''
new='''    pub async fn status(&self) -> VodJobStatus {
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.task.as_ref().is_some_and(|task| task.is_finished()) {
                runtime.task.take();
            }
        }
        self.status.read().await.clone()
    }
'''
s=rep(s,old,new,'single-lock VOD status reap')
write(p,s)

p='rust-web/src/main.rs'
s=read(p)
old='''async fn api_vod_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let status = state.vod.cancel().await.map_err(internal_error)?;
'''
new='''async fn api_vod_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let status = state.vod.cancel().await.map_err(internal_error)?;
'''
s=rep(s,old,new,'direct VOD cancel lifecycle lock')
write(p,s)

p='rust-web/src/vod_queue.rs'
s=read(p)
old='''pub(crate) async fn api_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
'''
new='''pub(crate) async fn api_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    // Match guarded restore's lifecycle -> config lock order. Holding lifecycle
    // through VodManager::cancel prevents another direct/queued job from starting
    // while the owned yt-dlp/ffmpeg process is still shutting down.
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let _config_guard = state.config_write_lock.lock().await;
    state
'''
s=rep(s,old,new,'queue cancel lifecycle order')
write(p,s)

assert 'let mut runtime = self.runtime.lock().await;' in read('rust-web/src/vod.rs')
assert read('rust-web/src/main.rs').count('let _lifecycle_guard = state.lifecycle_lock.lock().await;') >= 4
assert 'Match guarded restore' in read('rust-web/src/vod_queue.rs')
print('Phase 13 lifecycle hardening applied.')
