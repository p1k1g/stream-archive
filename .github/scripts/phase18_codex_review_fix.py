from pathlib import Path

vod_path = Path('rust-web/src/platform/chzzk/vod.rs')
text = vod_path.read_text(encoding='utf-8')

old = '''struct DestinationClaim {
    target: PathBuf,
    claim_path: PathBuf,
    lock: Option<File>,
}
'''
new = '''struct DestinationClaim {
    target: PathBuf,
    lock: Option<File>,
}
'''
if old not in text:
    raise RuntimeError('DestinationClaim struct anchor missing')
text = text.replace(old, new, 1)

old = '''impl Drop for DestinationClaim {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.finalizing_path());
        if let Some(lock) = self.lock.take() {
            let _ = FileExt::unlock(&lock);
            drop(lock);
        }
        let _ = fs::remove_file(&self.claim_path);
    }
}
'''
new = '''impl Drop for DestinationClaim {
    fn drop(&mut self) {
        // Keep the sidecar pathname as a reusable lock anchor. Unlinking it after
        // unlock lets a contender lock the old inode while a third process creates
        // and locks a new inode at the same pathname.
        let _ = fs::remove_file(self.finalizing_path());
        if let Some(lock) = self.lock.take() {
            let _ = FileExt::unlock(&lock);
            drop(lock);
        }
    }
}
'''
if old not in text:
    raise RuntimeError('DestinationClaim Drop anchor missing')
text = text.replace(old, new, 1)

old = '''                return Ok(DestinationClaim {
                    target,
                    claim_path,
                    lock: Some(lock),
                });
'''
new = '''                return Ok(DestinationClaim {
                    target,
                    lock: Some(lock),
                });
'''
if old not in text:
    raise RuntimeError('DestinationClaim construction anchor missing')
text = text.replace(old, new, 1)

old = '''    let destination = claim_collision_path(&output_dir, &base, "ts")?;
    let final_output = destination.target().to_path_buf();
    let staging_output = job_dir.join(MEDIA_FILE_NAME);
'''
new = '''    let mut destination = claim_collision_path(&output_dir, &base, "ts")?;
    let staging_output = job_dir.join(MEDIA_FILE_NAME);
'''
if old not in text:
    raise RuntimeError('run_download destination anchor missing')
text = text.replace(old, new, 1)

old = '''                if !finalize_output(&staged_file, &destination, cancel)? {
                    cleanup_job_media(&job_dir);
                    return Ok(());
                }
                let mut current = status.write().await;
                current.state = "COMPLETED".into();
                current.running = false;
                current.message = "CHZZK VOD 다운로드가 완료되었습니다.".into();
                current.output_file = Some(final_output.display().to_string());
                current.percent = 100.0;
                current.finished_at = Some(Utc::now().to_rfc3339());
                logs.push(format!(
                    "[VOD:CHZZK] completed file={}",
                    final_output.display()
                ))
                .await;
                return Ok(());
'''
new = '''                loop {
                    match finalize_output(&staged_file, &destination, cancel)? {
                        PublishOutcome::Published => break,
                        PublishOutcome::Cancelled => {
                            cleanup_job_media(&job_dir);
                            return Ok(());
                        }
                        PublishOutcome::Collision => {
                            logs.push(format!(
                                "[VOD:CHZZK] destination appeared during publish; selecting next name: {}",
                                destination.target().display()
                            ))
                            .await;
                            drop(destination);
                            destination = claim_collision_path(&output_dir, &base, "ts")?;
                        }
                    }
                }
                let final_output = destination.target().to_path_buf();
                let mut current = status.write().await;
                current.state = "COMPLETED".into();
                current.running = false;
                current.message = "CHZZK VOD 다운로드가 완료되었습니다.".into();
                current.output_file = Some(final_output.display().to_string());
                current.percent = 100.0;
                current.finished_at = Some(Utc::now().to_rfc3339());
                logs.push(format!(
                    "[VOD:CHZZK] completed file={}",
                    final_output.display()
                ))
                .await;
                return Ok(());
'''
if old not in text:
    raise RuntimeError('run_download finalize anchor missing')
