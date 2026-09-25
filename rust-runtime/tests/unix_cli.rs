#![cfg(unix)]

use rusqlite::{Connection, params};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use stream_archive_server::runtime_owner::runtime_control_socket_path;
use tempfile::TempDir;

const CLI: &str = env!("CARGO_BIN_EXE_stream-archive-cli");
const SERVER: &str = env!("CARGO_BIN_EXE_stream-archive-server");

struct Layout {
    _root: TempDir,
    backend: PathBuf,
    data: PathBuf,
    fake_bin: PathBuf,
    live: PathBuf,
}

impl Layout {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("Stream Archive CLI 테스트 🎬 ")
            .tempdir()
            .unwrap();
        let backend = root.path().join("backend 한글");
        let data = root.path().join("data 한글");
        let fake_bin = root.path().join("fake tools");
        let live = root.path().join("LIVE 저장 한글 🎬");
        fs::create_dir_all(&fake_bin).unwrap();
        fs::create_dir_all(&live).unwrap();
        Self {
            _root: root,
            backend,
            data,
            fake_bin,
            live,
        }
    }

    fn command(&self, binary: &str) -> Command {
        let mut command = Command::new(binary);
        command
            .env("STREAM_ARCHIVE_BACKEND_DIR", &self.backend)
            .env("STREAM_ARCHIVE_DATA_DIR", &self.data)
            .env(
                "STREAM_ARCHIVE_BACKUP_DIR",
                self._root.path().join("backups 한글"),
            );
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![self.fake_bin.clone()];
        paths.extend(std::env::split_paths(&existing));
        command.env("PATH", std::env::join_paths(paths).unwrap());
        command
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command(CLI).args(args).output().unwrap()
    }

    fn init(&self) {
        let output = self.cli(&["init"]);
        assert_success("init", &output);
        assert!(self.backend.join("vod").is_dir());
        assert!(self.data.join("stream-archive.db").is_file());
    }

    fn stage_fake_tools(&self) {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media_tool_fixture.rs");
        let compiled = self._root.path().join("media-tool-fixture");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = Command::new(rustc)
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&compiled)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "failed to compile shared media-tool fixture"
        );

        for name in ["streamlink", "yt-dlp", "ffmpeg"] {
            let target = self.fake_bin.join(name);
            fs::copy(&compiled, &target).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn json_output(label: &str, output: Output) -> Value {
    assert_success(label, &output);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{label} did not emit clean JSON: {error}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn unix_cli_binary_daily_use_smoke_is_json_clean_and_unicode_safe() {
    let layout = Layout::new();
    assert_success("help", &layout.cli(&["help"]));
    assert_success("version", &layout.cli(&["version"]));
    layout.init();
    layout.stage_fake_tools();

    assert_success("tools configure", &layout.cli(&["tools", "configure"]));

    let settings = layout.cli(&[
        "settings",
        "set",
        "OUTPUT_DIR",
        layout.live.to_str().unwrap(),
    ]);
    assert_success("settings set", &settings);

    let doctor = json_output("doctor", layout.cli(&["doctor", "--json"]));
    assert_eq!(doctor["runtime_ready"], true);
    let active_doctor = json_output(
        "doctor active tools",
        layout.cli(&["doctor", "--json", "--active-tools"]),
    );
    assert_eq!(active_doctor["runtime_ready"], true);

    let status = json_output("status", layout.cli(&["status", "--json"]));
    assert_eq!(status["backend"], layout.backend.display().to_string());
    assert_eq!(
        status["database"],
        layout.data.join("stream-archive.db").display().to_string()
    );
    assert_eq!(status["tools"].as_array().unwrap().len(), 3);

    let environment = json_output("settings show", layout.cli(&["settings", "show", "--json"]));
    assert!(environment.as_array().unwrap().iter().any(|item| {
        item["key"] == "OUTPUT_DIR" && item["value"] == layout.live.display().to_string()
    }));

    let providers = json_output(
        "providers status",
        layout.cli(&["providers", "status", "--json"]),
    );
    let provider_text = serde_json::to_string(&providers).unwrap();
    assert!(!provider_text.contains("SOOP_PASSWORD"));
    assert!(!provider_text.contains("NID_AUT"));

    assert_success(
        "channels add",
        &layout.cli(&[
            "channels",
            "add",
            "soop",
            "fixture-account",
            "Fixture Channel",
            layout.live.to_str().unwrap(),
        ]),
    );
    let channels = json_output("channels list", layout.cli(&["channels", "list", "--json"]));
    assert_eq!(channels.as_array().unwrap().len(), 1);
    assert_eq!(channels[0]["account"], "fixture-account");

    assert_success(
        "channels disable",
        &layout.cli(&["channels", "disable", "soop", "fixture-account"]),
    );
    let channels = json_output(
        "channels disabled",
        layout.cli(&["channels", "list", "--json"]),
    );
    assert_eq!(channels[0]["enabled"], false);
    assert_success(
        "channels enable",
        &layout.cli(&["channels", "enable", "soop", "fixture-account"]),
    );
    let channels = json_output(
        "channels enabled",
        layout.cli(&["channels", "list", "--json"]),
    );
    assert_eq!(channels[0]["enabled"], true);

    let watcher = json_output(
        "watcher status",
        layout.cli(&["watcher", "status", "--json"]),
    );
    assert_eq!(watcher["running"], false);

    let queue = json_output("queue list", layout.cli(&["queue", "list", "--json"]));
    assert_eq!(queue["queued_count"], 0);

    let queued = json_output(
        "queue add",
        layout.cli(&[
            "queue",
            "add",
            "https://vod.sooplive.com/player/123456789",
            "--output",
            layout.live.to_str().unwrap(),
            "--json",
        ]),
    );
    let queue_id = queued["id"].as_str().unwrap().to_string();
    let queue = json_output("queue populated", layout.cli(&["queue", "list", "--json"]));
    assert_eq!(queue["queued_count"], 1);
    json_output(
        "queue cancel",
        layout.cli(&["queue", "cancel", &queue_id, "--json"]),
    );
    json_output(
        "queue retry",
        layout.cli(&["queue", "retry", &queue_id, "--json"]),
    );
    json_output(
        "queue recancel",
        layout.cli(&["queue", "cancel", &queue_id, "--json"]),
    );
    json_output(
        "queue remove",
        layout.cli(&["queue", "remove", &queue_id, "--json"]),
    );
    let queue = json_output("queue cleared", layout.cli(&["queue", "list", "--json"]));
    assert_eq!(queue["queued_count"], 0);

    let history = json_output("history list", layout.cli(&["history", "list", "--json"]));
    assert!(history["live"].is_array());
    assert!(history["vod"].is_array());

    let backup = json_output("backup status", layout.cli(&["backup", "status", "--json"]));
    assert!(backup["policy"].is_object());
    let backup = json_output("backup create", layout.cli(&["backup", "create", "--json"]));
    assert!(!backup["backups"].as_array().unwrap().is_empty());

    assert_success(
        "channels remove",
        &layout.cli(&["channels", "remove", "soop", "fixture-account"]),
    );
    let channels = json_output(
        "channels removed",
        layout.cli(&["channels", "list", "--json"]),
    );
    assert!(channels.as_array().unwrap().is_empty());

    let storage = json_output("storage", layout.cli(&["storage", "--json"]));
    assert!(storage["volumes"].is_array());

    let logs = json_output("logs", layout.cli(&["logs", "--json"]));
    assert!(logs.is_array());

    let tools = json_output("tools", layout.cli(&["tools", "--json"]));
    assert_eq!(tools.as_array().unwrap().len(), 3);
}

#[test]
fn queue_cancel_reaches_active_runtime_owner() {
    let layout = Layout::new();
    layout.init();
    layout.stage_fake_tools();
    assert_success("tools configure", &layout.cli(&["tools", "configure"]));

    fs::write(layout.fake_bin.join("yt-dlp.mode"), "run-hang").unwrap();
    let queued = json_output(
        "queue add active-cancel",
        layout.cli(&[
            "queue",
            "add",
            "https://vod.sooplive.com/player/987654321",
            "--output",
            layout.live.to_str().unwrap(),
            "--json",
        ]),
    );
    let queue_id = queued["id"].as_str().unwrap().to_string();

    let mut owner = spawn_runtime(&layout, CLI, &["serve"]);
    wait_for_path(
        &runtime_control_socket_path(&layout.data.join("stream-archive.db")).unwrap(),
        Duration::from_secs(8),
    );
    wait_for_queue_state(&layout.data.join("stream-archive.db"), &queue_id, "RUNNING");

    let cancelled = json_output(
        "active queue cancel",
        layout.cli(&["queue", "cancel", &queue_id, "--json"]),
    );
    let item = cancelled["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == queue_id)
        .expect("cancelled Queue item missing from snapshot");
    assert_eq!(item["state"], "CANCELLED");

    send_sigterm(owner.id());
    let status = wait_for_exit(&mut owner, Duration::from_secs(8));
    assert!(status.success(), "runtime owner SIGTERM exit was {status}");
}

#[test]
fn one_shot_cli_observes_running_owner_without_recovering_active_rows() {
    let layout = Layout::new();
    layout.init();
    let mut owner = spawn_runtime(&layout, CLI, &["serve"]);
    wait_for_path(
        &runtime_control_socket_path(&layout.data.join("stream-archive.db")).unwrap(),
        Duration::from_secs(8),
    );

    let db = layout.data.join("stream-archive.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "INSERT INTO live_recordings(id,platform,account,channel_name,started_at,status) VALUES(?1,'SOOP','fixture','Fixture','2026-09-23T00:00:00Z','RECORDING')",
        params!["live-owner-active"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO vod_jobs(id,platform,kind,state,updated_at) VALUES(?1,'SOOP','DOWNLOAD','DOWNLOADING','2026-09-23T00:00:00Z')",
        params!["vod-owner-active"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO vod_queue(id,platform,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at) VALUES(?1,'SOOP','{}','https://fixture.invalid/vod','/tmp','RUNNING',0,'','2026-09-23T00:00:00Z','2026-09-23T00:00:00Z')",
        params!["queue-owner-active"],
    )
    .unwrap();
    drop(conn);

    let status = json_output("status with owner", layout.cli(&["status", "--json"]));
    assert_eq!(status["runtime_owner_active"], true);
    assert_eq!(status["scope"], "runtime-owner");

    let conn = Connection::open(&db).unwrap();
    let live_status: String = conn
        .query_row(
            "SELECT status FROM live_recordings WHERE id='live-owner-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let vod_state: String = conn
        .query_row(
            "SELECT state FROM vod_jobs WHERE id='vod-owner-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let queue_state: String = conn
        .query_row(
            "SELECT state FROM vod_queue WHERE id='queue-owner-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live_status, "RECORDING");
    assert_eq!(vod_state, "DOWNLOADING");
    assert_eq!(queue_state, "RUNNING");

    let restore = layout.cli(&["backup", "restore", "missing.db", "--yes"]);
    assert!(!restore.status.success());
    let stderr = String::from_utf8_lossy(&restore.stderr);
    assert!(
        stderr.contains("cannot restore while another Stream Archive runtime owns")
            || stderr.contains("another Stream Archive runtime owns"),
        "unexpected restore error: {stderr}"
    );

    let logs = json_output("remote runtime logs", layout.cli(&["logs", "--json"]));
    assert!(logs.as_array().unwrap().iter().any(|line| {
        line.as_str()
            .is_some_and(|line| line.contains("headless runtime ready"))
    }));

    send_sigterm(owner.id());
    let status = wait_for_exit(&mut owner, Duration::from_secs(8));
    assert!(status.success(), "runtime owner SIGTERM exit was {status}");
}

#[test]
fn provider_secret_cli_rejects_secret_as_argv_value() {
    let layout = Layout::new();
    layout.init();
    let output = layout.cli(&[
        "providers",
        "secret",
        "SOOP_PASSWORD",
        "should-not-be-an-argv-secret",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--stdin"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("should-not-be-an-argv-secret"));
}

#[test]
fn unix_cli_serve_sigterm_is_graceful_and_does_not_kill_unrelated_runtime() {
    let primary = Layout::new();
    primary.init();
    let unrelated = Layout::new();
    unrelated.init();

    let mut unrelated_child = spawn_runtime(&unrelated, SERVER, &[]);
    let mut cli_child = spawn_runtime(&primary, CLI, &["serve"]);

    thread::sleep(Duration::from_millis(500));
    assert!(cli_child.try_wait().unwrap().is_none());
    assert!(unrelated_child.try_wait().unwrap().is_none());

    send_sigterm(cli_child.id());
    let status = wait_for_exit(&mut cli_child, Duration::from_secs(8));
    assert!(status.success(), "CLI serve SIGTERM exit was {status}");

    assert!(
        unrelated_child.try_wait().unwrap().is_none(),
        "unrelated headless runtime must survive another CLI runtime SIGTERM"
    );

    send_sigterm(unrelated_child.id());
    let status = wait_for_exit(&mut unrelated_child, Duration::from_secs(8));
    assert!(
        status.success(),
        "compatibility server SIGTERM exit was {status}"
    );
}

fn wait_for_queue_state(db: &std::path::Path, id: &str, expected: &str) {
    let started = Instant::now();
    loop {
        let state = Connection::open(db)
            .ok()
            .and_then(|conn| {
                conn.query_row(
                    "SELECT state FROM vod_queue WHERE id=?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .ok()
            });
        if state.as_deref() == Some(expected) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Queue item {id} did not reach {expected}; last state={state:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_path(path: &std::path::Path, timeout: Duration) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < timeout,
            "path did not appear within {timeout:?}: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn spawn_runtime(layout: &Layout, binary: &str, args: &[&str]) -> Child {
    layout
        .command(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

const SIGTERM: i32 = 15;

fn send_sigterm(pid: u32) {
    let result = unsafe { kill(pid as i32, SIGTERM) };
    assert_eq!(result, 0, "failed to send SIGTERM to pid={pid}");
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(
            started.elapsed() < timeout,
            "process {} did not exit within {timeout:?}",
            child.id()
        );
        thread::sleep(Duration::from_millis(25));
    }
}
