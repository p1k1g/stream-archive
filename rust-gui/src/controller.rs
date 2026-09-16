use crate::{
    AppState, DiagnosticRow, MainWindow, SettingRow, native_picker, settings_adapter::SettingsDraft,
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::mpsc, time::Duration};
use stream_archive_server::{
    app_core::StreamArchiveCore,
    backend::resolve_backend_dir,
    diagnostics::DiagnosticsSnapshot,
    environment_settings::{EnvironmentSetting, SettingKind},
};

enum Request {
    Refresh,
    Reload,
    Save(BTreeMap<String, String>),
    Pick(usize, SettingKind, String),
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
    Picked(usize, Option<String>),
    Error(String),
}

pub struct Controller {
    _timer: Timer,
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

fn worker(requests: mpsc::Receiver<Request>, responses: mpsc::Sender<Response>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
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
    let core = match resolve_backend_dir().and_then(StreamArchiveCore::open) {
        Ok(opened) => opened.core,
        Err(error) => {
            let message = format!("Runtime initialization failed: {error:#}");
            let _ = responses.send(Response::Snapshot {
                fields: None,
                diagnostics: DiagnosticsSnapshot::unavailable(None, None, &message),
                backend: "Unavailable".into(),
                database: "Unavailable".into(),
                channel_count: "Unavailable".into(),
                message,
            });
            return;
        }
    };
    if responses.send(read_snapshot(&core, true, "Settings loaded from canonical SQLite. Save applies changes to future runtime operations.")).is_err() {
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
        };
        if responses.send(response).is_err() {
            break;
        }
    }
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

fn send(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
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

pub fn bind(ui: &MainWindow) -> Controller {
    let (sender, requests) = mpsc::channel();
    let (responses, receiver) = mpsc::channel();
    let state = ui.global::<AppState>();
    state.set_settings_busy(true);
    if let Err(error) = std::thread::Builder::new()
        .name("native-settings".into())
        .spawn(move || worker(requests, responses))
    {
        state.set_settings_busy(false);
        state.set_settings_message(format!("Cannot start worker: {error}").into());
    }
    let draft = Rc::new(RefCell::new(SettingsDraft::default()));
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
            send(
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
            send(&ui, &reload_sender, Request::Reload);
        }
    });
    let weak = ui.as_weak();
    let refresh_sender = sender.clone();
    state.on_refresh_requested(move || {
        if let Some(ui) = weak.upgrade() {
            send(&ui, &refresh_sender, Request::Refresh);
        }
    });
    let weak = ui.as_weak();
    let pick_draft = draft.clone();
    state.on_pick_setting(move |index| {
        if let Some(ui) = weak.upgrade() {
            if let Some(field) = usize::try_from(index)
                .ok()
                .and_then(|index| pick_draft.borrow().fields.get(index).cloned())
            {
                send(
                    &ui,
                    &sender,
                    Request::Pick(index as usize, field.kind, field.value),
                );
            }
        }
    });
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        while let Ok(response) = receiver.try_recv() {
            let state = ui.global::<AppState>();
            state.set_settings_busy(false);
            match response {
                Response::Snapshot {
                    fields,
                    diagnostics,
                    backend,
                    database,
                    channel_count,
                    message,
                } => {
                    if let Some(fields) = fields {
                        draft.borrow_mut().load(fields);
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_loaded(true);
                    }
                    bind_core_snapshot(&ui, diagnostics);
                    state.set_backend_path(backend.into());
                    state.set_database_path(database.into());
                    state.set_channel_count(channel_count.into());
                    state.set_settings_message(message.into());
                }
                Response::Picked(index, path) => {
                    if let Some(path) = path {
                        draft.borrow_mut().edit(index, path);
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_message(
                            "Selection added to draft; Save validates and persists it".into(),
                        );
                    } else {
                        state.set_settings_message("Selection cancelled; draft unchanged".into());
                    }
                }
                Response::Error(message) => state.set_settings_message(message.into()),
            }
        }
    });
    Controller { _timer: timer }
}
