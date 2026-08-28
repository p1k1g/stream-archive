# SOOP LIVE Downloader - Codex Instructions

## Current Baseline

Current version:

SOOP LIVE WinUI 3 v1.2.0-preview1-fix40

Target environment:

- Windows 11
- .NET SDK 8
- WinUI 3
- Microsoft Windows App SDK
- unpackaged application
- framework-dependent compact publish

Primary build:

BUILD_EXE.bat

Optional large self-contained build:

BUILD_SELFCONTAINED.bat

Generated project:

.generated\SOOPLiveWinUI

Publish output:

publish\

Do not create NEXT_CHANGES.txt.

Use only:

- README_WINUI3.txt
- VERSION.txt

for version/change documentation.


# IMPORTANT ARCHITECTURE

This project intentionally uses an official Microsoft WinUI CLI template.

Do NOT replace it with a manually-created XAML project.

Important historical constraint:

Earlier manually-created WinUI/XAML projects repeatedly failed during startup
and XAML compilation.

The working architecture is:

official Microsoft WinUI template
+
unpackaged configuration
+
untouched template App.xaml
+
untouched template App.xaml.cs
+
untouched template MainWindow.xaml
+
programmatic UI overlay in MainWindow.xaml.cs

The application's actual UI is built programmatically.

Preserve this architecture.


# PROJECT PREPARATION

PREPARE_PROJECT.bat / PREPARE_PROJECT.ps1 creates:

.generated\SOOPLiveWinUI

The generated csproj must remain:

WindowsPackageType=None

EnableWinAppRunSupport=false

WindowsAppSDKSelfContained=false for the compact build.

The compact BUILD_EXE.bat build is framework-dependent.

Do not accidentally convert it to WPF.


# WINFORMS TRAY SUPPORT

Tray support uses:

System.Windows.Forms.NotifyIcon

Do NOT add:

<UseWindowsForms>true</UseWindowsForms>

That previously activated an unwanted Windows Desktop/WPF build pipeline and
caused PresentationCore / PresentationFramework related failures.

Instead retain the current FrameworkReference approach:

Microsoft.WindowsDesktop.App.WindowsForms


# BACKEND LOCATION

The GUI must resolve backend only from:

AppContext.BaseDirectory\backend

Do NOT restore parent-directory fallback searching.

This rule prevents a new GUI from accidentally running an old backend version.

The publish directory MUST therefore contain:

publish\backend\SOOP_LIVE.ps1

PREPARE_PROJECT and BUILD_EXE contain validation for this.

Before considering a build successful, verify that file exists.


# BACKEND

Main watcher:

backend\SOOP_LIVE.ps1

The backend:

- checks SOOP LIVE state
- obtains global AID/master HLS through the Cloudflare Worker
- launches Streamlink for each recording
- monitors recorder PID and output-file growth
- checks disk free space
- supports channel/settings hot reload
- supports per-channel stop/resume commands


# CLOUDFLARE / SOOP

SOOP Korea/local API is used for LIVE/BNO/etc.

Cloudflare Worker provides the global playlist/AID URL.

CDN preference currently follows the existing backend implementation.

Do not expose or hardcode a private Worker API key into distributed defaults.


# AUTH

SOOP login credentials may be configured.

Login cookies are used for content that requires authentication.

Preserve:

SOOP_USERNAME
SOOP_PASSWORD
SOOP_PURGE_CREDENTIALS

Secret fields in the GUI may intentionally appear blank while preserving
previously configured values.

Do not display secrets back to the user.


# CHANNEL FILE

Channel file:

backend\SOOP_LIVE_CHANNELS.txt

Format:

Y|NAME|ACCOUNT|OUTDIR

Example:

Y|둘기얏|1004ysus|
Y|문월|moonwol0614|

OUTDIR may be empty.

Raw editor parser must support all line endings:

- CRLF
- LF
- CR

Do not regress this.

Channel raw editor and structured channel table must remain synchronized.

Raw pasted channel data must be accepted.

Existing .bak behavior must be preserved.


# SETTINGS / CHANNEL FILE WRITES

Configuration writes use a temporary file + validation + replacement approach.

Do not revert to directly truncating the active configuration file.

Reason:

The watcher performs hot reload and could otherwise read a half-written file.

Preserve atomic-style writes for:

SOOP_LIVE_SETTING.ini
SOOP_LIVE_CHANNELS.txt


# RECORDING BEHAVIOR

Once a channel starts recording, normal continuous LIVE polling is minimized.

The recorder watchdog checks:

- recorder process state
- file growth
- free disk space

Stall behavior:

stall timeout
→ immediate LIVE recheck
→ if still LIVE, refresh URL/restart recording

Default stall timeout has historically been around 90 seconds.

Preserve current configuration behavior.


