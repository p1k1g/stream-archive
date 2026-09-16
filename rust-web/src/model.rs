use crate::support::platform::PlatformId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub platform: PlatformId,
    pub enabled: bool,
    pub name: String,
    pub account: String,
    pub outdir: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ChannelRuntimeStatus {
    pub platform: PlatformId,
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
    pub platform: PlatformId,
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
    pub platform: PlatformId,
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
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub part_numbers: Vec<u32>,
    #[serde(default = "default_merge")]
    pub merge_parts: bool,
    #[serde(default)]
    pub output_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VodDownloadRequest {
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
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub part_numbers: Vec<u32>,
    #[serde(default = "default_merge")]
    pub merge_parts: bool,
    #[serde(default)]
    pub output_dir: String,
}

fn default_cookie_mode() -> String {
    "SOOP_LOGIN".into()
}

fn default_browser() -> String {
    "firefox".into()
}

fn default_max_retries() -> u32 {
    5
}

fn default_quality() -> String {
    "best".into()
}

fn default_merge() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodPartStatus {
    pub part: u32,
    pub state: String,
    pub percent: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VodJobStatus {
    pub id: String,
    pub state: String,
    pub vod_url: String,
    pub title: String,
    pub streamer: String,
    pub part_count: usize,
    pub current_part: usize,
    pub percent: f64,
    pub message: String,
    pub output_file: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub parts: Vec<VodPartStatus>,
}