text = text.replace(old, new, 1)

old = '''    let mut streamlink_stdout = streamlink_child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Streamlink stdout unavailable"))?;
'''
new = '''    let mut streamlink_stdout = match streamlink_child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_owned(&mut streamlink_child).await;
            bail!("Streamlink stdout unavailable");
        }
    };
'''
if old not in text:
    raise RuntimeError('streamlink stdout setup anchor missing')
text = text.replace(old, new, 1)

old = '''    let mut ffmpeg_child = ffmpeg_command
        .spawn()
        .with_context(|| format!("FFmpeg 실행 실패: {}", ffmpeg.display()))?;
    let mut ffmpeg_stdin = ffmpeg_child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("FFmpeg stdin unavailable"))?;
'''
new = '''    let mut ffmpeg_child = match ffmpeg_command.spawn() {
        Ok(child) => child,
        Err(err) => {
            terminate_owned(&mut streamlink_child).await;
            return Err(err).with_context(|| format!("FFmpeg 실행 실패: {}", ffmpeg.display()));
        }
    };
    let mut ffmpeg_stdin = match ffmpeg_child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            terminate_owned(&mut streamlink_child).await;
            terminate_owned(&mut ffmpeg_child).await;
            bail!("FFmpeg stdin unavailable");
        }
    };
'''
if old not in text:
    raise RuntimeError('ffmpeg setup anchor missing')
text = text.replace(old, new, 1)

start = text.index('fn publish_by_copy(')
end = text.index('\nfn cleanup_job_media(', start)
replacement = r'''#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishOutcome {
    Published,
    Cancelled,
    Collision,
}

fn publish_by_copy(
    source: &Path,
    destination: &DestinationClaim,
    cancel: &AtomicBool,
    link_error: &std::io::Error,
) -> Result<PublishOutcome> {
    let target = destination.target();
    if target.exists() {
        return Ok(PublishOutcome::Collision);
    }
    let temp = destination.finalizing_path();
    if temp.is_file() {
        fs::remove_file(&temp).with_context(|| {
            format!(
                "CHZZK VOD stale destination 임시 파일 정리 실패: {}",
                temp.display()
            )
        })?;
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(PublishOutcome::Cancelled);
    }

    let publish = (|| -> Result<PublishOutcome> {
        let mut input = File::open(source)
            .with_context(|| format!("CHZZK VOD staging 파일 열기 실패: {}", source.display()))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| {
                format!(
                    "CHZZK VOD destination 임시 파일 생성 실패: {}",
                    temp.display()
                )
            })?;
        let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
        loop {
            if cancel.load(Ordering::SeqCst) {
                drop(output);
                let _ = fs::remove_file(&temp);
                return Ok(PublishOutcome::Cancelled);
            }
            let read = input.read(&mut buffer).with_context(|| {
                format!("CHZZK VOD staging 파일 읽기 실패: {}", source.display())
            })?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).with_context(|| {
                format!("CHZZK VOD destination 임시 복사 실패: {}", temp.display())
            })?;
        }
        output.flush().with_context(|| {
            format!(
                "CHZZK VOD destination 임시 파일 flush 실패: {}",
                temp.display()
            )
        })?;
        output.sync_all().with_context(|| {
            format!(
                "CHZZK VOD destination 임시 파일 sync 실패: {}",
                temp.display()
            )
        })?;
        drop(output);

        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&temp);
            return Ok(PublishOutcome::Cancelled);
        }

        match fs::hard_link(&temp, target) {
            Ok(()) => {
                let _ = fs::remove_file(&temp);
                Ok(PublishOutcome::Published)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp);
                Ok(PublishOutcome::Collision)
            }
            Err(err) => Err(err).with_context(|| {
                format!(
                    "CHZZK VOD destination no-replace publish 실패: {} -> {}",
                    temp.display(),
                    target.display()
                )
            }),
        }
    })();

    match publish {
        Ok(PublishOutcome::Published) => {
            let _ = fs::remove_file(source);
            Ok(PublishOutcome::Published)
        }
        Ok(PublishOutcome::Cancelled) => {
            let _ = fs::remove_file(&temp);
            Ok(PublishOutcome::Cancelled)
        }
        Ok(PublishOutcome::Collision) => {
            let _ = fs::remove_file(&temp);
            Ok(PublishOutcome::Collision)
        }
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err).with_context(|| {
                format!(
                    "CHZZK VOD 최종 파일 이동 실패 (direct link: {link_error}): {} -> {}",
                    source.display(),
                    target.display()
                )
            })
        }
    }
}

fn finalize_output(
    source: &Path,
    destination: &DestinationClaim,
    cancel: &AtomicBool,
) -> Result<PublishOutcome> {
    let target = destination.target();
    if cancel.load(Ordering::SeqCst) {
        return Ok(PublishOutcome::Cancelled);
    }

    // hard_link is an atomic create-if-absent publication primitive for a complete
    // regular file. Unlike rename on Unix, it never replaces an existing target.
    match fs::hard_link(source, target) {
        Ok(()) => {
            let _ = fs::remove_file(source);
            Ok(PublishOutcome::Published)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(PublishOutcome::Collision)
        }
        Err(link_error) => publish_by_copy(source, destination, cancel, &link_error),
    }
}
'''
text = text[:start] + replacement + text[end:]

