//! Draft/patch handling is independent of Slint and native dialogs.
use std::collections::BTreeMap;
use stream_archive_server::environment_settings::EnvironmentSetting;

#[derive(Default)]
pub struct SettingsDraft {
    pub fields: Vec<EnvironmentSetting>,
    original: BTreeMap<String, String>,
}

impl SettingsDraft {
    pub fn load(&mut self, fields: Vec<EnvironmentSetting>) {
        self.original = fields
            .iter()
            .map(|field| (field.key.clone(), field.value.clone()))
            .collect();
        self.fields = fields;
    }

    pub fn edit(&mut self, index: usize, value: String) {
        if let Some(field) = self.fields.get_mut(index) {
            field.value = value;
        }
    }

    pub fn accept_selection(&mut self, index: usize, selection: Option<String>) -> bool {
        match (self.fields.get_mut(index), selection) {
            (Some(field), Some(path)) => {
                field.value = path;
                true
            }
            _ => false,
        }
    }

    // A remembered close choice persists one key without committing unrelated edits.
    pub fn accept_saved_value(&mut self, key: &str, value: &str) {
        if let Some(field) = self.fields.iter_mut().find(|field| field.key == key) {
            field.value = value.into();
            self.original.insert(key.into(), value.into());
        }
    }

    pub fn patch(&self) -> BTreeMap<String, String> {
        self.fields
            .iter()
            .filter(|field| self.original.get(&field.key) != Some(&field.value))
            .map(|field| (field.key.clone(), field.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::environment_settings::{SettingKind, snapshot};

    #[test]
    fn edits_only_changed_fields_and_reload_discards_draft() {
        let fields = snapshot(&BTreeMap::from([(
            "STREAMLINK_PATH".into(),
            "legacy-relative.exe".into(),
        )]));
        let mut draft = SettingsDraft::default();
        draft.load(fields.clone());
        assert!(draft.patch().is_empty());
        let index = draft
            .fields
            .iter()
            .position(|f| f.key == "CHECK_INTERVAL")
            .unwrap();
        draft.edit(index, "45".into());
        assert_eq!(
            draft.patch(),
            BTreeMap::from([("CHECK_INTERVAL".into(), "45".into())])
        );
        assert_eq!(draft.fields[0].kind, SettingKind::Executable);
        draft.load(fields);
        assert!(draft.patch().is_empty());
    }

    #[test]
    fn cancelled_or_stale_picker_does_not_change_the_draft() {
        let mut draft = SettingsDraft::default();
        draft.load(snapshot(&BTreeMap::new()));
        assert!(!draft.accept_selection(0, None));
        assert!(!draft.accept_selection(usize::MAX, Some("ignored".into())));
        assert!(draft.patch().is_empty());
        assert!(draft.accept_selection(0, Some("selected.exe".into())));
        assert_eq!(draft.patch()["STREAMLINK_PATH"], "selected.exe");
        assert!(!draft.accept_selection(0, None));
        assert_eq!(draft.patch()["STREAMLINK_PATH"], "selected.exe");
    }
    #[test]
    fn remembered_close_choice_preserves_other_unsaved_settings() {
        let mut draft = SettingsDraft::default();
        draft.load(snapshot(&BTreeMap::from([(
            "STREAM_ARCHIVE_CLOSE_ACTION".into(),
            "ASK".into(),
        )])));
        let index = draft
            .fields
            .iter()
            .position(|f| f.key == "CHECK_INTERVAL")
            .unwrap();
        draft.edit(index, "45".into());
        draft.accept_saved_value("STREAM_ARCHIVE_CLOSE_ACTION", "TRAY");
        assert_eq!(
            draft.patch(),
            BTreeMap::from([("CHECK_INTERVAL".into(), "45".into())])
        );
        assert_eq!(
            draft
                .fields
                .iter()
                .find(|f| f.key == "STREAM_ARCHIVE_CLOSE_ACTION")
                .unwrap()
                .value,
            "TRAY"
        );
    }
}
