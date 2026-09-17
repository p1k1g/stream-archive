#[cfg(test)]
use stream_archive_server::model::VodPartInfo;
use stream_archive_server::model::{
    VodAnalysisView, VodDownloadRequest, VodJobStatus, VodQualityOption,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityChoice {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartChoice {
    pub part: usize,
    pub duration_seconds: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisSync {
    Applied,
    AlreadyCurrent,
    Stale,
}

#[derive(Debug, Clone)]
pub struct VodDraft {
    pub url: String,
    pub output_directory: String,
    pub merge: bool,
    pub qualities: Vec<QualityChoice>,
    pub parts: Vec<PartChoice>,
    analyzed_url: Option<String>,
}

impl Default for VodDraft {
    fn default() -> Self {
        Self {
            url: String::new(),
            output_directory: String::new(),
            merge: true,
            qualities: Vec::new(),
            parts: Vec::new(),
            analyzed_url: None,
        }
    }
}

impl VodDraft {
    pub fn edit_url(&mut self, value: String) {
        if self.url == value {
            return;
        }
        self.url = value;
        self.analyzed_url = None;
        self.qualities.clear();
        self.parts.clear();
    }

    pub fn edit_output_directory(&mut self, value: String) {
        self.output_directory = value;
    }

    pub fn accept_output_selection(&mut self, value: Option<String>) -> bool {
        let Some(value) = value else {
            return false;
        };
        self.output_directory = value;
        true
    }

    pub fn toggle_merge(&mut self) {
        self.merge = !self.merge;
    }

    pub fn select_quality(&mut self, index: usize) {
        if index >= self.qualities.len() {
            return;
        }
        for (row_index, row) in self.qualities.iter_mut().enumerate() {
            row.selected = row_index == index;
        }
    }

    pub fn toggle_part(&mut self, index: usize) {
        if let Some(part) = self.parts.get_mut(index) {
            part.selected = !part.selected;
        }
    }

    pub fn select_all_parts(&mut self) {
        for part in &mut self.parts {
            part.selected = true;
        }
    }

    pub fn clear_parts(&mut self) {
        for part in &mut self.parts {
            part.selected = false;
        }
    }

    pub fn can_analyze(&self) -> bool {
        !self.url.trim().is_empty()
    }

    pub fn can_download(&self) -> bool {
        self.analysis_matches_current_url()
            && !self.output_directory.trim().is_empty()
            && self.qualities.iter().any(|quality| quality.selected)
            && self.parts.iter().any(|part| part.selected)
    }

    pub fn sync_analysis(&mut self, view: &VodAnalysisView) -> AnalysisSync {
        if normalize(&self.url) != normalize(&view.vod_url) {
            return AnalysisSync::Stale;
        }
        if self.analysis_matches_current_url()
            && !self.qualities.is_empty()
            && !self.parts.is_empty()
        {
            return AnalysisSync::AlreadyCurrent;
        }

        self.analyzed_url = Some(view.vod_url.clone());
        self.qualities = quality_choices(&view.qualities);
        self.parts = part_choices(view);
        AnalysisSync::Applied
    }

    pub fn download_request(&self) -> Option<VodDownloadRequest> {
        if !self.can_download() {
            return None;
        }
        let quality = self
            .qualities
            .iter()
            .find(|quality| quality.selected)?
            .value
            .clone();
        let parts = self
            .parts
            .iter()
            .filter(|part| part.selected)
            .map(|part| part.part)
            .collect();
        Some(VodDownloadRequest {
            vod_url: self.url.trim().to_string(),
            output_directory: self.output_directory.trim().to_string(),
            parts,
            quality,
            merge: self.merge,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            max_retries: 5,
        })
    }

    fn analysis_matches_current_url(&self) -> bool {
        self.analyzed_url
            .as_deref()
            .is_some_and(|url| normalize(url) == normalize(&self.url))
    }
}

fn quality_choices(qualities: &[VodQualityOption]) -> Vec<QualityChoice> {
    let selected = qualities
        .iter()
        .position(|quality| quality.value == "best")
        .unwrap_or(0);
    qualities
        .iter()
        .enumerate()
        .map(|(index, quality)| QualityChoice {
            value: quality.value.clone(),
            label: quality.label.clone(),
            selected: index == selected,
        })
        .collect()
}

fn part_choices(view: &VodAnalysisView) -> Vec<PartChoice> {
    if !view.parts.is_empty() {
        return view
            .parts
            .iter()
            .map(|part| PartChoice {
                part: part.part,
                duration_seconds: part.duration_seconds,
                selected: true,
            })
            .collect();
    }
    (1..=view.part_count)
        .map(|part| PartChoice {
            part,
            duration_seconds: 0,
            selected: true,
        })
        .collect()
}

fn normalize(value: &str) -> &str {
    value.trim()
}

pub fn format_duration(seconds: u64) -> String {
    if seconds == 0 {
        return "-".into();
    }
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub struct VodView {
    pub platform: String,
    pub state: String,
    pub state_tone: String,
    pub running: bool,
    pub message: String,
    pub current_part: String,
    pub part_count: String,
    pub percent: f32,
    pub percent_label: String,
    pub output_file: String,
    pub started_at: String,
    pub finished_at: String,
    pub title: String,
    pub streamer: String,
    pub streamer_id: String,
}

pub fn view(status: &VodJobStatus) -> VodView {
    let analysis = status.analysis.as_ref();
    let percent = status.percent.clamp(0.0, 100.0) as f32;
    VodView {
        platform: status.platform.to_string(),
        state: status.state.clone(),
        state_tone: state_tone(&status.state).into(),
        running: status.running,
        message: status.message.clone(),
        current_part: status.current_part.to_string(),
        part_count: status.part_count.to_string(),
        percent,
        percent_label: format!("{percent:.1}%"),
        output_file: status.output_file.clone().unwrap_or_default(),
        started_at: status.started_at.clone().unwrap_or_else(|| "-".into()),
        finished_at: status.finished_at.clone().unwrap_or_else(|| "-".into()),
        title: analysis
            .map(|value| value.title.clone())
            .unwrap_or_default(),
        streamer: analysis
            .map(|value| value.streamer.clone())
            .unwrap_or_default(),
        streamer_id: analysis
            .map(|value| value.streamer_id.clone())
            .unwrap_or_default(),
    }
}

fn state_tone(state: &str) -> &'static str {
    match state {
        "READY" | "COMPLETED" => "ok",
        "FAILED" => "error",
        "CANCELLING" | "CANCELLED" | "REFRESHING" => "warn",
        _ => "neutral",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(url: &str) -> VodAnalysisView {
        VodAnalysisView {
            vod_url: url.into(),
            title: "Title".into(),
            streamer: "Streamer".into(),
            streamer_id: "streamer-id".into(),
            part_count: 2,
            qualities: vec![
                VodQualityOption {
                    value: "720p".into(),
                    label: "720p".into(),
                },
                VodQualityOption {
                    value: "best".into(),
                    label: "Best".into(),
                },
            ],
            parts: vec![
                VodPartInfo {
                    part: 1,
                    duration_seconds: 65,
                },
                VodPartInfo {
                    part: 2,
                    duration_seconds: 3661,
                },
            ],
        }
    }

    #[test]
    fn analysis_selects_best_quality_and_all_parts() {
        let mut draft = VodDraft::default();
        draft.edit_url("https://vod.sooplive.com/player/123".into());
        assert_eq!(
            draft.sync_analysis(&analysis("https://vod.sooplive.com/player/123")),
            AnalysisSync::Applied
        );
        assert!(draft.qualities[1].selected);
        assert!(draft.parts.iter().all(|part| part.selected));
    }

    #[test]
    fn stale_analysis_is_ignored_after_url_edit() {
        let mut draft = VodDraft::default();
        draft.edit_url("https://chzzk.naver.com/video/new".into());
        assert_eq!(
            draft.sync_analysis(&analysis("https://vod.sooplive.com/player/old")),
            AnalysisSync::Stale
        );
        assert!(draft.qualities.is_empty());
        assert!(draft.parts.is_empty());
    }

    #[test]
    fn url_edit_invalidates_analysis_but_keeps_output_draft() {
        let mut draft = VodDraft::default();
        draft.edit_url("https://vod.sooplive.com/player/123".into());
        draft.edit_output_directory("D:\\VOD".into());
        draft.sync_analysis(&analysis("https://vod.sooplive.com/player/123"));
        draft.edit_url("https://chzzk.naver.com/video/456".into());
        assert_eq!(draft.output_directory, "D:\\VOD");
        assert!(!draft.can_download());
    }

    #[test]
    fn part_and_quality_selection_build_download_request() {
        let mut draft = VodDraft::default();
        draft.edit_url("https://vod.sooplive.com/player/123".into());
        draft.edit_output_directory("D:\\VOD".into());
        draft.sync_analysis(&analysis("https://vod.sooplive.com/player/123"));
        draft.select_quality(0);
        draft.toggle_part(1);
        let req = draft.download_request().unwrap();
        assert_eq!(req.quality, "720p");
        assert_eq!(req.parts, vec![1]);
        assert!(req.merge);
        assert_eq!(req.cookie_mode, "SOOP_LOGIN");
        assert!(req.yt_dlp_path.is_empty());
    }

    #[test]
    fn select_all_and_clear_all_parts_update_download_readiness() {
        let mut draft = VodDraft::default();
        draft.edit_url("https://vod.sooplive.com/player/123".into());
        draft.edit_output_directory("D:\\VOD".into());
        draft.sync_analysis(&analysis("https://vod.sooplive.com/player/123"));
        assert!(draft.can_download());
        draft.clear_parts();
        assert!(!draft.can_download());
        draft.select_all_parts();
        assert!(draft.can_download());
    }

    #[test]
    fn view_formats_terminal_state_and_progress() {
        let status = VodJobStatus {
            state: "COMPLETED".into(),
            running: false,
            current_part: 2,
            part_count: 2,
            percent: 100.0,
            analysis: Some(analysis("https://vod.sooplive.com/player/123")),
            ..Default::default()
        };
        let view = view(&status);
        assert_eq!(view.state_tone, "ok");
        assert_eq!(view.percent_label, "100.0%");
        assert_eq!(view.title, "Title");
    }

    #[test]
    fn duration_format_is_compact() {
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3661), "1:01:01");
    }
}
