# Phase 23.1 — Native Daily-use UX Closure

## 1. Goal

Phase 23.1 closes the remaining Windows Slint Native daily-use UX work after Phase 22 architecture/legacy closure.

This phase does not add providers, content types, runtime architecture, database authority, or process ownership behavior.

The product boundary remains:

```text
Windows Slint Native GUI
        ↓
StreamArchiveCore
        ↓
shared Rust runtime
        ↓
SQLite / Providers / Processes / Media tools
```

The Native GUI remains a presentation/control layer:

- no direct SQLite authority;
- no direct media-process ownership;
- no localhost HTTP application API;
- no browser/Web fallback.

## 2. Baseline audit

The audit reviewed:

```text
rust-gui/ui/app-window.slint
rust-gui/ui/maintenance.slint
rust-gui/ui/queue-history.slint
rust-gui/src/*_adapter.rs
docs/PHASE21_NATIVE_UX_POLISH.md
docs/PHASE21_NATIVE_PORTABLE.md
README.md
docs/OPERATIONS.md
```

Historical Phase 21 documentation remains historical. Web/browser QA items documented there are not restored into the current product contract.

The current Native pages audited were:

```text
Settings / General
Settings / Manage
Channels
LIVE
VOD
Queue
History
Backup / Restore
Diagnostics
Runtime Logs
Storage status
```

## 3. Main finding

The primary remaining daily-use UX risk was not missing functionality. It was fixed-height presentation around content that is intentionally allowed to wrap.

Affected examples included:

- long LIVE channel titles and output paths;
- long VOD runtime messages/output files/titles;
- long Queue titles, URLs, failure messages and output paths;
- long History titles;
- long backup filenames and SHA-256 values;
- long runtime log lines.

Keeping those cards at fixed heights could visually clip wrapped text even though the underlying data/state was correct.

Phase 23.1 therefore keeps the same state model and action callbacks while allowing content-bearing cards to grow from a minimum height.

## 4. LIVE

The LIVE page retains:

- watcher state;
- recording/offline/error metrics;
- shared storage diagnostics;
- active channel rows;
- per-channel stop/recheck/resume/password actions.

Changes:

- LIVE channel cards now use a minimum height instead of a fixed height;
- channel heading text can wrap when the displayed name/account is long;
- existing title/file/detail wrapping can now expand the card rather than being constrained by the old fixed card height.

No watcher, recorder or owned-process behavior changed.

## 5. Storage

Storage status remains driven by shared `storage_service` through `StreamArchiveCore`.

No Slint filesystem probing was introduced.

The existing volume/status/capacity/roles/detail presentation remains intact, including warning/error tones and explicit refresh.

## 6. Channels

The Channels page remains a compact editing surface for:

- platform;
- enabled state;
- display name;
- account/canonical channel ID;
- output override;
- resolve/remove actions.

The existing compact row layout is retained because LineEdit controls already handle long editable values internally and the row does not duplicate a separate read-only channel title.

No channel persistence/validation logic moved into Slint.

## 7. VOD

Supported scope remains exactly:

```text
SOOP VOD
CHZZK VOD
```

The VOD status/result cards now use minimum heights for content-bearing status and metadata blocks.

This allows:

- long runtime messages;
- long output paths;
- long titles/streamer metadata

to remain readable without changing analysis/download semantics.

Quality/PART selection, destination selection, merge behavior, enqueue/download and cancel callbacks are unchanged.

## 8. Queue

Queue runtime state/action rules remain sourced from the existing adapter/runtime model.

The UI changes:

- Queue job cards now grow from a minimum height rather than using a fixed height;
- long queue titles may wrap;
- existing URL/message/output/timing wrapping can expand the card.

Runtime action availability remains unchanged:

```text
QUEUED / STARTING / RUNNING -> cancel where supported
FAILED / CANCELLED / INTERRUPTED -> retry
terminal rows -> remove where supported
```

No new queue state machine was introduced.

## 9. History

The fixed-width `전체 / LIVE / VOD` controls remain unchanged so selecting a filter does not shift the toolbar.

