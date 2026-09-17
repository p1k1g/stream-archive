use crate::{
    AppState, ChannelConfigRow, DiagnosticRow, LiveChannelRow, MainWindow, SettingRow,
    channels_adapter::ChannelsDraft, live_adapter, native_picker, settings_adapter::SettingsDraft,
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
use stream_archive_server::{
    app_core::StreamArchiveCore,
    backend::resolve_backend_dir,
    diagnostics::DiagnosticsSnapshot,
    environment_settings::{EnvironmentSetting, SettingKind},
    model::{Channel, NativeWatcherStatus},
    support::platform::PlatformId,
};

enum Request {
    Refresh,
    Reload,
    Save(BTreeMap<String, String>),
    Pick(usize, SettingKind, String),
    ConfigReload,
    ChannelsSave(Vec<Channel>),
    ChannelResolve {
        index: usize,
        platform: PlatformId,
        account: String,
    },
    ProviderSave {
        username: String,
        worker_url: String,
        secrets: BTreeMap<String, String>,
    },
    ProviderTestSoop,
    LiveStatus {
        poll: bool,
    },
    LiveStart,
    LiveStop,
    LiveAction {
        target: String,
        action: String,
    },
    LivePassword {
        target: String,
        password: String,
    },
}

enum Response {
    Snapshot {
        fields: Option<Vec<EnvironmentSetting>>,
        diagnostics: DiagnosticsSnapshot,
        backend: String,
        database: String,
        channel_count: String,
        message: String,
    },
    Configuration {
        channels: Vec<Channel>,
        username: String,
        worker_url: String,
        secrets: BTreeMap<String, bool>,
        message: String,
    },
    ChannelResolved {
        index: usize,
        account: String,
        name: String,
    },
    ConfigMessage(String),
    ConfigError(String),
    Picked(usize, Option<String>),
    Live {
        status: NativeWatcherStatus,
        message: Option<String>,
        poll: bool,
    },
    LiveError {
        message: String,
        poll: bool,
    },
    Error(String),
}

pub struct Controller {
    _response_timer: Timer,
    _live_poll_timer: Timer,
}

fn read_snapshot(core: &StreamArchiveCore, include_settings: bool, message: &str) -> Response {
    let fields = if include_settings {
        match core.environment_settings() {
            Ok(fields) => Some(fields),
            Err(error) => return Response::Error(format!("Settings load failed: {error:#}")),
        }
    } else {
        None
    };
    Response::Snapshot {
        fields,
        diagnostics: core.diagnostics(),
        backend: core.backend_dir().display().to_string(),
        database: core.store().path().display().to_string(),
        channel_count: core
            .channels()
            .map(|c| c.len().to_string())
            .unwrap_or_else(|_| "Unavailable".into()),
        message: message.into(),
    }
}

fn configuration_snapshot(core: &StreamArchiveCore, message: &str) -> Response {
    let settings = match core.settings() {
        Ok(settings) => settings,
        Err(error) => {
            return Response::ConfigError(format!("Provider settings load failed: {error:#}"));
        }
    };
    let channels = match core.channels() {
        Ok(channels) => channels,
        Err(error) => {
            return Response::ConfigError(format!("Channel list load failed: {error:#}"));
        }
    };
    let secrets = match core.configured_secrets() {
        Ok(secrets) => secrets,
        Err(error) => {
            return Response::ConfigError(format!("Secret status load failed: {error:#}"));
        }
    };
    Response::Configuration {
        channels,
        username: settings.get("SOOP_USERNAME").cloned().unwrap_or_default(),
        worker_url: settings
            .get("CLOUDFLARE_WORKER_URL")
            .cloned()
            .unwrap_or_default(),
        secrets,
        message: message.into(),
    }
}

fn live_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
) -> Response {
    match runtime.block_on(core.watcher_status()) {
        Ok(status) => Response::Live {
            status,
            message: None,
            poll,
        },
        Err(error) => Response::LiveError {
            message: format!("LIVE status refresh failed: {error:#}"),
            poll,
        },
    }
}

