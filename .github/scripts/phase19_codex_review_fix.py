from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise RuntimeError(f"expected exactly one match in {path}, found {text.count(old)}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8", newline="\n")


# Preserve the Recording owner when a checked tree-stop fails. This lets the
# watcher retry instead of dropping the handle while descendants may still live.
replace_once(
    "rust-web/src/native_watcher.rs",
    '''        Err(err) => {
            state.detail = Some(format!("stop failed: {err}"));
            logs.push(format!(
                "[RUST:ERR] recorder stop failed {}/{} pid={}: {err:#}",
                state.channel.platform, state.channel.account, rec.pid
            ))
            .await;
        }
''',
    '''        Err(err) => {
            state.status = "ERROR".into();
            state.detail = Some(format!("stop failed: {err}"));
            logs.push(format!(
                "[RUST:ERR] recorder stop failed {}/{} pid={}: {err:#}; retaining ownership for retry",
                state.channel.platform, state.channel.account, rec.pid
            ))
            .await;
            state.recording = Some(rec);
        }
''',
)

# If a channel disappears while stop failed, keep the ChannelState around so
# the retained Recording owner is not dropped by states.remove().
replace_once(
    "rust-web/src/native_watcher.rs",
    '''    let existing: Vec<String> = states.keys().cloned().collect();
    for key in existing {
        if !incoming.contains_key(&key) {
            if let Some(mut state) = states.remove(&key) {
                stop_state_recording(&mut state, "CHANNEL REMOVED", recorder, logs).await;
                logs.push(format!(
                    "[RUST] channel removed: {}/{}",
                    state.channel.platform, state.channel.account
                ))
                .await;
            }
        }
    }
''',
    '''    let existing: Vec<String> = states.keys().cloned().collect();
    for key in existing {
        if !incoming.contains_key(&key) {
            let mut can_remove = false;
            if let Some(state) = states.get_mut(&key) {
                stop_state_recording(state, "CHANNEL REMOVED", recorder, logs).await;
                can_remove = state.recording.is_none();
                if !can_remove {
                    state.channel.enabled = false;
                    state.status = "ERROR".into();
                    state.detail = Some(
                        "channel removed, but recorder ownership is retained until process-tree cleanup succeeds"
                            .into(),
                    );
                    logs.push(format!(
                        "[RUST:WARN] channel removal deferred while recorder cleanup is retained: {}/{}",
                        state.channel.platform, state.channel.account
                    ))
                    .await;
                }
            }
            if can_remove {
                if let Some(state) = states.remove(&key) {
                    logs.push(format!(
                        "[RUST] channel removed: {}/{}",
                        state.channel.platform, state.channel.account
                    ))
                    .await;
                }
            }
        }
    }
''',
)

# Add a failure-path drain helper. Bounded readers can be blocked on send when
# a child exits; drain while waiting for readers so the newest stderr reaches
# the error tail instead of being discarded by abort().
replace_once(
    "rust-web/src/platform/chzzk/vod.rs",
    '''async fn abort_reader_tasks(tasks: Vec<JoinHandle<()>>) {
    for task in &tasks {
        task.abort();
    }
    join_reader_tasks(tasks).await;
}
''',
    '''async fn abort_reader_tasks(tasks: Vec<JoinHandle<()>>) {
    for task in &tasks {
        task.abort();
    }
    join_reader_tasks(tasks).await;
}

async fn drain_failed_reader_tasks(
    tasks: Vec<JoinHandle<()>>,
    log_rx: &mut mpsc::Receiver<String>,
    progress_rx: &mut mpsc::Receiver<String>,
    tail: &mut VecDeque<String>,
) {
    while !tasks.iter().all(|task| task.is_finished()) {
        tokio::select! {
            line = log_rx.recv(), if !log_rx.is_closed() => {
                if let Some(line) = line {
                    push_tail(tail, &line);
                }
            }
            _ = progress_rx.recv(), if !progress_rx.is_closed() => {}
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
    }
    while let Ok(line) = log_rx.try_recv() {
        push_tail(tail, &line);
    }
    while progress_rx.try_recv().is_ok() {}
    join_reader_tasks(tasks).await;
}
''',
)

old_streamlink_failure = '''                    terminate_owned(&mut ffmpeg_child).await;
                    pump.abort();
                    let _ = pump.await;
                    abort_reader_tasks(reader_tasks).await;
                    let _ = fs::remove_file(output);
                    while let Ok(line) = log_rx.try_recv() {
                        push_tail(&mut tail, &line);
                    }
                    bail!(
                        "Streamlink CHZZK 다운로드 실패 (exit={}): {}",
'''
new_streamlink_failure = '''                    terminate_owned(&mut ffmpeg_child).await;
                    pump.abort();
                    let _ = pump.await;
                    drain_failed_reader_tasks(
                        reader_tasks,
                        &mut log_rx,
                        &mut progress_rx,
                        &mut tail,
                    )
                    .await;
                    let _ = fs::remove_file(output);
                    bail!(
                        "Streamlink CHZZK 다운로드 실패 (exit={}): {}",
'''
replace_once("rust-web/src/platform/chzzk/vod.rs", old_streamlink_failure, new_streamlink_failure)

old_ffmpeg_failure = '''                    terminate_owned(&mut streamlink_child).await;
                    pump.abort();
                    let _ = pump.await;
                    abort_reader_tasks(reader_tasks).await;
                    let _ = fs::remove_file(output);
                    while let Ok(line) = log_rx.try_recv() {
                        push_tail(&mut tail, &line);
                    }
                    bail!(
                        "FFmpeg CHZZK MPEG-TS 저장 실패 (exit={}): {}",
'''
new_ffmpeg_failure = '''                    terminate_owned(&mut streamlink_child).await;
                    pump.abort();
                    let _ = pump.await;
                    drain_failed_reader_tasks(
                        reader_tasks,
                        &mut log_rx,
                        &mut progress_rx,
                        &mut tail,
                    )
                    .await;
                    let _ = fs::remove_file(output);
                    bail!(
                        "FFmpeg CHZZK MPEG-TS 저장 실패 (exit={}): {}",
'''
replace_once("rust-web/src/platform/chzzk/vod.rs", old_ffmpeg_failure, new_ffmpeg_failure)

print("Phase 19 Codex review fixes applied")