History row headings may now wrap while preserving the status indicator on the right.

A regression test explicitly guards the daily-use status labels used by the existing History search/filter UX:

```text
STOPPED   -> 중지됨
MERGING   -> 병합 중
RUNNING   -> 진행 중
CANCELLED -> 취소됨
RECORDING -> 녹화 중
COMPLETED -> 완료
```

Unknown future states remain visible rather than being dropped.

## 10. Settings / General

The General split remains:

- provider/authentication configuration;
- shared runtime/tool/path settings;
- native directory pickers;
- save/reload feedback.

No backup/diagnostics surface was duplicated back into General.

Runtime setting rows already use minimum-height cards and wrapping descriptions, while path/value editing remains inside LineEdit widgets.

No direct SQLite or secret-store access was added to Slint.

## 11. Settings / Manage

Manage remains the single administration surface for:

```text
Backup / Restore
Diagnostics
Runtime Logs
```

### Backup / Restore

Policy semantics remain unchanged:

- automatic backup;
- interval;
- keep count;
- retention days;
- `0 = unlimited`;
- environment-controlled read-only backup directory;
- integrity gate;
- pre-restore safety backup;
- watcher/VOD/queue restore gating.

Presentation changes:

- backup rows now grow from a minimum height;
- long backup filenames may wrap;
- SHA-256 is no longer constrained to a fixed 14px line;
- restore action/integrity state remains visible.

### Diagnostics

Diagnostics remain read-only in Phase 23.1.

No new health/preflight engine was added; that belongs to Phase 23.2.

Status text now uses the same explicit severity tone as the diagnostic row border:

- normal = green;
- warning = amber;
- error = red.

The runtime already treats the optional bundled VOD tools directory as informational/OK when absent, so Phase 23.1 does not weaken runtime diagnostic severity.

### Runtime Logs

Runtime log collection behavior remains unchanged:

- recent lines only;
- bounded list;
- multiline rows flattened by the adapter.

The log row presentation now uses a layout-managed wrapped Text element so a long line can increase row height rather than drawing beyond a minimum-height absolute-positioned container.

Log rotation/file-management features remain out of scope.

## 12. Empty / loading / error states

Existing empty/loading/error messaging was audited across the Native pages.

Current useful empty-state guidance is retained, including:

- no LIVE channels -> direct user to Channels;
- no configured channels -> explain how to add the first channel;
- empty VOD quality/PART state -> analyze first;
- empty Queue -> analyze/enqueue a VOD;
- empty History -> current filter has no matching records;
- empty Backup list -> no managed backup exists;
- empty Runtime Logs -> no runtime log yet.

Phase 23.1 avoids a large cross-page component refactor; consistency is achieved without changing runtime callbacks.

## 13. Long-content / resize behavior

The window contract remains:

```text
preferred: 1120 x 720
minimum:   1000 x 650
```

The phase targets supported desktop sizes rather than arbitrary tiny windows.

Content-bearing cards now have minimum heights where wrapped strings can grow.

Audited long-content categories:

- Korean/English channel names;
- VOD titles;
- URLs;
- deep Windows paths;
- output files;
- diagnostic details;
- error messages;
- SHA-256 values;
- runtime log lines.

## 14. No-clobber / claim contract

CHZZK publication safety is unchanged.

The phase does not modify:

```text
<final-media>.stream-archive.claim
collision suffixes
_02 / _03 behavior
.stream-archive.finalizing
crash recovery
publication atomicity
Windows Hidden attribute behavior
```

Internal claim files remain an implementation detail rather than a Native UX surface.

## 15. Backup / storage / process contracts

No SQLite schema or authority change is included.

Canonical data remains:

```text
data/stream-archive.db
```

No process lifecycle change is included.

The application continues to terminate only child/process trees it owns. Process-name-wide cleanup such as `taskkill /IM`, `pkill` or `killall` remains forbidden by the existing runtime contract.

## 16. Provider scope

No provider/content expansion is part of Phase 23.1.

Supported:

```text
SOOP LIVE
SOOP VOD
CHZZK LIVE
CHZZK VOD
```

