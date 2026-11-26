use super::*;
use tempfile::tempdir;

fn completed_vod_status(message: &str) -> VodJobStatus {
    VodJobStatus {
        platform: PlatformId::Soop,
        state: "COMPLETED".into(),
        running: false,
        job_id: Some("vod-history-test".into()),
        message: message.into(),
        current_part: 1,
        part_count: 1,
        percent: 100.0,
        output_file: Some("C:\\SOOP_VOD\\done.mp4".into()),
        started_at: Some("2026-09-10T00:00:00Z".into()),
        finished_at: Some("2026-09-10T00:01:00Z".into()),
        analysis: None,
    }
}

fn channel(name: &str, account: &str) -> Channel {
    Channel {
        platform: PlatformId::Soop,
        enabled: true,
        name: name.into(),
        account: account.into(),
        outdir: String::new(),
    }
}

#[test]
fn kick_channels_and_live_history_survive_backup_restore_without_schema_changes() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
    let kick = Channel {
        platform: PlatformId::Kick,
        enabled: true,
        name: "KICK 방송자".into(),
        account: "fixture".into(),
        outdir: "KICK 저장".into(),
    };
    let soop = Channel {
        platform: PlatformId::Soop,
        ..kick.clone()
    };
    store.sync_channels(&[kick.clone(), soop]).unwrap();
    store
        .start_live(&LiveHistoryItem {
            platform: PlatformId::Kick,
            id: "kick-live".into(),
            account: "fixture".into(),
            channel_name: kick.name.clone(),
            bno: Some("123".into()),
            title: Some("방송 제목".into()),
            started_at: "2026-10-04T00:00:00Z".into(),
            ..Default::default()
        })
        .unwrap();
    store
        .finish_live(
            "kick-live",
            "2026-10-04T01:00:00Z",
            3600,
            1024,
            "BROADCAST ENDED",
            "COMPLETED",
        )
        .unwrap();
    let backup = dir.path().join("backup.db");
    store.backup_to(&backup).unwrap();
    store.sync_channels(&[]).unwrap();
    store.restore_from(&backup).unwrap();
    assert!(store.channels().unwrap().contains(&kick));
    assert_eq!(store.channels().unwrap().len(), 2);
    let history = crate::history_service::load_history(store.path(), &Default::default()).unwrap();
    assert_eq!(history.live[0].platform, PlatformId::Kick);
    assert_eq!(history.live[0].title.as_deref(), Some("방송 제목"));
}

