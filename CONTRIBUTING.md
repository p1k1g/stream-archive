# Contributing to Stream Archive

Thanks for considering a contribution.

Stream Archive is currently a fast-moving pre-1.0 project. Please keep changes focused, preserve existing LIVE/VOD behavior unless a change is intentional and documented, and include regression coverage for behavior you modify.

## Supported project scope

The current product scope is intentionally limited to:

- SOOP LIVE recording
- SOOP VOD analysis/download
- CHZZK LIVE recording
- CHZZK VOD analysis/download

CATCH, clips, shorts/short-form media, community posts, and other non-LIVE/VOD content types are not currently supported. Large scope expansions should be discussed before implementation.

## Development workflow

1. Create a branch from the latest `main`.
2. Keep commits focused on one problem or feature area.
3. Run the applicable tests and runtime contract checks.
4. Open a Pull Request with a clear summary, regression risk, and validation notes.
5. Do not merge your own PR solely because CI is green when the change has meaningful runtime or data-loss risk; manual validation may still be required.

## Required safety invariants

Contributions must preserve these project-level rules unless the PR explicitly changes the architecture and updates the corresponding guards/tests:

- SQLite remains the canonical persistent configuration/history source.
- The application may terminate only child processes it created and owns.
- Do not add broad process-name termination such as `taskkill /IM ffmpeg.exe`, `pkill ffmpeg`, or equivalents.
- Secrets must not be returned by APIs or printed to logs.
- Secret storage must not silently downgrade to plaintext.
- VOD destination publication must remain collision-safe and cancellation-safe.
- Existing runtime contract guards must not be removed or weakened merely to make a change pass CI.

## Tests

At minimum, run the checks relevant to your change. The current Windows validation includes:

```powershell
./maintenance/Test-RuntimeContracts.ps1
cargo fmt --manifest-path rust-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path rust-runtime/Cargo.toml
cargo check --locked --manifest-path rust-runtime/Cargo.toml
cargo clippy --locked --manifest-path rust-runtime/Cargo.toml --all-targets --all-features -- -D warnings
```


Phase 20 expands this policy to Windows/Linux/macOS CI. A change that claims cross-platform support should include tests or an explicit explanation of what remains platform-specific.

## AI-assisted contributions

AI-assisted development is welcome, but the submitter remains responsible for the resulting code, tests, licensing, security, and correctness. Generated changes should be reviewed like any other contribution and should not introduce copied code with unclear provenance or incompatible licensing.

## Secrets and test data

Never commit or post:

- passwords
- Cloudflare/API keys
- `NID_AUT` / `NID_SES`
- management tokens
- session cookies
- private or subscription URLs containing sensitive authorization data
- private IPs, internal hostnames, or local data that you do not intend to publish

Redact logs before attaching them to an issue or PR.

## Licensing

By submitting a contribution, you agree that your contribution may be distributed under the project's `AGPL-3.0-or-later` license.

Third-party code must have a compatible license and must retain any required notices. New bundled third-party binaries are out of scope unless the PR also adds the necessary license, attribution, source-offer, packaging, and compliance work.
