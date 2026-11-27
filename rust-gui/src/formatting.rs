fn format_byte_units(bytes: u64, units: [&str; 4]) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} {}", value / GIB, units[3])
    } else if value >= MIB {
        format!("{:.1} {}", value / MIB, units[2])
    } else if value >= KIB {
        format!("{:.1} {}", value / KIB, units[1])
    } else {
        format!("{bytes} {}", units[0])
    }
}

pub fn format_bytes(bytes: u64) -> String {
    format_byte_units(bytes, ["B", "KiB", "MiB", "GiB"])
}

pub fn format_bytes_short(bytes: u64) -> String {
    format_byte_units(bytes, ["B", "KB", "MB", "GB"])
}

fn split_hms(seconds: u64) -> (u64, u64, u64) {
    (seconds / 3600, (seconds % 3600) / 60, seconds % 60)
}

pub fn format_duration_padded(seconds: u64) -> String {
    let (hours, minutes, secs) = split_hms(seconds);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

pub fn format_duration_compact(seconds: u64) -> String {
    if seconds == 0 {
        return "-".into();
    }
    let (hours, minutes, secs) = split_hms(seconds);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub fn format_elapsed_ms(started_ms: i64, now_ms: i64) -> String {
    if started_ms == i64::MIN {
        return String::new();
    }
    let (hours, minutes, secs) =
        split_hms((now_ms.saturating_sub(started_ms).max(0) / 1000) as u64);
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

pub fn format_elapsed_since(started_at: &str) -> String {
    if started_at.trim().is_empty() {
        return String::new();
    }
    let started_ms = stream_archive_server::history_service::history_timestamp_sort_key(started_at);
    if started_ms == i64::MIN {
        return String::new();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(started_ms);
    format_elapsed_ms(started_ms, now_ms)
}

pub fn format_transfer_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return String::new();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.0} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_binary_byte_units_stably() {
        assert_eq!(format_bytes(42), "42 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GiB");
    }

    #[test]
    fn formats_elapsed_duration_and_transfer_rates() {
        assert_eq!(format_elapsed_ms(i64::MIN, 1_000_000), "");
        assert_eq!(format_elapsed_ms(1_000, 1_000 + 3_725_000), "01:02:05");
        assert_eq!(format_elapsed_ms(5_000, 1_000), "00:00:00");

        assert_eq!(format_transfer_rate(0.0), "");
        assert_eq!(format_transfer_rate(f64::NAN), "");
        assert_eq!(format_transfer_rate(512.0), "512 B/s");
        assert_eq!(format_transfer_rate(840.0 * 1024.0), "840 KB/s");
        assert_eq!(format_transfer_rate(1.5 * 1024.0 * 1024.0), "1.5 MB/s");
    }
}
