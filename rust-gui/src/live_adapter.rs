use stream_archive_server::{
    model::{ChannelRuntimeStatus, NativeWatcherStatus},
    support::platform::PlatformId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveChannelView {
    pub target: String,
    pub platform: String,
    pub name: String,
    pub account: String,
    pub status: String,
    pub status_label: String,
    pub status_tone: String,
    pub title: String,
    pub bno: String,
    pub file: String,
    pub output_dir: String,
    pub size: String,
    pub started_at: String,
    pub elapsed: String,
    pub suppressed: bool,
    pub detail: String,
    pub can_stop_once: bool,
    pub can_resume: bool,
    pub can_recheck: bool,
    pub password_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveView {
    pub running: bool,
    pub state_label: String,
    pub engine: String,
    pub started_at: String,
    pub channel_count: String,
    pub recording_count: String,
    pub offline_count: String,
    pub error_count: String,
    pub channels: Vec<LiveChannelView>,
}

pub fn scoped_target(platform: PlatformId, account: &str) -> String {
    format!("{}:{}", platform.as_str(), account.trim())
}

pub fn provider_live_url(platform: &str, account: &str) -> Result<String, String> {
    let account = account.trim();
    if account.is_empty()
        || account.len() > 200
        || account.chars().any(|ch| {
            ch.is_whitespace()
                || ch.is_control()
                || matches!(ch, '/' | '\\' | '?' | '#' | '@' | ':')
        })
    {
        return Err("올바른 채널 계정이 아니어서 방송 페이지를 열 수 없습니다.".into());
    }
    match platform.trim().to_ascii_uppercase().as_str() {
        "SOOP" => Ok(format!("https://play.sooplive.com/{account}")),
        "CHZZK" => Ok(format!("https://chzzk.naver.com/live/{account}")),
        "KICK" => Ok(format!("https://kick.com/{account}")),
        other => Err(format!("지원하지 않는 플랫폼입니다: {other}")),
    }
}

pub fn validated_action(action: &str) -> Option<&'static str> {
    match action {
        "stop" => Some("stop"),
        "resume" => Some("resume"),
        "recheck" => Some("recheck"),
        _ => None,
    }
}

fn status_presentation(status: &str) -> (&str, &str) {
    match status {
        "UNKNOWN" => ("확인 중", "muted"),
        "OFFLINE" => ("오프라인", "muted"),
        "RECORDING" => ("녹화 중", "ok"),
        "PASSWORD_REQUIRED" => ("비밀번호 필요", "warn"),
        "DISABLED" => ("사용 안 함", "muted"),
        "WATCHER_STOPPED" => ("방송 감시 중지됨", "muted"),
        "PAUSED" => ("일시중지", "warn"),
        "LOW_DISK" => ("공간 부족", "error"),
        "STALLED" => ("녹화 정체", "error"),
        "AUTH" => ("인증 필요", "error"),
        "ERROR" => ("오류", "error"),
        other => (other, "warn"),
    }
}

fn format_bytes(bytes: u64) -> String {
    crate::formatting::format_bytes_short(bytes)
}

fn channel_view(channel: ChannelRuntimeStatus, watcher_running: bool) -> LiveChannelView {
    let target = scoped_target(channel.platform, &channel.account);
    let (status_label, status_tone) = status_presentation(&channel.status);
    let suppressed = channel.suppressed;
    let started_at = channel.started_at.unwrap_or_default();
    let elapsed = if channel.status == "RECORDING" {
        crate::formatting::format_elapsed_since(&started_at)
    } else {
        String::new()
    };
    LiveChannelView {
        target,
        platform: channel.platform.to_string(),
        name: channel.name,
        account: channel.account,
        status: channel.status.clone(),
        status_label: status_label.into(),
        status_tone: status_tone.into(),
        title: channel.title.unwrap_or_default(),
        bno: channel.bno.unwrap_or_default(),
        file: channel.file.unwrap_or_default(),
        output_dir: channel.output_dir.unwrap_or_default(),
        size: if channel.size_bytes == 0 {
            String::new()
        } else {
            format_bytes(channel.size_bytes)
        },
        started_at,
        elapsed,
        detail: channel.detail.unwrap_or_default(),
        can_stop_once: watcher_running && channel.status == "RECORDING" && !suppressed,
        can_resume: watcher_running && suppressed,
        can_recheck: watcher_running && channel.status != "DISABLED",
        password_required: watcher_running && channel.status == "PASSWORD_REQUIRED",
        suppressed,
    }
}

pub fn view(status: NativeWatcherStatus) -> LiveView {
    let running = status.running;
    LiveView {
        running,
        state_label: if running { "실행 중" } else { "중지됨" }.into(),
        engine: status.engine.into(),
        started_at: status.started_at.unwrap_or_else(|| "-".into()),
        channel_count: status.channel_count.to_string(),
        recording_count: status.recording_count.to_string(),
        offline_count: status.offline_count.to_string(),
        error_count: status.error_count.to_string(),
        channels: status
            .channels
            .into_iter()
            .map(|channel| channel_view(channel, running))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(status: &str) -> ChannelRuntimeStatus {
        ChannelRuntimeStatus {
            platform: PlatformId::Chzzk,
            account: "same-account".into(),
            name: "Example".into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn scoped_identity_keeps_platforms_unambiguous() {
        assert_eq!(scoped_target(PlatformId::Soop, "abc"), "SOOP:abc");
        assert_eq!(scoped_target(PlatformId::Chzzk, "abc"), "CHZZK:abc");
    }

    #[test]
    fn only_existing_runtime_actions_are_accepted() {
        assert_eq!(validated_action("stop"), Some("stop"));
        assert_eq!(validated_action("resume"), Some("resume"));
        assert_eq!(validated_action("recheck"), Some("recheck"));
        assert_eq!(validated_action("disable"), None);
    }

    #[test]
    fn password_and_action_state_follow_runtime_snapshot() {
        let mut status = NativeWatcherStatus {
            running: true,
            engine: "test",
            channels: vec![channel("PASSWORD_REQUIRED")],
            ..Default::default()
        };
        let password = view(status.clone()).channels.remove(0);
        assert!(password.password_required);
        assert!(password.can_recheck);
        assert!(!password.can_stop_once);

        let mut recording = channel("RECORDING");
        recording.bno = Some("123".into());
        status.channels = vec![recording];
        let recording = view(status.clone()).channels.remove(0);
        assert!(recording.can_stop_once);
        assert!(!recording.can_resume);

        status.channels[0].suppressed = true;
        let suppressed = view(status).channels.remove(0);
        assert!(!suppressed.can_stop_once);
        assert!(suppressed.can_resume);
    }

    #[test]
    fn stopped_snapshot_disables_live_actions_without_losing_rows() {
        let status = NativeWatcherStatus {
            running: false,
            engine: "test",
            channel_count: 1,
            channels: vec![channel("WATCHER_STOPPED")],
            ..Default::default()
        };
        let view = view(status);
        assert_eq!(view.state_label, "중지됨");
        assert_eq!(view.channels.len(), 1);
        assert!(!view.channels[0].can_recheck);
    }

    #[test]
    fn recording_metadata_is_presented_without_filesystem_probing() {
        let mut item = channel("RECORDING");
        item.title = Some("Live title".into());
        item.file = Some("C:/recordings/example.ts".into());
        item.size_bytes = 5 * 1024 * 1024;
        item.started_at = Some("2026-09-16T10:00:00Z".into());
        let status = NativeWatcherStatus {
            running: true,
            engine: "test",
            channels: vec![item],
            ..Default::default()
        };
        let row = view(status).channels.remove(0);
        assert_eq!(row.title, "Live title");
        assert_eq!(row.file, "C:/recordings/example.ts");
        assert_eq!(row.size, "5.0 MB");
        assert_eq!(row.started_at, "2026-09-16T10:00:00Z");
    }

    #[test]
    fn localized_error_status_labels_map_to_error_tone() {
        for (raw, label) in [
            ("LOW_DISK", "공간 부족"),
            ("STALLED", "녹화 정체"),
            ("AUTH", "인증 필요"),
        ] {
            let status = NativeWatcherStatus {
                running: true,
                engine: "test",
                channels: vec![channel(raw)],
                ..Default::default()
            };
            let row = view(status).channels.remove(0);
            assert_eq!(row.status_label, label);
            assert_eq!(row.status_tone, "error");
        }
    }

    #[test]
    fn provider_live_url_builds_allowlisted_https_urls_and_rejects_injection() {
        assert_eq!(
            provider_live_url("SOOP", "streamer01").unwrap(),
            "https://play.sooplive.com/streamer01"
        );
        assert_eq!(
            provider_live_url("CHZZK", "0123456789abcdef0123456789abcdef").unwrap(),
            "https://chzzk.naver.com/live/0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            provider_live_url("KICK", "example_slug").unwrap(),
            "https://kick.com/example_slug"
        );
        for bad in [
            "",
            "   ",
            "foo/bar",
            "foo?x=1",
            "foo#frag",
            "foo\\bar",
            "foo bar",
            "javascript:alert(1)",
        ] {
            assert!(provider_live_url("SOOP", bad).is_err());
        }
        assert!(provider_live_url("UNKNOWN", "abc").is_err());
    }
}
