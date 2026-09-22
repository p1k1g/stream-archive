# Phase 22.7 — Final Legacy / Compatibility Audit & Phase 22 Closure

## 1. Scope

Phase 22.7 is the final audit for Phase 22. It does not add product features.

The audit closes the sequence that:

1. audited legacy dependencies;
2. removed warning debt;
3. removed the browser/Axum presentation layer;
4. audited runtime/core compatibility boundaries;
5. renamed the shared runtime source path from `rust-web/` to `rust-runtime/`;
6. renamed the runtime workflows from `rust-web-*.yml` to `rust-runtime-*.yml`;
7. verifies that no active Web-era runtime dependency remains outside explicit compatibility cleanup or negative guards.

Baseline:

```text
main merge commit after Phase 22.6:
91ad4a802c5a88d64393cf08af5340374a4c142e
```

## 2. Current architecture

The supported architecture remains:

```text
Windows Slint Native GUI ─┐
Unix CLI/headless ─────────┼──> StreamArchiveCore / shared Rust runtime
Headless runtime binary ───┘             │
                                         ├─ SQLite
                                         ├─ LIVE/VOD services
                                         ├─ SOOP/CHZZK providers
                                         └─ Streamlink / yt-dlp / FFmpeg
```

Canonical repository identities:

```text
runtime source:            rust-runtime/
PR/runtime workflow:       .github/workflows/rust-runtime-check.yml
manual release workflow:   .github/workflows/rust-runtime-release.yml
Windows native executable: StreamArchive.exe
```

The Slint GUI calls `StreamArchiveCore`; it does not use localhost HTTP as an application API and does not own SQLite or provider/process implementation directly.

## 3. Phase 22.1–22.7 summary

### Phase 22.1 — Legacy dependency audit

Established the inventory for the browser/Web presentation layer, legacy runtime names, dependencies, packaging and compatibility surfaces.

### Phase 22.2 — Warning cleanup

Made project warnings actionable under strict Clippy validation without deleting runtime contracts merely to satisfy the compiler.

### Phase 22.3 — Web removal

Removed browser static presentation, Axum application routes, browser launcher/fallback packaging and related Web-only direct dependencies.

### Phase 22.4 — Runtime/core compatibility audit

Separated dead compatibility shims from boundaries still consumed by Slint, CLI/headless operation, storage and process ownership.

### Phase 22.5 — Runtime path cleanup

Renamed the shared source directory from `rust-web/` to `rust-runtime/` while deliberately retaining the externally meaningful `stream-archive-server` package/binary identity.

### Phase 22.6 — Runtime workflow naming cleanup

Renamed the current CI/release workflow files to:

```text
rust-runtime-check.yml
rust-runtime-release.yml
```

and migrated current references/guards.

### Phase 22.7 — Final audit

Re-audited active code, scripts, current documentation, guards, packaging and compatibility names. Two stale active remnants were corrected:

- CHZZK VOD new temporary jobs no longer use `backend/.rust-web/vod`;
- backup/restore stop-runtime errors no longer call the retained headless runtime a "Web compatibility server".

## 4. Legacy Web search classification

The final audit searched for Web-era names and concepts including:

```text
rust-web
RUN_WEB
RUN_SERVER_CONSOLE
stream-archive-launcher
axum
tower-http
tokio-stream
tracing-subscriber
localhost
127.0.0.1
Caddyfile
REVERSE_PROXY
LOCAL_LAUNCHER
web launcher
browser launcher
HTTP API
```

Results were classified rather than blindly deleted.

### A. Active dependency

The only material active runtime-path finding was CHZZK VOD temporary job storage:

```text
backend/.rust-web/vod
```

This was migrated to the canonical private runtime namespace:

```text
backend/.stream-archive/vod
```

SOOP VOD already uses the `.stream-archive` namespace.

### B. Historical documentation

Phase 19/21/22 documents retain accurate historical references to Web/Axum/`rust-web` architecture. They are evidence of the state at those phases and are not rewritten.

### C. Negative guards and current architecture documentation

Current guards and current docs intentionally mention retired concepts to prevent regression, for example:

- reject retired `rust-web-*.yml` workflow files;
- reject Web launcher/proxy artifacts from portable packages;
- reject direct Axum/localhost HTTP coupling in Slint/shared core;
- reject retired Web-only direct Cargo dependencies.

