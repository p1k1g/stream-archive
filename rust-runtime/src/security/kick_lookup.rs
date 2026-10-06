//! Killable native lookup: a blocked Keychain call cannot be cancelled as a thread.
//! The same executable preserves the native store identity and owns the DB lock.

use anyhow::{Result, bail};
use std::{
    io::Write,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

const WORKER_ARG: &str = "--internal-kick-secret-lookup";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: usize = 64 * 1024;

pub(super) fn worker_entry() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WORKER_ARG)) {
        return None;
    }
    let result = (|| -> Result<()> {
        let path = args.next().map(std::path::PathBuf::from);
        let Some(path) = path.filter(|path| path.is_file()) else {
            bail!("invalid lookup database");
        };
        if args.next().is_some() {
            bail!("invalid lookup arguments");
        }
        let store = crate::store::Store::open_observer(path)?;
        let mut token = super::read_kick_token(&store)?.into_bytes();
        let result = if token.len() <= MAX_OUTPUT {
            std::io::stdout().lock().write_all(&token)
        } else {
            Err(std::io::Error::other("lookup output too large"))
        };
        token.fill(0);
        result?;
        Ok(())
    })();
    // Native errors may contain secrets. Never send them to CLI logging/stderr.
    Some(if result.is_ok() { 0 } else { 1 })
}

pub(super) async fn read(store: &crate::store::Store, cancel: &AtomicBool) -> Result<String> {
    let executable = std::env::current_exe()
        .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 helper 경로 확인 실패"))?;
    let mut command = Command::new(executable);
    command.arg(WORKER_ARG).arg(store.path());
    run_lookup(&mut command, cancel, LOOKUP_TIMEOUT).await
}

async fn run_lookup(
    command: &mut Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String> {
    if cancel.load(Ordering::Acquire) {
        bail!("KICK 인증정보 조회가 취소되었습니다.");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let (mut child, mut owner) = crate::platform_runtime::spawn_owned(command)
        .await
        .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 helper 실행 실패"))?;
    let mut stdout = child.stdout.take().expect("piped lookup stdout");
    let mut bytes = Vec::new();
    let operation = async {
        (&mut stdout)
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 응답 읽기 실패"))?;
        if bytes.len() > MAX_OUTPUT {
            bail!("KICK 인증정보 조회 응답 크기 초과");
        }
        let status = child
            .wait()
            .await
            .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 helper 대기 실패"))?;
        if !status.success() {
            bail!("KICK native 인증정보 조회 실패. Secret Service / Keychain 상태를 확인하세요.");
        }
        Ok(())
    };
    let result: Result<()> = tokio::select! {
        result = operation => result,
        _ = tokio::time::sleep(timeout) => {
            Err(anyhow::anyhow!("KICK 인증정보 조회 시간이 초과되었습니다. Secret Service / Keychain 상태를 확인하세요."))
        }
        _ = wait_cancel(cancel) => Err(anyhow::anyhow!("KICK 인증정보 조회가 취소되었습니다.")),
    };
    // Reap even on success: descendants may still hold stdout or the file lock.
    // Dropping an unbounded spawn_blocking task would leave the lock held instead.
    let cleanup = owner.terminate(&mut child).await;
    let token = if cleanup.is_err() {
        Err(anyhow::anyhow!("KICK 인증정보 조회 helper 정리 실패"))
    } else {
        result.and_then(|()| {
            std::str::from_utf8(&bytes)
                .map(str::to_owned)
                .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 응답 형식 오류"))
        })
    };
    bytes.fill(0);
    token
}

async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hung_lookup_timeout_and_cancel_release_lock_and_preserve_unrelated_process() {
        for cancelled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = crate::store::Store::open(dir.path().join("test.db")).unwrap();
            let ready = dir.path().join("ready");
            let mut lock = store.path().as_os_str().to_os_string();
            lock.push(".kick-secret.lock");
            let mut unrelated = Command::new("sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut command = Command::new("python3");
            command
                .args(["-c", "import fcntl,sys,time,pathlib,subprocess; f=open(sys.argv[1],'a'); fcntl.flock(f,fcntl.LOCK_SH); p=subprocess.Popen(['sleep','30']); pathlib.Path(sys.argv[2]).write_text(str(p.pid)); time.sleep(30)"])
                .arg(lock)
                .arg(&ready);
            let cancel = AtomicBool::new(false);
            let lookup = run_lookup(&mut command, &cancel, Duration::from_secs(2));
            let trigger = async {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                while !ready.exists() {
                    assert!(tokio::time::Instant::now() < deadline);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(super::super::kick_secret_guard(&store).is_err());
                if cancelled {
                    cancel.store(true, Ordering::Release);
                }
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(8), async {
                tokio::join!(lookup, trigger)
            })
            .await
            .unwrap();
            let error = result.unwrap_err().to_string();
            assert!(error.contains(if cancelled { "취소" } else { "초과" }));
            assert!(super::super::kick_secret_guard(&store).is_ok());
            let descendant: u32 = std::fs::read_to_string(&ready).unwrap().parse().unwrap();
            assert!(!crate::platform_runtime::test_process_running(descendant));
            assert!(unrelated.try_wait().unwrap().is_none());
            unrelated.kill().await.unwrap();
            unrelated.wait().await.unwrap();
        }
    }

    #[tokio::test]
    async fn lookup_protocol_rejects_failure_invalid_and_oversized_output_without_exposing_it() {
        for (script, success) in [
            ("printf fixture-token", true),
            (
                "printf fixture-token; printf fixture-token >&2; exit 1",
                false,
            ),
            ("printf '\\377'", false),
            ("head -c 65537 /dev/zero", false),
        ] {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            let result = run_lookup(
                &mut command,
                &AtomicBool::new(false),
                Duration::from_secs(5),
            )
            .await;
            if success {
                assert_eq!(result.unwrap(), "fixture-token");
            } else {
                assert!(!result.unwrap_err().to_string().contains("fixture-token"));
            }
        }
    }
}
