# Unix / headless CLI

Linux and macOS use the CLI/headless product surface. Windows keeps the Slint
Native UI as the default surface. Both paths use the same StreamArchiveCore,
SQLite store, provider services, Queue, History, Backup and process ownership.

The Unix CLI binary is:

~~~text
stream-archive-cli
~~~

The compatibility headless binary remains:

~~~text
stream-archive-server
~~~

No localhost HTTP/Web application API is started.

## Build

From the repository root:

~~~bash
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml
~~~

Relevant binaries:

~~~text
rust-runtime/target/release/stream-archive-cli
rust-runtime/target/release/stream-archive-server
~~~

## First run

Create the backend/data layout:

~~~bash
./rust-runtime/target/release/stream-archive-cli init
~~~

Default layout:

~~~text
./backend/
  vod/
./data/
  stream-archive.db
~~~

Overrides:

~~~bash
export STREAM_ARCHIVE_BACKEND_DIR=/srv/stream-archive/backend
export STREAM_ARCHIVE_DATA_DIR=/srv/stream-archive/data
export STREAM_ARCHIVE_BACKUP_DIR=/srv/stream-archive/backups
~~~

Paths may contain whitespace and normal Unix Unicode characters.

## Command tree

Phase 23.5 completes the daily-use CLI surface:

~~~text
stream-archive-cli
├─ init
├─ status [--json]
├─ settings
│  ├─ show [--json]
│  └─ set <KEY> <VALUE>
├─ providers
│  ├─ status [--json]
│  ├─ set <KEY> <VALUE>
│  ├─ secret <KEY> --stdin
│  └─ test-soop
├─ tools [--json|configure]
├─ doctor [--json] [--active-tools]
├─ channels
│  ├─ list [--json]
│  ├─ add <platform> <account> <name> <output-dir> [--disabled]
│  ├─ remove <platform> <account>
│  ├─ enable <platform> <account>
│  ├─ disable <platform> <account>
│  ├─ action <platform> <account> <stop|resume|recheck>
│  └─ password <platform> <account> --stdin
├─ watcher
│  ├─ status [--json]
│  ├─ start
│  └─ stop
├─ vod
│  ├─ analyze <URL> [--json] [options]
│  ├─ download <URL> --output <DIR> [--json] [options]
│  ├─ status [--json]
│  └─ cancel [--json]
├─ queue
│  ├─ list [--json]
│  ├─ add <URL> --output <DIR> [options]
│  ├─ cancel <ID> [--json]
│  ├─ retry <ID> [--json]
│  └─ remove <ID> [--json]
├─ history list [--json] [filters]
├─ backup
│  ├─ status [--json]
│  ├─ create [--json]
│  └─ restore <FILE-NAME> --yes [--json]
├─ storage [--json]
├─ logs [--tail N] [--json]
├─ serve [--watch]
└─ version
~~~

New daily-use commands call StreamArchiveCore rather than implementing a second
database, provider, Queue, History, Backup or process layer.

## Media tool discovery

Inspect Streamlink, yt-dlp and FFmpeg:

~~~bash
stream-archive-cli tools
stream-archive-cli tools --json
~~~

Persist discovered absolute paths atomically:

~~~bash
stream-archive-cli tools configure
~~~

Discovery order remains:

1. canonical SQLite tool settings
2. Stream Archive backend layout
3. PATH
4. common Unix installation locations

This bootstrap command is intentionally allowed to use the existing direct
settings bootstrap path. Daily-use management commands use StreamArchiveCore.

## Doctor

Passive shared preflight:

~~~bash
stream-archive-cli doctor
stream-archive-cli doctor --json
~~~

Explicit local media-tool version probing:

~~~bash
stream-archive-cli doctor --active-tools
stream-archive-cli doctor --json --active-tools
~~~

Active tool probing executes local version commands only. It does not contact
SOOP or CHZZK.

Required Error checks are blocking. Optional/provider warnings remain
non-blocking.

## Runtime status

~~~bash
stream-archive-cli status
stream-archive-cli status --json
~~~

