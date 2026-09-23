use crate::{
    app_core::StreamArchiveCore,
    backend::resolve_backend_dir,
    headless::{run_headless, wait_for_shutdown_signal},
    history_service::HistoryFilter,
    model::{
        Channel, NativeWatcherStatus, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus,
        VodQueueSnapshot,
    },
    support::platform::PlatformId,
    tool_discovery::{ToolKind, ToolResolution, resolve_tool},
    unix_control::{RuntimeControlRequest, RuntimeControlServer, send_runtime_control},
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::Path,
    time::Duration,
};

const PROVIDER_SETTING_KEYS: &[&str] = &["SOOP_USERNAME", "CLOUDFLARE_WORKER_URL"];
const PROVIDER_SECRET_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
];

pub async fn run_management(command: &str, args: &[String]) -> Result<()> {
    match command {
        "status" => command_status(args).await,
        "settings" => command_settings(args).await,
        "providers" => command_providers(args).await,
        "channels" => command_channels(args).await,
        "watcher" => command_watcher(args).await,
        "vod" => command_vod(args).await,
        "queue" => command_queue(args).await,
        "history" => command_history(args).await,
        "backup" => command_backup(args).await,
        "storage" => command_storage(args).await,
        "logs" => command_logs(args).await,
        other => bail!("unsupported management command: {other}"),
    }
}

pub async fn run_serve(watch: bool) -> Result<()> {
    run_headless(watch).await
}

