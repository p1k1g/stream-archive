//! Opt-in, bounded lifecycle evidence. No credentials, paths, URLs or channel identifiers.
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

const FILE_BYTES: u64 = 1024 * 1024;
const ARCHIVES: usize = 3;
const QUEUE_SIZE: usize = 64;
const HEADER: &str = "timestamp_utc,pid,event,visible,page,channels,recordings,thumbnails,pending,rgba_bytes,live_polls,model_resets,row_updates,owned_pid,dropped\n";

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Started,
    WindowChanged,
    Sample,
    WatcherSample,
    WatcherStopped,
    ChannelsSaved,
    ChannelsReloaded,
    RecordingStarted,
    RecordingFinished,
    TraceGap,
}
impl Event {
    fn periodic(self) -> bool {
        matches!(self, Self::Sample | Self::WatcherSample)
    }
    fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::WindowChanged => "window_changed",
            Self::Sample => "ui_sample",
            Self::WatcherSample => "watcher_sample",
            Self::WatcherStopped => "watcher_stopped",
            Self::ChannelsSaved => "channels_saved",
            Self::ChannelsReloaded => "channels_reloaded",
            Self::RecordingStarted => "recording_started",
            Self::RecordingFinished => "recording_finished",
            Self::TraceGap => "trace_gap",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Live,
    Channels,
    Vod,
    Queue,
    History,
    Settings,
    Diagnostics,
    Other,
}
impl Page {
    pub fn from_name(name: &str) -> Self {
        match name {
            "LIVE" => Self::Live,
            "Channels" => Self::Channels,
            "VOD" => Self::Vod,
            "Queue" => Self::Queue,
            "History" => Self::History,
            "Settings" => Self::Settings,
            "Diagnostics" => Self::Diagnostics,
            _ => Self::Other,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Channels => "Channels",
            Self::Vod => "VOD",
            Self::Queue => "Queue",
            Self::History => "History",
            Self::Settings => "Settings",
            Self::Diagnostics => "Diagnostics",
            Self::Other => "Other",
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub visible: Option<bool>,
    pub page: Option<Page>,
    pub channels: Option<usize>,
    pub recordings: Option<usize>,
    pub thumbnails: Option<usize>,
    pub pending: Option<usize>,
    pub rgba_bytes: Option<u64>,
    pub live_polls: Option<u64>,
    pub model_resets: Option<u64>,
    pub row_updates: Option<u64>,
    pub owned_pid: Option<u32>,
}
struct Record {
    timestamp: chrono::DateTime<chrono::Utc>,
    event: Event,
    sample: Sample,
    dropped: u64,
}
fn field(value: Option<impl std::fmt::Display>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}
impl Record {
    fn csv(&self) -> String {
        let s = self.sample;
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            self.timestamp.to_rfc3339(),
            std::process::id(),
            self.event.name(),
            field(s.visible),
            s.page.map(Page::name).unwrap_or(""),
            field(s.channels),
            field(s.recordings),
            field(s.thumbnails),
            field(s.pending),
            field(s.rgba_bytes),
            field(s.live_polls),
            field(s.model_resets),
            field(s.row_updates),
            field(s.owned_pid),
            self.dropped,
        )
    }
}
#[derive(Default)]
struct QueueState {
    rows: VecDeque<Record>,
    dropped: u64,
    closed: bool,
}
#[derive(Default)]
struct TraceQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}
impl TraceQueue {
    fn push(&self, record: Record) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // Coalesce periodic samples before they can consume lifecycle capacity.
        let replace = state
            .rows
            .iter()
            .position(|old| old.event.periodic() && old.event.name() == record.event.name());
        if let Some(index) = replace {
            state.rows.remove(index);
            state.dropped = state.dropped.saturating_add(1);
        } else if state.rows.len() == QUEUE_SIZE {
            if record.event.periodic() {
                state.dropped = state.dropped.saturating_add(1);
                return;
            }
            // Preserve the newest boundary (especially a final finish/stop).
            // If boundaries alone overflow, explicitly report the lost evidence.
            let index = state
                .rows
                .iter()
                .position(|old| old.event.periodic())
                .unwrap_or(0);
            state.rows.remove(index);
            state.dropped = state.dropped.saturating_add(1);
        }
        state.rows.push_back(record);
        self.ready.notify_one();
    }
    fn next(&self) -> (Option<Record>, u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.rows.is_empty() && !state.closed {
            state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        let dropped = state.dropped;
        (state.rows.pop_front(), dropped)
    }
    fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.ready.notify_one();
    }
}
fn drain_trace(
    queue: &TraceQueue,
    mut append: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut reported = 0;
    loop {
        let (record, dropped) = queue.next();
        if dropped != reported {
            append(
                &Record {
                    timestamp: chrono::Utc::now(),
                    event: Event::TraceGap,
                    sample: Sample::default(),
                    dropped,
                }
                .csv(),
            )?;
            reported = dropped;
        }
        let Some(mut record) = record else {
            return Ok(());
        };
        record.dropped = dropped;
        append(&record.csv())?;
    }
}
struct Inner {
    queue: Arc<TraceQueue>,
    writer: Mutex<Option<JoinHandle<()>>>,
    enabled: Arc<AtomicBool>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        // Close first, then drain/join once the final service owner is released.
        self.queue.close();
        if let Some(writer) = self
            .writer
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = writer.join();
        }
    }
}
#[derive(Clone, Default)]
pub struct ResourceTrace(Option<Arc<Inner>>);
impl ResourceTrace {
    pub(crate) fn from_environment(data_dir: &Path) -> Self {
        if std::env::var("STREAM_ARCHIVE_RESOURCE_TRACE").as_deref() != Ok("1") {
            return Self::default();
        }
        Self::start(data_dir.join("logs"))
    }
    fn start(directory: PathBuf) -> Self {
        let queue = Arc::new(TraceQueue::default());
        let writer_queue = queue.clone();
        let enabled = Arc::new(AtomicBool::new(true));
        let running = enabled.clone();
        let writer = std::thread::Builder::new()
            .name("resource-trace".into())
            .spawn(move || {
                let result = (|| -> io::Result<()> {
                    let mut file = RotatingFile::open(directory, FILE_BYTES)?;
                    drain_trace(&writer_queue, |row| file.append(row))
                })();
                running.store(false, Ordering::Release);
                if result.is_err() {
                    // Do not expose filesystem paths or application data in diagnostics.
                    eprintln!("[RESOURCE:WARN] lifecycle trace disabled: file I/O failed");
                }
            });
        let Ok(writer) = writer else {
            return Self::default();
        };
        let trace = Self(Some(Arc::new(Inner {
            queue,
            writer: Mutex::new(Some(writer)),
            enabled,
        })));
        trace.record(Event::Started, Sample::default());
        trace
    }
    pub fn enabled(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|inner| inner.enabled.load(Ordering::Acquire))
    }
    pub fn record(&self, event: Event, sample: Sample) {
        let Some(inner) = self.0.as_ref().filter(|_| self.enabled()) else {
            return;
        };
        inner.queue.push(Record {
            timestamp: chrono::Utc::now(),
            event,
            sample,
            dropped: 0,
        });
    }
}
struct RotatingFile {
    directory: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}