Still unsupported:

```text
SOOP/CHZZK clips
CATCH
shorts
community/posts
other providers
```

## 17. Automated regression

The phase retains all existing runtime and Native tests and adds a History presentation regression covering common daily-use status labels.

Required validation:

```text
cargo fmt --manifest-path rust-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path rust-runtime/Cargo.toml
cargo check --locked --manifest-path rust-runtime/Cargo.toml
cargo clippy --locked --manifest-path rust-runtime/Cargo.toml --all-targets --all-features -- -D warnings

cargo fmt --manifest-path rust-gui/Cargo.toml -- --check
cargo check --locked --manifest-path rust-gui/Cargo.toml
cargo test --locked --manifest-path rust-gui/Cargo.toml
cargo clippy --locked --manifest-path rust-gui/Cargo.toml --all-targets --all-features -- -D warnings

maintenance/Test-RuntimeContracts.ps1
BUILD_PORTABLE.bat
```

First full PR validation completed successfully:

```text
GitHub Actions run: 35707712884

core-check (linux):   PASS
core-check (macos):   PASS
core-check (windows): PASS
windows-check:        PASS

windows-check:
- Runtime contract guard:         PASS
- Source archive metadata smoke:  PASS
- Windows portable package smoke: PASS
- Verify portable package:        PASS
```

The final documentation-only HEAD is revalidated before closure.

## 18. Windows portable manual QA checklist

Build and test the assembled product, not raw Cargo binaries:

1. run `BUILD_PORTABLE.bat`;
2. launch `dist\stream-archive\StreamArchive.exe` directly;
3. open Settings -> General and inspect provider/auth/runtime rows;
4. exercise native directory pickers and long paths;
5. open Settings -> Manage;
6. save backup policy, including `0 = unlimited`;
7. create/list a backup and inspect long filename/SHA-256 presentation;
8. verify restore confirmation and active-work gating;
9. inspect Diagnostics with both optional and required configuration missing/present;
10. inspect Runtime Logs with empty and long-line cases;
11. add/edit/remove SOOP and CHZZK channels;
12. test a long channel name and output override;
13. inspect LIVE storage state and manually refresh it;
14. start/stop Watcher and verify active/offline/error states;
15. analyze a SOOP or CHZZK VOD;
16. inspect a long VOD title, URL and output path;
17. enqueue a VOD and verify Queue cancel/retry/remove availability by state;
18. exercise History `전체 / LIVE / VOD`, search/status/date filters and long rows;
19. test normal, minimum-supported and maximized window sizes;
20. close/reopen the Native application and confirm canonical SQLite state persists.

Removed Phase 22 Web/browser fallback paths are intentionally absent from this checklist.

## 19. Remaining manual considerations

Automated CI can validate compile/test/contracts/package assembly but cannot replace visual inspection of arbitrary Windows fonts, monitor scaling, real user paths, or real provider media metadata.

The checklist above is the release/manual QA surface for those environment-dependent cases.

No remaining code-level Native UX blocker was identified that requires architecture, schema, provider-protocol or packaging redesign.

## 20. Phase 23.2 handoff

Phase 23.2 should build on the current read-only Diagnostics surface and create a shared runtime preflight/health model rather than adding more presentation-only checks.

Suggested Phase 23.2 targets:

- executable discovery + executable/version check;
- data/backup/output path readiness;
- SQLite integrity/readiness summary;
- native secret backend availability;
- required vs optional tool/config classification;
- one shared result model consumed by Native Diagnostics and Unix `doctor`;
- no provider network access unless explicitly designed as an opt-in probe.

Phase 23.1 does not implement this engine.

## 21. Status

Implementation and the first full CI/package validation are complete.

```text
Phase 23.1 implementation: COMPLETE
Automated CI/package validation: PASS
Codex review: PENDING FINAL REVIEW
```

The final documentation-only HEAD must pass the same CI contract and a final Codex review with no unresolved material findings.

After that final review gate:

```text
Phase 23.1 COMPLETE
Ready for Phase 23.2 — Diagnostics / Runtime Preflight
```