fn save_channels(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    mut channels: Vec<Channel>,
) -> Response {
    for channel in &mut channels {
        if channel.name.trim().is_empty() && !channel.account.trim().is_empty() {
            match runtime.block_on(core.resolve_channel_name(channel.platform, &channel.account)) {
                Ok(name) => channel.name = name,
                Err(error) => {
                    return Response::ConfigError(format!(
                        "Channel name lookup failed for {}/{}: {error:#}",
                        channel.platform, channel.account
                    ));
                }
            }
        }
    }

    match runtime.block_on(core.update_channels(&channels)) {
        Ok(_) => configuration_snapshot(
            core,
            "Channel list saved. A running watcher will pick up the canonical list through its normal reload policy.",
        ),
        Err(error) => Response::ConfigError(format!("Channel list was not saved: {error:#}")),
    }
}

fn worker(requests: mpsc::Receiver<Request>, responses: mpsc::Sender<Response>) {
    // Watcher/VOD operations spawn long-lived Tokio tasks. A single-worker
    // multi-thread runtime keeps those tasks moving between GUI requests while
    // retaining one dedicated native-runtime worker for controller requests.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = responses.send(Response::Error(format!(
                "Worker initialization failed: {error}"
            )));
            return;
        }
    };
    let backend = resolve_backend_dir();
    let backend_path = backend.as_ref().ok().cloned();
    let core = match backend.and_then(StreamArchiveCore::open) {
        Ok(opened) => opened.core,
        Err(error) => {
            let message = format!("Runtime initialization failed: {error:#}");
            let _ = responses.send(Response::Snapshot {
                fields: None,
                diagnostics: DiagnosticsSnapshot::startup_failure(
                    backend_path.as_deref(),
                    &message,
                ),
                backend: backend_path
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "Unavailable".into()),
                database: "Unavailable".into(),
                channel_count: "Unavailable".into(),
                message,
            });
            return;
        }
    };
    if responses
        .send(read_snapshot(
            &core,
            true,
            "Settings loaded from canonical SQLite. Save applies changes to future runtime operations.",
        ))
        .is_err()
    {
        return;
    }
    if responses
        .send(configuration_snapshot(
            &core,
            "Channels and provider credentials loaded from canonical SQLite.",
        ))
        .is_err()
    {
        return;
    }
    if responses.send(live_status(&core, &runtime, false)).is_err() {
        return;
    }

    for request in requests {
        let response = match request {
            Request::Refresh => read_snapshot(
                &core,
                false,
                "Diagnostics refreshed; unsaved edits retained",
            ),
            Request::Reload => {
                read_snapshot(&core, true, "Saved settings reloaded; draft discarded")
            }
            Request::Save(patch) => {
                match runtime.block_on(core.update_environment_settings(&patch)) {
                    Ok(_) => read_snapshot(
                        &core,
                        true,
                        "Saved to canonical SQLite. Active recordings keep their current configuration.",
                    ),
                    Err(error) => Response::Error(format!("Not saved: {error:#}")),
                }
            }
            Request::Pick(index, kind, initial) => match native_picker::pick(kind, &initial) {
                Ok(path) => Response::Picked(index, path),
                Err(error) => Response::Error(format!("Picker failed: {error}")),
            },
            Request::ConfigReload => configuration_snapshot(
                &core,
                "Saved channels and provider credentials reloaded; drafts discarded.",
            ),
            Request::ChannelsSave(channels) => save_channels(&core, &runtime, channels),
            Request::ChannelResolve {
                index,
                platform,
                account,
            } => match runtime.block_on(core.resolve_channel_name(platform, &account)) {
                Ok(name) => Response::ChannelResolved {
                    index,
                    account,
                    name,
                },
                Err(error) => Response::ConfigError(format!(
                    "Channel name lookup failed for {platform}/{account}: {error:#}"
                )),
            },
            Request::ProviderSave {
                username,
                worker_url,
                secrets,
            } => {
                let settings = BTreeMap::from([
                    ("SOOP_USERNAME".into(), username),
                    ("CLOUDFLARE_WORKER_URL".into(), worker_url),
                ]);
                match runtime.block_on(core.update_provider_configuration(&settings, &secrets)) {
                    Ok(_) => configuration_snapshot(
                        &core,
                        "Provider configuration saved. Secret values remain encrypted and are not read back into the UI.",
                    ),
                    Err(error) => Response::ConfigError(format!(
                        "Provider configuration was not saved: {error:#}"
                    )),
                }
            }
            Request::ProviderTestSoop => match runtime.block_on(core.test_soop_auth()) {
                Ok(message) => Response::ConfigMessage(message),
                Err(error) => {
                    Response::ConfigError(format!("SOOP / Worker test failed: {error:#}"))
                }
            },
            Request::LiveStatus { poll } => live_status(&core, &runtime, poll),
            Request::LiveStart => match runtime.block_on(core.start_watcher()) {
                Ok(status) => Response::Live {
                    status,
                    message: Some("LIVE watcher started".into()),
                    poll: false,
                },
                Err(error) => Response::LiveError {
                    message: format!("Watcher start failed: {error:#}"),
                    poll: false,
                },
            },
            Request::LiveStop => match runtime.block_on(core.stop_watcher()) {
                Ok(status) => Response::Live {
                    status,
                    message: Some("LIVE watcher stopped".into()),
                    poll: false,
                },
                Err(error) => Response::LiveError {
                    message: format!("Watcher stop failed: {error:#}"),
                    poll: false,
                },
            },
            Request::LiveAction { target, action } => {
                let Some(action) = live_adapter::validated_action(&action) else {
                    let _ = responses.send(Response::LiveError {
                        message: "Unsupported LIVE channel action".into(),
                        poll: false,
                    });
                    continue;
                };
                match runtime.block_on(core.channel_action(target, action)) {
                    Ok(()) => match runtime.block_on(core.watcher_status()) {
                        Ok(status) => Response::Live {
                            status,
                            message: Some(match action {
                                "stop" => "Current broadcast suppressed until it changes".into(),
                                "resume" => "Channel monitoring resumed".into(),
                                _ => "Channel recheck requested".into(),
                            }),
                            poll: false,
                        },
                        Err(error) => Response::LiveError {
                            message: format!(
                                "Action succeeded but status refresh failed: {error:#}"
                            ),
                            poll: false,
                        },
                    },
                    Err(error) => Response::LiveError {
                        message: format!("Channel action failed: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::LivePassword { target, password } => {
                match runtime.block_on(core.channel_password(target, password)) {
                    Ok(()) => match runtime.block_on(core.watcher_status()) {
                        Ok(status) => Response::Live {
                            status,
                            message: Some(
                                "Password supplied in memory and channel recheck requested".into(),
                            ),
                            poll: false,
                        },
                        Err(error) => Response::LiveError {
                            message: format!(
                                "Password accepted but status refresh failed: {error:#}"
                            ),
                            poll: false,
                        },
                    },
                    Err(error) => Response::LiveError {
                        message: format!("Password was not accepted: {error:#}"),
                        poll: false,
                    },
                }
            }
        };
        if responses.send(response).is_err() {
            break;
        }
    }

    runtime.block_on(core.shutdown());
}

fn render_draft(ui: &MainWindow, draft: &SettingsDraft) {
    let rows: Vec<_> = draft
        .fields
        .iter()
        .map(|field| SettingRow {
            key: field.key.clone().into(),
            value: field.value.clone().into(),
            description: field.description.clone().into(),
            pickable: field.kind != SettingKind::Text,
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_settings_rows(ModelRc::new(VecModel::from(rows)));
    state.set_settings_dirty(!draft.patch().is_empty());
}

fn render_channels(ui: &MainWindow, draft: &ChannelsDraft) {
    let rows: Vec<_> = draft
        .rows
        .iter()
        .map(|channel| ChannelConfigRow {
            platform: channel.platform.to_string().into(),
            enabled: channel.enabled,
            name: channel.name.clone().into(),
            account: channel.account.clone().into(),
            outdir: channel.outdir.clone().into(),
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_channel_config_rows(ModelRc::new(VecModel::from(rows)));
    state.set_channels_dirty(draft.dirty());
}

fn render_live(ui: &MainWindow, status: NativeWatcherStatus) {
    let view = live_adapter::view(status);
    let rows: Vec<_> = view
        .channels
        .into_iter()
        .map(|row| LiveChannelRow {
            target: row.target.into(),
            platform: row.platform.into(),
            name: row.name.into(),
            account: row.account.into(),
            status: row.status.into(),
            status_label: row.status_label.into(),
            status_tone: row.status_tone.into(),
            title: row.title.into(),
            bno: row.bno.into(),
            file: row.file.into(),
            size: row.size.into(),
            started_at: row.started_at.into(),
            suppressed: row.suppressed,
            detail: row.detail.into(),
            can_stop_once: row.can_stop_once,
            can_resume: row.can_resume,
            can_recheck: row.can_recheck,
            password_required: row.password_required,
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_live_running(view.running);
    state.set_live_state(view.state_label.into());
    state.set_live_engine(view.engine.into());
    state.set_live_started_at(view.started_at.into());
    state.set_live_channel_count(view.channel_count.into());
    state.set_live_recording_count(view.recording_count.into());
    state.set_live_offline_count(view.offline_count.into());
    state.set_live_error_count(view.error_count.into());
    state.set_live_rows(ModelRc::new(VecModel::from(rows)));
    state.set_live_loaded(true);
}

pub fn bind_core_snapshot(ui: &MainWindow, diagnostics: DiagnosticsSnapshot) {
    let state = ui.global::<AppState>();
    state.set_runtime_ready(diagnostics.runtime_ready);
    state.set_runtime_status(format!("Environment: {}", diagnostics.status.label()).into());
    state.set_tool_summary("See Settings diagnostics for media-tool availability".into());
    let rows: Vec<_> = diagnostics
        .items
        .into_iter()
        .map(|item| DiagnosticRow {
            name: item.name.into(),
            status: item.status.label().into(),
            detail: item.detail.into(),
        })
        .collect();
    state.set_diagnostics_rows(ModelRc::new(VecModel::from(rows)));
}

fn send_settings(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_settings_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_settings_busy(true);
            state.set_settings_message("Working...".into());
        }
        Err(_) => state.set_settings_message(
            "Runtime worker unavailable; fix the startup error and restart".into(),
        ),
    }
}

fn send_config(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_config_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_config_busy(true);
            state.set_config_message("Working...".into());
        }
        Err(_) => state.set_config_message("Configuration worker is unavailable".into()),
    }
}

fn send_live(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_live_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_live_busy(true);
            state.set_live_message("Working...".into());
        }
        Err(_) => state.set_live_message("LIVE runtime worker is unavailable".into()),
    }
}

pub fn bind(ui: &MainWindow) -> Controller {
    let (sender, requests) = mpsc::channel();
    let (responses, receiver) = mpsc::channel();
    let state = ui.global::<AppState>();
    state.set_settings_busy(true);
    state.set_config_busy(true);
    state.set_live_busy(true);
    if let Err(error) = std::thread::Builder::new()
        .name("native-runtime".into())
        .spawn(move || worker(requests, responses))
    {
        state.set_settings_busy(false);
        state.set_config_busy(false);
        state.set_live_busy(false);
        state.set_settings_message(format!("Cannot start worker: {error}").into());
        state.set_config_message(format!("Cannot start worker: {error}").into());
        state.set_live_message(format!("Cannot start worker: {error}").into());
    }

    let draft = Rc::new(RefCell::new(SettingsDraft::default()));
    let channels_draft = Rc::new(RefCell::new(ChannelsDraft::default()));
    let live_poll_in_flight = Rc::new(Cell::new(false));

    let weak = ui.as_weak();
    let edit_draft = draft.clone();
    state.on_setting_edited(move |index, value| {
        if let Some(ui) = weak.upgrade() {
            if index >= 0 && !ui.global::<AppState>().get_settings_busy() {
                let mut draft = edit_draft.borrow_mut();
                draft.edit(index as usize, value.to_string());
                ui.global::<AppState>()
                    .set_settings_dirty(!draft.patch().is_empty());
            }
        }
    });

    let weak = ui.as_weak();
    let save_draft = draft.clone();
    let save_sender = sender.clone();
    state.on_save_settings(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(
                &ui,
                &save_sender,
                Request::Save(save_draft.borrow().patch()),
            );
        }
    });

    let weak = ui.as_weak();
    let reload_sender = sender.clone();
    state.on_reload_settings(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(&ui, &reload_sender, Request::Reload);
        }
    });

    let weak = ui.as_weak();
    let refresh_sender = sender.clone();
    state.on_refresh_requested(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(&ui, &refresh_sender, Request::Refresh);
        }
    });

    let weak = ui.as_weak();
    let pick_draft = draft.clone();
    let pick_sender = sender.clone();
    state.on_pick_setting(move |index| {
        if let Some(ui) = weak.upgrade() {
            if let Some(field) = usize::try_from(index)
                .ok()
                .and_then(|index| pick_draft.borrow().fields.get(index).cloned())
            {
                send_settings(
                    &ui,
                    &pick_sender,
                    Request::Pick(index as usize, field.kind, field.value),
                );
            }
        }
    });

    let weak = ui.as_weak();
    let add_draft = channels_draft.clone();
    state.on_channel_add(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_config_busy() {
                return;
            }
            add_draft.borrow_mut().add();
            render_channels(&ui, &add_draft.borrow());
            ui.global::<AppState>()
                .set_config_message("New channel draft added; Save validates it.".into());
        }
    });

    let weak = ui.as_weak();
    let remove_draft = channels_draft.clone();
    state.on_channel_remove(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            remove_draft.borrow_mut().remove(index as usize);
            render_channels(&ui, &remove_draft.borrow());
            ui.global::<AppState>()
                .set_config_message("Channel removed from draft; Save to persist.".into());
        }
    });

    let weak = ui.as_weak();
    let edit_channels = channels_draft.clone();
    state.on_channel_edited(move |index, field, value| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            edit_channels
                .borrow_mut()
                .edit(index as usize, field.as_str(), value.to_string());
            ui.global::<AppState>()
                .set_channels_dirty(edit_channels.borrow().dirty());
        }
    });

    let weak = ui.as_weak();
    let enabled_channels = channels_draft.clone();
    state.on_channel_toggle_enabled(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            let index = index as usize;
            let enabled = enabled_channels
                .borrow()
                .rows
                .get(index)
                .map(|channel| !channel.enabled);
            if let Some(enabled) = enabled {
                enabled_channels.borrow_mut().set_enabled(index, enabled);
                render_channels(&ui, &enabled_channels.borrow());
            }
        }
    });

    let weak = ui.as_weak();
    let platform_channels = channels_draft.clone();
    state.on_channel_toggle_platform(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            platform_channels
                .borrow_mut()
                .toggle_platform(index as usize);
            render_channels(&ui, &platform_channels.borrow());
        }
    });

    let weak = ui.as_weak();
    let resolve_channels = channels_draft.clone();
    let resolve_sender = sender.clone();
    state.on_channel_resolve(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            let index = index as usize;
            let channel = resolve_channels.borrow().rows.get(index).cloned();
            let Some(channel) = channel else {
                return;
            };
            if channel.account.trim().is_empty() {
                ui.global::<AppState>()
                    .set_config_message("Enter an account / channel id before resolving.".into());
                return;
            }
            send_config(
                &ui,
                &resolve_sender,
                Request::ChannelResolve {
                    index,
                    platform: channel.platform,
                    account: channel.account,
                },
            );
        }
    });

    let weak = ui.as_weak();
    let save_channels = channels_draft.clone();
    let config_sender = sender.clone();
    state.on_save_channels(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(
                &ui,
                &config_sender,
                Request::ChannelsSave(save_channels.borrow().snapshot()),
            );
        }
    });

    let weak = ui.as_weak();
    let config_sender = sender.clone();
    state.on_reload_configuration(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(&ui, &config_sender, Request::ConfigReload);
        }
    });

    let weak = ui.as_weak();
    let provider_sender = sender.clone();
    state.on_save_provider(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<AppState>();
            let secrets = BTreeMap::from([
                (
                    "SOOP_PASSWORD".into(),
                    state.get_soop_password_draft().to_string(),
                ),
                (
                    "CLOUDFLARE_API_KEY".into(),
                    state.get_cloudflare_key_draft().to_string(),
                ),
                (
                    "CHZZK_NID_AUT".into(),
                    state.get_chzzk_nid_aut_draft().to_string(),
                ),
                (
                    "CHZZK_NID_SES".into(),
                    state.get_chzzk_nid_ses_draft().to_string(),
                ),
            ]);
            send_config(
                &ui,
                &provider_sender,
                Request::ProviderSave {
                    username: state.get_soop_username().to_string(),
                    worker_url: state.get_cloudflare_worker_url().to_string(),
                    secrets,
                },
            );
        }
    });

    let weak = ui.as_weak();
    let provider_sender = sender.clone();
    state.on_test_soop_auth(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(&ui, &provider_sender, Request::ProviderTestSoop);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_start(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStart);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_stop(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStop);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStatus { poll: false });
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_action(move |target, action| {
        if let Some(ui) = weak.upgrade() {
            send_live(
                &ui,
                &live_sender,
                Request::LiveAction {
                    target: target.to_string(),
                    action: action.to_string(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_password_submit(move |target, password| {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<AppState>();
            state.set_live_password_target("".into());
            state.set_live_password_label("".into());
            state.set_live_password_draft("".into());
            if password.is_empty() {
                state.set_live_message("Password is empty".into());
                return;
            }
            send_live(
                &ui,
                &live_sender,
                Request::LivePassword {
                    target: target.to_string(),
                    password: password.to_string(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let response_poll_flag = live_poll_in_flight.clone();
    let response_channels = channels_draft.clone();
    let response_timer = Timer::default();
    response_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        while let Ok(response) = receiver.try_recv() {
            let state = ui.global::<AppState>();
            match response {
                Response::Snapshot {
                    fields,
                    diagnostics,
                    backend,
                    database,
                    channel_count,
                    message,
                } => {
                    state.set_settings_busy(false);
                    let startup_failed = !state.get_live_loaded() && fields.is_none();
                    if let Some(fields) = fields {
                        draft.borrow_mut().load(fields);
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_loaded(true);
                    }
                    bind_core_snapshot(&ui, diagnostics);
                    state.set_backend_path(backend.into());
                    state.set_database_path(database.into());
                    state.set_channel_count(channel_count.into());
                    state.set_settings_message(message.clone().into());
                    if startup_failed {
                        state.set_config_busy(false);
                        state.set_config_message(message.clone().into());
                        state.set_live_busy(false);
                        state.set_live_message(message.into());
                    }
                }
                Response::Configuration {
                    channels,
                    username,
                    worker_url,
                    secrets,
                    message,
                } => {
                    state.set_config_busy(false);
                    response_channels.borrow_mut().load(channels);
                    render_channels(&ui, &response_channels.borrow());
                    state.set_channel_count(
                        response_channels.borrow().rows.len().to_string().into(),
                    );
                    state.set_soop_username(username.into());
                    state.set_cloudflare_worker_url(worker_url.into());
                    state.set_soop_password_configured(
                        secrets.get("SOOP_PASSWORD").copied().unwrap_or(false),
                    );
                    state.set_cloudflare_key_configured(
                        secrets.get("CLOUDFLARE_API_KEY").copied().unwrap_or(false),
                    );
                    state.set_chzzk_nid_aut_configured(
                        secrets.get("CHZZK_NID_AUT").copied().unwrap_or(false),
                    );
                    state.set_chzzk_nid_ses_configured(
                        secrets.get("CHZZK_NID_SES").copied().unwrap_or(false),
                    );
                    state.set_soop_password_draft("".into());
                    state.set_cloudflare_key_draft("".into());
                    state.set_chzzk_nid_aut_draft("".into());
                    state.set_chzzk_nid_ses_draft("".into());
                    state.set_config_loaded(true);
                    state.set_config_message(message.into());
                }
                Response::ChannelResolved {
                    index,
                    account,
                    name,
                } => {
                    state.set_config_busy(false);
                    let applied = {
                        let mut channels = response_channels.borrow_mut();
                        if channels
                            .rows
                            .get(index)
                            .is_some_and(|channel| channel.account == account)
                        {
                            channels.edit(index, "name", name.clone());
                            true
                        } else {
                            false
                        }
                    };
                    if applied {
                        render_channels(&ui, &response_channels.borrow());
                        state.set_config_message(format!("Resolved channel name: {name}").into());
                    } else {
                        state.set_config_message(
                            "Channel changed while lookup was running; lookup result ignored."
                                .into(),
                        );
                    }
                }
                Response::ConfigMessage(message) => {
                    state.set_config_busy(false);
                    state.set_config_message(message.into());
                }
                Response::ConfigError(message) => {
                    state.set_config_busy(false);
                    state.set_config_message(message.into());
                }
                Response::Picked(index, path) => {
                    state.set_settings_busy(false);
                    if draft.borrow_mut().accept_selection(index, path) {
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_message(
                            "Selection added to draft; Save validates and persists it".into(),
                        );
                    } else {
                        state.set_settings_message("Selection cancelled; draft unchanged".into());
                    }
                }
                Response::Live {
                    status,
                    message,
                    poll,
                } => {
                    if poll {
                        response_poll_flag.set(false);
                    } else {
                        state.set_live_busy(false);
                    }
                    render_live(&ui, status);
                    if let Some(message) = message {
                        state.set_live_message(message.into());
                    } else if !poll {
                        state.set_live_message("LIVE status refreshed".into());
                    }
                }
                Response::LiveError { message, poll } => {
                    if poll {
                        response_poll_flag.set(false);
                    } else {
                        state.set_live_busy(false);
                    }
                    state.set_live_message(message.into());
                }
                Response::Error(message) => {
                    state.set_settings_busy(false);
                    if !state.get_config_loaded() {
                        state.set_config_busy(false);
                        state.set_config_message(message.clone().into());
                    }
                    if !state.get_live_loaded() {
                        state.set_live_busy(false);
                        state.set_live_message(message.clone().into());
                    }
                    state.set_settings_message(message.into());
                }
            }
        }
    });

    let weak = ui.as_weak();
    let poll_sender = sender;
    let poll_flag = live_poll_in_flight;
    let live_poll_timer = Timer::default();
    live_poll_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(1500),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let state = ui.global::<AppState>();
            if state.get_active_page().as_str() != "LIVE"
                || state.get_live_busy()
                || poll_flag.get()
            {
                return;
            }
            if poll_sender.send(Request::LiveStatus { poll: true }).is_ok() {
                poll_flag.set(true);
            }
        },
    );

    Controller {
        _response_timer: response_timer,
        _live_poll_timer: live_poll_timer,
    }
}
