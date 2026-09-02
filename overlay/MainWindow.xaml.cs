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

public sealed partial class MainWindow : Window
{
    readonly BackendProcessService backend = new();
    readonly string backendDir;
    readonly string iniPath;
    readonly string channelPath;

    readonly Dictionary<string, ChannelStatus> statusMap =
        new(StringComparer.OrdinalIgnoreCase);
    readonly Dictionary<string, ChannelStatus> alertMap =
        new(StringComparer.OrdinalIgnoreCase);

    string? pendingRecordChannel;
    string? pendingRecordAccount;
    string? pendingRecordTitle;

    public ObservableCollection<ChannelStatus> RecordingItems { get; } = new();
    public ObservableCollection<ChannelStatus> OfflineItems { get; } = new();
    public ObservableCollection<ChannelStatus> StoppedItems { get; } = new();
    public ObservableCollection<ChannelStatus> AlertItems { get; } = new();
    public ObservableCollection<EditableChannel> ChannelItems { get; } = new();
    public ObservableCollection<EditableChannel> VisibleChannelItems { get; } = new();

    NavigationView Nav = null!;
    Button StartButton = null!;
    Button StopButton = null!;
    TextBlock RecordingCountText = null!;
    TextBlock OfflineCountText = null!;
    TextBlock StoppedCountText = null!;
    TextBlock AlertCountText = null!;
    TextBlock WatcherStateText = null!;
    TextBlock DiskSummaryText = null!;
    Button StopSelectedRecordingButton = null!;
    Button OpenSelectedRecordingFolderButton = null!;
    Button OfflineSummaryButton = null!;
    Button StoppedSummaryButton = null!;
    Button AlertSummaryButton = null!;
    ListView OfflineFlyoutList = null!;
    ListView StoppedFlyoutList = null!;
    ListView AlertFlyoutList = null!;
    Button ResumeStoppedButton = null!;
    ListView RecordingList = null!;
    StackPanel DashboardEmptyStatePanel = null!;
    TextBlock DashboardEmptyTitleText = null!;
    TextBlock DashboardEmptyDetailText = null!;
    Button DashboardEmptyStartButton = null!;
    TextBox ChannelsText = null!;
    TextBox OutputDirBox = null!;
    ComboBox QualityBox = null!;
    ComboBox FileNamePatternBox = null!;
    NumberBox MinDiskBox = null!;

    TextBox SoopUsernameBox = null!;
    PasswordBox SoopPasswordBox = null!;
    CheckBox SoopPurgeCredentialsCheck = null!;

    TextBox CloudflareWorkerUrlBox = null!;
    PasswordBox CloudflareApiKeyBox = null!;
    ComboBox MasterQualityBox = null!;

    TextBox StreamlinkPathBox = null!;
    TextBox StreamlinkFallbackBox = null!;

    CheckBox LogEnabledCheck = null!;
    TextBox LogDirBox = null!;
    NumberBox LogRetentionDaysBox = null!;

    NumberBox CheckIntervalBox = null!;
    NumberBox ChannelReloadIntervalBox = null!;
    NumberBox RecordRetryIntervalBox = null!;
    NumberBox RecordStallTimeoutBox = null!;
    NumberBox RecordMonitorIntervalBox = null!;
    NumberBox WorkerMaxRetryBox = null!;

    CheckBox ConsoleAutoFormatCheck = null!;
    CheckBox ConsoleColorCheck = null!;
    CheckBox ConsoleShowPathCheck = null!;
    CheckBox NotifyRecordStartCheck = null!;
    CheckBox NotifyRecordFinishCheck = null!;
    CheckBox NotifyWarningCheck = null!;

