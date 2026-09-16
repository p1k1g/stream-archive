use anyhow::{Context, Result, bail};
use std::{
    io,
    os::unix::process::CommandExt as _,
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

const TERM_GRACE: Duration = Duration::from_secs(2);
const KILL_SETTLE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(super) fn configure_process_group(command: &mut Command) {
    // setpgid(0, 0) runs in the child before exec. Every descendant that does
    // not intentionally detach therefore inherits a group created exclusively
    // for this Stream Archive-owned root process.
    command.as_std_mut().process_group(0);
}

pub(super) struct OwnedProcessGroup {
    pgid: libc::pid_t,
}

impl OwnedProcessGroup {
    pub(super) fn from_spawned_child(child: &Child) -> Result<Self> {
        let pid = child.id().context("spawned Unix child PID unavailable")?;
        let pgid = libc::pid_t::try_from(pid).context("spawned Unix child PID exceeds pid_t")?;
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
        let expected =
            libc::pid_t::try_from(pid).context("running Unix child PID exceeds pid_t")?;
        let actual = unsafe { libc::getpgid(expected) };
        if actual == -1 {
            return Err(io::Error::last_os_error())
                .with_context(|| format!("getpgid failed for running Unix child pid={pid}"));
        }
        if actual != expected {
            bail!(
                "running Unix child pid={pid} is not an isolated process-group leader (pgid={actual})"
            );
        }
        Ok(Self { pgid: actual })
    }

    fn signal_group(&self, signal: libc::c_int) -> Result<()> {
        let rc = unsafe { libc::kill(-self.pgid, signal) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
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
        let rc = unsafe { libc::kill(-self.pgid, 0) };
        if rc == 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::ESRCH => Ok(false),
            Some(code) if code == libc::EPERM => Ok(true),
            _ => Err(err).with_context(|| {
                format!(
                    "failed to query owned Unix process group pgid={}",
                    self.pgid
                )
            }),
        }
    }

    pub(super) fn terminate_now(&self) -> Result<()> {
        self.signal_group(libc::SIGTERM)?;
        let deadline = Instant::now() + TERM_GRACE;
        while self.group_exists()? && Instant::now() < deadline {
            std::thread::sleep(POLL_INTERVAL);
        }
        if self.group_exists()? {
            self.signal_group(libc::SIGKILL)?;
            let settle_deadline = Instant::now() + KILL_SETTLE;
            while self.group_exists()? && Instant::now() < settle_deadline {
                std::thread::sleep(POLL_INTERVAL);
            }
        }
        Ok(())
    }

    pub(super) async fn terminate(&self, child: &mut Child) -> Result<()> {
        self.signal_group(libc::SIGTERM)?;
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
            self.signal_group(libc::SIGKILL)?;
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
            let _ = self.signal_group(libc::SIGKILL);
        }
    }
}
