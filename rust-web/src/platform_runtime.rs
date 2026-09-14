//! Operating-system boundary for runtime process and file ownership operations.

#[cfg(windows)]
use anyhow::bail;
use anyhow::{Context, Result};
use std::path::Path;
#[cfg(windows)]
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

pub(crate) fn configure_utf8_cli(command: &mut Command) {
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
}

/// Terminates only the process tree rooted at a child spawned by this server.
///
/// Callers that do not surface termination errors must not return while a live
/// owned tree can still exist. Keep retrying the checked primitive until the
/// child has exited or the Windows tree kill succeeds.
pub(crate) async fn terminate_owned(child: &mut Child) {
    loop {
        match terminate_owned_checked(child).await {
            Ok(_) => return,
            Err(_) => {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

/// Checked form used when failure to stop an owned tree must be surfaced.
///
/// On Windows a transient `taskkill` launch/exit failure is retried before an
/// error is returned. If all attempts fail while the child is still running,
/// the `Child` is deliberately left alive and owned by the caller so it can be
/// retained and retried rather than dropped with descendants potentially alive.
pub(crate) async fn terminate_owned_checked(child: &mut Child) -> Result<Option<i32>> {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        const ATTEMPTS: usize = 3;
        let mut last_failure = String::new();
        let mut tree_stopped = false;

        for attempt in 0..ATTEMPTS {
            match Command::new("taskkill.exe")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/T")
                .arg("/F")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
            {
                Ok(status) if status.success() => {
                    tree_stopped = true;
                    break;
                }
                Ok(status) => {
                    if let Some(exit) = child.try_wait()? {
                        return Ok(exit.code());
                    }
                    last_failure = format!("taskkill exit={:?}", status.code());
                }
                Err(err) => {
                    if let Some(exit) = child.try_wait()? {
                        return Ok(exit.code());
                    }
                    last_failure = format!("taskkill launch failed: {err}");
                }
            }

            if attempt + 1 < ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        if !tree_stopped && child.try_wait()?.is_none() {
            bail!("failed to terminate owned pid={pid} tree after {ATTEMPTS} attempts: {last_failure}");
        }
    }

    #[cfg(not(windows))]
    let _ = child.kill().await;

    Ok(child.wait().await.ok().and_then(|status| status.code()))
}

#[cfg(windows)]
pub(crate) fn restrict_private_dir(dir: &Path) -> Result<()> {
    let username = std::env::var("USERNAME").context("Windows USERNAME 환경 변수가 없습니다.")?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let identity = if domain.trim().is_empty() || domain == "." {
        username
    } else {
        format!("{domain}\\{username}")
    };
    let status = std::process::Command::new("icacls.exe")
        .arg(dir)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{identity}:(OI)(CI)F"))
        .arg("/Q")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("CHZZK 임시 폴더 ACL 설정 실패: {}", dir.display()))?;
    if !status.success() {
        bail!("CHZZK 임시 폴더를 현재 사용자 전용으로 제한하지 못했습니다.");
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn restrict_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("CHZZK 임시 폴더 권한 설정 실패: {}", dir.display()))
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn restrict_private_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::terminate_owned_checked;
    use std::process::Stdio;
    use tokio::process::Command;

    #[tokio::test]
    async fn owned_child_is_terminated_and_reaped() {
        #[cfg(windows)]
        let mut child = Command::new("cmd.exe")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        assert!(child.id().is_some());
        terminate_owned_checked(&mut child).await.unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }
}
