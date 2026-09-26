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

## Build and package

Source build:

~~~bash
cargo build --locked --release --manifest-path rust-runtime/Cargo.toml
~~~

Relevant raw build outputs:

~~~text
rust-runtime/target/release/stream-archive-cli
rust-runtime/target/release/stream-archive-server
~~~

Phase 23.6 adds the canonical portable archive builder:

~~~bash
./BUILD_UNIX_PACKAGE.sh
~~~

It performs a locked release build, assembles a clean package tree, writes `RELEASE_INFO.txt` and package-local `SHA256SUMS.txt`, verifies the package, creates the TAR.GZ plus archive-level `.sha256`, then extracts it to a fresh whitespace/non-ASCII path and runs packaged CLI smoke.

CI has verified these native artifact names:

~~~text
stream-archive-linux-x64.tar.gz
stream-archive-macos-arm64.tar.gz
~~~

No cross-compiled architecture is advertised.

## Packaged first run

Extract the archive and enter the package root:

~~~bash
tar -xzf stream-archive-linux-x64.tar.gz
cd stream-archive

./bin/stream-archive-cli init
./bin/stream-archive-cli tools
./bin/stream-archive-cli tools configure
./bin/stream-archive-cli doctor --active-tools
~~~

macOS uses the same package layout and commands after extracting `stream-archive-macos-arm64.tar.gz`.

Package layout:

~~~text
stream-archive/
├─ bin/
│  ├─ stream-archive-cli
│  └─ stream-archive-server
├─ backend/
│  └─ vod/
├─ data/
├─ docs/
│  ├─ UNIX_CLI.md
│  └─ OPERATIONS.md
├─ LICENSE
├─ THIRD_PARTY_NOTICES.md
├─ RELEASE_INFO.txt
└─ SHA256SUMS.txt
~~~

The bundled `data/` directory is empty by design. Run from the package root when using package-local defaults, or explicitly set:

~~~bash
export STREAM_ARCHIVE_BACKEND_DIR=/srv/stream-archive/backend
export STREAM_ARCHIVE_DATA_DIR=/srv/stream-archive/data
export STREAM_ARCHIVE_BACKUP_DIR=/srv/stream-archive/backups
~~~

Paths may contain whitespace and normal Unix Unicode characters.

## Source-tree first run

When running directly from a source checkout instead of a Phase 23.6 archive:

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

The summary includes backend/database paths, runtime readiness, watcher/VOD
state, Queue counts, configured secret booleans, media-tool resolution and
backup status.

Only one foreground runtime owner may hold the canonical data directory at a
time. Short-lived management commands open a non-recovering observer core, so
they do not rewrite another process's active LIVE/VOD/Queue rows as interrupted.

When a runtime owner is active, `status` obtains the in-memory watcher/VOD state
through a local Unix-domain control socket. The socket is owner-only (`0600`),
uses a short hashed path so macOS `sockaddr_un` limits are respected, and is not
an HTTP/Web application API.

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

Watcher status and runtime actions are routed to the active runtime owner when
one exists:

~~~bash
stream-archive-cli watcher status --json
stream-archive-cli watcher stop
stream-archive-cli channels action soop example recheck
~~~

Protected broadcast passwords use stdin and remain memory-only. The one-shot
CLI forwards the secret over the owner-only Unix-domain socket to the running
watcher's in-memory password path; it is not stored in SQLite and is not placed
in argv:

~~~bash
printf '%s' "$STREAM_PASSWORD" |
  stream-archive-cli channels password soop example --stdin
~~~

Channel/settings writes remain canonical SQLite writes. CLI channel
add/remove/enable/disable use row-level SQLite mutations rather than replacing
the full channel snapshot, so overlapping one-shot CLI writers cannot silently
discard each other's channel changes. A running watcher periodically refreshes
its shared Store cache so committed updates become visible without adding a
second configuration authority.

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
state. It acquires the same exclusive runtime-owner lock as `serve`; therefore a
direct `vod analyze` / `vod download` is intentionally rejected while another
foreground Stream Archive runtime owns the same data directory. For unattended
operation under `serve`, enqueue work with `queue add` instead.

Ctrl+C or SIGTERM calls the shared VOD cancel path and core shutdown rather than
killing processes by executable name. Failed or cancelled analysis/download
terminal states return a non-zero CLI exit status.

A second terminal can inspect or cancel the active owner VOD through the local
Unix-domain control socket:

~~~bash
stream-archive-cli vod status --json
stream-archive-cli vod cancel
~~~

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

When a foreground runtime owns an active Queue item, queue cancel is routed
through the protected Unix-domain control socket so the owner process can
cancel its process-local VOD job and finish the canonical Queue lifecycle.
Queued-only cancellation still works directly against SQLite when no runtime
owner is active.

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

Restore additionally acquires the cross-process runtime-owner lock. It is
refused while another Stream Archive runtime owns the canonical database, so a
short-lived CLI process cannot overwrite SQLite underneath an active
watcher/downloader/Queue worker. Existing LIVE/VOD/Queue safety checks still
apply after exclusive ownership is obtained.

## Storage and logs

~~~bash
stream-archive-cli storage --json
stream-archive-cli logs --tail 100
stream-archive-cli logs --tail 100 --json
~~~

Storage uses the shared storage service.

Runtime LogBuffer remains process-local internally. When a foreground runtime
owner is active, `logs` reads that owner's buffer through the same protected
Unix-domain control socket; otherwise it shows the observer process's local
buffer.

## JSON and exit behavior

Major read commands support JSON for scripts and automation. JSON mode prints
the result to stdout; errors continue to stderr.

General exit contract:

- success: zero
- invalid command/argument: non-zero
- blocking doctor result: non-zero
- failed runtime/provider operation, including failed VOD analysis: non-zero
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
directories with whitespace and Unicode paths. Phase 23.6 additionally builds
the real release archive, validates package-local and archive checksums, extracts
the archive into a new whitespace/non-ASCII directory, then executes the packaged
CLI from that extraction.

The smoke covers:

- init
- tool discovery/configure
- JSON-only output for management reads
- settings/channel persistence
- Queue/History/Backup/Storage reads
- provider status redaction
- one-shot observer commands preserving active runtime rows
- restore rejection while another runtime owns the database
- runtime-owner status/log routing through the Unix-domain socket
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

Phase 23.6 completes portable archive readiness for the currently CI-verified
native Unix targets. It does not add a Linux/macOS GUI, package-manager package,
service installer, code signing, notarization, auto updater, Git tag, or public
GitHub Release.

The archives are checksum-verified but unsigned. On Linux the native secret
contract still requires `secret-tool` and a usable Secret Service session.
macOS continues to use Keychain. Missing native secret storage never enables a
plaintext fallback.

Public release/version/tag decisions and real provider/session QA remain Phase
23.7 Release Candidate / Final QA.
