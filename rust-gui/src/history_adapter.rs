use stream_archive_server::model::{HistoryResponse, LiveHistoryItem, VodHistoryItem};

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRowView {
    pub kind: String,
    pub platform: String,
    pub title: String,
    pub subject: String,
    pub state: String,
    pub state_tone: String,
    pub detail: String,
    pub timing: String,
    pub file: String,
    pub meta: String,
}

pub fn rows(history: HistoryResponse, view: &str) -> Vec<HistoryRowView> {
    let view = view.to_ascii_uppercase();
    let mut rows = Vec::new();
    if view != "VOD" {
        rows.extend(history.live.into_iter().map(live_row));
    }
    if view != "LIVE" {
        rows.extend(history.vod.into_iter().map(vod_row));
    }
    rows
}

fn live_row(item: LiveHistoryItem) -> HistoryRowView {
    let title = item
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| item.channel_name.clone());
    let subject = format!("{} · {}", item.channel_name, item.account);
    let timing = format!(
        "Started: {}  ·  Ended: {}",
        item.started_at,
        item.ended_at.clone().unwrap_or_else(|| "-".into())
    );
    let mut detail = format!(
        "Duration: {}  ·  Size: {}",
        format_duration(item.duration_seconds.max(0) as u64),
        format_bytes(item.size_bytes)
    );
    if let Some(reason) = item
        .reason
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        detail.push_str("  ·  ");
        detail.push_str(reason);
    }
    HistoryRowView {
        kind: "LIVE".into(),
        platform: item.platform.to_string(),
        title,
        subject,
        state_tone: state_tone(&item.status).into(),
        state: item.status,
        detail,
        timing,
        file: item.file_path.unwrap_or_default(),
        meta: item
            .bno
            .map(|bno| format!("Broadcast: {bno}"))
            .unwrap_or_default(),
    }
}

fn vod_row(item: VodHistoryItem) -> HistoryRowView {
    let title = if item.title.trim().is_empty() {
        "VOD job".into()
    } else {
        item.title.clone()
    };
    let subject = if item.streamer.trim().is_empty() {
        item.vod_url.clone()
    } else {
        format!("{} · {}", item.streamer, item.vod_url)
    };
    let timing = format!(
        "Started: {}  ·  Finished: {}",
        item.started_at.clone().unwrap_or_else(|| "-".into()),
        item.finished_at.clone().unwrap_or_else(|| "-".into())
    );
    let detail = if item.message.trim().is_empty() {
        format!("{} · {} parts", item.kind, item.part_count)
    } else {
        format!(
            "{} · {} parts · {}",
            item.kind, item.part_count, item.message
        )
    };
    HistoryRowView {
        kind: "VOD".into(),
        platform: item.platform.to_string(),
        title,
        subject,
        state_tone: state_tone(&item.state).into(),
        state: item.state,
        detail,
        timing,
        file: item.output_file.unwrap_or_default(),
        meta: item.kind,
    }
}

fn state_tone(state: &str) -> &'static str {
    match state.to_ascii_uppercase().as_str() {
        "COMPLETED" | "RECORDING" => "ok",
        "FAILED" | "ERROR" => "error",
        "INTERRUPTED" | "CANCELLED" | "LOW_DISK" | "STALLED" => "warn",
        _ => "neutral",
    }
}

pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::support::platform::PlatformId;

    #[test]
    fn duration_and_size_are_compact() {
        assert_eq!(format_duration(65), "01:05");
        assert_eq!(format_duration(3661), "01:01:01");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn view_separates_live_and_vod_without_dropping_unknown_states() {
        let history = HistoryResponse {
            live: vec![LiveHistoryItem {
                platform: PlatformId::Soop,
                id: "l1".into(),
                account: "account".into(),
                channel_name: "channel".into(),
                bno: None,
                title: None,
                file_path: None,
                started_at: "2026-09-17T00:00:00Z".into(),
                ended_at: None,
                duration_seconds: 0,
                size_bytes: 0,
                reason: None,
                status: "FUTURE_STATE".into(),
            }],
            vod: vec![VodHistoryItem {
                platform: PlatformId::Chzzk,
                id: "v1".into(),
                kind: "JOB".into(),
                vod_url: "url".into(),
                title: "title".into(),
                streamer: "streamer".into(),
                part_count: 1,
                state: "COMPLETED".into(),
                output_file: None,
                message: String::new(),
                started_at: None,
                finished_at: None,
            }],
        };
        let live = rows(history.clone(), "LIVE");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].state, "FUTURE_STATE");
        let vod = rows(history, "VOD");
        assert_eq!(vod.len(), 1);
        assert_eq!(vod[0].kind, "VOD");
    }
}
