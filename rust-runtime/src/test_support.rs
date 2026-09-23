use crate::{platform_runtime::test_process_running, tool_discovery::ToolKind};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child as StdChild, Command as StdCommand, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tempfile::TempDir;

static PROVIDER_FIXTURE: OnceLock<PathBuf> = OnceLock::new();

pub(crate) struct ProviderFixture {
    root: TempDir,
}

impl ProviderFixture {
    pub(crate) fn new() -> Self {
        Self {
            root: tempfile::Builder::new()
                .prefix("Stream Archive provider 한글 🎬 ")
                .tempdir()
                .expect("provider fixture tempdir"),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(crate) fn tool(&self, kind: ToolKind) -> PathBuf {
        let target = self.root.path().join(tool_name(kind));
        fs::copy(provider_fixture_path(), &target).expect("stage provider tool fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                .expect("mark provider fixture executable");
            if kind == ToolKind::Ffmpeg {
                let windows_layout = self.root.path().join("ffmpeg.exe");
                fs::copy(&target, &windows_layout).expect("stage Streamlink-layout FFmpeg fixture");
                fs::set_permissions(&windows_layout, fs::Permissions::from_mode(0o755))
                    .expect("mark Streamlink-layout FFmpeg fixture executable");
            }
        }
        target
    }

    pub(crate) fn set_mode(&self, executable: &Path, mode: &str) {
        fs::write(mode_path(executable), mode).expect("write provider fixture mode");
    }

    pub(crate) fn set_manifest_url(&self, executable: &Path, url: &str) {
        fs::write(sidecar_path(executable, "manifest-url"), url)
            .expect("write provider fixture manifest URL");
    }

    pub(crate) fn invocations(&self) -> String {
        fs::read_to_string(self.root.path().join("invocations.log")).unwrap_or_default()
    }

    pub(crate) fn child_pid_path(&self, executable: &Path) -> PathBuf {
        let name = executable
            .file_name()
            .expect("fixture executable name")
            .to_string_lossy();
        self.root.path().join(format!("{name}.child.pid"))
    }

    pub(crate) fn child_ready_path(&self, executable: &Path) -> PathBuf {
        let name = executable
            .file_name()
            .expect("fixture executable name")
            .to_string_lossy();
        self.root.path().join(format!("{name}.child.ready"))
    }

    pub(crate) async fn wait_for_path(&self, path: &Path) {
        let started = Instant::now();
        while !path.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(8),
                "timed out waiting for provider fixture path {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub(crate) async fn assert_child_stopped(&self, executable: &Path) {
        let pid_path = self.child_pid_path(executable);
        self.wait_for_path(&pid_path).await;
        let pid: u32 = fs::read_to_string(&pid_path)
            .expect("read provider fixture child pid")
            .trim()
            .parse()
            .expect("parse provider fixture child pid");
        let started = Instant::now();
        while test_process_running(pid) && started.elapsed() < Duration::from_secs(5) {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            !test_process_running(pid),
            "owned provider fixture descendant {pid} must be stopped"
        );
    }

    pub(crate) fn spawn_unrelated(&self) -> StdChild {
        let ready = self.root.path().join("unrelated.ready");
        StdCommand::new(provider_fixture_path())
            .env("STREAM_ARCHIVE_FIXTURE_GENERIC", "1")
            .args(["sleep", "30000"])
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn unrelated provider fixture")
    }

    pub(crate) async fn wait_for_unrelated(&self) {
        self.wait_for_path(&self.root.path().join("unrelated.ready"))
            .await;
    }
}

pub(crate) struct LocalManifestServer {
    url: String,
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LocalManifestServer {
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local manifest fixture");
        let address = listener.local_addr().expect("local manifest fixture address");
        listener
            .set_nonblocking(true)
            .expect("configure local manifest fixture");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::spawn(move || {
            let body =
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080\nfixture.ts\n";
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0u8; 4096];
                        let _ = stream.read(&mut request);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{address}/master.m3u8"),
            address,
            stop,
            thread: Some(thread),
        }
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for LocalManifestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = std::net::TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn provider_fixture_path() -> &'static Path {
    PROVIDER_FIXTURE
        .get_or_init(|| {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let source = manifest.join("tests/fixtures/media_tool_fixture.rs");
            let output_dir = manifest.join("target/test-fixtures");
            fs::create_dir_all(&output_dir).expect("create provider fixture output directory");
            let suffix = if cfg!(windows) { ".exe" } else { "" };
            let output = output_dir.join(format!("provider-media-tool-fixture{suffix}"));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
            let status = StdCommand::new(rustc)
                .arg("--edition=2024")
                .arg(&source)
                .arg("-o")
                .arg(&output)
                .status()
                .expect("compile provider media-tool fixture");
            assert!(
                status.success(),
                "failed to compile provider media-tool fixture"
            );
            output
        })
        .as_path()
}

fn tool_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Streamlink => {
            if cfg!(windows) {
                "streamlink.exe"
            } else {
                "streamlink"
            }
        }
        ToolKind::YtDlp => {
            if cfg!(windows) {
                "yt-dlp.exe"
            } else {
                "yt-dlp"
            }
        }
        ToolKind::Ffmpeg => {
            if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            }
        }
    }
}

fn mode_path(executable: &Path) -> PathBuf {
    sidecar_path(executable, "mode")
}

fn sidecar_path(executable: &Path, suffix: &str) -> PathBuf {
    let name = executable
        .file_name()
        .expect("fixture executable name")
        .to_string_lossy();
    executable
        .parent()
        .expect("fixture executable parent")
        .join(format!("{name}.{suffix}"))
}
