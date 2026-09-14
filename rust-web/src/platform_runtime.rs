//! Operating-system boundary for runtime process and file ownership operations.

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

/// Checked owned-tree termination used by LIVE recorder cleanup.
///
/// On Windows this function never returns while the owned child can still be
/// running merely because `taskkill` failed to launch or returned a failure.
/// It retries in bounded rounds with a delay between rounds, preserving the
/// `Child` owner on the stack until the exact PID tree exits. Any returned
/// error therefore cannot be used to abandon a still-running tree due to a
/// transient `taskkill` failure.
pub(crate) async fn terminate_owned_checked(child: &mut Child) -> Result<Option<i32>> {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        const ATTEMPTS_PER_ROUND: usize = 3;

        loop {
            let mut tree_stopped = false;

            for attempt in 0..ATTEMPTS_PER_ROUND {
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
                    Ok(_) | Err(_) => {
                        if let Ok(Some(exit)) = child.try_wait() {
                            return Ok(exit.code());
                        }
                    }
                }

                if attempt + 1 < ATTEMPTS_PER_ROUND {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }

            if tree_stopped {
                break;
            }
            if let Ok(Some(exit)) = child.try_wait() {
                return Ok(exit.code());
            }

            // Keep the Recording/Child owner live across retry rounds instead
            // of returning it to disposable watcher state while descendants
            // may still be recording.
            tokio::time::sleep(Duration::from_millis(250)).await;
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
        anyhow::bail!("CHZZK 임시 폴더를 현재 사용자 전용으로 제한하지 못했습니다.");
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
