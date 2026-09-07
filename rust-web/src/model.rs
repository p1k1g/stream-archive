use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub enabled: bool,
    pub name: String,
    pub account: String,
    pub outdir: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ChannelRuntimeStatus {
    pub account: String,
    pub name: String,
    pub status: String,
    pub bno: Option<String>,
    pub title: Option<String>,
    pub file: Option<String>,
    pub size_bytes: u64,
    pub started_at: Option<String>,
    pub suppressed: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatcherStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub last_exit_code: Option<i32>,
    pub engine: &'static str,
    pub started_at: Option<String>,
    pub channel_count: usize,
    pub recording_count: usize,
    pub offline_count: usize,
    pub error_count: usize,
    pub channels: Vec<ChannelRuntimeStatus>,
}

impl Default for WatcherStatus {
    fn default() -> Self {
        Self {
            running: false,
            pid: None,
            last_exit_code: None,
            engine: "rust-native",
            started_at: None,
            channel_count: 0,
            recording_count: 0,
            offline_count: 0,
            error_count: 0,
            channels: Vec::new(),
        }
    }
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