#[test]
fn provider_replacement_journals_all_superseded_references_atomically() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
    let old = "native-secret:v1:123e4567-e89b-12d3-a456-426614174000";
    let new = "native-secret:v1:123e4567-e89b-12d3-a456-426614174001";
    let keys = ["SOOP_PASSWORD", "CHZZK_NID_AUT", "CHZZK_NID_SES"];
    store
        .sync_settings(
            &keys
                .iter()
                .map(|key| (key.to_string(), old.into()))
                .collect(),
            "test",
        )
        .unwrap();
    let journals: Vec<_> = keys
        .iter()
        .map(|key| {
            (
                key.to_string(),
                format!("STREAM_ARCHIVE_{key}_CLEANUP_REFS"),
            )
        })
        .collect();
    for (_, journal) in &journals {
        store.retain_secret_cleanup(journal, new).unwrap();
    }
    let updates = keys
        .iter()
        .map(|key| (key.to_string(), new.into()))
        .collect();
    assert!(
        store
            .sync_settings_retiring_secrets(&updates, "test", &journals, |key, value| {
                if key == "CHZZK_NID_SES" {
                    anyhow::bail!("fixture failure");
                }
                crate::security::native_cleanup_reference(value, key)
            })
            .is_err()
    );
    for key in keys {
        assert_eq!(store.setting_value(key).unwrap().as_deref(), Some(old));
    }
    store
        .sync_settings_retiring_secrets(&updates, "test", &journals, |key, value| {
            crate::security::native_cleanup_reference(value, key)
        })
        .unwrap();
    for (key, journal) in journals {
        assert_eq!(store.setting_value(&key).unwrap().as_deref(), Some(new));
        assert_eq!(store.pending_secret_cleanup(&journal).unwrap(), vec![old]);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restore_preserves_provider_credentials_and_cleanup_including_clear_and_absence() {
    let dir = tempdir().unwrap();
    let old = "native-secret:v1:123e4567-e89b-12d3-a456-426614174000";
    let current = "native-secret:v1:123e4567-e89b-12d3-a456-426614174001";
    for key in [
        "SOOP_PASSWORD",
        "CLOUDFLARE_API_KEY",
        "CHZZK_NID_AUT",
        "CHZZK_NID_SES",
    ] {
        for value in [Some(current), Some(""), None] {
            let store = Store::open(
                dir.path()
                    .join(format!("{key}-{}.db", value.unwrap_or("absent").len())),
            )
            .unwrap();
            let journal = format!("STREAM_ARCHIVE_{key}_CLEANUP_REFS");
            store
                .sync_settings(
                    &BTreeMap::from([
                        (key.into(), old.into()),
                        (
                            journal.clone(),
                            serde_json::to_string(&vec![current]).unwrap(),
                        ),
                        ("TEST_RESTORE".into(), "backup".into()),
                    ]),
                    "test",
                )
                .unwrap();
            let backup = dir.path().join(format!(
                "{key}-{}-backup.db",
                value.unwrap_or("absent").len()
            ));
            store.backup_to(&backup).unwrap();
            let original = fs::read(&backup).unwrap();
            if let Some(value) = value {
                store
                    .sync_settings(
                        &BTreeMap::from([
                            (key.into(), value.into()),
                            (journal.clone(), serde_json::to_string(&vec![old]).unwrap()),
                        ]),
                        "test",
                    )
                    .unwrap();
            } else {
                store
                    .conn()
                    .unwrap()
                    .execute(
                        "DELETE FROM settings WHERE key=?1 OR key=?2",
                        params![key, journal],
                    )
                    .unwrap();
            }
            store.restore_from(&backup).unwrap();
            assert_eq!(store.setting_value(key).unwrap().as_deref(), value);
            assert_eq!(
                store.setting_value(&journal).unwrap(),
                value.map(|_| serde_json::to_string(&vec![old]).unwrap())
            );
            assert_eq!(
                store.setting_value("TEST_RESTORE").unwrap().as_deref(),
                Some("backup")
            );
            assert_eq!(fs::read(&backup).unwrap(), original);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restore_preserves_current_kick_rows_including_clear_and_absence() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    let store = Store::open(path.clone()).unwrap();
    let observer = Store::open_observer(path).unwrap();
    let cleanup_key = "STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS";
    let old = "native-secret:v1:123e4567-e89b-12d3-a456-426614174000";
    let new = "native-secret:v1:123e4567-e89b-12d3-a456-426614174001";
    let pending = format!("[\"{old}\"]");
    for (index, current) in [Some(new), Some(""), None].into_iter().enumerate() {
        store
            .sync_settings(
                &BTreeMap::from([
                    ("KICK_SESSION_TOKEN".into(), old.into()),
                    (cleanup_key.into(), format!("[\"{new}\"]")),
                    ("SOOP_PASSWORD".into(), "backup-soop-secret".into()),
                    ("TEST_RESTORE".into(), "backup-data".into()),
                ]),
                "backup-test",
            )
            .unwrap();
        let backup = dir.path().join(format!("old-backup-{index}.db"));
        store.backup_to(&backup).unwrap();
        let original = fs::read(&backup).unwrap();
        observer
            .sync_settings(
                &BTreeMap::from([
                    ("KICK_SESSION_TOKEN".into(), current.unwrap_or("").into()),
                    (cleanup_key.into(), pending.clone()),
                    ("SOOP_PASSWORD".into(), "current-soop-secret".into()),
                    ("TEST_RESTORE".into(), "current-data".into()),
                ]),
                "current-test",
            )
            .unwrap();
        if current.is_none() {
            observer
                .conn()
                .unwrap()
                .execute(
                    "DELETE FROM settings WHERE key IN ('KICK_SESSION_TOKEN','STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS')",
                    [],
                )
                .unwrap();
        }
        // The owning store still has the old cached value; use canonical rows.
        assert_eq!(
            store
                .setting_value("KICK_SESSION_TOKEN")
                .unwrap()
                .as_deref(),
            Some(old)
        );
        store.restore_from(&backup).unwrap();
        assert_eq!(
            store
                .setting_value("KICK_SESSION_TOKEN")
                .unwrap()
                .as_deref(),
            current
        );
        assert_eq!(
            store.setting_value(cleanup_key).unwrap().as_deref(),
            current.map(|_| pending.as_str())
        );
        assert_eq!(
            store.setting_value("TEST_RESTORE").unwrap().as_deref(),
            Some("backup-data")
        );
        assert_eq!(
            store.setting_value("SOOP_PASSWORD").unwrap().as_deref(),
            Some("current-soop-secret")
        );
        assert_eq!(fs::read(&backup).unwrap(), original);
        assert!(crate::security::kick_secret_guard(&observer).is_ok());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restore_rejects_credential_contention_and_staging_failure_without_changing_live_db() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
    let backup = dir.path().join("backup.db");
    store.backup_to(&backup).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("TEST_RESTORE".into(), "current-data".into())]),
            "test",
        )
        .unwrap();
    let guard = crate::security::kick_secret_guard(&store).unwrap();
    assert!(store.restore_from(&backup).is_err());
    drop(guard);
    Connection::open(&backup)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_preserved_kick BEFORE DELETE ON settings WHEN OLD.key='KICK_SESSION_TOKEN' BEGIN SELECT RAISE(ABORT,'staging failure'); END; INSERT INTO settings(key,value,source,updated_at) VALUES('KICK_SESSION_TOKEN','stale-native-reference','test','now');",
        )
        .unwrap();
    assert!(store.restore_from(&backup).is_err());
    assert_eq!(
        store.setting_value("TEST_RESTORE").unwrap().as_deref(),
        Some("current-data")
    );
    assert_ne!(
        store
            .setting_value("KICK_SESSION_TOKEN")
            .unwrap()
            .as_deref(),
        Some("stale-native-reference")
    );
    let persisted: String = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT value FROM settings WHERE key='TEST_RESTORE'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, "current-data");
    assert!(crate::security::kick_secret_guard(&store).is_ok());
}

