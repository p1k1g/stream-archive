use anyhow::{bail, Context, Result};
use atomic_write_file::AtomicWriteFile;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const FILE_NAME: &str = "SOOP_VOD_SETTING.ini";
const KEYS: &[&str] = &["YT_DLP_PATH", "FFMPEG_PATH"];

pub fn read(backend: &Path) -> Result<BTreeMap<String, String>> {
    let path = path(backend);
    let mut result = BTreeMap::from([
        ("YT_DLP_PATH".to_string(), String::new()),
        ("FFMPEG_PATH".to_string(), String::new()),
    ]);
    if !path.is_file() {
        return Ok(result);
    }

    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue; };
        let key = key.trim();
        if KEYS.contains(&key) {
            result.insert(key.to_string(), value.trim().to_string());
        }
    }
    Ok(result)
}

pub fn update(backend: &Path, updates: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in updates {
        if !KEYS.contains(&key.as_str()) {
            bail!("unsupported VOD tool setting: {key}");
        }
        if value.contains(['\r', '\n', '\0']) || value.len() > 2048 {
            bail!("invalid VOD tool path: {key}");
        }
    }

    let path = path(backend);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let original = fs::read_to_string(&path).unwrap_or_default();
    let newline = if original.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = original.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = Vec::new();
    let mut seen = BTreeMap::new();

    for raw in normalized.lines() {
        let trimmed = raw.trim();
        if !trimmed.starts_with('#') && !trimmed.starts_with(';') {
            if let Some((key, _)) = raw.split_once('=') {
                let key = key.trim();
                if let Some(value) = updates.get(key) {
                    output.push(format!("{key}={value}"));
                    seen.insert(key.to_string(), true);
                    continue;
                }
            }
        }
        output.push(raw.to_string());
    }

    if original.is_empty() {
        output.push("# SOOP VOD tool settings".to_string());
    }
    for key in KEYS {
        if let Some(value) = updates.get(*key) {
            if !seen.contains_key(*key) {
                output.push(format!("{key}={value}"));
            }
        }
    }

    let mut content = output.join(newline);
    if !content.is_empty() {
        content.push_str(newline);
    }
    backup_existing(&path)?;
    let mut file = AtomicWriteFile::options()
        .open(&path)
        .with_context(|| format!("failed to open atomic writer for {}", path.display()))?;
    file.write_all(content.as_bytes())?;
    file.commit()?;
    Ok(())
}

pub fn apply_defaults(backend: &Path, yt_dlp: &mut String, ffmpeg: &mut String) -> Result<()> {
    let values = read(backend)?;
    if yt_dlp.trim().is_empty() {
        *yt_dlp = values.get("YT_DLP_PATH").cloned().unwrap_or_default();
    }
    if ffmpeg.trim().is_empty() {
        *ffmpeg = values.get("FFMPEG_PATH").cloned().unwrap_or_default();
    }
    Ok(())
}

fn path(backend: &Path) -> PathBuf {
    backend.join("vod").join(FILE_NAME)
}

fn backup_existing(path: &Path) -> Result<()> {
    if path.is_file() {
        let mut backup = path.as_os_str().to_os_string();
        backup.push(".bak");
        fs::copy(path, PathBuf::from(backup))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_loads_tool_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("vod")).unwrap();
        let updates = BTreeMap::from([
            ("YT_DLP_PATH".to_string(), r"C:\Tools\yt-dlp.exe".to_string()),
            ("FFMPEG_PATH".to_string(), r"C:\Tools\ffmpeg.exe".to_string()),
        ]);
        update(dir.path(), &updates).unwrap();
        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded["YT_DLP_PATH"], r"C:\Tools\yt-dlp.exe");
        assert_eq!(loaded["FFMPEG_PATH"], r"C:\Tools\ffmpeg.exe");
    }
}
