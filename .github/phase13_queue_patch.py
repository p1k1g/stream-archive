from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# ---------------- model.rs ----------------
p='rust-web/src/model.rs'
s=read(p)
s=rep(s,
'#[derive(Debug, Clone, Deserialize)]\npub struct VodDownloadRequest {',
'#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct VodDownloadRequest {',
'VodDownloadRequest serialize')
append=r'''

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodQueueItem {
    pub id: String,
    pub vod_url: String,
    pub output_directory: String,
    pub state: String,
    pub attempts: u32,
    pub message: String,
    pub title: String,
    pub streamer: String,
    pub current_part: usize,
    pub part_count: usize,
    pub percent: f64,
    pub output_file: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodQueueSnapshot {
    pub active_id: Option<String>,
    pub queued_count: usize,
    pub items: Vec<VodQueueItem>,
}
'''
if 'pub struct VodQueueSnapshot' not in s:
    s += append
write(p,s)

# ---------------- vod.rs: validate before enqueue ----------------
p='rust-web/src/vod.rs'
s=read(p)
old=r'''async fn run_download(
    backend: &Path,
    req: VodDownloadRequest,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    if !req.quality.is_empty()
        && !Regex::new(r"^best(?:\[height<=\d+\])?$")
            .unwrap()
            .is_match(&req.quality)
    {
        bail!("지원하지 않는 VOD 화질 선택입니다.");
    }
    if req.output_directory.trim().is_empty() {
        bail!("VOD 출력 폴더가 비어 있습니다.");
    }
'''
new=r'''pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    if !req.quality.is_empty()
        && !Regex::new(r"^best(?:\[height<=\d+\])?$")
            .unwrap()
            .is_match(&req.quality)
    {
        bail!("지원하지 않는 VOD 화질 선택입니다.");
    }
    if req.output_directory.trim().is_empty() {
        bail!("VOD 출력 폴더가 비어 있습니다.");
    }
    Ok(())
}

async fn run_download(
    backend: &Path,
    req: VodDownloadRequest,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    validate_download_request(&req)?;
'''
s=rep(s,old,new,'download validation extraction')
write(p,s)

# ---------------- store.rs schema + startup recovery ----------------
p='rust-web/src/store.rs'
s=read(p)
queue_schema=r'''

            CREATE TABLE IF NOT EXISTS vod_queue (
                id TEXT PRIMARY KEY,
                request_json TEXT NOT NULL,
                vod_url TEXT NOT NULL,
                output_directory TEXT NOT NULL,
                state TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                message TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                streamer TEXT NOT NULL DEFAULT '',
                output_file TEXT,
                created_at TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS ix_vod_queue_state_created
                ON vod_queue(state, created_at);
'''
needle='''            CREATE INDEX IF NOT EXISTS ix_vod_jobs_started
                ON vod_jobs(started_at DESC, updated_at DESC);
'''
if s.count(needle) != 2:
    raise SystemExit(f'expected two store schema anchors, found {s.count(needle)}')
s=s.replace(needle, needle+queue_schema)
old='''        conn.execute(
            "UPDATE vod_jobs SET state='INTERRUPTED', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','ANALYZING','DOWNLOADING','REFRESHING','MERGING','CANCELLING')",
            params![now],
        )?;
        Ok(())
'''
new='''        conn.execute(
            "UPDATE vod_jobs SET state='INTERRUPTED', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','ANALYZING','DOWNLOADING','REFRESHING','MERGING','CANCELLING')",
            params![now],
        )?;
        conn.execute(
            "UPDATE vod_queue SET state='INTERRUPTED', message='서버 재시작으로 중단됨 · 재시도 가능', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','RUNNING','CANCELLING')",
            params![now],
        )?;
        Ok(())
'''
s=rep(s,old,new,'queue startup recovery')
write(p,s)

