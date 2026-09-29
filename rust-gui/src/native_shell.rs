use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub fn containing_directory(file_path: &str) -> Result<PathBuf> {
    let file_path = file_path.trim();
    if file_path.is_empty() {
        bail!("recording file path is empty");
    }
    let path = Path::new(file_path);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("recording file path has no parent directory")?;
    if !parent.is_dir() {
        bail!("recording directory does not exist: {}", parent.display());
    }
    Ok(parent.to_path_buf())
}

#[cfg(windows)]
pub fn open_containing_directory(file_path: &str) -> Result<()> {
    let directory = containing_directory(file_path)?;
    std::process::Command::new("explorer.exe")
        .arg(&directory)
        .spawn()
        .with_context(|| format!("failed to open {}", directory.display()))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn open_containing_directory(_file_path: &str) -> Result<()> {
    bail!("opening a LIVE recording folder is supported only by the Windows Native UI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containing_directory_preserves_spaces_and_unicode() {
        let directory =
            std::env::temp_dir().join(format!("Stream Archive 저장 폴더-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("방송 파일.ts");
        let resolved = containing_directory(file.to_string_lossy().as_ref()).unwrap();
        assert_eq!(resolved, directory);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn empty_or_missing_parent_is_rejected_without_shelling_out() {
        assert!(containing_directory("").is_err());
        let missing = std::env::temp_dir()
            .join("stream-archive-definitely-missing")
            .join("recording.ts");
        assert!(containing_directory(missing.to_string_lossy().as_ref()).is_err());
    }
}
