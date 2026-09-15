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
/// Windows LIVE callers should obtain this through [`spawn_owned`]. The child
/// is created suspended, its exact process handle is assigned to a kill-on-close
/// Job Object before the first user instruction can execute, and only then is
/// the child resumed. This closes both the root-exit and descendant PID-reuse
/// windows that exist when ownership is reconstructed after a normal spawn.
pub(crate) struct OwnedProcessTree {
    #[cfg(windows)]
    job: windows_tree::OwnedTreeJob,
}

impl OwnedProcessTree {
    /// Capture an already-running child for compatibility callers.
    ///
    /// Retained LIVE ownership must use `spawn_owned` instead: only suspended
    /// creation can guarantee that no descendant exists before the root enters
    /// the Job. This path assigns the exact root handle before any process
    /// snapshot and uses snapshot-time creation cutoffs for retroactive members.
    pub(crate) fn capture(child: &Child) -> Result<Self> {
        #[cfg(windows)]
        {
            let pid = child.id().context("spawned child PID unavailable")?;
            let job = windows_tree::OwnedTreeJob::capture_running_child(child)
                .with_context(|| format!("failed to retain Windows process tree pid={pid}"))?;
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
                    // Retry a transient TerminateJobObject/query failure instead
                    // of returning while any Job member may still be running.
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
                    // captured from the exact spawned Child handle is the only
                    // authoritative retained identity after spawn.
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

/// Spawns a runtime child with durable tree ownership established before the
/// child can execute user code.
///
/// Windows uses `CREATE_SUSPENDED`, assigns the exact spawned process handle to
/// a kill-on-close Job Object, and resumes the child only after assignment.
/// Therefore the child cannot create an FFmpeg/player descendant outside the
/// Job and no PID/tree snapshot is required for the retained LIVE path.
pub(crate) async fn spawn_owned(command: &mut Command) -> Result<(Child, OwnedProcessTree)> {
    #[cfg(windows)]
    {
        windows_tree::configure_suspended(command);
        let mut child = command
            .spawn()
            .context("failed to spawn suspended owned child")?;
        let pid = child.id().context("spawned child PID unavailable")?;

        let job = match windows_tree::OwnedTreeJob::capture_suspended_child(&child) {
            Ok(job) => job,
            Err(err) => {
                // CREATE_SUSPENDED guarantees the child has not executed and
                // therefore cannot have created descendants yet. Direct-child
                // cleanup is safe on this pre-ownership failure path.
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(err)
                    .with_context(|| format!("failed to assign suspended child pid={pid} to Job"));
            }
        };

        if let Err(err) = windows_tree::resume_child_threads(&child) {
            // The root is already inside the authoritative Job. Keep ownership
            // until every member is gone instead of dropping a kill-on-close
            // handle while the caller still believes spawn succeeded.
            loop {
                match job.active_process_count() {
                    Ok(0) => break,
                    Ok(_) | Err(_) => {
                        let _ = job.terminate();
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
            }
            let _ = child.wait().await;
            return Err(err).with_context(|| format!("failed to resume owned child pid={pid}"));
        }

        Ok((child, OwnedProcessTree { job }))
    }

    #[cfg(not(windows))]
    {
        let child = command.spawn().context("failed to spawn owned child")?;
        Ok((child, OwnedProcessTree {}))
    }
}

/// Terminates only the process tree rooted at a child spawned by this server.
///
/// This compatibility boundary is used by callers that do not retain a tree
/// owner from spawn. New retained Windows lifetimes should use `spawn_owned`.
pub(crate) async fn terminate_owned(child: &mut Child) {
    loop {
        if terminate_owned_checked(child).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

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

        match windows_tree::OwnedTreeJob::capture_running_child(child) {
            Ok(job) => {
                // The exact root handle was assigned before any descendant
                // snapshot. Snapshot absorption is creation-time bounded, so a
                // PID reused after the ToolHelp snapshot cannot be adopted.
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
    use anyhow::{Context, Result, anyhow};
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        ffi::c_void,
        mem::size_of,
        ptr,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::process::{Child, Command};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_SUSPENDED, GetProcessTimes, OpenProcess, OpenThread,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
                ResumeThread, THREAD_SUSPEND_RESUME,
            },
        },
    };

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    const WINDOWS_UNIX_EPOCH_DELTA_SECONDS: u64 = 11_644_473_600;
    const FILETIME_TICKS_PER_SECOND: u64 = 10_000_000;

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

    struct ProcessSnapshot {
        cutoff_creation_time: u64,
        children: HashMap<u32, Vec<u32>>,
        present: HashSet<u32>,
    }

    pub(super) fn configure_suspended(command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    fn filetime_value(value: FILETIME) -> u64 {
        ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
    }

    fn current_filetime_ticks() -> Result<u64> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?;
        let seconds = elapsed
            .as_secs()
            .checked_add(WINDOWS_UNIX_EPOCH_DELTA_SECONDS)
            .context("Windows FILETIME seconds overflow")?;
        let whole = seconds
            .checked_mul(FILETIME_TICKS_PER_SECOND)
            .context("Windows FILETIME tick overflow")?;
        Ok(whole + u64::from(elapsed.subsec_nanos()) / 100)
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

    fn child_raw_handle(child: &Child) -> Result<HANDLE> {
        child
            .raw_handle()
            .context("spawned child process handle unavailable")
    }

    fn child_identity(child: &Child) -> Result<ProcessIdentity> {
        let pid = child.id().context("spawned child PID unavailable")?;
        let handle = child_raw_handle(child)?;
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

    fn identity_belongs_to_snapshot(identity: ProcessIdentity, snapshot: &ProcessSnapshot) -> bool {
        identity.creation_time <= snapshot.cutoff_creation_time
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

        /// Exact retained ownership for a process created with CREATE_SUSPENDED.
        /// No process snapshot happens before assignment, so the root cannot
        /// create a descendant that escapes this Job.
        pub(super) fn capture_suspended_child(child: &Child) -> Result<Self> {
            let job = Self::create()?;
            let root_handle = child_raw_handle(child)?;
            job.assign_handle(root_handle)
                .context("failed to assign exact suspended root handle to Job")?;
            Ok(job)
        }

        /// Compatibility capture for an already-running child.
        ///
        /// The exact root handle is assigned first. Any descendants that existed
        /// before that assignment are absorbed from ToolHelp snapshots only when
        /// their creation time proves they existed at the corresponding snapshot,
        /// preventing a post-snapshot PID reuse from being adopted into the Job.
        pub(super) fn capture_running_child(child: &Child) -> Result<Self> {
            let root = child_identity(child)?;
            let job = Self::create()?;
            let root_handle = child_raw_handle(child)?;
            job.assign_handle(root_handle).with_context(|| {
                format!(
                    "failed to assign exact running root pid={} to Job before snapshot",
                    root.pid
                )
            })?;
            job.absorb_preexisting_descendants(root)?;
            Ok(job)
        }

        fn absorb_preexisting_descendants(&self, root: ProcessIdentity) -> Result<()> {
            let mut assigned = HashSet::from([root]);

            loop {
                let snapshot = process_snapshot()?;
                match query_identity(root.pid)? {
                    Some(current) if current != root => {
                        // The historical root PID now belongs to another process.
                        // Never traverse a snapshot through that replacement.
                        return Ok(());
                    }
                    _ => {}
                }

                let mut added = 0usize;
                let mut visited = HashSet::new();
                let mut queue = VecDeque::from([root.pid]);

                while let Some(parent_pid) = queue.pop_front() {
                    if !visited.insert(parent_pid) {
                        continue;
                    }
                    let Some(children) = snapshot.children.get(&parent_pid) else {
                        continue;
                    };
                    for child_pid in children.iter().copied() {
                        if child_pid == parent_pid {
                            continue;
                        }
                        let Some((process, identity)) = open_identity(
                            child_pid,
                            PROCESS_SET_QUOTA | PROCESS_TERMINATE,
                        )?
                        else {
                            continue;
                        };
                        if !identity_belongs_to_snapshot(identity, &snapshot) {
                            // The PID currently names a process created after the
                            // snapshot cutoff. It cannot be the snapshotted child.
                            continue;
                        }
                        if assigned.insert(identity) {
                            self.assign_handle(process.raw()).with_context(|| {
                                format!(
                                    "failed to assign snapshot-bound pid={} creation_time={} to Job",
                                    identity.pid, identity.creation_time
                                )
                            })?;
                            added += 1;
                        }
                        queue.push_back(child_pid);
                    }
                }

                if added == 0 {
                    return Ok(());
                }
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

    pub(super) fn resume_child_threads(child: &Child) -> Result<()> {
        let pid = child.id().context("suspended child PID unavailable")?;
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .context("CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD) failed");
        }
        let snapshot = OwnedHandle(snapshot as isize);
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut resumed = 0usize;
        let mut ok = unsafe { Thread32First(snapshot.raw(), &mut entry) };
        while ok != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(std::io::Error::last_os_error()).with_context(|| {
                        format!("OpenThread failed for suspended child thread={}", entry.th32ThreadID)
                    });
                }
                let thread = OwnedHandle(thread as isize);
                let previous = unsafe { ResumeThread(thread.raw()) };
                if previous == u32::MAX {
                    return Err(std::io::Error::last_os_error()).with_context(|| {
                        format!("ResumeThread failed for child thread={}", entry.th32ThreadID)
                    });
                }
                resumed += 1;
            }
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            ok = unsafe { Thread32Next(snapshot.raw(), &mut entry) };
        }
        if resumed == 0 {
            return Err(anyhow!("no suspended child thread found for pid={pid}"));
        }
        Ok(())
    }

    fn process_snapshot() -> Result<ProcessSnapshot> {
        // Take the cutoff before asking Windows for the snapshot. A process
        // created after this instant is rejected even if its reused PID equals
        // one present in the snapshot relationship.
        let cutoff_creation_time = current_filetime_ticks()?;
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .context("CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS) failed");
        }
        let snapshot_handle = OwnedHandle(snapshot as isize);

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut present = HashSet::new();

        let mut ok = unsafe { Process32FirstW(snapshot_handle.raw(), &mut entry) };
        while ok != 0 {
            present.insert(entry.th32ProcessID);
            children
                .entry(entry.th32ParentProcessID)
                .or_default()
                .push(entry.th32ProcessID);
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            ok = unsafe { Process32NextW(snapshot_handle.raw(), &mut entry) };
        }
        Ok(ProcessSnapshot {
            cutoff_creation_time,
            children,
            present,
        })
    }

    pub(super) fn process_tree_pids(root_pid: u32) -> Result<HashSet<u32>> {
        let snapshot = process_snapshot()?;
        let mut owned = HashSet::new();
        let mut queue = VecDeque::new();
        if snapshot.present.contains(&root_pid) || snapshot.children.contains_key(&root_pid) {
            queue.push_back(root_pid);
        }
        while let Some(pid) = queue.pop_front() {
            if snapshot.present.contains(&pid) {
                owned.insert(pid);
            }
            if let Some(descendants) = snapshot.children.get(&pid) {
                for child in descendants {
                    if *child != pid && !owned.contains(child) {
                        queue.push_back(*child);
                    }
                }
            }
        }
        if owned.is_empty() && !snapshot.children.contains_key(&root_pid) {
            return Ok(HashSet::new());
        }
        Ok(owned)
    }

    pub(super) fn child_pid_is_current(child: &Child) -> Result<bool> {
        let expected = child_identity(child)?;
        Ok(query_identity(expected.pid)? == Some(expected))
    }

    #[cfg(test)]
    pub(super) fn snapshot_cutoff_rejects_newer_identity() -> Result<bool> {
        let snapshot = ProcessSnapshot {
            cutoff_creation_time: 100,
            children: HashMap::new(),
            present: HashSet::new(),
        };
        Ok(identity_belongs_to_snapshot(
            ProcessIdentity {
                pid: 1,
                creation_time: 100,
            },
            &snapshot,
        ) && !identity_belongs_to_snapshot(
            ProcessIdentity {
                pid: 1,
                creation_time: 101,
            },
            &snapshot,
        ))
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
    use super::{spawn_owned, terminate_owned_checked};
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
    async fn windows_owned_spawn_retains_immediate_descendant() {
        use super::windows_tree::{process_tree_pids, snapshot_cutoff_rejects_newer_identity};
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
            "[IO.File]::WriteAllText($env:SOOP_JOB_TEST_PID_FILE, [string]$p.Id)"
        );
        let mut command = Command::new("powershell.exe");
        command
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
            .kill_on_drop(true);

        let (mut root, mut owner) = spawn_owned(&mut command).await.unwrap();
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

        root.wait().await.unwrap();
        assert!(
            owner.job.active_process_count().unwrap() > 0,
            "immediate descendant must already belong to retained Job after root exit"
        );
        assert!(
            process_tree_pids(root_pid)
                .unwrap()
                .contains(&descendant_pid),
            "descendant should still be alive after the short-lived root exits"
        );
        assert!(
            snapshot_cutoff_rejects_newer_identity().unwrap(),
            "snapshot identity cutoff must reject a process created after the snapshot boundary"
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