    AppBarButton SaveChannelsButton = null!;
    Button ReloadChannelsButton = null!;
    AppBarButton AddChannelButton = null!;
    AppBarButton ImportChannelsButton = null!;
    AppBarButton SelectedChannelActionsButton = null!;
    MenuFlyoutItem DeleteChannelMenuItem = null!;
    MenuFlyoutItem EnableSelectedChannelsMenuItem = null!;
    MenuFlyoutItem DisableSelectedChannelsMenuItem = null!;
    AppBarButton EditChannelButton = null!;
    Button ApplyRawChannelsButton = null!;
    ListView ChannelList = null!;
    CheckBox SelectAllChannelsCheckBox = null!;
    TextBlock SelectedChannelCountText = null!;
    TextBlock ChannelFilePathText = null!;
    TextBlock ChannelDirtyStateText = null!;
    TextBox ChannelSearchBox = null!;
    ComboBox ChannelFilterBox = null!;
    ComboBox UiDensityBox = null!;
    bool rawChannelTextDirty = false;
    bool suppressRawChannelTextChanged = false;
    string lastProgrammaticRawChannelText = "";
    string savedChannelTextSnapshot = "";
    bool channelTableChangesDirty = false;
    bool channelChangesDirty = false;
    bool suppressChannelSelectionSync = false;
    bool suppressChannelCollectionRefresh = false;
    Microsoft.UI.Dispatching.DispatcherQueueTimer? channelSearchDebounceTimer;
    bool suppressNavigationSelectionChanged = false;
    bool channelNavigationPromptOpen = false;
    bool dashboardWatcherRunning = false;
    bool watcherStartInProgress = false;
    bool watcherStopInProgress = false;
    bool watcherStopRequested = false;
    int? pendingWatcherExitCode = null;
    string currentViewTag = "dashboard";
    Button SaveSettingsButton = null!;
    TextBox LogBox = null!;
    FrameworkElement DashboardView = null!;
    FrameworkElement ChannelsView = null!;
    FrameworkElement SettingsView = null!;
    FrameworkElement VodViewHost = null!;
    FrameworkElement LogsView = null!;

    bool uiAutoFormat = true;
    bool uiConsoleColor = true;
    bool uiShowPath = false;
    bool uiNotifyRecordStart = false;
    bool uiNotifyRecordFinish = true;
    bool uiNotifyWarning = true;
    bool windowCleanupDone = false;
    bool allowRealClose = false;
    bool closeDialogOpen = false;
    FormsNotifyIcon? trayIcon;
    bool trayReady = false;
    UiPreferences uiPreferences = UiPreferences.Load();
    Microsoft.UI.Dispatching.DispatcherQueueTimer? uiPreferencesSaveTimer;
    readonly ConcurrentQueue<string> backendLineQueue = new();
    readonly BoundedConcurrentQueue<string> priorityBackendLineQueue = new(MaxQueuedPriorityEvents);
    readonly ConcurrentDictionary<string, ProgressSnapshot> latestProgressByChannel =
        new(StringComparer.OrdinalIgnoreCase);
    readonly WarningDeduplicator backendWarningDeduplicator = new(TimeSpan.FromSeconds(30));
    readonly DriveSpaceCache driveSpaceCache = new(
        TimeSpan.FromSeconds(5),
        root =>
        {
            try
            {
                var drive = new DriveInfo(root);
                return drive.IsReady ? (true, drive.AvailableFreeSpace) : (false, 0L);
            }
            catch { return (false, 0L); }
        });
    readonly Queue<string> logLines = new();
    readonly Dictionary<string, DateTime> recentStructuredEvents =
        new(StringComparer.OrdinalIgnoreCase);
    string lastGuiLogLine = "";
    bool logTextDirty = false;

