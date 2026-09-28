# Phase 23.9 — Final Manual RC / Public Release Preparation

Phase 23.9 freezes Stream Archive 0.5.2 as the final release-candidate line and
prepares the evidence/checklist required before the first public release.

This phase does not create a Git tag, GitHub Release, public release asset,
registry publication, signing or notarization.

## 1. Baseline

- RC version: `0.5.2`
- Phase 23.8 PR: #103
- Phase 23.8 tested head:
  `c088a448dfb41795883d05dfcbdda156eae9862c`
- Phase 23.8 merge commit:
  `65d2876bc4692004417715e968bb649969428ccb`
- merge commit versus tested head: merge-only commit, no file differences
- runtime Cargo version: `0.5.2`
- GUI Cargo version: `0.5.2`
- Phase 23.8 final Stream Archive check #227: **PASS**
- Phase 23.8 Codex review: **no major issues**
- unresolved Phase 23.8 review threads: **0**
- Phase 23.9 branch: `phase23-9-final-manual-rc-release-prep`

The Phase 23.8 merge commit becomes the Phase 23.9 RC baseline. Any subsequent
product-code change invalidates affected manual evidence and requires full
automated RC revalidation.

## 2. Release freeze

Phase 23.9 is a release-preparation phase.

Allowed changes:

- release blocker fixes;
- release documentation corrections;
- checklist/evidence updates.

Not allowed:

- feature additions;
- UI redesign/polish;
- cleanup/refactoring;
- dependency tuning;
- DB/schema redesign;
- provider redesign;
- package-layout changes.

## 3. Canonical artifact matrix

| Platform | Architecture | Canonical artifact | Current status |
|---|---|---|---|
| Windows | x64 | `stream-archive-windows-x64.zip` | AUTOMATED RC PASS / FINAL ARTIFACT DRY RUN REQUIRED |
| Linux | x64 | `stream-archive-linux-x64.tar.gz` | AUTOMATED RC PASS / FINAL ARTIFACT DRY RUN REQUIRED |
| macOS | arm64 | `stream-archive-macos-arm64.tar.gz` | AUTOMATED RC PASS / FINAL ARTIFACT DRY RUN REQUIRED |

Each canonical release artifact must include:

- `RELEASE_INFO.txt`;
- package-local `SHA256SUMS.txt`;
- empty runtime `data/` state;
- no runtime SQLite/WAL/SHM;
- no logs/backups/downloads/process state;
- no credentials/cookies;
- no bundled Streamlink, yt-dlp or FFmpeg.

Each final archive must have its sibling archive-level `.sha256` file.

## 4. Automated RC validation

### Phase 23.8 implementation evidence

Stream Archive check #227 on
`c088a448dfb41795883d05dfcbdda156eae9862c`:

- Windows core: **PASS**
- Linux core: **PASS**
- macOS core: **PASS**
- Rust fmt/test/check/clippy: **PASS**
- Slint compile/test/clippy: **PASS**
- RuntimeContracts: **PASS**
- Architecture: **PASS**
- Providers: **PASS**
- ProviderE2E: **PASS**
- UnixCli: **PASS**
- ProcessLifecycle: **PASS**
- MediaProcess: **PASS**
- StorageOwnership: **PASS**
- Security: **PASS**
- ToolDiscovery: **PASS**
- Packaging: **PASS**
- ReleaseCandidate: **PASS**
- ReleaseSafety: **PASS**
- Unix RC package smoke: **PASS**
- Windows portable package verification: **PASS**
- backup/restore regression: **PASS**
- corrupted archive rejection: **PASS**
- runtime-data leakage rejection: **PASS**
- release metadata provenance: **PASS**

### Phase 23.9 branch-head validation

Stream Archive check #229 on documentation/release-preparation head
`8a6d55115c730c39c3954925118fde17707da02b`: **PASS**

Verified again:

