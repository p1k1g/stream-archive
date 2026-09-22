# Phase 23.2 — Diagnostics / Runtime Preflight

## 1. Goal

Phase 23.2 turns the existing read-only diagnostics into one shared runtime preflight contract consumed by both Windows Native Diagnostics and the Unix/headless `stream-archive-cli doctor`.

The architecture remains:

```text
Windows Slint Native GUI ─┐
Unix CLI/headless ─────────┼──> StreamArchiveCore / shared Rust runtime
Headless runtime binary ───┘
                           │
                           └── shared DiagnosticsSnapshot
```

This phase is local/runtime readiness only. It does not perform provider network requests, login verification, media downloads or automatic repair.

## 2. Phase 23.1 baseline

Phase 23.1 closed Windows Native daily-use UX and established content-sized scroll-list rows with start alignment.

Phase 23.2 preserves that layout contract while enriching the Diagnostics presentation.

## 3. Previous diagnostics split

Before Phase 23.2:

- Windows Native used `StreamArchiveCore::diagnostics()`.
- `stream-archive-cli doctor` implemented separate backend/tool/secret checks.
- Warning and runtime usability were coupled through `runtime_ready = status == OK`.
- Diagnostic rows had presentation strings but no stable machine identity/category/requirement/remediation contract.

Phase 23.2 removes the CLI decision duplication and makes the shared snapshot authoritative.

## 4. Shared preflight model

The existing `DiagnosticItem` / `DiagnosticsSnapshot` model is extended rather than replaced.

Each check now carries:

```text
id
category
requirement
name
status
summary
detail
remediation
```

The snapshot adds:

```text
status
runtime_ready
attention_required
summary.ok
summary.warning
summary.error
summary.blocking_errors
items
```

## 5. Stable IDs and categories

Stable IDs include:

```text
runtime.backend
runtime.settings
runtime.bundled_vod_tools
runtime.resources

database.primary
database.integrity

storage.data
storage.live_output
storage.backup

tool.streamlink
tool.ytdlp
tool.ffmpeg

secret.native_store

provider.soop
provider.chzzk

backup.policy
```

Categories are:

```text
Runtime
Database
Storage
Tools
Secrets
Providers
Backup
```

UI labels are not used as programmatic identity.

## 6. Requirement semantics

Checks distinguish:

```text
Required
Optional
Informational
```

A required Error is blocking.

An Optional Error/Warning or Informational condition can require attention without making the base runtime unusable.

The optional bundled VOD tools directory remains informational. Tool discovery can still succeed through configured paths, PATH or common install locations.

## 7. Runtime usability

`runtime_ready` no longer means "there are zero warnings".

It now means:

```text
blocking required Error count == 0
```

The snapshot separately records `attention_required`.

This allows examples such as missing provider credentials or an optional backup path to be visible without incorrectly reporting the whole runtime as unusable.

Missing required media executables remain blocking to preserve the previous CLI doctor expectation for a fully media-capable runtime.

## 8. SQLite checks

The preflight checks:

- canonical DB file presence;
- regular-file expectation;
- read-only SQLite open;
- read-only `PRAGMA quick_check`.

Diagnostics do not:

- create the DB;
- migrate schema;
- run VACUUM;
- run REINDEX;
- repair the DB;
- modify settings.

An invalid/non-SQLite file produces a required Error.

## 9. Storage checks

The preflight covers:

- canonical backend;
- data directory;
- configured LIVE output directory;
- backup directory.

No writable temp-file probe is performed in Phase 23.2. This avoids changing the previously read-only diagnostics boundary. Write-path execution belongs to explicit runtime/media integration coverage.

Diagnostics never create a missing output/backup directory.

## 10. Media-tool checks

Streamlink, yt-dlp and FFmpeg use the existing `tool_discovery.rs` resolution contract.

Discovery remains filesystem-only in Phase 23.2.

The preflight does not spawn:

```text
streamlink --version
yt-dlp --version
ffmpeg -version
```

because bounded subprocess spawn/timeout/cancel/tree-cleanup behavior is intentionally handled in Phase 23.3 Media-tool Integration Harness.

A discovered tool with a fallback warning is Warning. A missing required tool is Error.

## 11. Native secret store

The shared model reports capability only:

- Windows: CurrentUser DPAPI boundary;
- macOS: Keychain boundary;
- Linux: `secret-tool` discovery with session explicitly not probed.

No secret value is read into the presentation model and no plaintext fallback is introduced.

## 12. Provider configuration readiness

SOOP and CHZZK checks are local configuration checks only.

SOOP readiness uses configured/not-configured state for:

- username;
- SOOP password;
- Worker URL;
- Worker API key.

CHZZK readiness uses configured/not-configured state for:

