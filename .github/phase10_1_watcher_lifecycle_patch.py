from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    s = p.read_text(encoding='utf-8')
    if s.count(old) < count:
        raise SystemExit(f'patch target not found: {path}\n{old[:300]}')
    p.write_text(s.replace(old, new, count), encoding='utf-8', newline='\n')

nw = 'rust-web/src/native_watcher.rs'
replace(nw,
'''        if let Some(task) = runtime.task.as_ref() {
            if !task.is_finished() {
                return Ok(self.snapshot.read().await.clone());
            }
        }
        runtime.task.take();
        runtime.stop_tx = None;
        runtime.command_tx = None;''',
'''        if let Some(task) = runtime.task.as_ref() {
            if !task.is_finished() {
                return Ok(self.snapshot.read().await.clone());
            }
        }
        if let Some(task) = runtime.task.take() {
            finish_watcher_task(task, &self.logs, &self.snapshot, "before restart").await;
        }
        runtime.stop_tx = None;
        runtime.command_tx = None;''')

replace(nw,
'''    pub async fn stop(&self) -> Result<WatcherStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if let Some(tx) = runtime.stop_tx.take() {
                let _ = tx.send(());
            }
            runtime.command_tx = None;
            runtime.task.take()
        };
        if let Some(task) = task {
            let _ = task.await;
        }
        self.snapshot.write().await.running = false;
        self.logs.push("[RUST] native watcher v4 stopped").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        let finished = {
            let runtime = self.runtime.lock().await;
            runtime.task.as_ref().is_some_and(|task| task.is_finished())
        };
        if finished {
            let mut runtime = self.runtime.lock().await;
            runtime.task.take();
            runtime.stop_tx = None;
            runtime.command_tx = None;
            self.snapshot.write().await.running = false;
        }
        Ok(self.snapshot.read().await.clone())
    }''',
'''    pub async fn stop(&self) -> Result<WatcherStatus> {
        // Keep the runtime mutex for the entire shutdown. Otherwise a concurrent Start/status
        // request can install or remove a newer watcher while the previous task is still exiting.
        let mut runtime = self.runtime.lock().await;
        if let Some(tx) = runtime.stop_tx.take() {
            let _ = tx.send(());
        }
        runtime.command_tx = None;
        if let Some(task) = runtime.task.take() {
            finish_watcher_task(task, &self.logs, &self.snapshot, "stop").await;
        }
        self.snapshot.write().await.running = false;
        self.logs.push("[RUST] native watcher v4 stopped").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        // Inspect and retire a finished task under one lock. The old two-lock sequence could
        // observe an old finished task, then accidentally take a newly-started task.
        let mut runtime = self.runtime.lock().await;
        if runtime.task.as_ref().is_some_and(|task| task.is_finished()) {
            if let Some(task) = runtime.task.take() {
                finish_watcher_task(task, &self.logs, &self.snapshot, "status reap").await;
            }
            runtime.stop_tx = None;
            runtime.command_tx = None;
            self.snapshot.write().await.running = false;
        }
        Ok(self.snapshot.read().await.clone())
    }''')

marker = '''async fn run_native_watcher(
    backend_dir: PathBuf,'''
helper = '''async fn finish_watcher_task(
    task: JoinHandle<()>,
    logs: &LogBuffer,
    snapshot: &Arc<RwLock<WatcherStatus>>,
    context: &str,
) {
    if let Err(err) = task.await {
        logs.push(format!(
            "[RUST:ERR] watcher task terminated unexpectedly ({context}): {err}"
        ))
        .await;
        let mut state = snapshot.write().await;
        state.running = false;
        state.recording_count = 0;
        state.error_count = state.error_count.saturating_add(1);
        for channel in &mut state.channels {
            if channel.status == "RECORDING" {
                channel.status = "ERROR".into();
                channel.detail = Some(
                    "watcher task terminated unexpectedly; recorder child was terminated".into(),
                );
            }
        }
    }
}

'''
replace(nw, marker, helper + marker)

rec = 'rust-web/src/recorder.rs'
replace(rec,
'''            .stderr(Stdio::piped())
            .kill_on_drop(false);''',
'''            .stderr(Stdio::piped())
            // If the watcher task panics or is otherwise dropped unexpectedly, do not leave
            // an unmanaged Streamlink process recording after the UI reports STOPPED.
            .kill_on_drop(true);''')
