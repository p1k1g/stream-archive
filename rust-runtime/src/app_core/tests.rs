use super::*;

#[tokio::test]
async fn background_start_is_shared_by_core_clones_and_shutdown_releases_captures() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("stream-archive.db")).unwrap();
    let core = StreamArchiveCore::assemble(directory.path().to_path_buf(), store).unwrap();
    let clone = core.clone();
    let vod = Arc::downgrade(&core.vod);
    for _ in 0..100 {
        core.spawn_auto_backup();
        clone.spawn_auto_backup();
        core.spawn_vod_history_sync();
        clone.spawn_vod_history_sync();
    }
    assert_eq!(core.background_tasks.task_count(), 2);
    tokio::join!(core.shutdown(), clone.shutdown());
    clone.spawn_auto_backup();
    clone.spawn_vod_history_sync();
    assert_eq!(core.background_tasks.task_count(), 0);
    drop(core);
    drop(clone);
    assert!(vod.upgrade().is_none());
}

const OLD_KICK_REFERENCE: &str = "native-secret:v1:123e4567-e89b-12d3-a456-426614174000";
const NEW_KICK_REFERENCE: &str = "native-secret:v1:123e4567-e89b-12d3-a456-426614174001";

#[test]
fn later_native_store_failure_rolls_back_all_prepared_credentials() {
    for fail_first_cleanup in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        let original = BTreeMap::from([
            ("CHZZK_NID_AUT".into(), "old-aut".into()),
            ("KICK_SESSION_TOKEN".into(), "old-kick".into()),
            ("SOOP_PASSWORD".into(), "old-soop".into()),
        ]);
        store.sync_settings(&original, "test").unwrap();
        let mut deleted = Vec::new();
        let result = save_provider_configuration(
            &store,
            &BTreeMap::new(),
            &original,
            |key, _, cleanup_key| {
                if key == "SOOP_PASSWORD" {
                    bail!("native store failed");
                }
                let reference = if key == "CHZZK_NID_AUT" {
                    OLD_KICK_REFERENCE
                } else {
                    NEW_KICK_REFERENCE
                };
                store.retain_secret_cleanup(cleanup_key, reference)?;
                Ok(reference.into())
            },
            &mut |key, reference| {
                deleted.push((key.to_string(), reference.to_string()));
                if fail_first_cleanup && key == "CHZZK_NID_AUT" {
                    bail!("native cleanup failed");
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(deleted.len(), 2);
        for (key, value) in &original {
            assert_eq!(store.setting_value(key).unwrap().as_ref(), Some(value));
        }
        assert!(
            store
                .pending_secret_cleanup(KICK_CLEANUP_KEY)
                .unwrap()
                .is_empty()
        );
        let pending = store
            .pending_secret_cleanup("STREAM_ARCHIVE_CHZZK_NID_AUT_CLEANUP_REFS")
            .unwrap();
        assert_eq!(
            pending,
            if fail_first_cleanup {
                vec![OLD_KICK_REFERENCE.to_string()]
            } else {
                Vec::new()
            }
        );
    }
}

#[test]
fn provider_clear_retries_native_cleanup_and_preserves_other_settings() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([
                ("CHZZK_NID_AUT".into(), OLD_KICK_REFERENCE.into()),
                ("CHZZK_NID_SES".into(), NEW_KICK_REFERENCE.into()),
                ("CLOUDFLARE_API_KEY".into(), "keep-worker".into()),
            ]),
            "test",
        )
        .unwrap();
    assert!(
        clear_provider_secrets_in_store(&store, &["CHZZK_NID_AUT", "CHZZK_NID_SES"], |_, _| bail!(
            "test failure"
        ))
        .is_err()
    );
    assert_eq!(store.setting_value("CHZZK_NID_AUT").unwrap().unwrap(), "");
    assert_eq!(store.setting_value("CHZZK_NID_SES").unwrap().unwrap(), "");
    assert_eq!(
        store
            .pending_secret_cleanup("STREAM_ARCHIVE_CHZZK_NID_AUT_CLEANUP_REFS")
            .unwrap(),
        vec![OLD_KICK_REFERENCE]
    );
    assert_eq!(
        store
            .pending_secret_cleanup("STREAM_ARCHIVE_CHZZK_NID_SES_CLEANUP_REFS")
            .unwrap(),
        vec![NEW_KICK_REFERENCE]
    );
    let mut deleted = Vec::new();
    clear_provider_secrets_in_store(&store, &["CHZZK_NID_AUT", "CHZZK_NID_SES"], |_, value| {
        deleted.push(value.to_string());
        Ok(())
    })
    .unwrap();
    assert_eq!(deleted, vec![OLD_KICK_REFERENCE, NEW_KICK_REFERENCE]);
    for key in ["CHZZK_NID_AUT", "CHZZK_NID_SES"] {
        assert!(
            store
                .pending_secret_cleanup(&format!("STREAM_ARCHIVE_{key}_CLEANUP_REFS"))
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(
        store.setting_value("CLOUDFLARE_API_KEY").unwrap().unwrap(),
        "keep-worker"
    );
}

#[test]
fn kick_replacement_removes_previous_native_item_only_after_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    store
        .retain_secret_cleanup(KICK_CLEANUP_KEY, NEW_KICK_REFERENCE)
        .unwrap();
    let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
    let mut deleted = Vec::new();
    commit_kick_configuration(&store, &updates, &mut |reference| {
        assert_eq!(
            store.setting_value("KICK_SESSION_TOKEN")?.unwrap(),
            NEW_KICK_REFERENCE
        );
        deleted.push(reference.to_string());
        Ok(())
    })
    .unwrap();
    assert_eq!(deleted, vec![OLD_KICK_REFERENCE]);
    assert!(
        store
            .pending_secret_cleanup(KICK_CLEANUP_KEY)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn kick_replacement_cleans_new_native_item_on_sqlite_commit_failure() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_kick_update BEFORE UPDATE ON settings WHEN NEW.key='KICK_SESSION_TOKEN' BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
    )
    .unwrap();
    let mut deleted = Vec::new();
    let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
    assert!(
        commit_kick_configuration(&store, &updates, &mut |reference| {
            deleted.push(reference.to_string());
            Ok(())
        })
        .is_err()
    );
    assert_eq!(deleted, vec![NEW_KICK_REFERENCE]);
    assert_eq!(
        store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
        OLD_KICK_REFERENCE
    );
    assert!(
        store
            .pending_secret_cleanup(KICK_CLEANUP_KEY)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn kick_secret_guard_serializes_observer_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let owner = Store::open(path.clone()).unwrap();
    let observer = Store::open_observer(path).unwrap();
    let guard = crate::security::kick_secret_guard(&owner).unwrap();
    assert!(crate::security::kick_secret_guard(&observer).is_err());
    drop(guard);
    assert!(crate::security::kick_secret_guard(&observer).is_ok());
}

#[test]
fn kick_failed_save_retains_new_reference_when_rollback_cleanup_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    store
        .retain_secret_cleanup(KICK_CLEANUP_KEY, NEW_KICK_REFERENCE)
        .unwrap();
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_kick_update BEFORE UPDATE ON settings WHEN NEW.key='KICK_SESSION_TOKEN' BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
    )
    .unwrap();
    let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
    assert!(commit_kick_configuration(&store, &updates, &mut |_| bail!("locked")).is_err());
    assert_eq!(
        store.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
        vec![NEW_KICK_REFERENCE]
    );
    cleanup_kick_secrets(&store, &mut |reference| {
        assert_eq!(reference, NEW_KICK_REFERENCE);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
        OLD_KICK_REFERENCE
    );
}

#[test]
fn kick_retired_native_cleanup_failure_survives_reopen_and_is_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = Store::open(path.clone()).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    let updates = BTreeMap::from([("KICK_SESSION_TOKEN".into(), NEW_KICK_REFERENCE.into())]);
    assert!(commit_kick_configuration(&store, &updates, &mut |_| bail!("locked")).is_err());
    drop(store);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        reopened
            .setting_value("KICK_SESSION_TOKEN")
            .unwrap()
            .unwrap(),
        NEW_KICK_REFERENCE
    );
    assert_eq!(
        reopened.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
        vec![OLD_KICK_REFERENCE]
    );
    cleanup_kick_secrets(&reopened, &mut |reference| {
        assert_eq!(reference, OLD_KICK_REFERENCE);
        Ok(())
    })
    .unwrap();
    assert!(
        reopened
            .pending_secret_cleanup(KICK_CLEANUP_KEY)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        reopened
            .setting_value("KICK_SESSION_TOKEN")
            .unwrap()
            .unwrap(),
        NEW_KICK_REFERENCE
    );
}