async fn command_status(args: &[String]) -> Result<()> {
    let json_mode = only_json(args, "status")?;
    let core = open_core()?;
    let diagnostics = core.diagnostics();
    let local_watcher = core.watcher_status().await?;
    let local_vod = core.local_vod_status().await;
    let remote = runtime_control(
        &core,
        RuntimeControlRequest {
            command: "runtime.status".into(),
            target: None,
            action: None,
            secret: None,
            max_lines: None,
        },
    )
    .await?;
    let watcher_value = remote
        .as_ref()
        .and_then(|value| value.get("watcher"))
        .cloned()
        .unwrap_or(serde_json::to_value(&local_watcher)?);
    let vod_value = remote
        .as_ref()
        .and_then(|value| value.get("vod"))
        .cloned()
        .unwrap_or(serde_json::to_value(&local_vod)?);
    let queue = core.queue_snapshot().await?;
    let backup = core.backup_snapshot().await?;
    let secrets = core.configured_secrets()?;
    let settings = environment_settings_map(&core)?;
    let tools = resolve_all(core.backend_dir(), &settings);
    let tool_rows = tools
        .iter()
        .map(|tool| {
            json!({
                "tool": tool.kind.label(),
                "found": tool.found(),
                "source": tool.source.as_str(),
                "path": tool.path.as_ref().map(|path| path.display().to_string()),
            })
        })
        .collect::<Vec<_>>();
    let runtime_owner_active = remote.is_some();

    let value = json!({
        "backend": core.backend_dir().display().to_string(),
        "database": core.store().path().display().to_string(),
        "runtime_ready": diagnostics.runtime_ready,
        "blocking_errors": diagnostics.summary.blocking_errors,
        "runtime_owner_active": runtime_owner_active,
        "watcher": watcher_value,
        "vod": vod_value,
        "queue": {
            "active_id": queue.active_id,
            "queued_count": queue.queued_count,
        },
        "configured_secrets": secrets,
        "tools": tool_rows,
        "backup": backup,
        "scope": if runtime_owner_active { "runtime-owner" } else { "observer" },
    });

    if json_mode {
        print_json(&value)?;
    } else {
        println!("Stream Archive status");
        println!("backend       : {}", core.backend_dir().display());
        println!("database      : {}", core.store().path().display());
        println!("runtime ready : {}", diagnostics.runtime_ready);
        println!("blocking      : {}", diagnostics.summary.blocking_errors);
        println!("runtime owner : {}", yes_no(runtime_owner_active));
        println!(
            "watcher       : {}",
            watcher_value
                .get("running")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        println!(
            "recordings    : {}",
            watcher_value
                .get("recording_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        );
        println!(
            "VOD running   : {}",
            vod_value
                .get("running")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        println!(
            "queue active  : {}",
            queue.active_id.as_deref().unwrap_or("-")
        );
        println!("queue queued  : {}", queue.queued_count);
        println!("backup dir    : {}", backup.directory);
        for tool in tools {
            println!(
                "{:<10} : {} ({})",
                tool.kind.label(),
                if tool.found() { "OK" } else { "MISSING" },
                tool.source
            );
        }
    }
    core.shutdown().await;
    Ok(())
}

async fn command_settings(args: &[String]) -> Result<()> {
    let core = open_core()?;
    match args {
        [action] if action == "show" => print_environment_settings(&core, false)?,
        [action, flag] if action == "show" && flag == "--json" => {
            print_environment_settings(&core, true)?
        }
        [action, key, value] if action == "set" => {
            let updates = BTreeMap::from([(key.clone(), value.clone())]);
            let saved = core.update_environment_settings(&updates).await?;
            println!("updated environment setting: {key}");
            for item in saved.into_iter().filter(|item| item.key == *key) {
                println!("{}={}", item.key, item.value);
            }
        }
        _ => bail!("usage: stream-archive-cli settings show [--json] | settings set <KEY> <VALUE>"),
    }
    core.shutdown().await;
    Ok(())
}

fn print_environment_settings(core: &StreamArchiveCore, json_mode: bool) -> Result<()> {
    let settings = core.environment_settings()?;
    if json_mode {
        print_json(&settings)?;
    } else {
        println!("runtime settings");
        for item in settings {
            println!("{:<24} {}", item.key, item.value);
            println!("  {}", item.description);
        }
    }
    Ok(())
}

async fn command_providers(args: &[String]) -> Result<()> {
    let core = open_core()?;
    match args {
        [action] if action == "status" => print_provider_status(&core, false)?,
        [action, flag] if action == "status" && flag == "--json" => {
            print_provider_status(&core, true)?
        }
        [action, key, value] if action == "set" => {
            if !PROVIDER_SETTING_KEYS.contains(&key.as_str()) {
                bail!(
                    "provider setting must be one of: {}",
                    PROVIDER_SETTING_KEYS.join(", ")
                );
            }
            let settings = BTreeMap::from([(key.clone(), value.clone())]);
            core.update_provider_configuration(&settings, &BTreeMap::new())
                .await?;
            println!("updated provider setting: {key}");
        }
        [action, key, source] if action == "secret" && source == "--stdin" => {
            validate_provider_secret_key(key)?;
            let secret = read_secret_stdin()?;
            let secrets = BTreeMap::from([(key.clone(), secret)]);
            core.update_provider_configuration(&BTreeMap::new(), &secrets)
                .await?;
            println!("updated provider secret: {key}");
        }
        [action] if action == "test-soop" => {
            println!("{}", core.test_soop_auth().await?);
        }
        _ => bail!(
            "usage: stream-archive-cli providers status [--json] | providers set <KEY> <VALUE> | providers secret <KEY> --stdin | providers test-soop"
        ),
    }
    core.shutdown().await;
    Ok(())
}

fn validate_provider_secret_key(key: &str) -> Result<()> {
    if PROVIDER_SECRET_KEYS.contains(&key) {
        Ok(())
    } else {
        bail!(
            "provider secret must be one of: {}",
            PROVIDER_SECRET_KEYS.join(", ")
        )
    }
}

fn print_provider_status(core: &StreamArchiveCore, json_mode: bool) -> Result<()> {
    let settings = core.settings()?;
    let secrets = core.configured_secrets()?;
    let value = json!({
        "SOOP": {
            "username_configured": configured_setting(&settings, "SOOP_USERNAME"),
            "password_configured": configured_secret(&secrets, "SOOP_PASSWORD"),
            "worker_url_configured": configured_setting(&settings, "CLOUDFLARE_WORKER_URL"),
            "worker_key_configured": configured_secret(&secrets, "CLOUDFLARE_API_KEY"),
        },
        "CHZZK": {
            "nid_aut_configured": configured_secret(&secrets, "CHZZK_NID_AUT"),
            "nid_ses_configured": configured_secret(&secrets, "CHZZK_NID_SES"),
        }
    });
    if json_mode {
        print_json(&value)?;
    } else {
        println!("provider configuration");
        println!(
            "SOOP username       : {}",
            yes_no(configured_setting(&settings, "SOOP_USERNAME"))
        );
        println!(
            "SOOP password       : {}",
            yes_no(configured_secret(&secrets, "SOOP_PASSWORD"))
        );
        println!(
            "Cloudflare worker   : {}",
            yes_no(configured_setting(&settings, "CLOUDFLARE_WORKER_URL"))
        );
        println!(
            "Cloudflare API key  : {}",
            yes_no(configured_secret(&secrets, "CLOUDFLARE_API_KEY"))
        );
        println!(
            "CHZZK NID_AUT       : {}",
            yes_no(configured_secret(&secrets, "CHZZK_NID_AUT"))
        );
        println!(
            "CHZZK NID_SES       : {}",
            yes_no(configured_secret(&secrets, "CHZZK_NID_SES"))
        );
    }
    Ok(())
}

async fn command_channels(args: &[String]) -> Result<()> {
    let core = open_core()?;
    match args {
        [action] if action == "list" => print_channels(&core.channels()?, false)?,
        [action, flag] if action == "list" && flag == "--json" => {
            print_channels(&core.channels()?, true)?
        }
        [action, platform, account, name, outdir] if action == "add" => {
            add_channel(&core, platform, account, name, outdir, true).await?;
        }
        [action, platform, account, name, outdir, flag]
            if action == "add" && flag == "--disabled" =>
        {
            add_channel(&core, platform, account, name, outdir, false).await?;
        }
        [action, platform, account] if action == "remove" => {
            mutate_channel(&core, platform, account, ChannelMutation::Remove).await?;
        }
        [action, platform, account] if action == "enable" => {
            mutate_channel(&core, platform, account, ChannelMutation::Enabled(true)).await?;
        }
        [action, platform, account] if action == "disable" => {
            mutate_channel(&core, platform, account, ChannelMutation::Enabled(false)).await?;
        }
        [action, platform, account, verb] if action == "action" => {
            let platform = platform.parse::<PlatformId>()?;
            require_runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "channel.action".into(),
                    target: Some(format!("{platform}:{account}")),
                    action: Some(verb.clone()),
                    secret: None,
                    max_lines: None,
                },
            )
            .await?;
            println!("channel action sent to running watcher: {platform}/{account} {verb}");
        }
        [action, platform, account, source] if action == "password" && source == "--stdin" => {
            let platform = platform.parse::<PlatformId>()?;
            let secret = read_secret_stdin()?;
            require_runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "channel.password".into(),
                    target: Some(format!("{platform}:{account}")),
                    action: None,
                    secret: Some(secret),
                    max_lines: None,
                },
            )
            .await?;
            println!("protected-stream password supplied to running watcher for {platform}/{account}");
        }
        _ => bail!(
            "usage: stream-archive-cli channels list [--json] | channels add <platform> <account> <name> <output-dir> [--disabled] | channels remove|enable|disable <platform> <account> | channels action <platform> <account> <stop|resume|recheck> | channels password <platform> <account> --stdin"
        ),
    }
    core.shutdown().await;
    Ok(())
}