- NID_AUT;
- NID_SES.

The snapshot contains only `configured` / `not configured`; credential values are never serialized.

Missing provider credentials are Optional warnings and do not block the base runtime.

## 13. Backup checks

The shared preflight includes:

- backup directory;
- automatic backup policy enabled state;
- interval;
- keep count;
- retention.

It does not create a backup or mutate the policy.

The CLI resolves the environment override, configured backup directory or existing default resolution without creating it.

## 14. Native Diagnostics

Settings -> Manage -> Diagnostics now renders:

- runtime usable / blocking summary;
- OK / Warning / Error counts;
- blocking error count;
- category;
- requirement;
- status;
- summary;
- detail;
- remediation.

Status remains textual as well as color-coded.

The Phase 23.1 `alignment: start` layout remains intact.

## 15. CLI doctor migration

`stream-archive-cli doctor` now uses the same `DiagnosticsSnapshot` construction as Native Diagnostics rather than its previous independent tool/path/secret decision tree.

Human-readable output includes each check and summary.

## 16. JSON and exit-code contract

Machine-readable output:

```bash
stream-archive-cli doctor --json
```

serializes the shared snapshot.

Exit semantics:

```text
0        no blocking required Error
non-zero one or more blocking required Errors
```

Warnings alone do not produce a failure exit status.

## 17. Security boundary

The preflight never serializes provider credential values.

Hidden settings are converted to boolean configured flags before they reach the diagnostics result.

No provider login/API/network probe runs in this phase.

## 18. Passive vs active probes

Phase 23.2 keeps the shared preflight passive/read-only:

```text
filesystem discovery
settings/configured-state inspection
read-only SQLite quick_check
native secret-store capability discovery
```

Not included:

```text
external executable subprocess version probe
writable temp-file probe
provider network request
media URL access
download/transcode
```

The executable/process lifecycle portion is a Phase 23.3 concern.

## 19. Tests

Regression coverage includes:

- warning-only snapshot remains runtime usable;
- required Error blocks runtime;
- summary/blocking counts;
- informational bundled-tools directory;
- stable tool check IDs;
- missing/discovered/fallback tool states;
- valid SQLite quick_check;
- invalid SQLite file;
- provider diagnostics contain no secret/config values;
- backup diagnostics remain non-creating/read-only;
- CLI warning-only exit semantics;
- CLI required-error exit semantics;
- CLI secret loading returns configured booleans instead of secret values.

## 20. Runtime contracts

Existing runtime contract guards remain authoritative.

Phase 23.2 does not reintroduce:

- browser/Web UI;
- localhost application API;
- direct Slint SQLite ownership;
- global process-name termination;
- plaintext secret fallback.

## 21. Known limitations

Phase 23.2 intentionally does not prove that a discovered executable can successfully run.

Linux Secret Service session usability is not mutated/probed; `secret-tool` availability is reported while session state remains explicitly unprobed.

Provider readiness represents local configuration completeness, not authenticated network validity.

## 22. Manual QA

Windows portable:

```text
Settings -> Manage -> Diagnostics
```

Check:

- runtime usable summary;
- OK/Warning/Error counts;
- category and Required/Optional/Informational labels;
- long detail/remediation wrapping;
- missing tool state;
- provider credential warnings;
- maximum/minimum supported window sizes;
- no Phase 23.1 row expansion regression.

Unix/headless:

```bash
stream-archive-cli doctor
stream-archive-cli doctor --json
```

Validate human/JSON views use the same shared snapshot and that no secret value appears.

## 23. Phase 23.3 handoff

Phase 23.3 should add a deterministic Media-tool Integration Harness for:

```text
spawn
bounded stdout/stderr
version probe
timeout
cancel
owned-process cleanup
exit-code mapping
path-with-spaces / Unicode paths
```

using fixtures/fake executables rather than provider network downloads.

Provider network/E2E remains Phase 23.4.

## 24. Validation closure

GitHub Actions run:

```text
35790143183
```

Results:

```text
core-check (linux)   PASS
core-check (macos)   PASS
core-check (windows) PASS
windows-check        PASS

Runtime contract guard          PASS
Source archive metadata smoke   PASS
Windows portable package smoke PASS
Verify portable package        PASS
```

The runtime contract guard was updated for the Phase 23.2 shared-preflight contract rather than weakening it. It now also protects the requirement model, stable IDs, read-only SQLite integrity check, blocking-error readiness semantics, Native/CLI shared model, and local/no-secret-value preflight boundary.

Final Codex review is requested only after this closure commit reaches a green final HEAD.

## 25. Status

```text
Phase 23.2 COMPLETE
Ready for Phase 23.3 — Media-tool Integration Harness
```
