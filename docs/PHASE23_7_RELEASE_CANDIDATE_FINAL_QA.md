# Phase 23.7 — Release Candidate / Final QA

Phase 23.7 validates the Phase 23.6 portable artifacts as release candidates.
It is a verification phase, not a feature-development phase. It does not create
a Git tag, publish a GitHub Release, upload public release assets, publish a
package registry artifact, or perform signing/notarization.

## Baseline

- Phase 23.6 PR: #101
- Phase 23.6 merged main commit: `c62bd1259a1cc00c32ca53966f9363b044f3f6c4`
- Phase 23.6 tested PR head: `2d5babde185a8a8ce82e3786e3d46d9b3af078be`
- Phase 23.6 final PR CI: Stream Archive check #214 — PASS
- Phase 23.6 merge commit vs tested head: merge-only commit, no file differences
- RC version: `0.5.2`
- Runtime/GUI version parity: `0.5.2 == 0.5.2`
- Phase 23.7 branch: `phase23-7-release-candidate-final-qa`

The final Phase 23.7 tested implementation commit and CI run are recorded in the
CI evidence section after the branch validation completes.

## Verified platform / artifact matrix

Only targets built natively by the retained GitHub Actions matrix are advertised.

| Platform | Native CI target | RC artifact |
|---|---|---|
| Windows | x64 / MSVC | `stream-archive-windows-x64.zip` |
| Linux | x64 / GNU | `stream-archive-linux-x64.tar.gz` |
| macOS | ARM64 / Apple Darwin | `stream-archive-macos-arm64.tar.gz` |

No additional architecture is claimed by Phase 23.7.

## Artifact contract

Every RC artifact must retain:

- archive-level SHA-256 sibling checksum;
- package-local `SHA256SUMS.txt`;
- `RELEASE_INFO.txt` with product/version/commit/built_at;
- clean shipped `data/`;
- no SQLite runtime database, log, claim sidecar, downloaded media, backup data,
  provider credential, or test state;
- no bundled Streamlink, yt-dlp, or FFmpeg;
- canonical license and third-party notices.

The PR CI deliberately corrupts copies of both Unix and Windows archives and
requires the package verifier to reject the checksum mismatch.

## Automated RC validation

Status: **PENDING** until the Phase 23.7 PR workflow completes.

The automated RC gate includes the retained Phase 23.6 coverage plus the
following Phase 23.7 artifact-level checks:

- Linux/macOS TAR.GZ checksum and fresh extraction;
- Linux/macOS package-local checksum validation;
- packaged CLI `version`, `help`, `init`, and `status --json`;
- whitespace and non-ASCII extraction/runtime paths;
- package-external fresh runtime data initialization;
- persisted settings and channel state;
- managed backup creation from the extracted package;
- state mutation followed by managed backup restore;
- a second fresh extraction (replacement package) reusing the preserved
  external runtime data and confirming settings/channel persistence;
- corrupted Unix archive rejection;
- Windows ZIP tree/checksum/fresh-extraction verification;
- Windows official empty-data enforcement;
- packaged Windows offline backup creation plus metadata;
- packaged Windows offline restore and byte-for-byte restored backup hash;
- packaged Windows restore rejection after backup/metadata SHA-256 mismatch;
- corrupted Windows ZIP rejection;
- Phase 23.6 release-metadata provenance regressions;
- provider offline E2E for SOOP LIVE/VOD and CHZZK LIVE/VOD;
- owned-process cancellation/unrelated-process protection;
- BackupManager real-SQLite round-trip unit regression;
- security, native-secret and release-safety contracts.

### Fresh install result

- Linux: **PENDING CI** — extracted artifact initializes an external fresh
  backend/data/backup layout.
- macOS: **PENDING CI** — same artifact-level flow as Linux.
- Windows package structure/checksum/fresh extraction: **PENDING CI**.
- Windows Native GUI first interactive startup: **MANUAL TEST REQUIRED**.

The Unix smoke intentionally keeps mutable runtime data outside the extracted
package so official package cleanliness remains testable after initialization.

### Upgrade result

Phase 23.7 automatically verifies **package replacement with preserved external
data** on Linux/macOS by extracting a second copy of the same RC archive and
opening the data produced by the first extraction.

A true **cross-version upgrade** from a prior independently built/published
version is **MANUAL TEST REQUIRED**. The CI repository does not synthesize an
older release artifact and does not claim that same-version replacement proves
schema migration compatibility.