These references remain intentional.

### D. Persisted/bounded compatibility

`backend/.rust-web/vod` is retained only as a bounded stale-cleanup input. New jobs are never created there.

The root `.gitignore` therefore ignores both:

```text
backend/.stream-archive/
backend/.rust-web/
```

The first is current transient runtime state; the second exists only so old private temporary files do not become visible/untracked before bounded cleanup removes stale CHZZK jobs.

### Test-only localhost use

The `127.0.0.1` references in `platform_runtime.rs` are test-only `ping` helper processes used to exercise owned-process termination and descendant ownership on Windows. They are not an application HTTP listener.

### Transitive `tower-http`

`tower-http` remains in Cargo lockfiles only as a transitive dependency of `reqwest 0.12.28`. It is not a direct `rust-runtime/Cargo.toml` dependency and does not represent restoration of the retired Axum/Web presentation layer.

## 5. Compatibility inventory

The following compatibility identity is intentionally retained:

```text
Cargo package:   stream-archive-server
Rust crate:      stream_archive_server
default-run:     stream-archive-server
headless binary: stream-archive-server[.exe]
CLI binary:      stream-archive-cli
```

Current consumers include:

- `rust-runtime/Cargo.toml`;
- `rust-gui/Cargo.toml` path dependency;
- Rust imports through `stream_archive_server`;
- `stream-archive-cli serve` sibling/PATH lookup;
- `BUILD_PORTABLE.bat`;
- generated `RUN_HEADLESS.bat`;
- backup/restore process detection;
- release metadata package lookup;
- CI/package checksum verification;
- architecture/release guards;
- operator documentation.

A hard rename would therefore be a compatibility migration, not simple cleanup.

## 6. Why `stream-archive-server` remains

Phase 22.7 does not rename `stream-archive-server`.

The name is no longer evidence of an HTTP server implementation: the headless binary boots through `StreamArchiveCore` and the architecture guard explicitly rejects Axum/router/listener state.

Changing the name now would require coordinated compatibility work across CLI binary discovery, portable packaging, operator scripts, process detection, release metadata and external scripts that repository search cannot discover.

If a future product benefit justifies a migration, it should be a separate bounded phase with a transition such as:

```text
new canonical binary: stream-archive-runtime
temporary compatibility lookup/alias: stream-archive-server
```

No such migration is required to close Phase 22.

## 7. Runtime/core boundary

The final audit confirms that the runtime/core boundary remains shared rather than duplicated.

Important retained boundaries include:

- `StreamArchiveCore`;
- the public settings read facade required by Native GUI consumers;
- queue/history/backup services;
- provider-neutral LIVE/VOD facades;
- process lifecycle ownership primitives;
- tool discovery;
- native secret storage;
- platform runtime helpers.

The guards continue to reject resurrection of dead generic manager/lock getters and generic configuration compatibility facades while preserving APIs still consumed across crates.

## 8. Storage/config state

Canonical database:

```text
data/stream-archive.db
```

The final state remains:

- SQLite is the runtime settings/history/queue authority;
- legacy INI/TXT mirrors are not a runtime source of truth;
- `soop.db` to `stream-archive.db` is a bounded filename migration performed by shared core startup;
- backup policy is read/written through the SQLite-backed store;
- Native GUI uses `StreamArchiveCore` rather than direct SQLite ownership;
- new private VOD temporary jobs use `backend/.stream-archive/vod`.

No SQLite schema change is part of Phase 22.7.

## 9. Provider state

Supported provider/content boundaries remain unchanged:

```text
SOOP LIVE
SOOP VOD
CHZZK LIVE
CHZZK VOD
```

Phase 22.7 adds no clip/CATCH/shorts/community/post support and does not broaden provider behavior.

The provider guard continues to preserve:

- SOOP and CHZZK platform registration;
- provider-specific validation/authentication;
- encrypted CHZZK secret reuse;
- LIVE output/timestamp contracts;
- SOOP VOD yt-dlp ownership;
- CHZZK VOD Streamlink/FFmpeg boundaries;
- shared-core routing from Native UI.

## 10. Process ownership state

The process lifecycle contract remains:

