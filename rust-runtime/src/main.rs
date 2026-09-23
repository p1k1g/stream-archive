use anyhow::Result;
use std::env;
use stream_archive_server::headless::run_headless;

#[tokio::main]
async fn main() -> Result<()> {
    run_headless(env_flag("STREAM_ARCHIVE_START_WATCHER")).await
}

fn env_flag(name: &str) -> bool {
    env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "y" | "yes" | "true" | "on"
        )
    })
}
