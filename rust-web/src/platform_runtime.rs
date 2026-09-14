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
/// Do not use root-process exit as proof that descendants are gone. The checked
/// Windows primitive captures the owned tree in a kill-on-close Job Object
/// before attempting termination, so ownership remains independently
/// enforceable even when the root exits first.
pub(crate) async fn terminate_owned(child: &mut Child) {
    loop {
        if terminate_owned_checked(child).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Checked owned-tree termination used by LIVE recorder cleanup.
///
/// On Windows the root PID is first captured together with its current
/// descendants in a kill-on-close Job Object. Future descendants of captured
/// members inherit the Job. We therefore never interpret `Child::try_wait()`
/// on the root as proof that the whole tree is gone.
pub(crate) async fn terminate_owned_checked(child: &mut Child) -> Result<Option<i32>> {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        terminate_windows_tree(pid).await?;
    }

    #[cfg(not(windows))]
    let _ = child.kill().await;

    Ok(child.wait().await.ok().and_then(|status| status.code()))
}

#[cfg(windows)]
async fn terminate_windows_tree(root_pid: u32) -> Result<()> {
    loop {
        match windows_tree::OwnedTreeJob::capture(root_pid) {
            Ok(None) => return Ok(()),
            Ok(Some(job)) => {
                // Preserve the existing exact-PID Windows tree request as a
                // best-effort fast path. Job ownership is the safety boundary:
                // even if taskkill fails or the root exits during this await,
                // the independently-owned descendants remain terminable.
                let _ = Command::new("taskkill.exe")
                    .arg("/PID")
                    .arg(root_pid.to_string())
                    .arg("/T")
                    .arg("/F")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;

                job.terminate()
                    .context("failed to terminate Windows owned process Job")?;

                // Do not return until the exact captured root lineage has
                // disappeared from the process table. Root exit alone is not
                // sufficient because a descendant may still be running.
                for _ in 0..200 {
                    if windows_tree::process_tree_pids(root_pid)?.is_empty() {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
            Err(_) => {
                // If Job setup is transiently unavailable, keep trying the
                // exact-PID tree fallback and then retry Job capture. Never
                // convert a root-only exit into cleanup success.
                let _ = Command::new("taskkill.exe")
                    .arg("/PID")
                    .arg(root_pid.to_string())
                    .arg("/T")
                    .arg("/F")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
                if windows_tree::process_tree_pids(root_pid)?.is_empty() {
                    return Ok(());
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(windows)]
mod windows_tree {
    use anyhow::{Context, Result, anyhow};
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        ffi::c_void,
        mem::size_of,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };

    struct OwnedHandle(isize);

    impl OwnedHandle {
        fn raw(&self) -> HANDLE {
            self.0 as HANDLE
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if self.0 != 0 && self.0 != INVALID_HANDLE_VALUE as isize {
                unsafe {
                    let _ = CloseHandle(self.raw());
                }
            }
        }
    }

    pub(super) struct OwnedTreeJob {
        handle: OwnedHandle,
    }

    impl OwnedTreeJob {
        fn create() -> Result<Self> {
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(std::io::Error::last_os_error()).context("CreateJobObjectW failed");
            }
            let job = Self {
                handle: OwnedHandle(handle as isize),
            };

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    job.handle.raw(),
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("SetInformationJobObject(KILL_ON_JOB_CLOSE) failed");
            }
            Ok(job)
        }

        /// Capture the current root lineage. Assigning the root first makes
        /// subsequently-created descendants inherit the Job; repeated process
        /// snapshots then absorb descendants that existed before assignment.
        pub(super) fn capture(root_pid: u32) -> Result<Option<Self>> {
            let initial = process_tree_pids(root_pid)?;
            if initial.is_empty() {
                return Ok(None);
            }

            let job = Self::create()?;
            let mut assigned = HashSet::new();

            loop {
                let current = process_tree_pids(root_pid)?;
                if current.is_empty() {
                    return if assigned.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(job))
                    };
                }

                for pid in current.iter().copied() {
                    if assigned.contains(&pid) {
                        continue;
                    }
                    match job.assign(pid) {
                        Ok(()) => {
                            assigned.insert(pid);
                        }
                        Err(err) => {
                            // A process may vanish between snapshot and open.
                            // Ignore only that race; a still-live member that
                            // cannot be assigned means Job ownership was not
                            // established and must not be reported as success.
                            if process_tree_pids(root_pid)?.contains(&pid) {
                                return Err(err).with_context(|| {
                                    format!("failed to assign owned pid={pid} to Job Object")
                                });
                            }
                        }
                    }
                }

                let after = process_tree_pids(root_pid)?;
                if after.iter().all(|pid| assigned.contains(pid)) {
                    return Ok(Some(job));
                }
            }
        }

        fn assign(&self, pid: u32) -> Result<()> {
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return Err(std::io::Error::last_os_error())
                    .with_context(|| format!("OpenProcess failed for pid={pid}"));
            }
            let process = OwnedHandle(process as isize);
            let ok = unsafe { AssignProcessToJobObject(self.handle.raw(), process.raw()) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error())
                    .with_context(|| format!("AssignProcessToJobObject failed for pid={pid}"));
            }
            Ok(())
        }

        pub(super) fn terminate(&self) -> Result<()> {
            let ok = unsafe { TerminateJobObject(self.handle.raw(), 1) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error()).context("TerminateJobObject failed");
            }
            Ok(())
        }
    }

    pub(super) fn process_tree_pids(root_pid: u32) -> Result<HashSet<u32>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .context("CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS) failed");
        }
        let snapshot = OwnedHandle(snapshot as isize);

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut present = HashSet::new();

        let mut ok = unsafe { Process32FirstW(snapshot.raw(), &mut entry) };
        while ok != 0 {
            present.insert(entry.th32ProcessID);
            children
                .entry(entry.th32ParentProcessID)
                .or_default()
                .push(entry.th32ProcessID);
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            ok = unsafe { Process32NextW(snapshot.raw(), &mut entry) };
        }

        let mut owned = HashSet::new();
        let mut queue = VecDeque::new();
        if present.contains(&root_pid) || children.contains_key(&root_pid) {
            queue.push_back(root_pid);
        }
        while let Some(pid) = queue.pop_front() {
            if present.contains(&pid) {
                owned.insert(pid);
            }
            if let Some(descendants) = children.get(&pid) {
                for child in descendants {
                    if *child != pid && !owned.contains(child) {
                        queue.push_back(*child);
                    }
                }
            }
        }

        // If the root has already exited, it is intentionally absent from the
        // result while descendants whose recorded parent is root_pid remain.
        if owned.is_empty() && !children.contains_key(&root_pid) {
            return Ok(HashSet::new());
        }
        Ok(owned)
    }

    #[allow(dead_code)]
    fn _handle_is_send_guard(_: &OwnedTreeJob) -> Result<()> {
        if size_of::<HANDLE>() == 0 {
            return Err(anyhow!("unreachable"));
        }
        Ok(())
    }
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

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_job_retains_descendant_after_root_exit() {
        use super::windows_tree::{OwnedTreeJob, process_tree_pids};
        use std::{env, fs, time::Instant};
        use uuid::Uuid;

        let pid_file = env::temp_dir().join(format!(
            "stream-archive-job-test-{}.pid",
            Uuid::new_v4().simple()
        ));
        let script = concat!(
            "$p = Start-Process -FilePath \"$env:SystemRoot\\System32\\ping.exe\" ",
            "-ArgumentList @('-n','10','127.0.0.1') -WindowStyle Hidden -PassThru; ",
            "[IO.File]::WriteAllText($env:SOOP_JOB_TEST_PID_FILE, [string]$p.Id); ",
            "Start-Sleep -Milliseconds 1200"
        );
        let mut root = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .env("SOOP_JOB_TEST_PID_FILE", &pid_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let root_pid = root.id().unwrap();

        let started = Instant::now();
        let descendant_pid = loop {
            if let Ok(text) = fs::read_to_string(&pid_file)
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                break pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "timed out waiting for descendant pid"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };

        let job = OwnedTreeJob::capture(root_pid)
            .unwrap()
            .expect("root tree must exist while capture starts");
        root.wait().await.unwrap();

        assert!(
            process_tree_pids(root_pid)
                .unwrap()
                .contains(&descendant_pid),
            "descendant should still be alive after the root exits"
        );

        job.terminate().unwrap();
        for _ in 0..200 {
            if !process_tree_pids(root_pid)
                .unwrap()
                .contains(&descendant_pid)
            {
                let _ = fs::remove_file(&pid_file);
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        let _ = fs::remove_file(&pid_file);
        panic!("Job Object did not terminate surviving descendant pid={descendant_pid}");
    }
}
