use anyhow::{Context, Result};
use fs2::FileExt;
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub const RUNTIME_LOCK_FILE_NAME: &str = "stream-archive.runtime.lock";
#[cfg(unix)]
const RUNTIME_CONTROL_SOCKET_PREFIX: &str = "stream-archive-";

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
                format!(
                    "failed to create runtime lock directory {}",
                    parent.display()
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open runtime owner lock {}", path.display()))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file, path })),
            Err(error) if lock_is_contended(&error) => Ok(None),
            Err(error) => Err(error).with_context(|| {
                format!("failed to acquire runtime owner lock {}", path.display())
            }),
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

fn lock_is_contended(error: &std::io::Error) -> bool {
    if error.kind() == ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return true;
    }
    false
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
    let mut hasher = Sha256::new();
    hasher.update(database_path.as_os_str().as_encoded_bytes());
    let digest = hasher.finalize();
    let suffix = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(std::env::temp_dir().join(format!(
        "{RUNTIME_CONTROL_SOCKET_PREFIX}{suffix}.sock"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn runtime_control_socket_path_is_short_and_stable_for_long_unicode_data_paths() {
        let long = std::env::temp_dir()
            .join("Stream Archive 매우 긴 데이터 경로 🎬")
            .join("nested directory with spaces")
            .join("stream-archive.db");
        let first = runtime_control_socket_path(&long).unwrap();
        let second = runtime_control_socket_path(&long).unwrap();
        assert_eq!(first, second);
        assert!(first.file_name().unwrap().to_string_lossy().starts_with(RUNTIME_CONTROL_SOCKET_PREFIX));
        assert!(
            first.as_os_str().as_encoded_bytes().len() < 100,
            "Unix socket path must stay below conservative sockaddr_un limits: {}",
            first.display()
        );
    }

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
