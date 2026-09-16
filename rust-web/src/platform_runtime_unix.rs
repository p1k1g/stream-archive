use anyhow::{Context, Result, bail};
use std::{
    io,
    os::unix::process::CommandExt as _,
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const ESRCH: i32 = 3;
const EPERM: i32 = 1;
const TERM_GRACE: Duration = Duration::from_secs(2);
const KILL_SETTLE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

type UnixPid = i32;

unsafe extern "C" {
    #[link_name = "kill"]
    fn posix_kill(pid: UnixPid, signal: i32) -> i32;
    #[link_name = "getpgid"]
    fn posix_getpgid(pid: UnixPid) -> UnixPid;
}

pub(super) fn configure_process_group(command: &mut Command) {
    // setpgid(0, 0) runs in the child before exec. Every descendant that does
    // not intentionally detach therefore inherits a group created exclusively
    // for this Stream Archive-owned root process.
    command.as_std_mut().process_group(0);
}

pub(super) struct OwnedProcessGroup {
    pgid: UnixPid,
}

impl OwnedProcessGroup {
    pub(super) fn from_spawned_child(child: &Child) -> Result<Self> {
        let pid = child.id().context("spawned Unix child PID unavailable")?;
        let pgid = UnixPid::try_from(pid).context("spawned Unix child PID exceeds pid_t")?;
        if pgid <= 0 {
            bail!("spawned Unix child PID is not positive: {pid}");
        }
        Ok(Self { pgid })
    }

    /// Compatibility capture is safe only when the already-running child is
    /// itself an isolated process-group leader. Never adopt the server's own
    /// process group or an unrelated group discovered from a numeric PID.
    pub(super) fn capture_running_child(child: &Child) -> Result<Self> {
        let pid = child.id().context("running Unix child PID unavailable")?;
        let expected = UnixPid::try_from(pid).context("running Unix child PID exceeds pid_t")?;
        let actual = process_group(expected)
            .with_context(|| format!("getpgid failed for running Unix child pid={pid}"))?;
        if actual != expected {
            bail!(
                "running Unix child pid={pid} is not an isolated process-group leader (pgid={actual})"
            );
        }
        Ok(Self { pgid: actual })
    }

    fn signal_group(&self, signal: i32) -> Result<()> {
        // Negative PID is the POSIX process-group addressing form. `pgid` is
        // created and retained by this owner; no process-name lookup is used.
        let rc = unsafe { posix_kill(-self.pgid, signal) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(ESRCH) {
            return Ok(());
        }
        Err(err).with_context(|| {
            format!(
                "failed to signal owned Unix process group pgid={} signal={signal}",
                self.pgid
            )
        })
    }

    fn group_exists(&self) -> Result<bool> {
        let rc = unsafe { posix_kill(-self.pgid, 0) };
        if rc == 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(ESRCH) => Ok(false),
            Some(EPERM) => Ok(true),
            _ => Err(err).with_context(|| {
                format!(
                    "failed to query owned Unix process group pgid={}",
                    self.pgid
                )
            }),
        }
    }

    pub(super) fn terminate_now(&self) -> Result<()> {
        self.signal_group(SIGTERM)?;
        let deadline = Instant::now() + TERM_GRACE;
        while self.group_exists()? && Instant::now() < deadline {
            std::thread::sleep(POLL_INTERVAL);
        }
        if self.group_exists()? {
            self.signal_group(SIGKILL)?;
            let settle_deadline = Instant::now() + KILL_SETTLE;
            while self.group_exists()? && Instant::now() < settle_deadline {
                std::thread::sleep(POLL_INTERVAL);
            }
        }
        Ok(())
    }

    pub(super) async fn terminate(&self, child: &mut Child) -> Result<()> {
        self.signal_group(SIGTERM)?;
        let deadline = Instant::now() + TERM_GRACE;
        loop {
            // Reap the root as soon as it exits. Otherwise a zombie group leader
            // can make kill(-pgid, 0) look live for the whole graceful window.
            let _ = child.try_wait()?;
            if !self.group_exists()? || Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        if self.group_exists()? {
            self.signal_group(SIGKILL)?;
            let settle_deadline = Instant::now() + KILL_SETTLE;
            loop {
                let _ = child.try_wait()?;
                if !self.group_exists()? || Instant::now() >= settle_deadline {
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
        Ok(())
    }
}

impl Drop for OwnedProcessGroup {
    fn drop(&mut self) {
        // Windows Job Objects have kill-on-close semantics. Unix has no durable
        // process-group handle, so retain the same fail-closed ownership rule by
        // sending SIGKILL only while the owned group still exists at drop time.
        if self.group_exists().unwrap_or(false) {
            let _ = self.signal_group(SIGKILL);
        }
    }
}

fn process_group(pid: UnixPid) -> Result<UnixPid, io::Error> {
    let pgid = unsafe { posix_getpgid(pid) };
    if pgid == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(pgid)
    }
}

#[cfg(test)]
pub(super) fn test_process_group(pid: i32) -> Result<i32, io::Error> {
    process_group(pid)
}

#[cfg(test)]
pub(super) fn test_process_exists(pid: i32) -> bool {
    let rc = unsafe { posix_kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(ESRCH)
}
