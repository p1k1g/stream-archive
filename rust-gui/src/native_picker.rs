//! GUI-only platform boundary. A cancelled dialog never edits or persists data.
use stream_archive_server::environment_settings::SettingKind;

#[cfg(windows)]
mod windows;

pub fn pick(kind: SettingKind, initial: &str) -> Result<Option<String>, String> {
    if kind == SettingKind::Text {
        return Err("This setting does not support a picker".into());
    }
    #[cfg(windows)]
    {
        windows::pick(kind, initial).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = initial;
        Err("Native picker is supported only by the Windows desktop application".into())
    }
}

pub fn pick_directory(initial: &str) -> Result<Option<String>, String> {
    pick(SettingKind::Directory, initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fields_never_invoke_a_dialog() {
        assert!(pick(SettingKind::Text, "").is_err());
    }
}
