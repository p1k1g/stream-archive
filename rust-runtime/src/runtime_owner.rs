use anyhow::{Context, Result};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub const RUNTIME_LOCK_FILE_NAME: &str = "stream-archive.runtime.lock";
#[cfg(unix)]
pub const RUNTIME_CONTROL_SOCKET_NAME: &str = "stream-archive.runtime.sock";

pub struct RuntimeOwnerGuard {
    file: File,
    path: PathBuf,
}

impl RuntimeOwnerGuard {
    pub fn acquire(database_path: &Path) -> Result<Self> {
        Self::try_acquire(database_path)?.ok_or_else(|| {
            anyhow::anyhow!(
                "another Stream Archive runtime owns {}",
                runtime_lock_path(database_path)
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "the canonical data directory".into())
            )
        })
    }

    pub fn try_acquire(database_path: &Path) -> Result<Option<Self>> {
        let path = runtime_lock_path(database_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create runtime lock directory {}", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open runtime owner lock {}", path.display()))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file, path })),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error)
                .with_context(|| format!("failed to acquire runtime owner lock {}", path.display())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RuntimeOwnerGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub fn runtime_owner_active(database_path: &Path) -> Result<bool> {
    Ok(RuntimeOwnerGuard::try_acquire(database_path)?.is_none())
}

pub fn runtime_lock_path(database_path: &Path) -> Result<PathBuf> {
    let parent = database_path
        .parent()
        .context("database path has no parent directory")?;
    Ok(parent.join(RUNTIME_LOCK_FILE_NAME))
}

#[cfg(unix)]
pub fn runtime_control_socket_path(database_path: &Path) -> Result<PathBuf> {
    let parent = database_path
        .parent()
        .context("database path has no parent directory")?;
    Ok(parent.join(RUNTIME_CONTROL_SOCKET_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_lock_blocks_second_owner_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("stream-archive.db");
        let first = RuntimeOwnerGuard::acquire(&db).unwrap();
        assert!(runtime_owner_active(&db).unwrap());
        drop(first);
        assert!(!runtime_owner_active(&db).unwrap());
    }
}
