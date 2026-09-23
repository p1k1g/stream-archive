use crate::{app_core::StreamArchiveCore, backend::resolve_backend_dir};
use anyhow::Result;
use std::env;

pub async fn run_headless(watch: bool) -> Result<()> {
    let backend_dir = resolve_backend_dir()?;
    let opened = StreamArchiveCore::open(&backend_dir)?;
    let migrated_legacy_db = opened.migrated_legacy_db;
    let core = opened.core;

    core.logs()
        .push(format!(
            "[RUNTIME] Stream Archive headless runtime ready; backend={} db={}",
            core.backend_dir().display(),
            core.store().path().display()
        ))
        .await;
    if migrated_legacy_db {
        core.logs()
            .push(format!(
                "[DB] migrated legacy database filename to {}",
                core.store().path().display()
            ))
            .await;
    }

    core.spawn_vod_history_sync();
    core.spawn_auto_backup();
    let _ = core.spawn_queue_worker();

    println!();
    println!("Stream Archive headless runtime");
    println!("Version : {}", env!("CARGO_PKG_VERSION"));
    println!("Backend : {}", core.backend_dir().display());
    println!("Data    : {}", core.store().path().display());
    println!("Config  : SQLite");
    println!("Watcher : Rust native");
    println!("Recorder: Rust RecorderManager -> Streamlink");
    println!("VOD     : Rust VodManager -> yt-dlp/ffmpeg");
    println!("Queue   : SQLite persistent FIFO worker");
    println!("Backup  : SQLite online backup + retention");
    println!();
    println!("No HTTP/Web listener is started.");
    println!(
        "Press Ctrl+C (or send SIGTERM on Unix) to stop the runtime and owned LIVE/VOD processes."
    );
    println!();

    if watch {
        match core.start_watcher().await {
            Ok(status) => {
                core.logs()
                    .push(format!(
                        "[RUNTIME] watcher auto-start result: running={}",
                        status.running
                    ))
                    .await;
            }
            Err(error) => {
                core.logs()
                    .push(format!(
                        "[RUNTIME:ERR] watcher auto-start failed: {error:#}"
                    ))
                    .await;
                eprintln!("watcher auto-start failed: {error:#}");
            }
        }
    }

    wait_for_shutdown_signal().await?;
    core.shutdown().await;
    Ok(())
}

pub async fn wait_for_shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {}
        }
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn headless_source_keeps_no_http_listener_contract() {
        let source = include_str!("headless.rs");
        assert!(source.contains("No HTTP/Web listener is started."));
        assert!(!source.contains("axum"));
        assert!(!source.contains("TcpListener"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_headless_source_handles_sigterm() {
        let source = include_str!("headless.rs");
        assert!(source.contains("SignalKind::terminate"));
        assert!(source.contains("core.shutdown().await"));
    }
}
