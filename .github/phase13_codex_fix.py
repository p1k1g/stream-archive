from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# --- Codex P2 #1: preserve terminal VOD result even if a later direct job replaces status ---
p='rust-web/src/vod.rs'
s=read(p)
s=rep(s,
'''    collections::{BTreeMap, HashMap},''',
'''    collections::{BTreeMap, HashMap, VecDeque},''',
'VecDeque import')

s=rep(s,
'''pub struct VodManager {
    backend_dir: PathBuf,
    logs: LogBuffer,
    runtime: Mutex<JobRuntime>,
    status: Arc<RwLock<VodJobStatus>>,
}
''',
'''const TERMINAL_CACHE_LIMIT: usize = 32;

pub struct VodManager {
    backend_dir: PathBuf,
    logs: LogBuffer,
    runtime: Mutex<JobRuntime>,
    status: Arc<RwLock<VodJobStatus>>,
    terminal: Arc<Mutex<VecDeque<(String, VodJobStatus)>>>,
}
''',
'VodManager terminal cache field')

s=rep(s,
'''            status: Arc::new(RwLock::new(VodJobStatus::default())),
        }
    }

    pub async fn status(&self) -> VodJobStatus {
''',
'''            status: Arc::new(RwLock::new(VodJobStatus::default())),
            terminal: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn status(&self) -> VodJobStatus {
''',
'terminal cache init')

s=rep(s,
'''        self.status.read().await.clone()
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
''',
'''        self.status.read().await.clone()
    }

    pub async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        let terminal = self.terminal.lock().await;
        terminal
            .iter()
            .rev()
            .find(|(id, _)| id == job_id)
            .map(|(_, status)| status.clone())
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
''',
'terminal status accessor')

s=rep(s,
'''        let logs = self.logs.clone();
        let status = self.status.clone();
        let job_id = Uuid::new_v4().to_string();
''',
'''        let logs = self.logs.clone();
        let status = self.status.clone();
        let terminal = self.terminal.clone();
        let job_id = Uuid::new_v4().to_string();
        let terminal_job_id = job_id.clone();
''',
'capture terminal cache in job task')

old='''        let task = tokio::spawn(async move {
            let result = match kind {
                VodJobKind::Analyze(req) => run_analysis(&backend, req, &logs, &status, &cancel)
                    .await
                    .map(|_| ()),
                VodJobKind::Download(req) => {
                    run_download(&backend, req, &logs, &status, &cancel).await
                }
            };
            let mut s = status.write().await;
            s.running = false;
            s.finished_at = Some(Utc::now().to_rfc3339());
            if cancel.load(Ordering::SeqCst) {
                s.state = "CANCELLED".into();
                s.message = "VOD 작업이 취소되었습니다.".into();
                logs.push("[VOD] job cancelled").await;
            } else if let Err(err) = result {
                s.state = "FAILED".into();
                s.message = redact(&format!("{err:#}"));
                logs.push(format!("[VOD:ERR] {}", s.message)).await;
            }
        });
'''
new='''        let task = tokio::spawn(async move {
            let result = match kind {
                VodJobKind::Analyze(req) => run_analysis(&backend, req, &logs, &status, &cancel)
                    .await
                    .map(|_| ()),
                VodJobKind::Download(req) => {
                    run_download(&backend, req, &logs, &status, &cancel).await
                }
            };
            let final_status = {
                let mut s = status.write().await;
                s.running = false;
                s.finished_at = Some(Utc::now().to_rfc3339());
                if cancel.load(Ordering::SeqCst) {
                    s.state = "CANCELLED".into();
                    s.message = "VOD 작업이 취소되었습니다.".into();
                    logs.push("[VOD] job cancelled").await;
                } else if let Err(err) = result {
                    s.state = "FAILED".into();
                    s.message = redact(&format!("{err:#}"));
                    logs.push(format!("[VOD:ERR] {}", s.message)).await;
                }
                s.clone()
            };
            let mut terminal = terminal.lock().await;
            if terminal.len() >= TERMINAL_CACHE_LIMIT {
                terminal.pop_front();
            }
            terminal.push_back((terminal_job_id, final_status));
        });
'''
s=rep(s,old,new,'persist terminal job status')
write(p,s)