Required manual cross-version sequence:

1. retain the previous package and create a verified backup;
2. stop the active runtime cleanly;
3. extract the RC into a separate directory;
4. reuse the canonical existing data directory;
5. verify settings/channels/history/queue and diagnostics;
6. restart once more and verify persistence.

### Rollback result

- Package rollback before first RC startup: documented and expected.
- Package rollback after RC startup with no incompatible schema change:
  **MANUAL TEST REQUIRED**.
- Database restore from a verified pre-upgrade backup: automated backup/restore
  primitives are covered, but the complete cross-version rollback procedure is
  **MANUAL TEST REQUIRED**.
- Arbitrary schema downgrade compatibility is **NOT GUARANTEED** and is not
  claimed by this phase.

## Backup / restore result

Automated coverage includes:

- real SQLite BackupManager round-trip and safety copy in Rust tests;
- retention ownership rules and policy validation;
- Unix packaged CLI managed backup creation and restore;
- restore refusal while another runtime owns the database;
- Windows packaged offline backup script output and metadata;
- Windows packaged offline restore;
- Windows SHA-256 mismatch rejection for a corrupted backup.

Manual Native UI backup/restore interaction remains **MANUAL TEST REQUIRED**.

## Provider regression matrix

| Provider path | Offline automated regression | Real provider/session RC |
|---|---|---|
| SOOP LIVE | PENDING CI | MANUAL TEST REQUIRED |
| SOOP VOD | PENDING CI | MANUAL TEST REQUIRED |
| CHZZK LIVE | PENDING CI | MANUAL TEST REQUIRED |
| CHZZK VOD | PENDING CI | MANUAL TEST REQUIRED |

Offline provider tests use fake local media executables and do not require
provider credentials or Internet access. They retain invocation, timeout,
failure mapping, cancellation, Unicode path, descendant ownership and unrelated
process protection.

Real-session checks must verify discovery, start/download, progress, cancel,
clean completion and output files without committing credentials or cookies.

## Media tool matrix

Streamlink, yt-dlp and FFmpeg remain external dependencies.

Automated contracts cover:

- configured-path priority;
- bundled-layout discovery support without redistributing the executables;
- PATH/common-location discovery;
- invalid explicit path handling;
- fake executable invocation;
- failure/timeout/cancellation behavior;
- Unicode/whitespace paths;
- package rejection if media-tool executable names are bundled.

Real installed-tool version probing through Diagnostics/`doctor --active-tools`
is **MANUAL TEST REQUIRED** on the RC host.

## Release metadata

The retained metadata contract is:

~~~text
product=Stream Archive
version=<version>
commit=<commit|commit-dirty|unknown>
built_at=<timestamp>
~~~

Phase 23.6 regressions remain mandatory:

- clean checkout -> commit;
- dirty checkout -> `<commit>-dirty`;
- source archive without Git -> `unknown`;
- failed `git status` -> `unknown`;
- linked worktree -> linked worktree commit;
- dirty linked worktree -> `<commit>-dirty`;
- dirty GitHub Actions checkout -> `<commit>-dirty`.

## Checksum verification

Positive verification:

- package-local executable checksums;
- archive-level SHA-256;
- fresh extraction after archive checksum validation.

Negative verification:

- corrupted Unix TAR.GZ must be rejected;
- corrupted Windows ZIP must be rejected;
- Windows offline restore must reject backup content that no longer matches its
  metadata SHA-256.

## Security / secret verification

Automated contracts retain:

- Windows DPAPI;
- Linux Secret Service / `secret-tool` boundary;
- macOS Keychain Services boundary;
- native-secret reference rather than plaintext fallback;
- stdin-only Unix secret writes;
- provider status redaction;
- private temporary directory permissions;
- current-tree and reachable-history release-safety scans.

A real Linux Secret Service session and a real macOS Keychain credential
round-trip are **MANUAL TEST REQUIRED**. CI does not inject real provider
credentials.

## Manual RC validation

Status: **REMAINING**

### Windows Native UI

