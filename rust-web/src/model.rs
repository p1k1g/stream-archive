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
pub struct NativeWatcherStatus {
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

impl Default for NativeWatcherStatus {
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
    pub watcher: NativeWatcherStatus,
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

#[derive(Debug, Clone, Serialize)]
pub struct ChannelLookupResponse {
    pub account: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LiveHistoryItem {
    pub id: String,
    pub account: String,
    pub channel_name: String,
    pub bno: Option<String>,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: i64,
    pub size_bytes: u64,
    pub reason: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodHistoryItem {
    pub id: String,
    pub kind: String,
    pub vod_url: String,
    pub title: String,
    pub streamer: String,
    pub part_count: usize,
    pub state: String,
    pub output_file: Option<String>,
    pub message: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HistoryResponse {
    pub live: Vec<LiveHistoryItem>,
    pub vod: Vec<VodHistoryItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VodAnalyzeRequest {
    pub vod_url: String,
    #[serde(default = "default_cookie_mode")]
    pub cookie_mode: String,
    #[serde(default)]
    pub cookie_file: String,
    #[serde(default = "default_browser")]
    pub browser_name: String,
    #[serde(default)]
    pub yt_dlp_path: String,
    #[serde(default)]
    pub ffmpeg_path: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VodDownloadRequest {
    pub vod_url: String,
    pub output_directory: String,
    #[serde(default)]
    pub parts: Vec<usize>,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default = "default_true")]
    pub merge: bool,
    #[serde(default = "default_cookie_mode")]
    pub cookie_mode: String,
    #[serde(default)]
    pub cookie_file: String,
    #[serde(default = "default_browser")]
    pub browser_name: String,
    #[serde(default)]
    pub yt_dlp_path: String,
    #[serde(default)]
    pub ffmpeg_path: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_cookie_mode() -> String { "SOOP_LOGIN".to_string() }
fn default_browser() -> String { "firefox".to_string() }
fn default_quality() -> String { "best".to_string() }
fn default_max_retries() -> u32 { 5 }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodQualityOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodPartInfo {
    pub part: usize,
    pub duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodAnalysisView {
    pub vod_url: String,
    pub title: String,
    pub streamer: String,
    pub streamer_id: String,
    pub part_count: usize,
    pub qualities: Vec<VodQualityOption>,
    pub parts: Vec<VodPartInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VodJobStatus {
    pub state: String,
    pub running: bool,
    pub job_id: Option<String>,
    pub message: String,
    pub current_part: usize,
    pub part_count: usize,
    pub percent: f64,
    pub output_file: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub analysis: Option<VodAnalysisView>,
}

impl Default for VodJobStatus {
    fn default() -> Self {
        Self {
            state: "IDLE".to_string(),
            running: false,
            job_id: None,
            message: String::new(),
            current_part: 0,
            part_count: 0,
            percent: 0.0,
            output_file: None,
            started_at: None,
            finished_at: None,
            analysis: None,
        }
    }
}
