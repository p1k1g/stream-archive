use crate::{
    model::{HistoryResponse, LiveHistoryItem, VodHistoryItem},
    support::platform::PlatformId,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{Connection, params};
use std::path::Path;

pub const HISTORY_DEFAULT_LIMIT: usize = 100;
pub const HISTORY_MAX_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryFilter {
    pub q: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

impl HistoryFilter {
    pub fn normalized(&self) -> Result<NormalizedHistoryFilter> {
        let from = normalize_date_input(self.from.as_deref())?;
        let to = normalize_date_input(self.to.as_deref())?;
        if let (Some(from), Some(to)) = (&from, &to) {
            if from > to {
                bail!("history from date must not be after to date");
            }
        }
        Ok(NormalizedHistoryFilter {
            needle: self
                .q
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_lowercase),
            statuses: normalize_status_input(self.status.as_deref()),
            from,
            to,
            limit: self
                .limit
                .unwrap_or(HISTORY_DEFAULT_LIMIT)
                .clamp(1, HISTORY_MAX_LIMIT),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHistoryFilter {
    pub needle: Option<String>,
    pub statuses: Option<Vec<String>>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: usize,
}

pub fn load_history(path: &Path, filter: &HistoryFilter) -> Result<HistoryResponse> {
    let normalized = filter.normalized()?;
    let mut history = load_history_compat(path, HISTORY_MAX_LIMIT)?;

    history.live.retain(|item| {
        text_matches_live(item, normalized.needle.as_deref())
            && normalized
                .statuses
                .as_deref()
                .is_none_or(|wanted| status_matches(&item.status, wanted))
            && date_matches(
                &item.started_at,
                normalized.from.as_deref(),
                normalized.to.as_deref(),
            )
    });
    history.vod.retain(|item| {
        text_matches_vod(item, normalized.needle.as_deref())
            && normalized
                .statuses
                .as_deref()
                .is_none_or(|wanted| status_matches(&item.state, wanted))
            && item.started_at.as_deref().is_none_or(|started| {
                date_matches(
                    started,
                    normalized.from.as_deref(),
                    normalized.to.as_deref(),
                )
            })
    });
    history.live.truncate(normalized.limit);
    history.vod.truncate(normalized.limit);
    Ok(history)
}

fn stored_platform(value: String) -> PlatformId {
    value.parse().unwrap_or_default()
}

fn load_history_compat(path: &Path, limit: usize) -> rusqlite::Result<HistoryResponse> {
    let limit = limit.clamp(1, HISTORY_MAX_LIMIT) as i64;
    let conn = Connection::open(path)?;

    let mut live_stmt = conn.prepare(
        "SELECT COALESCE(platform,'SOOP'),id,account,channel_name,bno,title,file_path,started_at,ended_at,duration_seconds,size_bytes,reason,status FROM live_recordings ORDER BY started_at DESC LIMIT ?1",
    )?;
    let live = live_stmt
        .query_map(params![limit], |row| {
            Ok(LiveHistoryItem {
                platform: stored_platform(row.get(0)?),
                id: row.get(1)?,
                account: row.get(2)?,
                channel_name: row.get(3)?,
                bno: row.get(4)?,
                title: row.get(5)?,
                file_path: row.get(6)?,
                started_at: row.get(7)?,
                ended_at: row.get(8)?,
                duration_seconds: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                size_bytes: row.get::<_, Option<i64>>(10)?.unwrap_or(0).max(0) as u64,
                reason: row.get(11)?,
                status: row
                    .get::<_, Option<String>>(12)?
                    .unwrap_or_else(|| "UNKNOWN".to_string()),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut vod_stmt = conn.prepare(
        "SELECT COALESCE(platform,'SOOP'),id,COALESCE(kind,'JOB'),COALESCE(vod_url,''),COALESCE(title,''),COALESCE(streamer,''),COALESCE(part_count,0),COALESCE(state,'UNKNOWN'),output_file,COALESCE(message,''),started_at,finished_at FROM vod_jobs ORDER BY COALESCE(started_at,updated_at) DESC LIMIT ?1",
    )?;
    let vod = vod_stmt
        .query_map(params![limit], |row| {
            Ok(VodHistoryItem {
                platform: stored_platform(row.get(0)?),
                id: row.get(1)?,
                kind: row.get(2)?,
                vod_url: row.get(3)?,
                title: row.get(4)?,
                streamer: row.get(5)?,
                part_count: row.get::<_, i64>(6)?.max(0) as usize,
                state: row.get(7)?,
                output_file: row.get(8)?,
                message: row.get(9)?,
                started_at: row.get(10)?,
                finished_at: row.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(HistoryResponse { live, vod })
}

fn text_matches_live(item: &LiveHistoryItem, needle: Option<&str>) -> bool {
    let Some(needle) = needle else {
        return true;
    };
    [
        Some(item.platform.as_str()),
        Some(item.account.as_str()),
        Some(item.channel_name.as_str()),
        item.title.as_deref(),
        item.file_path.as_deref(),
        item.reason.as_deref(),
        Some(item.status.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(needle))
}

fn text_matches_vod(item: &VodHistoryItem, needle: Option<&str>) -> bool {
    let Some(needle) = needle else {
        return true;
    };
    [
        Some(item.platform.as_str()),
        Some(item.kind.as_str()),
        Some(item.vod_url.as_str()),
        Some(item.title.as_str()),
        Some(item.streamer.as_str()),
        item.output_file.as_deref(),
        Some(item.message.as_str()),
        Some(item.state.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(needle))
}

fn normalize_status_term(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn normalize_status_input(value: Option<&str>) -> Option<Vec<String>> {
    let value = value?.trim();
    if value.is_empty() || value == "전체" {
        return None;
    }

    let needle = normalize_status_term(value);
    let aliases: &[(&[&str], &[&str])] = &[
        (&["완료"], &["COMPLETED"]),
        (&["실패"], &["FAILED"]),
        (&["비정상 종료", "중단됨", "중단"], &["INTERRUPTED"]),
        (&["취소됨", "취소"], &["CANCELLED"]),
        (&["중지됨", "중지"], &["STOPPED"]),
        (&["대기"], &["READY", "QUEUED", "IDLE"]),
        (
            &["진행 중", "진행중", "진행"],
            &[
                "RUNNING",
                "STARTING",
                "ANALYZING",
                "DOWNLOADING",
                "REFRESHING",
                "MERGING",
                "CANCELLING",
            ],
        ),
        (&["준비 중", "준비중"], &["STARTING"]),
        (&["준비됨"], &["READY"]),
        (&["분석 중", "분석중"], &["ANALYZING"]),
        (&["다운로드 중", "다운로드중"], &["DOWNLOADING"]),
        (&["인증 갱신 중"], &["REFRESHING"]),
        (&["병합 중", "병합중"], &["MERGING"]),
        (&["취소 중", "취소중"], &["CANCELLING"]),
        (&["녹화 중", "녹화중"], &["RECORDING"]),
        (&["오프라인"], &["OFFLINE"]),
        (&["오류"], &["ERROR"]),
        (&["비활성"], &["DISABLED"]),
        (&["디스크 부족", "디스크 공간 부족"], &["LOW_DISK"]),
        (&["녹화 정지", "정체됨", "정체"], &["STALLED"]),
        (&["인증 필요"], &["AUTH"]),
        (&["비밀번호 필요"], &["PASSWORD_REQUIRED"]),
        (&["현재방송 중지", "현재 방송 중지"], &["PAUSED"]),
        (&["확인중", "확인 중"], &["UNKNOWN"]),
        (&["방송중", "방송 중"], &["LIVE"]),
    ];

    let mut matches = Vec::<String>::new();
    for (terms, statuses) in aliases {
        let alias_matches = terms
            .iter()
            .any(|term| normalize_status_term(term).contains(&needle));
        let canonical_matches = statuses
            .iter()
            .any(|status| normalize_status_term(status).contains(&needle));
        if alias_matches || canonical_matches {
            for status in *statuses {
                if !matches.iter().any(|existing| existing == status) {
                    matches.push((*status).to_string());
                }
            }
        }
    }

    if matches.is_empty() {
        Some(vec![value.to_ascii_uppercase()])
    } else {
        Some(matches)
    }
}

fn status_matches(actual: &str, wanted: &[String]) -> bool {
    wanted
        .iter()
        .any(|status| actual.eq_ignore_ascii_case(status))
}

fn normalize_date_input(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("history date must use a valid YYYY-MM-DD date"))?;
    Ok(Some(value.to_string()))
}

pub fn format_history_timestamp_local(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| timestamp.to_string())
}

pub fn history_timestamp_sort_key(timestamp: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn date_matches(timestamp: &str, from: Option<&str>, to: Option<&str>) -> bool {
    let local_date = DateTime::parse_from_rfc3339(timestamp)
        .map(|value| {
            value
                .with_timezone(&Local)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| timestamp.get(..10).unwrap_or(timestamp).to_string());
    from.is_none_or(|min| local_date.as_str() >= min)
        && to.is_none_or(|max| local_date.as_str() <= max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::VodJobStatus, store::Store};

    #[test]
    fn filter_normalizes_case_dates_and_limit() {
        let filter = HistoryFilter {
            q: Some("  Hello ".into()),
            status: Some(" completed ".into()),
            from: Some("2026-09-01".into()),
            to: Some("2026-09-30".into()),
            limit: Some(9999),
        };
        let normalized = filter.normalized().unwrap();
        assert_eq!(normalized.needle.as_deref(), Some("hello"));
        assert_eq!(normalized.statuses, Some(vec!["COMPLETED".to_string()]));
        assert_eq!(normalized.limit, HISTORY_MAX_LIMIT);
        assert!(
            HistoryFilter {
                from: Some("2026/09/01".into()),
                ..Default::default()
            }
            .normalized()
            .is_err()
        );
    }

    #[test]
    fn korean_status_aliases_map_to_canonical_history_states() {
        let completed = HistoryFilter {
            status: Some("완료".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(completed.statuses, Some(vec!["COMPLETED".to_string()]));

        let waiting = HistoryFilter {
            status: Some("대기".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(
            waiting.statuses,
            Some(vec![
                "READY".to_string(),
                "QUEUED".to_string(),
                "IDLE".to_string()
            ])
        );

        let active = HistoryFilter {
            status: Some("진행 중".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert!(
            active
                .statuses
                .as_ref()
                .is_some_and(|states| states.contains(&"DOWNLOADING".to_string()))
        );

        let canonical = HistoryFilter {
            status: Some("cancelled".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(canonical.statuses, Some(vec!["CANCELLED".to_string()]));

        let stopped = HistoryFilter {
            status: Some("STOP".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(stopped.statuses, Some(vec!["STOPPED".to_string()]));

        let merging = HistoryFilter {
            status: Some("병합".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(merging.statuses, Some(vec!["MERGING".to_string()]));

        let progressing = HistoryFilter {
            status: Some("진행".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert!(progressing
            .statuses
            .as_ref()
            .is_some_and(|states| states.contains(&"DOWNLOADING".to_string())));

        let cancelling = HistoryFilter {
            status: Some("취소".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        let cancelling_states = cancelling.statuses.unwrap();
        assert!(cancelling_states.contains(&"CANCELLED".to_string()));
        assert!(cancelling_states.contains(&"CANCELLING".to_string()));
    }

    #[test]
    fn date_filter_is_inclusive_in_system_local_time() {
        let local_date = DateTime::parse_from_rfc3339("2026-09-08T03:00:00Z")
            .unwrap()
            .with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        assert!(date_matches(
            "2026-09-08T03:00:00Z",
            Some(&local_date),
            Some(&local_date)
        ));
    }

    #[test]
    fn timestamp_display_and_sort_use_rfc3339_instant() {
        assert_eq!(
            history_timestamp_sort_key("2026-09-18T11:44:15Z"),
            history_timestamp_sort_key("2026-09-18T20:44:15+09:00")
        );
        let rendered = format_history_timestamp_local("2026-09-18T11:44:15Z");
        assert!(!rendered.contains('T'));
        assert!(!rendered.contains("+00:00"));
    }

    #[test]
    fn vod_search_status_and_limit_use_canonical_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stream-archive.db")).unwrap();
        let mut status = VodJobStatus::default();
        status.job_id = Some("job-1".into());
        status.state = "COMPLETED".into();
        status.message = "Needle Message".into();
        status.started_at = Some("2026-09-08T03:00:00Z".into());
        status.finished_at = Some("2026-09-08T04:00:00Z".into());
        store.upsert_vod(&status).unwrap();

        let history = load_history(
            store.path(),
            &HistoryFilter {
                q: Some("needle".into()),
                status: Some("completed".into()),
                from: Some("2026-09-08".into()),
                to: Some("2026-09-08".into()),
                limit: Some(1),
            },
        )
        .unwrap();
        assert_eq!(history.vod.len(), 1);
        assert_eq!(history.vod[0].state, "COMPLETED");
    }
}
