# Phase 21.5 — Native VOD Operations

## Scope

The Slint VOD page uses the existing shared Rust VOD runtime instead of adding a GUI-specific downloader.

`Slint -> GUI controller / vod_adapter -> StreamArchiveCore -> VodManager -> SOOP / CHZZK provider -> media tools`

The native frontend does not call localhost HTTP, open SQLite directly, contact provider endpoints directly, or spawn/kill yt-dlp, Streamlink, or FFmpeg itself. Tool defaults, provider authentication, process ownership, cancellation, and history persistence remain behind the shared runtime boundary.

Phase 21.5 covers a single VOD operation only: Analyze, metadata/quality/part selection, Download, Progress, and Cancel. Persistent Queue and History UI remain Phase 21.6 work.

## Native VOD workflow

1. Enter a supported SOOP or CHZZK VOD URL.
2. Select **Analyze**.
3. Wait for the runtime to return canonical metadata, quality options, and part information.
4. Select a quality. The runtime-provided `value`/`label` pair remains authoritative.
5. Select all parts or only the required parts.
6. Select the output directory with the native folder picker or enter it manually.
7. Choose whether multiple parts should be merged.
8. Select **Download**.
9. The page polls `StreamArchiveCore::vod_status()` while active and displays state, current part, total parts, percentage, runtime message, timestamps, and final output file.
10. Use **Cancel** to cancel through `StreamArchiveCore::cancel_vod()`.

Changing the URL invalidates the previous analysis selection while retaining the output-directory draft. A result belonging to an older URL is ignored instead of being applied to the current draft.

## Shared runtime ownership

Analyze and Download send request models to `StreamArchiveCore`; the GUI leaves configured yt-dlp/FFmpeg paths blank so the canonical persisted VOD tool defaults are applied by the shared core. SOOP/CHZZK URL validation, cookie/authentication behavior, provider-specific download mechanics, retry policy, and child-process ownership stay in the existing provider/runtime modules.

Application shutdown continues through `StreamArchiveCore::shutdown()`. Phase 21.4 manually verified that closing the Native app while LIVE recording also terminates the owned Streamlink process. Phase 21.5 must preserve the same ownership rule for VOD children.

## Manual Windows acceptance

Run from the repository root:

```powershell
cargo run --locked --manifest-path rust-gui\Cargo.toml
```

Then verify:

1. **SOOP Analyze** — enter a valid SOOP VOD URL and confirm Analyze reaches `READY` and displays title/streamer.
2. **CHZZK Analyze** — enter a valid CHZZK video URL and confirm Analyze reaches `READY` and displays title/streamer.
3. **Quality list** — confirm only runtime-returned quality choices are displayed and one is selected by default.
4. **Part list** — confirm SOOP multipart metadata shows the expected part count and durations. CHZZK should follow its existing single-video contract.
5. **Select All / Clear** — clear all parts and confirm Download becomes unavailable; select all and confirm readiness returns when the other required fields are valid.
6. **Partial SOOP download** — select only one or a subset of parts and verify only those parts are requested/downloaded.
7. **Quality selection** — choose a non-default available quality and verify the runtime accepts it.
8. **Native output picker** — select an output directory and confirm it is copied into the draft. Cancel the picker and confirm the previous path is retained.
9. **Full download** — start a normal VOD download and confirm state/progress update without manual Refresh.
10. **Progress** — confirm percentage and current part change while downloading and the page remains responsive.
11. **Completion** — confirm `COMPLETED`, 100%, final output path, and the actual output file.
12. **Cancel** — cancel an active Analyze/Download and confirm it reaches the runtime terminal cancel state without leaving the GUI busy indefinitely.
13. **Cancel then restart** — after Cancel, Analyze/Download another VOD successfully.
14. **Application shutdown** — close the Native app during an active VOD job and verify Stream Archive-owned yt-dlp/Streamlink/FFmpeg child processes do not remain orphaned.
15. **Invalid URL** — use an unsupported/invalid URL and confirm a useful error is shown without panic or window exit.
16. **Authentication failure** — where reproducible, use missing/invalid provider credentials and confirm the existing provider error is surfaced without exposing secret values.
17. **Missing tool** — temporarily point/remove a required media tool and confirm the runtime reports the missing tool clearly.
18. **UI responsiveness** — navigate Dashboard/LIVE/Channels/Settings during Analyze/Download and confirm the Slint window remains responsive.
19. **LIVE regression** — start the LIVE watcher and confirm its polling/status/actions still work after using VOD.
20. **LIVE + VOD coexistence** — where the existing runtime contract permits it, run the watcher while analyzing/downloading a VOD and confirm neither presentation path corrupts the other.
21. **Settings/Diagnostics regression** — confirm Phase 21.3/21.4 Settings, credentials, diagnostics, resize/maximize, Channels Resolve, and LIVE controls remain functional.

## Acceptance notes

- The VOD page polls only while selected and prevents overlapping VOD poll requests.
- Explicit Analyze/Download/Cancel/picker operations suppress overlapping VOD polling through the native busy state.
- Temporary status polling failures must leave the last good visible snapshot rather than clearing the page.
- Unknown future runtime state strings are displayed rather than converted into GUI-owned runtime states.
- Queue persistence/retry/reorder and History browsing are intentionally excluded from this phase.
