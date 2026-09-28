# Phase 23.8 — Release Cleanup / Code Reduction

Phase 23.8 reduces duplicated production code while preserving the Phase 23.7
Release Candidate behavior and contracts. It does not add features, change the
database schema, redesign the Native UI, alter provider semantics, change CLI
syntax, change package layout, or publish a release/tag.

## Phase 23.7 baseline

- Phase 23.7 PR: #102
- merged main commit: `9028665020430d85c9f83ee93f2c1d60f28f9ca8`
- tested Phase 23.7 head: `17be1a939174490bd431f9bd28ec2db08546096e`
- Stream Archive check #221: **PASS**
- merge commit vs tested head: merge-only commit, no file differences
- runtime version: `0.5.2`
- GUI version: `0.5.2`
- Phase 23.8 branch: `phase23-8-release-cleanup-code-reduction`

## Scope

The cleanup is deliberately conservative:

- shared media-tool configured resolution;
- duplicate GUI byte formatting;
- private one-call bootstrap wrapper removal;
- regression guard/test coverage for the shared helper.

No provider API/auth/request logic, SQLite schema, process ownership, backup
format, package layout, CLI contract, Settings key, or Slint information
architecture is changed.

## Before metrics

Metrics were counted from the Phase 23.7 merged main tree. Rust production/test
LOC are source-line counts with dedicated test files and `#[cfg(test)] mod ...`
blocks separated.

| Metric | Before |
|---|---:|
| Rust total LOC | 25,951 |
| Rust production LOC | 20,005 |
| Rust test LOC | 5,946 |
| Slint LOC | 2,376 |
| Maintenance/workflow LOC | 2,894 |
| Direct runtime production dependencies | 14 |
| Runtime dev dependencies | 1 |
| Direct GUI production dependencies | 4 |
| GUI build dependencies | 1 |

### Artifact size baseline

Phase 23.7 Stream Archive check #221 did not retain workflow artifacts. Therefore
reliable baseline sizes for `StreamArchive.exe`, `stream-archive-server.exe`,
`stream-archive-cli`, Windows ZIP, Linux TAR.GZ and macOS TAR.GZ are
**NOT MEASURED** rather than reconstructed from an unrelated build.

## Dead code / wrapper audit

Rust `clippy -D warnings` already prevents ordinary private unused code from
remaining unnoticed. The audit therefore concentrated on redundant private
wrappers and duplicated implementation.

Removed:

- private bootstrap `doctor_snapshot()`, which only forwarded to
  `collect_read_only_preflight()`;
- two separate private `resolve_all()` media-tool resolution implementations
  in bootstrap CLI and daily-use Unix CLI.

Retained intentionally:

- the bounded `soop.db -> stream-archive.db` compatibility migration;
- public/shared facade methods that form the StreamArchiveCore architecture
  boundary even when their implementation is thin;
- the public legacy-friendly `support::resolve_channel_name(account)` helper.
  It has no current in-tree call site, but removing a public/shared boundary
  immediately before the first release would violate the Phase 23.8 no-public-
  API-redesign rule.

## Dependency audit

All direct runtime dependencies have active source references:

- anyhow
- base64
- chrono
- fs2
- regex
- reqwest
- rusqlite
- serde / serde_json
- sha2
- tokio
- url
- uuid
- windows-sys

The runtime `tempfile` dependency remains test-only.

GUI dependencies remain actively used:

- slint
- stream-archive-server
- tokio
- windows
- slint-build (build dependency)

No dependency was removed. Aggressive `default-features = false` tuning was
not attempted because the current dependencies are active and the release is
already at RC stage.

## Duplicate consolidation

### Shared media-tool resolution

Before Phase 23.8, the same sequence existed in multiple presentation/service
paths:

1. iterate `ToolKind::ALL`;
2. expand each `setting_keys()`;
3. read configured values from a `BTreeMap`;
4. call `resolve_tool()`;
5. collect the resolutions.

Phase 23.8 moves that infrastructure-only operation to
`tool_discovery::resolve_all_tools()`.

Consumers now reuse the shared helper:

- bootstrap `stream-archive-cli tools`;
- daily-use Unix CLI status/diagnostics;
- passive Diagnostics tool checks;
- active Diagnostics tool probes.

Discovery order and setting keys are unchanged.

A direct unit regression preserves Streamlink -> yt-dlp -> FFmpeg kind order,
and the existing ToolDiscovery contract prevents presentation CLIs from
reintroducing their own `fn resolve_all(...)`.

### Shared GUI byte formatter

`history_adapter` and `maintenance_adapter` had byte-for-byte identical
KiB/MiB/GiB formatting implementations. They now use
`rust-gui/src/formatting.rs`.