# DISK PROTECTION

Minimum free space is configurable.

Disk space must be checked against the actual output drive.

Disk UNKNOWN and low disk conditions must fail safely.

The dashboard disk summary must only include channels currently:

● REC

PAUSED/restart-waiting cards must not contribute stale disk entries.


# FILE NAMING

Existing filename pattern logic must remain compatible.

Collision protection must remain.

Do not overwrite an existing recording.

Current suffix protection has a high collision cap.

Filename-safe sanitization must remain.


# PER-CHANNEL CONTROL COMMANDS

Do not use a single append/read/delete control file.

Current structure:

backend\control\<GUID>.cmd

GUI creates a temporary file and then renames/moves it to .cmd.

Watcher claims the command by renaming it to .processing.

This prevents command-loss races.


# STOP CURRENT BROADCAST

Command:

STOP_ONCE|ACCOUNT

Behavior:

- stop only that channel's recorder
- remember the current BNO as SuppressedBno
- do not automatically restart the same BNO
- transient OFFLINE responses must not clear that suppression

Do not implement stop by killing every Streamlink process.


# RESUME CURRENT BROADCAST

Command:

RESUME_ONCE|ACCOUNT

Behavior:

- clear SuppressedBno
- schedule immediate LIVE recheck
- if the same broadcast is still LIVE, start recording again
- use a new collision-safe output filename

GUI stopped state:

Ⅱ PAUSED

PAUSED must visually differ from REC.

Current intended color:

yellow / amber style

Active recording:

● REC

Current intended color:

green


# POWERSHELL $PID WARNING

PowerShell automatic variable:

$PID

is read-only and PowerShell variable names are case-insensitive.

NEVER define a parameter like:

$Pid
$pid

for custom functions.

Use:

$ProcessId

instead.

A previous implementation failed with:

VariableNotWritable

because $Pid collided with $PID.


# PROCESS TERMINATION SAFETY

Never run process-name-wide commands such as:

taskkill /IM streamlink.exe
taskkill /IM python.exe

The user may have unrelated Streamlink/Python processes.

Terminate only process trees owned by this GUI/watcher.

Expected flow:

1. taskkill /PID exact_pid /T /F
2. inspect exit code
3. verify process actually exited
4. if necessary, fallback to that exact Process object with entireProcessTree

Never broaden the scope.


# Stop-ChannelRecording RETURN VALUE

Stop-ChannelRecording returns success/failure.

Do NOT ignore a false result.

Current fix34 behavior:

CHANNEL DISABLED:

If recorder termination fails:

- state must not silently disappear
- keep/recover state so another hot-reload can retry
- log ERROR

CHANNEL REMOVED:

Remove state only after recorder termination is confirmed.

If recorder remains alive:

- retain state
- log ERROR
- retry later


# DASHBOARD

Top summary:

- active recording count
- offline count
- watcher state
- disk free space

Active count must count only:

Status == "● REC"


# OFFLINE UX

Do not permanently display OFFLINE rows under the recording dashboard.

The top Offline summary card is clickable.

Clicking it opens:

OfflineFlyoutList

showing currently offline channels.

The old OfflineList and BuildOfflineTemplate dashboard implementation were
removed in fix34.

Do not reintroduce them unless redesigning intentionally.


# RECORDING CARD

Static area during a recording:

- recording start time
- BJ/channel name
- broadcast title
- output filename

These values should NOT be rewritten on every progress sample.

Dynamic area:

- file size
- elapsed recording time
- download speed

Only those values should normally update while recording.


# PROGRESS FORMAT

Backend must emit progress with the BJ name, stable account ID, and metrics on
the SAME line.

Expected form:

[HH:mm:ss] BJ_NAME [account=SOOP_ID] : RECORDING | [download] Written ...

This is intentional.

Do not return to:

[BJ name]
[download] ...

correlation.

Concurrent recorder console output can interleave, making preceding-header
correlation unsafe.


# GUI PROGRESS PARSING

GUI uses CompactRecording lines.

Do not guess the channel from a prior unrelated console line.

Older:

currentProgressChannel
ChannelHeader

logic was removed in fix34.

Do not reintroduce that state unless absolutely necessary.


# PROGRESS SOURCE

Download speed shown by the backend is based on real output-file growth.

Conceptually:

(current file size - previous file size)
/
sample time difference

This is intentionally more robust than depending on Streamlink console text.

It is effectively a short interval average rate.

Preserve unless there is a clear reason to change.


# UI UPDATE PERFORMANCE

Backend stdout is not dispatched directly to the UI one line at a time.

Architecture:

Backend stdout/stderr
→ ConcurrentQueue
→ DispatcherTimer
→ batch processing
→ progress coalesced by channel

Current UI flush interval is approximately 250 ms.

Only the latest progress for a channel during a batch should matter.