The summary includes backend/database paths, runtime readiness, local watcher and
VOD state, Queue counts, configured secret booleans, media-tool resolution and
backup status.

The active watcher/VOD objects are process-owned. Because Stream Archive does
not add a cross-process HTTP or socket control service, a short-lived management
CLI invocation does not introspect the in-memory watcher of another already
running process. Persistent work should be owned by a foreground headless
runtime.

## Settings

Show editable non-secret runtime settings:

~~~bash
stream-archive-cli settings show
stream-archive-cli settings show --json
~~~

Update one validated setting:

~~~bash
stream-archive-cli settings set OUTPUT_DIR "/srv/archive/LIVE 저장"
stream-archive-cli settings set QUALITY best
stream-archive-cli settings set CHECK_INTERVAL 30
stream-archive-cli settings set MIN_FREE_SPACE_GB 20
~~~

Supported keys come from the existing environment_settings service. Arbitrary
SQLite writes are not exposed.

## Provider configuration and secrets

Provider readiness without secret values:

~~~bash
stream-archive-cli providers status
stream-archive-cli providers status --json
~~~

Non-secret settings:

~~~bash
stream-archive-cli providers set SOOP_USERNAME my-account
stream-archive-cli providers set CLOUDFLARE_WORKER_URL https://example.invalid/worker
~~~

Provider secrets are stdin-only:

~~~bash
printf '%s' "$SOOP_PASSWORD" |
  stream-archive-cli providers secret SOOP_PASSWORD --stdin

printf '%s' "$CHZZK_NID_AUT" |
  stream-archive-cli providers secret CHZZK_NID_AUT --stdin
~~~

Supported secret keys:

- SOOP_PASSWORD
- CLOUDFLARE_API_KEY
- CHZZK_NID_AUT
- CHZZK_NID_SES

The CLI does not provide password/NID option values that would expose secrets in
shell history or the process argument list.

Linux persists secrets through Secret Service. macOS persists them through
Keychain. SQLite stores opaque native-secret references rather than plaintext.
If the native store is unavailable the write fails closed; there is no plaintext
fallback.

SOOP authentication can be explicitly tested:

~~~bash
stream-archive-cli providers test-soop
~~~

This is a provider network operation and is not used by automatic CI.

## Channels and LIVE watcher

List channels:

~~~bash
stream-archive-cli channels list
stream-archive-cli channels list --json
~~~

Add or update state:

~~~bash
stream-archive-cli channels add soop example "Example" "/srv/archive/live"
stream-archive-cli channels disable soop example
stream-archive-cli channels enable soop example
stream-archive-cli channels remove soop example
~~~

Channel list writes go through StreamArchiveCore::update_channels and the
existing validation rules.

Run the watcher as a foreground headless runtime:

~~~bash
stream-archive-cli watcher start
~~~

Equivalent persistent runtime entry:

~~~bash
stream-archive-cli serve --watch
~~~

Stop it with Ctrl+C or SIGTERM from the service/container supervisor.

The one-shot watcher status/stop commands use the current CLI process core:

~~~bash
stream-archive-cli watcher status --json
stream-archive-cli watcher stop
~~~

Channel runtime actions are available when the watcher belongs to that process:

~~~bash
stream-archive-cli channels action soop example recheck
~~~

Protected broadcast passwords use stdin and remain memory-only:

~~~bash
printf '%s' "$STREAM_PASSWORD" |
  stream-archive-cli channels password soop example --stdin
~~~

## VOD

Analyze:

~~~bash
stream-archive-cli vod analyze "https://..." --json
~~~

Direct foreground download:

~~~bash
stream-archive-cli vod download "https://..." \
  --output "/srv/archive/VOD 저장" \
  --quality best \
  --parts 1,2
~~~

Useful options:

~~~text
--cookie-mode <MODE>
--cookie-file <PATH>
--browser <NAME>
--max-retries <N>
--quality <QUALITY>
--parts <1,2,...>
--no-merge
--yt-dlp <PATH>
--ffmpeg <PATH>
--json
~~~