- Windows core: **PASS**
- Linux core: **PASS**
- macOS core: **PASS**
- RuntimeContracts: **PASS**
- source release metadata smoke: **PASS**
- Windows portable package build/verification: **PASS**
- Windows offline backup/restore: **PASS**
- runtime-data release archive rejection: **PASS**
- corrupted Windows archive rejection: **PASS**
- final Windows release archive verification: **PASS**
- Linux/macOS RC package smoke and corrupted archive rejection: **PASS**

The evidence-only follow-up head was revalidated by Stream Archive check #230
on `c5acf510e683cffab99e67103ff624de697f20ad`: **PASS**.

The authoritative completion criterion remains that the final PR branch-head
Stream Archive check is green; this wording avoids changing release evidence
solely to chase a newer run number.

## 5. Windows Native UI manual RC

Status: **MANUAL TEST REQUIRED**

Use a fresh extraction of `stream-archive-windows-x64.zip`, not a development
checkout.

### Startup

- [ ] `StreamArchive.exe` launches
- [ ] no startup crash
- [ ] Native UI renders correctly
- [ ] no unexpected console window
- [ ] clean shutdown
- [ ] restart succeeds

### Settings

- [ ] Settings load
- [ ] output/download path change
- [ ] backup path change
- [ ] explicit Streamlink path
- [ ] explicit yt-dlp path
- [ ] explicit FFmpeg path
- [ ] save
- [ ] restart
- [ ] persistence confirmed

### Diagnostics

- [ ] refresh
- [ ] Streamlink discovery/probe
- [ ] yt-dlp discovery/probe
- [ ] FFmpeg discovery/probe
- [ ] invalid path produces understandable error
- [ ] missing tool produces understandable state
- [ ] valid path recovery refreshes correctly

### Paths

- [ ] ASCII path
- [ ] whitespace path
- [ ] Korean/non-ASCII path

Suggested test roots:

