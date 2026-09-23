#[cfg(unix)]
use crate::{
    app_core::StreamArchiveCore,
    runtime_owner::{runtime_control_socket_path, runtime_owner_active},
};
#[cfg(unix)]
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::oneshot,
    task::JoinHandle,
};

#[cfg(unix)]
const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeControlRequest {
    pub command: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RuntimeControlResponse {
    ok: bool,
    data: Value,
    error: Option<String>,
}

#[cfg(unix)]
pub struct RuntimeControlServer {
    path: PathBuf,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

#[cfg(unix)]
impl RuntimeControlServer {
    pub async fn start(core: StreamArchiveCore) -> Result<Self> {
        if !core.owns_runtime() {
            bail!("runtime control server requires the runtime-owner core");
        }
        let path = runtime_control_socket_path(core.store().path())?;
        if path.exists() {
            fs::remove_file(&path).with_context(|| {
                format!("failed to remove stale runtime socket {}", path.display())
            })?;
        }
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("failed to bind runtime control socket {}", path.display()))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).with_context(|| {
            format!(
                "failed to protect runtime control socket {}",
                path.display()
            )
        })?;

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let task_path = path.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _)) => {
                                if let Err(error) = handle_connection(stream, &core).await {
                                    core.logs()
                                        .push(format!("[RUNTIME:WARN] local control request failed: {error:#}"))
                                        .await;
                                }
                            }
                            Err(error) => {
                                core.logs()
                                    .push(format!("[RUNTIME:WARN] local control accept failed: {error}"))
                                    .await;
                                break;
                            }
                        }
                    }
                }
            }
            let _ = fs::remove_file(task_path);
        });

        Ok(Self {
            path,
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
        })
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
async fn handle_connection(stream: UnixStream, core: &StreamArchiveCore) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
        return Ok(());
    }
    if read > MAX_CONTROL_MESSAGE_BYTES {
        bail!("runtime control request exceeds size limit");
    }
    let request: RuntimeControlRequest =
        serde_json::from_str(line.trim_end()).context("invalid runtime control request")?;
    let response = match execute_request(core, request).await {
        Ok(data) => RuntimeControlResponse {
            ok: true,
            data,
            error: None,
        },
        Err(error) => RuntimeControlResponse {
            ok: false,
            data: Value::Null,
            error: Some(format!("{error:#}")),
        },
    };
    let mut encoded = serde_json::to_vec(&response)?;
    encoded.push(b'\n');
    write_half.write_all(&encoded).await?;
    write_half.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
async fn execute_request(
    core: &StreamArchiveCore,
    request: RuntimeControlRequest,
) -> Result<Value> {
    match request.command.as_str() {
        "runtime.status" => Ok(json!({
            "watcher": core.watcher_status().await?,
            "vod": core.vod_status().await?,
        })),
        "watcher.status" => Ok(serde_json::to_value(core.watcher_status().await?)?),
        "watcher.stop" => Ok(serde_json::to_value(core.stop_watcher().await?)?),
        "channel.action" => {
            let target = request.target.context("channel target is required")?;
            let action = request.action.context("channel action is required")?;
            core.channel_action(target, &action).await?;
            Ok(json!({"accepted": true}))
        }
        "channel.password" => {
            let target = request.target.context("channel target is required")?;
            let secret = request.secret.context("stream password is required")?;
            core.channel_password(target, secret).await?;
            Ok(json!({"accepted": true}))
        }
        "vod.status" => Ok(serde_json::to_value(core.vod_status().await?)?),
        "vod.cancel" => Ok(serde_json::to_value(core.cancel_vod().await?)?),
        "logs" => Ok(serde_json::to_value(
            core.runtime_logs(request.max_lines.unwrap_or(100)).await,
        )?),
        other => bail!("unsupported runtime control command: {other}"),
    }
}

#[cfg(unix)]
pub async fn send_runtime_control(
    database_path: &Path,
    request: RuntimeControlRequest,
) -> Result<Option<Value>> {
    if !runtime_owner_active(database_path)? {
        return Ok(None);
    }
    let path = runtime_control_socket_path(database_path)?;
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "a Stream Archive runtime is active but its control socket is unavailable: {}",
            path.display()
        )
    })?;
    let (read_half, mut write_half) = stream.into_split();
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    write_half.write_all(&encoded).await?;
    write_half.shutdown().await?;

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
        bail!("runtime control socket closed without a response");
    }
    if read > MAX_CONTROL_MESSAGE_BYTES {
        bail!("runtime control response exceeds size limit");
    }
    let response: RuntimeControlResponse =
        serde_json::from_str(line.trim_end()).context("invalid runtime control response")?;
    if response.ok {
        Ok(Some(response.data))
    } else {
        bail!(
            "{}",
            response
                .error
                .unwrap_or_else(|| "runtime control command failed".into())
        )
    }
}

#[cfg(not(unix))]
pub async fn send_runtime_control(
    _database_path: &std::path::Path,
    _request: RuntimeControlRequest,
) -> anyhow::Result<Option<Value>> {
    Ok(None)
}

#[cfg(not(unix))]
pub struct RuntimeControlServer;

#[cfg(not(unix))]
impl RuntimeControlServer {
    pub async fn start(_core: crate::app_core::StreamArchiveCore) -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub async fn shutdown(self) {}
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn unix_control_contract_is_socket_not_http_and_protects_secret_transport() {
        let source = include_str!("unix_control.rs");
        assert!(source.contains("UnixListener"));
        assert!(source.contains("PermissionsExt"));
        assert!(source.contains("0o600"));
        assert!(source.contains("\"channel.password\""));
        assert!(!source.contains("TcpListener"));
        assert!(!source.contains("axum"));
    }
}
