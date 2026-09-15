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

/// Durable process-tree owner retained for a runtime child.
///
/// On Windows this owns a kill-on-close Job Object captured from the exact
/// spawned child handle. The root process is assigned through that handle
/// before PID-based descendant absorption, and snapshot assignments are pinned
/// to process creation identities so PID reuse cannot redirect ownership.
pub(crate) struct OwnedProcessTree {
    #[cfg(windows)]
    job: windows_tree::OwnedTreeJob,
}

impl OwnedProcessTree {
    pub(crate) fn capture(child: &Child) -> Result<Self> {
        #[cfg(windows)]
        {
            let pid = child.id().context("spawned child PID unavailable")?;
            let job = windows_tree::OwnedTreeJob::capture_child(child)?
                .with_context(|| format!("child exited before retained Job ownership pid={pid}"))?;
            Ok(Self { job })
        }

        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    /// Synchronous retained-owner cleanup used when polling observes the root
    /// already exited. Root exit is not treated as descendant cleanup.
    pub(crate) fn terminate_now(&self) -> Result<()> {
        #[cfg(windows)]
        loop {
            match self.job.active_process_count() {
                Ok(0) => break,
                Ok(_) | Err(_) => {
                    // The retained Job handle is the authoritative identity.
                    // Retry a transient TerminateJobObject/query failure
                    // instead of returning ownership to watcher state while
                    // any Job member may still be running.
                    let _ = self.job.terminate();
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
        Ok(())
    }

    /// Terminates the retained owned tree and reaps the root child.
    pub(crate) async fn terminate(&mut self, child: &mut Child) -> Result<Option<i32>> {
        #[cfg(windows)]
        loop {
            match self.job.active_process_count() {
                Ok(0) => break,
                Ok(_) | Err(_) => {
                    // Never fall back to a historical root PID here. The Job
                    // was captured from the exact spawned Child handle and is
                    // the only authoritative retained identity after spawn.
                    let _ = self.job.terminate();
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }

        #[cfg(not(windows))]
        if child.try_wait()?.is_none() {
            let _ = child.kill().await;
        }

        Ok(child.wait().await.ok().and_then(|status| status.code()))
    }
}

/// Terminates only the process tree rooted at a child spawned by this server.
///
/// Do not use root-process exit as proof that descendants are gone. Windows
/// cleanup first captures the exact spawned process handle into a Job Object;
/// retained LIVE cleanup keeps that Job for the whole recording lifetime.
pub(crate) async fn terminate_owned(child: &mut Child) {
    loop {
        if terminate_owned_checked(child).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Checked owned-tree termination used for callers that did not retain an owner
/// from spawn. LIVE recorder cleanup uses `OwnedProcessTree` instead.
pub(crate) async fn terminate_owned_checked(child: &mut Child) -> Result<Option<i32>> {
    #[cfg(windows)]
    if child.id().is_some() {
        terminate_windows_tree(child).await?;
    }

    #[cfg(not(windows))]
    let _ = child.kill().await;

    Ok(child.wait().await.ok().and_then(|status| status.code()))
}

#[cfg(windows)]
async fn terminate_windows_tree(child: &Child) -> Result<()> {
    loop {
        let Some(root_pid) = child.id() else {
            return Ok(());
        };

        match windows_tree::OwnedTreeJob::capture_child(child) {
            Ok(None) => return Ok(()),
            Ok(Some(job)) => {
                // The exact Child handle has already been assigned to this Job,
                // so do not send a historical PID to taskkill after capture.
                loop {
                    match job.active_process_count() {
                        Ok(0) => return Ok(()),
                        Ok(_) | Err(_) => {
                            let _ = job.terminate();
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }
                }
            }
            Err(_) => {
                // Generic callers do not retain a Job from spawn. Preserve the
                // exact-PID fallback only while the PID still resolves to the
                // same process object as this Child handle. Never target a PID
                // that has already been reused by Windows.
                if !windows_tree::child_pid_is_current(child)? {
                    return Ok(());
                }
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
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(windows)]
mod windows_tree {
    use anyhow::{Context, Result};
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        ffi::c_void,
        mem::size_of,
        os::windows::io::AsRawHandle,
        ptr,
    };
    use tokio::process::Child;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
                PROCESS_TERMINATE,
            },
        },
    };

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_INVALID_PARAMETER: i32 = 87;

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

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct ProcessIdentity {
        pid: u32,
        creation_time: u64,
    }

    fn filetime_value(value: FILETIME) -> u64 {
        ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
    }

    fn process_creation_time(handle: HANDLE) -> Result<u64> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok =
            unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error()).context("GetProcessTimes failed");
        }
        Ok(filetime_value(creation))
    }

    fn child_identity(child: &Child) -> Result<ProcessIdentity> {
        let pid = child.id().context("spawned child PID unavailable")?;
        let handle = child.as_raw_handle() as HANDLE;
        Ok(ProcessIdentity {
            pid,
            creation_time: process_creation_time(handle)
                .with_context(|| format!("failed to read creation identity for child pid={pid}"))?,
        })
    }

    fn open_identity(pid: u32, access: u32) -> Result<Option<(OwnedHandle, ProcessIdentity)>> {
        let process = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                return Ok(None);
            }
            if err.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                return Err(err)
                    .with_context(|| format!("OpenProcess access denied for pid={pid}"));
            }
            return Ok(None);
        }
        let process = OwnedHandle(process as isize);
        let identity = ProcessIdentity {
            pid,
            creation_time: process_creation_time(process.raw())
                .with_context(|| format!("failed to read creation identity for pid={pid}"))?,
        };
        Ok(Some((process, identity)))
    }

    fn query_identity(pid: u32) -> Result<Option<ProcessIdentity>> {
        Ok(open_identity(pid, 0)?.map(|(_, identity)| identity))
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

        /// Capture the exact spawned root process first, then absorb descendants
        /// that existed before Job assignment. Repeated snapshots are anchored
        /// to the root's creation identity, so a reused numeric PID is never
        /// followed or assigned to this retained Job.
        pub(super) fn capture_child(child: &Child) -> Result<Option<Self>> {
            let root = child_identity(child)?;
            let Some(initial) = process_tree_identities(root)? else {
                return Ok(None);
            };

            let job = Self::create()?;
            let root_handle = child.as_raw_handle() as HANDLE;
            job.assign_handle(root_handle).with_context(|| {
                format!(
                    "failed to assign exact spawned root pid={} to Job",
                    root.pid
                )
            })?;

            let mut assigned = HashSet::new();
            assigned.insert(root);
            let mut current = initial;

            loop {
                for identity in current.iter().copied() {
                    if assigned.contains(&identity) {
                        continue;
                    }
                    if job.assign_identity(identity)? {
                        assigned.insert(identity);
                    }
                }

                let Some(after) = process_tree_identities(root)? else {
                    // The original root exited or its numeric PID was reused.
                    // Never follow that PID again. The exact root was already
                    // assigned, and future descendants inherit its Job.
                    return Ok(Some(job));
                };
                if after.iter().all(|identity| assigned.contains(identity)) {
                    return Ok(Some(job));
                }
                current = after;
            }
        }

        fn contains_handle(&self, process: HANDLE) -> Result<bool> {
            let mut in_job = 0;
            let ok = unsafe { IsProcessInJob(process, self.handle.raw(), &mut in_job) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error()).context("IsProcessInJob failed");
            }
            Ok(in_job != 0)
        }

        fn assign_handle(&self, process: HANDLE) -> Result<()> {
            if self.contains_handle(process)? {
                return Ok(());
            }
            let ok = unsafe { AssignProcessToJobObject(self.handle.raw(), process) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("AssignProcessToJobObject failed");
            }
            Ok(())
        }

        fn assign_identity(&self, expected: ProcessIdentity) -> Result<bool> {
            let Some((process, current)) =
                open_identity(expected.pid, PROCESS_SET_QUOTA | PROCESS_TERMINATE)?
            else {
                return Ok(false);
            };
            if current != expected {
                // PID was reused between the snapshot and OpenProcess. Never
                // assign the replacement process to our retained Job.
                return Ok(false);
            }
            self.assign_handle(process.raw()).with_context(|| {
                format!(
                    "failed to assign pinned owned pid={} creation_time={} to Job",
                    expected.pid, expected.creation_time
                )
            })?;
            Ok(true)
        }

        pub(super) fn active_process_count(&self) -> Result<u32> {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle.raw(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast::<c_void>(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("QueryInformationJobObject(BasicAccounting) failed");
            }
            Ok(info.ActiveProcesses)
        }

        pub(super) fn terminate(&self) -> Result<()> {
            let ok = unsafe { TerminateJobObject(self.handle.raw(), 1) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error()).context("TerminateJobObject failed");
            }
            Ok(())
        }
    }

    fn process_tree_identities(root: ProcessIdentity) -> Result<Option<HashSet<ProcessIdentity>>> {
        let (children, present) = process_snapshot()?;
        if !present.contains(&root.pid) {
            return Ok(None);
        }
        if query_identity(root.pid)? != Some(root) {
            // The numeric root PID now names a different process. Never seed a
            // traversal from it, even if the replacement has descendants.
            return Ok(None);
        }

        let mut owned = HashSet::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([root.pid]);
        owned.insert(root);

        while let Some(pid) = queue.pop_front() {
            if !visited.insert(pid) {
                continue;
            }
            if let Some(descendants) = children.get(&pid) {
                for child_pid in descendants.iter().copied() {
                    if child_pid == pid {
                        continue;
                    }
                    if let Some(identity) = query_identity(child_pid)? {
                        owned.insert(identity);
                        queue.push_back(child_pid);
                    }
                }
            }
        }
        Ok(Some(owned))
    }

    fn process_snapshot() -> Result<(HashMap<u32, Vec<u32>>, HashSet<u32>)> {
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
        Ok((children, present))
    }

    pub(super) fn process_tree_pids(root_pid: u32) -> Result<HashSet<u32>> {
        let (children, present) = process_snapshot()?;
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
        if owned.is_empty() && !children.contains_key(&root_pid) {
            return Ok(HashSet::new());
        }
        Ok(owned)
    }

    pub(super) fn child_pid_is_current(child: &Child) -> Result<bool> {
        let expected = child_identity(child)?;
        Ok(query_identity(expected.pid)? == Some(expected))
    }

    #[cfg(test)]
    pub(super) fn child_snapshot_rejects_wrong_creation_identity(child: &Child) -> Result<bool> {
        let mut wrong = child_identity(child)?;
        wrong.creation_time = wrong.creation_time.wrapping_add(1);
        Ok(process_tree_identities(wrong)?.is_none())
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
    use super::{OwnedProcessTree, terminate_owned_checked};
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
    async fn windows_retained_tree_owner_survives_root_exit() {
        use super::windows_tree::{
            child_snapshot_rejects_wrong_creation_identity, process_tree_pids,
        };
        use std::{
            env, fs,
            time::{Duration, Instant},
        };
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

        assert!(
            child_snapshot_rejects_wrong_creation_identity(&root).unwrap(),
            "snapshot traversal must reject a mismatched root creation identity"
        );

        // This mirrors the production LIVE shape: retain ownership from the
        // exact spawned Child handle, keep that owner in the recording, then
        // allow the root to exit before cleanup is requested.
        let mut owner = OwnedProcessTree::capture(&root).unwrap();
        root.wait().await.unwrap();

        assert!(
            process_tree_pids(root_pid)
                .unwrap()
                .contains(&descendant_pid),
            "descendant should still be alive after the root exits"
        );

        owner.terminate(&mut root).await.unwrap();
        assert_eq!(
            owner.job.active_process_count().unwrap(),
            0,
            "retained Job must be empty before cleanup returns"
        );
        let _ = fs::remove_file(&pid_file);
    }
}
