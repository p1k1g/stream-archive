//! Test harness only: global SQLite state lives until process exit. Keep the
//! presentation source free of child-process ownership, including fixture code.
use std::{path::PathBuf, process::Command};

pub fn run(name: &str, test: impl FnOnce(PathBuf)) {
    if let Some(directory) = std::env::var_os("STREAM_ARCHIVE_TEST_DESKTOP_DIR") {
        test(PathBuf::from(directory));
        return;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("stream-archive-close-{nonce}"));
    std::fs::create_dir_all(&directory).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("STREAM_ARCHIVE_TEST_DESKTOP_DIR", &directory)
        .output()
        .expect("run isolated desktop lifecycle test");
    assert!(
        output.status.success(),
        "isolated lifecycle test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The child exited; its global SQLite connection and OS ownership locks
    // must now be released. Do not ignore Windows sharing violations here.
    std::fs::remove_dir_all(directory).unwrap();
}
