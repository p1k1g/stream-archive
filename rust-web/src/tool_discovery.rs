use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Streamlink,
    YtDlp,
    Ffmpeg,
}

impl ToolKind {
    pub const ALL: [Self; 3] = [Self::Streamlink, Self::YtDlp, Self::Ffmpeg];

    pub fn label(self) -> &'static str {
        match self {
            Self::Streamlink => "streamlink",
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "ffmpeg",
        }
    }

    pub fn primary_setting_key(self) -> &'static str {
        match self {
            Self::Streamlink => "STREAMLINK_PATH",
            Self::YtDlp => "YT_DLP_PATH",
            Self::Ffmpeg => "FFMPEG_PATH",
        }
    }

    pub fn setting_keys(self) -> &'static [&'static str] {
        match self {
            Self::Streamlink => &["STREAMLINK_PATH", "STREAMLINK_FALLBACK"],
            Self::YtDlp => &["YT_DLP_PATH"],
            Self::Ffmpeg => &["FFMPEG_PATH"],
        }
    }

    pub fn binary_names(self) -> &'static [&'static str] {
        #[cfg(windows)]
        {
            match self {
                Self::Streamlink => &["streamlink.exe", "streamlink"],
                Self::YtDlp => &["yt-dlp.exe", "yt-dlp"],
                Self::Ffmpeg => &["ffmpeg.exe", "ffmpeg"],
            }
        }
        #[cfg(not(windows))]
        {
            match self {
                Self::Streamlink => &["streamlink", "streamlink.exe"],
                Self::YtDlp => &["yt-dlp", "yt-dlp.exe"],
                Self::Ffmpeg => &["ffmpeg", "ffmpeg.exe"],
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolResolution {
    pub kind: ToolKind,
    pub path: Option<PathBuf>,
    pub source: String,
    pub warnings: Vec<String>,
}

impl ToolResolution {
    pub fn found(&self) -> bool {
        self.path.is_some()
    }
}

pub fn resolve_tool(
    kind: ToolKind,
    backend_dir: &Path,
    configured: &[(&str, &str)],
) -> ToolResolution {
    let mut warnings = Vec::new();

    for (key, raw) in configured {
        let value = raw.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("AUTO") {
            continue;
        }
        let path = PathBuf::from(value);
        if executable_file(&path) {
            return ToolResolution {
                kind,
                path: Some(path),
                source: format!("config:{key}"),
                warnings,
            };
        }
        warnings.push(format!("{key} points to a missing/non-executable file: {value}"));
    }

    let bundled_root = match kind {
        ToolKind::Streamlink => backend_dir.to_path_buf(),
        ToolKind::YtDlp | ToolKind::Ffmpeg => backend_dir.join("vod"),
    };
    for name in kind.binary_names() {
        let path = bundled_root.join(name);
        if executable_file(&path) {
            return ToolResolution {
                kind,
                path: Some(path),
                source: "bundled".into(),
                warnings,
            };
        }
    }

    for name in kind.binary_names() {
        if let Some(path) = find_command(name) {
            return ToolResolution {
                kind,
                path: Some(path),
                source: "PATH".into(),
                warnings,
            };
        }
    }

    for dir in common_tool_directories() {
        for name in kind.binary_names() {
            let path = dir.join(name);
            if executable_file(&path) {
                return ToolResolution {
                    kind,
                    path: Some(path),
                    source: "common-path".into(),
                    warnings,
                };
            }
        }
    }

    ToolResolution {
        kind,
        path: None,
        source: "missing".into(),
        warnings,
    }
}

pub fn find_command(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 && executable_file(candidate) {
        return Some(candidate.to_path_buf());
    }
    env::var_os("PATH").and_then(|value| {
        env::split_paths(&value)
            .map(|dir| dir.join(name))
            .find(|path| executable_file(path))
    })
}

pub fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn common_tool_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(windows)]
    {
        if let Some(program_files) = env::var_os("ProgramFiles") {
            let root = PathBuf::from(program_files);
            dirs.push(root.join("Streamlink").join("bin"));
            dirs.push(root.join("Streamlink").join("ffmpeg"));
        }
    }

    #[cfg(unix)]
    {
        dirs.extend([
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/local/bin"),
            PathBuf::from("/snap/bin"),
        ]);
        if let Some(home) = env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local").join("bin"));
        }
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_executable(path: &Path) {
        fs::write(path, b"test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[test]
    fn explicit_configured_path_wins() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join(ToolKind::Ffmpeg.binary_names()[0]);
        make_executable(&binary);
        let text = binary.to_string_lossy().to_string();
        let resolved = resolve_tool(
            ToolKind::Ffmpeg,
            temp.path(),
            &[("FFMPEG_PATH", text.as_str())],
        );
        assert_eq!(resolved.path.as_deref(), Some(binary.as_path()));
        assert_eq!(resolved.source, "config:FFMPEG_PATH");
    }

    #[test]
    fn bundled_layout_is_discovered() {
        let temp = tempfile::tempdir().unwrap();
        let vod = temp.path().join("vod");
        fs::create_dir_all(&vod).unwrap();
        let binary = vod.join(ToolKind::YtDlp.binary_names()[0]);
        make_executable(&binary);
        let resolved = resolve_tool(ToolKind::YtDlp, temp.path(), &[]);
        assert_eq!(resolved.path.as_deref(), Some(binary.as_path()));
        assert_eq!(resolved.source, "bundled");
    }

    #[test]
    fn invalid_explicit_path_is_reported_without_becoming_a_result() {
        let temp = tempfile::tempdir().unwrap();
        let resolved = resolve_tool(
            ToolKind::Streamlink,
            temp.path(),
            &[("STREAMLINK_PATH", "/definitely/not/a/tool")],
        );
        assert!(!resolved.warnings.is_empty());
        assert_ne!(
            resolved.path.as_deref(),
            Some(Path::new("/definitely/not/a/tool"))
        );
    }
}