#[test]
fn kick_clear_commits_cleanup_before_deletion_and_retries_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let owner = Store::open(path.clone()).unwrap();
    let observer = Store::open_observer(path.clone()).unwrap();
    observer
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    assert!(
        clear_kick_token_in_store(&owner, |value| {
            assert_eq!(value, OLD_KICK_REFERENCE);
            observer.refresh_config_cache()?;
            assert_eq!(observer.setting_value("KICK_SESSION_TOKEN")?.unwrap(), "");
            bail!("credential store locked")
        })
        .is_err()
    );
    assert_eq!(
        owner.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
        ""
    );
    drop(owner);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        reopened.pending_secret_cleanup(KICK_CLEANUP_KEY).unwrap(),
        vec![OLD_KICK_REFERENCE]
    );
    clear_kick_token_in_store(&reopened, |value| {
        assert_eq!(value, OLD_KICK_REFERENCE);
        Ok(())
    })
    .unwrap();
    assert!(
        reopened
            .pending_secret_cleanup(KICK_CLEANUP_KEY)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn kick_clear_does_not_delete_native_item_when_db_transition_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    store
        .sync_settings(
            &BTreeMap::from([("KICK_SESSION_TOKEN".into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_clear BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
    )
    .unwrap();
    assert!(
        clear_kick_token_in_store(&store, |_| {
            panic!("native deletion must not happen before a committed DB transition")
        })
        .is_err()
    );
    assert_eq!(
        store.setting_value("KICK_SESSION_TOKEN").unwrap().unwrap(),
        OLD_KICK_REFERENCE
    );
    assert!(
        store
            .pending_secret_cleanup(KICK_CLEANUP_KEY)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn kick_clear_preserves_replacement_written_during_native_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let owner = Store::open(path.clone()).unwrap();
    let observer = Store::open_observer(path).unwrap();
    let key = "KICK_SESSION_TOKEN";
    observer
        .sync_settings(
            &BTreeMap::from([(key.into(), OLD_KICK_REFERENCE.into())]),
            "test",
        )
        .unwrap();
    clear_kick_token_in_store(&owner, |value| {
        assert_eq!(value, OLD_KICK_REFERENCE);
        observer.sync_settings(
            &BTreeMap::from([(key.into(), NEW_KICK_REFERENCE.into())]),
            "test",
        )
    })
    .unwrap();
    owner.refresh_config_cache().unwrap();
    assert_eq!(
        owner.setting_value(key).unwrap().unwrap(),
        NEW_KICK_REFERENCE
    );
}

#[tokio::test]
async fn desktop_close_action_defaults_to_exit_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let backend = dir.path().join("backend");
    std::fs::create_dir_all(&backend).unwrap();
    let db = Store::default_path(&backend);
    let core = StreamArchiveCore::assemble(backend, Store::open(db.clone()).unwrap()).unwrap();
    assert_eq!(
        core.settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
        "EXIT"
    );
    core.update_environment_settings(&BTreeMap::from([(
        "STREAM_ARCHIVE_CLOSE_ACTION".into(),
        "TRAY".into(),
    )]))
    .await
    .unwrap();
    assert!(
        core.update_environment_settings(&BTreeMap::from([(
            "STREAM_ARCHIVE_CLOSE_ACTION".into(),
            "invalid".into()
        ),]))
            .await
            .is_err()
    );
    core.shutdown().await;
    drop(core);
    let reopened = Store::open(db).unwrap();
    assert_eq!(
        reopened.safe_settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
        "TRAY"
    );
}

#[tokio::test]
async fn desktop_close_action_defaults_to_exit_and_ask_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let backend = dir.path().join("backend");
    std::fs::create_dir_all(&backend).unwrap();
    let db = Store::default_path(&backend);
    let core = StreamArchiveCore::assemble(backend, Store::open(db.clone()).unwrap()).unwrap();
    assert_eq!(
        core.settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
        "EXIT"
    );
    core.update_environment_settings(&BTreeMap::from([(
        "STREAM_ARCHIVE_CLOSE_ACTION".into(),
        "ASK".into(),
    )]))
    .await
    .unwrap();
    assert!(
        core.update_environment_settings(&BTreeMap::from([(
            "STREAM_ARCHIVE_CLOSE_ACTION".into(),
            "invalid".into()
        ),]))
            .await
            .is_err()
    );
    core.shutdown().await;
    drop(core);
    let reopened = Store::open(db).unwrap();
    assert_eq!(
        reopened.safe_settings().unwrap()["STREAM_ARCHIVE_CLOSE_ACTION"],
        "ASK"
    );
}

#[tokio::test]
async fn native_restore_requires_idle_runtime_and_refreshes_canonical_store() {
    let dir = tempfile::tempdir().unwrap();
    let backend = dir.path().join("app").join("backend");
    std::fs::create_dir_all(&backend).unwrap();
    let store = Store::open(
        dir.path()
            .join("app")
            .join("data")
            .join("stream-archive.db"),
    )
    .unwrap();
    let core = StreamArchiveCore::assemble(backend, store).unwrap();

    core.store
        .sync_settings(
            &BTreeMap::from([("TEST_RESTORE".into(), "before".into())]),
            "test",
        )
        .unwrap();
    let snapshot = core.create_manual_backup().await.unwrap();
    let backup = snapshot
        .backups
        .iter()
        .find(|item| item.kind == "manual")
        .unwrap()
        .file_name
        .clone();

    core.store
        .sync_settings(
            &BTreeMap::from([("TEST_RESTORE".into(), "after".into())]),
            "test",
        )
        .unwrap();

    let outcome = core.restore_backup(&backup).await.unwrap();
    assert_eq!(outcome.restored.file_name, backup);
    assert_eq!(outcome.safety_backup.kind, "pre_restore");
    assert_eq!(
        core.store.setting_value("TEST_RESTORE").unwrap().as_deref(),
        Some("before")
    );
}

#[tokio::test]
async fn native_backup_policy_round_trips_through_shared_manager() {
    let dir = tempfile::tempdir().unwrap();
    let backend = dir.path().join("backend");
    std::fs::create_dir_all(&backend).unwrap();
    let store = Store::open(dir.path().join("stream-archive.db")).unwrap();
    let core = StreamArchiveCore::assemble(backend, store).unwrap();

    let policy = BackupPolicy {
        enabled: false,
        interval_hours: 12,
        keep_count: 4,
        retention_days: 7,
    };
    let saved = core.update_backup_policy(&policy, None).await.unwrap();
    assert_eq!(saved.policy, policy);
    let loaded = core.backup_snapshot().await.unwrap();
    assert_eq!(loaded.policy, policy);
}

#[tokio::test]
async fn native_storage_snapshot_uses_canonical_settings_and_store() {
    let dir = tempfile::tempdir().unwrap();
    let backend = dir.path().join("backend");
    std::fs::create_dir_all(&backend).unwrap();
    let store = Store::open(dir.path().join("stream-archive.db")).unwrap();
    let core = StreamArchiveCore::assemble(backend, store).unwrap();

    core.store
        .sync_settings(
            &BTreeMap::from([
                (
                    "OUTPUT_DIR".into(),
                    dir.path().join("live").display().to_string(),
                ),
                ("MIN_FREE_SPACE_GB".into(), "3.5".into()),
            ]),
            "test",
        )
        .unwrap();

    let snapshot = core.storage_snapshot().unwrap();
    assert_eq!(snapshot.threshold_gb, 3.5);
    assert!(!snapshot.volumes.is_empty());
    assert!(
        snapshot
            .volumes
            .iter()
            .any(|volume| volume.roles.iter().any(|role| role == "LIVE 기본"))
    );
}

#[tokio::test]
async fn runtime_logs_are_bounded_by_requested_tail() {
    let dir = tempfile::tempdir().unwrap();
    let core = StreamArchiveCore::assemble(
        dir.path().to_path_buf(),
        Store::open(dir.path().join("stream-archive.db")).unwrap(),
    )
    .unwrap();
    for index in 0..8 {
        core.logs.push(format!("line-{index}")).await;
    }
    let tail = core.runtime_logs(3).await;
    assert_eq!(tail, vec!["line-5", "line-6", "line-7"]);
}

#[tokio::test]
async fn notification_setting_defaults_enabled_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stream-archive.db");
    let core =
        StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db.clone()).unwrap())
            .unwrap();
    assert_eq!(
        core.settings().unwrap()["STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"],
        "true"
    );
    core.update_environment_settings(&BTreeMap::from([(
        "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS".into(),
        "false".into(),
    )]))
    .await
    .unwrap();
    drop(core);
    let reopened = Store::open(db).unwrap();
    assert_eq!(
        reopened.safe_settings().unwrap()["STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"],
        "false"
    );
}

