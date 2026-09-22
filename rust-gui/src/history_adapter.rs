use stream_archive_server::{
    history_service::{format_history_timestamp_local, history_timestamp_sort_key},
    model::{HistoryResponse, LiveHistoryItem, VodHistoryItem},
};

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

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarDayView {
    pub day: String,
    pub date: String,
    pub in_month: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarMonthView {
    pub year: i32,
    pub month: u32,
    pub label: String,
    pub days: Vec<CalendarDayView>,
}

pub fn calendar_initial(selected: &str) -> CalendarMonthView {
    let (year, month, _) = parse_date(selected).unwrap_or_else(today_utc);
    calendar_month(year, month, selected)
}

pub fn calendar_shift(year: i32, month: u32, delta: i32, selected: &str) -> CalendarMonthView {
    let month_index = year * 12 + month as i32 - 1 + delta;
    let shifted_year = month_index.div_euclid(12);
    let shifted_month = month_index.rem_euclid(12) as u32 + 1;
    calendar_month(shifted_year, shifted_month, selected)
}

pub fn calendar_month(year: i32, month: u32, selected: &str) -> CalendarMonthView {
    let month = month.clamp(1, 12);
    let first_day = days_from_civil(year, month, 1);
    let first_weekday = (first_day + 4).rem_euclid(7);
    let mut days = Vec::with_capacity(42);
    for index in 0..42_i64 {
        let day_offset = index - first_weekday;
        let (cell_year, cell_month, cell_day) = civil_from_days(first_day + day_offset);
        let date = format!("{cell_year:04}-{cell_month:02}-{cell_day:02}");
        days.push(CalendarDayView {
            day: cell_day.to_string(),
            in_month: cell_year == year && cell_month == month,
            selected: date == selected.trim(),
            date,
        });
    }
    CalendarMonthView {
        year,
        month,
        label: format!("{year}년 {}", month_name(month)),
        days,
    }
}

fn parse_date(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.trim().split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some((year, month, day))
}

fn today_utc() -> (i32, u32, u32) {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_from_days(days)
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "1월",
        2 => "2월",
        3 => "3월",
        4 => "4월",
        5 => "5월",
        6 => "6월",
        7 => "7월",
        8 => "8월",
        9 => "9월",
        10 => "10월",
        11 => "11월",
        12 => "12월",
        _ => "",
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year as i64 - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

pub fn rows(history: HistoryResponse, view: &str) -> Vec<HistoryRowView> {
    let view = view.to_ascii_uppercase();
    let mut rows = Vec::new();
    if view != "VOD" {
        rows.extend(history.live.into_iter().map(|item| {
            let sort_key = history_timestamp_sort_key(&item.started_at);
            (sort_key, live_row(item))
        }));
    }
    if view != "LIVE" {
        rows.extend(history.vod.into_iter().map(|item| {
            let sort_key = item
                .started_at
                .as_deref()
                .or(item.finished_at.as_deref())
                .map(history_timestamp_sort_key)
                .unwrap_or(i64::MIN);
            (sort_key, vod_row(item))
        }));
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

fn live_row(item: LiveHistoryItem) -> HistoryRowView {
    let title = item
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| item.channel_name.clone());
    let subject = format!("{} · {}", item.channel_name, item.account);
    let timing = format!(
        "시작: {}  ·  종료: {}",
        format_history_timestamp_local(&item.started_at),
        item.ended_at
            .as_deref()
            .map(format_history_timestamp_local)
            .unwrap_or_else(|| "-".into())
    );
    let mut detail = format!(
        "길이: {}  ·  크기: {}",
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
        state: history_state_label(&item.status).into(),
        detail,
        timing,
        file: item.file_path.unwrap_or_default(),
        meta: item
            .bno
            .map(|bno| format!("방송: {bno}"))
            .unwrap_or_default(),
    }
}

fn vod_row(item: VodHistoryItem) -> HistoryRowView {
    let title = if item.title.trim().is_empty() {
        "VOD 작업".into()
    } else {
        item.title.clone()
    };
    let subject = if item.streamer.trim().is_empty() {
        item.vod_url.clone()
    } else {
        format!("{} · {}", item.streamer, item.vod_url)
    };
    let timing = format!(
        "시작: {}  ·  완료: {}",
        item.started_at
            .as_deref()
            .map(format_history_timestamp_local)
            .unwrap_or_else(|| "-".into()),
        item.finished_at
            .as_deref()
            .map(format_history_timestamp_local)
            .unwrap_or_else(|| "-".into())
    );
    let detail = if item.message.trim().is_empty() {
        format!("{} · PART {}개", item.kind, item.part_count)
    } else {
        format!(
            "{} · PART {}개 · {}",
            item.kind, item.part_count, item.message
        )
    };
    HistoryRowView {
        kind: "VOD".into(),
        platform: item.platform.to_string(),
        title,
        subject,
        state_tone: state_tone(&item.state).into(),
        state: history_state_label(&item.state).into(),
        detail,
        timing,
        file: item.output_file.unwrap_or_default(),
        meta: item.kind,
    }
}

fn history_state_label(state: &str) -> &str {
    match state.to_ascii_uppercase().as_str() {
        "IDLE" => "대기",
        "READY" => "준비됨",
        "QUEUED" => "대기 중",
        "STARTING" => "시작 중",
        "ANALYZING" => "분석 중",
        "RUNNING" => "진행 중",
        "DOWNLOADING" => "다운로드 중",
        "REFRESHING" => "인증 갱신 중",
        "MERGING" => "병합 중",
        "RECORDING" => "녹화 중",
        "STOPPED" => "중지됨",
        "COMPLETED" => "완료",
        "FAILED" => "실패",
        "ERROR" => "오류",
        "CANCELLING" => "취소 중",
        "CANCELLED" => "취소됨",
        "INTERRUPTED" => "중단됨",
        "LOW_DISK" => "디스크 공간 부족",
        "STALLED" => "정체됨",
        _ => state,
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
    fn calendar_builds_six_weeks_and_tracks_selected_date() {
        let month = calendar_month(2026, 9, "2026-09-18");
        assert_eq!(month.label, "2026년 9월");
        assert_eq!(month.days.len(), 42);
        assert!(
            month
                .days
                .iter()
                .any(|day| day.date == "2026-09-18" && day.selected)
        );
        assert!(month.days.iter().any(|day| !day.in_month));
    }

    #[test]
    fn calendar_shift_crosses_year_boundaries() {
        let next = calendar_shift(2026, 12, 1, "");
        assert_eq!((next.year, next.month), (2027, 1));
        let previous = calendar_shift(2026, 1, -1, "");
        assert_eq!((previous.year, previous.month), (2025, 12));
    }

    #[test]
    fn duration_and_size_are_compact() {
        assert_eq!(format_duration(65), "01:05");
        assert_eq!(format_duration(3661), "01:01:01");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn history_status_labels_cover_daily_use_filters() {
        assert_eq!(history_state_label("STOPPED"), "중지됨");
        assert_eq!(history_state_label("MERGING"), "병합 중");
        assert_eq!(history_state_label("RUNNING"), "진행 중");
        assert_eq!(history_state_label("CANCELLED"), "취소됨");
        assert_eq!(history_state_label("RECORDING"), "녹화 중");
        assert_eq!(history_state_label("COMPLETED"), "완료");
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

    #[test]
    fn all_view_interleaves_live_and_vod_by_timestamp() {
        let history = HistoryResponse {
            live: vec![LiveHistoryItem {
                platform: PlatformId::Soop,
                id: "live-old".into(),
                account: "account".into(),
                channel_name: "channel".into(),
                bno: None,
                title: Some("older live".into()),
                file_path: None,
                started_at: "2026-09-18T10:00:00Z".into(),
                ended_at: None,
                duration_seconds: 0,
                size_bytes: 0,
                reason: None,
                status: "STOPPED".into(),
            }],
            vod: vec![VodHistoryItem {
                platform: PlatformId::Soop,
                id: "vod-new".into(),
                kind: "DOWNLOAD".into(),
                vod_url: "url".into(),
                title: "newer vod".into(),
                streamer: "streamer".into(),
                part_count: 1,
                state: "COMPLETED".into(),
                output_file: None,
                message: String::new(),
                started_at: Some("2026-09-18T11:00:00Z".into()),
                finished_at: Some("2026-09-18T11:10:00Z".into()),
            }],
        };

        let all = rows(history, "ALL");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, "VOD");
        assert_eq!(all[1].kind, "LIVE");
    }
}
