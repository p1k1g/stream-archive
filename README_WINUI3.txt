SOOP LIVE Downloader - WinUI 3
Version 1.2.0-preview1-fix80

BUG FIX
-------
fix14 GUI saved these INI keys but the backend SOOP_LIVE.ps1 did not read them:
- CONSOLE_AUTO_FORMAT
- CONSOLE_COLOR
- CONSOLE_SHOW_PATH

Therefore the settings previously had no backend effect.

fix15 implements them in BOTH backend output and WinUI dashboard.

CONSOLE_AUTO_FORMAT
-------------------
Y:
- backend sorts channels by name
- grouped RECORDING console layout
- GUI recording/offline lists sort by channel name

N:
- backend does not apply name sorting
- compact one-line recording output
- GUI keeps arrival/channel-processing order

CONSOLE_COLOR
-------------
Y:
- backend uses status colors where supported
- GUI REC/status highlighting is enabled

N:
- backend uses plain/default console colors
- GUI status text uses neutral colors

CONSOLE_SHOW_PATH
-----------------
Y:
- backend progress includes "to <full path>"
- GUI shows full recording file path below the recording row

N:
- backend progress omits the full file path
- GUI hides the path

HOT RELOAD
----------
All three backend values are included in Update-HotConfig.
Changing and saving the setting while Watcher is running takes effect without
requiring a full restart, though GUI templates are also refreshed immediately.

TEST
----
1. PREPARE_PROJECT.bat
2. RUN_SOURCE.bat
3. Start Watcher.
4. Test each display checkbox ON/OFF and save.
5. Confirm both Logs/backend output and Dashboard behavior change.
6. BUILD_EXE.bat after validation.


preview1-fix16
-------------
Fixes C# compiler errors introduced in fix15:
- CS9006
- CS1073