- [ ] MANUAL TEST REQUIRED — launch extracted `StreamArchive.exe`.
- [ ] MANUAL TEST REQUIRED — Settings load and persist.
- [ ] MANUAL TEST REQUIRED — Diagnostics refresh and tool errors.
- [ ] MANUAL TEST REQUIRED — VOD download directory selection.
- [ ] MANUAL TEST REQUIRED — optional bundled-tools directory behavior.
- [ ] MANUAL TEST REQUIRED — backup directory and Native Backup/Restore.
- [ ] MANUAL TEST REQUIRED — close and restart.
- [ ] MANUAL TEST REQUIRED — whitespace/non-ASCII install path.
- [ ] MANUAL TEST REQUIRED — read-only/permission error presentation.
- [ ] MANUAL TEST REQUIRED — missing external tools produce actionable diagnostics.

### Windows headless

- [ ] MANUAL TEST REQUIRED — run extracted `RUN_HEADLESS.bat`.
- [ ] MANUAL TEST REQUIRED — clean shutdown and restart with preserved data.

### Real provider sessions

For each SOOP LIVE, SOOP VOD, CHZZK LIVE and CHZZK VOD:

- [ ] MANUAL TEST REQUIRED — URL/account discovery.
- [ ] MANUAL TEST REQUIRED — media tool invocation.
- [ ] MANUAL TEST REQUIRED — start/download progress.
- [ ] MANUAL TEST REQUIRED — cancellation.
- [ ] MANUAL TEST REQUIRED — clean terminal state.
- [ ] MANUAL TEST REQUIRED — expected output file.

### Cross-version upgrade / rollback

- [ ] MANUAL TEST REQUIRED — previous package + pre-upgrade backup.
- [ ] MANUAL TEST REQUIRED — RC package with preserved existing data.
- [ ] MANUAL TEST REQUIRED — settings/channels/history/queue verification.
- [ ] MANUAL TEST REQUIRED — restart persistence.
- [ ] MANUAL TEST REQUIRED — package rollback in a schema-compatible case.
- [ ] MANUAL TEST REQUIRED — backup restore when rollback requires database recovery.

## Known issues / non-blocking limitations

- Artifacts are checksum-verified but unsigned.
- Windows Authenticode signing is not configured.
- macOS code signing/notarization is not configured.
- No MSI, deb/rpm, Homebrew, Snap/Flatpak/AppImage package is provided.
- No systemd/launchd installer is provided.
- Linux secret writes require `secret-tool` and a usable Secret Service session.
- Real provider APIs/sessions remain outside credential-free CI.
- Windows Native GUI interaction cannot be fully validated by headless CI.

These are known release constraints, not hidden PASS claims.

## Release blockers

For merging the Phase 23.7 engineering PR, automated CI must be green and no
P0/P1 release-correctness defect may remain.

For a **public release**, the following remain blockers until completed:

1. Windows Native UI extracted-package smoke;
2. real SOOP LIVE/VOD session smoke;
3. real CHZZK LIVE/VOD session smoke;
4. one representative cross-version upgrade/rollback test;
5. final review of unsigned/notarization expectations.

## Signing / notarization status

- Windows Authenticode: **NOT CONFIGURED**
- macOS code signing: **NOT CONFIGURED**
- macOS notarization: **NOT CONFIGURED**
- Linux package signing: **NOT CONFIGURED**

No bypass guidance is added.

## Release workflow boundary

`.github/workflows/rust-runtime-release.yml` remains an RC artifact collection
workflow only:

- manual `workflow_dispatch`;
- `contents: read`;
- native Windows/Linux/macOS runners;
- canonical builders/verifiers;
- Actions artifacts with bounded retention;
- no tag;
- no GitHub Release;
- no public release upload;
- no package-registry publish.

## CI evidence

Phase 23.6 baseline:

- Stream Archive check #214 on `2d5babde185a8a8ce82e3786e3d46d9b3af078be`: **PASS**
- merge commit `c62bd1259a1cc00c32ca53966f9363b044f3f6c4` adds no file changes over that tested head.

Phase 23.7:

- Tested implementation commit: **PENDING**
- Stream Archive check: **PENDING**
- Windows core: **PENDING**
- Linux core: **PENDING**
- macOS core: **PENDING**
- final Windows package/archive job: **PENDING**

## Final RC readiness summary

### Automated RC validation

**PENDING**

### Manual RC validation

**REMAINING**

### Public release readiness

**NOT READY FOR PUBLIC RELEASE YET.**

This status is intentionally conservative. The automated artifact and contract
suite must pass first, and the manual extracted-Windows/provider/cross-version
checks above must be completed before the user makes the public release/tag
decision.