#[test]
fn fresh_store_is_first_run_until_user_configuration_is_written() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
    assert!(store.is_first_run_unconfigured().unwrap());

    store
        .sync_settings(
            &BTreeMap::from([("CHECK_INTERVAL".into(), "30".into())]),
            "native-environment",
        )
        .unwrap();
    assert!(!store.is_first_run_unconfigured().unwrap());
}

#[test]
fn channel_cache_preserves_database_sort_order_after_write() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join(DATABASE_FILE);
    let store = Store::open(db_path.clone()).unwrap();
    store
        .sync_channels(&[
            channel("Zulu", "z"),
            channel("alpha", "b"),
            channel("ALPHA", "a"),
        ])
        .unwrap();

    let immediate = store
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.account)
        .collect::<Vec<_>>();
    assert_eq!(immediate, vec!["a", "b", "z"]);

    let reopened = Store::open(db_path).unwrap();
    let after_restart = reopened
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.account)
        .collect::<Vec<_>>();
    assert_eq!(after_restart, immediate);
}

#[test]
fn legacy_database_filename_is_migrated_once() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join(LEGACY_DATABASE_FILE);
    let legacy_store = Store::open(legacy.clone()).unwrap();
    legacy_store
        .sync_settings(
            &BTreeMap::from([("OUTPUT_DIR".into(), "C:\\legacy-live".into())]),
            "test",
        )
        .unwrap();
    drop(legacy_store);

    let current = dir.path().join(DATABASE_FILE);
    assert!(Store::migrate_legacy_database(&current).unwrap());
    let migrated = Store::open(current.clone()).unwrap();
    assert_eq!(
        migrated.setting_value("OUTPUT_DIR").unwrap().as_deref(),
        Some("C:\\legacy-live")
    );
    assert!(current.is_file());
    assert!(!legacy.is_file());
    assert!(!Store::migrate_legacy_database(&current).unwrap());
}

#[test]
fn read_preflight_settings_uses_canonical_store_filtering() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join(DATABASE_FILE);
    let store = Store::open(db_path.clone()).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([
                ("SOOP_USERNAME".into(), "tester".into()),
                ("CHZZK_NID_AUT".into(), "native-secret:v1:configured".into()),
                ("YT_DLP_PATH".into(), "/opt/tools/yt-dlp".into()),
            ]),
            "test",
        )
        .unwrap();

    let input = Store::read_preflight_settings(&db_path).unwrap();
    assert_eq!(
        input.values.get("SOOP_USERNAME").map(String::as_str),
        Some("tester")
    );
    assert_eq!(
        input.values.get("YT_DLP_PATH").map(String::as_str),
        Some("/opt/tools/yt-dlp")
    );
    assert!(!input.values.contains_key("CHZZK_NID_AUT"));
    assert_eq!(input.configured_secrets.get("CHZZK_NID_AUT"), Some(&true));
}

#[test]
fn legacy_schema_is_promoted_to_soop_platform_identity() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE channels (
                account TEXT PRIMARY KEY COLLATE NOCASE,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                outdir TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO channels(account,name,enabled,outdir,updated_at)
            VALUES('legacy','Legacy',1,'','2026-09-10T00:00:00Z');"#,
        )
        .unwrap();
    }
    let store = Store::open(db_path).unwrap();
    let channels = store.channels().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].platform, PlatformId::Soop);
    assert_eq!(channels[0].account, "legacy");

    let conn = store.conn().unwrap();
    let columns = table_columns(&conn, "channels").unwrap();
    assert_eq!(
        columns
            .iter()
            .find(|(name, _)| name == "platform")
            .map(|(_, pk)| *pk),
        Some(1)
    );
    assert_eq!(
        columns
            .iter()
            .find(|(name, _)| name == "account")
            .map(|(_, pk)| *pk),
        Some(2)
    );
}