Do not create a Dispatcher callback for every Streamlink/backend output line.


# GUI LOG

GUI log is event-oriented.

Keep approximately latest 50 lines.

Do not flood the log with download progress.

Useful events include:

- Watcher start/stop
- RECORD START
- RECORD FINISHED
- recorder exit
- stall
- retry
- low disk
- disk unknown
- Worker/auth problems
- channel stop/resume
- settings/channel hot reload
- errors/warnings

Clearing the log must clear both:

- displayed TextBox
- internal log queue


# FILE LOG

Do not repeatedly log OFFLINE state.

Backend file logs should focus on meaningful events.

Do not restore noisy repetitive OFFLINE logging.


# DASHBOARD FLICKER

Normal progress updates must not reorder/rebuild the ListView.

fix34 changed sorting from:

ObservableCollection.Clear()
then Add(...)

to minimal:

ObservableCollection.Move(...)

only when the item is genuinely out of position.

Preserve this.

During normal progress updates there should be:

- no collection Clear
- no remove/add
- no sort

Only bound metric properties should change.


# PAUSED CARD

When user stops a current broadcast:

Status:

Ⅱ PAUSED

Detail:

사용자 일시정지 · 다시 시작 가능

The card remains selectable.

The action button becomes:

▶ 선택 채널 녹화 재시작

When actual recording resumes:

Status returns to:

● REC


# SELECTED CHANNEL ACTION BUTTON

No selection:

button disabled

Selected active REC:

■ 선택 채널 녹화 중지

Selected PAUSED:

▶ 선택 채널 녹화 재시작


# TRAY

Tray implementation uses NotifyIcon.

X close behavior:

- system tray
- full exit
- cancel

User choice may be remembered.

Tray menu:

- Open
- Watcher Start
- Watcher Stop
- Full Exit

Full exit must terminate only the GUI-owned watcher process tree.

Recording-finished notification can open the recording folder.

No automatic Windows startup is currently wanted.

No automatic tray-start is currently wanted.


# CUSTOM ICON

The bundled app icon is custom/generic artwork.

Do not describe it as the official SOOP icon.


# SETTINGS DISPLAY OPTIONS

Preserve current meanings for:

CONSOLE_AUTO_FORMAT
CONSOLE_COLOR
CONSOLE_SHOW_PATH

CONSOLE_SHOW_PATH affects visual/path display but the GUI may still need to
know the actual recording path internally.


# BUILD ERRORS

Very important:

When BUILD_EXE.bat reports:

WMC1509
WMC9999

do NOT immediately treat the XAML compiler as the root cause.

Historically these frequently appeared as downstream errors after an earlier
C# compile error.

Always find and fix the FIRST C# error first.

Examples previously encountered:

- unterminated string literal
- missing FlyoutPlacementMode namespace
- missing Regex declaration

Then rebuild.


# BUILD VALIDATION

Before declaring a change complete:

1. Inspect changed C# and PowerShell code.
2. Search for dangling references to removed fields/methods.
3. Check Regex declarations against their usages.
4. Run BUILD_EXE.bat when execution is available.
5. Confirm compact publish succeeds.
6. Confirm:
   publish\backend\SOOP_LIVE.ps1
   exists.
7. Inspect the first real compile/runtime error before secondary WMC errors.
8. Run git diff.
9. Run git status.
10. Summarize exactly what changed and what was actually tested.


# SOURCE CLEANUP RULES

When removing dead code, do not use overly broad regex replacement that can
cross neighboring declarations.

A previous cleanup intended to remove ChannelHeader and accidentally deleted:

DownloadProgressWithPath

which caused CS0103 compile failures.

Prefer exact/balanced edits.

After cleanup always search for:

identifier definition count
identifier usage count

before considering it complete.


# REGRESSION REQUIREMENTS

Do not regress:

- official WinUI template architecture
- unpackaged execution
- compact framework-dependent publish
- backend bundled in publish
- tray support
- safe GUI-owned process-tree shutdown
- channel parser CRLF/LF/CR support
- raw channel editor
- structured channel management
- .bak backups
- atomic settings/channel saves
- Worker/auth logic
- Streamlink recording
- disk protection
- stall recovery
- multi-channel recording
- deterministic channel-tagged progress
- low-CPU UI batching
- latest-50 event log
- Offline flyout
- per-channel STOP_ONCE
- BNO suppression
- RESUME_ONCE
- yellow PAUSED status
- minimal-move dashboard sorting


# CHANGE POLICY

For substantial changes:

- update VERSION.txt
- append concise history to README_WINUI3.txt

Do not add NEXT_CHANGES.txt.

Do not silently make unrelated refactors.

Before editing, explain the intended scope if the requested change could
affect recording reliability, process ownership, or hot reload behavior.