old = '''        assert!(!publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap());
        assert!(source.exists());
'''
new = '''        assert_eq!(
            publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap(),
            PublishOutcome::Cancelled
        );
        assert!(source.exists());
'''
if old not in text:
    raise RuntimeError('cancelled publish test anchor missing')
text = text.replace(old, new, 1)

old = '''        assert!(publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap());
        assert!(!source.exists());
'''
new = '''        assert_eq!(
            publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap(),
            PublishOutcome::Published
        );
        assert!(!source.exists());
'''
if old not in text:
    raise RuntimeError('atomic copy publish test anchor missing')
text = text.replace(old, new, 1)

anchor = '''    #[test]
    fn cancelled_copy_publish_keeps_final_unpublished() {
'''
insert = '''    #[test]
    fn destination_claim_sidecar_is_reused_after_release() {
        let temp = tempfile::tempdir().unwrap();
        let target;
        let sidecar;
        {
            let first = claim_collision_path(temp.path(), "same", "ts").unwrap();
            target = first.target().to_path_buf();
            sidecar = claim_path(&target);
            assert!(sidecar.is_file());
        }
        assert!(sidecar.is_file());
        let second = claim_collision_path(temp.path(), "same", "ts").unwrap();
        assert_eq!(second.target(), target.as_path());
        assert!(sidecar.is_file());
    }

    #[test]
    fn late_external_collision_is_not_clobbered_and_retargets() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.ts");
        fs::write(&source, b"complete-media").unwrap();
        let first = claim_collision_path(temp.path(), "final", "ts").unwrap();
        let first_target = first.target().to_path_buf();
        fs::write(&first_target, b"external-data").unwrap();
        let cancel = AtomicBool::new(false);

        assert_eq!(
            finalize_output(&source, &first, &cancel).unwrap(),
            PublishOutcome::Collision
        );
        assert_eq!(fs::read(&first_target).unwrap(), b"external-data");
        assert!(source.exists());
        drop(first);

        let second = claim_collision_path(temp.path(), "final", "ts").unwrap();
        assert_eq!(second.target().file_name().unwrap(), "final_02.ts");
        assert_eq!(
            finalize_output(&source, &second, &cancel).unwrap(),
            PublishOutcome::Published
        );
        assert_eq!(fs::read(second.target()).unwrap(), b"complete-media");
        assert!(!source.exists());
    }

'''
if anchor not in text:
    raise RuntimeError('test insertion anchor missing')
text = text.replace(anchor, insert + anchor, 1)

vod_path.write_text(text, encoding='utf-8')