async fn add_channel(
    core: &StreamArchiveCore,
    platform: &str,
    account: &str,
    name: &str,
    outdir: &str,
    enabled: bool,
) -> Result<()> {
    let platform = platform.parse::<PlatformId>()?;
    let mut channels = core.channels()?;
    if channels.iter().any(|channel| {
        channel.platform == platform && channel.account.eq_ignore_ascii_case(account)
    }) {
        bail!("channel already exists: {platform}/{account}");
    }
    channels.push(Channel {
        platform,
        enabled,
        name: name.to_string(),
        account: account.to_string(),
        outdir: outdir.to_string(),
    });
    let saved = core.update_channels(&channels).await?;
    println!(
        "channel added: {platform}/{account} ({} total)",
        saved.len()
    );
    Ok(())
}

enum ChannelMutation {
    Remove,
    Enabled(bool),
}

async fn mutate_channel(
    core: &StreamArchiveCore,
    platform: &str,
    account: &str,
    mutation: ChannelMutation,
) -> Result<()> {
    let platform = platform.parse::<PlatformId>()?;
    let mut channels = core.channels()?;
    let before = channels.len();
    match mutation {
        ChannelMutation::Remove => {
            channels.retain(|channel| {
                !(channel.platform == platform && channel.account.eq_ignore_ascii_case(account))
            });
            if channels.len() == before {
                bail!("channel not found: {platform}/{account}");
            }
        }
        ChannelMutation::Enabled(enabled) => {
            let channel = channels
                .iter_mut()
                .find(|channel| {
                    channel.platform == platform && channel.account.eq_ignore_ascii_case(account)
                })
                .ok_or_else(|| anyhow::anyhow!("channel not found: {platform}/{account}"))?;
            channel.enabled = enabled;
        }
    }
    core.update_channels(&channels).await?;
    println!("channel updated: {platform}/{account}");
    Ok(())
}

fn print_channels(channels: &[Channel], json_mode: bool) -> Result<()> {
    if json_mode {
        print_json(channels)?;
    } else {
        println!("channels");
        for channel in channels {
            println!(
                "{} {:8} {:<24} {:<24} {}",
                if channel.enabled { "ON " } else { "OFF" },
                channel.platform,
                channel.account,
                channel.name,
                channel.outdir
            );
        }
        if channels.is_empty() {
            println!("  <none>");
        }
    }
    Ok(())
}

async fn command_watcher(args: &[String]) -> Result<()> {
    match args {
        [action] if action == "start" => run_headless(true).await,
        [action] if action == "status" => {
            let core = open_core()?;
            if let Some(value) = runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "watcher.status".into(),
                    target: None,
                    action: None,
                    secret: None,
                    max_lines: None,
                },
            )
            .await?
            {
                print_watcher_value(&value, false)?;
            } else {
                let status = core.watcher_status().await?;
                print_watcher(&status, false)?;
            }
            core.shutdown().await;
            Ok(())
        }
        [action, flag] if action == "status" && flag == "--json" => {
            let core = open_core()?;
            if let Some(value) = runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "watcher.status".into(),
                    target: None,
                    action: None,
                    secret: None,
                    max_lines: None,
                },
            )
            .await?
            {
                print_json(&value)?;
            } else {
                let status = core.watcher_status().await?;
                print_watcher(&status, true)?;
            }
            core.shutdown().await;
            Ok(())
        }
        [action] if action == "stop" => {
            let core = open_core()?;
            let value = require_runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "watcher.stop".into(),
                    target: None,
                    action: None,
                    secret: None,
                    max_lines: None,
                },
            )
            .await?;
            print_watcher_value(&value, false)?;
            core.shutdown().await;
            Ok(())
        }
        _ => bail!(
            "usage: stream-archive-cli watcher status [--json] | watcher start | watcher stop"
        ),
    }
}

