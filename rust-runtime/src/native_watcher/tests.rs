use super::*;

fn channel(name: &str, account: &str) -> Channel {
    Channel {
        platform: PlatformId::Soop,
        enabled: true,
        name: name.into(),
        account: account.into(),
        outdir: String::new(),
    }
}

fn chzzk_channel(enabled: bool) -> Channel {
    Channel {
        platform: PlatformId::Chzzk,
        enabled,
        name: "CHZZK".into(),
        account: "0123456789abcdef0123456789abcdef".into(),
        outdir: String::new(),
    }
}

#[test]
fn watcher_initial_and_reload_snapshots_observe_external_secret_changes() {
    let dir = tempfile::tempdir().unwrap();
    let streamlink_fixture = dir.path().join("streamlink.exe");
    fs::write(&streamlink_fixture, b"test fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&streamlink_fixture, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = dir.path().join("test.db");
    let owner = store::Store::open(path.clone()).unwrap();
    owner
        .sync_settings(
            &BTreeMap::from([
                ("SOOP_USERNAME".into(), "test-user".into()),
                (
                    "SOOP_PASSWORD".into(),
                    "native-secret:v1:123e4567-e89b-12d3-a456-426614174000".into(),
                ),
                (
                    "CLOUDFLARE_WORKER_URL".into(),
                    "https://worker.example".into(),
                ),
                ("CLOUDFLARE_API_KEY".into(), "test-worker-key".into()),
                (
                    "OUTPUT_DIR".into(),
                    dir.path().join("output").display().to_string(),
                ),
            ]),
            "test",
        )
        .unwrap();
    let observer = store::Store::open_observer(path).unwrap();
    // Owner cache still contains a retired native reference; no watcher refreshed it.
    observer
        .sync_settings(
            &BTreeMap::from([("SOOP_PASSWORD".into(), "replacement".into())]),
            "test",
        )
        .unwrap();
    let (initial, config) = WatcherConfig::load_if_changed(&owner, dir.path(), true, None).unwrap();
    assert_eq!(config.unwrap().soop_password, "replacement");
    assert!(
        WatcherConfig::load_if_changed(&owner, dir.path(), true, Some(&initial))
            .unwrap()
            .1
            .is_none()
    );
    observer
        .sync_settings(
            &BTreeMap::from([("SOOP_PASSWORD".into(), String::new())]),
            "test",
        )
        .unwrap();
    let (cleared, config) =
        WatcherConfig::load_if_changed(&owner, dir.path(), true, Some(&initial)).unwrap();
    assert_eq!(config.unwrap().soop_password, "");
    // Channel reload must also acquire a fresh snapshot, even without a settings tick.
    observer
        .sync_settings(
            &BTreeMap::from([("SOOP_PASSWORD".into(), "new-login".into())]),
            "test",
        )
        .unwrap();
    assert_eq!(
        WatcherConfig::load_if_changed(&owner, dir.path(), true, None)
            .unwrap()
            .1
            .unwrap()
            .soop_password,
        "new-login"
    );
    assert_ne!(cleared, initial);
}

#[test]
fn safe_filename_replaces_windows_reserved_chars() {
    assert_eq!(safe_name("a:b/c*?d", 80), "a_b_c__d");
    assert_eq!(safe_name("CON", 80), "_CON");
}

#[test]
fn channel_signature_is_case_insensitive_for_accounts() {
    let a = vec![channel("A", "UserA")];
    let b = vec![channel("A", "usera")];
    assert_eq!(channel_signature(&a), channel_signature(&b));
}

#[test]
fn soop_worker_credentials_are_needed_only_for_enabled_soop_channels() {
    assert!(!channels_require_soop(&[chzzk_channel(true)]));

    let mut disabled_soop = channel("SOOP", "disabled");
    disabled_soop.enabled = false;
    assert!(!channels_require_soop(&[
        disabled_soop,
        chzzk_channel(true)
    ]));

    assert!(channels_require_soop(&[
        channel("SOOP", "enabled"),
        chzzk_channel(true),
    ]));
}

#[test]
fn recorder_exit_outcome_distinguishes_crashes() {
    assert_eq!(
        recording_exit_outcome(Some(0)),
        (true, "NORMAL".to_string())
    );
    assert_eq!(
        recording_exit_outcome_with_size(Some(0), 1024),
        (true, "NORMAL".to_string())
    );
    assert_eq!(
        recording_exit_outcome_with_size(Some(0), 0),
        (false, "EMPTY OUTPUT (0 bytes)".to_string())
    );
    assert_eq!(
        recording_exit_outcome(Some(23)),
        (false, "RECORDER EXIT CODE=23".to_string())
    );
    assert_eq!(
        recording_exit_outcome(None),
        (false, "RECORDER EXIT CODE=unknown".to_string())
    );
}

#[test]
fn watcher_exit_replaces_paused_state_and_clears_runtime_markers() {
    let mut disabled = channel("Disabled", "disabled");
    disabled.enabled = false;
    let mut states = HashMap::new();
    let mut live = ChannelState::new(channel("Live", "live"));
    live.status = "PAUSED".into();
    live.last_broadcast_id = Some("123".into());
    live.suppressed_broadcast_id = Some("123".into());
    live.detail = Some("old detail".into());
    states.insert(channel_key(PlatformId::Soop, "live"), live);
    states.insert(
        channel_key(PlatformId::Soop, "disabled"),
        ChannelState::new(disabled),
    );

    mark_watcher_stopped(&mut states);

    let live = states.get(&channel_key(PlatformId::Soop, "live")).unwrap();
    assert_eq!(live.status, "WATCHER_STOPPED");
    assert!(live.last_broadcast_id.is_none());
    assert!(live.suppressed_broadcast_id.is_none());
    assert!(live.detail.is_none());
    assert_eq!(
        states
            .get(&channel_key(PlatformId::Soop, "disabled"))
            .unwrap()
            .status,
        "DISABLED"
    );
}

#[test]
fn channel_key_namespaces_accounts_by_platform() {
    assert_eq!(channel_key(PlatformId::Soop, "User"), "SOOP:user");
    assert_eq!(
        channel_key(PlatformId::Chzzk, "ABCDEF0123456789ABCDEF0123456789"),
        "CHZZK:abcdef0123456789abcdef0123456789"
    );
}

#[test]
fn probe_error_backoff_and_duplicate_log_suppression() {
    assert_eq!(
        backoff_delay(30, 1, PROBE_ERROR_MAX_BACKOFF_SECS).as_secs(),
        30
    );
    assert_eq!(
        backoff_delay(30, 2, PROBE_ERROR_MAX_BACKOFF_SECS).as_secs(),
        60
    );
    assert_eq!(
        backoff_delay(30, 3, PROBE_ERROR_MAX_BACKOFF_SECS).as_secs(),
        120
    );
    assert_eq!(
        backoff_delay(30, 6, PROBE_ERROR_MAX_BACKOFF_SECS).as_secs(),
        900
    );
    assert_eq!(
        backoff_delay(30, 12, PROBE_ERROR_MAX_BACKOFF_SECS).as_secs(),
        900
    );

    assert!(should_log_probe_error(1, false));
    assert!(!should_log_probe_error(2, true));
    assert!(should_log_probe_error(2, false));
    assert!(!should_log_probe_error(9, true));
    assert!(should_log_probe_error(10, true));
    assert!(!should_log_probe_error(11, true));
    assert!(should_log_probe_error(20, true));
}

#[tokio::test]
async fn bounded_concurrent_runner_caps_parallelism_and_collects_all_results() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let items: Vec<usize> = (0..18).collect();

    let mut results = run_bounded_concurrent(items, 4, |item| {
        let in_flight = in_flight.clone();
        let peak = peak.clone();
        async move {
            let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
            item * 2
        }
    })
    .await;

    results.sort_unstable();
    let expected: Vec<usize> = (0..18).map(|i| i * 2).collect();
    assert_eq!(results, expected);
    assert!(peak.load(Ordering::SeqCst) <= 4);
    assert!(peak.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn completion_revision_survives_same_broadcast_restart_in_snapshots() {
    let channel = Channel {
        platform: PlatformId::Soop,
        enabled: true,
        name: "test".into(),
        account: "test".into(),
        outdir: String::new(),
    };
    let mut state = ChannelState::new(channel);
    state.last_broadcast_id = Some("123".into());
    state.thumbnail_url = Some("https://fixture.invalid/thumbnail.jpg".into());
    state.recording_finished("123");
    assert!(state.thumbnail_url.is_none());
    let first = state.completion_revision;
    state.recording_finished("123");
    assert!(state.completion_revision > first);
    let snapshot = Arc::new(RwLock::new(WatcherStatus::default()));
    let mut states = HashMap::from([("SOOP:test".into(), state)]);
    update_snapshot(&mut states, &snapshot, Path::new("")).await;
    let snapshot = snapshot.read().await;
    assert_eq!(
        snapshot.channels[0].completed_broadcast_id.as_deref(),
        Some("123")
    );
    assert!(snapshot.channels[0].completion_revision > first);
}