# ---------------- new vod_queue.rs ----------------
write('rust-web/src/vod_queue.rs', r'''use crate::{
    ApiResult, AppState, authorize, internal_error,
    backend::LogBuffer,
    model::{VodDownloadRequest, VodJobStatus, VodQueueItem, VodQueueSnapshot},
    primary_config::apply_vod_tool_defaults,
    store::Store,
    vod::{VodManager, validate_download_request},
};
use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

const QUEUE_LIMIT: usize = 100;
const WORKER_IDLE: Duration = Duration::from_millis(750);
const STATUS_POLL: Duration = Duration::from_millis(500);

#[derive(Debug)]
struct ClaimedItem {
    id: String,
    request_json: String,
}

#[derive(Clone)]
pub struct VodQueueManager {
    store: Store,
    vod: Arc<VodManager>,
    logs: LogBuffer,
    lifecycle_lock: Arc<Mutex<()>>,
    active_id: Arc<Mutex<Option<String>>>,
}

impl VodQueueManager {
    pub fn new(
        store: Store,
        vod: Arc<VodManager>,
        logs: LogBuffer,
        lifecycle_lock: Arc<Mutex<()>>,
    ) -> Result<Self> {
        let manager = Self {
            store,
            vod,
            logs,
            lifecycle_lock,
            active_id: Arc::new(Mutex::new(None)),
        };
        manager.mark_interrupted()?;
        Ok(manager)
    }

    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move { self.worker_loop().await });
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(self.store.path()).with_context(|| {
            format!("failed to open queue database {}", self.store.path().display())
        })?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(conn)
    }

    fn mark_interrupted(&self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            "UPDATE vod_queue SET state='INTERRUPTED', message='서버 재시작으로 중단됨 · 재시도 가능', finished_at=COALESCE(finished_at, ?1), updated_at=?1 WHERE state IN ('STARTING','RUNNING','CANCELLING')",
            params![now],
        )?;
        Ok(())
    }

    pub async fn enqueue(&self, req: VodDownloadRequest) -> Result<VodQueueItem> {
        validate_download_request(&req)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let request_json = serde_json::to_string(&req)?;
        self.conn()?.execute(
            r#"INSERT INTO vod_queue(id,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
               VALUES(?1,?2,?3,?4,'QUEUED',0,'대기 중',?5,?5)"#,
            params![id, request_json, req.vod_url, req.output_directory, now],
        )?;
        self.logs
            .push(format!("[VOD_QUEUE] queued id={id} url={}", req.vod_url))
            .await;
        self.item(&id)?.context("queued item disappeared")
    }

    pub async fn snapshot(&self) -> Result<VodQueueSnapshot> {
        let active_id = self.active_id.lock().await.clone();
        let current = self.vod.status().await;
        let mut items = self.list_items()?;
        if let Some(active) = active_id.as_deref() {
            if let Some(item) = items.iter_mut().find(|item| item.id == active) {
                apply_runtime_status(item, &current);
            }
        }
        let queued_count = items.iter().filter(|item| item.state == "QUEUED").count();
        Ok(VodQueueSnapshot {
            active_id,
            queued_count,
            items,
        })
    }

    pub async fn has_pending_or_active(&self) -> Result<bool> {
        if self.active_id.lock().await.is_some() {
            return Ok(true);
        }
        let count: i64 = self.conn()?.query_row(
            "SELECT COUNT(*) FROM vod_queue WHERE state IN ('QUEUED','STARTING','RUNNING','CANCELLING')",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub async fn cancel(&self, id: &str) -> Result<()> {
        if self.active_id.lock().await.as_deref() == Some(id) {
            let now = Utc::now().to_rfc3339();
            self.conn()?.execute(
                "UPDATE vod_queue SET state='CANCELLING', message='취소 요청 중…', updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            let status = self.vod.cancel().await?;
            self.finish_item(id, &status)?;
            self.logs.push(format!("[VOD_QUEUE] cancelled id={id}")).await;
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let changed = self.conn()?.execute(
            "UPDATE vod_queue SET state='CANCELLED', message='대기열에서 취소됨', finished_at=?2, updated_at=?2 WHERE id=?1 AND state='QUEUED'",
            params![id, now],
        )?;
        if changed == 0 {
            bail!("취소할 수 있는 대기 작업이 아닙니다.");
        }
        self.logs.push(format!("[VOD_QUEUE] cancelled queued id={id}")).await;
        Ok(())
    }

    pub async fn retry(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let changed = self.conn()?.execute(
            "UPDATE vod_queue SET state='QUEUED', message='재시도 대기 중', output_file=NULL, started_at=NULL, finished_at=NULL, updated_at=?2 WHERE id=?1 AND state IN ('FAILED','CANCELLED','INTERRUPTED')",
            params![id, now],
        )?;
        if changed == 0 {
            bail!("재시도할 수 있는 작업이 아닙니다.");
        }
        self.logs.push(format!("[VOD_QUEUE] retry queued id={id}")).await;
        Ok(())
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        if self.active_id.lock().await.as_deref() == Some(id) {
            bail!("실행 중인 큐 작업은 삭제할 수 없습니다.");
        }
        let changed = self.conn()?.execute(
            "DELETE FROM vod_queue WHERE id=?1 AND state NOT IN ('QUEUED','STARTING','RUNNING','CANCELLING')",
            params![id],
        )?;
        if changed == 0 {
            bail!("대기/실행 중인 작업은 먼저 취소하세요.");
        }
        self.logs.push(format!("[VOD_QUEUE] removed id={id}")).await;
        Ok(())
    }

    async fn worker_loop(self: Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            if self.active_id.lock().await.is_some() || self.vod.status().await.running {
                tokio::time::sleep(WORKER_IDLE).await;
                continue;
            }

            let lifecycle_guard = self.lifecycle_lock.lock().await;
            if self.vod.status().await.running {
                drop(lifecycle_guard);
                tokio::time::sleep(WORKER_IDLE).await;
                continue;
            }
            let claimed = match self.claim_next() {
                Ok(Some(item)) => item,
                Ok(None) => {
                    drop(lifecycle_guard);
                    tokio::time::sleep(WORKER_IDLE).await;
                    continue;
                }
                Err(err) => {
                    drop(lifecycle_guard);
                    self.logs
                        .push(format!("[VOD_QUEUE:WARN] claim failed: {err:#}"))
                        .await;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            *self.active_id.lock().await = Some(claimed.id.clone());
            let req: VodDownloadRequest = match serde_json::from_str(&claimed.request_json) {
                Ok(req) => req,
                Err(err) => {
                    let _ = self.fail_item(&claimed.id, &format!("저장된 요청 해석 실패: {err}"));
                    *self.active_id.lock().await = None;
                    drop(lifecycle_guard);
                    continue;
                }
            };
            let started = self.vod.download(req).await;
            drop(lifecycle_guard);
            let started = match started {
                Ok(status) => status,
                Err(err) => {
                    let _ = self.fail_item(&claimed.id, &format!("VOD 시작 실패: {err:#}"));
                    self.logs
                        .push(format!("[VOD_QUEUE:ERR] start failed id={} err={err:#}", claimed.id))
                        .await;
                    *self.active_id.lock().await = None;
                    continue;
                }
            };
            let Some(job_id) = started.job_id.clone() else {
                let _ = self.fail_item(&claimed.id, "VOD job id가 생성되지 않았습니다.");
                *self.active_id.lock().await = None;
                continue;
            };
            let _ = self.mark_running(&claimed.id);
            self.logs
                .push(format!("[VOD_QUEUE] started id={} job={job_id}", claimed.id))
                .await;

            loop {
                tokio::time::sleep(STATUS_POLL).await;
                let status = self.vod.status().await;
                if status.job_id.as_deref() != Some(job_id.as_str()) {
                    if !status.running {
                        let _ = self.fail_item(&claimed.id, "VOD 상태 추적이 끊겼습니다.");
                        break;
                    }
                    continue;
                }
                if !status.running {
                    let _ = self.finish_item(&claimed.id, &status);
                    let level = if status.state == "COMPLETED" { "VOD_QUEUE" } else { "VOD_QUEUE:WARN" };
                    self.logs
                        .push(format!("[{level}] finished id={} state={} message={}", claimed.id, status.state, status.message))
                        .await;
                    break;
                }
            }
            *self.active_id.lock().await = None;
        }
    }

    fn claim_next(&self) -> Result<Option<ClaimedItem>> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let row = tx
            .query_row(
                "SELECT id,request_json FROM vod_queue WHERE state='QUEUED' ORDER BY created_at,id LIMIT 1",
                [],
                |row| Ok(ClaimedItem { id: row.get(0)?, request_json: row.get(1)? }),
            )
            .optional()?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let now = Utc::now().to_rfc3339();
        let changed = tx.execute(
            "UPDATE vod_queue SET state='STARTING', attempts=attempts+1, message='작업 시작 중…', started_at=?2, finished_at=NULL, updated_at=?2 WHERE id=?1 AND state='QUEUED'",
            params![row.id, now],
        )?;
        tx.commit()?;
        Ok((changed == 1).then_some(row))
    }

    fn mark_running(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            "UPDATE vod_queue SET state='RUNNING', message='다운로드 진행 중', updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        Ok(())
    }

    fn fail_item(&self, id: &str, message: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            "UPDATE vod_queue SET state='FAILED', message=?2, finished_at=?3, updated_at=?3 WHERE id=?1",
            params![id, message, now],
        )?;
        Ok(())
    }

    fn finish_item(&self, id: &str, status: &VodJobStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let (title, streamer) = status
            .analysis
            .as_ref()
            .map(|a| (a.title.as_str(), a.streamer.as_str()))
            .unwrap_or(("", ""));
        self.conn()?.execute(
            "UPDATE vod_queue SET state=?2, message=?3, title=CASE WHEN ?4='' THEN title ELSE ?4 END, streamer=CASE WHEN ?5='' THEN streamer ELSE ?5 END, output_file=COALESCE(?6,output_file), finished_at=?7, updated_at=?7 WHERE id=?1",
            params![id, status.state, status.message, title, streamer, status.output_file, now],
        )?;
        Ok(())
    }

    fn list_items(&self) -> Result<Vec<VodQueueItem>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"SELECT id,vod_url,output_directory,state,attempts,message,title,streamer,output_file,created_at,started_at,finished_at,updated_at
               FROM vod_queue ORDER BY
                 CASE state WHEN 'RUNNING' THEN 0 WHEN 'STARTING' THEN 0 WHEN 'CANCELLING' THEN 0 WHEN 'QUEUED' THEN 1 ELSE 2 END,
                 created_at DESC LIMIT ?1"#,
        )?;
        stmt.query_map(params![QUEUE_LIMIT as i64], |row| {
            Ok(VodQueueItem {
                id: row.get(0)?,
                vod_url: row.get(1)?,
                output_directory: row.get(2)?,
                state: row.get(3)?,
                attempts: row.get::<_, i64>(4)?.max(0) as u32,
                message: row.get(5)?,
                title: row.get(6)?,
                streamer: row.get(7)?,
                current_part: 0,
                part_count: 0,
                percent: 0.0,
                output_file: row.get(8)?,
                created_at: row.get(9)?,
                started_at: row.get(10)?,
                finished_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    fn item(&self, id: &str) -> Result<Option<VodQueueItem>> {
        Ok(self.list_items()?.into_iter().find(|item| item.id == id))
    }
}

fn apply_runtime_status(item: &mut VodQueueItem, status: &VodJobStatus) {
    item.state = if status.running { "RUNNING".into() } else { status.state.clone() };
    item.message = status.message.clone();
    item.current_part = status.current_part;
    item.part_count = status.part_count;
    item.percent = status.percent;
    if let Some(file) = &status.output_file {
        item.output_file = Some(file.clone());
    }
    if let Some(analysis) = &status.analysis {
        item.title = analysis.title.clone();
        item.streamer = analysis.streamer.clone();
    }
}

pub(crate) async fn api_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodQueueSnapshot>> {
    authorize(&headers, &state)?;
    Ok(Json(state.vod_queue.snapshot().await.map_err(internal_error)?))
}

pub(crate) async fn api_enqueue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<VodDownloadRequest>,
) -> ApiResult<Json<VodQueueItem>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    let tools = state.store.vod_tool_settings().map_err(internal_error)?;
    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
    let item = state
        .vod_queue
        .enqueue(req)
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(item))
}

pub(crate) async fn api_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .cancel(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn api_retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .retry(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn api_remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .remove(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn request(out: &str) -> VodDownloadRequest {
        VodDownloadRequest {
            vod_url: "https://vod.sooplive.com/player/123456789".into(),
            output_directory: out.into(),
            parts: vec![],
            quality: "best".into(),
            merge: true,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            max_retries: 5,
        }
    }

    #[tokio::test]
    async fn queued_item_can_be_cancelled_retried_and_removed() {
        let dir = tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("app").join("data").join("soop.db")).unwrap();
        let logs = LogBuffer::new();
        let vod = Arc::new(VodManager::new(backend, logs.clone()));
        let queue = VodQueueManager::new(store, vod, logs, Arc::new(Mutex::new(()))).unwrap();
        let item = queue.enqueue(request("C:\\SOOP_VOD")).await.unwrap();
        assert_eq!(queue.snapshot().await.unwrap().queued_count, 1);
        queue.cancel(&item.id).await.unwrap();
        assert_eq!(queue.item(&item.id).unwrap().unwrap().state, "CANCELLED");
        queue.retry(&item.id).await.unwrap();
        assert_eq!(queue.item(&item.id).unwrap().unwrap().state, "QUEUED");
        queue.cancel(&item.id).await.unwrap();
        queue.remove(&item.id).await.unwrap();
        assert!(queue.item(&item.id).unwrap().is_none());
    }

    #[tokio::test]
    async fn startup_marks_running_queue_item_interrupted() {
        let dir = tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("app").join("data").join("soop.db")).unwrap();
        let logs = LogBuffer::new();
        let vod = Arc::new(VodManager::new(backend, logs.clone()));
        let queue = VodQueueManager::new(store.clone(), vod.clone(), logs.clone(), Arc::new(Mutex::new(()))).unwrap();
        let item = queue.enqueue(request("C:\\SOOP_VOD")).await.unwrap();
        queue.conn().unwrap().execute("UPDATE vod_queue SET state='RUNNING' WHERE id=?1", params![item.id]).unwrap();
        let restarted = VodQueueManager::new(store, vod, logs, Arc::new(Mutex::new(()))).unwrap();
        assert_eq!(restarted.item(&item.id).unwrap().unwrap().state, "INTERRUPTED");
    }
}
''')

