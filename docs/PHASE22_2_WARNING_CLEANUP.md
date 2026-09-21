# Phase 22.2 — Compiler / Clippy Warning Cleanup & Dead-Code Preparation

Phase 22.2 follows the Phase 22.1 Web / Legacy Audit. Its goal is to remove confirmed dead code and warning noise without changing user-visible behavior or prematurely removing the Web presentation layer.

## Outcome

- `rust-web` crate retained.
- Native/shared runtime behavior retained.
- Compiler / Clippy cleanup completed without adding broad warning suppressions.
- CI now treats project Clippy warnings as errors for:
  - `rust-web` on Windows, Linux and macOS;
  - `rust-gui` on Windows.
- Phase 22.3 Web-removal scope is clearer, but no Web presentation removal was performed here.

## Confirmed dead cleanup

Removed only items verified as unreferenced:

- `BackupManager::list`;
- `security::is_protected`;
- direct `atomic-write-file` dependency.

Both lockfiles were refreshed after dependency cleanup.

## Duplicate compilation cleanup

The server binary previously recompiled several shared source files through a private module tree. That produced misleading dead-code/unused warnings for code that is used by the Native/shared library path.

Phase 22.2 changed the server binary to reuse canonical library modules instead of compiling the same source again.

Also removed duplicate adapter loading for:

- History service;
- Queue service.

This reduces warning noise while keeping one canonical runtime implementation.

## Clippy cleanup

Strict Clippy checks were added to CI:

```text
cargo clippy --locked --manifest-path rust-web/Cargo.toml --all-targets --all-features -- -D warnings
cargo clippy --locked --manifest-path rust-gui/Cargo.toml --all-targets --all-features -- -D warnings
```

Cleanup covered control-flow simplification, unused imports, needless returns, sort/replace simplifications, explicit file-open semantics, and small argument-grouping structures where appropriate.

Existing narrow `allow` attributes that predated Phase 22.2 were not expanded into broad warning suppression.

## Process-lifecycle compatibility boundary

An important Phase 22.2 finding was that some process-lifecycle helpers looked dead to the compiler but were still architecture contracts.

The first cleanup pass over-isolated compatibility capture helpers into test-only code. The runtime contract guard correctly rejected that change.

The final implementation keeps compatibility boundaries available in production:

- common `terminate_owned` compatibility entry point;
- Windows exact-child identity / ToolHelp compatibility capture;
- Windows exact-PID `taskkill /PID /T /F` fallback;
- Unix compatibility process-group capture.

Unix compatibility capture now participates in the checked fallback path: an already-running child process group is adopted only when `getpgid(child_pid) == child_pid`. Otherwise cleanup is limited to the direct child, preventing accidental adoption of the server or unrelated process groups.

Retained ownership through `spawn_owned` remains the preferred path for normal runtime children.

## Final validation

Final validation baseline:

```text
GitHub Actions run #743

core-check (linux)    PASS
core-check (macos)    PASS
core-check (windows)  PASS
windows-check         PASS
```

The successful Windows job includes:

- Runtime contract guard;
- source archive metadata smoke;
- Windows portable package smoke;
- portable package verification.

The successful core checks include formatting, unit tests, compile checks, and strict Clippy checks.

## Phase 22.3 handoff

### Safe Web-only removal candidates

- browser static assets under `rust-web/web/`;
- Web auth/session/CSRF adapter code;
- SSE / realtime Web transport;
- Web local picker;
- Web-only launcher behavior;
- Web-only dependencies after call-site verification.

### Requires further verification

- `rust-web/src/main.rs`, because CLI `serve` still depends on the server entry point;
- Web adapter modules that share models or configuration contracts;
- build/package scripts that still include Web fallback artifacts;
- workflow and maintenance guards that encode Web compatibility;
- reverse-proxy / remote-Web documentation and deployment examples.

### Must remain / shared runtime code

- `rust-web` crate itself;
- `StreamArchiveCore`;
- Store / migrations;
- settings and configuration;
- providers;
- watcher / recorder;
- VOD;
- Queue / History / Backup / Storage;
- diagnostics and security;
- tool discovery;
- retained process ownership and compatibility process-lifecycle boundaries.

Phase 22.3 should remove presentation-only Web code without collapsing the shared runtime boundary.
