use crate::{
    ApiResult, AppState, authorize,
    backend::LogBuffer,
    internal_error,
    model::{VodDownloadRequest, VodJobStatus, VodQueueItem, VodQueueSnapshot},
    primary_config::apply_vod_tool_defaults,
    store::Store,
    support::platform::{PlatformId, detect_vod_platform},
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
            format!(
                "failed to open queue database {}",
                self.store.path().display()
            )
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
        let platform = detect_vod_platform(&req.vod_url)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let request_json = serde_json::to_string(&req)?;
        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM vod_queue WHERE state IN ('QUEUED','STARTING','RUNNING','CANCELLING')",
                [],
                |row| row.get(0),
            )?;
            if pending >= QUEUE_LIMIT as i64 {
                bail!("VOD 다운로드 큐는 실행/대기 작업을 최대 {QUEUE_LIMIT}건까지 보관합니다.");
            }
            tx.execute(
                r#"INSERT INTO vod_queue(id,platform,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
                   VALUES(?1,?2,?3,?4,?5,'QUEUED',0,'대기 중',?6,?6)"#,
                params![
                    id,
                    platform.as_str(),
                    request_json,
                    req.vod_url,
                    req.output_directory,
                    now
                ],
            )?;
            tx.commit()?;
        }
        self.logs
            .push(format!(
                "[VOD_QUEUE] queued id={id} platform={platform} url={}",
                req.vod_url
            ))
            .await;
        self.item(&id)?.context("queued item disappeared")
    }

    pub async fn snapshot(&self) -> Result<VodQueueSnapshot> {
        let active_id = self.active_id.lock().await.clone();
        let current = self.vod.status().await;
        let conn = self.conn()?;
        let mut items = Self::list_items_from_conn(&conn)?;
        let queued_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM vod_queue WHERE state='QUEUED'",
            [],
            |row| row.get(0),
        )?;
        drop(conn);

        if let Some(active) = active_id.as_deref() {
            if let Some(item) = items.iter_mut().find(|item| item.id == active) {
                apply_runtime_status(item, &current);
            }
        }
        Ok(VodQueueSnapshot {
            active_id,
            queued_count: queued_count.max(0) as usize,
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
            self.logs
                .push(format!("[VOD_QUEUE] cancelled id={id}"))
                .await;
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
        self.logs
            .push(format!("[VOD_QUEUE] cancelled queued id={id}"))
            .await;
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
        self.logs
            .push(format!("[VOD_QUEUE] retry queued id={id}"))
            .await;
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
        self.logs
            .push(format!("[VOD_QUEUE] removed id={id}"))
            .await;
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
                        .push(format!(
                            "[VOD_QUEUE:ERR] start failed id={} err={err:#}",
                            claimed.id
                        ))
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
                .push(format!(
                    "[VOD_QUEUE] started id={} job={job_id} platform={}",
                    claimed.id, started.platform
                ))
                .await;

            loop {
                tokio::time::sleep(STATUS_POLL).await;
                if let Some(status) = self.vod.terminal_status(&job_id).await {
                    let _ = self.finish_item(&claimed.id, &status);
                    let level = if status.state == "COMPLETED" {
                        "VOD_QUEUE"
                    } else {
                        "VOD_QUEUE:WARN"
                    };
                    self.logs
                        .push(format!(
                            "[{level}] finished id={} state={} message={}",
                            claimed.id, status.state, status.message
                        ))
                        .await;
                    break;
                }
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
                    let level = if status.state == "COMPLETED" {
                        "VOD_QUEUE"
                    } else {
                        "VOD_QUEUE:WARN"
                    };
                    self.logs
                        .push(format!(
                            "[{level}] finished id={} state={} message={}",
                            claimed.id, status.state, status.message
                        ))
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
                |row| {
                    Ok(ClaimedItem {
                        id: row.get(0)?,
                        request_json: row.get(1)?,
                    })
                },
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
            "UPDATE vod_queue SET platform=?2, state=?3, message=?4, title=CASE WHEN ?5='' THEN title ELSE ?5 END, streamer=CASE WHEN ?6='' THEN streamer ELSE ?6 END, output_file=COALESCE(?7,output_file), finished_at=?8, updated_at=?8 WHERE id=?1",
            params![
                id,
                status.platform.as_str(),
                status.state,
                status.message,
                title,
                streamer,
                status.output_file,
                now
            ],
        )?;
        Ok(())
    }

    fn list_items(&self) -> Result<Vec<VodQueueItem>> {
        let conn = self.conn()?;
        Self::list_items_from_conn(&conn)
    }

    fn list_items_from_conn(conn: &Connection) -> Result<Vec<VodQueueItem>> {
        let mut stmt = conn.prepare(
            r#"SELECT platform,id,vod_url,output_directory,state,attempts,message,title,streamer,output_file,created_at,started_at,finished_at,updated_at
               FROM vod_queue ORDER BY
                 CASE state WHEN 'RUNNING' THEN 0 WHEN 'STARTING' THEN 0 WHEN 'CANCELLING' THEN 0 WHEN 'QUEUED' THEN 1 ELSE 2 END,
                 created_at DESC LIMIT ?1"#,
        )?;
        stmt.query_map(params![QUEUE_LIMIT as i64], queue_item_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn item(&self, id: &str) -> Result<Option<VodQueueItem>> {
        let conn = self.conn()?;
        conn.query_row(
            r#"SELECT platform,id,vod_url,output_directory,state,attempts,message,title,streamer,output_file,created_at,started_at,finished_at,updated_at
               FROM vod_queue WHERE id=?1"#,
            params![id],
            queue_item_from_row,
        )
        .optional()
        .map_err(Into::into)
    }
}

fn stored_platform(value: String) -> PlatformId {
    value.parse().unwrap_or_default()
}

fn queue_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VodQueueItem> {
    Ok(VodQueueItem {
        platform: stored_platform(row.get(0)?),
        id: row.get(1)?,
        vod_url: row.get(2)?,
        output_directory: row.get(3)?,
        state: row.get(4)?,
        attempts: row.get::<_, i64>(5)?.max(0) as u32,
        message: row.get(6)?,
        title: row.get(7)?,
        streamer: row.get(8)?,
        current_part: 0,
        part_count: 0,
        percent: 0.0,
        output_file: row.get(9)?,
        created_at: row.get(10)?,
        started_at: row.get(11)?,
        finished_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn apply_runtime_status(item: &mut VodQueueItem, status: &VodJobStatus) {
    item.platform = status.platform;
    item.state = if status.running {
        "RUNNING".into()
    } else {
        status.state.clone()
    };
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
    Ok(Json(
        state.vod_queue.snapshot().await.map_err(internal_error)?,
    ))
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
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
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
        assert_eq!(item.platform, PlatformId::Soop);
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
    async fn queue_limit_keeps_all_pending_rows_visible_and_rejects_overflow() {
        let dir = tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("app").join("data").join("soop.db")).unwrap();
        let logs = LogBuffer::new();
        let vod = Arc::new(VodManager::new(backend, logs.clone()));
        let queue = VodQueueManager::new(store, vod, logs, Arc::new(Mutex::new(()))).unwrap();
        let request_json = serde_json::to_string(&request("C:\\SOOP_VOD")).unwrap();
        let conn = queue.conn().unwrap();
        for i in 0..QUEUE_LIMIT {
            let id = format!("queued-{i:03}");
            let now = format!("2026-09-09T00:{:02}:00Z", i % 60);
            conn.execute(
                r#"INSERT INTO vod_queue(id,platform,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
                   VALUES(?1,'SOOP',?2,'https://vod.sooplive.com/player/123456789','C:\SOOP_VOD','QUEUED',0,'대기 중',?3,?3)"#,
                params![id, request_json, now],
            )
            .unwrap();
        }
        drop(conn);

        let snapshot = queue.snapshot().await.unwrap();
        assert_eq!(snapshot.queued_count, QUEUE_LIMIT);
        assert_eq!(
            snapshot
                .items
                .iter()
                .filter(|item| item.state == "QUEUED")
                .count(),
            QUEUE_LIMIT
        );
        let err = queue.enqueue(request("C:/SOOP_VOD")).await.unwrap_err();
        assert!(err.to_string().contains("최대"));
    }

    #[tokio::test]
    async fn startup_marks_running_queue_item_interrupted() {
        let dir = tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("app").join("data").join("soop.db")).unwrap();
        let logs = LogBuffer::new();
        let vod = Arc::new(VodManager::new(backend, logs.clone()));
        let queue = VodQueueManager::new(
            store.clone(),
            vod.clone(),
            logs.clone(),
            Arc::new(Mutex::new(())),
        )
        .unwrap();
        let item = queue.enqueue(request("C:\\SOOP_VOD")).await.unwrap();
        queue
            .conn()
            .unwrap()
            .execute(
                "UPDATE vod_queue SET state='RUNNING' WHERE id=?1",
                params![item.id],
            )
            .unwrap();
        let restarted = VodQueueManager::new(store, vod, logs, Arc::new(Mutex::new(()))).unwrap();
        assert_eq!(
            restarted.item(&item.id).unwrap().unwrap().state,
            "INTERRUPTED"
        );
    }
}