- Stream Archive terminates processes it owns;
- process-name-wide `taskkill /IM`, `pkill` and `killall` are forbidden;
- LIVE Streamlink ownership is retained;
- VOD cancellation terminates retained Streamlink/FFmpeg ownership;
- Windows retained children use Job Object ownership;
- Unix retained children use isolated process groups;
- exact-PID Windows fallback remains bounded to non-retained compatibility callers.

No ownership helper was removed merely because it had a small internal caller count.

## 11. Portable package state

The portable contract remains:

Required:

```text
StreamArchive.exe
stream-archive-server.exe
RUN.bat
RUN_HEADLESS.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
RELEASE_INFO.txt
SHA256SUMS.txt
backend/
data/
maintenance/
docs/
LICENSE
THIRD_PARTY_NOTICES.md
```

Forbidden:

```text
stream-archive-launcher.exe
RUN_WEB.bat
RUN_SERVER_CONSOLE.bat
Caddyfile.example
REVERSE_PROXY.md
LOCAL_LAUNCHER.md
```

Default launch remains:

```text
RUN.bat -> StreamArchive.exe
```

`RUN_HEADLESS.bat` remains the optional compatibility/headless path.

## 12. Runtime guards

The consolidated runtime contract entry point remains:

```text
maintenance/Test-RuntimeContracts.ps1
```

Current component guards:

```text
Architecture.ps1
Providers.ps1
ProcessLifecycle.ps1
StorageOwnership.ps1
Security.ps1
ToolDiscovery.ps1
ReleaseSafety.ps1
```

Phase 22.7 strengthens the storage/release contracts so that:

- CHZZK new temporary jobs must use `.stream-archive/vod`;
- the legacy `.rust-web/vod` boundary must remain explicit and cleanup-only;
- a regression test must prove old stale CHZZK temp jobs are scavenged without reuse;
- both canonical transient state and bounded legacy state stay ignored by Git.

## 13. Current documentation audit

Current operational/developer documents intentionally describe the Native/core architecture and warn against restoring retired Web behavior:

- `README.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `docs/OPERATIONS.md`;
- `docs/UNIX_CLI.md`.

Historical Phase documents are preserved rather than rewritten to make old architecture names disappear.

The backup/restore maintenance errors now describe `stream-archive-server` as the optional headless runtime rather than a Web compatibility server.

## 14. Validation

Required local/CI-equivalent validation contract:

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

PR validation must finish with:

```text
core-check (linux)
core-check (macos)
core-check (windows)
windows-check
```

The final CI run/result is recorded below after PR validation.

```text
GitHub Actions run: 35700982962
- core-check (linux):   PASS
- core-check (macos):   PASS
- core-check (windows): PASS
- windows-check:        PASS

windows-check:
- Runtime contract guard:          PASS
- Source archive metadata smoke:   PASS
- Windows portable package smoke:  PASS
- Verify portable package:         PASS

Codex review: PASS — no major issues reported on PR #95 (closure implementation reviewed at `03cb3cff66`).
```

## 15. Remaining risks

Remaining risks are compatibility/operations risks rather than Phase 22 blockers:

1. external scripts outside the repository may depend on `stream-archive-server`;
2. the bounded `.rust-web/vod` stale-cleanup path should remain until compatibility removal is explicitly scheduled;
3. real-session/provider/media-tool integration remains different from repository-hosted CI and belongs to later product/integration work;
4. Unix packaging/install guidance and broader headless productization remain future work rather than Phase 22 cleanup.

## 16. Phase 23 handoff recommendations

Phase 23 should return to product capability and real-use completeness rather than continue namespace cleanup.

Good candidates include:

- real-session / real-tool integration coverage;
- Native UX completion and operational polish;
- stronger release/install workflows;
- user-facing runtime/diagnostic quality;
- provider feature work only when explicitly scoped.

A `stream-archive-server` hard rename should not be bundled into unrelated Phase 23 feature work.

## 17. Closure status

PR CI validation passed across Linux, macOS and Windows, including runtime contracts and the portable package verification path.

```text
Phase 22 status: COMPLETE
Phase 22 COMPLETE
Ready for Phase 23
```

Codex review is the final PR review gate; any material finding must be resolved before merge, but the Phase 22 implementation/audit scope itself has no remaining known blocker.