    DispatcherTimer? uiFlushTimer;
    long queuedBackendLines = 0;
    long flushedBackendLines = 0;
    long droppedBackendLines = 0;
    long pendingSuppressedWarningLines = 0;
    DateTime lastUiFlush = DateTime.MinValue;
    DateTime lastDiskEstimateRefresh = DateTime.MinValue;
    DateTime lastWarningDedupReport = DateTime.MinValue;
    const int MaxGuiLogLines = 50;
    const int MaxQueuedBackendEvents = 2000;
    const int MaxQueuedPriorityEvents = 512;
    const int UiFlushMilliseconds = 250;
    static readonly string[] GuiLogTokens =
    {
        "WATCHER", "RECORD START", "RECORD FINISHED", "RECORDER EXIT", "RECORD STALLED",
        "RETRY", "LOW DISK", "DISK UNKNOWN", "CHANNEL STOP", "CHANNEL DISABLED", "CHANNEL REMOVED",
        "RECHECK",
        "SETTING", "HOT RELOAD", "AUTH", "WORKER", "[ERROR]", "[WARN]", "ERROR", "FAILED"
    };
    static readonly HttpClient SoopProfileClient = new()
    {
        Timeout = TimeSpan.FromSeconds(10)
    };



    static readonly SolidColorBrush Bg = DesignTokens.AppBackground;
    static readonly SolidColorBrush Card = DesignTokens.Surface;
    static readonly SolidColorBrush White = DesignTokens.TextPrimary;
    static readonly SolidColorBrush Muted = DesignTokens.TextSecondary;
    static readonly SolidColorBrush Accent = DesignTokens.Accent;

    static Button ApplyButtonMetricsFix39(Button button, double minWidth = 92)
    {
        return DesignTokens.StyleButton(button, minWidth);
    }

    static NavigationViewItem NavigationItemFix39(string text, string tag, Symbol symbol)
    {
        var item = new NavigationViewItem
        {
            Content = text,
            Tag = tag,
            Icon = new SymbolIcon(symbol)
        };
        ToolTipService.SetToolTip(item, text);
        AutomationProperties.SetName(item, text);
        return item;
    }


