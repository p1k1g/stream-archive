#[cfg(test)]
use crate::backend::LogBuffer;
use crate::{ApiResult, AppState, internal_error};
use axum::{
    extract::State,
    http::HeaderMap,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::{
    sync::mpsc,
    time::{Instant, MissedTickBehavior, interval, sleep_until},
};
use tokio_stream::wrappers::ReceiverStream;

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const MIN_EVENT_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
// Established cookie-authenticated streams are revoked within this interval.
const SESSION_REVALIDATE_INTERVAL: Duration = Duration::from_secs(5);
const LOG_LINES: usize = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamAuth {
    LocalBypass,
    Session,
}

fn authorize_stream(headers: &HeaderMap, state: &AppState) -> ApiResult<StreamAuth> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(StreamAuth::LocalBypass);
    }
    // SSE is a read-only GET. Native EventSource cannot attach X-CSRF-Token,
    // so validate the HttpOnly session cookie without weakening CSRF checks on
    // any mutating API. Recovery Bearer mode remains on REST polling because
    // native EventSource also cannot attach Authorization.
    state.auth.authorize_session_readonly(headers)?;
    Ok(StreamAuth::Session)
}

async fn snapshot_event(state: &AppState) -> Event {
    let (watcher, watcher_error) = match state.watcher.status().await {
        Ok(status) => (status, None),
        Err(err) => (Default::default(), Some(err.to_string())),
    };
    let vod = state.vod.status().await;
    let queue = state.vod_queue.snapshot().await.ok();
    let logs = state.logs.tail(LOG_LINES).await;
    let payload = json!({
        "phase": "phase13-vod-queue-alerts",
        "status": {
            "watcher": watcher,
            "backend_dir": state.backend_dir.display().to_string(),
            "bind": state.bind,
            "phase": "phase13-vod-queue-alerts"
        },
        "vod": vod,
        "queue": queue,
        "logs": {"lines": logs},
        "watcher_error": watcher_error
    });
    Event::default().event("snapshot").data(payload.to_string())
}

pub(crate) async fn api_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let auth_mode = authorize_stream(&headers, &state)?;
    let session_headers = (auth_mode == StreamAuth::Session).then_some(headers.clone());

    let mut log_events = state.logs.subscribe();
    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(8);
    tokio::spawn(async move {
        let mut tick = interval(SNAPSHOT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut revalidate = interval(SESSION_REVALIDATE_INTERVAL);
        revalidate.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // The initial request was already authenticated above. Do not spend the
        // first loop iteration re-reading SQLite solely because interval() ticks
        // immediately on creation.
        revalidate.tick().await;

        // Streamlink can emit many stderr lines in a short burst. A log event should
        // wake the UI promptly, but rebuilding the complete watcher/VOD/queue/log
        // snapshot for every line multiplies SQLite/process-state reads. Coalesce
        // those bursts while keeping the regular one-second snapshot unchanged.
        let mut next_event_snapshot = Instant::now();
        let mut log_snapshot_pending = false;

        loop {
            let should_snapshot = tokio::select! {
                _ = tick.tick() => {
                    log_snapshot_pending = false;
                    true
                }
                _ = revalidate.tick(), if session_headers.is_some() => {
                    let Some(ref headers) = session_headers else { continue };
                    if state.auth.authorize_session_readonly(headers).is_err() {
                        break;
                    }
                    false
                }
                event = log_events.recv() => match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if Instant::now() >= next_event_snapshot {
                            true
                        } else {
                            log_snapshot_pending = true;
                            false
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
                _ = sleep_until(next_event_snapshot), if log_snapshot_pending => {
                    log_snapshot_pending = false;
                    true
                }
            };

            if !should_snapshot {
                continue;
            }
            if sender.send(Ok(snapshot_event(&state).await)).await.is_err() {
                break;
            }
            next_event_snapshot = Instant::now() + MIN_EVENT_SNAPSHOT_INTERVAL;
        }
    });

    Ok(Sse::new(ReceiverStream::new(receiver)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("phase11-keep-alive"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_buffer_wakes_realtime_subscribers() {
        let logs = LogBuffer::new();
        let mut receiver = logs.subscribe();
        logs.push("realtime-test").await;
        assert!(receiver.recv().await.is_ok());
        assert_eq!(logs.tail(1).await, vec!["realtime-test"]);
    }

    #[test]
    fn event_snapshots_are_coalesced_below_periodic_interval() {
        assert!(MIN_EVENT_SNAPSHOT_INTERVAL > Duration::ZERO);
        assert!(MIN_EVENT_SNAPSHOT_INTERVAL < SNAPSHOT_INTERVAL);
    }

    #[test]
    fn session_revalidation_is_shorter_than_keep_alive_window() {
        assert!(SESSION_REVALIDATE_INTERVAL < Duration::from_secs(15));
    }
}