# ---------------- main.rs ----------------
p='rust-web/src/main.rs'
s=read(p)
s=rep(s,'mod vod;\nmod vod_tool_settings;','mod vod;\nmod vod_queue;\nmod vod_tool_settings;','vod_queue module')
s=rep(s,'routing::{get, post},','routing::{delete, get, post},','delete routing import')
s=rep(s,'use vod::VodManager;','use vod::VodManager;\nuse vod_queue::VodQueueManager;','queue import')
s=rep(s,'    vod: Arc<VodManager>,\n    store: Store,','    vod: Arc<VodManager>,\n    vod_queue: Arc<VodQueueManager>,\n    store: Store,','state queue')
old='''    let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
    let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
    let state = AppState {
'''
new='''    let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
    let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
    let lifecycle_lock = Arc::new(Mutex::new(()));
    let vod_queue = Arc::new(VodQueueManager::new(
        store.clone(),
        vod.clone(),
        logs.clone(),
        lifecycle_lock.clone(),
    )?);
    let state = AppState {
'''
s=rep(s,old,new,'queue construction')
s=rep(s,'        vod: vod.clone(),\n        store: store.clone(),','        vod: vod.clone(),\n        vod_queue: vod_queue.clone(),\n        store: store.clone(),','queue state init')
s=rep(s,'        lifecycle_lock: Arc::new(Mutex::new(())),','        lifecycle_lock: lifecycle_lock.clone(),','lifecycle reuse')
s=s.replace('[SERVER] Phase 12 backup/retention ready;','[SERVER] Phase 13 VOD queue/alerts ready;',1)
s=rep(s,'    backup::spawn_auto_backup(backups.clone(), logs.clone());\n','    backup::spawn_auto_backup(backups.clone(), logs.clone());\n    vod_queue.clone().spawn();\n','spawn queue')
s=rep(s,'.route("/phase12.js", get(phase12_js))\n','.route("/phase12.js", get(phase12_js))\n        .route("/phase13.js", get(phase13_js))\n','phase13 static route')
s=rep(s,'.route("/api/vod/cancel", post(api_vod_cancel))\n','.route("/api/vod/cancel", post(api_vod_cancel))\n        .route("/api/vod/queue", get(vod_queue::api_list).post(vod_queue::api_enqueue))\n        .route("/api/vod/queue/{id}/cancel", post(vod_queue::api_cancel))\n        .route("/api/vod/queue/{id}/retry", post(vod_queue::api_retry))\n        .route("/api/vod/queue/{id}", delete(vod_queue::api_remove))\n','queue routes')
s=s.replace('println!("SOOP Rust Web - Phase 12");','println!("SOOP Rust Web - Phase 13");',1)
s=rep(s,'    println!("Backup  : SQLite online backup + retention + guarded restore");\n','    println!("Backup  : SQLite online backup + retention + guarded restore");\n    println!("VODQueue: SQLite persistent FIFO queue + retry/cancel controls");\n','queue console')
s=rep(s,'''async fn phase12_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase12.js"),
    )
}
''','''async fn phase12_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase12.js"),
    )
}
async fn phase13_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase13.js"),
    )
}
''','phase13 handler')
write(p,s)