fn print_watcher_value(value: &Value, json_mode: bool) -> Result<()> {
    if json_mode {
        return print_json(value);
    }
    println!("watcher");
    println!(
        "  running    : {}",
        value.get("running").and_then(Value::as_bool).unwrap_or(false)
    );
    println!(
        "  channels   : {}",
        value.get("channel_count").and_then(Value::as_u64).unwrap_or(0)
    );
    println!(
        "  recordings : {}",
        value
            .get("recording_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    println!(
        "  offline    : {}",
        value.get("offline_count").and_then(Value::as_u64).unwrap_or(0)
    );
    println!(
        "  errors     : {}",
        value.get("error_count").and_then(Value::as_u64).unwrap_or(0)
    );
    Ok(())
}

fn print_watcher(status: &NativeWatcherStatus, json_mode: bool) -> Result<()> {
    if json_mode {
        print_json(status)?;
    } else {
        println!("watcher");
        println!("  running    : {}", status.running);
        println!("  channels   : {}", status.channel_count);
        println!("  recordings : {}", status.recording_count);
        println!("  offline    : {}", status.offline_count);
        println!("  errors     : {}", status.error_count);
        for channel in &status.channels {
            println!(
                "  {} / {} / {} / {}",
                channel.platform, channel.account, channel.name, channel.status
            );
        }
    }
    Ok(())
}

async fn command_vod(args: &[String]) -> Result<()> {
    let Some(action) = args.first().map(String::as_str) else {
        bail!("usage: stream-archive-cli vod <analyze|download|status|cancel> ...");
    };
    match action {
        "status" => {
            let json_mode = only_json(&args[1..], "vod status")?;
            let core = open_core()?;
            if let Some(value) = runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "vod.status".into(),
                    target: None,
                    action: None,
                    secret: None,
                    max_lines: None,
                },
            )
            .await?
            {
                if json_mode {
                    print_json(&value)?;
                } else {
                    print_vod_value(&value)?;
                }
            } else {
                let status = core.local_vod_status().await;
                print_vod_status(&status, json_mode)?;
            }
            core.shutdown().await;
        }
        "cancel" => {
            let json_mode = only_json(&args[1..], "vod cancel")?;
            let core = open_core()?;
            let value = require_runtime_control(
                &core,
                RuntimeControlRequest {
                    command: "vod.cancel".into(),
                    target: None,
                    action: None,
                    secret: None,
                    max_lines: None,
                },
            )
            .await?;
            if json_mode {
                print_json(&value)?;
            } else {
                print_vod_value(&value)?;
            }
            core.shutdown().await;
        }
        "analyze" => {
            let parsed = parse_vod_analyze_args(&args[1..])?;
            let core = open_owner_core()?;
            let control = RuntimeControlServer::start(core.clone()).await?;
            let initial = core.analyze_vod(parsed.request).await?;
            let final_status = await_vod_foreground(&core, initial).await?;
            print_vod_status(&final_status, parsed.json)?;
            control.shutdown().await;
            core.shutdown().await;
            ensure_vod_success(&final_status)?;
        }
        "download" => {
            let parsed = parse_vod_download_args(&args[1..])?;
            let core = open_owner_core()?;
            let control = RuntimeControlServer::start(core.clone()).await?;
            let initial = core.download_vod(parsed.request).await?;
            let final_status = await_vod_foreground(&core, initial).await?;
            print_vod_status(&final_status, parsed.json)?;
            control.shutdown().await;
            core.shutdown().await;
            ensure_vod_success(&final_status)?;
        }
        _ => bail!("usage: stream-archive-cli vod <analyze|download|status|cancel> ..."),
    }
    Ok(())
}

async fn await_vod_foreground(
    core: &StreamArchiveCore,
    initial: VodJobStatus,
) -> Result<VodJobStatus> {
    if !initial.running {
        return Ok(initial);
    }

    let signal = wait_for_shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            result = &mut signal => {
                result?;
                let cancelled = core.cancel_vod().await?;
                core.shutdown().await;
                bail!("VOD operation cancelled by shutdown signal (state={})", cancelled.state);
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                let status = core.vod_status().await?;
                if !status.running {
                    return Ok(status);
                }
            }
        }
    }
}

fn ensure_vod_success(status: &VodJobStatus) -> Result<()> {
    match status.state.as_str() {
        "COMPLETED" | "ANALYZED" => Ok(()),
        "CANCELLED" => bail!("VOD operation cancelled"),
        "FAILED" | "ERROR" => bail!("VOD operation failed: {}", status.message),
        other if !status.running => {
            if status.message.trim().is_empty() {
                Ok(())
            } else {
                bail!("VOD operation ended in state {other}: {}", status.message)
            }
        }
        _ => Ok(()),
    }
}

