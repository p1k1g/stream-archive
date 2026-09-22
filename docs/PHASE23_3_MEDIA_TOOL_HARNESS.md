# Phase 23.3 — Media-tool Integration Harness

## 1. Goal

Phase 23.3 adds a deterministic, provider-network-free integration harness for local media-tool subprocess lifecycle.

Covered tools:

- Streamlink
- yt-dlp
- FFmpeg

The phase validates process execution itself:

```text
resolve -> spawn -> capture -> exit / timeout / cancel -> owned-tree cleanup
```

It does not validate SOOP/CHZZK network behavior. Provider E2E remains Phase 23.4.

## 2. Phase 23.2 baseline

Phase 23.2 established one shared runtime preflight consumed by Native Diagnostics and CLI doctor.

The default preflight remains passive/read-only:

- filesystem/tool discovery
- SQLite quick_check
- canonical settings state
- secret-store capability
- provider configured/not-configured state
- backup readiness

Phase 23.3 does not turn application startup into an active subprocess probe.

## 3. Existing process architecture audit

The runtime already had durable process-tree ownership before this phase:

### Windows

`platform_runtime::spawn_owned()` creates the process suspended, assigns the exact child handle to a kill-on-close Job Object, then resumes the child.

This prevents descendants from escaping between spawn and Job assignment.

### Unix

`spawn_owned()` establishes a dedicated process group before exec. Cancellation targets only the retained group.

### Existing LIVE/VOD paths

LIVE recorder and provider VOD implementations already use the shared ownership primitive.

CHZZK VOD additionally owns a Streamlink -> FFmpeg pipe and bounded progress/log channels.

SOOP VOD uses owned subprocesses plus provider-specific progress and partial-output cleanup.

Those long-running provider-specific streaming paths remain unchanged in Phase 23.3 because replacing them with a one-shot capture runner would weaken their progress/pipe semantics.

## 4. Shared media-process runner

New module:

```text
rust-runtime/src/media_process.rs
```

The runner accepts:

```text
program
argument vector
working directory
environment overrides
timeout
capture limit
optional cancellation token
```

It returns structured state:

```text
Success
ProcessFailure
Timeout
Cancelled
SpawnFailure
```

plus:

```text
exit code
bounded stdout tail
bounded stderr tail
truncation flags
duration
spawn error
```

Arguments are passed with `Command::arg/args`; no shell command string is assembled.

## 5. Deterministic fixture

Fixture source:

```text
rust-runtime/tests/fixtures/media_tool_fixture.rs
```

Tests compile it with the same Rust toolchain already required by Cargo tests.

No Python, shell, real FFmpeg, real yt-dlp or real Streamlink installation is required.

The fixture can:

- echo argument boundaries;
- emit stdout/stderr;
- return arbitrary non-zero status;
- produce large simultaneous stdout/stderr;
- emit invalid UTF-8 bytes;
- expose cwd/environment;
- write a caller-owned output file;
- sleep;
- spawn a descendant;
- provide a ready marker;
- emulate a version command;
- fail or hang a version probe.

The generated fixture binary stays under the test target directory and is not part of the portable package.

## 6. Bounded output

stdout and stderr are drained concurrently.

Only a bounded tail is retained.

The result records whether stdout/stderr were truncated.

This protects against pipe deadlocks and unbounded memory growth while keeping the final error/output tail.

Invalid UTF-8 is converted lossily rather than panicking.

## 7. Timeout

The runner polls with a bounded timeout.

On timeout, cleanup is delegated to the retained `OwnedProcessTree`.

The runner does not use a process-name lookup.

## 8. Cancellation

`MediaCancellation` is an explicit local token.

A cancelled run reports `Cancelled`, not `Timeout`.

Tests wait for a deterministic fixture ready marker before cancelling instead of depending on arbitrary short sleeps.

## 9. Process-tree ownership

The media runner reuses `platform_runtime::spawn_owned()`.

Windows therefore inherits the existing Job Object contract.

Unix inherits the existing dedicated process-group contract.

The integration harness creates a fixture descendant and verifies that timeout/cancel cleanup removes the owned descendant.

## 10. Unrelated-process protection

Cancellation testing also starts an unrelated instance of the same fixture executable.

Cancelling the owned fixture tree must not terminate the unrelated process.

This protects against regression toward process-name-wide termination.

## 11. Exit mapping

The harness distinguishes:

```text
exit 0              -> Success
non-zero exit       -> ProcessFailure
deadline reached    -> Timeout
explicit cancel     -> Cancelled
spawn failure       -> SpawnFailure
```

Provider-specific error mapping remains outside this generic layer.

## 12. Paths and Unicode

Fixture tests cover argument/path values containing:

- spaces;
- Korean text;
- Unicode/emoji.

Working-directory and environment propagation are also exercised.

Because arguments are not routed through a shell, shell quoting is not required.

## 13. Output-file ownership

The generic media-process runner owns process lifecycle, not provider media-file policy.

A regression test verifies that a successful fixture-created output remains present.

Existing SOOP/CHZZK code continues to own:

- partial-media cleanup;
- finalizing sidecars;
- destination claims;
- atomic publish behavior;
- successful final-output retention.

## 14. Version probe

Phase 23.3 adds `probe_tool_version()`.