    static readonly Regex DownloadProgressWithPath = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+\[download\]\s+Written\s+(?<size>.+?)\s+to\s+(?<file>.+?)\s+\((?<duration>\d{2}:\d{2}:\d{2})\s+@\s+(?<rate>.+?)\)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex DownloadProgressNoPath = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+\[download\]\s+Written\s+(?<size>.+?)\s+\((?<duration>\d{2}:\d{2}:\d{2})\s+@\s+(?<rate>.+?)\)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex CompactRecording = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*RECORDING\s*\|\s*(?<progress>\[download\].*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex PlainOffline = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*OFFLINE$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordStartChannelLine = new(
        @"^Channel\s*:\s*(?<value>.+)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordStartTitleLine = new(
        @"^Title\s*:\s*(?<value>.*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordStartAccountLine = new(
        @"^Account\s*:\s*(?<value>[A-Za-z0-9_]+)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordStartOutputLine = new(
        @"^Output\s*:\s*(?<value>.+)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordFinishedLog = new(
        @"RECORD FINISHED channel=(?<name>.+?) duration=(?<duration>\S+) size=(?<size>.+?) reason=(?<reason>.+?) file=(?<file>.+)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex RecordFinishedEvent = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)\s+\[account=(?<account>[A-Za-z0-9_]+)\]\s*:\s*RECORD FINISHED\s*\|\s*duration=(?<duration>[^|]*)\|\s*size=(?<size>[^|]*)\|\s*reason=(?<reason>[^|]*)\|\s*file=(?<file>.*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex ChannelStopRequested = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*CHANNEL STOP REQUESTED BNO=(?<bno>\S*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex ChannelStopCompleted = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*CHANNEL STOP COMPLETED BNO=(?<bno>\S*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex ChannelStopFailed = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*CHANNEL STOP FAILED(?:\s*-\s*(?<error>.*))?$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex ChannelResumeRequested = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*CHANNEL RESUME REQUESTED(?: BNO=(?<bno>\S*))?$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex ChannelRemovedOrDisabled = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)\s+\[account=(?<account>[A-Za-z0-9_]+)\]\s*:\s*CHANNEL\s+(?<action>REMOVED|DISABLED)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex DashboardHealthState = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*(?<status>LOW DISK(?: SPACE)?|DISK SPACE UNKNOWN|CHECK ERROR|LOGIN REQUIRED|RECORD START FAILED|WORKER COOLDOWN)(?:\s*-\s*(?<detail>.*)|\s*\((?<detail2>.*)\))?$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex DashboardHealthCleared = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*DISK SPACE OK",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public MainWindow()
    {
        WriteStartupLog("MainWindow constructor entered");

        DesignTokens.ApplyDensity(uiPreferences.UiDensity);

        try
        {
            // This is Microsoft's untouched official MainWindow.xaml.
            InitializeComponent();
            WriteStartupLog("Official Vanilla MainWindow InitializeComponent OK");
        }
        catch (Exception ex)
        {
            WriteStartupLog("Official Vanilla MainWindow InitializeComponent FAILED", ex);
            throw;
        }

        try
        {
            Content = BuildUi();
            WriteStartupLog("SOOP programmatic Content assignment OK");
        }
        catch (Exception ex)
        {
            WriteStartupLog("SOOP programmatic UI FAILED", ex);
            throw;
        }

        Title = "SOOP LIVE Downloader";

        try
        {
            backendDir = ResolveBackendDirectory();
            WriteStartupLog("Backend directory resolved: " + backendDir);
        }
        catch (Exception ex)
        {
            WriteStartupLog("Backend directory resolution FAILED", ex);
            throw;
        }

        iniPath = Path.Combine(backendDir, "SOOP_LIVE_SETTING.ini");
        channelPath = Path.Combine(backendDir, "SOOP_LIVE_CHANNELS.txt");

        backend.Output += Backend_Output;
        backend.Exited += Backend_Exited;

        ConfigureWindow();
        LoadStaticFiles();
        RefreshVodLoginAvailability();
        LoadRecentRecordingsFix51();
        InitializeTrayIcon();
        HookAppWindowClosing();
        InitializeUiFlushTimer();

        RestoreInitialViewFix59();

        Closed += MainWindow_Closed;

        WriteStartupLog($"MainWindow constructor completed; UI batching={UiFlushMilliseconds}ms; logLines={MaxGuiLogLines}");
    }

    FrameworkElement BuildUi()
    {
        var root = new Grid { Background = Bg, RequestedTheme = ElementTheme.Dark };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var header = new Grid
        {
            Background = MakeBrush("#F7F8FA"),
            Padding = new Thickness(18, 13, 18, 13),
            ColumnSpacing = 12,
            RequestedTheme = ElementTheme.Light
        };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var titleStack = new StackPanel();
        titleStack.Children.Add(new TextBlock
        {
            Text = "SOOP LIVE Downloader",
            Foreground = MakeBrush("#111111"),
            FontSize = 22,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        titleStack.Children.Add(new TextBlock
        {
            Text = "WinUI 3 · v1.2.0-preview1-fix80",
            Foreground = MakeBrush("#667085"),
            FontSize = 12
        });

        StartButton = DesignTokens.StyleButton(new Button { Content = "▶ Watcher 시작" }, 126, primary: true);
        StopButton = DesignTokens.StyleButton(new Button
        {
            Content = "■ Watcher 중지",
            IsEnabled = false,
            Foreground = DesignTokens.Danger
        }, 126);
        AutomationProperties.SetName(StartButton, "Watcher 시작");
        AutomationProperties.SetName(StopButton, "Watcher 중지");
        StartButton.Click += StartButton_Click;
        StopButton.Click += StopButton_Click;
        DesignTokens.AddAccelerator(StartButton, Windows.System.VirtualKey.R, Windows.System.VirtualKeyModifiers.Control);
        DesignTokens.AddAccelerator(
            StopButton,
            Windows.System.VirtualKey.R,
            Windows.System.VirtualKeyModifiers.Control | Windows.System.VirtualKeyModifiers.Shift);

        var buttonStack = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            VerticalAlignment = VerticalAlignment.Center
        };
        buttonStack.Children.Add(StartButton);
        buttonStack.Children.Add(StopButton);

        Grid.SetColumn(buttonStack, 1);
        header.Children.Add(titleStack);
        header.Children.Add(buttonStack);
        root.Children.Add(header);

        Nav = new NavigationView
        {
            RequestedTheme = ElementTheme.Dark,
            IsBackButtonVisible = NavigationViewBackButtonVisible.Collapsed,
            IsSettingsVisible = false,
            PaneDisplayMode = NavigationViewPaneDisplayMode.LeftCompact,
            CompactPaneLength = 52,
            OpenPaneLength = 210
        };

        Nav.MenuItems.Add(NavigationItemFix39("대시보드", "dashboard", Symbol.Home));
        Nav.MenuItems.Add(NavigationItemFix39("채널 관리", "channels", Symbol.People));
        Nav.MenuItems.Add(NavigationItemFix39("설정", "settings", Symbol.Setting));
        Nav.MenuItems.Add(NavigationItemFix39("최근 녹화", "recent", Symbol.Video));
        Nav.MenuItems.Add(NavigationItemFix39("VOD 다운로드", "vod", Symbol.Download));
        Nav.MenuItems.Add(NavigationItemFix39("로그", "logs", Symbol.Document));
        Nav.SelectionChanged += Nav_SelectionChanged;

        var contentRoot = new Grid { Background = Bg };
        DashboardView = BuildDashboard();
        ChannelsView = BuildChannelsView();
        SettingsView = BuildSettingsViewFix36();
        RecentRecordingsView = BuildRecentRecordingsViewFix51();
        VodViewHost = BuildVodView();
        LogsView = BuildLogsView();

        contentRoot.Children.Add(DashboardView);
        contentRoot.Children.Add(ChannelsView);
        contentRoot.Children.Add(SettingsView);
        contentRoot.Children.Add(RecentRecordingsView);
        contentRoot.Children.Add(VodViewHost);
        contentRoot.Children.Add(LogsView);

        Nav.Content = contentRoot;
        Grid.SetRow(Nav, 1);
        root.Children.Add(Nav);

        return root;
    }

    static void WriteStartupLog(string message, Exception? ex = null)
    {
        try
        {
            var path = Path.Combine(AppContext.BaseDirectory, "SOOPLiveWinUI_startup.log");
            var text = $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] {message}{Environment.NewLine}";
            if (ex != null)
                text += ex + Environment.NewLine;
            File.AppendAllText(path, text, new UTF8Encoding(false));
        }
        catch { }
    }

    async void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (suppressNavigationSelectionChanged ||
            args.SelectedItemContainer?.Tag is not string tag ||
            tag == currentViewTag)
        {
            return;
        }

        if (currentViewTag == "channels" && channelChangesDirty)
        {
            if (channelNavigationPromptOpen)
            {
                RestoreNavigationSelection();
                return;
            }

            channelNavigationPromptOpen = true;
            var canLeave = false;
            try
            {
                canLeave = await ConfirmLeaveChannelsAsync();
            }
            finally
            {
                channelNavigationPromptOpen = false;
            }

            if (!canLeave)
            {
                RestoreNavigationSelection();
                return;
            }
        }

        if (currentViewTag == "settings" && settingsChangesDirty)
        {
            var canLeaveSettings = await ConfirmLeaveSettingsFix36Async();
            if (!canLeaveSettings)
            {
                RestoreNavigationSelection();
                return;
            }
        }

        currentViewTag = tag;
        ShowView(tag);
        uiPreferences.LastView = tag;
        ScheduleUiPreferencesSaveFix59();
    }

    void RestoreNavigationSelection()
    {
        var previous = Nav.MenuItems
            .OfType<NavigationViewItem>()
            .FirstOrDefault(x => string.Equals(x.Tag as string, currentViewTag, StringComparison.Ordinal));

        if (previous == null)
            return;

        suppressNavigationSelectionChanged = true;
        try
        {
            Nav.SelectedItem = previous;
        }
        finally
        {
            suppressNavigationSelectionChanged = false;
        }
    }

    void ShowView(string tag)
    {
        DashboardView.Visibility = tag == "dashboard" ? Visibility.Visible : Visibility.Collapsed;
        ChannelsView.Visibility = tag == "channels" ? Visibility.Visible : Visibility.Collapsed;
        SettingsView.Visibility = tag == "settings" ? Visibility.Visible : Visibility.Collapsed;
        RecentRecordingsView.Visibility = tag == "recent" ? Visibility.Visible : Visibility.Collapsed;
        VodViewHost.Visibility = tag == "vod" ? Visibility.Visible : Visibility.Collapsed;
        LogsView.Visibility = tag == "logs" ? Visibility.Visible : Visibility.Collapsed;

        if (tag is "channels" or "settings")
            LoadStaticFiles();
        if (tag == "vod")
            RefreshVodLoginAvailability();
    }

    void RestoreInitialViewFix59()
    {
        currentViewTag = uiPreferences.LastView;
        var item = Nav.MenuItems.OfType<NavigationViewItem>()
            .FirstOrDefault(candidate => string.Equals(
                candidate.Tag as string,
                currentViewTag,
                StringComparison.Ordinal));
        if (item == null)
        {
            currentViewTag = "dashboard";
            item = Nav.MenuItems.OfType<NavigationViewItem>().First();
        }
        suppressNavigationSelectionChanged = true;
        try { Nav.SelectedItem = item; }
        finally { suppressNavigationSelectionChanged = false; }
        ShowView(currentViewTag);
    }

    string? ValidateBeforeStart()
    {
        var cfg = IniService.Read(iniPath);

        string G(string key) => cfg.TryGetValue(key, out var v) ? v?.Trim() ?? "" : "";

        if (string.IsNullOrWhiteSpace(G("CLOUDFLARE_WORKER_URL")))
            return "Cloudflare Worker URL이 비어 있습니다.";

        if (!G("CLOUDFLARE_WORKER_URL").StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            return "Cloudflare Worker URL은 https:// 로 시작해야 합니다.";

        if (string.IsNullOrWhiteSpace(G("CLOUDFLARE_API_KEY")))
            return "Cloudflare API Key가 비어 있습니다.";

        var output = G("OUTPUT_DIR");
        if (string.IsNullOrWhiteSpace(output))
            return "기본 녹화 경로가 비어 있습니다.";

        try
        {
            Directory.CreateDirectory(output);
        }
        catch (Exception ex)
        {
            return "기본 녹화 경로를 사용할 수 없습니다.\n" + ex.Message;
        }

        if (ChannelItems.Count(x => x.Enabled) == 0)
            return "활성화된 채널이 없습니다.";

        return null;
    }

    static void AtomicWriteAllText(string path,string content,Encoding encoding)
    {
        var full=Path.GetFullPath(path);
        var dir=Path.GetDirectoryName(full) ?? throw new InvalidOperationException("파일 디렉터리를 확인할 수 없습니다.");
        Directory.CreateDirectory(dir);
        var temp=Path.Combine(dir,"."+Path.GetFileName(full)+"."+Guid.NewGuid().ToString("N")+".tmp");
        try
        {
            File.WriteAllText(temp,content,encoding);
            _=File.ReadAllText(temp,encoding);
            if(File.Exists(full))
            {
                var rb=full+".replace.bak";
                try
                {
                    File.Replace(temp,full,rb,true);
                    try{File.Delete(rb);}catch{}
                }
                catch(PlatformNotSupportedException){File.Move(temp,full,true);}
                catch(IOException){File.Move(temp,full,true);}
            }
            else File.Move(temp,full);
        }
        finally{try{if(File.Exists(temp))File.Delete(temp);}catch{}}
    }

    static void BackupFile(string path)
    {
        if (!File.Exists(path))
            return;

        try
        {
            File.Copy(path, path + ".bak", true);
        }
        catch { }
    }

    static void UpdateIniFile(string path, Dictionary<string, string> updates)
    {
        var lines = File.Exists(path)
            ? File.ReadAllLines(path, Encoding.UTF8).ToList()
            : new List<string>();

        var done = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        for (int i = 0; i < lines.Count; i++)
        {
            var raw = lines[i];
            var trimmed = raw.Trim();
            if (trimmed.Length == 0 || trimmed.StartsWith("#") || trimmed.StartsWith(";") || trimmed.StartsWith("["))
                continue;

            var eq = trimmed.IndexOf('=');
            if (eq <= 0) continue;

            var key = trimmed[..eq].Trim();
            if (!updates.TryGetValue(key, out var value)) continue;

            lines[i] = $"{key}={value}";
            done.Add(key);
        }

        foreach (var kv in updates)
        {
            if (!done.Contains(kv.Key))
                lines.Add($"{kv.Key}={kv.Value}");
        }

        var content=string.Join(Environment.NewLine,lines);
        if(lines.Count>0) content+=Environment.NewLine;
        AtomicWriteAllText(path,content,new UTF8Encoding(false));
    }

    void ClearLog_Click(object sender, RoutedEventArgs e)
    {
        logLines.Clear();
        lastGuiLogLine = "";
        LogBox.Text="";
    }

    void OpenBackend_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var startInfo = new ProcessStartInfo("explorer.exe") { UseShellExecute = true };
            startInfo.ArgumentList.Add(Path.GetFullPath(backendDir));
            Process.Start(startInfo);
        }
        catch { }
    }

