# Phase 21.3 — Settings, native picker and diagnostics

## Configuration investigation and scope

The canonical configuration is the `settings` table in `data/stream-archive.db`;
there is no runtime INI/TXT settings file to load. `Store::default_path` resolves
`STREAM_ARCHIVE_DATA_DIR`, otherwise the data folder beside the backend directory.
`backend::resolve_backend_dir` resolves `STREAM_ARCHIVE_BACKEND_DIR`, then the
existing working-directory/executable backend candidates. These startup-managed
locations are read-only in the native UI; changing the active database is not a
safe settings edit.

Existing Web routes `/api/settings` and `/api/vod/tool-settings` read the same
Store cache, use `primary_config` validators and persist with
`Store::sync_settings`. That transaction updates SQLite and the in-process cache.
Slint does not call these HTTP routes. Separate running processes retain their
existing cache semantics; this phase does not add cross-process cache invalidation.

Native Settings edits the existing `STREAMLINK_PATH`, `STREAMLINK_FALLBACK`,
`YT_DLP_PATH`, `FFMPEG_PATH`, `OUTPUT_DIR`, `CHECK_INTERVAL`, `MIN_FREE_SPACE_GB`,
and `QUALITY` keys. Other settings, credentials, backup policy and provider
controls remain in the current Web interface until their native parity phases.
Only changed fields are sent. Existing relative/stale Web settings are left alone
unless edited. New explicit native paths must be absolute, existing files/folders;
Windows tool paths must be `.exe` or `.com`. Blank and Streamlink `AUTO` preserve
the existing automatic discovery contract. Existing Web/CLI validation is unchanged.

`Slint -> settings draft/controller -> StreamArchiveCore::update_environment_settings`
validates the complete patch with the existing general/VOD validators plus shared
environment path checks, then takes the core configuration lock and writes one
Store transaction. An invalid VOD field cannot partially save the general fields.
A failed save retains the draft; cancelling a picker retains the previous value.
Refresh updates diagnostics without discarding edits. The explicitly labelled
Discard/reload action reloads the saved values. Saving does not restart recordings.

## Native picker and dependency review

`rust-gui/src/native_picker` owns Windows `IFileOpenDialog`, with
`FOS_PICKFOLDERS` for directories and an executable filter for tools. A dedicated
worker initializes an STA COM apartment, releases COM interfaces and allocated
paths, and balances `CoUninitialize`. Cancel is distinct from failure. No HTTP,
PowerShell bridge or child process is used. The UI remains responsive and blocks
conflicting settings actions while a dialog or save is in progress. The dialog is
unowned (no Slint HWND transfer) and the whole app process closes it on application
exit; native window ownership/accessibility polish remains a desktop QA item.

The GUI adds direct dependencies on the already locked `tokio` and Windows-only
`windows = 0.62.2`. Microsoft maintains windows-rs; it is MIT OR Apache-2.0 and
provides the supported Common Item Dialog API. This reuses Slint's existing locked
Windows crate rather than adding a separate cross-platform dialog library.
`windows-sys` in the core does not provide the convenient typed COM interface
needed here. COM/Shell features affect only the GUI; neither the core manifest nor
its lockfile changes. The existing portable package still ships the same server
and launcher, not Slint (packaging migration is Phase 21.8).

Sources:
- https://github.com/microsoft/windows-rs
- https://crates.io/crates/windows/0.62.2
- https://learn.microsoft.com/en-us/windows/win32/shell/common-file-dialog

## Diagnostics

`StreamArchiveCore::diagnostics` returns a serializable `DiagnosticsSnapshot` with
OK/Warning/Error rows and aggregate environment readiness. It reports canonical
backend/database paths, database existence, SQLite configuration load status,
data/output/bundled-tool directories, configured media paths, discovered executable
paths and discovery warnings, and native provider/secret-store resources.

These are read-only filesystem/discovery checks. No network/authentication,
secret decryption, media execution, write probe or recorder startup is performed.
Discovery shares the Phase 20 `tool_discovery` service; it is not a promise that
every provider's media operation or legacy provider-specific fallback will succeed.
Missing tools are warnings; unavailable backend/database/configuration are errors.
Slint only renders status labels/colors; it does not decide readiness.

## Verification

Existing JS, core Rust format/unit/check/clippy, runtime contract, archive metadata
and Windows portable build/verification gates are retained. GUI format/check stay
locked; GUI adapter unit tests are added to Windows CI. Architecture guards now
inspect every GUI Rust module, including the picker, instead of only main.rs.
Shared tests cover valid/invalid/missing/wrong-kind paths, numeric constraints,
serialization, no-write diagnostics, status aggregation and atomic persisted
settings visible through the same Web Store getters. Adapter tests do not open a
dialog. Native dialog interaction still requires an interactive Windows session.

Manual Windows acceptance:
1. Run `cargo run --locked --manifest-path rust-gui/Cargo.toml` with the normal backend.
2. Open Settings: verify saved values, read-only paths and diagnostic statuses.
3. Browse to an executable/folder (including a Korean/space-containing path); cancel
   another selection and verify the draft is unchanged.
4. Save valid values, restart and verify persistence; check the same keys in Web
   after restarting that process. An invalid/missing path must leave all saved keys unchanged.
5. Edit a value, refresh diagnostics (draft survives), then discard/reload.
6. Start with an invalid backend/database and verify a visible startup error.

Later phases: LIVE controls (21.4), VOD operations (21.5), Queue/History, credentials
and remaining settings parity, window-owner/accessibility review, and Slint release
packaging (21.8). This phase does not change providers or media process ownership.