Arguments:

```text
Streamlink -> --version
yt-dlp     -> --version
FFmpeg     -> -version
```

The probe uses:

- retained process ownership;
- bounded capture;
- bounded timeout;
- structured exit mapping.

The first non-empty output line is retained as the version string.

Tests cover success for all three tool kinds plus failure and timeout.

## 15. Diagnostics integration

Phase 23.2 passive diagnostics remains the default.

New explicit service:

```text
collect_active_local_preflight()
```

performs only local executable version probes after normal tool discovery.

It does not contact providers or media URLs.

Native application startup continues to use the passive snapshot.

## 16. CLI doctor integration

Existing commands remain passive:

```bash
stream-archive-cli doctor
stream-archive-cli doctor --json
```

Explicit active local probing is available with:

```bash
stream-archive-cli doctor --active-tools
stream-archive-cli doctor --json --active-tools
```

The JSON schema keeps the Phase 23.2 stable diagnostic IDs and summary model.

A failed required active tool probe becomes a blocking tool Error.

## 17. LIVE/VOD compatibility

Phase 23.3 intentionally does not replace the existing provider streaming subprocess loops.

Regression contracts remain responsible for:

- LIVE retained ownership;
- SOOP VOD owned progress/capture children;
- CHZZK Streamlink/FFmpeg retained trees;
- bounded VOD log/progress channels;
- cancellation readers;
- partial/finalizing cleanup;
- destination claim/no-clobber behavior.

## 18. Security and logging

The generic runner does not log full argument vectors or environment values.

Version probing supplies only the version flag.

Provider credentials are not introduced into the shared runner.

The diagnostics layer still does not read plaintext provider secrets.

## 19. Runtime contract guard

Phase 23.3 adds:

```text
maintenance/guards/MediaProcess.ps1
```

The contract requires:

- shared media runner;
- retained `spawn_owned` usage;
- bounded capture;
- cancellation;
- shared version probe;
- active preflight delegation;
- CLI active probe remaining explicit;
- core deterministic integration tests.

Existing process-lifecycle guards continue to prohibit process-name-wide termination.

## 20. Automated tests

The new harness covers:

- argument boundaries and Unicode;
- stdout/stderr;
- non-zero exit;
- structured spawn failure;
- concurrent large output;
- bounded tail/truncation;
- invalid UTF-8;
- cwd/environment propagation;
- caller-owned output preservation;
- timeout;
- owned descendant cleanup;
- explicit cancellation;
- unrelated process protection;
- version probe for Streamlink/yt-dlp/FFmpeg;
- version failure;
- version timeout.

Provider-network access is not used.

## 21. Platform matrix

The runtime harness is exercised by the existing whole-crate Cargo test matrix:

```text
Windows
Linux
macOS
```

Windows tests continue to run serially under CI to avoid Job Object test contention.

## 22. Manual developer validation

From repository root on Windows:

```powershell
git fetch origin
git switch phase23-media-tool-harness
git pull --ff-only origin phase23-media-tool-harness

cargo fmt --manifest-path .\rust-runtime\Cargo.toml -- --check
cargo test --locked --manifest-path .\rust-runtime\Cargo.toml -- --test-threads=1
cargo check --locked --manifest-path .\rust-runtime\Cargo.toml
cargo clippy --locked --manifest-path .\rust-runtime\Cargo.toml --all-targets --all-features -- -D warnings

cargo fmt --manifest-path .\rust-gui\Cargo.toml -- --check
cargo check --locked --manifest-path .\rust-gui\Cargo.toml
cargo test --locked --manifest-path .\rust-gui\Cargo.toml
cargo clippy --locked --manifest-path .\rust-gui\Cargo.toml --all-targets --all-features -- -D warnings

Set-ExecutionPolicy -Scope Process Bypass
.\maintenance\Test-RuntimeContracts.ps1

.\BUILD_PORTABLE.bat
```

Optional CLI smoke after building the runtime:

```powershell
.\rust-runtime\target\debug\stream-archive-cli.exe doctor
.\rust-runtime\target\debug\stream-archive-cli.exe doctor --active-tools
.\rust-runtime\target\debug\stream-archive-cli.exe doctor --json --active-tools
```

The active smoke requires the locally configured/discovered tools to exist. Mandatory CI tests do not.

## 23. Known limitations

Phase 23.3 does not:

- perform real SOOP/CHZZK requests;
- validate provider credentials over the network;
- run real downloads in CI;
- replace provider-specific streaming progress pipelines;
- define a new media-file cleanup policy.

The active version probe is explicit. Native startup remains passive.

## 24. Phase 23.4 handoff

Phase 23.4 — Provider E2E Validation should use the now-tested local process layer while validating:

- SOOP authentication/network behavior;
- CHZZK authentication/network behavior;
- public/private VOD;
- real LIVE start/stop;
- provider retry/network failure behavior.

## 25. Status

Implementation is in validation.

Closure requires:

```text
core-check (linux)
core-check (macos)
core-check (windows)
windows-check
runtime contract guard
portable package smoke
portable verification
final Codex review on final HEAD
unresolved review threads = 0
```

After those gates:

```text
Phase 23.3 COMPLETE
Ready for Phase 23.4 — Provider E2E Validation
```