# --- Codex P2 #2: enforce pending queue limit atomically and count from DB ---
p='rust-web/src/vod_queue.rs'
s=read(p)
old='''    pub async fn enqueue(&self, req: VodDownloadRequest) -> Result<VodQueueItem> {
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
'''
new='''    pub async fn enqueue(&self, req: VodDownloadRequest) -> Result<VodQueueItem> {
        validate_download_request(&req)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let request_json = serde_json::to_string(&req)?;
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
            r#"INSERT INTO vod_queue(id,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
               VALUES(?1,?2,?3,?4,'QUEUED',0,'대기 중',?5,?5)"#,
            params![id, request_json, req.vod_url, req.output_directory, now],
        )?;
        tx.commit()?;
        drop(conn);
        self.logs
            .push(format!("[VOD_QUEUE] queued id={id} url={}", req.vod_url))
            .await;
        self.item(&id)?.context("queued item disappeared")
    }
'''
s=rep(s,old,new,'atomic queue limit')

s=rep(s,
'''        let queued_count = items.iter().filter(|item| item.state == "QUEUED").count();
        Ok(VodQueueSnapshot {
''',
'''        let queued_count = self.queued_count()?;
        Ok(VodQueueSnapshot {
''',
'queue count from DB')

s=rep(s,
'''    pub async fn has_pending_or_active(&self) -> Result<bool> {
''',
'''    fn queued_count(&self) -> Result<usize> {
        let count: i64 = self.conn()?.query_row(
            "SELECT COUNT(*) FROM vod_queue WHERE state='QUEUED'",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub async fn has_pending_or_active(&self) -> Result<bool> {
''',
'queued_count helper')

# Prefer cached terminal result for the queue-owned job. A later direct VOD job
# can safely replace VodManager.status() without losing the queue result.
old='''            loop {
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
'''
new='''            loop {
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
'''
s=rep(s,old,new,'terminal status preservation in queue worker')

# Add regression for queue capacity/count/actionability. Populate rows directly so
# the test stays fast, then verify enqueue rejects item 101 and all waiting rows
# remain visible under the display limit.
needle='''    #[tokio::test]
    async fn startup_marks_running_queue_item_interrupted() {
'''
insert='''    #[tokio::test]
    async fn queue_limit_keeps_all_pending_rows_visible_and_rejects_overflow() {
        let dir = tempdir().unwrap();
        let backend = dir.path().join("app").join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let store = Store::open(dir.path().join("app").join("data").join("soop.db")).unwrap();
        let logs = LogBuffer::new();
        let vod = Arc::new(VodManager::new(backend, logs.clone()));
        let queue = VodQueueManager::new(store, vod, logs, Arc::new(Mutex::new(()))).unwrap();
        let request_json = serde_json::to_string(&request("C:\\\\SOOP_VOD")).unwrap();
        let conn = queue.conn().unwrap();
        for i in 0..QUEUE_LIMIT {
            let id = format!("queued-{i:03}");
            let now = format!("2026-09-09T00:{:02}:00Z", i % 60);
            conn.execute(
                r#"INSERT INTO vod_queue(id,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
                   VALUES(?1,?2,'https://vod.sooplive.com/player/123456789','C:\\SOOP_VOD','QUEUED',0,'대기 중',?3,?3)"#,
                params![id, request_json, now],
            )
            .unwrap();
        }
        drop(conn);

        let snapshot = queue.snapshot().await.unwrap();
        assert_eq!(snapshot.queued_count, QUEUE_LIMIT);
        assert_eq!(snapshot.items.iter().filter(|item| item.state == "QUEUED").count(), QUEUE_LIMIT);
        let err = queue.enqueue(request("C:\\SOOP_VOD")).await.unwrap_err();
        assert!(err.to_string().contains("최대"));
    }

'''+needle
s=rep(s,needle,insert,'queue limit regression test')
write(p,s)

# Static sanity checks.
vod=read('rust-web/src/vod.rs')
q=read('rust-web/src/vod_queue.rs')
assert 'terminal_status(&self, job_id: &str)' in vod
assert 'TERMINAL_CACHE_LIMIT' in vod
assert 'self.vod.terminal_status(&job_id).await' in q
assert 'pending >= QUEUE_LIMIT as i64' in q
assert 'let queued_count = self.queued_count()?' in q
assert 'queue_limit_keeps_all_pending_rows_visible_and_rejects_overflow' in q
print('Phase 13 Codex fixes sanity: PASS')