Direct VOD stays attached to the CLI until the operation reaches a terminal
state. Ctrl+C or SIGTERM calls the shared VOD cancel path and core shutdown
rather than killing processes by executable name.

Status/cancel are also exposed through the shared core:

~~~bash
stream-archive-cli vod status --json
stream-archive-cli vod cancel
~~~

As with watcher state, there is no cross-process control plane. Use the signal
path to stop a foreground VOD command from another terminal.

## Queue

Queue entries are persistent SQLite data:

~~~bash
stream-archive-cli queue list --json
stream-archive-cli queue add "https://..." --output "/srv/archive/vod"
stream-archive-cli queue cancel <ID>
stream-archive-cli queue retry <ID>
stream-archive-cli queue remove <ID>
~~~

queue add persists work. The Queue worker is owned by the long-running headless
runtime, so normal unattended operation should keep stream-archive-cli serve
running.

## History

~~~bash
stream-archive-cli history list --json
stream-archive-cli history list --status COMPLETED --limit 50
stream-archive-cli history list --from 2026-09-01 --to 2026-09-30
stream-archive-cli history list --q channel-name
~~~

The CLI reuses HistoryFilter; it does not issue its own History SQL.

## Backup

~~~bash
stream-archive-cli backup status --json
stream-archive-cli backup create
~~~

Restore requires explicit confirmation:

~~~bash
stream-archive-cli backup restore stream_archive_manual_YYYYMMDD_HHMMSS.db --yes
~~~

The shared restore service still refuses restore while LIVE recording, VOD work
or Queue work makes replacement unsafe.

## Storage and logs

~~~bash
stream-archive-cli storage --json
stream-archive-cli logs --tail 100
stream-archive-cli logs --tail 100 --json
~~~

Storage uses the shared storage service.

Runtime LogBuffer is process-local. The logs command therefore shows lines
owned by the current CLI process; Phase 23.5 does not add a remote log/IPC
service.

## JSON and exit behavior

Major read commands support JSON for scripts and automation. JSON mode prints
the result to stdout; errors continue to stderr.

General exit contract:

- success: zero
- invalid command/argument: non-zero
- blocking doctor result: non-zero
- failed runtime/provider operation: non-zero
- foreground operation cancelled by signal: non-zero

Secrets are represented only by configured/not-configured booleans.

## SIGINT / SIGTERM

stream-archive-cli serve, watcher start and direct foreground VOD use the shared
headless/operation lifecycle.

On Unix:

~~~text
SIGINT ─┐
        ├─ shared cancellation / StreamArchiveCore::shutdown()
SIGTERM ┘
~~~

The compatibility stream-archive-server binary uses the same headless runner.
Owned media process-group cleanup remains in the shared runtime ownership layer.
There is no process-name-wide kill.

## Automated Unix validation

Linux/macOS CI runs the real binaries against isolated temporary backend/data
directories with whitespace and Unicode paths.

The smoke covers:

- init
- tool discovery/configure
- JSON-only output for management reads
- settings/channel persistence
- Queue/History/Backup/Storage reads
- provider status redaction
- real SIGTERM against stream-archive-cli serve
- real SIGTERM against stream-archive-server
- unrelated runtime survival

Phase 23.3 and 23.4 media process/provider E2E tests continue to cover owned
media descendants, cancellation and unrelated-process protection.

## Optional real-session smoke

On a real Linux/macOS host:

~~~bash
stream-archive-cli init
stream-archive-cli tools configure
stream-archive-cli doctor --active-tools
stream-archive-cli providers status
stream-archive-cli channels list
stream-archive-cli serve --watch
~~~

From another terminal, verify graceful shutdown:

~~~bash
kill -TERM <pid>
~~~

Do not place real provider credentials in repository files, issue logs,
screenshots or CI configuration.

## Phase boundary

Phase 23.5 completes the Unix CLI/headless management surface and lifecycle.
Unix installation bundles, Homebrew/deb/rpm/pkg packaging, service installers,
signing/notarization and release publishing remain Phase 23.6 Packaging /
Release Readiness.
