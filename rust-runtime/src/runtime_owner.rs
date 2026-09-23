use anyhow::{Context, Result, bail};
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
        let path = runtime_lock_path(database_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create runtime lock directory {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open runtime owner lock {}", path.display()))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { file, path }),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                bail!("another Stream Archive runtime owns {}", path.display())
            }
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
    match RuntimeOwnerGuard::acquire(database_path) {
        Ok(guard) => {
            drop(guard);
            Ok(false)
        }
        Err(error) if error.to_string().starts_with("another Stream Archive runtime owns ") => {
            Ok(true)
        }
        Err(error) => Err(error),
    }
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
