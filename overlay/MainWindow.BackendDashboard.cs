using System.Collections.Concurrent;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using WinRT.Interop;
using DrawingIcon = System.Drawing.Icon;
using DrawingSystemIcons = System.Drawing.SystemIcons;
using FormsNotifyIcon = System.Windows.Forms.NotifyIcon;
using FormsContextMenuStrip = System.Windows.Forms.ContextMenuStrip;
using FormsToolStripMenuItem = System.Windows.Forms.ToolStripMenuItem;
using FormsToolTipIcon = System.Windows.Forms.ToolTipIcon;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    static readonly Regex CompactProgressMetrics = new(
        @"^\[download\]\s+Written\s+(?<size>.+?)(?:\s+to\s+.+?)?\s+\((?<duration>\d{2}:\d{2}:\d{2})\s+@\s+(?<rate>.+?)\)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    void InitializeUiFlushTimer()
    {
        uiFlushTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(UiFlushMilliseconds)
        };

        uiFlushTimer.Tick += (_, _) => FlushBackendUiQueue();
        uiFlushTimer.Start();
    }

    void EnqueueBackendLine(string? line)
    {
        if (string.IsNullOrWhiteSpace(line))
            return;

        // Coalesce progress before it enters the event queue. This keeps only
        // one producer-side sample per stable channel even if the UI thread is
        // blocked, preventing progress output from causing unbounded memory.
        var progress = CompactRecording.Match(line.Trim());
        if (progress.Success)
        {
            var account = progress.Groups["account"].Value.Trim();
            var name = progress.Groups["name"].Value.Trim();
            var key = string.IsNullOrWhiteSpace(account)
                ? "name:" + name
                : "account:" + account;
            var metrics = CompactProgressMetrics.Match(progress.Groups["progress"].Value);
            if (!string.IsNullOrWhiteSpace(name) && metrics.Success)
                latestProgressByChannel[key] = new ProgressSnapshot(
                    account,
                    name,
                    metrics.Groups["size"].Value.Trim(),
                    metrics.Groups["duration"].Value.Trim(),
                    metrics.Groups["rate"].Value.Trim());
            return;
        }

        var critical = IsCriticalBackendEventLine(line);
        if (!critical && IsRepeatableWarningLine(line) &&
            backendWarningDeduplicator.ShouldSuppress(line, DateTime.UtcNow))
            return;

        if (critical)
        {
            priorityBackendLineQueue.Enqueue(line);
            return;
        }

        backendLineQueue.Enqueue(line);
        var queued = Interlocked.Increment(ref queuedBackendLines);
        while (queued > MaxQueuedBackendEvents && backendLineQueue.TryDequeue(out _))
        {
            Interlocked.Decrement(ref queuedBackendLines);
            Interlocked.Increment(ref droppedBackendLines);
            queued = Interlocked.Read(ref queuedBackendLines);
        }
    }

    void FlushBackendUiQueue()
    {
        ReconcileWatcherRunningUiFix39();
        Interlocked.Add(
            ref pendingSuppressedWarningLines,
            backendWarningDeduplicator.TakeSuppressedCount());
        var warningReportDue =
            (DateTime.UtcNow - lastWarningDedupReport).TotalSeconds >= 10;
        var suppressedWarnings = warningReportDue
            ? Interlocked.Exchange(ref pendingSuppressedWarningLines, 0)
            : 0;

        if (priorityBackendLineQueue.IsEmpty &&
            backendLineQueue.IsEmpty &&
            latestProgressByChannel.IsEmpty)
        {
            if (suppressedWarnings > 0)
            {
                AppendLog($"[WARN] 반복 backend 경고 {suppressedWarnings}줄 병합");
                FlushLogText();
                lastWarningDedupReport = DateTime.UtcNow;
            }
            return;
        }

        // Bound work per UI tick so a noisy backend can never monopolize
        // the WinUI dispatcher indefinitely.
        const int maxLinesPerFlush = 400;
        int count = 0;

        while (count < maxLinesPerFlush && priorityBackendLineQueue.TryDequeue(out var priorityLine))
        {
            ProcessBackendLine(priorityLine);
            count++;
        }

        while (count < maxLinesPerFlush && backendLineQueue.TryDequeue(out var line))
        {
            if (Interlocked.Decrement(ref queuedBackendLines) < 0)
                Interlocked.Exchange(ref queuedBackendLines, 0);
            ProcessBackendLine(line);
            count++;
        }

        flushedBackendLines += count;

        var dropped = Interlocked.Exchange(ref droppedBackendLines, 0);
        if (dropped > 0)
            AppendLog($"[WARN] GUI backend event queue overflow · 오래된 {dropped}줄 생략");
        var droppedPriority = priorityBackendLineQueue.TakeDroppedCount();
        if (droppedPriority > 0)
            AppendLog($"[WARN] GUI priority event queue overflow · 오래된 {droppedPriority}줄 생략");
        if (suppressedWarnings > 0)
        {
            AppendLog($"[WARN] 반복 backend 경고 {suppressedWarnings}줄 병합");
            lastWarningDedupReport = DateTime.UtcNow;
        }

        // Progress is already coalesced on the producer thread. Consume at
        // most the current latest value for each channel during this UI tick.
        if (!latestProgressByChannel.IsEmpty)
        {
            const int maxProgressPerFlush = 200;
            var progressCount = 0;
            foreach (var entry in latestProgressByChannel)
            {
                if (progressCount >= maxProgressPerFlush) break;
                if (latestProgressByChannel.TryRemove(entry.Key, out var snapshot))
                {
                    ApplyProgress(snapshot);
                    progressCount++;
                }
            }
        }

        FlushLogText();
        lastUiFlush = DateTime.Now;
    }

    static bool IsCriticalBackendEventLine(string line) =>
        line.StartsWith(BackendEventParser.Prefix, StringComparison.Ordinal) ||
        line.Contains(" : RECORD FINISHED | ", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : CHANNEL REMOVED", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : CHANNEL DISABLED", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : CHANNEL STOP ", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : LOW DISK", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : DISK SPACE UNKNOWN", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" : WORKER COOLDOWN", StringComparison.OrdinalIgnoreCase) ||
        line.Contains("WATCHER ERROR", StringComparison.OrdinalIgnoreCase);

    static bool IsRepeatableWarningLine(string line) =>
        line.Contains("[WARN]", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" WARNING", StringComparison.OrdinalIgnoreCase) ||
        line.Contains(" retry ", StringComparison.OrdinalIgnoreCase) ||
        line.Contains("failed; retry", StringComparison.OrdinalIgnoreCase);

    void ProcessBackendLine(string line)
    {
        ReconcileWatcherRunningUiFix39();
        AppendLog(line);

        var text = line.Trim();
        if (text.Length == 0) return;

        // JSON v1 is the authoritative machine protocol. Human-readable text
        // remains below as a compatibility fallback for older backends.
        var isJsonEvent = text.StartsWith(BackendEventParser.Prefix, StringComparison.Ordinal);
        if (BackendEventParser.TryParse(text, out var backendEvent) && backendEvent is not null)
        {
            if (!isJsonEvent && WasRecentlyHandledStructured(
                    backendEvent.Type, backendEvent.Account, backendEvent.Name))
                return;
            ProcessStructuredBackendEvent(backendEvent);
            return;
        }

        var stopRequested = ChannelStopRequested.Match(text);
        if (stopRequested.Success)
        {
            var name = stopRequested.Groups["name"].Value.Trim();
            var account = stopRequested.Groups["account"].Value.Trim();
            var item = GetOrCreate(account, name);

            item.Status = "중지 요청 중";
            item.Detail = "녹화 프로세스가 종료되는지 확인하고 있습니다.";

            UpdateCounts();
            UpdateSelectedRecordingActionButton();
            return;
        }

        var stopCompleted = ChannelStopCompleted.Match(text);
        if (stopCompleted.Success)
        {
            var item = GetOrCreate(
                stopCompleted.Groups["account"].Value.Trim(),
                stopCompleted.Groups["name"].Value.Trim());
            item.Status = "직접 중지";
            item.Detail = "지금 방송은 자동으로 다시 녹화하지 않습니다.";
            item.IsSuspended = true;
            MoveToStopped(item);
            UpdateSelectedRecordingActionButton();
            return;
        }

        var stopFailed = ChannelStopFailed.Match(text);
        if (stopFailed.Success)
        {
            var item = GetOrCreate(
                stopFailed.Groups["account"].Value.Trim(),
                stopFailed.Groups["name"].Value.Trim());
            item.Status = "● REC";
            item.Detail = "중지하지 못했습니다. 녹화 상태를 다시 확인해 주세요.";
            item.IsSuspended = false;
            MoveToRecording(item);
            UpdateSelectedRecordingActionButton();
            return;
        }

        var resumeRequested = ChannelResumeRequested.Match(text);
        if (resumeRequested.Success)
        {
            var name = resumeRequested.Groups["name"].Value.Trim();
            var account = resumeRequested.Groups["account"].Value.Trim();
            var item = GetOrCreate(account, name);

            item.Status = "… 재시작 대기";
            item.Detail = "현재 LIVE 상태 재확인 중";
            item.IsSuspended = false;
            StoppedItems.Remove(item);
            UpdateCounts();
            UpdateSelectedRecordingActionButton();
            return;
        }

        var healthCleared = DashboardHealthCleared.Match(text);
        if (healthCleared.Success)
        {
            ClearDashboardAlert(
                healthCleared.Groups["account"].Value.Trim(),
                healthCleared.Groups["name"].Value.Trim());
            return;
        }

        var health = DashboardHealthState.Match(text);
        if (health.Success)
        {
            var rawStatus = health.Groups["status"].Value.Trim().ToUpperInvariant();
            var status = rawStatus switch
            {
                "LOW DISK" or "LOW DISK SPACE" => "디스크 공간 부족",
                "DISK SPACE UNKNOWN" => "디스크 확인 실패",
                "CHECK ERROR" => "방송 상태 확인 실패",
                "LOGIN REQUIRED" => "로그인 확인 필요",
                "RECORD START FAILED" => "녹화 시작 실패",
                "WORKER COOLDOWN" => "Worker 복구 대기",
                _ => "확인 필요"
            };
            var detail = health.Groups["detail"].Success
                ? health.Groups["detail"].Value.Trim()
                : health.Groups["detail2"].Value.Trim();
            if (rawStatus == "RECORD START FAILED")
            {
                var failedItem = GetOrCreate(
                    health.Groups["account"].Value.Trim(),
                    health.Groups["name"].Value.Trim());
                RecordingItems.Remove(failedItem);
                failedItem.RateText = "-";
                failedItem.RateBytesPerSecond = 0;
                failedItem.DisplayDrive = "";
                failedItem.Drive = "";
                ClearPendingProgress(failedItem.Account, failedItem.Name);
                RefreshDiskSummary();
            }
            SetDashboardAlert(
                health.Groups["account"].Value.Trim(),
                health.Groups["name"].Value.Trim(),
                status,
                detail);
            return;
        }

        var finished = RecordFinishedLog.Match(text);
        if (finished.Success)
        {
            var name = finished.Groups["name"].Value.Trim();
            var duration = finished.Groups["duration"].Value.Trim();
            var size = finished.Groups["size"].Value.Trim();
            var reason = finished.Groups["reason"].Value.Trim();
            var file = finished.Groups["file"].Value.Trim();

            ShowRecordingFinishedNotification(name, duration, size, reason, file);
        }

        // Start-ChannelRecording always prints a human-readable block:
        // Channel : ...
        // Title   : ...
        // Output  : C:\...
        // Parse this directly so the GUI knows the file path independently
        // of CONSOLE_SHOW_PATH and dashboard progress formatting.
        var startChannelLine = RecordStartChannelLine.Match(text);
        if (startChannelLine.Success)
        {
            pendingRecordChannel = startChannelLine.Groups["value"].Value.Trim();
            pendingRecordAccount = null;
            pendingRecordTitle = null;
        }

        var startAccountLine = RecordStartAccountLine.Match(text);
        if (startAccountLine.Success && !string.IsNullOrWhiteSpace(pendingRecordChannel))
            pendingRecordAccount = startAccountLine.Groups["value"].Value.Trim();

        var startTitleLine = RecordStartTitleLine.Match(text);
        if (startTitleLine.Success && !string.IsNullOrWhiteSpace(pendingRecordChannel))
        {
            pendingRecordTitle = startTitleLine.Groups["value"].Value.Trim();
        }

        var startOutputLine = RecordStartOutputLine.Match(text);
        if (startOutputLine.Success && !string.IsNullOrWhiteSpace(pendingRecordChannel))
        {
            var outputFile = startOutputLine.Groups["value"].Value.Trim();
            var item = GetOrCreate(pendingRecordAccount, pendingRecordChannel!);
            var notifyStart = item.Status != "● REC" ||
                !string.Equals(item.FilePath, outputFile, StringComparison.OrdinalIgnoreCase);

            item.Status = "● REC";
            item.Time = DateTime.Now.ToString("HH:mm:ss");
            item.Detail = "녹화 중";
            item.IsSuspended = false;
            item.SizeText = "-";
            item.ElapsedText = "-";
            item.RateText = "-";
            item.RateBytesPerSecond = 0;
            item.FilePath = outputFile;

            if (!string.IsNullOrWhiteSpace(pendingRecordTitle))
                item.Title = pendingRecordTitle!;

            UpdateDrive(item, outputFile);
            MoveToRecording(item);

            if (notifyStart && uiNotifyRecordStart)
                ShowGuiNotification("녹화 시작", $"{item.Name}\n{item.FileName}", FormsToolTipIcon.Info);

            pendingRecordChannel = null;
            pendingRecordAccount = null;
            pendingRecordTitle = null;
            return;
        }

        var compact = CompactRecording.Match(text);
        if (compact.Success)
        {
            var channel = compact.Groups["name"].Value.Trim();
            var account = compact.Groups["account"].Value.Trim();

            var embedded = "[" + compact.Groups["time"].Value + "] " +
                           compact.Groups["progress"].Value.Trim();

            var pWithPath = DownloadProgressWithPath.Match(embedded);
            if (pWithPath.Success)
            {
                ApplyProgress(account, channel, pWithPath);
                return;
            }

            var pNoPath = DownloadProgressNoPath.Match(embedded);
            if (pNoPath.Success)
                ApplyProgress(account, channel, pNoPath);

            return;
        }

        // Untagged raw progress is intentionally ignored by the GUI.
        // fix33 uses only CompactRecording lines where the BJ name and
        // progress metrics are guaranteed to be on the same line.
        if (DownloadProgressWithPath.IsMatch(text) ||
            DownloadProgressNoPath.IsMatch(text))
        {
            return;
        }

        var off = PlainOffline.Match(text);
        if (off.Success)
        {
            var item = GetOrCreate(
                off.Groups["account"].Value.Trim(),
                off.Groups["name"].Value.Trim());
            item.Time = off.Groups["time"].Value;
            item.Status = "OFFLINE";
            item.Detail = "";
            MoveToOffline(item);
        }
    }

    void ProcessStructuredBackendEvent(BackendEvent backendEvent)
    {
        var expiry = DateTime.UtcNow.AddMinutes(-1);
        foreach (var expiredKey in recentStructuredEvents
            .Where(entry => entry.Value < expiry)
            .Select(entry => entry.Key)
            .ToArray())
            recentStructuredEvents.Remove(expiredKey);
        recentStructuredEvents[StructuredEventKey(
            backendEvent.Type, backendEvent.Account, backendEvent.Name)] = DateTime.UtcNow;
        switch (backendEvent.Type)
        {
            case "recording_started":
            {
                var item = GetOrCreate(backendEvent.Account, backendEvent.Name);
                var notifyStart = item.Status != "● REC" ||
                    !string.Equals(item.FilePath, backendEvent.File, StringComparison.OrdinalIgnoreCase);
                item.Status = "● REC";
                item.Title = backendEvent.Title;
                item.Time = DateTime.Now.ToString("HH:mm:ss");
                item.Detail = "녹화 중";
                item.IsSuspended = false;
                item.SizeText = "-";
                item.ElapsedText = "-";
                item.RateText = "-";
                item.RateBytesPerSecond = 0;
                item.FilePath = backendEvent.File;
                UpdateDrive(item, backendEvent.File);
                MoveToRecording(item);
                ClearDashboardAlert(item.Account, item.Name);
                if (notifyStart && uiNotifyRecordStart)
                    ShowGuiNotification("녹화 시작", $"{item.Name}\n{item.FileName}", FormsToolTipIcon.Info);
                break;
            }
            case "recording_finished":
                HandleRecordingFinished(
                    backendEvent.Account, backendEvent.Name, backendEvent.Duration,
                    backendEvent.Size, backendEvent.Reason, backendEvent.File);
                break;
            case "recording_stalled":
                SetDashboardAlert(
                    backendEvent.Account, backendEvent.Name, "녹화 재시작 확인",
                    string.IsNullOrWhiteSpace(backendEvent.Detail)
                        ? "파일 증가가 멈춰 방송 상태와 녹화 재시작을 확인하고 있습니다."
                        : backendEvent.Detail);
                break;
            case "low_disk":
                SetDashboardAlert(
                    backendEvent.Account, backendEvent.Name, "디스크 공간 부족",
                    string.IsNullOrWhiteSpace(backendEvent.Detail)
                        ? "최소 디스크 여유 공간에 도달했습니다."
                        : backendEvent.Detail);
                break;
            case "worker_cooldown":
                SetDashboardAlert(
                    backendEvent.Account, backendEvent.Name, "Worker 복구 대기",
                    backendEvent.CooldownSeconds is int seconds
                        ? $"Worker 요청을 {seconds}초 후 다시 시도합니다."
                        : backendEvent.Detail);
                break;
            case "channel_removed":
            case "channel_disabled":
                RemoveDashboardChannelByIdentity(backendEvent.Account, backendEvent.Name);
                break;
        }
    }

    static string StructuredEventKey(string type, string account, string name) =>
        type + "|" + (string.IsNullOrWhiteSpace(account) ? "name:" + name : "account:" + account);

    bool WasRecentlyHandledStructured(string type, string account, string name)
    {
        var key = StructuredEventKey(type, account, name);
        if (!recentStructuredEvents.TryGetValue(key, out var handledAt)) return false;
        if ((DateTime.UtcNow - handledAt).TotalSeconds <= 5) return true;
        recentStructuredEvents.Remove(key);
        return false;
    }

    void ApplyProgress(string? account, string? name, Match progress)
    {
        if (string.IsNullOrWhiteSpace(name))
            return;

        var item = GetOrCreate(account, name!);

        // During steady-state recording only the three metric bindings below
        // change. This avoids list reordering/recreation and keeps the static
        // metadata side visually stable.
        if (item.Status != "● REC")
            item.Status = "● REC";

        if (item.Detail != "녹화 중")
            item.Detail = "녹화 중";

        if (item.IsSuspended)
            item.IsSuspended = false;

        item.SizeText = progress.Groups["size"].Value.Trim();
        item.ElapsedText = progress.Groups["duration"].Value.Trim();
        item.RateText = progress.Groups["rate"].Value.Trim();
        item.RateBytesPerSecond = ParseRateBytesPerSecond(item.RateText);

        if ((DateTime.Now - lastDiskEstimateRefresh).TotalSeconds >= 5)
        {
            lastDiskEstimateRefresh = DateTime.Now;
            RefreshDiskSummary();
        }

        // Compact backend progress intentionally does not need to update
        // FilePath/drive. Those are fixed by RECORD START.
        var alertCleared = ClearDashboardAlert(item.Account, item.Name, refresh: false);
        if (!RecordingItems.Contains(item))
            MoveToRecording(item);
        else if (alertCleared)
            UpdateCounts(refreshDisk: false);
    }

    void ApplyProgress(ProgressSnapshot progress)
    {
        if (string.IsNullOrWhiteSpace(progress.Name)) return;
        var item = GetOrCreate(progress.Account, progress.Name);
        if (item.Status != "● REC") item.Status = "● REC";
        if (item.Detail != "녹화 중") item.Detail = "녹화 중";
        if (item.IsSuspended) item.IsSuspended = false;
        item.SizeText = progress.Size;
        item.ElapsedText = progress.Duration;
        item.RateText = progress.Rate;
        item.RateBytesPerSecond = ParseRateBytesPerSecond(progress.Rate);
        if ((DateTime.Now - lastDiskEstimateRefresh).TotalSeconds >= 5)
        {
            lastDiskEstimateRefresh = DateTime.Now;
            RefreshDiskSummary();
        }
        var alertCleared = ClearDashboardAlert(item.Account, item.Name, refresh: false);
        if (!RecordingItems.Contains(item)) MoveToRecording(item);
        else if (alertCleared) UpdateCounts(refreshDisk: false);
    }

    void HandleRecordingFinished(
        string account,
        string name,
        string duration,
        string size,
        string reason,
        string file)
    {
        var displayReason = BackendEventParser.NormalizeRecordingFinishedReason(reason);
        ShowRecordingFinishedNotification(name, duration, size, displayReason, file);
        ClearPendingProgress(account, name);

        var item = GetOrCreate(account, name);
        AddRecentRecordingFix51(
            account, name, item.Title, duration, size, displayReason, file);
        RecordingItems.Remove(item);
        item.RateText = "-";
        item.RateBytesPerSecond = 0;
        item.DisplayDrive = "";
        item.Drive = "";

        var normalizedReason = displayReason.ToUpperInvariant();
        if (normalizedReason == "NORMAL")
        {
            ClearDashboardAlert(account, name, refresh: false);
            item.Status = "완료";
            item.Detail = "정상 종료";
            UpdateCounts();
            return;
        }
        if (normalizedReason is "CHANNEL REMOVED" or "CHANNEL DISABLED" or "WATCHER EXIT")
        {
            RemoveDashboardChannel(item);
            return;
        }

        if (normalizedReason == "USER CHANNEL STOP")
        {
            // CHANNEL STOP COMPLETED follows and moves this same item into the
            // selectable PAUSED collection. Until then it must not count REC.
            item.Status = "중지 확인 중";
            item.Detail = "사용자 중지 완료를 확인하고 있습니다.";
            UpdateCounts();
            return;
        }

        var detail = normalizedReason switch
        {
            "LOW DISK SPACE" => "최소 디스크 여유 공간에 도달해 녹화가 중단되었습니다.",
            "RECORD STALLED" => "파일 증가가 멈춰 방송 상태와 녹화 재시작을 확인하고 있습니다.",
            _ when normalizedReason.StartsWith("RECORDER EXIT", StringComparison.Ordinal) =>
                "녹화 프로세스가 종료되어 방송 상태와 재시작을 확인하고 있습니다.",
            _ => displayReason
        };
        SetDashboardAlert(account, name, "녹화 재시작 확인", detail);
    }

    void ClearPendingProgress(string? account, string name)
    {
        if (!string.IsNullOrWhiteSpace(account))
            latestProgressByChannel.TryRemove("account:" + account.Trim(), out _);
        if (!string.IsNullOrWhiteSpace(name))
            latestProgressByChannel.TryRemove("name:" + name.Trim(), out _);
    }

    void RemoveDashboardChannel(ChannelStatus item)
    {
        RecordingItems.Remove(item);
        OfflineItems.Remove(item);
        StoppedItems.Remove(item);
        ClearDashboardAlert(item.Account, item.Name, refresh: false);
        ClearPendingProgress(item.Account, item.Name);

        foreach (var key in statusMap
            .Where(x => ReferenceEquals(x.Value, item))
            .Select(x => x.Key)
            .ToArray())
            statusMap.Remove(key);

        if (ReferenceEquals(RecordingList?.SelectedItem, item))
            RecordingList.SelectedItem = null;
        UpdateSelectedRecordingActionButton();
        UpdateCounts();
    }

    void RemoveDashboardChannelByIdentity(string account, string name)
    {
        var key = string.IsNullOrWhiteSpace(account)
            ? "name:" + name
            : "account:" + account;
        if (statusMap.TryGetValue(key, out var item))
        {
            RemoveDashboardChannel(item);
            return;
        }

        // RECORD FINISHED may already have removed the same channel. Clear
        // residual queue/alert state without recreating a transient card.
        ClearPendingProgress(account, name);
        ClearDashboardAlert(account, name);
    }

    ChannelStatus GetOrCreate(string? account, string name)
    {
        var configured = !string.IsNullOrWhiteSpace(account)
            ? ChannelItems.FirstOrDefault(x =>
                string.Equals(x.Account, account, StringComparison.OrdinalIgnoreCase))
            : ChannelItems.FirstOrDefault(x =>
                string.Equals(x.Name, name, StringComparison.OrdinalIgnoreCase));

        var resolvedAccount = string.IsNullOrWhiteSpace(account)
            ? configured?.Account ?? ""
            : account.Trim();
        var key = string.IsNullOrWhiteSpace(resolvedAccount)
            ? "name:" + name
            : "account:" + resolvedAccount;

        if (!statusMap.TryGetValue(key, out var item))
        {
            item = new ChannelStatus
            {
                Name = name,
                Account = resolvedAccount
            };
            statusMap[key] = item;
        }
        else
        {
            if (!string.IsNullOrWhiteSpace(name))
                item.Name = name;
            if (!string.IsNullOrWhiteSpace(resolvedAccount))
                item.Account = resolvedAccount;
        }

        return item;
    }

    void MoveToRecording(ChannelStatus item)
    {
        OfflineItems.Remove(item);
        StoppedItems.Remove(item);
        ClearDashboardAlert(item.Account, item.Name, refresh: false);
        if (!RecordingItems.Contains(item))
            RecordingItems.Add(item);

        if (uiAutoFormat)
            SortDashboardItems();

        UpdateCounts();
    }

    void MoveToOffline(ChannelStatus item)
    {
        var alreadyOffline = OfflineItems.Contains(item) &&
            !RecordingItems.Contains(item) &&
            !StoppedItems.Contains(item);
        if (alreadyOffline)
            return;

        RecordingItems.Remove(item);
        StoppedItems.Remove(item);
        if (!OfflineItems.Contains(item))
            OfflineItems.Add(item);

        if (uiAutoFormat)
            SortDashboardItems();

        UpdateCounts();
    }

    void MoveToStopped(ChannelStatus item)
    {
        RecordingItems.Remove(item);
        OfflineItems.Remove(item);
        ClearDashboardAlert(item.Account, item.Name, refresh: false);
        if (!StoppedItems.Contains(item))
            StoppedItems.Add(item);

        if (uiAutoFormat)
            SortDashboardItems();

        UpdateCounts();
    }

    static string DashboardKey(string? account, string name) =>
        string.IsNullOrWhiteSpace(account)
            ? "name:" + name.Trim()
            : "account:" + account.Trim();

    void SetDashboardAlert(string? account, string name, string status, string detail)
    {
        var key = DashboardKey(account, name);
        if (!alertMap.TryGetValue(key, out var item))
        {
            item = new ChannelStatus { Account = account?.Trim() ?? "", Name = name.Trim() };
            alertMap[key] = item;
            AlertItems.Add(item);
        }

        if (string.Equals(item.Status, status, StringComparison.Ordinal) &&
            string.Equals(item.Detail, detail, StringComparison.Ordinal))
            return;

        item.Status = status;
        item.Detail = detail;
        item.Time = DateTime.Now.ToString("HH:mm:ss");
        item.Title = status switch
        {
            "디스크 공간 부족" => "권장 작업: 녹화 폴더 또는 설정에서 최소 여유 공간을 확인하세요.",
            "Worker 복구 대기" => "권장 작업: 자동 재시도를 기다리거나 설정의 Worker 연결을 확인하세요.",
            "로그인 확인 필요" => "권장 작업: 설정에서 SOOP 로그인을 다시 확인하세요.",
            "녹화 시작 실패" => "권장 작업: 즉시 다시 확인하거나 로그에서 상세 오류를 확인하세요.",
            _ => "권장 작업: 즉시 다시 확인하거나 녹화 폴더와 로그를 확인하세요."
        };
        if (uiNotifyWarning)
            ShowGuiNotification("녹화 확인 필요", $"{name}\n{detail}", FormsToolTipIcon.Warning);
        if (uiAutoFormat)
            SortDashboardItems();
        UpdateCounts(refreshDisk: false);
    }

    bool ClearDashboardAlert(string? account, string name, bool refresh = true)
    {
        var key = DashboardKey(account, name);
        if (!alertMap.Remove(key, out var alert))
            return false;

        AlertItems.Remove(alert);
        if (refresh)
            UpdateCounts(refreshDisk: false);
        return true;
    }

    void UpdateCounts(bool refreshDisk = true)
    {
        SetTextIfChangedFix38(
            RecordingCountText,
            RecordingItems.Count(x => x.Status == "● REC").ToString());
        SetTextIfChangedFix38(OfflineCountText, OfflineItems.Count.ToString());
        SetTextIfChangedFix38(StoppedCountText, StoppedItems.Count.ToString());
        SetTextIfChangedFix38(AlertCountText, AlertItems.Count.ToString());
        if (refreshDisk)
            RefreshDiskSummary();
        UpdateDashboardEmptyState();
    }

    static void SetTextIfChangedFix38(TextBlock target, string value)
    {
        if (!string.Equals(target.Text, value, StringComparison.Ordinal))
            target.Text = value;
    }

    void UpdateDashboardEmptyState()
    {
        if (DashboardEmptyStatePanel == null)
            return;

        DashboardEmptyStatePanel.Visibility = RecordingItems.Count == 0
            ? Visibility.Visible
            : Visibility.Collapsed;

        if (dashboardWatcherRunning)
        {
            DashboardEmptyTitleText.Text = "현재 녹화 중인 방송이 없습니다.";
            DashboardEmptyDetailText.Text =
                $"등록된 채널 {ChannelItems.Count}개의 방송 상태를 확인하고 있습니다.";
            DashboardEmptyStartButton.Visibility = Visibility.Collapsed;
        }
        else
        {
            DashboardEmptyTitleText.Text = "현재 채널 확인이 중지되어 있습니다.";
            DashboardEmptyDetailText.Text =
                "방송 상태를 확인하려면 Watcher를 시작해 주세요.";
            DashboardEmptyStartButton.Visibility = Visibility.Visible;
            DashboardEmptyStartButton.IsEnabled = StartButton?.IsEnabled != false;
        }
    }

    void UpdateDrive(ChannelStatus item, string file)
    {
        if (string.IsNullOrWhiteSpace(file)) return;
        try
        {
            var root=Path.GetPathRoot(Path.GetFullPath(file));
            if (string.IsNullOrWhiteSpace(root)) return;
            if (!driveSpaceCache.TryGetAvailableBytes(root, out var availableBytes)) return;
            var free=availableBytes/1024d/1024d/1024d;
            item.Drive=$"{root.TrimEnd('\\')} {free:N1} GB";
            RefreshDiskSummary();
        }
        catch { }
    }

    void RefreshDiskSummary()
    {
        if (DiskSummaryText == null)
            return;

        var activeItems = RecordingItems
            .Where(x => x.Status == "● REC")
            .ToList();

        var rateByRoot = new Dictionary<string, double>(StringComparer.OrdinalIgnoreCase);
        var itemsByRoot = new Dictionary<string, List<ChannelStatus>>(StringComparer.OrdinalIgnoreCase);

        foreach (var item in activeItems)
        {
            try
            {
                var root = Path.GetPathRoot(Path.GetFullPath(item.FilePath));
                if (string.IsNullOrWhiteSpace(root))
                    continue;
                if (!itemsByRoot.TryGetValue(root, out var rootItems))
                {
                    rootItems = new List<ChannelStatus>();
                    itemsByRoot[root] = rootItems;
                }
                rootItems.Add(item);
                rateByRoot[root] = rateByRoot.GetValueOrDefault(root) + item.RateBytesPerSecond;
            }
            catch { }
        }

        // PAUSED/restart-waiting cards must never contribute stale drive info.
        foreach (var item in RecordingItems.Where(x => x.Status != "● REC"))
            item.DisplayDrive = "";

        if (itemsByRoot.Count == 0)
        {
            DiskSummaryText.Text = "-";

            foreach (var item in activeItems)
                item.DisplayDrive = "";

            return;
        }

        var summaries = new List<string>();
        foreach (var entry in itemsByRoot.OrderBy(x => x.Key))
        {
            try
            {
                if (!driveSpaceCache.TryGetAvailableBytes(entry.Key, out var availableBytes))
                    continue;

                var freeBytes = (double)availableBytes;
                var freeGb = freeBytes / 1024d / 1024d / 1024d;
                var configuredReserveGb = MinDiskBox?.Value;
                var reserveGb = configuredReserveGb is double value && !double.IsNaN(value)
                    ? value
                    : 20d;
                var usableBytes = Math.Max(0d, freeBytes - reserveGb * 1024d * 1024d * 1024d);
                var rate = rateByRoot.GetValueOrDefault(entry.Key);
                var estimate = FormatDiskTimeEstimate(usableBytes, rate);
                var display = $"{entry.Key.TrimEnd('\\')} {freeGb:N1} GB";
                if (!string.IsNullOrWhiteSpace(estimate))
                    display += $" · {estimate}";
                summaries.Add(display);
                foreach (var item in entry.Value)
                    item.Drive = display;
            }
            catch { }
        }

        DiskSummaryText.Text = summaries.Count == 0
            ? "-"
            : string.Join(" · ", summaries);

        var showPerCard = summaries.Count > 1;

        foreach (var item in activeItems)
            item.DisplayDrive = showPerCard ? item.Drive : "";
    }

    static double ParseRateBytesPerSecond(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return 0;
        var match = Regex.Match(
            text,
            @"(?<value>[0-9]+(?:[.,][0-9]+)?)\s*(?<unit>[KMGT]?B)/s",
            RegexOptions.IgnoreCase);
        if (!match.Success ||
            !double.TryParse(
                match.Groups["value"].Value.Replace(',', '.'),
                System.Globalization.NumberStyles.Float,
                System.Globalization.CultureInfo.InvariantCulture,
                out var value))
            return 0;

        return match.Groups["unit"].Value.ToUpperInvariant() switch
        {
            "KB" => value * 1024d,
            "MB" => value * 1024d * 1024d,
            "GB" => value * 1024d * 1024d * 1024d,
            "TB" => value * 1024d * 1024d * 1024d * 1024d,
            _ => value
        };
    }

    static string FormatDiskTimeEstimate(double usableBytes, double bytesPerSecond)
    {
        if (bytesPerSecond <= 0 || usableBytes <= 0)
            return usableBytes <= 0 ? "여유 한도 도달" : "남은 시간 계산 중";

        var hours = usableBytes / bytesPerSecond / 3600d;
        if (hours < 1) return "1시간 미만";
        if (hours < 24) return $"약 {Math.Max(1, Math.Floor(hours)):0}시간";
        var days = hours / 24d;
        if (days < 3) return $"약 {Math.Max(1, Math.Floor(days)):0}일";
        return "3일 이상";
    }
}
