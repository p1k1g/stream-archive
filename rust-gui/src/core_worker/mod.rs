//! Dedicated background worker owning `StreamArchiveCore` and its Tokio runtime.
//! All persistence, process ownership, secret encryption, and network operations stay here.

mod handlers;
mod protocol;
mod snapshots;

pub use protocol::{Request, Response};
pub use snapshots::localized_setting_description;

use handlers::{LoopAction, handle_request};
use snapshots::{
    configuration_snapshot, live_status, maintenance_snapshot, queue_status, read_snapshot,
    storage_status, vod_status,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use stream_archive_server::{
    app_core::StreamArchiveCore, backend::resolve_backend_dir, diagnostics::DiagnosticsSnapshot,
    history_service::HistoryFilter,
};

pub(super) fn spawn_detached_task(
    name: &str,
    f: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(|_| ())
        .map_err(|error| format!("백그라운드 작업 스레드를 시작할 수 없습니다: {error}"))
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn spawn_worker(
    requests: mpsc::Receiver<Request>,
    responses: mpsc::Sender<Response>,
    stopping: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("stream-archive-core-worker".into())
        .spawn(move || worker(requests, responses, stopping))
}

fn worker(
    requests: mpsc::Receiver<Request>,
    responses: mpsc::Sender<Response>,
    stopping: Arc<AtomicBool>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = responses.send(Response::Error(format!("Worker 초기화 실패: {error}")));
            return;
        }
    };
    let backend = resolve_backend_dir();
    let backend_path = backend.as_ref().ok().cloned();
    let core = match backend.and_then(StreamArchiveCore::open) {
        Ok(opened) => opened.core,
        Err(error) => {
            let message = format!("런타임 초기화 실패: {error:#}");
            let _ = responses.send(Response::Snapshot {
                fields: None,
                diagnostics: DiagnosticsSnapshot::startup_failure(
                    backend_path.as_deref(),
                    &message,
                ),
                window_bounds: None,
                first_run: false,
                startup_failed: true,
                message,
            });
            return;
        }
    };
    worker_loop(&core, &runtime, requests, &responses, &stopping);
    runtime.block_on(core.shutdown());
    let _ = responses.send(Response::ExitComplete);
}

pub fn worker_loop(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    requests: mpsc::Receiver<Request>,
    responses: &mpsc::Sender<Response>,
    stopping: &AtomicBool,
) {
    let _ = responses.send(Response::ResourceTrace(core.resource_trace()));
    let mut download_events = core.subscribe_download_events();
    let event_responses = responses.clone();
    let _download_event_guard = AbortOnDrop(runtime.spawn(async move {
        loop {
            match download_events.recv().await {
                Ok(first) => {
                    let mut batch = vec![first];
                    batch.extend(crate::notifications::drain_download_events(|| {
                        download_events.try_recv()
                    }));
                    if event_responses.send(Response::Downloads(batch)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    eprintln!("Windows download notifications skipped {count} older events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));

    runtime.block_on(async {
        core.spawn_vod_history_sync();
        core.spawn_queue_worker();
        core.spawn_auto_backup();
    });
    if !send_initial_snapshots(core, runtime, responses) {
        return;
    }

    for request in requests {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        if handle_request(core, runtime, responses, request) == LoopAction::Stop {
            break;
        }
    }
}

fn send_initial_snapshots(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
) -> bool {
    if responses
        .send(read_snapshot(
            core,
            true,
            "canonical SQLite에서 설정을 불러왔습니다. 저장한 변경사항은 이후 런타임 작업부터 적용됩니다.",
        ))
        .is_err()
    {
        return false;
    }
    if responses
        .send(configuration_snapshot(
            core,
            true,
            true,
            "canonical SQLite에서 채널 및 서비스 연결 정보를 불러왔습니다.",
        ))
        .is_err()
    {
        return false;
    }
    if responses
        .send(live_status(core, runtime, false, None))
        .is_err()
    {
        return false;
    }
    if responses.send(storage_status(core, false, 0)).is_err() {
        return false;
    }
    if responses.send(vod_status(core, runtime, false)).is_err() {
        return false;
    }
    if responses.send(queue_status(core, runtime, false)).is_err() {
        return false;
    }
    let history_response = match core.history(&HistoryFilter::default()) {
        Ok(history) => Response::History {
            history,
            view: "ALL".into(),
            message: Some("canonical SQLite에서 기록을 불러왔습니다".into()),
        },
        Err(error) => Response::HistoryError(format!("기록 불러오기 실패: {error:#}")),
    };
    if responses.send(history_response).is_err() {
        return false;
    }
    responses
        .send(maintenance_snapshot(
            core,
            runtime,
            false,
            "공유 런타임 서비스에서 관리 상태를 불러왔습니다.",
        ))
        .is_ok()
}

#[cfg(test)]
#[path = "../../tests/support/desktop_process.rs"]
mod desktop_process;

#[cfg(test)]
mod desktop_lifecycle_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn confirmed_shutdown_does_not_execute_queued_mutations() {
        desktop_process::run(
            "core_worker::desktop_lifecycle_tests::confirmed_shutdown_does_not_execute_queued_mutations",
            |directory| {
                let backend = directory.join("backend");
                std::fs::create_dir_all(&backend).unwrap();
                let core = StreamArchiveCore::open(&backend).unwrap().core;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .unwrap();
                let (sender, requests) = mpsc::channel();
                let (responses, _receiver) = mpsc::channel();
                sender
                    .send(Request::Save(BTreeMap::from([(
                        "CHECK_INTERVAL".into(),
                        "99".into(),
                    )])))
                    .unwrap();
                sender.send(Request::Shutdown).unwrap();
                drop(sender);
                let stopping = AtomicBool::new(true);
                worker_loop(&core, &runtime, requests, &responses, &stopping);
                assert_eq!(core.settings().unwrap()["CHECK_INTERVAL"], "30");
                runtime.block_on(core.shutdown());
                drop(runtime);
                drop(core);
            },
        );
    }
}