Cause:
Interpolated raw strings ($""") were used for XAML DataTemplate text while
the XAML itself contains {Binding ...} expressions. C# interpreted the binding
braces as interpolation syntax.

Fix:
- DataTemplate XAML now uses non-interpolated raw strings.
- Dynamic status color / optional path row are inserted with string.Replace().
- No backend behavior change from fix15.


preview1-fix17
-------------
Standalone settings CLI bug fix.

Problem:
General Settings -> All Settings and several other submenus called:
  $cfg = Read-Ini
without passing SOOP_LIVE_SETTING.ini.

Read-Ini then received an empty LiteralPath and failed with:
  Cannot bind argument to parameter 'LiteralPath' because it is an empty string.

Fix:
- All zero-argument Read-Ini calls now use:
    Read-Ini $IniPath
- Read-Ini is also hardened with:
    param([string]$Path = $IniPath)
  and falls back to $IniPath when blank.

Affected menu functions included:
- All Settings
- Runtime / Hot Reload Settings
- Recording Settings
- SOOP Login Settings
- Cloudflare Worker Settings
- Log Settings


preview1-fix18
-------------
Window-close process cleanup fix.

Problem:
Closing the WinUI window with the X button terminated only the GUI.
The PowerShell watcher remained alive in the background, so killing a
Streamlink/Python recorder process caused the watcher to start it again.

Fix:
- MainWindow hooks the WinUI Window.Closed event.
- On X close, BackendProcessService.StopNow() is called synchronously.
- StopNow uses:
    taskkill /PID <watcher-pid> /T /F
  so the whole descendant process tree is terminated:
    GUI-managed PowerShell watcher
      -> Streamlink
         -> Python / child processes
- Fallback uses Process.Kill(entireProcessTree: true).
- Dispose() now uses the same StopNow cleanup path.
- Duplicate-close cleanup is guarded.

Expected behavior:
- Stop button:
    stops Watcher/recorders, GUI stays open.
- X button:
    stops Watcher/recorders first, then GUI closes.
- After X close, SOOP_LIVE.ps1 / streamlink / recorder Python processes
  started by this GUI should no longer remain or restart.

Test:
1. PREPARE_PROJECT.bat
2. RUN_SOURCE.bat
3. Start Watcher and confirm a recording is active.
4. Click the window X without pressing Stop.
5. Confirm the output file stops growing.
6. Confirm the related powershell/streamlink/python child processes disappear.


preview1-fix19
-------------
Dashboard path-display fix:
- Progress parsing is now split into two explicit patterns:
  1) Written <size> to <full path> (...)
  2) Written <size> (...)
- The path-aware pattern is always tried first.
- This prevents the optional regex branch from swallowing "to <path>" as part
  of the size text.
- When CONSOLE_SHOW_PATH=Y, FilePath is stored and displayed on the second
  line of the recording card.

BAT cleanup:
Removed obsolete diagnostic BAT files when present:
- RUN_VANILLA.bat
- COMPARE_DIAG.bat
- RUN_SOURCE_DIAG.bat
- CHECK_ENV.bat
- CHECK_PREREQ.bat

Retained root BAT files:
- PREPARE_PROJECT.bat
- RUN_SOURCE.bat
- BUILD_EXE.bat

Retained backend CLI BAT files:
- backend\SOOP_LIVE.bat
- backend\SOOP_LIVE_SETTING.bat

Those backend BATs remain intentionally because they provide the verified
standalone CLI watcher/settings workflow.


preview1-fix20
-------------
Publish size optimization.

Why the old BUILD_EXE was 600+ MB:
- --self-contained true
- WindowsAppSDKSelfContained=true
This copied the .NET runtime and Windows App SDK runtime into the output.

New default:
BUILD_EXE.bat
- framework-dependent
- WindowsPackageType=None
- WindowsAppSDKSelfContained=false
- output: publish- requires .NET 8 Desktop Runtime x64 + Windows App Runtime already installed

Optional:
BUILD_SELFCONTAINED.bat
- keeps the old large all-in-one deployment
- output: publish_selfcontained
RUN_SOURCE.bat note:
dotnet run already builds an EXE, but it lives under:
.generated\SOOPLiveWinUIin\...
Use SHOW_BUILT_EXE.bat to display those paths.


preview1-fix21
-------------
Recording path display reliability fix.

Root cause addressed:
The GUI previously depended mainly on periodic [download] progress text to
discover the output file path. This made FilePath dependent on console format
and parser timing.

New behavior:
- GUI directly parses the RECORD START information block emitted by backend:
    Channel : <name>
    Title   : <title>
    Output  : <full path>
- Output path is stored in ChannelStatus.FilePath immediately when recording
  starts.
- CONSOLE_SHOW_PATH now controls DISPLAY only. It no longer determines whether
  the GUI internally knows the path.
- When enabled, dashboard displays:
    파일 : C:\...\recording.ts

Backend correction:
- Initial CONSOLE_AUTO_FORMAT / CONSOLE_COLOR / CONSOLE_SHOW_PATH loading was
  accidentally placed inside Update-HotConfig in an earlier patch.
- Initial loading is now in Main after reading SOOP_LIVE_SETTING.ini.
- Hot reload keeps only newConfig-based updates.

Next planned UI change:
- Dedicated application icon for EXE, taskbar and window.
- Remove the generic/default WinUI application image.
See NEXT_CHANGES.txt.


preview1-fix22
-------------
UI/UX consolidation release.

1. Vanilla project cleanup
- .generated\VanillaWinUI is no longer created.
- PREPARE_PROJECT creates only .generated\SOOPLiveWinUI.

2. Vanilla-style UI polish
- Cleaner header and restrained styling.
- Recording card information density reduced.
- Large right-side broadcast-title column removed.
- Broadcast title remains as a small secondary line.
- Optional file path appears as a small tertiary line.

3. Dedicated application icon
- Custom generic SOOP LIVE Downloader icon included.
- EXE ApplicationIcon configured.
- WinUI AppWindow icon configured.
- Tray icon uses the same icon.
- This is custom artwork, not an official SOOP brand asset.

4. X close behavior
- X asks:
  * System tray
  * Exit completely
  * Cancel
- Optional "remember this choice".
- Remembered preference is stored in SOOPLiveWinUI.user.json.
- Tray mode keeps Watcher/recording alive.
- Exit mode stops the GUI-owned watcher process tree.

5. System tray
- Double-click: reopen window.
- Context menu:
  * Open
  * Watcher start
  * Watcher stop
  * Exit completely

6. Recording-finished notification
- Backend RECORD FINISHED log is detected.
- Tray balloon shows channel / duration / size / reason.
- Clicking the notification opens the recording folder.

7. Existing fixes retained
- X full-exit process-tree cleanup.
- Dashboard recording-path display.
- Compact framework-dependent BUILD_EXE.
- Optional BUILD_SELFCONTAINED.


preview1-fix22-fix1
-------------------
BUILD_EXE / publish compilation fix.

Observed error:
MC6000:
PresentationCore, PresentationFramework must be included in the .NET Framework
assembly reference list.

Cause:
fix22 enabled:
  <UseWindowsForms>true</UseWindowsForms>

In this WinUI 3 project that caused WindowsDesktop/WPF XAML build targets to
process the WinUI App.xaml, resulting in the WPF assembly requirement.

Fix:
- Removed UseWindowsForms=true.
- Added only:
    <FrameworkReference Include="Microsoft.WindowsDesktop.App.WindowsForms" />
- System.Windows.Forms.NotifyIcon/tray functionality is retained.
- WinUI XAML continues through the Windows App SDK compiler only.

Also recorded for the next revision:
- suppress repetitive OFFLINE rows from file logs
- event-focused logs
- GUI Logs tab limited to the latest 50 lines


preview1-fix23
-------------
Selected usability/stability improvements.

1. Channel management table
- Structured ListView over SOOP_LIVE_CHANNELS.txt.
- Add / Edit / Delete / Enable-disable toggle / Reload / Save.
- Raw text editor remains below for advanced direct editing.
- Save creates SOOP_LIVE_CHANNELS.txt.bak first.

2. Event-focused logs
- Repetitive OFFLINE state is not intended for daily file logs.
- GUI Logs tab keeps only the latest 50 lines.

3. Start validation
Before Watcher start, GUI checks:
- Cloudflare Worker URL exists and uses https://
- API key exists
- output directory is usable
- at least one channel is enabled

4. Per-channel recording stop
- Select a recording row and click "선택 채널 녹화 중지".
- GUI writes STOP_ONCE|<account> to SOOP_LIVE_CONTROL.txt.
- Backend consumes the command, stops only that recording, and suppresses
  restart until that current live session becomes OFFLINE.

5. Backups
- Settings save creates SOOP_LIVE_SETTING.ini.bak.
- Channel save creates SOOP_LIVE_CHANNELS.txt.bak.

Not added:
- Windows automatic startup
- automatic tray-start mode


preview1-fix24
-------------
System tray visibility fix.

Symptom:
Choosing "시스템 트레이로" hid the window, but no tray icon was visible.

Likely cause:
System.Windows.Forms.NotifyIcon can be Visible=true while still not appearing
when Icon is null. The custom .ico asset was not guaranteed to be copied into
the runtime/publish output folder.

fix24:
- Forces Assets\SOOPLiveDownloader.ico and .png to CopyToOutputDirectory and
  CopyToPublishDirectory.
- Loads the custom icon from multiple runtime candidate paths.
- Falls back to SystemIcons.Application if the custom icon is missing/invalid.
- Assigns Icon BEFORE setting NotifyIcon.Visible=true.
- Tracks trayReady state.
- X -> tray will NOT hide the window if tray initialization failed.
- Tray initialization/restore/hide errors are written to
  SOOPLiveWinUI_startup.log.
- Existing tray menu remains:
  Open / Watcher Start / Watcher Stop / Exit.
- All fix23 functionality is retained:
  channel table management, latest-50 GUI logs, start validation,
  per-channel recording stop, .bak backups.


preview1-fix25
-------------
High-CPU / "Not Responding" GUI fix.

Observed:
- SOOPLiveWinUI.exe remained alive and downloading normally.
- One GUI thread consumed essentially an entire CPU core continuously.
- Windows reported Responding=False.
- No .NET Runtime/Application Hang error was recorded.

Changes:
1. Backend stdout/stderr no longer posts one DispatcherQueue callback per line.
   Lines are stored in a ConcurrentQueue.
2. WinUI flushes backend output every 250 ms in bounded batches.
3. At most 400 raw backend lines are processed per UI tick.
4. Recording progress is coalesced by channel:
   within one UI interval only the newest progress row per channel is applied.
5. GUI log is stored separately and limited to 50 lines.
   LogBox.Text is rebuilt only once per UI flush, not once per backend line.
6. Repetitive OFFLINE rows are suppressed from the GUI log.
7. Progress updates no longer call MoveToRecording / collection re-sort when
   the channel is already in RecordingItems.
8. Model property setters avoid PropertyChanged when the value did not change.
9. UI flush timer is stopped during real application shutdown.

All previous behavior retained:
- robust tray icon / X tray-or-exit dialog
- recording-finished notifications
- channel table add/edit/delete/toggle
- start validation
- per-channel recording stop
- .bak settings/channel backup
- compact and self-contained build options

Important:
Forcibly terminating SOOPLiveWinUI.exe in Task Manager does NOT guarantee that
the watcher/streamlink/python child processes stop. Use the normal
"완전히 종료" action when possible so the watcher process tree is terminated.


preview1-fix25-fix1
-------------------
Compile fix for fix25.

Observed:
CS0136:
- progressWithPath
- progressNoPath

Cause:
The new queued-progress/coalescing block declared local variables using the
same names as the existing progress parser later in ProcessBackendLine().
C# does not allow those local names to be redeclared in the enclosing method
scope in this arrangement.

Fix:
- progressWithPath     -> queuedProgressWithPath
- progressNoPath       -> queuedProgressNoPath
- progressMatch        -> queuedProgressMatch

The WMC1509/WMC9999 XAML messages that followed were secondary build errors
after the C# compilation failure.

All fix25 UI batching/high-CPU changes are retained.


preview1-fix26
-------------
Channel-management synchronization fix.

Observed:
- Channel dashboard/backend could recognize configured channels.
- Channel-management table could appear empty.
- Pasting an older SOOP_LIVE_CHANNELS.txt into the raw editor and pressing
  Save could result in only the header remaining.

Root cause:
SaveChannels_Click always called SyncRawChannelTextFromItems() first.
Therefore the structured table was treated as authoritative even when the
user had just pasted/edited the raw text. If ChannelItems was empty, the
pasted raw text was overwritten with only:
  # ENABLED|NAME|ACCOUNT|OUTDIR

fix26:
- Tracks whether the raw channel editor was modified by the user.
- If raw text is newer, Save parses RAW -> structured table first.
- If structured buttons are newer, Save writes TABLE -> raw text.
- Add/Edit/Delete/Toggle first apply any pending raw edits so pasted channels
  cannot be silently lost.
- Added "원본 적용" button for explicit RAW -> TABLE refresh.
- Shows the exact active SOOP_LIVE_CHANNELS.txt path in the Channel page.
- Reload resets raw dirty state and reparses the file.
- Save reports the channel count and exact target file path.
- Invalid raw lines are rejected with line number and expected format.
- Existing .bak backup behavior remains.

All fix25 UI batching/high-CPU fixes and earlier features are retained.


preview1-fix27
-------------
Channel raw-editor save hardening.

Reported valid input:
  # ENABLED|NAME|ACCOUNT|OUTDIR
  Y|둘기얏|1004ysus|
  Y|고라니율|golaniyule0|
  ...
was incorrectly reported as having no valid channels.

fix27:
- Save no longer depends on rawChannelTextDirty to choose the source.
- If the raw editor has any non-comment data row, RAW is always authoritative.
- If raw has only the header but the table has rows, TABLE is authoritative.
- Y|NAME|ACCOUNT| with an empty OUTDIR is explicitly supported.
- Parser uses Split('|', 4) for simpler/safer handling.
- After successful raw parsing, text is normalized from parsed table rows.
- "원본 적용" rejects an actual zero-row parse with useful diagnostics.
- Save errors include raw character count and candidate channel-row count.
- Exact target channel file path display and .bak backups remain.

All fix25 high-CPU batching and previous functionality are retained.


preview1-fix27-fix1
-------------------
Compile fix for fix27.

Observed:
CS1039 / CS1003 / CS1010 around MainWindow.xaml.cs line 2062.

Cause:
Some diagnostic strings added in fix27 were emitted with physical newlines
inside normal C# quoted string literals.

Fix:
- Replaced those physical line breaks with escaped \n sequences.
- Channel raw parser/save-source changes from fix27 are unchanged.
- WMC1509/WMC9999 messages were downstream build failures after the C# syntax
  error and should disappear once compilation proceeds normally.


preview1-fix28
-------------
Channel raw-editor line-ending compatibility fix.

Observed:
The raw editor visibly contained valid rows such as:
  Y|둘기얏|1004ysus|
  Y|문월|moonwol0614|
but "원본 적용" reported:
  유효한 채널 행을 찾지 못했습니다.
  원본 문자 수: 136

Root cause:
The previous parser used:
  Replace("\r", "").Split('\n')

If the WinUI TextBox/pasted content uses CR-only line endings, removing '\r'
collapses every visible row into one physical string. Because the resulting
string begins with '# ENABLED...', the parser treats the entire content as
one comment line and returns zero channels.

fix28:
- Added SplitChannelLines().
- Supports CRLF (\r\n), LF (\n), and CR (\r) line endings.
- Both raw parser and Save source detection use the same normalized splitter.
- Zero-channel diagnostic also reports detected line count.
- Empty OUTDIR remains supported.
- All prior channel synchronization and UI high-CPU fixes are retained.


preview1-fix29
--------------
Applied 15 requested changes: NEXT_CHANGES removal; atomic INI/channel saves; verified process-tree termination; race-free per-command control files; event-only backend/GUI logs; clear-log queue fix; BNO-based STOP_ONCE; fixed local backend path; disk summary card; recording progress-focused rows; short filename + full-path tooltip; compact OFFLINE rows; OFFLINE time removal; selection-gated per-channel stop button.


preview1-fix30
--------------
1. Recorder stop bug fixed
- PowerShell automatic variable $PID is read-only and case-insensitive.
- fix29 used a Stop-RecorderProcessTree parameter named $Pid, which collided
  with $PID and caused VariableNotWritable when stopping a recording.
- Renamed the parameter to $ProcessId.
- Applies to both:
  * dashboard "선택 채널 녹화 중지"
  * channel disable/remove paths that stop an active recorder

2. Dashboard OFFLINE UX
- OFFLINE rows are no longer permanently rendered below RECORDING.
- The top "오프라인" summary card is clickable.
- Clicking it opens a flyout showing the current OFFLINE channel list.
- Offline count remains visible at all times.
- This leaves the main dashboard focused on active recordings.

All fix29 stability hardening and dashboard improvements are retained.


preview1-fix30-fix1
-------------------
Compile fix for fix30.

Observed:
CS0103: 'FlyoutPlacementMode' does not exist in the current context.

Cause:
FlyoutPlacementMode is defined under:
  Microsoft.UI.Xaml.Controls.Primitives

Fix:
The offline summary-card flyout now uses:
  Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom

All fix30 behavior is retained:
- PowerShell $PID collision fix
- dashboard OFFLINE flyout
- all fix29 stability/dashboard changes


preview1-fix31
--------------
Published EXE startup fix.

Symptom:
- BUILD_EXE.bat / publish succeeds.
- publish\SOOPLiveWinUI.exe does not open.

Root cause:
fix29 intentionally restricted backend discovery to:
  AppContext.BaseDirectory\backend

However PREPARE_PROJECT copied the backend into the generated project source
tree without explicitly marking backend\**\* for SDK publish output.
Therefore dotnet publish could omit publish\backend\SOOP_LIVE.ps1.
ResolveBackendDirectory then threw before the window finished opening.

fix31:
- Generated csproj explicitly includes backend\**\* as Content.
- CopyToOutputDirectory=PreserveNewest.
- CopyToPublishDirectory=PreserveNewest.
- PREPARE_PROJECT verifies generated-project backend\SOOP_LIVE.ps1.
- BUILD_EXE verifies publish\backend\SOOP_LIVE.ps1 and fails loudly if absent.
- MainWindow startup log now records successful/failed backend resolution.
- Strict current-version backend path policy is retained; no old-parent-folder
  fallback is reintroduced.

All fix30/fix29 functionality is retained.


preview1-fix31-fix1
-------------------
Startup NullReferenceException fix.

Observed startup log:
  SOOP programmatic UI FAILED
  NullReferenceException
  MainWindow.BuildDashboard()

Root cause:
fix30 removed the permanently-rendered dashboard OFFLINE ListView, but one
old statement remained:
  OfflineList.ItemsSource = OfflineItems;

OfflineList was therefore null during BuildDashboard() and the application
exited before the window appeared.

Fix:
- Removed the stale OfflineList.ItemsSource assignment.
- Current OFFLINE data is bound only to OfflineFlyoutList, opened from the
  clickable top "오프라인" summary card.
- RecordingList.ItemsSource remains unchanged.
- fix31 published-backend verification and all earlier functionality remain.


preview1-fix32
--------------
Recording resume + stable dashboard refresh.

1. Stop / resume current live broadcast
- A user-stopped card remains visible as:
    ■ 중지됨
    사용자 중지 · 다시 시작 가능
- Selecting it changes the dashboard action button to:
    ▶ 선택 채널 녹화 재시작
- RESUME_ONCE clears the current BNO suppression and schedules an immediate
  LIVE recheck. If the same broadcast is still LIVE, recording starts again
  into a new collision-safe output filename.
- Active recording cards still use:
    ■ 선택 채널 녹화 중지

2. Progress display / flicker reduction
- Start time, BJ name, title and filename are static after RECORD START.
- ApplyProgress no longer overwrites the start time every refresh.
- Only three dedicated properties are updated during normal recording:
    file size
    elapsed recording time
    download rate
- These values are rendered in a visually separate right-side progress panel.
- Disk free-space lookup is not repeated for every progress update when the
  output file is unchanged.

3. Progress batching bug fixed
- CONSOLE_AUTO_FORMAT=N compact RECORDING lines are now coalesced by channel.
- CONSOLE_AUTO_FORMAT=Y raw [download] lines use the preceding channel header
  as their channel key.
- This restores file size / elapsed / download-rate output while preventing
  noisy progress lines from repeatedly repainting the whole dashboard.

4. Recording summary count
- Top "녹화 중" counts only cards currently in ● REC state.
- User-stopped/restart-waiting cards can stay visible without inflating the
  active recording count.

All fix31 published-backend verification, fix30 OFFLINE flyout and fix29
stability hardening are retained.


preview1-fix33
--------------
Deterministic multi-channel progress and stable metric refresh.

1. Missing progress on one of several simultaneous recordings
- fix32 could correlate an untagged [download] line to the most recently seen
  channel header.
- With concurrent channels, output ordering can interleave and that heuristic
  is not reliable.
- Backend now ALWAYS emits:
    [HH:mm:ss] BJ_NAME : RECORDING | [download] ...
  with the BJ name and metrics on the same physical line.
- GUI only accepts those channel-tagged CompactRecording progress lines.
- Untagged raw [download] lines are ignored for dashboard metrics.
- Every simultaneous recording therefore has an independent progress key.

2. Flicker / redraw reduction
- No collection sorting, removal, or re-add happens during ordinary metric
  updates.
- Start time, BJ, title, filename remain static.
- Only SizeText, ElapsedText and RateText receive steady-state updates.
- Metric columns use fixed widths so changing numeric text does not shift the
  surrounding card layout.
- The selection action button is no longer rewritten on every metric sample.

3. Pause state presentation
- User-stopped current broadcast remains selectable in the recording area.
- Status changes from green "● REC" to yellow "Ⅱ PAUSED".
- Detail:
    사용자 일시정지 · 다시 시작 가능
- Selecting the paused card exposes:
    ▶ 선택 채널 녹화 재시작
- Resume returns the status to green REC when recording actually resumes.

All fix32 resume behavior, fix31 publish verification, fix30 OFFLINE flyout,
and fix29 stability hardening are retained.


preview1-fix34
--------------
Maintenance / long-running stability cleanup.

1. Dead GUI code removed
- Removed obsolete dashboard OfflineList.
- Removed obsolete BuildOfflineTemplate().
- ApplyConsoleDisplayOptions now refreshes OfflineFlyoutList directly.
- Removed currentProgressChannel and ChannelHeader correlation logic left from
  the pre-fix33 untagged progress implementation.
- CompactRecording now passes its channel name as a local value.

2. Disk summary includes only active recordings
- The top disk-free card now considers only Status == "● REC".
- PAUSED/restart-waiting cards cannot leave stale drive entries in the summary.
- Per-card multi-drive text is also cleared for non-REC cards.

3. Channel disable/delete recorder-stop failure is no longer ignored
- CHANNEL DISABLED:
  If the owned recorder cannot be stopped, the in-memory state is restored to
  enabled and an ERROR is logged. A later hot-reload retries the stop.
- CHANNEL REMOVED:
  State is removed only after Stop-ChannelRecording confirms success.
  On failure the state is retained for another retry.
- No process-name-wide kill was introduced.

4. Dashboard sorting no longer Clear/Add rebuilds collections
- Previous SortDashboardItems called Clear() and re-added every card.
- fix34 computes the desired order and uses ObservableCollection.Move only for
  items that are actually out of position.
- This preserves existing ListView item containers as much as possible and
  reduces state-change flicker.

All fix33 deterministic multi-channel progress / yellow PAUSED UI, fix32
resume support, fix31 publish verification, fix30 OFFLINE flyout, and fix29
stability hardening are retained.


preview1-fix34-fix1
-------------------
Compile-only correction for fix34.

Observed:
CS0103: DownloadProgressWithPath does not exist in the current context.

Cause:
The fix34 dead-code cleanup intended to remove only the obsolete ChannelHeader
Regex declaration. The cleanup pattern crossed declaration boundaries and also
removed the still-required DownloadProgressWithPath declaration.

Fix:
- Restored DownloadProgressWithPath exactly from fix33.
- ChannelHeader/currentProgressChannel/OfflineList dead code remains removed.
- No fix34 runtime behavior was reverted.


preview1-fix34-fix2
-------------------
Channel management usability and data-loss protection.

1. Version consistency
- Updated the window version label, VERSION.txt, README header, maintenance
  prompt, and source/build labels to 1.2.0-preview1-fix34-fix2.
- Generated .NET project Version and InformationalVersion now use the same
  product version instead of the SDK default.

2. Unsaved-change protection
- Channel table and raw-editor changes now share an explicit dirty state.
- The channel page shows whether changes are saved or still pending.
- Reloading the saved file requires confirmation when changes are pending.
- Leaving the channel page offers Save and continue, Continue without saving,
  or Cancel instead of silently replacing pending edits.

3. Multi-channel actions
- Channel ListView selection mode is now Multiple and uses selection checkboxes.
- Added Select all, selected count, bulk delete, bulk enable, and bulk disable.
- Bulk delete confirms the affected channels and remains pending until Save.

4. Clearer commands
- Renamed the ambiguous raw/reload/save commands to describe their data flow.
- Added tooltips explaining that raw-to-list does not save, reload reads the
  last saved file, and Save keeps the existing .bak behavior.


preview1-fix35
--------------
Account-first channel registration and stable GUI identity.

1. Account-only channel add
- SOOP account ID or supported channel URL is the only required identity input.
- The GUI queries SOOP station status and fills the broadcaster nickname.
- Existing manually stored names remain unchanged unless the user explicitly
  requests a name refresh from the edit dialog.
- Network lookup failure uses the normalized account ID as a safe temporary
  display name; a confirmed nonexistent account is rejected.

2. Validation and compatibility
- Duplicate account IDs are rejected case-insensitively.
- Raw channel rows accept an empty NAME and fall back to ACCOUNT.
- Existing Y|NAME|ACCOUNT|OUTDIR files remain compatible.
- Account IDs and supported SOOP channel URLs are normalized before saving.

3. Stable account-keyed dashboard state
- Backend dashboard/progress/control lines include [account=SOOP_ID].
- GUI statusMap correlation now uses account ID rather than mutable nickname.
- Same-name broadcasters and nickname refreshes no longer merge dashboard state.
- Legacy output without the account marker retains a name-based fallback.

4. Runtime live-name fallback
- When a saved fallback NAME equals ACCOUNT, a later LIVE response may promote
  BJNICK for the current watcher runtime.
- Custom and previously stored display names are not overwritten automatically.

All fix34-fix2 channel multi-selection, unsaved-change protection, atomic
channel saves, account-based control commands, and recorder safety remain.


preview1-fix36
--------------
Settings usability/safety overhaul and channel edit/save state corrections.

1. Recording and path settings
- Added folder Browse, Open, and write-test actions for recording and log paths.
- The recording path shows its drive, current free space, and configured limit.
- The UI explains that a per-channel directory overrides the default path.
- Common recording/login settings remain visible; Worker, Streamlink,
  monitoring/retry, and log/display settings are grouped under Advanced.

2. Authentication and dependency checks
- SOOP password and Worker API Key now show whether a saved value exists.
- Blank secret fields preserve saved values; explicit Delete actions stage
  deletion, removing the former keep/delete ambiguity.
- Added SOOP login, Worker health/API-key, Streamlink version, and folder tests.
- Worker request quality is described separately from final recording quality.

3. Settings change safety
- Added a fixed bottom dirty-state bar with Discard and Save actions.
- Save is enabled only after a setting changes.
- Leaving the page with pending settings requires an explicit decision.
- Each section shows its apply timing and recommended numeric values.
- Added section-level restore actions and relationship validation; stall time
  must be at least twice the recorder monitor interval.
- Added INI export/import. Export can remove secrets for sharing; import shows
  whether secrets are present, requires confirmation, and preserves a .bak.
- Save confirmation summarizes immediate, next-recording, and restart effects.

4. Sequential channel edit fix
- Programmatic raw-editor synchronization is no longer treated as a new user
  edit, including deferred TextChanged delivery.
- Raw text is reparsed during Save only when the user actually edited it.
- Selection is captured/restored by stable account ID across necessary list
  rebuilds and after a successful channel edit.
- Users can edit the same or different channels repeatedly; all edits remain
  pending until the explicit Change Save action.

5. Channel dirty state after Save
- Change Save normalizes/writes the channel file, clears both raw/table dirty
  state, and restores the selected rows.
- A programmatic raw TextBox update can no longer turn the saved indicator back
  into an unsaved indicator after the write completes.

All fix35 account-first registration/name lookup/account-keyed dashboard state,
fix34-fix2 multi-selection and data-loss protection, and earlier recorder
safety/performance behavior remain.


Planned roadmap after fix36 (not implemented)
---------------------------------------------

Priority 1 - state correctness and stop reliability

1. Channel dirty indicator after Change Save
- The yellow unsaved indicator can still return after a successful channel
  save and must be treated as an unresolved fix36 bug.
- Normalize CRLF, LF, and CR before comparing raw-editor content.
- After the atomic write succeeds, capture the actual TextBox content as the
  saved snapshot and clear raw/table dirty state together.
- Programmatic table-to-raw synchronization must never raise a user-edit dirty
  transition, including deferred TextChanged delivery.
- Save must finish with "✓ 저장된 상태" and the Save button disabled until a
  real user edit occurs.

2. Watcher exit dashboard reconciliation
- The current UI can show "Watcher 중지됨" while stale REC cards and an active
  count remain. When the backend exits, clear/reconcile active cards, count,
  selection, and active-drive disk data.
- Show "중지됨" for exit code 0; show an error state/code only for abnormal
  exit. Offline counts shown while stopped must be marked as stale or replaced
  by "-".

3. Reliable selected-channel stop
- Backend STOP_ONCE must report separate accepted/completed/failed events and
  must not ignore Stop-ChannelRecording's Boolean result.
- Remove the card from RECORDING only after recorder termination is confirmed.
- On failure, keep the card with a clear error and Retry action.
- Preserve SuppressedBno for the current broadcast and never affect another
  channel recorder or the Watcher.

4. Separate stopped-channel flyout and natural Korean copy
- A successfully stopped channel disappears from the active recording list.
- Add a separate summary/flyout so RESUME_ONCE remains available.
- Final Korean UI copy:

  Summary title:
    "직접 중지한 채널"

  Item detail:
    "문월:-) · 지금 방송은 자동으로 다시 녹화하지 않습니다."

  Action:
    "녹화 다시 시작"

- Avoid awkward copy such as "사용자가 중지한 방송" or "현재 방송 재녹화
  억제 중".

5. Account-keyed progress and actionable health states
- The 250 ms progress coalescing dictionary must use account ID, not nickname,
  so same-name channels cannot overwrite each other's progress.
- Parse and display LOW DISK, DISK UNKNOWN, CHECK ERROR, authentication errors,
  and Worker errors as structured dashboard warning/error states.

Priority 2 - dashboard and channel-management UX

6. Natural dashboard empty states
- Watcher running, no active recordings:

    "현재 녹화 중인 방송이 없습니다."
    "등록된 채널 7개의 방송 상태를 확인하고 있습니다."

- Watcher stopped:

    "현재 채널 확인이 중지되어 있습니다."
    "방송 상태를 확인하려면 Watcher를 시작해 주세요."
    [Watcher 시작]

- Use the enabled-channel count in the running message, not a hard-coded value.

7. Dashboard clarity and responsive layout
- Rename the global button to "Watcher 중지" so it is not confused with
  "선택 채널 녹화 중지".
- Add a meaningful empty-state panel instead of a large blank area.
- Make summary/recording cards adapt to narrower windows and show the last
  successful status-update time when information may be stale.

8. Channel management
- Add nickname/account search and active/inactive/path filters.
- Replace raw True/False display with natural active/inactive labels and add
  column headers, double-click Edit, copy actions, and responsive overflow.
- Collapse the raw editor by default as an Advanced tool and show parse errors
  with exact line numbers.
- Add selected/all-channel SOOP name refresh with a preview of changed names.
- Add folder Browse/Open actions to the per-channel output-directory editor.

9. Recent recordings and logs
- Add a recent-recordings view with channel, finish reason, duration, size,
  file/folder actions, and a bounded history.
- Replace the plain latest-50-lines log box with structured severity/channel
  filters, search, auto-scroll control, copy/export, and log-folder access.

Priority 3 - long-term reliability and security

10. Machine-readable backend events
- Keep human-readable console output, but emit separate JSON events for the GUI
  with event type, account ID, BNO, status, paths, metrics, and error details.
- Stop relying on multiple regular expressions for critical state transitions.

11. Protected credentials
- Move SOOP password and Worker API Key from plaintext INI storage to Windows
  Credential Manager or DPAPI while preserving a safe migration path.

12. Regression tests
- Add automated tests for CRLF/LF/CR parsing, dirty/saved transitions,
  account-keyed progress, stop ACK success/failure, Watcher exit cleanup,
  BNO suppression/resume, filename collisions, and low-disk behavior.

Suggested fix37 scope:
1) dirty indicator after channel Save,
2) Watcher-exit dashboard cleanup,
3) stop completed/failed ACK,
4) remove stopped cards plus a separate stopped-channel resume flyout,
5) account-keyed progress coalescing,
6) structured low-disk/check/auth/Worker states,
7) natural empty-state copy and channel search/filter.


fix37 implemented changes
-------------------------

1. Channel saved-state correctness
- CRLF, LF, and CR are normalized before comparisons.
- A successful atomic write captures the actual editor content as the saved
  snapshot and clears raw/table dirty state together.
- Deferred programmatic TextChanged events no longer restore the yellow
  unsaved indicator after Change Save.

2. Dashboard lifecycle and natural Korean copy
- Watcher exit clears recording/offline/stopped/alert cards, selections,
  queued progress, account status, and stale disk information.
- Normal exit displays "중지됨"; abnormal exit displays its error code.
- The running and stopped empty states use the agreed natural Korean wording,
  and the global stop button is explicitly named "Watcher 중지".

3. Reliable per-channel stop and resume
- STOP_ONCE emits REQUESTED, COMPLETED, or FAILED based on the recorder process
  termination result. A failed stop clears broadcast suppression.
- A channel leaves RECORDING only after COMPLETED. It moves to a separate
  "직접 중지" flyout with the detail "지금 방송은 자동으로 다시 녹화하지
  않습니다." and the action "녹화 다시 시작".

4. Account-first progress and health visibility
- Progress coalescing uses account ID when present, preventing same-name
  channels from overwriting one another's latest progress line.
- Low disk, disk-check failure, broadcast-check failure, and login-required
  events include account IDs and appear in a separate dashboard alert flyout.

5. Channel-management usability
- Search supports channel name and account ID.
- Filters cover all, active, inactive, and per-channel output-path rows.
- True/False and blank paths are shown as natural active/inactive and
  default-path labels.
- The raw editor is collapsed by default as an advanced tool.

fix38 implemented changes
-------------------------

1. Channel import
- "채널 가져오기" accepts TXT/BAK/CSV files using the existing pipe-delimited
  channel schema and reports exact invalid or duplicate line numbers.
- The preview separates new and existing account IDs. Users can add only new
  channels or also update existing channel name/enabled/path values.
- Cancel leaves the list untouched. Applied imports remain yellow unsaved
  changes until the explicit "변경 저장" action.

2. Reliable Watcher start and natural wording
- Header, dashboard, and tray starts share one guarded start state. Both start
  buttons are disabled during startup, and an already-running process is never
  reported as a failed start.
- BackendProcessService captures each process instance in its exit handler and
  cleans up a partially initialized process instead of leaving it running.
- "backend 폴더 열기" is now "프로그램 폴더 열기".

3. UI and code optimization
- Channel list replacement/import/delete paths batch collection notifications
  and rebuild search/filter results once.
- Repeated OFFLINE and identical alert rows no longer re-sort/recount the whole
  dashboard. Count text and log text update only when their value changed.
- Frequently requested status brushes are cached, common progress/log rows use
  cheap text routing before regex parsing, and queue accounting is balanced.
- The unused legacy BuildSettingsView implementation was removed; the fix36+
  settings partial remains the single active settings UI.

4. Backend optimization
- Channel files are parsed only when LastWriteTime changes while the cached
  channel set still drives disable/remove retry reconciliation.
- Hot-setting metadata checks run once per second rather than every 500 ms.
- Control commands use an account-ID index instead of scanning every state.
- Dashboard progress reuses the recorder monitor's latest file-size sample,
  and PowerShell hot-path collections use generic lists instead of array +=.

5. Build and file optimization
- BUILD_EXE and BUILD_SELFCONTAINED automatically run SYNC_PROJECT.ps1, so the
  latest overlay/backend and VERSION always reach the generated project.
- PREPARE_PROJECT skips redundant template installation when WinUI is ready.
- Runtime publish folders omit PDB files; source ZIPs still exclude generated
  and publish directories.

Remaining roadmap after fix38
-----------------------------
- Worker-specific structured alert events and JSON GUI events.
- Bulk SOOP name refresh with preview, recent-recording history, structured
  log search/filter/export, and responsive narrow-window layouts.
- Protected Windows credential storage and automated regression tests.

fix39 implemented changes
-------------------------

1. Channel command layout and visible save action
- The long single-line toolbar is split into primary commands, a fixed-right
  "변경 저장" action, selection controls, and raw-editor-only advanced tools.
- Enable, disable, and delete commands now live under "선택 작업", reducing
  clutter without removing functionality.
- Search/filter and select-all controls have their own responsive row, so the
  save action is no longer pushed outside the window.

2. Compact navigation that remains readable
- Dashboard, channel management, settings, and logs now have distinct icons.
- Closing the navigation pane switches to a 52-pixel icon rail instead of
  clipping portions of Korean menu labels. Each item keeps a tooltip and an
  accessible name.

3. Consistent action sizing
- User-facing action buttons share a 34-pixel minimum height, predictable
  horizontal padding, and role-based minimum widths.
- The Settings save bar now stretches across the content area. Export/import
  and discard/save use matched sizes, while "설정 저장" is the primary action.

4. Watcher state reconciliation
- Header, dashboard, and tray entry points continue to use the same backend
  process service. A 250 ms UI reconciliation checks the real process state.
- When backend activity proves Watcher is running, stale stopped text and a
  disabled top-right stop button repair themselves automatically.
- Reconciliation is suspended during a requested shutdown; only the actual
  backend exit transitions the UI to the stopped state.

Remaining roadmap after fix39
-----------------------------
- Worker-specific structured alert events and JSON GUI events.
- Bulk SOOP name refresh with preview, recent-recording history, and structured
  log search/filter/export.
- Protected Windows credential storage and automated regression tests.

fix40 implemented changes
-------------------------

1. Actionable Watcher startup failures
- Header, dashboard, and tray starts now wait for a backend-ready signal,
  immediate exit, or a bounded still-initializing result instead of treating
  one fixed 300 ms sample as the complete startup result.
- BackendProcessService publishes process ownership before launch completion,
  preserves the latest redirected stdout/stderr lines, and retains the exit
  code even when PowerShell exits immediately.
- A failed start now shows the exit code and recent backend output. The same
  details are appended to SOOPLiveWinUI_startup.log, and an empty exception can
  no longer produce a blank dialog.
- The normal dashboard reset may clear stale status rows, but it can no longer
  destroy the separate startup diagnostic buffer.
- A temporary initial SOOP login/network failure is now non-fatal. The Watcher
  starts without login, records a warning, and can retry authentication later
  when a channel actually requires it.

2. Adaptive Settings layout
- Settings cards now stretch to the available content width instead of being
  fixed to a left-aligned 920-pixel column.
- Text, password, quality, and path controls resize with their cards. Path rows
  use star-sized fields with fixed action buttons, preventing wasted space and
  reducing clipping at narrower widths.
- Monitoring fields switch from two columns to one column below 620 pixels.
- Horizontal scrolling is disabled so the page reflows inside the visible
  viewport while retaining the fixed bottom save bar.

Remaining roadmap after fix40
-----------------------------
- Worker-specific structured alert events and JSON GUI events.
- Bulk SOOP name refresh with preview, recent-recording history, and structured
  log search/filter/export.
- Protected Windows credential storage and automated regression tests.

fix41 implemented changes
-------------------------

1. Watcher start reset reliability
- Dashboard, header, and tray Watcher starts no longer mutate the SelectedItems
  collection of single-selection recording lists during dashboard reset.
- Reset now clears SelectedItem directly, preventing the WinRT illegal-method
  exception that could stop startup before PowerShell was launched.
- Empty startup exception messages now include the exception type and HRESULT,
  while full details continue to be written to SOOPLiveWinUI_startup.log.

fix42 implemented changes
-------------------------

1. Accurate Watcher stop state
- A non-zero PowerShell exit code caused by terminating the GUI-owned process
  tree is now shown as a normal stop when it follows an explicit user request.
- Unexpected backend exits without a pending stop request still retain the
  existing error-exit status and exit code.

2. Typed numeric setting changes
- NumberBox keyboard edits now mark Settings dirty immediately, enabling Save
  before focus leaves the field. ValueChanged remains in place for committed,
  spin-button, paste, and programmatic value changes.

fix43 implemented changes
-------------------------

1. Verified Watcher termination
- Backend process-tree termination now returns success/failure to the GUI.
- The GUI reports a stop-confirmation warning when the exact owned process tree
  could not be confirmed stopped instead of treating intent alone as success.

2. Validated numeric settings
- NumberBox dirty tracking now follows actual Text changes rather than every
  KeyUp, covering paste and accessibility input without navigation-key noise.
- Empty, invalid, and out-of-range numeric values are rejected before saving;
  they are no longer silently replaced with defaults.
- Numeric INI values are written with invariant-culture formatting.

fix44 implemented changes
-------------------------

1. Open an active recording folder
- Selecting a recording now enables a compact Folder Open action beside the
  existing per-channel Stop action in the dashboard header.
- The action uses the actual output file path reported by the backend, opens
  only its existing parent directory, and shows a clear error when the path is
  not available instead of creating or guessing a folder.

fix45 implemented changes
-------------------------

1. Configurable tray notifications
- Settings can independently enable recording-start, recording-finished, and
  actionable warning notifications. Defaults avoid noisy start notifications
  while retaining completion and important failure/disk/auth warnings.

2. Recording card context actions
- Right-clicking a recording card offers Folder Open, Select File, Copy Path,
  and Stop Current Recording without adding permanent dashboard button clutter.
- All path actions use the backend-reported output path.

3. Conservative disk-time estimate
- The disk summary combines actual free space with the summed real file-growth
  rates of active REC items on each output drive.
- It subtracts the configured minimum-free-space reserve and displays a stable
  tier such as under one hour, approximate hours/days, or three days or more.
- PAUSED and restart-waiting items remain excluded from disk calculations.

fix46 implemented changes
-------------------------

1. Deterministic recording-finish lifecycle
- The backend emits recording-finished and channel-removed/disabled events with
  stable account IDs on one line so the GUI can safely correlate concurrent
  recorder output.
- Finished, removed, and disabled recordings are removed from the active REC
  collection immediately; queued stale progress is discarded at the same time.

2. Recoverable alerts and disk cleanup
- Recorder exits, stalls, low disk, and restart failures remain visible under
  Needs Attention while a retry is pending.
- The alert is cleared as soon as deterministic progress or a new recording
  start proves that downloading resumed.
- Only current REC items contribute to free-space and remaining-time summaries,
  so removed or stopped channels cannot leave stale drive estimates behind.

fix47 implemented changes
-------------------------

1. Bounded backend-to-UI memory
- Progress is coalesced by stable account on the producer thread before it can
  enter the event queue, so a blocked UI retains only one sample per channel.
- Routine backend events use a bounded 2,000-line queue. Old lines are released
  under sustained overflow and a single dropped-line summary is shown, while
  lifecycle/disk/stop events use a separate priority queue and are never dropped.

2. Lower hot-path allocation and I/O
- Parsed numeric transfer rates are retained on ChannelStatus and reused by the
  disk estimator instead of parsing the formatted UI rate every refresh.
- Alert-only count changes no longer trigger output-drive free-space queries.
- GUI log lines are bounded, consecutive duplicates are skipped, and multiline
  HTML/JSON fragments are excluded from the event-oriented log.

3. Deterministic resource cleanup
- Backend event subscriptions now use named handlers and are removed when the
  window closes. Timers, bounded queues, progress samples, dashboard maps, and
  log references are cleared without forcing GC or broad process termination.

fix48 implemented changes
-------------------------

1. Worker endpoint circuit breaker
- Two complete failed Worker request cycles open a shared endpoint circuit.
- Cooldown grows through bounded 30, 60, 120, and 300 second tiers. Existing
  recordings continue; only new playlist acquisition is delayed.
- A successful probe or saved Worker URL/API-key change resets the circuit.

2. Compact recovery diagnostics
- External HTML/JSON errors are reduced to a whitespace-normalized 300-character
  summary before reaching stdout and the GUI event log.
- During an open circuit, each affected channel receives a stable account-tagged
  WORKER COOLDOWN event and schedules its next check at the cooldown boundary.
- Needs Attention shows Worker recovery wait and clears normally when a new
  recording start/progress proves recovery.

GitHub and Codex cloud preparation
----------------------------------
- Generated projects, publish output, logs, runtime control files, local INI
  credentials, and personal channel lists are excluded by .gitignore.
- Commit the `.example` setting and channel files, never the corresponding
  local runtime files without the `.example` suffix.
- A clean clone seeds missing runtime files from the examples during prepare,
  synchronization, and publish. Private values must be entered locally.
- Codex cloud can edit and review this repository, but the final WinUI 3 build
  and EXE test must run on Windows. See CLOUD_SETUP.md.

fix49 implemented changes
-------------------------

1. Wildcard-safe recording growth watchdog
- The watchdog and RECORD FINISHED size calculation now use PowerShell
  `-LiteralPath` for the generated output filename.
- Broadcast titles containing valid filename characters such as `[` and `]`
  are no longer interpreted as wildcard patterns. Their real file growth is
  detected instead of remaining at a false `0 B` and restarting every 90 seconds.
- The configured 90-second genuine no-growth recovery, immediate LIVE recheck,
  collision-safe naming, and exact owned-process termination remain unchanged.

fix50 implemented changes
-------------------------

1. Versioned backend event protocol
- Critical lifecycle events now emit `@@SOOP_EVENT@@` JSON version 1 records for
  recording start/finish/stall, low disk, Worker cooldown, and channel removal
  or disable. Human-readable lines remain for users and older GUI fallback.
- `BackendEventParser` owns JSON and legacy text parsing, preserving Korean and
  delimiter characters such as `[`, `]`, `|`, and `=` without correlation loss.
- Structured lifecycle lines use the priority UI queue and recent-event
  suppression prevents the paired legacy line from applying the same action twice.

2. Automated protocol regressions
- A dependency-free .NET 8 console test covers every introduced JSON event,
  legacy fallback parsing, malformed/version-mismatched JSON, Korean text, and
  special-character names, titles, and paths.
- A PowerShell regression verifies literal-path size checks for wildcard-like
  filenames and guards against reintroducing non-literal watchdog access.

3. Bounded redacted recorder diagnostics
- Unexpected recorder exits and genuine RECORD STALLED stops retain only the
  final 50 stderr lines under `backend\logs\recorder-diagnostics`.
- Authorization, cookie, password, API-key, AID, and token-shaped values are
  redacted before writing; only the newest 20 diagnostic files are retained.
- Normal recording completion, user stop, channel removal, and low-disk stops
  continue deleting temporary recorder console files without diagnostic churn.

fix51 implemented changes
-------------------------

1. Recent recording history
- A dedicated Recent Recordings page keeps the newest 200 completed/interrupted
  recordings in an atomically replaced local JSON file under `backend\history`.
- Users can open the folder, select an existing file, copy its path, or clear
  history without deleting any recording file.

2. Actionable Needs Attention cards
- Alert cards now show a timestamp, structured status/detail, and a recommended
  action. Selected alerts offer immediate recheck, recording-folder, Settings,
  and Logs shortcuts.
- RECHECK uses the existing GUID temporary-command-to-`.cmd` claim protocol and
  only schedules that exact account for an immediate LIVE check.

3. Safe diagnostics and channel-name preview
- Logs now provides one-click diagnostic copy with app/runtime/watcher state and
  the bounded GUI event tail, after credential and token-shaped values are redacted.
- Channel Management can check selected channels (or all when none are selected)
  in bounded batches, preview old/new names and failures, and stage confirmed
  changes without saving until the existing atomic Save action is used.

4. Advanced-settings dirty-state correction
- Expanding or collapsing Advanced Settings suppresses NumberBox layout/format
  callbacks across the UI transition. An existing real edit remains dirty, while
  opening an untouched panel no longer asks the user to save unchanged settings.

fix52 implemented changes
-------------------------

1. Windows DPAPI credential protection
- New Settings saves protect SOOP_PASSWORD and CLOUDFLARE_API_KEY with Windows
  DPAPI CurrentUser scope and an application-specific entropy value. INI files
  contain only `dpapi:v1:` ciphertext; secret fields remain blank in the GUI.
- Legacy plaintext remains readable for one-way migration on the next explicit
  save/import. The watcher decrypts only in memory and fails safely when another
  Windows user or damaged ciphertext cannot be unprotected.
- Settings backups created during GUI or CLI writes are also migrated to DPAPI,
  so a legacy plaintext value is not retained in the newly written .bak file.
- Shared exports exclude secrets by default; optional full exports contain only
  current-user DPAPI ciphertext and imports re-protect secrets before file write.

2. Completed role boundaries
- MainWindow remains programmatic WinUI composition and thin event coordination;
  backend event parsing/process ownership, Settings, imports, recent-history
  persistence, diagnostics construction, and DPAPI are isolated in dedicated files.
- Recent history validation/atomic replacement and diagnostic redaction no longer
  live in the Window partial, reducing UI lifecycle coupling and test surface.

3. Feature-based PowerShell modules
- SOOP_LIVE.ps1 is now orchestration-only and fail-fast dot-sources Security,
  Core/config, Network/auth/Worker, and Recorder/control modules.
- Generated-project synchronization now recursively copies backend modules and
  hash-verifies every required file. Compact/self-contained builds verify the
  main watcher and all modules without broad process or path fallback changes.

4. Windows build and publish CI
- A windows-latest workflow runs parser/DPAPI and PowerShell regressions, prepares
  the official WinUI template, invokes BUILD_EXE.bat, verifies unpackaged compact
  properties and publish/backend modules, and uploads the win-x64 artifact.

fix53 implemented changes
-------------------------

1. Windows source-test CRLF correction
- The distributed-secret invariant now requires a real non-line-ending character
  after `=`. Empty SOOP_PASSWORD/CLOUDFLARE_API_KEY defaults no longer become
  false positives because `.` consumed the CR in Windows CRLF files.

2. Normal recorder-exit classification
- Recorder processes are given a final WaitForExit/Refresh before reading the
  exit code. Code 0 and the Windows/PowerShell unavailable-code case are emitted
  as NORMAL; known non-zero codes retain diagnostics and confirmation alerts.
- The GUI also normalizes legacy `RECORDER EXIT CODE=` and code 0 finish events
  to NORMAL, clears any stale channel alert, migrates matching recent-history
  reasons, and keeps non-zero exits actionable.
- Parser regressions cover empty, zero, unavailable, and non-zero finish reasons.

fix54 implemented changes
-------------------------

1. Fresh-machine WinUI template bootstrap
- PREPARE_PROJECT now treats `dotnet new list winui` returning no templates as
  an expected probe result even while the rest of the script remains fail-fast.
- The probe captures native output/exit code under a narrowly scoped Continue
  policy, restores ErrorActionPreference in finally, and then installs the
  official Microsoft WinUI C# template pack.
- A source regression protects the scoped error-policy restoration, native exit
  capture, and official template installation fallback used by Windows CI.

fix55 implemented changes
-------------------------

1. Clear channel enabled/disabled visuals
- Channel rows now use separate green/neutral badges, card backgrounds, borders,
  name colors, and opacity so disabled entries are distinguishable at a glance.

2. Channel-name preview actions
- The bulk name lookup preview now exposes three explicit choices: apply changes,
  acknowledge the result without applying, or cancel.

3. Distinct recent-recordings navigation
- Recent recordings now uses the Video symbol while Logs retains Document.

4. Advanced Settings dirty stabilization
- First-time NumberBox formatting after import/save can be deferred until the
  collapsed advanced panel is measured. A scoped 500 ms layout transition guard
  now absorbs those programmatic callbacks without clearing pre-existing edits.
- Source regressions protect all four UI behaviors.

fix56 implemented changes
-------------------------

1. Windows PowerShell 5.1-safe source regressions
- PowerShell 5.1 reads BOM-less scripts through the active ANSI code page. Korean
  regex literals in UserFeaturesSource.Tests could therefore become invalid
  tokens before any assertion ran.
- Korean UI labels are now reconstructed from Unicode code points inside an
  ASCII-only test script; the recent-navigation assertion uses its stable tag.
- The literal-path Korean filename fixture is also constructed from code points,
  and SecurityAndModules.Tests rejects future non-ASCII PowerShell test sources.

fix57 implemented changes
-------------------------

1. Modern design foundation
- DesignTokens centralizes semantic surfaces, text, borders, status colors,
  spacing, radii, control sizing, high-contrast fallbacks, command buttons,
  and keyboard accelerators without changing the programmatic overlay model.
- Watcher and Settings primary actions now share the common control treatment.

2. Responsive channel and recent-recording views
- Channel and recent-recording ListViews switch templates only when their host
  crosses a compact/wide width threshold. Normal progress never rebuilds them.
- Compact cards stack secondary metadata while wide cards retain column layouts.

3. Command surfaces and accessibility
- Channel management and recent recordings use CommandBar primary/overflow
  actions instead of fixed horizontal button rows.
- High-contrast mode avoids disabled-row opacity, recent cards use WinUI theme
  resources, command items expose automation names/tooltips, and state badges
  keep text labels in addition to color.
- Keyboard access includes Ctrl+R/Ctrl+Shift+R for Watcher, Ctrl+N/Ctrl+S/Ctrl+F
  for channel management, and Ctrl+S for Settings.
- Source regressions protect design tokens, responsive templates, CommandBar,
  high-contrast, and accelerator wiring.

fix58 implemented changes
-------------------------

1. Debounced channel search
- Channel-name/account typing waits for a quiet 250 ms interval before applying
  the filter. Filter dropdown changes remain immediate.

2. Minimal visible-channel synchronization
- Filtering no longer clears and rebuilds VisibleChannelItems. Removed rows are
  deleted, new rows inserted, and retained rows moved only when their position
  actually changes. Reapplying the same filter emits no collection mutations.
- Retained selections are restored by stable account ID, and the first visible
  row is used as a scroll anchor across filter updates.

3. Large-list regression
- A dependency-free 10,000-channel regression validates filtering, no-op refresh,
  ordering, reference preservation, and move-only reordering behavior.

fix59 implemented changes
-------------------------

1. Snapshot-based Settings dirty state
- Settings controls are normalized into a deterministic snapshot after load and
  save. Dirty state now means the current snapshot actually differs, so changing
  a value back to its saved value clears the Save prompt automatically.
- Secret replacement/deletion intent and invalid in-progress NumberBox text are
  included without exposing stored secrets.

2. Atomic per-user UI state
- Close behavior, last tab, window size, and UI density now live under LocalAppData
  instead of the publish directory. Writes use validated temporary JSON followed
  by atomic replacement, with one-time migration from the legacy adjacent file.
- Window resize writes are debounced, and the last tab and valid saved dimensions
  are restored at startup. Compact, normal, and comfortable density are available
  in Settings and apply to design-token spacing on the next full UI construction.

3. Background recent-history coalescing
- Recent-recording snapshots are copied on the UI thread, coalesced for 250 ms,
  and atomically written by one background worker. Shutdown unsubscribes backend
  events and flushes the newest pending snapshot before clearing UI collections.
- Automated regressions cover deterministic Settings snapshots, LocalAppData
  placement, and a 100-update burst collapsing into one physical history write.

fix60 implemented changes
-------------------------

1. Bounded priority and warning paths
- Critical backend events now use a newest-retaining priority queue capped at
  512 entries. Overflow is counted and reported instead of growing indefinitely.
- Identical non-critical warning/retry lines are deduplicated for 30 seconds
  after timestamp normalization, with a merged-line summary in the GUI log.

2. Lower-allocation progress snapshots
- Producer-side progress parsing now stores a small value-type snapshot containing
  only account, name, size, duration, and rate. UI flushing no longer creates a
  ConcurrentDictionary ToArray snapshot or reparses the original console line.
- At most 200 channel progress snapshots are applied per dispatcher tick.

3. Cached drive information
- Available-space queries are cached per output root for five seconds, including
  safe negative results. Recording cards and the disk estimate reuse that cache.
- Cache and deduplication state are cleared on Watcher reset and window shutdown.

4. Virtual long-duration soak regression
- A dependency-free virtual 24-hour/64-channel test exercises 86,400 progress and
  warning ticks, four output roots, and a 100,000-event priority burst. It asserts
  hard queue bounds, warning suppression, channel-bounded progress, and reduced
  drive queries.

fix61 implemented changes
-------------------------

1. Isolated VOD pipeline
- Added a non-interactive, request-file-driven VOD backend under backend/vod.
  Core, authenticated-cookie, yt-dlp download/resume, and ffmpeg concat logic
  are separate modules and are never loaded by the LIVE watcher bootstrap.
- VOD temporary cookies and concat metadata live in a per-job LocalAppData
  directory and are removed after completion. Authentication output and URL
  query strings are redacted from failure events.

2. WinUI VOD workspace
- Added a dedicated VOD navigation page with URL/output/PART/cookie controls,
  structured version-1 VOD event parsing, progress display, and cancellation.
- VOD uses its own process service and exact owned process-tree termination;
  Watcher start/stop remains connected only to the existing LIVE service.
- VOD settings use validated atomic LocalAppData JSON and do not participate in
  SOOP_LIVE_SETTING.ini hot reload or Settings dirty tracking.
- Completed VOD jobs are retained in a separate bounded, atomically replaced
  LocalAppData history without storing cookie contents or authentication data.

3. Packaging and regressions
- Prepare, sync, compact/self-contained publish, and Windows CI now verify the
  VOD entry script and all required modules in addition to the LIVE backend.
- Dependency-free regressions cover PART ranges, malformed/versioned JSON,
  Korean/special-character fields, module isolation, literal paths, redaction,
  resume flags, and exact process ownership.

fix62 implemented changes
-------------------------

1. Stored SOOP login for VOD
- VOD now defaults to a stored-login mode that reads SOOP_USERNAME and the
  DPAPI-protected SOOP_PASSWORD from SOOP_LIVE_SETTING.ini inside the isolated
  VOD process. Credentials, DPAPI ciphertext, and cookie values are never added
  to the GUI request JSON.
- The VOD process creates and verifies its own HttpClient/CookieContainer login
  session, exports only SOOP-domain cookies to a per-job Netscape cookie file,
  and never shares the LIVE process or its in-memory CookieContainer.
- FILE and BROWSER cookie modes remain available as explicit fallbacks.

2. Short-lived subscription authorization renewal
- private_auth.php is called inside every PART download attempt, so each retry
  obtains a fresh short-lived authorization cookie before yt-dlp resumes its
  existing partial download.
- On retries, stored-login mode first creates a new independent SOOP login
  session; browser mode re-exports the browser cookie. Exponential backoff is
  bounded and failures preserve the partial download for yt-dlp continuation.
- Per-job cookies are deleted on success, failure, cancellation, and window
  shutdown together with the private job directory.

3. UI and regressions
- The VOD page shows stored SOOP login as the default mode, hides irrelevant
  cookie-path controls, and reports whether Settings contains both login fields.
- Regressions assert DPAPI credential resolution, independent login, Netscape
  export, request secrecy, retry-loop authorization refresh, and cleanup.

fix63 implemented changes
-------------------------

1. Windows PowerShell 5.1 VOD encoding fix
- Every VOD PowerShell entry/module file that contains Korean UI or diagnostic
  text is now stored as UTF-8 with BOM. Windows PowerShell 5.1 therefore no
  longer decodes those files through the active ANSI code page and corrupts
  quoted strings into cascading parser errors.

2. Encoding and parser regression guard
- The VOD source regression now reads the raw bytes of the entry script and all
  four modules, requires the EF BB BF UTF-8 BOM, and invokes the native
  System.Management.Automation parser for every file before other assertions.
- The guard runs before the behavioral Netscape-cookie regression in Windows CI,
  so an encoding or syntax regression fails with the exact affected file and
  first parser message.

fix64 implemented changes
-------------------------

1. End-to-end UTF-8 VOD subprocess contract
- VodProcessService now configures UTF-8 input/output inside Windows PowerShell,
  decodes redirected stdout/stderr as UTF-8, and sets Python/yt-dlp UTF-8
  environment variables. The VOD entry script repeats the console contract as a
  defensive fallback.
- A Windows-only regression launches Windows PowerShell from a Korean and
  wildcard-containing path and verifies Korean structured stdout, Korean stderr,
  and secret redaction round-trip through VodProcessService.

2. Actionable Cookie/auth failures
- yt-dlp flat-playlist metadata can omit the top-level uploader_id. VOD now
  falls back to the first PART uploader_id/uploader/upload_date before calling
  private_auth; an empty strm_id had caused both stored-login and FILE-cookie
  authorization to fail even when their cookies were valid.
- FILE mode validates that the supplied file contains a real seven-field
  Netscape cookie row and reports how to obtain a compatible cookies.txt instead
  of failing later with a generic exit code.
- private_auth failures retain a bounded redacted response/exit summary, and the
  VOD page now preserves the structured failure or redacted stderr tail beside
  the exit code instead of replacing it with only "VOD job exited (1)".

3. Isolated yt-dlp diagnostics and locale-safe progress
- Metadata JSON stdout is no longer merged with stderr. Per-operation stderr is
  kept in the private job directory, reduced to a redacted tail for errors, and
  deleted immediately after use (or with the job directory on cancellation).
- PART stderr is likewise separated from progress stdout. Percentage values are
  parsed with invariant culture before they enter versioned JSON events.
- Source and behavior regressions cover the UTF-8 process boundary, JSON/stderr
  separation, invariant parsing, valid FILE cookies, and malformed-cookie
  rejection.

fix65 implemented changes
-------------------------

1. Configurable VOD tools and deterministic metadata
- The VOD page now persists optional yt-dlp and ffmpeg executable paths and
  passes only those non-secret paths to the isolated VOD request. Explicitly
  configured missing executables fail with an actionable message; empty fields
  retain the bundled/INI/PATH fallback order.
- yt-dlp metadata stdout is separated from stderr, materialized as a UTF-8 JSON
  file in the private per-job directory, read back explicitly as UTF-8, and
  removed immediately after parsing.

2. Unicode and external-tool path safety
- Generated VOD names and ffmpeg concat entries are normalized to Unicode NFC.
  Output paths are resolved to full paths and rejected above the conservative
  240-character interoperability limit before yt-dlp or ffmpeg starts.
- ffmpeg concat escaping is centralized and preserves Korean characters and
  apostrophes. Explorer launches now pass paths through ProcessStartInfo
  ArgumentList rather than hand-built quoted command strings.

3. Locale and path regressions
- Deterministic regressions cover ko-KR, en-US, and de-DE selection/event
  parsing, decomposed Hangul normalization, Korean/apostrophe concat paths,
  full-path rejection, configured tool-path serialization, and UTF-8 metadata
  source invariants.

fix66 implemented changes
-------------------------

1. Native executable browsing
- The optional yt-dlp and ffmpeg fields now include accessible Windows file
  picker buttons filtered to .exe files. The selected full path is retained by
  the existing atomic VOD settings store; manual input and automatic lookup
  remain available.

2. Subscription VOD 403 recovery
- Analysis showed that the previous retry loop renewed the SOOP login and
  private_auth cookie but kept using the m3u8 URL captured before the first
  attempt. That URL can expire independently for subscriber-only VODs.
- Every PART attempt now re-runs authenticated VOD metadata analysis to obtain
  its current URL, then calls private_auth.php with that URL immediately before
  yt-dlp. Retries also create a fresh isolated base session where applicable.
  This covers later PARTs whose URLs expire while earlier PARTs download.
- yt-dlp receives the same browser User-Agent, VOD Origin, Referer, and updated
  Netscape cookie jar. A manifest 403 is classified as authorization expiry so
  the UI explains that the login session, URL, and short-lived cookie are being
  renewed rather than showing only a generic downloader error.

3. Isolation and regression coverage
- The retry implementation remains wholly under backend/vod and does not load
  into or share state with SOOP_LIVE.ps1. Credentials and cookies remain outside
  request JSON and are deleted with the private job directory.
- A Windows PowerShell regression uses a deterministic fake yt-dlp 403 and
  verifies that the second attempt renews the base session, metadata URL, and
  private authorization. Source regression also guards picker and header wiring.

fix67 implemented changes
-------------------------

1. Windows PowerShell 5.1 native stderr retry fix
- The fix66 regression exposed that Windows PowerShell 5.1 can convert native
  yt-dlp stderr into a terminating NativeCommandError while the VOD entry script
  uses ErrorActionPreference=Stop. The exception bypassed exit-code inspection,
  403 classification, and the intended second authorization attempt.
- Metadata yt-dlp, download yt-dlp, and private_auth curl calls now use Continue
  only inside their native process boundary, capture the native exit code and
  redirected diagnostics, and restore the caller's error preference in finally.
  PowerShell errors outside those narrow native boundaries still fail fast.

2. Deterministic retry regression cleanup
- Mock counters no longer leak post-increment values into the PowerShell output
  pipeline, so the refreshed metadata object remains the only function result.
- Failure output now includes the observed failed/base/metadata/authorization
  counters, making any future Windows CI regression immediately diagnosable.

fix68 implemented changes
-------------------------

1. Stored-login VOD session warm-up
- The isolated SOOP_LOGIN CookieContainer previously performed login and account
  verification, then exported immediately. Unlike a browser session, it had not
  visited the requested VOD player before private_auth and could omit session
  state established by the player flow.
- The isolated HttpClient now validates and opens the requested HTTPS SOOP VOD
  URL after login verification and before Netscape export. Every base-session
  renewal repeats that player warm-up. The session remains VOD-job-local and is
  never shared with LIVE.

2. Actionable final download failure
- Per-attempt private_auth and yt-dlp details are retained in redacted form.
  Exhausting retries now reports the last real diagnostic after the PART number
  instead of replacing it with only "PART download failed".
- The deterministic 403 regression now requires the final exception to retain
  the 403 diagnostic as well as verifying all renewal counters.

fix69 implemented changes
-------------------------

1. CloudFront cookie scope repair
- The retained fix68 diagnostic confirmed that private_auth completed but the
  generic m3u8 manifest request still received HTTP 403. A signed CloudFront
  cookie can be present in the job jar yet remain scoped to a SOOP host instead
  of the CDN host contained in the current manifest URL.
- After each successful private_auth call, only CloudFront-Policy,
  CloudFront-Signature, CloudFront-Key-Pair-Id, and CloudFront-Expires are copied
  to an exact, secure cookie scope for the HTTPS manifest host when no matching
  scope exists. Login cookies such as AuthTicket are never copied to the CDN.

2. Manifest authorization preflight
- Before starting yt-dlp, the backend now requests the current m3u8 with the
  same job cookie jar, User-Agent, Referer, and Origin. A non-2xx response stays
  inside the existing bounded renewal loop instead of spending a full yt-dlp
  attempt with authorization that is already known to be unusable.
- The redacted final diagnostic reports only HTTP status, curl exit code, and
  manifest host. Cookie values and URL query strings are not emitted.

3. Regression coverage
- Cookie regressions verify that exactly the signed CloudFront cookies can be
  aliased to a manifest host. The deterministic 403 test stubs the preflight so
  it continues to isolate the retry state machine, and source tests require the
  scope repair and preflight wiring.

fix70 implemented changes
-------------------------

1. Signed-cookie FILE mode
- A cookies.txt containing only CloudFront-Key-Pair-Id, CloudFront-Policy, and
  CloudFront-Signature is now recognized as a pre-authorized CDN cookie set,
  not mistaken for a SOOP login session.
- Metadata discovery uses yt-dlp ignore-no-formats-error so the title, uploader,
  PART count, and source manifest URLs can be returned before the protected
  manifest is opened. The signed cookies are then scoped to the manifest host
  and preflighted directly. If a signed-only file expires, the UI asks for a
  fresh three-cookie export because it cannot renew without login cookies.

2. Two-phase VOD workflow
- The first action analyzes the VOD only. PART and quality controls remain
  disabled until the backend reports the title, streamer, actual PART count,
  and authorized master-manifest variants.
- After analysis, the PART field is validated against the discovered count and
  the action changes to download. Changing the URL, cookie mode/source, or
  yt-dlp path invalidates the analysis and requires a fresh check.

3. Quality selection
- The authorized master m3u8 preflight parses EXT-X-STREAM-INF resolution
  heights and returns highest-quality automatic plus available resolution
  choices. The selected format expression is passed to yt-dlp for every PART.
- Structured VOD events now carry a quality array; no Cookie value, password,
  DPAPI ciphertext, or signed URL is added to the GUI request/event protocol.


fix71 implemented changes
-------------------------

1. CloudFront cookie canonicalization
- Before private_auth.php renewal, expired CloudFront signed cookies are removed
  while SOOP login cookies remain in the isolated VOD job jar.
- After issuance, exactly one Key-Pair-Id, Policy, and Signature value is scoped
  to the current manifest host. This prevents curl and yt-dlp from sending stale
  and fresh cookies with the same name, which CloudFront rejects with HTTP 403.
- A mixed FILE/BROWSER jar with SOOP login cookies can renew an expired signed
  triplet during analysis; a signed-only jar instead reports that a fresh export
  is required because it has no login session from which to renew authorization.

2. yt-dlp metadata URL compatibility
- Manifest URL resolution now checks url, manifest_url, manifestUrl, hls_url,
  and format-level URL fields. This handles flat metadata where yt-dlp reports
  the PART successfully but leaves the top-level entry URL empty.
- If those fields are still empty, the authenticated SOOP mobile VOD metadata
  endpoint supplies the ordered data.files URLs without opening the protected
  m3u8. This avoids the metadata/authentication circular dependency.

3. Regression coverage
- Cookie tests now inject stale exact-host values plus fresh parent-domain values
  and require the resulting jar to contain only the fresh canonical triplet.
- The authorization regression verifies manifest_url fallback behavior.


fix72 implemented changes
-------------------------

1. Cookie-file bootstrap ordering
- CloudFront-Policy is decoded before yt-dlp metadata extraction. Its signed CDN
  resource is used to scope the three Cookie values before the first protected
  m3u8 request, eliminating the previous URL-before-auth circular dependency.
- An exact non-wildcard policy resource can also serve as the first PART manifest
  fallback when yt-dlp omits every URL field.

2. Exact CloudFront preflight
- Manifest preflight sends exactly Key-Pair-Id, Policy, and Signature through a
  temporary curl config instead of relying on Netscape domain matching. The
  config is deleted immediately and Cookie values remain out of request JSON,
  events, logs, and the process command line.

3. Regression coverage
- Tests validate CloudFront-safe base64 Policy decoding, pre-metadata FILE scope,
  explicit three-Cookie curl configuration, and PolicyResource propagation.


fix73 implemented changes
-------------------------

1. Multi-PART URL retention
- Per-attempt metadata still prefers a newly returned manifest URL, but an empty
  or shortened refresh can no longer erase the valid URL from initial analysis.
- This specifically allows PART 2 and later to continue when yt-dlp returns all
  URLs during analysis but omits a later entry after PART 1 completes.

2. Stored-login authorization capture
- Stored-login initialization removes incidental player-page CloudFront cookies
  so private_auth.php is always called for the actual extracted PART URL.
- private_auth response headers are captured and all three CloudFront Set-Cookie
  values are imported into the isolated job jar even when curl rejects their CDN
  domain for normal cookie-jar processing. Header files are deleted immediately.

3. Regression coverage
- Tests require initial PART URL preservation and deterministic Set-Cookie import
  for Key-Pair-Id, Policy, and Signature without exposing their real values.


fix74 implemented changes
-------------------------

1. Cookie file picker and stored-login compatibility
- FILE mode now shows a Windows .txt picker beside the Cookie path while browser
  mode keeps the same editable browser-name field.
- Stored login imports CloudFront credentials from either private_auth Set-Cookie
  headers or supported JSON key/policy/signature fields before manifest preflight.

2. Cancellation cleanup
- Every VOD target is registered in the private job directory before yt-dlp or
  ffmpeg starts. Backend finally cleanup and GUI forced-cancel cleanup remove only
  that job's .part, .ytdl, .temp, fragment files, and incomplete merge target.
- Completed individual PART files and a successfully completed merge are retained.

3. Merge input isolation
- Structured events now write directly to redirected stdout, not PowerShell's
  success pipeline. Invoke-VodDownloads therefore returns only real mp4 paths and
  ffmpeg no longer receives an @@SOOP_VOD_EVENT@@ JSON line as a PART filename.

4. Regression coverage
- Tests cover Set-Cookie and JSON authorization import, success-pipeline isolation,
  owned temporary-file cleanup, Cookie picker wiring, and merge completion state.


fix75 implemented changes
-------------------------

1. Windows PowerShell cancellation cleanup
- Owned-output registry records are parsed with an explicit Regex Match instead of
  relying on the automatic Matches variable after a negative match expression.
- Residual part, ytdl, and temp files are selected by case-insensitive literal file
  name prefixes, avoiding wildcard-provider differences on Windows PowerShell 5.1.
- GUI forced-cancel cleanup uses the same literal prefix rules.

2. Regression diagnostics
- Cancellation regression failures now identify whether a completed PART was
  removed or which incomplete artifact was retained.


fix76 implemented changes
-------------------------

1. Deterministic yt-dlp residue deletion
- Standard target.part, target.ytdl, and target.temp files are deleted by their
  exact literal paths before optional prefix enumeration handles variants.
- Owned-output records use delimiter indexes and substrings only, avoiding both
  automatic match state and regex differences in Windows PowerShell 5.1.
- Forced GUI cleanup applies the same exact-path-first policy.


fix77 implemented changes
-------------------------

1. Stored-login subdomain authentication
- CookieContainer parent-domain cookies are exported canonically as .sooplive.com
  with Netscape include-subdomains enabled, so private_auth on live.sooplive.com
  receives the same login tickets as a browser-exported Cookie file.
- private_auth JSON failures are decoded to a readable code and message.

2. Responsive VOD progress
- The VOD progress bar stretches with the form while remaining capped at the same
  980-pixel maximum width as the other VOD controls.

3. Regression coverage
- Cookie tests cover a parent domain returned without a leading dot, and source
  checks enforce both cookie canonicalization and the progress width bound.


fix78 implemented changes
-------------------------

1. Windows PowerShell 5.1 parser compatibility
- Delimited the interpolated private_auth failure-code variable before its colon,
  preventing PowerShell from interpreting it as an invalid scoped variable.
- The VOD source regression now requires the parser-safe interpolation form.


fix79 implemented changes
-------------------------

1. Stable responsive VOD progress track
- The progress track takes its width from the laid-out VOD form, not from the
  indicator, and remains capped to the same 980-pixel width as the form controls.
- Progress values now fill a stable track without changing its horizontal size.

2. Per-PART duration planning
- Analysis events include each PART duration as invariant seconds from yt-dlp
  metadata, with SOOP API file metadata used as a fallback when available.
- The VOD analysis summary lists every PART as PART N : HH시 MM분, or explicitly
  reports when that PART has no duration metadata.

3. Regression coverage
- Parser and formatter regressions cover the duration event contract and display,
  while PowerShell regressions cover numeric, time-string, and missing durations.


fix80 implemented changes
-------------------------

1. Conservative LIVE BJ filename sanitization
- Channel/BJ names now pass through a dedicated filename sanitizer for output
  folders and recording filenames. Windows-invalid characters and Unicode symbol,
  format, surrogate, private-use, and unassigned categories become underscores.
- Displayed BJ names remain unchanged; only filesystem path components are cleaned.
- Channel output-directory checks now use LiteralPath so bracketed names cannot be
  interpreted as PowerShell wildcard expressions.

2. Recorder launch diagnostics
- Recorder startup validates the exact Streamlink executable and output directory.
- Process-start failures now report executable, output directory, output filename,
  and the underlying error instead of only the generic Windows file-not-found text.

3. Regression coverage
- Recorder regressions cover a BJ name containing a heart symbol and a Windows-
  invalid colon without embedding non-ASCII source text in the test script.
