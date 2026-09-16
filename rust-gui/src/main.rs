use slint::ComponentHandle;
use stream_archive_server::{app_core::StreamArchiveCore, backend::resolve_backend_dir};

slint::include_modules!();

fn configured(value: Option<&String>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn bind_core_snapshot(ui: &MainWindow, core: &StreamArchiveCore) {
    let state = ui.global::<AppState>();
    let settings = core.settings().unwrap_or_default();
    let vod_tools = core.vod_tool_settings().unwrap_or_default();
    let channel_count = core.channels().map(|channels| channels.len()).unwrap_or(0);

    let streamlink = configured(settings.get("STREAMLINK_PATH"));
    let yt_dlp = configured(vod_tools.get("YT_DLP_PATH"));
    let ffmpeg = configured(vod_tools.get("FFMPEG_PATH"));

    state.set_runtime_ready(true);
    state.set_runtime_status("Shared Rust core ready".into());
    state.set_backend_path(core.backend_dir().display().to_string().into());
    state.set_database_path(core.store().path().display().to_string().into());
    state.set_channel_count(channel_count.to_string().into());
    state.set_tool_summary(
        format!(
            "Streamlink {}  |  yt-dlp {}  |  FFmpeg {}",
            if streamlink { "✓" } else { "-" },
            if yt_dlp { "✓" } else { "-" },
            if ffmpeg { "✓" } else { "-" }
        )
        .into(),
    );
}

fn bind_bootstrap_error(ui: &MainWindow, message: impl Into<String>) {
    let state = ui.global::<AppState>();
    state.set_runtime_ready(false);
    state.set_runtime_status(message.into().into());
    state.set_backend_path("-".into());
    state.set_database_path("-".into());
    state.set_channel_count("0".into());
    state.set_tool_summary("Streamlink -  |  yt-dlp -  |  FFmpeg -".into());
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;

    let core = match resolve_backend_dir().and_then(StreamArchiveCore::open) {
        Ok(opened) => {
            bind_core_snapshot(&ui, &opened.core);
            Some(opened.core)
        }
        Err(err) => {
            bind_bootstrap_error(&ui, format!("Runtime initialization failed: {err:#}"));
            None
        }
    };

    let weak = ui.as_weak();
    ui.global::<AppState>().on_refresh_requested(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if let Some(core) = core.as_ref() {
            bind_core_snapshot(&ui, core);
        } else {
            bind_bootstrap_error(
                &ui,
                "Runtime unavailable; restart after fixing backend configuration",
            );
        }
    });

    ui.run()
}