impl RotatingFile {
    fn path(&self, index: usize) -> PathBuf {
        self.directory.join(if index == 0 {
            "resource-lifecycle-v1.csv".into()
        } else {
            format!("resource-lifecycle-v1.{index}.csv")
        })
    }
    fn open(directory: PathBuf, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let mut result = Self {
            directory,
            file: None,
            bytes: 0,
            limit,
        };
        result.reopen()?;
        Ok(result)
    }
    fn reopen(&mut self) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(0))?;
        self.bytes = file.metadata()?.len();
        if self.bytes == 0 {
            file.write_all(HEADER.as_bytes())?;
            self.bytes = HEADER.len() as u64;
        }
        self.file = Some(file);
        Ok(())
    }
    fn append(&mut self, row: &str) -> io::Result<()> {
        if row.len() as u64 + HEADER.len() as u64 > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "trace row too large",
            ));
        }
        if self.bytes + row.len() as u64 > self.limit {
            self.file.take(); // Windows rename requires releasing our file handle.
            for index in (1..=ARCHIVES).rev() {
                let target = self.path(index);
                match fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
                let source = self.path(index - 1);
                match fs::rename(source, target) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            self.reopen()?;
        }
        self.file.as_mut().unwrap().write_all(row.as_bytes())?;
        self.bytes += row.len() as u64;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_bounds_disk_and_preserves_complete_csv_rows() {
        let dir = std::env::temp_dir().join(format!("resource-trace-{}", uuid::Uuid::new_v4()));
        let mut file = RotatingFile::open(dir.clone(), 512).unwrap();
        let row = Record {
            timestamp: chrono::Utc::now(),
            event: Event::RecordingFinished,
            sample: Sample {
                owned_pid: Some(42),
                ..Default::default()
            },
            dropped: 0,
        }
        .csv();
        for _ in 0..100 {
            file.append(&row).unwrap();
        }
        drop(file);
        let paths: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(paths.len(), ARCHIVES + 1);
        for path in paths {
            let text = fs::read_to_string(path.unwrap().path()).unwrap();
            assert!(text.starts_with(HEADER));
            assert!(text.ends_with('\n'));
            assert!(text.len() <= 512);
            assert!(text.lines().all(|line| line.split(',').count() == 15));
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn last_owner_drains_writer_and_releases_thread() {
        let dir = std::env::temp_dir().join(format!("resource-trace-{}", uuid::Uuid::new_v4()));
        let trace = ResourceTrace::start(dir.clone());
        let enabled = trace.0.as_ref().unwrap().enabled.clone();
        let clone = trace.clone();
        drop(trace);
        assert!(enabled.load(Ordering::Acquire));
        clone.record(
            Event::WindowChanged,
            Sample {
                visible: Some(false),
                ..Default::default()
            },
        );
        drop(clone);
        assert!(!enabled.load(Ordering::Acquire));
        let text = fs::read_to_string(dir.join("resource-lifecycle-v1.csv")).unwrap();
        assert!(text.contains("window_changed,false"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn disabled_trace_does_not_allocate_a_writer_and_unknown_pages_are_redacted() {
        let trace = ResourceTrace::default();
        trace.record(Event::Sample, Sample::default());
        assert!(!trace.enabled());
        assert_eq!(Page::from_name("secret=do-not-log").name(), "Other");
    }
    fn record(event: Event, pid: u32) -> Record {
        Record {
            timestamp: chrono::Utc::now(),
            event,
            sample: Sample {
                owned_pid: Some(pid),
                ..Default::default()
            },
            dropped: 0,
        }
    }
    #[test]
    fn samples_coalesce_and_final_finish_survives_a_full_queue() {
        let queue = TraceQueue::default();
        for _ in 0..100 {
            queue.push(record(Event::Sample, 0));
        }
        for pid in 1..QUEUE_SIZE as u32 {
            queue.push(record(Event::RecordingStarted, pid));
        }
        queue.push(record(Event::RecordingFinished, 42));
        let state = queue.state.lock().unwrap();
        assert_eq!(state.rows.len(), QUEUE_SIZE);
        assert_eq!(state.dropped, 100);
        assert!(state.rows.iter().all(|r| !r.event.periodic()));
        assert_eq!(
            state.rows.back().unwrap().event.name(),
            "recording_finished"
        );
    }
    #[test]
    fn boundary_overflow_is_bounded_and_emits_gap_before_final_finish() {
        let queue = TraceQueue::default();
        for pid in 0..QUEUE_SIZE as u32 {
            queue.push(record(Event::RecordingStarted, pid));
        }
        queue.push(record(Event::RecordingFinished, 42));
        queue.close();
        let mut csv = String::new();
        drain_trace(&queue, |row| {
            csv.push_str(row);
            Ok(())
        })
        .unwrap();
        let rows: Vec<_> = csv.lines().collect();
        assert_eq!(rows.len(), QUEUE_SIZE + 1);
        assert!(rows[0].contains(",trace_gap,"));
        assert!(rows[0].ends_with(",1"));
        assert!(rows.last().unwrap().contains(",recording_finished,"));
        assert!(rows.last().unwrap().ends_with(",42,1"));
    }
    #[test]
    fn final_dropped_sample_is_reported_even_without_a_later_event() {
        let queue = TraceQueue::default();
        queue.push(record(Event::RecordingStarted, 42));
        let mut csv = String::new();
        let mut appended = false;
        drain_trace(&queue, |row| {
            csv.push_str(row);
            if !appended {
                appended = true;
                // Simulate producers filling the queue while the writer is busy.
                for pid in 0..QUEUE_SIZE as u32 {
                    queue.push(record(Event::RecordingFinished, pid));
                }
                queue.push(record(Event::Sample, 0));
                queue.close();
            }
            Ok(())
        })
        .unwrap();
        assert!(csv.contains(",trace_gap,"));
        assert!(csv.lines().last().unwrap().ends_with(",1"));
    }
}