#[tokio::test]
async fn environment_patch_is_atomic_and_uses_existing_web_keys() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stream-archive.db");
    let core =
        StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db.clone()).unwrap())
            .unwrap();
    let before = core.settings().unwrap();
    let invalid = BTreeMap::from([
        ("CHECK_INTERVAL".into(), "42".into()),
        (
            "FFMPEG_PATH".into(),
            dir.path().join("missing.exe").display().to_string(),
        ),
    ]);
    assert!(core.update_environment_settings(&invalid).await.is_err());
    assert_eq!(core.settings().unwrap(), before);
    let valid = BTreeMap::from([
        ("CHECK_INTERVAL".into(), "42".into()),
        ("FFMPEG_PATH".into(), String::new()),
    ]);
    core.update_environment_settings(&valid).await.unwrap();
    assert_eq!(core.settings().unwrap()["CHECK_INTERVAL"], "42");
    assert_eq!(core.vod_tool_settings().unwrap()["FFMPEG_PATH"], "");
    drop(core);
    let reopened = Store::open(db).unwrap();
    assert_eq!(reopened.safe_settings().unwrap()["CHECK_INTERVAL"], "42");
}

#[tokio::test]
async fn native_provider_update_reuses_safe_setting_validation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stream-archive.db");
    let core =
        StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db).unwrap()).unwrap();

    let valid = BTreeMap::from([
        ("SOOP_USERNAME".into(), "tester".into()),
        (
            "CLOUDFLARE_WORKER_URL".into(),
            "https://worker.example.test".into(),
        ),
    ]);
    core.update_provider_configuration(&valid, &BTreeMap::new())
        .await
        .unwrap();
    let settings = core.settings().unwrap();
    assert_eq!(settings["SOOP_USERNAME"], "tester");
    assert_eq!(
        settings["CLOUDFLARE_WORKER_URL"],
        "https://worker.example.test"
    );

    let invalid = BTreeMap::from([("CHECK_INTERVAL".into(), "1".into())]);
    assert!(
        core.update_provider_configuration(&invalid, &BTreeMap::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn queue_facade_applies_existing_tool_defaults_and_history_filter_validates() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stream-archive.db");
    let core =
        StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db).unwrap()).unwrap();
    let req = VodDownloadRequest {
        vod_url: "https://vod.sooplive.com/player/123456789".into(),
        output_directory: dir.path().display().to_string(),
        parts: vec![],
        quality: "best".into(),
        merge: true,
        cookie_mode: "SOOP_LOGIN".into(),
        cookie_file: String::new(),
        browser_name: "firefox".into(),
        yt_dlp_path: String::new(),
        ffmpeg_path: String::new(),
        max_retries: 5,
    };
    let queued = core.enqueue_vod(req).await.unwrap();
    assert_eq!(queued.state, "QUEUED");
    assert!(
        core.history(&HistoryFilter {
            from: Some("bad-date".into()),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn window_bounds_round_trip_validates_and_preserves_first_run_flag() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stream-archive.db");
    let core =
        StreamArchiveCore::assemble(dir.path().to_path_buf(), Store::open(db).unwrap()).unwrap();

    assert_eq!(core.window_bounds().unwrap(), None);
    assert!(core.is_first_run_unconfigured().unwrap());

    core.save_window_bounds("120,80,1280,820,0").unwrap();
    assert_eq!(
        core.window_bounds().unwrap().as_deref(),
        Some("120,80,1280,820,0")
    );
    // Saving window geometry alone must not suppress first-run onboarding.
    assert!(core.is_first_run_unconfigured().unwrap());

    for bad in [
        "",
        "120,80,500,820,0",
        "120,80,1280,400,0",
        "120,80,1280,820,2",
        "120,80,1280,820,0\n1",
        "not,valid,bounds,at,all",
    ] {
        assert!(core.save_window_bounds(bad).is_err());
    }
}
