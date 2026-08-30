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
    void HookAppWindowClosing()
    {
        try
        {
            var hwnd = WindowNative.GetWindowHandle(this);
            var id = Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = AppWindow.GetFromWindowId(id);

            appWindow.Closing += async (sender, args) =>
            {
                if (allowRealClose)
                    return;

                args.Cancel = true;

                if (closeDialogOpen)
                    return;

                var action = uiPreferences.CloseAction?.ToUpperInvariant() ?? "ASK";

                if (action == "TRAY")
                {
                    HideToTray();
                    return;
                }

                if (action == "EXIT")
                {
                    ExitApplication();
                    return;
                }

                closeDialogOpen = true;
                try
                {
                    var remember = new CheckBox
                    {
                        Content = "이 선택을 다음부터 기억",
                        Margin = new Thickness(0, 10, 0, 0)
                    };

                    var panel = new StackPanel { Spacing = 6 };
                    panel.Children.Add(new TextBlock
                    {
                        Text = "창을 닫을 때 어떻게 할까요?",
                        TextWrapping = TextWrapping.Wrap
                    });
                    panel.Children.Add(new TextBlock
                    {
                        Text = "트레이로 보내면 Watcher와 녹화는 계속 실행됩니다.",
                        Foreground = Muted,
                        TextWrapping = TextWrapping.Wrap
                    });
                    panel.Children.Add(remember);

                    var dialog = new ContentDialog
                    {
                        Title = "SOOP LIVE Downloader",
                        Content = panel,
                        PrimaryButtonText = "시스템 트레이로",
                        SecondaryButtonText = "완전히 종료",
                        CloseButtonText = "취소",
                        DefaultButton = ContentDialogButton.Primary,
                        XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
                    };

                    var result = await dialog.ShowAsync();

                    if (result == ContentDialogResult.Primary)
                    {
                        if (remember.IsChecked == true)
                        {
                            uiPreferences.CloseAction = "TRAY";
                            uiPreferences.Save();
                        }
                        HideToTray();
                    }
                    else if (result == ContentDialogResult.Secondary)
                    {
                        if (remember.IsChecked == true)
                        {
                            uiPreferences.CloseAction = "EXIT";
                            uiPreferences.Save();
                        }
                        ExitApplication();
                    }
                }
                finally
                {
                    closeDialogOpen = false;
                }
            };
        }
        catch (Exception ex)
        {
            WriteStartupLog("HookAppWindowClosing FAILED", ex);
        }
    }

    void InitializeTrayIcon()
    {
        try
        {
            trayReady = false;

            var iconCandidates = new[]
            {
                Path.Combine(AppContext.BaseDirectory, "Assets", "SOOPLiveDownloader.ico"),
                Path.Combine(Directory.GetCurrentDirectory(), "Assets", "SOOPLiveDownloader.ico"),
                Path.Combine(backendDir, "..", "Assets", "SOOPLiveDownloader.ico")
            };

            string? iconPath = iconCandidates
                .Select(Path.GetFullPath)
                .FirstOrDefault(File.Exists);

            var menu = new FormsContextMenuStrip();

            var openItem = new FormsToolStripMenuItem("SOOP LIVE Downloader 열기");
            openItem.Click += (_, _) => DispatcherQueue.TryEnqueue(ShowFromTray);

            var startItem = new FormsToolStripMenuItem("Watcher 시작");
            startItem.Click += (_, _) => DispatcherQueue.TryEnqueue(() =>
            {
                if (!backend.IsRunning)
                    StartButton_Click(StartButton, new RoutedEventArgs());
            });

            var stopItem = new FormsToolStripMenuItem("Watcher 중지");
            stopItem.Click += (_, _) => DispatcherQueue.TryEnqueue(() =>
            {
                if (backend.IsRunning)
                    StopButton_Click(StopButton, new RoutedEventArgs());
            });

            var exitItem = new FormsToolStripMenuItem("완전히 종료");
            exitItem.Click += (_, _) => DispatcherQueue.TryEnqueue(ExitApplication);

            menu.Items.Add(openItem);
            menu.Items.Add(new System.Windows.Forms.ToolStripSeparator());
            menu.Items.Add(startItem);
            menu.Items.Add(stopItem);
            menu.Items.Add(new System.Windows.Forms.ToolStripSeparator());
            menu.Items.Add(exitItem);

            trayIcon = new FormsNotifyIcon
            {
                Text = "SOOP LIVE Downloader",
                ContextMenuStrip = menu,
                Visible = false
            };

            // NotifyIcon does not appear if Icon is null.
            // Load our custom icon first; if the file is unavailable for any
            // reason, fall back to a guaranteed Windows system icon.
            if (!string.IsNullOrWhiteSpace(iconPath))
            {
                try
                {
                    trayIcon.Icon = new DrawingIcon(iconPath);
                    WriteStartupLog("Tray icon loaded: " + iconPath);
                }
                catch (Exception ex)
                {
                    WriteStartupLog("Custom tray icon load FAILED; using system fallback", ex);
                    trayIcon.Icon = DrawingSystemIcons.Application;
                }
            }
            else
            {
                WriteStartupLog("Custom tray icon file not found; using system fallback");
                trayIcon.Icon = DrawingSystemIcons.Application;
            }

            trayIcon.DoubleClick += (_, _) => DispatcherQueue.TryEnqueue(ShowFromTray);

            // Important: set Visible only after Icon has been assigned.
            trayIcon.Visible = true;
            trayReady = trayIcon.Icon != null;

            WriteStartupLog(
                trayReady
                    ? "Tray icon initialized and visible"
                    : "Tray icon initialization completed but Icon is null");
        }
        catch (Exception ex)
        {
            trayReady = false;

            try
            {
                trayIcon?.Dispose();
                trayIcon = null;
            }
            catch { }

            WriteStartupLog("InitializeTrayIcon FAILED", ex);
        }
    }

    async void HideToTray()
    {
        if (!trayReady || trayIcon == null || trayIcon.Icon == null)
        {
            WriteStartupLog("HideToTray blocked: tray icon is not ready");

            await ShowDialogAsync(
                "시스템 트레이 사용 불가",
                "트레이 아이콘을 초기화하지 못해 창을 숨기지 않았습니다.\n" +
                "SOOPLiveWinUI_startup.log에서 Tray 관련 오류를 확인할 수 있습니다.");
            return;
        }

        try
        {
            trayIcon.Visible = true;

            var hwnd = WindowNative.GetWindowHandle(this);
            ShowWindow(hwnd, 0);

            trayIcon.ShowBalloonTip(
                2500,
                "SOOP LIVE Downloader",
                backend.IsRunning
                    ? "시스템 트레이에서 Watcher와 녹화를 계속 실행합니다."
                    : "시스템 트레이로 이동했습니다.",
                FormsToolTipIcon.Info);

            WriteStartupLog("Window hidden to system tray");
        }
        catch (Exception ex)
        {
            WriteStartupLog("HideToTray FAILED", ex);

            try
            {
                ShowFromTray();
            }
            catch { }
        }
    }

    void ShowFromTray()
    {
        try
        {
            var hwnd = WindowNative.GetWindowHandle(this);
            ShowWindow(hwnd, 5);
            SetForegroundWindow(hwnd);

            if (trayIcon != null)
                trayIcon.Visible = true;

            WriteStartupLog("Window restored from system tray");
        }
        catch (Exception ex)
        {
            WriteStartupLog("ShowFromTray FAILED", ex);
        }
    }

    void ExitApplication()
    {
        if (allowRealClose)
            return;

        allowRealClose = true;

        try
        {
            backend.StopNow();
        }
        catch { }

        try
        {
            if (trayIcon != null)
            {
                trayIcon.Visible = false;
                trayIcon.Dispose();
            }
            trayIcon = null;
            trayReady = false;
        }
        catch { }

        try
        {
            Close();
        }
        catch { }
    }

    void ShowRecordingFinishedNotification(
        string channel,
        string duration,
        string size,
        string reason,
        string file)
    {
        try
        {
            if (trayIcon == null || !uiNotifyRecordFinish)
                return;

            var body = $"{channel} 녹화가 종료되었습니다.\n{duration} · {size}";
            if (!string.IsNullOrWhiteSpace(reason) &&
                !string.Equals(reason, "NORMAL", StringComparison.OrdinalIgnoreCase))
                body += $"\n{reason}";

            trayIcon.Tag = file;
            trayIcon.BalloonTipClicked -= TrayBalloonClicked;
            trayIcon.BalloonTipClicked += TrayBalloonClicked;

            trayIcon.ShowBalloonTip(
                8000,
                "녹화 종료",
                body,
                FormsToolTipIcon.Info);
        }
        catch { }
    }

    void ShowGuiNotification(string title, string body, FormsToolTipIcon icon)
    {
        try
        {
            trayIcon?.ShowBalloonTip(6000, title, body, icon);
        }
        catch { }
    }

    void TrayBalloonClicked(object? sender, EventArgs e)
    {
        try
        {
            var file = trayIcon?.Tag as string;
            if (string.IsNullOrWhiteSpace(file))
                return;

            var dir = Path.GetDirectoryName(file);
            if (string.IsNullOrWhiteSpace(dir) || !Directory.Exists(dir))
                return;

            Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"")
            {
                UseShellExecute = true
            });
        }
        catch { }
    }

    void MainWindow_Closed(object sender, WindowEventArgs args)
    {
        if (windowCleanupDone)
            return;

        windowCleanupDone = true;
        channelSearchDebounceTimer?.Stop();

        try
        {
            uiFlushTimer?.Stop();
            uiFlushTimer = null;
        }
        catch { }

        backend.Output -= Backend_Output;
        backend.Exited -= Backend_Exited;

        while (backendLineQueue.TryDequeue(out _)) { }
        while (priorityBackendLineQueue.TryDequeue(out _)) { }
        latestProgressByChannel.Clear();
        recentStructuredEvents.Clear();
        ResetBackendQueueCounters();
        RecordingItems.Clear();
        OfflineItems.Clear();
        StoppedItems.Clear();
        AlertItems.Clear();
        statusMap.Clear();
        alertMap.Clear();
        logLines.Clear();
        lastGuiLogLine = "";

        try
        {
            if (trayIcon != null)
            {
                trayIcon.Visible = false;
                trayIcon.Dispose();
            }
            trayIcon = null;
            trayReady = false;
        }
        catch { }

        try
        {
            if (!allowRealClose)
                backend.StopNow();
        }
        catch { }

        try { backend.Dispose(); } catch { }
    }

    void ResetBackendQueueCounters()
    {
        Interlocked.Exchange(ref queuedBackendLines, 0);
        Interlocked.Exchange(ref droppedBackendLines, 0);
    }

    void Backend_Output(string line) => EnqueueBackendLine(line);

    void Backend_Exited(int code) =>
        DispatcherQueue.TryEnqueue(() => OnBackendExited(code));

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    static extern bool SetForegroundWindow(IntPtr hWnd);

    void ConfigureWindow()
    {
        try
        {
            var hwnd = WindowNative.GetWindowHandle(this);
            var id = Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = AppWindow.GetFromWindowId(id);
            appWindow.Resize(new Windows.Graphics.SizeInt32(1280, 820));

            var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "SOOPLiveDownloader.ico");
            if (File.Exists(iconPath))
                appWindow.SetIcon(iconPath);
        }
        catch { }
    }

    static string ResolveBackendDirectory()
    {
        var candidate = Path.GetFullPath(
            Path.Combine(AppContext.BaseDirectory, "backend"));
        var watcher = Path.Combine(candidate, "SOOP_LIVE.ps1");

        if (!File.Exists(watcher))
        {
            throw new DirectoryNotFoundException(
                "현재 배포본의 backend 파일을 찾을 수 없습니다.\n" +
                "필요 파일: " + watcher + "\n" +
                "BUILD_EXE.bat으로 publish 폴더를 다시 생성해 주세요.");
        }

        return candidate;
    }

    void LoadStaticFiles()
    {
        ChannelFilePathText.Text = "채널 파일 : " + channelPath;

        if (File.Exists(channelPath))
        {
            SetRawChannelText(File.ReadAllText(channelPath, Encoding.UTF8), markDirty: false);
            LoadChannelItemsFromText(ChannelsText.Text);
            rawChannelTextDirty = false;
        }
        else
        {
            SetRawChannelText(
                "# ENABLED|NAME|ACCOUNT|OUTDIR" + Environment.NewLine,
                markDirty: false);
            ChannelItems.Clear();
            rawChannelTextDirty = false;
        }

        SetChannelChangesDirty(false);
        savedChannelTextSnapshot = NormalizeChannelText(ChannelsText.Text);
        lastProgrammaticRawChannelText = savedChannelTextSnapshot;
        channelTableChangesDirty = false;
        ClearChannelSelection();

        settingsLoading = true;
        clearSoopPassword = false;
        clearCloudflareApiKey = false;
        var cfg = IniService.Read(iniPath);

        string G(string key, string fallback = "") =>
            cfg.TryGetValue(key, out var v) ? v : fallback;

        static bool B(string text, bool fallback = false)
        {
            if (string.IsNullOrWhiteSpace(text)) return fallback;
            return text.Trim().Equals("Y", StringComparison.OrdinalIgnoreCase) ||
                   text.Trim().Equals("YES", StringComparison.OrdinalIgnoreCase) ||
                   text.Trim().Equals("TRUE", StringComparison.OrdinalIgnoreCase) ||
                   text.Trim() == "1";
        }

        static void SetNumber(NumberBox box, string value, double fallback)
        {
            if (double.TryParse(value, out var n)) box.Value = n;
            else box.Value = fallback;
        }

        OutputDirBox.Text = G("OUTPUT_DIR", @"C:\SOOP_LIVE");

        var quality = G("QUALITY", "best").Trim().ToLowerInvariant();
        QualityBox.SelectedItem = quality switch
        {
            "master" => "원본/마스터 (master)",
            "1080p" => "1080p",
            "720p" => "720p",
            _ => "최고 화질 (best)"
        };

        var pattern = G("FILE_NAME_PATTERN", "LEGACY").ToUpperInvariant();
        FileNamePatternBox.SelectedItem = pattern switch
        {
            "TITLE_NUMBER" => "제목 + 번호 — 260826_방송제목_01_BJ.ts",
            "TIME_TITLE" => "시간 + 제목 — 260826_153000_방송제목_BJ.ts",
            "BJ_TITLE" => "BJ + 제목 — 260826_BJ_방송제목.ts",
            _ => "기본 — 260826_153000_BJ.ts"
        };

        SetNumber(MinDiskBox, G("MIN_FREE_SPACE_GB", "20"), 20);

        SoopUsernameBox.Text = G("SOOP_USERNAME", "");
        SoopPasswordBox.Password = "";
        SoopPurgeCredentialsCheck.IsChecked = B(G("SOOP_PURGE_CREDENTIALS", "Y"), true);

        CloudflareWorkerUrlBox.Text = G("CLOUDFLARE_WORKER_URL", "");
        CloudflareApiKeyBox.Password = "";

        var masterQuality = G("MASTER_QUALITY", "auto").Trim().ToLowerInvariant();
        MasterQualityBox.SelectedItem = masterQuality switch
        {
            "master" => "master",
            "1080p" => "1080p",
            "720p" => "720p",
            _ => "자동 (auto)"
        };

        StreamlinkPathBox.Text = G("STREAMLINK_PATH", "AUTO");
        StreamlinkFallbackBox.Text = G("STREAMLINK_FALLBACK", @"C:\Program Files\Streamlink\bin\streamlink.exe");

        SetNumber(CheckIntervalBox, G("CHECK_INTERVAL", "30"), 30);
        SetNumber(ChannelReloadIntervalBox, G("CHANNEL_RELOAD_INTERVAL", "2"), 2);
        SetNumber(RecordRetryIntervalBox, G("RECORD_RETRY_INTERVAL", "5"), 5);
        SetNumber(RecordStallTimeoutBox, G("RECORD_STALL_TIMEOUT", "90"), 90);
        SetNumber(RecordMonitorIntervalBox, G("RECORD_MONITOR_INTERVAL", "5"), 5);
        SetNumber(WorkerMaxRetryBox, G("WORKER_MAX_RETRY", "3"), 3);

        LogEnabledCheck.IsChecked = B(G("LOG_ENABLED", "Y"), true);
        LogDirBox.Text = G("LOG_DIR", @".\logs");
        SetNumber(LogRetentionDaysBox, G("LOG_RETENTION_DAYS", "30"), 30);

        ConsoleAutoFormatCheck.IsChecked = B(G("CONSOLE_AUTO_FORMAT", "Y"), true);
        ConsoleColorCheck.IsChecked = B(G("CONSOLE_COLOR", "Y"), true);
        ConsoleShowPathCheck.IsChecked = B(G("CONSOLE_SHOW_PATH", "N"), false);
        uiNotifyRecordStart = B(G("GUI_NOTIFY_RECORD_START", "N"), false);
        uiNotifyRecordFinish = B(G("GUI_NOTIFY_RECORD_FINISH", "Y"), true);
        uiNotifyWarning = B(G("GUI_NOTIFY_WARNING", "Y"), true);
        NotifyRecordStartCheck.IsChecked = uiNotifyRecordStart;
        NotifyRecordFinishCheck.IsChecked = uiNotifyRecordFinish;
        NotifyWarningCheck.IsChecked = uiNotifyWarning;

        ApplyConsoleDisplayOptions(
            ConsoleAutoFormatCheck.IsChecked == true,
            ConsoleColorCheck.IsChecked == true,
            ConsoleShowPathCheck.IsChecked == true);
        UpdateSecretStatusFix36(cfg);
        RefreshPathStatusFix36();
        RefreshLogPathStatusFix36();
        settingsLoading = false;
        SetSettingsDirtyFix36(false);
    }

    async void StartButton_Click(object sender, RoutedEventArgs e)
    {
        if (watcherStartInProgress)
            return;

        if (backend.IsRunning)
        {
            ApplyWatcherRunningUiFix38();
            return;
        }

        watcherStartInProgress = true;
        watcherStopRequested = false;
        pendingWatcherExitCode = null;
        SetWatcherStartControlsFix38(false);
        try
        {
            var validationError = ValidateBeforeStart();
            if (!string.IsNullOrWhiteSpace(validationError))
            {
                await ShowDialogAsync("Watcher 시작 전 확인", validationError);
                return;
            }

            ResetDashboardState();
            WatcherStateText.Text = "시작 중";
            backend.Start(backendDir);
            dashboardWatcherRunning = true;
            var startupState = await backend.WaitForStartupAsync(TimeSpan.FromSeconds(12));
            if (startupState == BackendProcessService.StartupState.Exited || !backend.IsRunning)
            {
                // Give redirected stdout/stderr a brief chance to finish after
                // the process exit event, then surface the captured cause.
                await Task.Delay(150);
                var details = backend.GetRecentOutput(12).Trim();
                var exitText = backend.LastExitCode is int exitCode
                    ? $"종료 코드: {exitCode}"
                    : "종료 코드를 확인하지 못했습니다.";
                var message = "Watcher가 초기화 중 종료되었습니다.\n" + exitText;
                if (!string.IsNullOrWhiteSpace(details))
                    message += "\n\n백엔드 출력:\n" + details;
                else
                    message += "\n\n백엔드 출력이 없습니다. 프로그램 폴더의 SOOPLiveWinUI_startup.log를 확인해 주세요.";
                throw new InvalidOperationException(message);
            }
            ApplyWatcherRunningUiFix38();
            AppendLog(startupState == BackendProcessService.StartupState.Ready
                ? "[GUI] Watcher 시작 완료"
                : "[GUI] Watcher 프로세스 실행 중 · 백엔드 초기화 응답 대기");
        }
        catch (Exception ex)
        {
            if (backend.IsRunning)
            {
                ApplyWatcherRunningUiFix38();
                AppendLog("[GUI] 시작 응답 확인 중 예외가 있었지만 Watcher는 실행 중: " + ex.Message);
            }
            else
            {
                dashboardWatcherRunning = false;
                WatcherStateText.Text = "시작 실패";
                var failureMessage = DescribeWatcherStartException(ex);
                WriteStartupLog("Watcher startup FAILED", ex);
                await ShowDialogAsync("Watcher 시작 실패", failureMessage);
            }
        }
        finally
        {
            watcherStartInProgress = false;
            SetWatcherStartControlsFix38(!backend.IsRunning);
            StopButton.IsEnabled = backend.IsRunning;
            UpdateDashboardEmptyState();
        }
    }

    void ApplyWatcherRunningUiFix38()
    {
        dashboardWatcherRunning = true;
        SetWatcherStartControlsFix38(false);
        StopButton.IsEnabled = true;
        WatcherStateText.Text = "실행 중";
        UpdateDashboardEmptyState();
    }

    void ReconcileWatcherRunningUiFix39()
    {
        // Every start entry point owns the same BackendProcessService instance.
        // If output or the timer proves that process is alive, repair stale
        // dashboard/header controls instead of trusting an earlier UI result.
        if (watcherStopInProgress || !backend.IsRunning)
            return;

        if (!dashboardWatcherRunning ||
            StartButton.IsEnabled ||
            !StopButton.IsEnabled ||
            !string.Equals(WatcherStateText.Text, "실행 중", StringComparison.Ordinal))
        {
            ApplyWatcherRunningUiFix38();
        }
    }

    void SetWatcherStartControlsFix38(bool enabled)
    {
        StartButton.IsEnabled = enabled;
        if (DashboardEmptyStartButton != null)
            DashboardEmptyStartButton.IsEnabled = enabled;
    }

    async void StopButton_Click(object sender, RoutedEventArgs e)
    {
        if (watcherStopInProgress)
            return;

        watcherStopInProgress = true;
        watcherStopRequested = true;
        StopButton.IsEnabled = false;
        WatcherStateText.Text = "종료 중";
        AppendLog("[GUI] Watcher 종료 요청");

        try
        {
            var stopped = await backend.StopAsync();

            if (!stopped || backend.IsRunning)
            {
                watcherStopRequested = false;
                WatcherStateText.Text = "종료 확인 필요";
                StopButton.IsEnabled = true;
                AppendLog("[GUI] Watcher 종료 실패 · 프로세스 상태를 다시 확인해 주세요.");

                if (pendingWatcherExitCode is int exitCode)
                {
                    pendingWatcherExitCode = null;
                    CompleteWatcherExit(exitCode, requestedStop: false);
                }
            }
            else if (pendingWatcherExitCode is int exitCode)
            {
                pendingWatcherExitCode = null;
                CompleteWatcherExit(exitCode, requestedStop: true);
            }
        }
        catch (Exception ex)
        {
            watcherStopRequested = false;
            WatcherStateText.Text = "종료 확인 필요";
            StopButton.IsEnabled = backend.IsRunning;
            AppendLog("[GUI] Watcher 종료 확인 실패: " + ex.Message);

            if (pendingWatcherExitCode is int exitCode)
            {
                pendingWatcherExitCode = null;
                CompleteWatcherExit(exitCode, requestedStop: false);
            }
        }
        finally
        {
            watcherStopInProgress = false;
            ReconcileWatcherRunningUiFix39();
        }
    }

    void OnBackendExited(int code)
    {
        if (watcherStopInProgress)
        {
            // StopAsync verifies exact-process-tree termination off the UI
            // thread. Defer classification until that result is available.
            pendingWatcherExitCode = code;
            return;
        }

        CompleteWatcherExit(code, watcherStopRequested);
    }

    void CompleteWatcherExit(int code, bool requestedStop)
    {
        watcherStopRequested = false;
        pendingWatcherExitCode = null;
        watcherStopInProgress = false;
        dashboardWatcherRunning = false;
        ResetDashboardState();
        SetWatcherStartControlsFix38(true);
        StopButton.IsEnabled = false;
        // taskkill terminates the GUI-owned PowerShell process tree and can
        // produce a non-zero process exit code. That is still a normal stop
        // when it directly follows the user's explicit Watcher stop request.
        WatcherStateText.Text = requestedStop || code == 0
            ? "중지됨"
            : $"오류 종료 ({code})";
        OfflineCountText.Text = "-";
        StoppedCountText.Text = "-";
        AlertCountText.Text = "-";
        UpdateDashboardEmptyState();
        AppendLog(requestedStop
            ? $"[GUI] Watcher 사용자 요청으로 중지 Exit={code}"
            : $"[GUI] Watcher 종료 Exit={code}");
    }

    void ResetDashboardState()
    {
        while (backendLineQueue.TryDequeue(out _))
            Interlocked.Decrement(ref queuedBackendLines);
        while (priorityBackendLineQueue.TryDequeue(out _)) { }
        if (Interlocked.Read(ref queuedBackendLines) < 0)
            Interlocked.Exchange(ref queuedBackendLines, 0);
        Interlocked.Exchange(ref droppedBackendLines, 0);
        latestProgressByChannel.Clear();
        recentStructuredEvents.Clear();
        RecordingItems.Clear();
        OfflineItems.Clear();
        StoppedItems.Clear();
        AlertItems.Clear();
        statusMap.Clear();
        alertMap.Clear();
        pendingRecordChannel = null;
        pendingRecordAccount = null;
        pendingRecordTitle = null;
        // These ListViews use single-selection mode. Mutating SelectedItems in
        // that mode can throw a WinRT E_ILLEGAL_METHOD_CALL before the watcher
        // process is even started; clear the single SelectedItem instead.
        if (RecordingList != null)
            RecordingList.SelectedItem = null;
        if (StoppedFlyoutList != null)
            StoppedFlyoutList.SelectedItem = null;
        UpdateCounts();
    }

    static string DescribeWatcherStartException(Exception ex)
    {
        if (!string.IsNullOrWhiteSpace(ex.Message))
            return ex.Message;

        var exceptionType = ex.GetType().FullName ?? ex.GetType().Name;
        var details = $"예외 형식: {exceptionType}\nHRESULT: 0x{ex.HResult:X8}";
        if (ex.InnerException is { } inner)
        {
            var innerType = inner.GetType().FullName ?? inner.GetType().Name;
            details += $"\n내부 예외: {innerType}";
            if (!string.IsNullOrWhiteSpace(inner.Message))
                details += "\n" + inner.Message;
        }

        return "Watcher 시작 준비 중 오류가 발생했습니다.\n" + details;
    }
}