# ---------------- realtime.rs ----------------
p='rust-web/src/realtime.rs'
s=read(p)
s=rep(s,'    let vod = state.vod.status().await;\n    let logs = state.logs.tail(LOG_LINES).await;','    let vod = state.vod.status().await;\n    let queue = state.vod_queue.snapshot().await.ok();\n    let logs = state.logs.tail(LOG_LINES).await;','queue realtime snapshot')
s=rep(s,'        "vod": vod,\n        "logs": {"lines": logs},','        "vod": vod,\n        "queue": queue,\n        "logs": {"lines": logs},','queue realtime payload')
s=s.replace('"phase": "phase12-backup-retention"','"phase": "phase13-vod-queue-alerts"')
write(p,s)

# ---------------- backup.rs: restore guard also sees queued VOD ----------------
p='rust-web/src/backup.rs'
s=read(p)
old='''    if state.vod.status().await.running {
        return Err((
            StatusCode::CONFLICT,
            "VOD 작업을 중지한 뒤 복원하세요.".into(),
        ));
    }
'''
new='''    if state.vod.status().await.running {
        return Err((
            StatusCode::CONFLICT,
            "VOD 작업을 중지한 뒤 복원하세요.".into(),
        ));
    }
    if state
        .vod_queue
        .has_pending_or_active()
        .await
        .map_err(internal_error)?
    {
        return Err((
            StatusCode::CONFLICT,
            "VOD 다운로드 큐를 비운 뒤 복원하세요.".into(),
        ));
    }
'''
s=rep(s,old,new,'restore queue guard')
write(p,s)

