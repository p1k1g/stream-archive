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
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::oneshot,
    task::JoinHandle,
};

#[cfg(unix)]
const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;
#[cfg(unix)]
const CONTROL_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
// Command execution can involve graceful owned-process cleanup; do not abort it
// just because a client stops reading or shutdown is requested.
#[cfg(unix)]
const CONTROL_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

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

#[cfg(unix)]
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
                                match handle_connection(stream, &core, &mut shutdown_rx).await {
                                    Ok(true) => break,
                                    Ok(false) => {},
                                    Err(error) => {
                                        core.logs()
                                            .push(format!("[RUNTIME:WARN] local control request failed: {error:#}"))
                                            .await;
                                    }
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
async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut message = Vec::new();
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            if message.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let count = newline.map_or(bytes.len(), |i| i + 1);
        if count > MAX_CONTROL_MESSAGE_BYTES.saturating_sub(message.len()) {
            bail!("runtime control message exceeds size limit");
        }
        message.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    Ok(Some(
        String::from_utf8(message).context("invalid runtime control UTF-8")?,
    ))
}

#[cfg(unix)]
async fn control_io<F, T>(
    io: F,
    shutdown: &mut oneshot::Receiver<()>,
    timeout: std::time::Duration,
) -> Result<Option<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::select! {
        biased;
        _ = shutdown => Ok(None),
        result = tokio::time::timeout(timeout, io) => {
            Ok(Some(result.context("runtime control I/O timed out")??))
        }
    }
}

#[cfg(unix)]
fn encode_response(response: RuntimeControlResponse) -> Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(&response)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_CONTROL_MESSAGE_BYTES {
        encoded = serde_json::to_vec(&RuntimeControlResponse {
            ok: false,
            data: Value::Null,
            error: Some("runtime control response exceeds size limit; request fewer log lines or a smaller snapshot".into()),
        })?;
        encoded.push(b'\n');
    }
    Ok(encoded)
}

#[cfg(unix)]
async fn handle_connection(
    stream: UnixStream,
    core: &StreamArchiveCore,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<bool> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let Some(line) = control_io(read_message(&mut reader), shutdown, CONTROL_IO_TIMEOUT).await?
    else {
        return Ok(true);
    };
    let Some(line) = line else {
        return Ok(false);
    };
    let request: RuntimeControlRequest =
        serde_json::from_str(line.trim_end()).context("invalid runtime control request")?;
    // Finish accepted commands before observing shutdown again. Dropping a
    // stop/cancel future can detach its JoinHandle halfway through cleanup.
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
    let encoded = encode_response(response)?;
    let written = control_io(
        async {
            write_half.write_all(&encoded).await?;
            write_half.shutdown().await?;
            Ok(())
        },
        shutdown,
        CONTROL_IO_TIMEOUT,
    )
    .await?;
    Ok(written.is_none())
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
        "queue.cancel" => {
            let id = request.target.context("queue item id is required")?;
            Ok(serde_json::to_value(core.cancel_queue_item(&id).await?)?)
        }
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
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_CONTROL_MESSAGE_BYTES {
        bail!("runtime control request exceeds size limit");
    }
    let path = runtime_control_socket_path(database_path)?;
    let stream = tokio::time::timeout(CONTROL_IO_TIMEOUT, UnixStream::connect(&path))
        .await
        .context("runtime control connect timed out")?
        .with_context(|| {
            format!(
                "a Stream Archive runtime is active but its control socket is unavailable: {}",
                path.display()
            )
        })?;
    let (read_half, mut write_half) = stream.into_split();
    tokio::time::timeout(CONTROL_IO_TIMEOUT, async {
        write_half.write_all(&encoded).await?;
        write_half.shutdown().await
    })
    .await
    .context("runtime control write timed out")??;
    let mut reader = BufReader::new(read_half);
    let line = tokio::time::timeout(CONTROL_RESPONSE_TIMEOUT, read_message(&mut reader))
        .await
        .context("runtime control response timed out")??
        .context("runtime control socket closed without a response")?;
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
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_oversized_unterminated_message_before_draining_the_payload() {
        let input = vec![b'x'; 4 * 1024 * 1024];
        let mut reader = input.as_slice();
        assert!(
            read_message(&mut reader)
                .await
                .unwrap_err()
                .to_string()
                .contains("size limit")
        );
        // A contiguous oversized slice is rejected before copying any bytes.
        assert_eq!(reader.len(), input.len());
        let mut reader = BufReader::with_capacity(1024, input.as_slice());
        assert!(read_message(&mut reader).await.is_err());
        assert!(reader.get_ref().len() >= input.len() - MAX_CONTROL_MESSAGE_BYTES - 1024);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_message_preserves_utf8_eof_and_exact_wire_limit() {
        let input = "{\"command\":\"logs\",\"target\":\"한글\"}\nextra";
        let mut reader = input.as_bytes();
        assert_eq!(
            read_message(&mut reader).await.unwrap().unwrap(),
            input.split_inclusive('\n').next().unwrap()
        );
        assert_eq!(
            read_message(&mut reader).await.unwrap().as_deref(),
            Some("extra")
        );
        assert!(read_message(&mut reader).await.unwrap().is_none());
        let input = format!("{}\n", "x".repeat(MAX_CONTROL_MESSAGE_BYTES - 1));
        assert_eq!(
            read_message(&mut input.as_bytes())
                .await
                .unwrap()
                .unwrap()
                .len(),
            MAX_CONTROL_MESSAGE_BYTES
        );
        assert!(read_message(&mut [0xff, b'\n'].as_slice()).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_read_and_write_observe_shutdown_and_timeout() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(32);
        let mut reader = BufReader::new(reader);
        writer.write_all(b"partial").await.unwrap();
        let (stop, mut stopped) = oneshot::channel();
        assert!(
            control_io(
                read_message(&mut reader),
                &mut stopped,
                std::time::Duration::from_millis(10)
            )
            .await
            .is_err()
        );
        stop.send(()).unwrap();
        assert!(
            control_io(read_message(&mut reader), &mut stopped, CONTROL_IO_TIMEOUT)
                .await
                .unwrap()
                .is_none()
        );

        let (mut writer, _unread) = tokio::io::duplex(32);
        let (stop, mut stopped) = oneshot::channel();
        let payload = [b'x'; 1024];
        let write = async {
            writer.write_all(&payload).await?;
            Ok(())
        };
        assert!(
            control_io(write, &mut stopped, std::time::Duration::from_millis(10))
                .await
                .is_err()
        );
        stop.send(()).unwrap();
        assert!(
            control_io(
                async {
                    writer.write_all(&payload).await?;
                    Ok(())
                },
                &mut stopped,
                CONTROL_IO_TIMEOUT
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn oversized_response_returns_bounded_error_without_disclosing_payload() {
        let encoded = encode_response(RuntimeControlResponse {
            ok: true,
            data: json!("sensitive".repeat(MAX_CONTROL_MESSAGE_BYTES)),
            error: None,
        })
        .unwrap();
        assert!(encoded.len() <= MAX_CONTROL_MESSAGE_BYTES);
        let response: RuntimeControlResponse = serde_json::from_slice(&encoded).unwrap();
        assert!(!response.ok);
        assert!(response.data.is_null());
        assert!(!String::from_utf8(encoded).unwrap().contains("sensitive"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_control_contract_is_socket_not_http_and_protects_secret_transport() {
        let source = include_str!("unix_control.rs");
        assert!(source.contains("UnixListener"));
        assert!(source.contains("PermissionsExt"));
        assert!(source.contains("0o600"));
        assert!(source.contains("\"channel.password\""));
    }
}
