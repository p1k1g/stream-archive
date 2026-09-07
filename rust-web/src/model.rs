use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub enabled: bool,
    pub name: String,
    pub account: String,
    pub outdir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatcherStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub last_exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub watcher: WatcherStatus,
    pub backend_dir: String,
    pub bind: String,
    pub phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsResponse {
    pub values: BTreeMap<String, String>,
    pub hidden_keys: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogsResponse {
    pub lines: Vec<String>,
}