- `C:\Stream Archive RC\`
- `C:\테스트\Stream Archive\`

## 6. Windows headless manual RC

Status: **MANUAL TEST REQUIRED**

- [ ] fresh extracted package
- [ ] `RUN_HEADLESS.bat`
- [ ] runtime startup
- [ ] canonical SQLite initialization
- [ ] logs
- [ ] shutdown
- [ ] restart
- [ ] settings/data persistence
- [ ] same canonical data as Native UI
- [ ] concurrent ownership conflict is handled safely

## 7. Provider manual matrix

Real provider/session testing is intentionally outside credential-free CI.

| Provider path | Status |
|---|---|
| SOOP LIVE | **MANUAL TEST REQUIRED** |
| SOOP VOD | **MANUAL TEST REQUIRED** |
| CHZZK LIVE | **MANUAL TEST REQUIRED** |
| CHZZK VOD | **MANUAL TEST REQUIRED** |

### LIVE evidence checklist

For SOOP LIVE and CHZZK LIVE:

- [ ] provider discovery
- [ ] online/status lookup
- [ ] recording start
- [ ] output file creation
- [ ] recording progress
- [ ] cancellation
- [ ] owned process cleanup
- [ ] second recording can start
- [ ] normal stop/completion
- [ ] History record
- [ ] restart persistence

### VOD evidence checklist

For SOOP VOD and CHZZK VOD:

- [ ] URL/identifier discovery
- [ ] metadata/title
- [ ] output directory
- [ ] external tool invocation
- [ ] progress
- [ ] cancellation
- [ ] process cleanup
- [ ] retry
- [ ] completion
- [ ] output file
- [ ] Queue state
- [ ] History state
- [ ] restart persistence

Do not record credential, cookie or token values in this document, logs or PR
comments.

## 8. External media tools

Status: **MANUAL TEST REQUIRED**

Tools:

- Streamlink
- yt-dlp
- FFmpeg

Record only non-secret evidence:

| Tool | Version | Discovery source | Resolved path | Probe | Provider use |
|---|---|---|---|---|---|
| Streamlink | PENDING | PENDING | PENDING | PENDING | PENDING |
| yt-dlp | PENDING | PENDING | PENDING | PENDING | PENDING |
| FFmpeg | PENDING | PENDING | PENDING | PENDING | PENDING |

Manual checks:

- [ ] PATH discovery
- [ ] explicit configured path
- [ ] invalid path
- [ ] missing tool
- [ ] version probe
- [ ] whitespace path
- [ ] Unicode path
- [ ] timeout/cancel behavior where applicable

External tools remain external dependencies and are not bundled into official
release archives.

## 9. Fresh install

Status: **MANUAL TEST REQUIRED**

Start with no DB/settings/backups/logs.

- [ ] first launch
- [ ] directories initialized
- [ ] SQLite created
- [ ] defaults available
- [ ] diagnostics usable
- [ ] providers configurable
- [ ] media tools discovered
- [ ] shutdown
- [ ] restart
- [ ] settings persist
- [ ] no repository-relative runtime dependency

## 10. Cross-version upgrade

Status: **MANUAL TEST REQUIRED**

Required procedure:

1. stop the existing runtime;
2. create and verify a backup;
3. retain the previous package;
4. extract 0.5.2 RC separately;
5. reuse the existing canonical data directory;
6. launch the RC;
7. verify DB/settings/channels/History/Queue/Diagnostics;
8. stop;
9. restart;
10. confirm persistence.

Any unexpected data reset, data deletion or forced reinitialization is a
release blocker until understood and resolved.

## 11. Rollback

Status: **MANUAL TEST REQUIRED**

Tested claims must remain narrow.

- [ ] before first RC launch: previous package can still be used
- [ ] after RC launch: previous package behavior checked against the observed DB state
- [ ] verified pre-upgrade backup restoration procedure checked when needed

**Arbitrary schema downgrade compatibility is not guaranteed.**

## 12. Backup / restore manual RC

Status: **MANUAL TEST REQUIRED**

- [ ] create backup
- [ ] metadata present
- [ ] SHA-256 present/valid
- [ ] list backup
- [ ] mutate state
- [ ] restore
- [ ] Settings restored
- [ ] Channels restored
- [ ] History restored
- [ ] restart persists restored state
- [ ] invalid backup rejected
- [ ] corrupted backup rejected
- [ ] active runtime restore blocked
- [ ] whitespace path
- [ ] Unicode path

Automated backup/restore coverage is already green; this section is the final
real-use confirmation.

## 13. Secret storage manual RC

### Windows DPAPI

Status: **MANUAL TEST REQUIRED**

- [ ] secret write
- [ ] restart
- [ ] secret remains available through the application
- [ ] no plaintext SQLite/config/log leakage

### Linux Secret Service

Status: **MANUAL TEST REQUIRED / NOT TESTED if environment unavailable**

- [ ] `secret-tool` available
- [ ] usable Secret Service session
- [ ] secret write/read
- [ ] unavailable session fails closed

### macOS Keychain Services

Status: **MANUAL TEST REQUIRED / NOT TESTED if environment unavailable**

- [ ] Keychain write/read
- [ ] restart persistence
- [ ] failure path fails closed

## 14. Runtime-data leakage

Automated status: **PASS**

Final release-artifact confirmation remains required for the exact workflow
artifacts selected for publication.

Must not appear:

- SQLite DB/WAL/SHM
- logs
- lock/PID/socket files
- active claim/runtime state
- credentials/cookies
- downloaded media
- backup DBs
- fixture output
- user settings/config
- temporary files

## 15. Known issues

### KI-001

- Severity: P3
- Platform: Windows
- Area: Distribution trust
- Description: release artifact is not Authenticode signed.
- Workaround: verify the published SHA-256 checksum and source provenance.
- Release blocker: no, subject to the user's release decision.

### KI-002

- Severity: P3
- Platform: macOS
- Area: Distribution trust
- Description: release artifact is not code-signed/notarized.
- Workaround: verify the published SHA-256 checksum and source provenance.
- Release blocker: no, subject to the user's release decision.

### KI-003

- Severity: P3
- Platform: all
- Area: Installation
- Description: no MSI, deb/rpm, Homebrew, Snap/Flatpak/AppImage or service installer.
- Workaround: use the canonical portable ZIP/TAR.GZ package.
- Release blocker: no.

### KI-004

- Severity: P2 until real-provider RC completes
- Platform: all supported provider paths
- Area: Provider validation
- Description: credential-free CI validates provider/media behavior offline, but
  real SOOP/CHZZK sessions still require final manual RC.
- Workaround: complete the provider matrix before public release.
- Release blocker: **yes until required manual provider validation completes**.

### KI-005

- Severity: P2 until Native RC completes
- Platform: Windows
- Area: Native UI validation
- Description: automated compile/unit/adapter coverage is green, but final
  interaction testing of the extracted Native application is manual.
- Workaround: complete the Windows Native checklist.
- Release blocker: **yes until required manual Native validation completes**.

## 16. Release blockers

Current blockers:

1. Windows Native UI final manual RC not yet recorded.
2. Real SOOP LIVE/VOD validation not yet recorded.
3. Real CHZZK LIVE/VOD validation not yet recorded.
4. Representative cross-version upgrade/rollback not yet recorded.
5. Final real media-tool/secret-store environment checks remain where applicable.

No automated-validation blocker is currently known from Phase 23.8.

## 17. Non-blocking limitations

- unsigned Windows artifact;
- macOS signing/notarization not configured;
- no OS package-manager installer;
- no service installer;
- no claim of arbitrary DB schema downgrade compatibility.

## 18. Signing / notarization

| Platform | Status |
|---|---|
| Windows Authenticode | **NOT CONFIGURED** |
| macOS code signing | **NOT CONFIGURED** |
| macOS notarization | **NOT CONFIGURED** |
| Linux package signing | **NOT CONFIGURED** |

Phase 23.9 does not perform signing/notarization automatically.

## 19. Release notes

Release notes file:

`docs/RELEASE_NOTES_0_5_2.md`

Status: **FINAL RC DRAFT — PUBLICATION PENDING MANUAL RC**

The notes must not claim completed real-provider or Native UI validation until
that evidence is supplied.

## 20. Public release dry run

Expected tag:

`v0.5.2`

Expected release title:

`Stream Archive 0.5.2`

The repository currently has no existing `v0.5.2` tag or release.

Expected assets from the canonical release workflow:

- `stream-archive-windows-x64.zip`
- `stream-archive-windows-x64.zip.sha256`
- `stream-archive-linux-x64.tar.gz`
- `stream-archive-linux-x64.tar.gz.sha256`
- `stream-archive-macos-arm64.tar.gz`
- `stream-archive-macos-arm64.tar.gz.sha256`

The release workflow remains manual, read-only with respect to repository
contents, and uploads verified GitHub Actions artifacts only. It does not create
a tag or GitHub Release.

## 21. Manual evidence format

When manual results are supplied, record:

```text
Area:
Result: PASS / FAIL / BLOCKED / NOT TESTED
Environment:
Tool versions:
Expected:
Actual:
Notes:
```

Never include credential/cookie/token values.

## 22. Severity / release decision

- P0: data loss, security/privacy exposure, release integrity failure.
- P1: core runtime/package/provider path unusable.
- P2: important correctness/edge case.
- P3: non-blocking UX/documentation/distribution limitation.

P0/P1 are release blockers. P2 is evaluated by impact; the required manual RC
items above are treated as blockers until their evidence exists.

## 23. Final readiness

```text
Automated RC validation:
PASS (Phase 23.8 #227, Phase 23.9 #229 and #230)

Phase 23.9 final PR branch-head CI:
MUST BE GREEN (authoritative PR check status)

Manual RC validation:
REMAINING

Release blockers:
Windows Native UI, real provider sessions, representative upgrade/rollback,
and remaining real-environment tool/secret checks.

Public release readiness:
MANUAL VALIDATION REMAINING
```

No public tag/Release should be created from this state.