fn print_vod_value(value: &Value) -> Result<()> {
    println!("VOD");
    println!(
        "  state    : {}",
        value.get("state").and_then(Value::as_str).unwrap_or("IDLE")
    );
    println!(
        "  running  : {}",
        value.get("running").and_then(Value::as_bool).unwrap_or(false)
    );
    println!(
        "  progress : {:.1}%",
        value.get("percent").and_then(Value::as_f64).unwrap_or(0.0)
    );
    if let Some(message) = value.get("message").and_then(Value::as_str)
        && !message.trim().is_empty()
    {
        println!("  message  : {message}");
    }
    if let Some(output) = value.get("output_file").and_then(Value::as_str) {
        println!("  output   : {output}");
    }
    Ok(())
}

fn print_vod_status(status: &VodJobStatus, json_mode: bool) -> Result<()> {
    if json_mode {
        print_json(status)?;
    } else {
        println!("VOD");
        println!("  platform : {}", status.platform);
        println!("  state    : {}", status.state);
        println!("  running  : {}", status.running);
        println!("  progress : {:.1}%", status.percent);
        println!("  part     : {}/{}", status.current_part, status.part_count);
        if !status.message.trim().is_empty() {
            println!("  message  : {}", status.message);
        }
        if let Some(output) = &status.output_file {
            println!("  output   : {output}");
        }
        if let Some(analysis) = &status.analysis {
            println!("  title    : {}", analysis.title);
            println!("  streamer : {}", analysis.streamer);
            println!("  parts    : {}", analysis.part_count);
        }
    }
    Ok(())
}

struct ParsedAnalyze {
    request: VodAnalyzeRequest,
    json: bool,
}

fn parse_vod_analyze_args(args: &[String]) -> Result<ParsedAnalyze> {
    let Some(url) = args.first() else {
        bail!("usage: stream-archive-cli vod analyze <URL> [--json] [VOD auth/tool options]");
    };
    if url.starts_with("--") {
        bail!("VOD URL is required before options");
    }
    let mut request = VodAnalyzeRequest {
        vod_url: url.clone(),
        cookie_mode: "SOOP_LOGIN".into(),
        cookie_file: String::new(),
        browser_name: "firefox".into(),
        yt_dlp_path: String::new(),
        ffmpeg_path: String::new(),
        max_retries: 5,
    };
    let mut json_mode = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--json" if !json_mode => {
                json_mode = true;
                index += 1;
            }
            "--json" => bail!("duplicate option: --json"),
            "--cookie-mode" => {
                request.cookie_mode = option_value(args, &mut index, "--cookie-mode")?;
            }
            "--cookie-file" => {
                request.cookie_file = option_value(args, &mut index, "--cookie-file")?;
            }
            "--browser" => {
                request.browser_name = option_value(args, &mut index, "--browser")?;
            }
            "--max-retries" => {
                request.max_retries = option_value(args, &mut index, "--max-retries")?
                    .parse()
                    .context("--max-retries must be an integer")?;
            }
            "--yt-dlp" => {
                request.yt_dlp_path = option_value(args, &mut index, "--yt-dlp")?;
            }
            "--ffmpeg" => {
                request.ffmpeg_path = option_value(args, &mut index, "--ffmpeg")?;
            }
            other => bail!("unsupported vod analyze option: {other}"),
        }
    }
    Ok(ParsedAnalyze {
        request,
        json: json_mode,
    })
}

struct ParsedDownload {
    request: VodDownloadRequest,
    json: bool,
}

fn parse_vod_download_args(args: &[String]) -> Result<ParsedDownload> {
    let Some(url) = args.first() else {
        bail!("VOD URL is required");
    };
    if url.starts_with("--") {
        bail!("VOD URL is required before options");
    }
    let mut output = None;
    let mut quality = "best".to_string();
    let mut parts = Vec::new();
    let mut merge = true;
    let mut cookie_mode = "SOOP_LOGIN".to_string();
    let mut cookie_file = String::new();
    let mut browser_name = "firefox".to_string();
    let mut yt_dlp_path = String::new();
    let mut ffmpeg_path = String::new();
    let mut max_retries = 5u32;
    let mut json_mode = false;
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--json" if !json_mode => {
                json_mode = true;
                index += 1;
            }
            "--json" => bail!("duplicate option: --json"),
            "--output" => output = Some(option_value(args, &mut index, "--output")?),
            "--quality" => quality = option_value(args, &mut index, "--quality")?,
            "--parts" => {
                parts = parse_parts(&option_value(args, &mut index, "--parts")?)?;
            }
            "--no-merge" => {
                merge = false;
                index += 1;
            }
            "--cookie-mode" => {
                cookie_mode = option_value(args, &mut index, "--cookie-mode")?;
            }
            "--cookie-file" => {
                cookie_file = option_value(args, &mut index, "--cookie-file")?;
            }
            "--browser" => browser_name = option_value(args, &mut index, "--browser")?,
            "--max-retries" => {
                max_retries = option_value(args, &mut index, "--max-retries")?
                    .parse()
                    .context("--max-retries must be an integer")?;
            }
            "--yt-dlp" => yt_dlp_path = option_value(args, &mut index, "--yt-dlp")?,
            "--ffmpeg" => ffmpeg_path = option_value(args, &mut index, "--ffmpeg")?,
            other => bail!("unsupported VOD download option: {other}"),
        }
    }

    let output_directory = output.context("--output <DIR> is required")?;
    Ok(ParsedDownload {
        request: VodDownloadRequest {
            vod_url: url.clone(),
            output_directory,
            parts,
            quality,
            merge,
            cookie_mode,
            cookie_file,
            browser_name,
            yt_dlp_path,
            ffmpeg_path,
            max_retries,
        },
        json: json_mode,
    })
}

