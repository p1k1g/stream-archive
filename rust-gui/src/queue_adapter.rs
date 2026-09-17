use stream_archive_server::model::{VodQueueItem, VodQueueSnapshot};

#[derive(Debug, Clone, PartialEq)]
pub struct QueueView {
    pub active: String,
    pub queued: String,
    pub completed: String,
    pub failed: String,
    pub rows: Vec<QueueRowView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueRowView {
    pub id: String,
    pub platform: String,
    pub title: String,
    pub streamer: String,
    pub url: String,
    pub state: String,
    pub state_label: String,
    pub state_tone: String,
    pub attempts: String,
    pub message: String,
    pub percent: f32,
    pub percent_label: String,
    pub part_progress: String,
    pub output_directory: String,
    pub output_file: String,
    pub created_at: String,
    pub started_at: String,
    pub finished_at: String,
    pub can_cancel: bool,
    pub can_retry: bool,
    pub can_remove: bool,
}

pub fn view(snapshot: VodQueueSnapshot) -> QueueView {
    let active = snapshot
        .items
        .iter()
        .filter(|item| matches!(item.state.as_str(), "STARTING" | "RUNNING" | "CANCELLING"))
        .count();
    let queued = snapshot
        .items
        .iter()
        .filter(|item| item.state == "QUEUED")
        .count();
    let completed = snapshot
        .items
        .iter()
        .filter(|item| item.state == "COMPLETED")
        .count();
    let failed = snapshot
        .items
        .iter()
        .filter(|item| matches!(item.state.as_str(), "FAILED" | "INTERRUPTED"))
        .count();
    QueueView {
        active: active.to_string(),
        queued: queued.to_string(),
        completed: completed.to_string(),
        failed: failed.to_string(),
        rows: snapshot.items.into_iter().map(row).collect(),
    }
}

fn row(item: VodQueueItem) -> QueueRowView {
    let state = item.state.to_ascii_uppercase();
    let (state_label, state_tone) = state_meta(&state);
    let percent = item.percent.clamp(0.0, 100.0) as f32;
    QueueRowView {
        id: item.id,
        platform: item.platform.to_string(),
        title: if item.title.trim().is_empty() {
            "VOD download".into()
        } else {
            item.title
        },
        streamer: item.streamer,
        url: item.vod_url,
        state: state.clone(),
        state_label: state_label.into(),
        state_tone: state_tone.into(),
        attempts: item.attempts.to_string(),
        message: item.message,
        percent,
        percent_label: format!("{percent:.1}%"),
        part_progress: if item.part_count > 0 {
            format!("Part {} / {}", item.current_part, item.part_count)
        } else {
            "Part -".into()
        },
        output_directory: item.output_directory,
        output_file: item.output_file.unwrap_or_default(),
        created_at: item.created_at,
        started_at: item.started_at.unwrap_or_else(|| "-".into()),
        finished_at: item.finished_at.unwrap_or_else(|| "-".into()),
        can_cancel: matches!(state.as_str(), "QUEUED" | "STARTING" | "RUNNING"),
        can_retry: matches!(state.as_str(), "FAILED" | "CANCELLED" | "INTERRUPTED"),
        can_remove: matches!(
            state.as_str(),
            "FAILED" | "CANCELLED" | "INTERRUPTED" | "COMPLETED"
        ),
    }
}

fn state_meta(state: &str) -> (&'static str, &'static str) {
    match state {
        "QUEUED" => ("Queued", "neutral"),
        "STARTING" => ("Starting", "warn"),
        "RUNNING" => ("Running", "ok"),
        "CANCELLING" => ("Cancelling", "warn"),
        "COMPLETED" => ("Completed", "ok"),
        "FAILED" => ("Failed", "error"),
        "CANCELLED" => ("Cancelled", "neutral"),
        "INTERRUPTED" => ("Interrupted", "warn"),
        other => (other, "neutral"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::support::platform::PlatformId;

    fn item(state: &str) -> VodQueueItem {
        VodQueueItem {
            platform: PlatformId::Soop,
            id: state.into(),
            vod_url: "url".into(),
            output_directory: "out".into(),
            state: state.into(),
            attempts: 1,
            message: String::new(),
            title: String::new(),
            streamer: String::new(),
            current_part: 1,
            part_count: 2,
            percent: 50.0,
            output_file: None,
            created_at: "now".into(),
            started_at: None,
            finished_at: None,
            updated_at: "now".into(),
        }
    }

    #[test]
    fn queue_actions_follow_runtime_contract() {
        let running = row(item("RUNNING"));
        assert!(running.can_cancel);
        assert!(!running.can_retry);
        assert!(!running.can_remove);
        let failed = row(item("FAILED"));
        assert!(!failed.can_cancel);
        assert!(failed.can_retry);
        assert!(failed.can_remove);
        let cancelling = row(item("CANCELLING"));
        assert!(!cancelling.can_cancel);
    }

    #[test]
    fn unknown_state_remains_visible() {
        let unknown = row(item("FUTURE_STATE"));
        assert_eq!(unknown.state_label, "FUTURE_STATE");
        assert_eq!(unknown.state_tone, "neutral");
    }

    #[test]
    fn summary_counts_active_queued_completed_and_failed() {
        let snapshot = VodQueueSnapshot {
            active_id: Some("RUNNING".into()),
            queued_count: 1,
            items: vec![
                item("RUNNING"),
                item("QUEUED"),
                item("COMPLETED"),
                item("INTERRUPTED"),
            ],
        };
        let view = view(snapshot);
        assert_eq!(view.active, "1");
        assert_eq!(view.queued, "1");
        assert_eq!(view.completed, "1");
        assert_eq!(view.failed, "1");
    }
}