guard_path = Path('maintenance/Test-Phase18ChzzkVod.ps1')
guard = guard_path.read_text(encoding='utf-8')
old = '''Assert-Match $chzzkVod 'claim_collision_path' 'Atomic destination claim helper is missing.'
Assert-Match $chzzkVod 'lock\\.try_lock_exclusive\\(\\)' 'Destination filename claim is not protected by an OS lock.'
Assert-Match $chzzkVod 'finalizing_path' 'Destination-side atomic publication temp path is missing.'
'''
new = '''Assert-Match $chzzkVod 'claim_collision_path' 'Atomic destination claim helper is missing.'
Assert-Match $chzzkVod 'lock\\.try_lock_exclusive\\(\\)' 'Destination filename claim is not protected by an OS lock.'
Assert-Match $chzzkVod 'reusable lock anchor' 'Destination claim sidecars must remain reusable lock anchors after release.'
Assert-NotMatch $chzzkVod 'remove_file\\(&self\\.claim_path\\)' 'Destination claim teardown must not unlink the reusable lock anchor.'
Assert-Match $chzzkVod 'finalizing_path' 'Destination-side atomic publication temp path is missing.'
'''
if old not in guard:
    raise RuntimeError('guard destination anchor missing')
guard = guard.replace(old, new, 1)

old = '''Assert-Match $chzzkVod 'publish_by_copy' 'Cross-volume atomic publication helper is missing.'
Assert-NotMatch $chzzkVod 'fs::copy\\(source,\\s*target\\)' 'CHZZK VOD must never copy directly into the final pathname.'
'''
new = '''Assert-Match $chzzkVod 'publish_by_copy' 'Cross-volume atomic publication helper is missing.'
Assert-Match $chzzkVod 'fs::hard_link\\(' 'Final publication must use an atomic no-replace primitive.'
Assert-Match $chzzkVod 'PublishOutcome::Collision' 'Late external destination collisions must be detected without clobbering.'
Assert-NotMatch $chzzkVod 'fs::copy\\(source,\\s*target\\)' 'CHZZK VOD must never copy directly into the final pathname.'
'''
if old not in guard:
    raise RuntimeError('guard publish anchor missing')
guard = guard.replace(old, new, 1)

old = '''Assert-Match $chzzkVod 'concurrent_destination_claims_choose_distinct_collision_paths' 'Concurrent destination claim regression test is missing.'
Assert-Match $chzzkVod 'cancelled_copy_publish_keeps_final_unpublished' 'Cancelled destination publication regression test is missing.'
'''
new = '''Assert-Match $chzzkVod 'concurrent_destination_claims_choose_distinct_collision_paths' 'Concurrent destination claim regression test is missing.'
Assert-Match $chzzkVod 'destination_claim_sidecar_is_reused_after_release' 'Reusable destination-claim sidecar regression test is missing.'
Assert-Match $chzzkVod 'late_external_collision_is_not_clobbered_and_retargets' 'Late external no-clobber collision regression test is missing.'
Assert-Match $chzzkVod 'cancelled_copy_publish_keeps_final_unpublished' 'Cancelled destination publication regression test is missing.'
'''
if old not in guard:
    raise RuntimeError('guard tests anchor missing')
guard = guard.replace(old, new, 1)

old = '''Assert-Match $chzzkVod 'taskkill\\.exe' 'Windows owned-process cancellation path is missing.'
Assert-Match $chzzkVod '\\.arg\\("/PID"\\)' 'CHZZK VOD cancellation is not PID scoped.'
'''
new = '''Assert-Match $chzzkVod 'taskkill\\.exe' 'Windows owned-process cancellation path is missing.'
Assert-Match $chzzkVod 'Err\\(err\\)\\s*=>\\s*\\{[\\s\\S]*?terminate_owned\\(&mut streamlink_child\\)\\.await;[\\s\\S]*?FFmpeg 실행 실패' 'Streamlink owned tree is not terminated when downstream FFmpeg setup fails.'
Assert-Match $chzzkVod '\\.arg\\("/PID"\\)' 'CHZZK VOD cancellation is not PID scoped.'
'''
if old not in guard:
    raise RuntimeError('guard process cleanup anchor missing')
guard = guard.replace(old, new, 1)

guard_path.write_text(guard, encoding='utf-8')
print('Applied final Phase 18 Codex review fixes')