fn option_value(args: &[String], index: &mut usize, name: &str) -> Result<String> {
    let value = args
        .get(*index + 1)
        .cloned()
        .with_context(|| format!("{name} requires a value"))?;
    *index += 2;
    Ok(value)
}

fn parse_parts(raw: &str) -> Result<Vec<usize>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    raw.split(',')
        .map(|value| {
            let part = value
                .trim()
                .parse::<usize>()
                .with_context(|| format!("invalid VOD part: {value}"))?;
            if part == 0 {
                bail!("VOD part numbers start at 1");
            }
            Ok(part)
        })
        .collect()
}

async fn command_queue(args: &[String]) -> Result<()> {
    let Some(action) = args.first().map(String::as_str) else {
        bail!("usage: stream-archive-cli queue <list|add|cancel|retry|remove> ...");
    };
    let core = open_core()?;
    match action {
        "list" => {
            let json_mode = only_json(&args[1..], "queue list")?;
            let snapshot = core.queue_snapshot().await?;
            print_queue(&snapshot, json_mode)?;
        }
        "add" => {
            let parsed = parse_vod_download_args(&args[1..])?;
            let item = core.enqueue_vod(parsed.request).await?;
            if parsed.json {
                print_json(&item)?;
            } else {
                println!("queued VOD: {} {}", item.id, item.vod_url);
            }
        }
        "cancel" | "retry" | "remove" => {
            let (id, json_mode) = parse_id_and_json(&args[1..], &format!("queue {action}"))?;
            let snapshot = match action {
                "cancel" => core.cancel_queue_item(id).await?,
                "retry" => core.retry_queue_item(id).await?,
                "remove" => core.remove_queue_item(id).await?,
                _ => unreachable!(),
            };
            print_queue(&snapshot, json_mode)?;
        }
        _ => bail!("usage: stream-archive-cli queue <list|add|cancel|retry|remove> ..."),
    }
    core.shutdown().await;
    Ok(())
}

fn print_queue(snapshot: &VodQueueSnapshot, json_mode: bool) -> Result<()> {
    if json_mode {
        print_json(snapshot)?;
    } else {
        println!("VOD queue");
        println!("active : {}", snapshot.active_id.as_deref().unwrap_or("-"));
        println!("queued : {}", snapshot.queued_count);
        for item in &snapshot.items {
            println!(
                "{} {:<10} {:>6.1}% {}",
                item.id, item.state, item.percent, item.vod_url
            );
        }
    }
    Ok(())
}

async fn command_history(args: &[String]) -> Result<()> {
    let Some(action) = args.first().map(String::as_str) else {
        bail!("usage: stream-archive-cli history list [filters]");
    };
    if action != "list" {
        bail!("usage: stream-archive-cli history list [filters]");
    }
    let (filter, json_mode) = parse_history_args(&args[1..])?;
    let core = open_core()?;
    let history = core.history(&filter)?;
    if json_mode {
        print_json(&history)?;
    } else {
        println!("LIVE history ({})", history.live.len());
        for item in &history.live {
            println!(
                "{} {} {:<10} {} {}",
                item.started_at,
                item.platform,
                item.status,
                item.channel_name,
                item.title.as_deref().unwrap_or("")
            );
        }
        println!("VOD history ({})", history.vod.len());
        for item in &history.vod {
            println!(
                "{} {:<10} {} {}",
                item.started_at.as_deref().unwrap_or("-"),
                item.state,
                item.streamer,
                item.title
            );
        }
    }
    core.shutdown().await;
    Ok(())
}

fn parse_history_args(args: &[String]) -> Result<(HistoryFilter, bool)> {
    let mut filter = HistoryFilter::default();
    let mut json_mode = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" if !json_mode => {
                json_mode = true;
                index += 1;
            }
            "--json" => bail!("duplicate option: --json"),
            "--q" => filter.q = Some(option_value(args, &mut index, "--q")?),
            "--status" => filter.status = Some(option_value(args, &mut index, "--status")?),
            "--from" => filter.from = Some(option_value(args, &mut index, "--from")?),
            "--to" => filter.to = Some(option_value(args, &mut index, "--to")?),
            "--limit" => {
                filter.limit = Some(
                    option_value(args, &mut index, "--limit")?
                        .parse()
                        .context("--limit must be an integer")?,
                );
            }
            other => bail!("unsupported history option: {other}"),
        }
    }
    filter.normalized()?;
    Ok((filter, json_mode))
}

