# Phase 23.4 — Provider E2E Validation

Phase 23.4 validates the production media-tool invocation boundaries for SOOP and
CHZZK without contacting either provider from automated tests.

## Scope and execution boundaries

The automated harness exercises the real Rust command construction and process
ownership code with the Rust fixture from
\`rust-runtime/tests/fixtures/media_tool_fixture.rs\`.

| Provider flow | Production subprocess boundary | Phase 23.4 validation |
| --- | --- | --- |
| SOOP LIVE | \`RecorderManager -> Streamlink -> spawn_owned\` | Stream URL, quality, output path, Unicode/whitespace, exit/spawn mapping, owned-tree cancellation |
| CHZZK LIVE | \`RecorderManager -> Streamlink -> spawn_owned\`, with FFmpeg player arguments | plugin preflight, cookie-file transport, FFmpeg player contract, output path, Unicode/whitespace |
| SOOP VOD | one-shot yt-dlp capture through \`media_process\`; streaming progress through retained \`spawn_owned\` | metadata invocation, yt-dlp download arguments, FFmpeg override, non-zero/spawn/timeout/cancel |
| CHZZK VOD | one-shot Streamlink capture through \`media_process\`; Streamlink -> FFmpeg streaming pipe through two retained \`spawn_owned\` owners | Streamlink/FFmpeg arguments, cookie-file transport, UTF-8 environment, output, non-zero/spawn/timeout/cancel |

The Phase 23.3 \`media_process\` runner is intentionally used for bounded,
one-shot capture commands. Long-lived streaming/progress paths keep their
retained \`spawn_owned\` ownership because they need live stdout/stderr parsing,
stdin piping, or a child handle that remains owned after the start call. Routing
those paths through a capture-to-completion helper would change production
semantics rather than merely validate them.

## Deterministic Rust fixture

The fixture is compiled by the Rust test suite and copied to an isolated
Unicode temporary directory under the executable names \`streamlink\`,
\`yt-dlp\`, and \`ffmpeg\` (with platform-appropriate suffixes).

It can:

- record argv, cwd, and selected UTF-8 environment values;
- emit deterministic metadata and progress;
- create deterministic output artifacts;
- exit non-zero;
- hang long enough to exercise timeout;
- create an owned descendant for cancellation/tree-cleanup tests.

The fixture performs no SOOP, CHZZK, YouTube, Streamlink-plugin, or yt-dlp
remote request. The SOOP full-manager case serves its deterministic HLS master
playlist from a test-owned `127.0.0.1` HTTP listener; test builds accept that
loopback URL only for this fixture. CHZZK remote metadata/authentication remains
outside the automatic harness, while the production post-metadata
download/retry/publish path is shared directly with the E2E test.

Synthetic cookie files are used only to verify cookie-file transport. Real
\`SOOP_PASSWORD\`, \`worker_key\`, \`NID_AUT\`, and \`NID_SES\` values are not
loaded into fixture arguments or invocation logs.

## Result and ownership coverage

The focused \`provider_e2e\` tests cover:

- SOOP LIVE success, non-zero exit, spawn failure, cancellation, and unrelated
  process protection;
- CHZZK LIVE plugin/cookie/FFmpeg-player invocation and Unicode output;
- SOOP VOD full-manager completion with configured tool resolution, local-only
  manifest authorization, metadata/download invocation, non-zero exit, spawn
  failure, timeout, cancellation, descendant cleanup, and unrelated process
  protection;
- CHZZK VOD production prepared-download completion with configured tool
  resolution, Streamlink-to-FFmpeg invocation, Unicode output, non-zero exit,
  spawn failure, timeout, and cancellation cleanup.

Phase 23.3 continues to own the lower-level generic runner tests for bounded
stdout/stderr capture, invalid UTF-8, timeout tree cleanup, late cancellation,
and tool version probes. Existing CHZZK job-state tests continue to guard the
rule that completed work wins over a late cancellation signal.

## Focused validation

From the repository root:

\`\`\`powershell
cargo test --locked --manifest-path .\rust-runtime\Cargo.toml provider_e2e -- --test-threads=1
cargo test --locked --manifest-path .\rust-runtime\Cargo.toml media_process::tests -- --test-threads=1
\`\`\`

Full validation remains:

\`\`\`powershell
cargo fmt --manifest-path .\rust-runtime\Cargo.toml -- --check
cargo check --locked --manifest-path .\rust-runtime\Cargo.toml
cargo test --locked --manifest-path .\rust-runtime\Cargo.toml
cargo clippy --locked --manifest-path .\rust-runtime\Cargo.toml --all-targets --all-features -- -D warnings
.\maintenance\Test-RuntimeContracts.ps1
.\BUILD_PORTABLE.bat
\`\`\`

The pull-request workflow runs the Rust suite on Windows, Linux, and macOS.
Windows additionally runs the runtime contracts and portable package smoke.

## Optional local provider smoke

The automatic E2E tests do not prove that a provider's current remote API or a
specific account/broadcast is available. A developer who explicitly wants a
local smoke test should first verify installed tools:

\`\`\`powershell
.\rust-runtime\target\debug\stream-archive-cli.exe doctor --active-tools
\`\`\`

Then use the normal Native UI with the existing local provider configuration:
start one known LIVE channel or one small VOD operation, confirm that output
starts, and cancel/stop promptly. Do not add real credentials to test source,
command lines, screenshots, or issue/PR logs. This manual smoke is opt-in and is
not run by CI.
