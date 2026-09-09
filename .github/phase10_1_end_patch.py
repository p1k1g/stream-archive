from pathlib import Path

p = Path('rust-web/src/native_watcher.rs')
s = p.read_text(encoding='utf-8')

old = '''                monitor_recordings(&mut states, &config, &recorder, &logs).await;
                poll_channels(&mut states, &config, &mut session, &recorder, &logs).await;
                update_snapshot(&states, &snapshot).await;'''
new = '''                check_recording_broadcasts(&mut states, &config, &mut session, &recorder, &logs).await;
                monitor_recordings(&mut states, &config, &recorder, &logs).await;
                poll_channels(&mut states, &config, &mut session, &recorder, &logs).await;
                update_snapshot(&states, &snapshot).await;'''
if old not in s:
    raise SystemExit('loop insertion target not found')
s = s.replace(old, new, 1)

marker = '''async fn monitor_recordings(
    states: &mut HashMap<String, ChannelState>,'''
func = r'''async fn check_recording_broadcasts(
    states: &mut HashMap<String, ChannelState>,
    config: &WatcherConfig,
    session: &mut SoopSession,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else {
            continue;
        };
        let Some(recording_bno) = state.recording.as_ref().map(|rec| rec.bno.clone()) else {
            continue;
        };
        if Instant::now() < state.next_check {
            continue;
        }
        state.next_check = Instant::now() + Duration::from_secs(config.check_interval.max(1));

        match session.live_info(&state.channel.account).await {
            Ok(LiveResult::Offline) => {
                clear_stream_password(&state.channel.account);
                stop_state_recording(state, "BROADCAST ENDED", recorder, logs).await;
                state.status = "OFFLINE".into();
                state.last_bno = None;
                state.detail = None;
                logs.push(format!(
                    "[RUST] broadcast ended account={}",
                    state.channel.account
                ))
                .await;
            }
            Ok(LiveResult::Live(live)) if live.bno != recording_bno => {
                clear_stream_password(&state.channel.account);
                stop_state_recording(state, "BROADCAST CHANGED", recorder, logs).await;
                state.status = "UNKNOWN".into();
                state.last_bno = Some(live.bno);
                state.detail = None;
                state.next_check = Instant::now();
                logs.push(format!(
                    "[RUST] broadcast number changed account={}; restarting discovery",
                    state.channel.account
                ))
                .await;
            }
            Ok(LiveResult::Live(_)) => {}
            Ok(LiveResult::AuthRequired) => {
                logs.push(format!(
                    "[RUST:WARN] live recheck requires SOOP auth while recording {}; keeping recorder running",
                    state.channel.account
                ))
                .await;
            }
            Err(err) => {
                logs.push(format!(
                    "[RUST:WARN] live recheck failed while recording {}; keeping recorder running: {err:#}",
                    state.channel.account
                ))
                .await;
            }
        }
    }
}

'''
if marker not in s:
    raise SystemExit('function insertion target not found')
s = s.replace(marker, func + marker, 1)
p.write_text(s, encoding='utf-8', newline='\n')
