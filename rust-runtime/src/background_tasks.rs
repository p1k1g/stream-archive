//! Core-owned background work. Tasks never capture this owner themselves.
use std::{collections::BTreeMap, future::Future, sync::Mutex};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Backup,
    VodHistory,
}

#[derive(Default)]
struct State {
    stopping: bool,
    tasks: BTreeMap<Kind, JoinHandle<()>>,
}

pub(crate) struct BackgroundTasks {
    state: Mutex<State>,
    stop: watch::Sender<bool>,
    shutdown: tokio::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl Default for BackgroundTasks {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            stop: watch::channel(false).0,
            shutdown: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

impl BackgroundTasks {
    #[cfg(test)]
    pub(crate) fn task_count(&self) -> usize {
        self.state.lock().unwrap().tasks.len()
    }
    pub(crate) fn start<F, Fut>(&self, kind: Kind, make: F)
    where
        F: FnOnce(watch::Receiver<bool>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping || state.tasks.contains_key(&kind) {
            return;
        }
        let task = tokio::spawn(make(self.stop.subscribe()));
        state.tasks.insert(kind, task);
    }

    pub(crate) fn request_stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopping = true;
        self.stop.send_replace(true);
    }

    pub(crate) async fn shutdown(&self) {
        // Concurrent shutdown callers also wait for the first caller's joins.
        let mut draining = self.shutdown.lock().await;
        let tasks = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopping = true;
            self.stop.send_replace(true);
            std::mem::take(&mut state.tasks)
        };
        draining.extend(tasks.into_values());
        while let Some(task) = draining.last_mut() {
            let _ = task.await;
            draining.pop();
        }
    }
}

impl Drop for BackgroundTasks {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        for task in self.shutdown.get_mut().iter() {
            task.abort();
        }
        for task in self
            .state
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .tasks
            .values()
        {
            task.abort();
        }
    }
}

pub(crate) async fn stopped(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow_and_update() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Release(Arc<AtomicUsize>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn repeated_start_and_concurrent_shutdown_release_exactly_two_tasks() {
        let tasks = Arc::new(BackgroundTasks::default());
        let released = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            for kind in [Kind::Backup, Kind::VodHistory] {
                let released = released.clone();
                tasks.start(kind, move |mut stop| async move {
                    let _release = Release(released);
                    stopped(&mut stop).await;
                });
            }
        }
        tokio::join!(tasks.shutdown(), tasks.shutdown());
        assert_eq!(released.load(Ordering::SeqCst), 2);
        tasks.start(Kind::Backup, |_| async {
            panic!("restarted after shutdown")
        });
        assert!(tasks.state.lock().unwrap().tasks.is_empty());
    }

    #[tokio::test]
    async fn last_owner_drop_aborts_and_releases_task_captures() {
        let tasks = BackgroundTasks::default();
        let captured = Arc::new(());
        let weak = Arc::downgrade(&captured);
        tasks.start(Kind::Backup, move |_| async move {
            let _captured = captured;
            std::future::pending::<()>().await;
        });
        drop(tasks);
        tokio::task::yield_now().await;
        assert!(weak.upgrade().is_none());
    }

    #[tokio::test]
    async fn cancelled_shutdown_keeps_joins_for_the_next_shutdown_caller() {
        let tasks = BackgroundTasks::default();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let captured = Arc::new(());
        let weak = Arc::downgrade(&captured);
        tasks.start(Kind::Backup, move |mut stop| async move {
            let _captured = captured;
            stopped(&mut stop).await;
            let _ = finished.await;
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), tasks.shutdown())
                .await
                .is_err()
        );
        assert!(weak.upgrade().is_some());
        finish.send(()).unwrap();
        tasks.shutdown().await;
        assert!(weak.upgrade().is_none());
    }
}