async fn command_backup(args: &[String]) -> Result<()> {
    let Some(action) = args.first().map(String::as_str) else {
        bail!("usage: stream-archive-cli backup <status|create|restore> ...");
    };
    let core = open_core()?;
    match action {
        "status" => {
            let json_mode = only_json(&args[1..], "backup status")?;
            let snapshot = core.backup_snapshot().await?;
            print_backup(&snapshot, json_mode)?;
        }
        "create" => {
            let json_mode = only_json(&args[1..], "backup create")?;
            let snapshot = core.create_manual_backup().await?;
            print_backup(&snapshot, json_mode)?;
        }
        "restore" => {
            let (file_name, json_mode) = parse_restore_args(&args[1..])?;
            let result = core.restore_backup(file_name).await?;
            if json_mode {
                print_json(&result)?;
            } else {
                println!("restored backup: {}", result.restored.file_name);
                println!("safety backup : {}", result.safety_backup.file_name);
            }
        }
        _ => bail!("usage: stream-archive-cli backup <status|create|restore> ..."),
    }
    core.shutdown().await;
    Ok(())
}

fn parse_restore_args(args: &[String]) -> Result<(&str, bool)> {
    let Some(file_name) = args.first() else {
        bail!("usage: stream-archive-cli backup restore <FILE-NAME> --yes [--json]");
    };
    let mut yes = false;
    let mut json_mode = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--yes" if !yes => yes = true,
            "--json" if !json_mode => json_mode = true,
            "--yes" | "--json" => bail!("duplicate backup restore option: {arg}"),
            _ => bail!("usage: stream-archive-cli backup restore <FILE-NAME> --yes [--json]"),
        }
    }
    if !yes {
        bail!("backup restore is destructive; pass --yes after verifying the backup");
    }
    Ok((file_name, json_mode))
}

fn print_backup(snapshot: &crate::backup_service::BackupSnapshot, json_mode: bool) -> Result<()> {
    if json_mode {
        print_json(snapshot)?;
    } else {
        println!("backup");
        println!("  directory      : {}", snapshot.directory);
        println!("  enabled        : {}", snapshot.policy.enabled);
        println!("  interval hours : {}", snapshot.policy.interval_hours);
        println!("  keep count     : {}", snapshot.policy.keep_count);
        println!("  retention days : {}", snapshot.policy.retention_days);
        for item in &snapshot.backups {
            println!(
                "  {} {} {} bytes {}",
                item.created_at, item.file_name, item.size_bytes, item.integrity
            );
        }
    }
    Ok(())
}

async fn command_storage(args: &[String]) -> Result<()> {
    let json_mode = only_json(args, "storage")?;
    let core = open_core()?;
    let snapshot = core.storage_snapshot()?;
    if json_mode {
        print_json(&snapshot)?;
    } else {
        println!("storage");
        println!("database bytes : {}", snapshot.database_size_bytes);
        println!("threshold GB   : {}", snapshot.threshold_gb);
        for volume in &snapshot.volumes {
            println!(
                "{:<8} free={} total={} used={:.1}% roles={}",
                volume.status,
                volume.free_bytes,
                volume.total_bytes,
                volume.used_percent,
                volume.roles.join(", ")
            );
            for path in &volume.paths {
                println!("  {path}");
            }
            if let Some(error) = &volume.error {
                println!("  error: {error}");
            }
        }
    }
    core.shutdown().await;
    Ok(())
}

async fn command_logs(args: &[String]) -> Result<()> {
    let (tail, json_mode) = parse_logs_args(args)?;
    let core = open_core()?;
    let logs = if let Some(value) = runtime_control(
        &core,
        RuntimeControlRequest {
            command: "logs".into(),
            target: None,
            action: None,
            secret: None,
            max_lines: Some(tail),
        },
    )
    .await?
    {
        serde_json::from_value::<Vec<String>>(value)
            .context("invalid runtime log response")?
    } else {
        core.runtime_logs(tail).await
    };
    if json_mode {
        print_json(&logs)?;
    } else {
        for line in logs {
            println!("{line}");
        }
    }
    core.shutdown().await;
    Ok(())
}

fn parse_logs_args(args: &[String]) -> Result<(usize, bool)> {
    let mut tail = 100usize;
    let mut json_mode = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" if !json_mode => {
                json_mode = true;
                index += 1;
            }
            "--json" => bail!("duplicate option: --json"),
            "--tail" => {
                tail = option_value(args, &mut index, "--tail")?
                    .parse()
                    .context("--tail must be an integer")?;
                if tail == 0 {
                    bail!("--tail must be at least 1");
                }
            }
            other => bail!("unsupported logs option: {other}"),
        }
    }
    Ok((tail, json_mode))
}

fn open_core() -> Result<StreamArchiveCore> {
    let backend = resolve_backend_dir()?;
    Ok(StreamArchiveCore::open_observer(&backend)?.core)
}

fn open_owner_core() -> Result<StreamArchiveCore> {
    let backend = resolve_backend_dir()?;
    Ok(StreamArchiveCore::open(&backend)?.core)
}