    bool IsGuiEventLogLine(string line)
    {
        if (string.IsNullOrWhiteSpace(line)) return false;
        var text=line.Trim();
        if (text.StartsWith('<') || text.StartsWith('"') ||
            text is "{" or "}" or "[" or "]") return false;
        // The frequent dashboard/progress rows have stable text markers.
        // Avoid running four regular expressions for every such line.
        if (text.Contains(" : RECORDING | ", StringComparison.OrdinalIgnoreCase) ||
            text.EndsWith(" : OFFLINE", StringComparison.OrdinalIgnoreCase) ||
            text.Contains("[download]", StringComparison.OrdinalIgnoreCase)) return false;
        var u=text.ToUpperInvariant();
        foreach (var token in GuiLogTokens)
            if (u.Contains(token, StringComparison.Ordinal)) return true;
        return false;
    }

    void AppendLog(string line)
    {
        if (!IsGuiEventLogLine(line)) return;
        line = line.Trim();
        const int maxEventCharacters = 600;
        if (line.Length > maxEventCharacters)
            line = line[..maxEventCharacters] + " … [truncated]";
        if (string.Equals(lastGuiLogLine, line, StringComparison.Ordinal))
            return;
        lastGuiLogLine = line;
        logLines.Enqueue(line);
        while(logLines.Count>MaxGuiLogLines) logLines.Dequeue();
        logTextDirty = true;
    }

    void FlushLogText()
    {
        if (!logTextDirty)
            return;

        var next = string.Join(Environment.NewLine, logLines);

        if (string.Equals(LogBox.Text, next, StringComparison.Ordinal))
        {
            logTextDirty = false;
            return;
        }

        LogBox.Text = next;
        logTextDirty = false;

        try
        {
            LogBox.Select(LogBox.Text.Length, 0);
        }
        catch { }
    }

    async Task ShowDialogAsync(string title, string content)
    {
        if (string.IsNullOrWhiteSpace(content))
            content = "오류 상세 내용이 전달되지 않았습니다. 프로그램 로그를 확인해 주세요.";

        var dialog = new ContentDialog
        {
            Title = title,
            Content = content,
            CloseButtonText = "확인",
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };
        await dialog.ShowAsync();
    }
}