The following intentionally remain separate because their presentation
semantics differ:

- LIVE recording size uses KB/MB/GB labels;
- storage capacity uses `format_bytes_compact` including TiB and different
  precision;
- VOD and History duration formatters use different zero/padding rules.

## Provider-neutral cleanup

No provider process lifecycle code was changed.

SOOP/CHZZK VOD capture helpers look structurally similar but have different
timeout/logging/merge semantics and are strongly covered by process-ownership
guards. Consolidating those immediately before first release would be a
medium-risk abstraction change with little proven reduction benefit.

Status: **DEFERRED**

## CLI / shared cleanup

Completed:

- removed duplicate CLI-local tool resolver;
- removed one private pass-through doctor wrapper;
- retained all command names, argument forms, JSON schema and exit semantics.

The daily-use CLI continues to use StreamArchiveCore. No second persistence or
configuration authority was introduced.

## Packaging / maintenance cleanup

No builder/verifier responsibility was merged or rearranged.

The only maintenance change is an extension of the existing ToolDiscovery
guard so the new shared resolver remains the source of truth.

Windows/Unix canonical builders, package verifiers, release metadata and
checksum contracts remain unchanged.

## Explicitly deferred refactors

The following were reviewed and intentionally not performed:

- UI navigation/layout/visual redesign;
- Dashboard/History/Settings restructuring;
- provider-specific auth/API/parser abstraction;
- SOOP/CHZZK VOD process orchestration redesign;
- async runtime redesign;
- StreamArchiveCore facade removal;
- SQLite schema/migration changes;
- backup/restore format or semantics changes;
- packaging layout changes;
- aggressive Cargo feature trimming;
- maintenance backup/restore common-file extraction that would add a new
  packaged script dependency.

## After metrics

Current Phase 23.8 source metrics:

| Metric | After | Delta |
|---|---:|---:|
| Rust total LOC | 25,934 | -17 |
| Rust production LOC | 19,967 | **-38** |
| Rust test LOC | 5,967 | +21 |
| Slint LOC | 2,376 | 0 |
| Maintenance/workflow LOC | 2,901 | +7 |
| Direct runtime production dependencies | 14 | 0 |
| Runtime dev dependencies | 1 | 0 |
| Direct GUI production dependencies | 4 | 0 |
| GUI build dependencies | 1 | 0 |

The goal was not LOC reduction by itself. The useful signal is that production
duplication decreased while regression/test protection increased.

### Artifact size after

**NOT MEASURED** until a retained comparable release-artifact build exists.
Phase 23.8 does not invent a before/after artifact-size claim from non-comparable
builds.

## Regression results

Status: **PASS** on Stream Archive check #226 for cleanup implementation head
`bf69f58729aebedd67209c30bb54d5dcf663aff7`.

Verified:

- Windows core: **PASS**
- Linux core: **PASS**
- macOS core: **PASS**
- Rust fmt/test/check/clippy: **PASS**
- Slint compile/test/clippy: **PASS**
- Runtime contracts including ProviderE2E, ProcessLifecycle, MediaProcess,
  Security, ToolDiscovery, ReleaseCandidate and ReleaseSafety: **PASS**
- Unix CLI integration and source-archive metadata regression: **PASS**
- Linux/macOS RC archive smoke: **PASS**
- Linux/macOS backup/restore, replacement extraction and persisted data reuse:
  **PASS**
- corrupted Unix archive rejection: **PASS**
- Windows portable build/verification: **PASS**
- Windows offline backup/restore including corrupted-backup rejection: **PASS**
- runtime-data archive rejection: **PASS**
- corrupted Windows ZIP rejection: **PASS**
- final Windows archive verification: **PASS**

## Known issues

No new known runtime issue is introduced by the cleanup.

Existing release constraints remain:

- unsigned Windows artifact;
- macOS signing/notarization not configured;
- no package-manager/service-manager installer;
- real SOOP/CHZZK sessions are outside credential-free CI.

## Manual RC status

**MANUAL TEST REQUIRED AFTER PHASE 23.8 MERGE**

The final manual RC remains:

- Windows Native UI;
- Settings;
- Diagnostics;
- SOOP LIVE;
- SOOP VOD;
- CHZZK LIVE;
- CHZZK VOD;
- representative cross-version upgrade;
- rollback;
- real installed media tools;
- Linux/macOS native secret store where applicable.

## Final cleanup summary

Phase 23.8 deliberately stops at low-risk, behavior-preserving reductions.
Production Rust is smaller, duplicated tool discovery and GUI formatting are
centralized, and test/guard coverage is stronger.

High-risk architecture/UI/provider changes are deferred until after the first
public release.