#[test]
fn unchanged_vod_history_upsert_does_not_touch_updated_at() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join(DATABASE_FILE)).unwrap();
    let status = completed_vod_status("완료");
    store.upsert_vod(&status).unwrap();
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "UPDATE vod_jobs SET updated_at='sentinel' WHERE id='vod-history-test'",
            [],
        )
        .unwrap();
    }

    store.upsert_vod(&status).unwrap();
    let unchanged: String = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT updated_at FROM vod_jobs WHERE id='vod-history-test'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unchanged, "sentinel");

    let mut changed = status.clone();
    changed.message = "완료됨".into();
    store.upsert_vod(&changed).unwrap();
    let changed_at: String = store
        .conn()
        .unwrap()
        .query_row(
            "SELECT updated_at FROM vod_jobs WHERE id='vod-history-test'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(changed_at, "sentinel");
}

#[test]
fn observer_open_preserves_active_runtime_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("observer.db");
    let store = Store::open(path.clone()).unwrap();
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO live_recordings(id,platform,account,channel_name,started_at,status) VALUES('live-active','SOOP','fixture','Fixture','2026-09-23T00:00:00Z','RECORDING')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vod_jobs(id,platform,kind,state,updated_at) VALUES('vod-active','SOOP','DOWNLOAD','DOWNLOADING','2026-09-23T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vod_queue(id,platform,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at) VALUES('queue-active','SOOP','{}','https://fixture.invalid/vod','/tmp','RUNNING',0,'','2026-09-23T00:00:00Z','2026-09-23T00:00:00Z')",
            [],
        )
        .unwrap();
    }
    drop(store);

    let observer = Store::open_observer(path).unwrap();
    let conn = observer.conn().unwrap();
    let live_status: String = conn
        .query_row(
            "SELECT status FROM live_recordings WHERE id='live-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let vod_state: String = conn
        .query_row(
            "SELECT state FROM vod_jobs WHERE id='vod-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let queue_state: String = conn
        .query_row(
            "SELECT state FROM vod_queue WHERE id='queue-active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live_status, "RECORDING");
    assert_eq!(vod_state, "DOWNLOADING");
    assert_eq!(queue_state, "RUNNING");
}

#[test]
fn kick_token_is_hidden_from_safe_settings_and_preflight_values() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([(
                "KICK_SESSION_TOKEN".into(),
                "native-secret:v1:configured".into(),
            )]),
            "test",
        )
        .unwrap();
    assert!(
        !store
            .safe_settings()
            .unwrap()
            .contains_key("KICK_SESSION_TOKEN")
    );
    assert_eq!(
        store
            .configured_secrets()
            .unwrap()
            .get("KICK_SESSION_TOKEN"),
        Some(&true)
    );
}

#[test]
fn kick_native_creation_requires_durable_intent_and_survives_readonly_commit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("readonly.db");
    let store = Store::open(path.clone()).unwrap();
    let cleanup_key = "STREAM_ARCHIVE_KICK_SECRET_CLEANUP_REFS";
    let protected = crate::security::protect_native_secret_with(
        "fixture-token",
        |reference| store.retain_secret_cleanup(cleanup_key, reference),
        |reference, _| {
            assert_eq!(
                store.pending_secret_cleanup(cleanup_key)?,
                vec![format!("native-secret:v1:{reference}")]
            );
            store.conn()?.execute_batch("PRAGMA query_only=ON")?;
            Ok(())
        },
    )
    .unwrap();
    let error = store
        .sync_settings_retiring_secret(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), protected.clone())]),
            "test",
            "KICK_SESSION_TOKEN",
            cleanup_key,
            |_| Ok(None),
        )
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<rusqlite::Error>()
            .unwrap()
            .sqlite_error_code(),
        Some(rusqlite::ErrorCode::ReadOnly)
    );
    assert!(
        store
            .retain_secret_cleanup(cleanup_key, &protected)
            .is_err()
    );
    assert!(
        store
            .finish_secret_cleanup(cleanup_key, &protected)
            .is_err()
    );
    drop(store);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        reopened.pending_secret_cleanup(cleanup_key).unwrap(),
        vec![protected.clone()]
    );
    reopened
        .finish_secret_cleanup(cleanup_key, &protected)
        .unwrap();
    assert!(
        reopened
            .pending_secret_cleanup(cleanup_key)
            .unwrap()
            .is_empty()
    );

    reopened
        .conn()
        .unwrap()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    let result = crate::security::protect_native_secret_with(
        "fixture-token",
        |reference| reopened.retain_secret_cleanup(cleanup_key, reference),
        |_, _| panic!("native credential must not be created before durable intent"),
    );
    assert!(result.is_err());
}