# ---------------- phase13.js ----------------
write('rust-web/web/phase13.js', r'''(()=>{
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
''')

# ---------------- index.html ----------------
p='rust-web/web/index.html'
s=read(p)
s=s.replace('Rust Web Phase 12 · 백업 / 보관','Rust Web Phase 13 · VOD 큐 / 알림',1)
old='''  <p id="vodMessage" class="hint"></p><p id="vodMeta" class="mono"></p><p id="vodOutputFile" class="mono"></p>
</section>
</div>

<div class="tab-page" data-tab-page="history" hidden>'''
new='''  <p id="vodMessage" class="hint"></p><p id="vodMeta" class="mono"></p><p id="vodOutputFile" class="mono"></p>
</section>
<section>
  <div class="title"><h2>VOD 다운로드 큐</h2><div><button id="vodNotify" type="button">브라우저 알림 켜기</button> <button id="p13QueueRefresh" type="button">새로고침</button></div></div>
  <div class="summary"><span>실행 <b id="p13QueueActive">0</b></span><span>대기 <b id="p13QueueWaiting">0</b></span><span>처리 방식 <b>1건씩 순차 실행</b></span></div>
  <p class="hint">위의 <b>큐에 추가</b>를 누르면 여러 VOD를 저장해 두고 한 건씩 순서대로 다운로드합니다. 실패·취소·서버 재시작으로 중단된 작업은 재시도할 수 있습니다. 브라우저 알림은 현재 브라우저에만 저장됩니다.</p>
  <div class="table"><table><thead><tr><th>상태</th><th>VOD</th><th>시도</th><th>진행</th><th>메시지 / 결과</th><th>제어</th></tr></thead><tbody id="p13QueueRows"></tbody></table></div>
</section>
</div>

<div class="tab-page" data-tab-page="history" hidden>'''
s=rep(s,old,new,'queue UI section')
s=s.replace('복원은 Watcher와 VOD가 모두 중지된 상태에서만 가능합니다.','복원은 Watcher와 VOD가 모두 중지되고 VOD 다운로드 큐가 비어 있는 상태에서만 가능합니다.',1)
s=s.replace('v=p12-backup1','v=p13-queue1')
s=rep(s,'<script src="/phase12.js?v=p13-queue1"></script>\n</body></html>','<script src="/phase12.js?v=p13-queue1"></script>\n<script src="/phase13.js?v=p13-queue1"></script>\n</body></html>','phase13 script include')
write(p,s)

# ---------------- style.css minor functional polish only ----------------
p='rust-web/web/style.css'
s=read(p)
if '.secondary{' not in s:
    s += '\n.secondary{background:#253142;color:#d8e2ee}.secondary:hover{background:#303e50}#p13QueueRows td:nth-child(2){min-width:260px}#p13QueueRows td:nth-child(5){min-width:240px}\n'
write(p,s)

# Sanity checks
assert Path('rust-web/src/vod_queue.rs').is_file()
assert Path('rust-web/web/phase13.js').is_file()
main=read('rust-web/src/main.rs')
idx=read('rust-web/web/index.html')
store=read('rust-web/src/store.rs')
assert 'mod vod_queue;' in main
assert '/api/vod/queue' in main
assert '/phase13.js' in main
assert 'CREATE TABLE IF NOT EXISTS vod_queue' in store
assert 'phase13.js?v=p13-queue1' in idx
assert 'VOD 다운로드 큐' in idx
print('Phase 13 queue/alerts patch sanity: PASS')
