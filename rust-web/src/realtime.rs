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
    time::{MissedTickBehavior, interval},
};
use tokio_stream::wrappers::ReceiverStream;

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const LOG_LINES: usize = 160;

fn authorize_stream(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(());
    }
    // Native EventSource cannot attach the recovery Bearer header. Session-cookie
    // clients use SSE; recovery-token mode intentionally remains on REST polling.
    state.auth.authorize_session(headers)
}

async fn snapshot_event(state: &AppState) -> Event {
    let (watcher, watcher_error) = match state.watcher.status().await {
        Ok(status) => (status, None),
        Err(err) => (Default::default(), Some(err.to_string())),
    };
    let vod = state.vod.status().await;
    let logs = state.logs.tail(LOG_LINES).await;
    let payload = json!({
        "phase": "phase11-realtime-sse",
        "status": {
            "watcher": watcher,
            "backend_dir": state.backend_dir.display().to_string(),
            "bind": state.bind,
            "phase": "phase11-realtime-sse"
        },
        "vod": vod,
        "logs": {"lines": logs},
        "watcher_error": watcher_error
    });
    Event::default().event("snapshot").data(payload.to_string())
}

pub(crate) async fn api_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    authorize_stream(&headers, &state)?;

    let mut log_events = state.logs.subscribe();
    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(8);
    tokio::spawn(async move {
        let mut tick = interval(SNAPSHOT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                event = log_events.recv() => match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            if sender.send(Ok(snapshot_event(&state).await)).await.is_err() {
                break;
            }
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
}
