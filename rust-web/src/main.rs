use anyhow::Result;
use std::env;
use stream_archive_server::{app_core::StreamArchiveCore, backend::resolve_backend_dir};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
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
    println!("Press Ctrl+C to stop the runtime and owned LIVE/VOD processes.");
    println!();

    if env_flag("STREAM_ARCHIVE_START_WATCHER") {
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

    signal::ctrl_c().await?;
    core.shutdown().await;
    Ok(())
}

fn env_flag(name: &str) -> bool {
    env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "y" | "yes" | "true" | "on"
        )
    })
}