async fn runtime_control(
    core: &StreamArchiveCore,
    request: RuntimeControlRequest,
) -> Result<Option<Value>> {
    send_runtime_control(core.store().path(), request).await
}

async fn require_runtime_control(
    core: &StreamArchiveCore,
    request: RuntimeControlRequest,
) -> Result<Value> {
    runtime_control(core, request).await?.ok_or_else(|| {
        anyhow::anyhow!(
            "no running Stream Archive runtime owns this data directory; start `stream-archive-cli serve` or `watcher start` first"
        )
    })
}

fn environment_settings_map(core: &StreamArchiveCore) -> Result<BTreeMap<String, String>> {
    Ok(core
        .environment_settings()?
        .into_iter()
        .map(|item| (item.key, item.value))
        .collect())
}

fn resolve_all(backend: &Path, settings: &BTreeMap<String, String>) -> Vec<ToolResolution> {
    ToolKind::ALL
        .into_iter()
        .map(|kind| {
            let configured = kind
                .setting_keys()
                .iter()
                .map(|key| (*key, settings.get(*key).map(String::as_str).unwrap_or("")))
                .collect::<Vec<_>>();
            resolve_tool(kind, backend, &configured)
        })
        .collect()
}

fn configured_setting(settings: &BTreeMap<String, String>, key: &str) -> bool {
    settings
        .get(key)
        .is_some_and(|value| !value.trim().is_empty())
}

fn configured_secret(secrets: &BTreeMap<String, bool>, key: &str) -> bool {
    secrets.get(key).copied().unwrap_or(false)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn read_secret_stdin() -> Result<String> {
    let mut value = String::new();
    io::stdin()
        .read_to_string(&mut value)
        .context("failed to read secret from stdin")?;
    while value.ends_with('\r') || value.ends_with('\n') {
        value.pop();
    }
    if value.is_empty() {
        bail!("secret input from stdin is empty");
    }
    Ok(value)
}

fn only_json(args: &[String], command: &str) -> Result<bool> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--json" => Ok(true),
        _ => bail!("usage: stream-archive-cli {command} [--json]"),
    }
}

fn parse_id_and_json<'a>(args: &'a [String], command: &str) -> Result<(&'a str, bool)> {
    match args {
        [id] => Ok((id, false)),
        [id, flag] if flag == "--json" => Ok((id, true)),
        _ => bail!("usage: stream-archive-cli {command} <ID> [--json]"),
    }
}

fn print_json(value: &(impl Serialize + ?Sized)) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_secret_keys_are_explicit_and_values_never_part_of_status() {
        assert!(validate_provider_secret_key("SOOP_PASSWORD").is_ok());
        assert!(validate_provider_secret_key("CHZZK_NID_AUT").is_ok());
        assert!(validate_provider_secret_key("NOT_A_SECRET").is_err());

        let settings = BTreeMap::from([
            ("SOOP_USERNAME".into(), "private-user".into()),
            (
                "CLOUDFLARE_WORKER_URL".into(),
                "https://worker.example".into(),
            ),
        ]);
        let secrets = BTreeMap::from([("SOOP_PASSWORD".into(), true)]);
        let value = json!({
            "username_configured": configured_setting(&settings, "SOOP_USERNAME"),
            "password_configured": configured_secret(&secrets, "SOOP_PASSWORD"),
        });
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(!encoded.contains("private-user"));
        assert!(!encoded.contains("worker.example"));
    }

    #[test]
    fn parses_vod_parts_and_rejects_zero() {
        assert_eq!(parse_parts("1, 2,3").unwrap(), vec![1, 2, 3]);
        assert!(parse_parts("0").is_err());
        assert!(parse_parts("x").is_err());
    }

    #[test]
    fn parses_download_options_with_unicode_path_and_argument_boundaries() {
        let args = vec![
            "https://vod.sooplive.com/player/123".into(),
            "--output".into(),
            "/tmp/Stream Archive 테스트 🎬".into(),
            "--quality".into(),
            "best[height<=1080]".into(),
            "--parts".into(),
            "1,3".into(),
            "--no-merge".into(),
            "--json".into(),
        ];
        let parsed = parse_vod_download_args(&args).unwrap();
        assert_eq!(
            parsed.request.output_directory,
            "/tmp/Stream Archive 테스트 🎬"
        );
        assert_eq!(parsed.request.parts, vec![1, 3]);
        assert!(!parsed.request.merge);
        assert!(parsed.json);
    }

    #[test]
    fn restore_requires_explicit_confirmation() {
        assert!(parse_restore_args(&["backup.db".into()]).is_err());
        assert_eq!(
            parse_restore_args(&["backup.db".into(), "--yes".into()])
                .unwrap()
                .0,
            "backup.db"
        );
    }

    #[test]
    fn json_flag_contract_rejects_unknown_output_options() {
        assert!(!only_json(&[], "status").unwrap());
        assert!(only_json(&["--json".into()], "status").unwrap());
        assert!(only_json(&["--yaml".into()], "status").is_err());
    }
}
