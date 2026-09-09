from pathlib import Path

p=Path('rust-web/src/backup.rs')
s=p.read_text(encoding='utf-8')

def rep(old,new,label):
    global s
    if old not in s:
        raise SystemExit(f'patch target not found: {label}')
    s=s.replace(old,new,1)

rep('''struct BackupMetadata {
    version: u32,
    created_at: String,
    kind: String,
    source: String,
    sha256: String,
    size_bytes: u64,
}''','''struct BackupMetadata {
    #[serde(default)]
    version: u32,
    created_at: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    source: String,
    sha256: String,
    size_bytes: u64,
}''','legacy metadata defaults')

rep('''        let safety = self.create_locked("pre_restore")?;
        self.store.restore_from(&source)?;
        Ok((selected, safety))
    }

    fn create_locked(&self, kind: &str) -> Result<BackupInfo> {
        fs::create_dir_all(self.backup_dir.as_path())?;''','''        // Do not run retention while creating the safety copy. With a very small
        // keep-count (for example 1), cleanup here could delete `source` before
        // SQLite has restored it.
        let safety = self.create_locked_with_cleanup("pre_restore", false)?;
        self.store.restore_from(&source)?;
        let policy = self.policy()?;
        self.cleanup_locked(&policy)?;
        Ok((selected, safety))
    }

    fn create_locked(&self, kind: &str) -> Result<BackupInfo> {
        self.create_locked_with_cleanup(kind, true)
    }

    fn create_locked_with_cleanup(&self, kind: &str, cleanup: bool) -> Result<BackupInfo> {
        fs::create_dir_all(self.backup_dir.as_path())?;''','restore cleanup ordering')

rep('''        let policy = self.policy()?;
        self.cleanup_locked(&policy)?;
        Ok(BackupInfo {''','''        if cleanup {
            let policy = self.policy()?;
            self.cleanup_locked(&policy)?;
        }
        Ok(BackupInfo {''','conditional cleanup')

rep('''    let kind = metadata
        .as_ref()
        .map(|m| m.kind.clone())
        .unwrap_or_else(|| infer_kind(&file_name));''','''    let kind = metadata
        .as_ref()
        .map(|m| {
            if m.kind.trim().is_empty() {
                infer_kind(&file_name)
            } else {
                m.kind.clone()
            }
        })
        .unwrap_or_else(|| infer_kind(&file_name));''','legacy kind inference')

# Strengthen the round-trip test against the keep-count=1 restore race.
rep('''        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let backup = runtime.block_on(manager.create_manual()).unwrap();''','''        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        store
            .sync_settings(
                &std::collections::BTreeMap::from([("BACKUP_KEEP_COUNT".into(), "1".into())]),
                "test",
            )
            .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let backup = runtime.block_on(manager.create_manual()).unwrap();''','keep count race test')

# Add legacy metadata compatibility test before module closing brace.
needle='''        assert_eq!(
            store.setting_value("TEST").unwrap().as_deref(),
            Some("before")
        );
    }
}'''
replacement='''        assert_eq!(
            store.setting_value("TEST").unwrap().as_deref(),
            Some("before")
        );
    }

    #[test]
    fn legacy_metadata_without_kind_or_version_remains_restorable() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("soop-recorder");
        let backend = app.join("backend");
        fs::create_dir_all(&backend).unwrap();
        let store = Store::open(app.join("data").join("soop.db")).unwrap();
        let manager = BackupManager::open(store.clone(), &backend).unwrap();
        let path = manager.backup_dir().join("soop_20260909_071240.db");
        store.backup_to(&path).unwrap();
        let sha = sha256_file(&path).unwrap();
        let size = fs::metadata(&path).unwrap().len();
        let legacy = json!({
            "created_at": Utc::now().to_rfc3339(),
            "source": store.path().display().to_string(),
            "backup": path.display().to_string(),
            "sha256": sha,
            "size_bytes": size
        });
        fs::write(path.with_extension("db.json"), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let info = inspect_backup(&path).unwrap();
        assert_eq!(info.integrity, "OK");
        assert_eq!(info.kind, "legacy");
    }
}'''
rep(needle,replacement,'legacy metadata test')

p.write_text(s,encoding='utf-8',newline='\n')
print('Phase 12 follow-up patch: PASS')
