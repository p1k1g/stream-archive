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

    Button SaveChannelsButton = null!;
    Button ReloadChannelsButton = null!;
    Button AddChannelButton = null!;
    Button ImportChannelsButton = null!;
    Button SelectedChannelActionsButton = null!;
    MenuFlyoutItem DeleteChannelMenuItem = null!;
    MenuFlyoutItem EnableSelectedChannelsMenuItem = null!;
    MenuFlyoutItem DisableSelectedChannelsMenuItem = null!;
    Button EditChannelButton = null!;
    Button ApplyRawChannelsButton = null!;
    ListView ChannelList = null!;
    CheckBox SelectAllChannelsCheckBox = null!;
    TextBlock SelectedChannelCountText = null!;
    TextBlock ChannelFilePathText = null!;
    TextBlock ChannelDirtyStateText = null!;
    TextBox ChannelSearchBox = null!;
    ComboBox ChannelFilterBox = null!;
    bool rawChannelTextDirty = false;
    bool suppressRawChannelTextChanged = false;
    string lastProgrammaticRawChannelText = "";
    string savedChannelTextSnapshot = "";
    bool channelTableChangesDirty = false;
    bool channelChangesDirty = false;
    bool suppressChannelSelectionSync = false;
    bool suppressChannelCollectionRefresh = false;
    bool suppressNavigationSelectionChanged = false;
    bool channelNavigationPromptOpen = false;
    bool dashboardWatcherRunning = false;
    bool watcherStartInProgress = false;
    bool watcherStopInProgress = false;
    string currentViewTag = "dashboard";
    Button SaveSettingsButton = null!;
    TextBox LogBox = null!;
    FrameworkElement DashboardView = null!;
    FrameworkElement ChannelsView = null!;
    FrameworkElement SettingsView = null!;
    FrameworkElement LogsView = null!;

    bool uiAutoFormat = true;
    bool uiConsoleColor = true;
    bool uiShowPath = false;
    bool windowCleanupDone = false;
    bool allowRealClose = false;
    bool closeDialogOpen = false;
    FormsNotifyIcon? trayIcon;
    bool trayReady = false;
    UiPreferences uiPreferences = UiPreferences.Load();
    readonly ConcurrentQueue<string> backendLineQueue = new();
    readonly Queue<string> logLines = new();
    bool logTextDirty = false;
    readonly Dictionary<string, string> pendingProgressByChannel =
        new(StringComparer.OrdinalIgnoreCase);

    DispatcherTimer? uiFlushTimer;
    long queuedBackendLines = 0;
    long flushedBackendLines = 0;
    DateTime lastUiFlush = DateTime.MinValue;
    const int MaxGuiLogLines = 50;
    const int UiFlushMilliseconds = 250;
    static readonly string[] GuiLogTokens =
    {
        "WATCHER", "RECORD START", "RECORD FINISHED", "RECORDER EXIT", "RECORD STALLED",
        "RETRY", "LOW DISK", "DISK UNKNOWN", "CHANNEL STOP", "CHANNEL DISABLED", "CHANNEL REMOVED",
        "SETTING", "HOT RELOAD", "AUTH", "WORKER", "[ERROR]", "[WARN]", "ERROR", "FAILED"
    };
    static readonly HttpClient SoopProfileClient = new()
    {
        Timeout = TimeSpan.FromSeconds(10)
    };



    static readonly SolidColorBrush Bg = MakeBrush("#252A31");
    static readonly SolidColorBrush Card = MakeBrush("#20252C");
    static readonly SolidColorBrush White = MakeBrush("#FFFFFF");
    static readonly SolidColorBrush Muted = MakeBrush("#A7B0BE");
    static readonly SolidColorBrush Accent = MakeBrush("#42D987");

    static Button ApplyButtonMetricsFix39(Button button, double minWidth = 92)
    {
        button.MinHeight = 34;
        button.MinWidth = minWidth;
        button.Padding = new Thickness(14, 6, 14, 6);
        return button;
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

    static readonly Regex RecordStart = new(
        @"^\[(?<date>[^]]+)\] \[INFO\] RECORD START channel=(?<name>.+?) account=(?<account>[A-Za-z0-9_]+) bno=(?<bno>\S+) (?<rest>.+)$",
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

    static readonly Regex DashboardHealthState = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*(?<status>LOW DISK(?: SPACE)?|DISK SPACE UNKNOWN|CHECK ERROR|LOGIN REQUIRED|RECORD START FAILED)(?:\s*-\s*(?<detail>.*)|\s*\((?<detail2>.*)\))?$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex DashboardHealthCleared = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)(?:\s+\[account=(?<account>[A-Za-z0-9_]+)\])?\s*:\s*DISK SPACE OK",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public MainWindow()
    {
        WriteStartupLog("MainWindow constructor entered");

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

        backend.Output += line => EnqueueBackendLine(line);
        backend.Exited += code => DispatcherQueue.TryEnqueue(() => OnBackendExited(code));

        ConfigureWindow();
        LoadStaticFiles();
        InitializeTrayIcon();
        HookAppWindowClosing();
        InitializeUiFlushTimer();

        Nav.SelectedItem = Nav.MenuItems[0];
        ShowView("dashboard");

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
            Text = "WinUI 3 · v1.2.0-preview1-fix40",
            Foreground = MakeBrush("#667085"),
            FontSize = 12
        });

        StartButton = ApplyButtonMetricsFix39(new Button { Content = "▶ Watcher 시작" }, 126);
        StopButton = ApplyButtonMetricsFix39(new Button { Content = "■ Watcher 중지", IsEnabled = false }, 126);
        StartButton.Click += StartButton_Click;
        StopButton.Click += StopButton_Click;

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
        Nav.MenuItems.Add(NavigationItemFix39("로그", "logs", Symbol.Document));
        Nav.SelectionChanged += Nav_SelectionChanged;

        var contentRoot = new Grid { Background = Bg };
        DashboardView = BuildDashboard();
        ChannelsView = BuildChannelsView();
        SettingsView = BuildSettingsViewFix36();
        LogsView = BuildLogsView();

        contentRoot.Children.Add(DashboardView);
        contentRoot.Children.Add(ChannelsView);
        contentRoot.Children.Add(SettingsView);
        contentRoot.Children.Add(LogsView);

        Nav.Content = contentRoot;
        Grid.SetRow(Nav, 1);
        root.Children.Add(Nav);

        return root;
    }

    FrameworkElement BuildDashboard()
    {
        var scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto
        };

        var stack = new StackPanel
        {
            Padding = new Thickness(18),
            Spacing = 14
        };

        var summary = new Grid { ColumnSpacing = 8 };
        for (int i = 0; i < 6; i++)
            summary.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        RecordingCountText = new TextBlock
        {
            Text = "0",
            Foreground = Accent,
            FontSize = 28,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };
        OfflineCountText = new TextBlock
        {
            Text = "0",
            Foreground = White,
            FontSize = 28,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };
        StoppedCountText = new TextBlock
        {
            Text = "0",
            Foreground = MakeBrush("#F4C95D"),
            FontSize = 28,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };
        AlertCountText = new TextBlock
        {
            Text = "0",
            Foreground = MakeBrush("#F08080"),
            FontSize = 28,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };
        WatcherStateText = new TextBlock
        {
            Text = "중지됨",
            Foreground = White,
            FontSize = 20,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };

        DiskSummaryText = new TextBlock
        {
            Text = "-",
            Foreground = White,
            FontSize = 17,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            TextTrimming = TextTrimming.CharacterEllipsis
        };

        summary.Children.Add(MakeSummaryCard("녹화 중", RecordingCountText, 0));

        OfflineSummaryButton = MakeClickableSummaryCard("오프라인", OfflineCountText, 1);
        OfflineFlyoutList = new ListView
        {
            ItemsSource = OfflineItems,
            SelectionMode = ListViewSelectionMode.None,
            MinWidth = 300,
            MaxHeight = 420,
            ItemTemplate = BuildOfflineFlyoutTemplate()
        };

        var offlineFlyoutPanel = new StackPanel
        {
            Spacing = 8,
            MinWidth = 320
        };
        offlineFlyoutPanel.Children.Add(new TextBlock
        {
            Text = "현재 방송하지 않는 채널",
            FontSize = 16,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        offlineFlyoutPanel.Children.Add(OfflineFlyoutList);

        OfflineSummaryButton.Flyout = new Flyout
        {
            Content = offlineFlyoutPanel,
            Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom
        };

        summary.Children.Add(OfflineSummaryButton);

        StoppedSummaryButton = MakeClickableSummaryCard("직접 중지", StoppedCountText, 2);
        StoppedFlyoutList = new ListView
        {
            ItemsSource = StoppedItems,
            SelectionMode = ListViewSelectionMode.Single,
            MinWidth = 410,
            MaxHeight = 420,
            ItemTemplate = BuildOfflineFlyoutTemplate()
        };
        StoppedFlyoutList.SelectionChanged += (_, _) =>
            ResumeStoppedButton.IsEnabled = StoppedFlyoutList.SelectedItem != null;
        ResumeStoppedButton = ApplyButtonMetricsFix39(new Button
        {
            Content = "녹화 다시 시작",
            IsEnabled = false,
            HorizontalAlignment = HorizontalAlignment.Right
        }, 126);
        ResumeStoppedButton.Click += ResumeStopped_Click;
        var stoppedPanel = new StackPanel { Spacing = 8, MinWidth = 430 };
        stoppedPanel.Children.Add(new TextBlock
        {
            Text = "직접 중지한 채널",
            FontSize = 16,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        stoppedPanel.Children.Add(StoppedFlyoutList);
        stoppedPanel.Children.Add(ResumeStoppedButton);
        StoppedSummaryButton.Flyout = new Flyout
        {
            Content = stoppedPanel,
            Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom
        };
        summary.Children.Add(StoppedSummaryButton);

        AlertSummaryButton = MakeClickableSummaryCard("확인 필요", AlertCountText, 3);
        AlertFlyoutList = new ListView
        {
            ItemsSource = AlertItems,
            SelectionMode = ListViewSelectionMode.None,
            MinWidth = 410,
            MaxHeight = 420,
            ItemTemplate = BuildOfflineFlyoutTemplate()
        };
        var alertPanel = new StackPanel { Spacing = 8, MinWidth = 430 };
        alertPanel.Children.Add(new TextBlock
        {
            Text = "확인이 필요한 채널",
            FontSize = 16,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        alertPanel.Children.Add(AlertFlyoutList);
        AlertSummaryButton.Flyout = new Flyout
        {
            Content = alertPanel,
            Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom
        };
        summary.Children.Add(AlertSummaryButton);
        summary.Children.Add(MakeSummaryCard("Watcher", WatcherStateText, 4));
        summary.Children.Add(MakeSummaryCard("디스크 여유", DiskSummaryText, 5));

        stack.Children.Add(summary);
        var recordingHeader = new Grid();
        recordingHeader.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        recordingHeader.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var recordingTitle = new TextBlock
        {
            Text = "RECORDING",
            Foreground = Accent,
            FontSize = 16,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        };

        StopSelectedRecordingButton = ApplyButtonMetricsFix39(new Button
        {
            Content = "선택 채널 녹화 중지",
            IsEnabled = false
        }, 154);
        StopSelectedRecordingButton.Click += StopSelectedRecording_Click;
        Grid.SetColumn(StopSelectedRecordingButton, 1);

        recordingHeader.Children.Add(recordingTitle);
        recordingHeader.Children.Add(StopSelectedRecordingButton);
        stack.Children.Add(recordingHeader);

        DashboardEmptyTitleText = new TextBlock
        {
            FontSize = 18,
            Foreground = White,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            HorizontalAlignment = HorizontalAlignment.Center
        };
        DashboardEmptyDetailText = new TextBlock
        {
            Foreground = Muted,
            TextAlignment = TextAlignment.Center,
            TextWrapping = TextWrapping.Wrap,
            HorizontalAlignment = HorizontalAlignment.Center
        };
        DashboardEmptyStartButton = ApplyButtonMetricsFix39(new Button
        {
            Content = "Watcher 시작",
            HorizontalAlignment = HorizontalAlignment.Center
        }, 118);
        DashboardEmptyStartButton.Click += StartButton_Click;
        DashboardEmptyStatePanel = new StackPanel
        {
            Spacing = 8,
            Padding = new Thickness(20, 42, 20, 28)
        };
        DashboardEmptyStatePanel.Children.Add(DashboardEmptyTitleText);
        DashboardEmptyStatePanel.Children.Add(DashboardEmptyDetailText);
        DashboardEmptyStatePanel.Children.Add(DashboardEmptyStartButton);
        stack.Children.Add(DashboardEmptyStatePanel);

        RecordingList = new ListView
        {
            RequestedTheme = ElementTheme.Dark,
            SelectionMode = ListViewSelectionMode.Single
        };
        RecordingList.SelectionChanged += (_, _) =>
            UpdateSelectedRecordingActionButton();
        RecordingList.ItemTemplate = BuildRecordingTemplate();
        stack.Children.Add(RecordingList);
        RecordingList.ItemsSource = RecordingItems;

        UpdateDashboardEmptyState();

        scroll.Content = stack;
        return scroll;
    }

    Border MakeSummaryCard(string title, TextBlock value, int column)
    {
        var panel = new StackPanel { Spacing = 4 };
        panel.Children.Add(new TextBlock
        {
            Text = title,
            Foreground = Muted,
            FontSize = 13
        });
        panel.Children.Add(value);

        var card = new Border
        {
            Background = MakeBrush("#20252C"),
            CornerRadius = new CornerRadius(9),
            Padding = new Thickness(16, 13, 16, 13),
            Child = panel
        };

        Grid.SetColumn(card, column);
        return card;
    }

    Button MakeClickableSummaryCard(string title, TextBlock value, int column)
    {
        var panel = new StackPanel
        {
            Spacing = 4,
            HorizontalAlignment = HorizontalAlignment.Stretch
        };
        panel.Children.Add(new TextBlock
        {
            Text = title,
            Foreground = Muted,
            FontSize = 13
        });
        panel.Children.Add(value);

        var button = new Button
        {
            Background = MakeBrush("#20252C"),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(16, 13, 16, 13),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            Content = panel,
            CornerRadius = new CornerRadius(9)
        };

        Grid.SetColumn(button, column);
        return button;
    }

    DataTemplate BuildRecordingTemplate()
    {
        var xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="#20252C" CornerRadius="8" Padding="11" Margin="0,0,0,7">
    <Grid ColumnSpacing="14">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="*"/>
        <ColumnDefinition Width="310"/>
      </Grid.ColumnDefinitions>

      <!-- Static recording metadata: changes only on start/stop/resume -->
      <Grid Grid.Column="0" ColumnSpacing="10">
        <Grid.RowDefinitions>
          <RowDefinition Height="Auto"/>
          <RowDefinition Height="Auto"/>
          <RowDefinition Height="Auto"/>
        </Grid.RowDefinitions>
        <Grid.ColumnDefinitions>
          <ColumnDefinition Width="82"/>
          <ColumnDefinition Width="155"/>
          <ColumnDefinition Width="92"/>
          <ColumnDefinition Width="*"/>
        </Grid.ColumnDefinitions>

        <TextBlock Grid.Row="0" Grid.Column="0" Text="{Binding Time}" Foreground="#D7DEE8"/>
        <TextBlock Grid.Row="0" Grid.Column="1" Text="{Binding Name}" Foreground="White" FontWeight="SemiBold"/>
        <TextBlock Grid.Row="0" Grid.Column="2" Text="{Binding Status}" Foreground="{Binding StatusBrush}" FontWeight="SemiBold"/>
        <TextBlock Grid.Row="0" Grid.Column="3" Text="{Binding Detail}" Foreground="#D7DEE8"
                   TextTrimming="CharacterEllipsis"/>

        <TextBlock Grid.Row="1" Grid.Column="1" Grid.ColumnSpan="3"
                   Text="{Binding Title}" Foreground="#9AA5B4" FontSize="11"
                   TextTrimming="CharacterEllipsis" Margin="0,4,0,0"/>

        __PATH_ROW__
      </Grid>

      <!-- Only this panel is expected to change while recording -->
      <Border Grid.Column="1" Width="310" Background="#191E24" CornerRadius="6" Padding="12,8">
        <Grid ColumnSpacing="14">
          <Grid.ColumnDefinitions>
            <ColumnDefinition Width="92"/>
            <ColumnDefinition Width="92"/>
            <ColumnDefinition Width="92"/>
          </Grid.ColumnDefinitions>

          <StackPanel Grid.Column="0" Spacing="2">
            <TextBlock Text="파일 크기" Foreground="#768397" FontSize="10"/>
            <TextBlock Text="{Binding SizeText}" Foreground="White" FontWeight="SemiBold"/>
          </StackPanel>

          <StackPanel Grid.Column="1" Spacing="2">
            <TextBlock Text="녹화 시간" Foreground="#768397" FontSize="10"/>
            <TextBlock Text="{Binding ElapsedText}" Foreground="White" FontWeight="SemiBold"/>
          </StackPanel>

          <StackPanel Grid.Column="2" Spacing="2">
            <TextBlock Text="다운로드" Foreground="#768397" FontSize="10"/>
            <TextBlock Text="{Binding RateText}" Foreground="White" FontWeight="SemiBold"/>
          </StackPanel>
        </Grid>
      </Border>
    </Grid>
  </Border>
</DataTemplate>
""";

        var pathRow = uiShowPath
            ? """
<Grid Grid.Row="2" Grid.Column="1" Grid.ColumnSpan="3" Margin="0,3,0,0" ColumnSpacing="6">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="Auto"/>
    <ColumnDefinition Width="*"/>
  </Grid.ColumnDefinitions>
  <TextBlock Grid.Column="0" Text="파일 :" Foreground="#768397" FontSize="11"/>
  <TextBlock Grid.Column="1"
             Text="{Binding FileName}"
             ToolTipService.ToolTip="{Binding FilePath}"
             Foreground="#8F9BAC"
             FontSize="11"
             TextTrimming="CharacterEllipsis"/>
</Grid>
"""
            : "";

        xaml = xaml.Replace("__PATH_ROW__", pathRow);

        return (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }

    DataTemplate BuildOfflineFlyoutTemplate()
    {
        var statusColor = uiConsoleColor ? "#A7B0BE" : "#D7DEE8";
        var xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Grid Padding="5,4" ColumnSpacing="12">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="180"/>
      <ColumnDefinition Width="*"/>
    </Grid.ColumnDefinitions>
    <TextBlock Grid.Column="0" Text="{Binding Name}" FontWeight="SemiBold"/>
    <TextBlock Grid.Column="1" Text="{Binding Status}" Foreground="__STATUS_COLOR__"/>
  </Grid>
</DataTemplate>
""";
        xaml = xaml.Replace("__STATUS_COLOR__", statusColor);
        return (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }



    void ApplyConsoleDisplayOptions(bool autoFormat, bool color, bool showPath)
    {
        uiAutoFormat = autoFormat;
        uiConsoleColor = color;
        uiShowPath = showPath;

        if (RecordingList != null)
            RecordingList.ItemTemplate = BuildRecordingTemplate();

        if (OfflineFlyoutList != null)
            OfflineFlyoutList.ItemTemplate = BuildOfflineFlyoutTemplate();

        if (StoppedFlyoutList != null)
            StoppedFlyoutList.ItemTemplate = BuildOfflineFlyoutTemplate();

        if (AlertFlyoutList != null)
            AlertFlyoutList.ItemTemplate = BuildOfflineFlyoutTemplate();

        if (uiAutoFormat)
        {
            SortDashboardItems();
        }
    }

    void SortDashboardItems()
    {
        static void SortCollection(
            ObservableCollection<ChannelStatus> items)
        {
            if (items.Count < 2)
                return;

            var desired = items
                .OrderBy(x => x.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();

            for (var targetIndex = 0;
                 targetIndex < desired.Count;
                 targetIndex++)
            {
                var wanted = desired[targetIndex];

                if (ReferenceEquals(items[targetIndex], wanted))
                    continue;

                var currentIndex = items.IndexOf(wanted);
                if (currentIndex >= 0 && currentIndex != targetIndex)
                    items.Move(currentIndex, targetIndex);
            }
        }

        SortCollection(RecordingItems);
        SortCollection(OfflineItems);
        SortCollection(StoppedItems);
        SortCollection(AlertItems);
    }

    FrameworkElement BuildChannelsView()
    {
        var root = new Grid
        {
            Padding = new Thickness(20),
            Visibility = Visibility.Collapsed,
            RequestedTheme = ElementTheme.Dark
        };

        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var header = new Grid { Margin = new Thickness(0, 0, 0, 12) };
        header.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        header.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var title = new StackPanel();
        title.Children.Add(new TextBlock
        {
            Text = "채널 관리",
            Foreground = White,
            FontSize = 22,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        title.Children.Add(new TextBlock
        {
            Text = "기본은 표에서 관리하고, 아래 원본 편집기는 고급 편집용입니다.",
            Foreground = Muted,
            Margin = new Thickness(0, 4, 0, 0)
        });

        ChannelFilePathText = new TextBlock
        {
            Text = "채널 파일 : 확인 중",
            Foreground = MakeBrush("#7F8A99"),
            FontSize = 11,
            Margin = new Thickness(0, 4, 0, 0),
            TextTrimming = TextTrimming.CharacterEllipsis
        };
        title.Children.Add(ChannelFilePathText);

        ChannelDirtyStateText = new TextBlock
        {
            Text = "✓ 저장된 상태",
            Foreground = MakeBrush("#7F8A99"),
            FontSize = 12,
            Margin = new Thickness(0, 5, 0, 0)
        };
        title.Children.Add(ChannelDirtyStateText);

        AddChannelButton = ApplyButtonMetricsFix39(new Button { Content = "채널 추가" });
        ImportChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "채널 가져오기" }, 118);
        EditChannelButton = ApplyButtonMetricsFix39(new Button { Content = "수정" });
        SelectedChannelActionsButton = ApplyButtonMetricsFix39(new Button { Content = "선택 작업 ▾", IsEnabled = false }, 108);
        ApplyRawChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "목록에 반영" }, 110);
        ReloadChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "저장본 다시 읽기" }, 132);
        SaveChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "변경 저장", IsEnabled = false }, 108);

        var selectedActionsFlyout = new MenuFlyout();
        EnableSelectedChannelsMenuItem = new MenuFlyoutItem { Text = "선택 채널 활성화", IsEnabled = false };
        DisableSelectedChannelsMenuItem = new MenuFlyoutItem { Text = "선택 채널 비활성화", IsEnabled = false };
        DeleteChannelMenuItem = new MenuFlyoutItem { Text = "선택 채널 삭제", IsEnabled = false };
        selectedActionsFlyout.Items.Add(EnableSelectedChannelsMenuItem);
        selectedActionsFlyout.Items.Add(DisableSelectedChannelsMenuItem);
        selectedActionsFlyout.Items.Add(new MenuFlyoutSeparator());
        selectedActionsFlyout.Items.Add(DeleteChannelMenuItem);
        SelectedChannelActionsButton.Flyout = selectedActionsFlyout;

        AddChannelButton.Click += AddChannel_Click;
        ImportChannelsButton.Click += ImportChannelsFix38_Click;
        EditChannelButton.Click += EditChannel_Click;
        DeleteChannelMenuItem.Click += DeleteChannel_Click;
        EnableSelectedChannelsMenuItem.Click += EnableSelectedChannels_Click;
        DisableSelectedChannelsMenuItem.Click += DisableSelectedChannels_Click;
        ApplyRawChannelsButton.Click += ApplyRawChannels_Click;
        ReloadChannelsButton.Click += ReloadChannels_Click;
        SaveChannelsButton.Click += SaveChannels_Click;

        ToolTipService.SetToolTip(
            ApplyRawChannelsButton,
            "아래 원본 편집기 내용을 검사하여 위 채널 목록에 반영합니다. 파일에는 저장하지 않습니다.");
        ToolTipService.SetToolTip(
            ReloadChannelsButton,
            "마지막으로 저장된 채널 파일을 다시 읽습니다. 저장하지 않은 변경은 확인 후 버립니다.");
        ToolTipService.SetToolTip(
            SaveChannelsButton,
            "현재 채널 목록을 파일에 저장하고 기존 파일을 .bak으로 백업합니다.");

        SelectAllChannelsCheckBox = new CheckBox
        {
            Content = "전체 선택",
            VerticalAlignment = VerticalAlignment.Center
        };
        SelectAllChannelsCheckBox.Checked += SelectAllChannelsCheckBox_Checked;
        SelectAllChannelsCheckBox.Unchecked += SelectAllChannelsCheckBox_Unchecked;

        SelectedChannelCountText = new TextBlock
        {
            Text = "0개 선택",
            Foreground = Muted,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 6, 0)
        };

        var commandBar = new Grid
        {
            ColumnSpacing = 12,
            Margin = new Thickness(0, 12, 0, 0)
        };
        commandBar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        commandBar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var primaryActions = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8
        };
        primaryActions.Children.Add(AddChannelButton);
        primaryActions.Children.Add(ImportChannelsButton);
        primaryActions.Children.Add(EditChannelButton);
        primaryActions.Children.Add(SelectedChannelActionsButton);
        commandBar.Children.Add(primaryActions);
        Grid.SetColumn(SaveChannelsButton, 1);
        commandBar.Children.Add(SaveChannelsButton);
        Grid.SetRow(commandBar, 1);

        header.Children.Add(title);
        header.Children.Add(commandBar);

        ChannelSearchBox = new TextBox
        {
            PlaceholderText = "채널명 또는 계정 ID 검색",
            Width = 300
        };
        ChannelFilterBox = new ComboBox
        {
            Width = 170,
            ItemsSource = new[] { "전체 채널", "활성 채널", "비활성 채널", "개별 저장 경로" },
            SelectedIndex = 0
        };
        ChannelSearchBox.TextChanged += (_, _) => RefreshChannelFilter();
        ChannelFilterBox.SelectionChanged += (_, _) => RefreshChannelFilter();
        ChannelItems.CollectionChanged += (_, _) =>
        {
            if (suppressChannelCollectionRefresh)
                return;
            RefreshChannelFilter();
            UpdateDashboardEmptyState();
        };
        var filterBar = new Grid
        {
            ColumnSpacing = 8,
            Margin = new Thickness(0, 0, 0, 8)
        };
        filterBar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        filterBar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        filterBar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        filterBar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        filterBar.Children.Add(SelectAllChannelsCheckBox);
        Grid.SetColumn(SelectedChannelCountText, 1);
        filterBar.Children.Add(SelectedChannelCountText);
        ChannelSearchBox.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(ChannelSearchBox, 2);
        filterBar.Children.Add(ChannelSearchBox);
        Grid.SetColumn(ChannelFilterBox, 3);
        filterBar.Children.Add(ChannelFilterBox);
        Grid.SetRow(filterBar, 1);

        ChannelList = new ListView
        {
            ItemsSource = VisibleChannelItems,
            SelectionMode = ListViewSelectionMode.Multiple,
            Margin = new Thickness(0, 0, 0, 10)
        };
        ChannelList.ItemTemplate = BuildChannelTemplate();
        ChannelList.SelectionChanged += ChannelList_SelectionChanged;

        ChannelsText = new TextBox
        {
            AcceptsReturn = true,
            IsReadOnly = false,
            TextWrapping = TextWrapping.NoWrap,
            FontFamily = new FontFamily("Consolas"),
            Background = MakeBrush("#171B20"),
            Foreground = White,
            BorderBrush = MakeBrush("#3A414A"),
            Padding = new Thickness(10)
        };
        ChannelsText.TextChanged += ChannelsText_TextChanged;

        var rawToggle = ApplyButtonMetricsFix39(new Button
        {
            Content = "원본 편집기 펼치기",
            Margin = new Thickness(0, 2, 0, 0)
        }, 142);
        var rawActions = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            Visibility = Visibility.Collapsed
        };
        rawActions.Children.Add(ApplyRawChannelsButton);
        rawActions.Children.Add(ReloadChannelsButton);
        ChannelsText.Visibility = Visibility.Collapsed;
        ChannelsText.Height = 220;
        rawToggle.Click += (_, _) =>
        {
            var open = ChannelsText.Visibility != Visibility.Visible;
            ChannelsText.Visibility = open ? Visibility.Visible : Visibility.Collapsed;
            rawActions.Visibility = open ? Visibility.Visible : Visibility.Collapsed;
            rawToggle.Content = open ? "원본 편집기 접기" : "원본 편집기 펼치기";
        };
        var rawPanel = new StackPanel { Spacing = 6 };
        var rawHeader = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        rawHeader.Children.Add(rawToggle);
        rawHeader.Children.Add(rawActions);
        rawPanel.Children.Add(rawHeader);
        rawPanel.Children.Add(ChannelsText);

        Grid.SetRow(ChannelList, 2);
        Grid.SetRow(rawPanel, 3);
        root.Children.Add(header);
        root.Children.Add(filterBar);
        root.Children.Add(ChannelList);
        root.Children.Add(rawPanel);
        RefreshChannelFilter();
        return root;
    }

    DataTemplate BuildChannelTemplate()
    {
        const string xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="#20252C" CornerRadius="7" Padding="10" Margin="0,0,0,5">
    <Grid ColumnSpacing="12">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="80"/>
        <ColumnDefinition Width="160"/>
        <ColumnDefinition Width="220"/>
        <ColumnDefinition Width="*"/>
      </Grid.ColumnDefinitions>
      <TextBlock Grid.Column="0" Text="{Binding EnabledText}" Foreground="#AAB4C3"/>
      <TextBlock Grid.Column="1" Text="{Binding Name}" Foreground="White" FontWeight="SemiBold"/>
      <TextBlock Grid.Column="2" Text="{Binding Account}" Foreground="#D7DEE8"/>
      <TextBlock Grid.Column="3" Text="{Binding OutputDisplay}" Foreground="#9AA5B4" TextTrimming="CharacterEllipsis"/>
    </Grid>
  </Border>
</DataTemplate>
""";
        return (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }

    FrameworkElement BuildLogsView()
    {
        var root = new Grid
        {
            Padding = new Thickness(20),
            Visibility = Visibility.Collapsed,
            RequestedTheme = ElementTheme.Dark
        };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var buttons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            Margin = new Thickness(0, 0, 0, 10)
        };

        var clear = ApplyButtonMetricsFix39(new Button { Content = "로그 지우기" });
        var open = ApplyButtonMetricsFix39(new Button { Content = "프로그램 폴더 열기" }, 142);
        clear.Click += ClearLog_Click;
        open.Click += OpenBackend_Click;
        buttons.Children.Add(clear);
        buttons.Children.Add(open);

        LogBox = new TextBox
        {
            AcceptsReturn = true,
            IsReadOnly = true,
            TextWrapping = TextWrapping.NoWrap,
            FontFamily = new FontFamily("Consolas"),
            Background = MakeBrush("#171B20"),
            Foreground = MakeBrush("#D7DEE8"),
            BorderBrush = MakeBrush("#3A414A"),
            Padding = new Thickness(10)
        };

        Grid.SetRow(LogBox, 1);
        root.Children.Add(buttons);
        root.Children.Add(LogBox);
        return root;
    }

    StackPanel SettingsCard(string title, string description)
    {
        var body = new StackPanel
        {
            Spacing = 8,
            Padding = new Thickness(18)
        };

        body.Children.Add(new TextBlock
        {
            Text = title,
            Foreground = White,
            FontSize = 18,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });

        body.Children.Add(new TextBlock
        {
            Text = description,
            Foreground = Muted,
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 0, 0, 5)
        });

        return body;
    }

    TextBlock FieldLabel(string text) => new()
    {
        Text = text,
        Foreground = MakeBrush("#D0D6DF"),
        Margin = new Thickness(0, 5, 0, 0)
    };

    TextBox WideTextBox() => new()
    {
        IsReadOnly = false,
        HorizontalAlignment = HorizontalAlignment.Stretch
    };

    NumberBox NumberSetting(double min, double max, double width) => new()
    {
        Width = width,
        Minimum = min,
        Maximum = max,
        SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact,
        HorizontalAlignment = HorizontalAlignment.Left
    };

    static SolidColorBrush MakeBrush(string hex)
    {
        hex = hex.TrimStart('#');
        return new SolidColorBrush(Windows.UI.Color.FromArgb(
            255,
            Convert.ToByte(hex.Substring(0, 2), 16),
            Convert.ToByte(hex.Substring(2, 2), 16),
            Convert.ToByte(hex.Substring(4, 2), 16)));
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
            if (trayIcon == null)
                return;

            var body = $"{channel} 녹화가 종료되었습니다.\n{duration} · {size}";
            if (!string.IsNullOrWhiteSpace(reason))
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

        try
        {
            uiFlushTimer?.Stop();
            uiFlushTimer = null;
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
            if (!allowRealClose)
                backend.StopNow();
        }
        catch { }

        try { backend.Dispose(); } catch { }
    }

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
        StopButton.IsEnabled = false;
        WatcherStateText.Text = "종료 중";
        AppendLog("[GUI] Watcher 종료 요청");

        try
        {
            await backend.StopAsync();

            if (backend.IsRunning)
            {
                WatcherStateText.Text = "종료 확인 필요";
                StopButton.IsEnabled = true;
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
        watcherStopInProgress = false;
        dashboardWatcherRunning = false;
        ResetDashboardState();
        SetWatcherStartControlsFix38(true);
        StopButton.IsEnabled = false;
        WatcherStateText.Text = code == 0 ? "중지됨" : $"오류 종료 ({code})";
        OfflineCountText.Text = "-";
        StoppedCountText.Text = "-";
        AlertCountText.Text = "-";
        UpdateDashboardEmptyState();
        AppendLog($"[GUI] Watcher 종료 Exit={code}");
    }

    void ResetDashboardState()
    {
        while (backendLineQueue.TryDequeue(out _)) { }
        Interlocked.Exchange(ref queuedBackendLines, 0);
        pendingProgressByChannel.Clear();
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

        backendLineQueue.Enqueue(line);
        Interlocked.Increment(ref queuedBackendLines);
    }

    void FlushBackendUiQueue()
    {
        ReconcileWatcherRunningUiFix39();

        if (backendLineQueue.IsEmpty)
            return;

        // Bound work per UI tick so a noisy backend can never monopolize
        // the WinUI dispatcher indefinitely.
        const int maxLinesPerFlush = 400;
        int count = 0;

        while (count < maxLinesPerFlush && backendLineQueue.TryDequeue(out var line))
        {
            ProcessBackendLine(line, deferProgressUi: true);
            count++;
        }

        flushedBackendLines += count;
        Interlocked.Add(ref queuedBackendLines, -count);

        // Progress can be very noisy. During one 250 ms window keep only
        // the newest row per channel and update the UI once.
        if (pendingProgressByChannel.Count > 0)
        {
            var latest = pendingProgressByChannel.Values.ToArray();
            pendingProgressByChannel.Clear();

            foreach (var progressLine in latest)
                ProcessBackendLine(progressLine, deferProgressUi: false);
        }

        FlushLogText();
        lastUiFlush = DateTime.Now;
    }

    void ProcessBackendLine(string line, bool deferProgressUi)
    {
        ReconcileWatcherRunningUiFix39();
        AppendLog(line);

        var text = line.Trim();
        if (text.Length == 0) return;

        // Progress rows are the noisiest output. During queue draining,
        // cache the newest line per channel and defer actual WinUI updates.
        if (deferProgressUi)
        {
            // fix33 backend guarantees:
            // [time] CHANNEL : RECORDING | [download] ...
            // The channel key and the metrics now travel in the same line.
            var queuedCompact = CompactRecording.Match(text);
            if (queuedCompact.Success)
            {
                var channel = queuedCompact.Groups["name"].Value.Trim();
                var account = queuedCompact.Groups["account"].Value.Trim();
                var progressKey = string.IsNullOrWhiteSpace(account)
                    ? "name:" + channel
                    : "account:" + account;

                if (!string.IsNullOrWhiteSpace(channel))
                    pendingProgressByChannel[progressKey] = line;

                return;
            }

            // Ignore untagged raw [download] lines for dashboard metrics.
            // Guessing their channel from a previous console line is unsafe
            // when several recorder processes are active concurrently.
            if (DownloadProgressWithPath.IsMatch(text) ||
                DownloadProgressNoPath.IsMatch(text))
            {
                return;
            }
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
                _ => "확인 필요"
            };
            var detail = health.Groups["detail"].Success
                ? health.Groups["detail"].Value.Trim()
                : health.Groups["detail2"].Value.Trim();
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

            item.Status = "● REC";
            item.Time = DateTime.Now.ToString("HH:mm:ss");
            item.Detail = "녹화 중";
            item.IsSuspended = false;
            item.SizeText = "-";
            item.ElapsedText = "-";
            item.RateText = "-";
            item.FilePath = outputFile;

            if (!string.IsNullOrWhiteSpace(pendingRecordTitle))
                item.Title = pendingRecordTitle!;

            UpdateDrive(item, outputFile);
            if (!RecordingItems.Contains(item))
                MoveToRecording(item);

            pendingRecordChannel = null;
            pendingRecordAccount = null;
            pendingRecordTitle = null;
            return;
        }

        var start = RecordStart.Match(text);
        if (start.Success)
        {
            var name = start.Groups["name"].Value.Trim();
            var account = start.Groups["account"].Value.Trim();
            ParseRecordStartRest(start.Groups["rest"].Value, out var title, out var file);

            var item = GetOrCreate(account, name);
            item.Status = "● REC";
            item.Title = title;
            item.Time = DateTime.Now.ToString("HH:mm:ss");
            item.Detail = "녹화 중";
            item.IsSuspended = false;
            item.SizeText = "-";
            item.ElapsedText = "-";
            item.RateText = "-";
            item.FilePath = file;
            UpdateDrive(item, file);
            if (!RecordingItems.Contains(item))
                MoveToRecording(item);
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

        // Compact backend progress intentionally does not need to update
        // FilePath/drive. Those are fixed by RECORD START.
        if (!RecordingItems.Contains(item))
            MoveToRecording(item);
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
            !StoppedItems.Contains(item) &&
            !alertMap.ContainsKey(DashboardKey(item.Account, item.Name));
        if (alreadyOffline)
            return;

        RecordingItems.Remove(item);
        StoppedItems.Remove(item);
        ClearDashboardAlert(item.Account, item.Name, refresh: false);
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
        if (uiAutoFormat)
            SortDashboardItems();
        UpdateCounts();
    }

    bool ClearDashboardAlert(string? account, string name, bool refresh = true)
    {
        var key = DashboardKey(account, name);
        if (!alertMap.Remove(key, out var alert))
            return false;

        AlertItems.Remove(alert);
        if (refresh)
            UpdateCounts();
        return true;
    }

    void UpdateCounts()
    {
        SetTextIfChangedFix38(
            RecordingCountText,
            RecordingItems.Count(x => x.Status == "● REC").ToString());
        SetTextIfChangedFix38(OfflineCountText, OfflineItems.Count.ToString());
        SetTextIfChangedFix38(StoppedCountText, StoppedItems.Count.ToString());
        SetTextIfChangedFix38(AlertCountText, AlertItems.Count.ToString());
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

    static void ParseRecordStartRest(string rest, out string title, out string file)
    {
        title = "";
        file = "";

        var fileIndex = rest.LastIndexOf(" file=", StringComparison.OrdinalIgnoreCase);
        if (fileIndex >= 0)
        {
            file = rest[(fileIndex + 6)..].Trim();
            rest = rest[..fileIndex];
        }

        var hlsIndex = rest.IndexOf(" hls=", StringComparison.OrdinalIgnoreCase);
        var titlePart = hlsIndex >= 0 ? rest[..hlsIndex].Trim() : rest.Trim();

        if (titlePart.StartsWith("title=", StringComparison.OrdinalIgnoreCase))
            title = titlePart[6..].Trim();
    }

    void UpdateDrive(ChannelStatus item, string file)
    {
        if (string.IsNullOrWhiteSpace(file)) return;
        try
        {
            var root=Path.GetPathRoot(Path.GetFullPath(file));
            if (string.IsNullOrWhiteSpace(root)) return;
            var drive=new DriveInfo(root);
            if (!drive.IsReady) return;
            var free=drive.AvailableFreeSpace/1024d/1024d/1024d;
            item.Drive=$"{drive.Name.TrimEnd('\\')} {free:N1} GB";
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

        var perRoot = new Dictionary<string, string>(
            StringComparer.OrdinalIgnoreCase);

        foreach (var item in activeItems)
        {
            if (string.IsNullOrWhiteSpace(item.Drive))
                continue;

            var n = item.Drive.IndexOf(' ');
            var root = n > 0 ? item.Drive[..n] : item.Drive;
            perRoot[root] = item.Drive;
        }

        // PAUSED/restart-waiting cards must never contribute stale drive info.
        foreach (var item in RecordingItems.Where(x => x.Status != "● REC"))
            item.DisplayDrive = "";

        if (perRoot.Count == 0)
        {
            DiskSummaryText.Text = "-";

            foreach (var item in activeItems)
                item.DisplayDrive = "";

            return;
        }

        DiskSummaryText.Text =
            string.Join(" · ", perRoot.Values.OrderBy(x => x));

        var showPerCard = perRoot.Count > 1;

        foreach (var item in activeItems)
            item.DisplayDrive = showPerCard ? item.Drive : "";
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
        LogsView.Visibility = tag == "logs" ? Visibility.Visible : Visibility.Collapsed;

        if (tag is "channels" or "settings")
            LoadStaticFiles();
    }

    async void SaveChannels_Click(object sender, RoutedEventArgs e)
    {
        await SaveChannelsAsync(showSuccess: true);
    }

    async Task<bool> SaveChannelsAsync(bool showSuccess)
    {
        try
        {
            var selectedAccounts = GetSelectedChannels()
                .Select(x => x.Account)
                .Where(x => !string.IsNullOrWhiteSpace(x))
                .ToList();
            var rawText = ChannelsText.Text ?? "";

            var rawDataLines = SplitChannelLines(rawText)
                .Select(x => x.Trim())
                .Where(x => x.Length > 0 && !x.StartsWith("#"))
                .ToList();

            // Source-of-truth rule:
            // 1) If the raw editor contains at least one actual channel line,
            //    RAW always wins. This makes pasted legacy channel files safe.
            // 2) If raw contains only comments/header but the structured table
            //    has rows, TABLE wins.
            // 3) If both are empty, reject the save.
            if (rawChannelTextDirty && rawDataLines.Count > 0)
            {
                var parseError =
                    TryLoadChannelItemsFromRawText(rawText, out var parsedCount);

                if (!string.IsNullOrWhiteSpace(parseError))
                {
                    await ShowDialogAsync(
                        "채널 저장 확인",
                        parseError +
                        $"\n\n원본 문자 수: {rawText.Length}" +
                        $"\n채널 후보 행 수: {rawDataLines.Count}");
                    return false;
                }

                if (parsedCount == 0)
                {
                    await ShowDialogAsync(
                        "채널 저장 확인",
                        "원본 편집기에서 채널 행을 찾았지만 파싱 결과가 0개입니다.\n" +
                        $"원본 문자 수: {rawText.Length}\n" +
                        $"채널 후보 행 수: {rawDataLines.Count}");
                    return false;
                }

                // Normalize the raw editor after a successful parse.
                SyncRawChannelTextFromItems(markDirty: true);
            }
            else if (ChannelItems.Count > 0)
            {
                SyncRawChannelTextFromItems(markDirty: true);
            }
            else
            {
                await ShowDialogAsync(
                    "채널 저장 확인",
                    "저장할 채널이 없습니다.\n\n" +
                    "형식 예시:\n" +
                    "Y|둘기얏|1004ysus|\n" +
                    "Y|문월|moonwol0614|");
                return false;
            }

            BackupFile(channelPath);
            AtomicWriteAllText(channelPath,ChannelsText.Text ?? "",new UTF8Encoding(false));

            savedChannelTextSnapshot = NormalizeChannelText(ChannelsText.Text);
            lastProgrammaticRawChannelText = savedChannelTextSnapshot;
            rawChannelTextDirty = false;
            channelTableChangesDirty = false;
            SetChannelChangesDirty(false);
            RestoreChannelSelection(selectedAccounts);

            if (showSuccess)
            {
                await ShowDialogAsync(
                    "채널 저장",
                    $"SOOP_LIVE_CHANNELS.txt 저장 완료\n" +
                    $"채널 {ChannelItems.Count}개\n" +
                    $"경로: {channelPath}\n" +
                    "이전 파일은 .bak으로 보관했습니다.");
            }

            return true;
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("채널 저장 실패", ex.Message);
            return false;
        }
    }

    async void ReloadChannels_Click(object sender, RoutedEventArgs e)
    {
        if (channelChangesDirty)
        {
            var dialog = new ContentDialog
            {
                Title = "저장본 다시 읽기",
                Content = "저장하지 않은 변경 사항을 버리고 마지막 저장 상태로 돌아갈까요?",
                PrimaryButtonText = "다시 읽기",
                CloseButtonText = "취소",
                DefaultButton = ContentDialogButton.Close,
                XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
            };

            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
                return;
        }

        ReloadChannelsFromDisk();
    }

    async void ApplyRawChannels_Click(object sender, RoutedEventArgs e)
    {
        var rawText = ChannelsText.Text ?? "";
        var error = TryLoadChannelItemsFromRawText(rawText, out var count);

        if (!string.IsNullOrWhiteSpace(error))
        {
            await ShowDialogAsync("원본 적용 실패", error);
            return;
        }

        if (count == 0)
        {
            var detectedLines = SplitChannelLines(rawText).Count();
            await ShowDialogAsync(
                "원본 적용",
                $"유효한 채널 행을 찾지 못했습니다.\n" +
                $"원본 문자 수: {rawText.Length}\n" +
                $"감지된 줄 수: {detectedLines}");
            return;
        }

        SyncRawChannelTextFromItems(markDirty: true);
        rawChannelTextDirty = false;

        await ShowDialogAsync(
            "원본 적용",
            $"원본에서 채널 {count}개를 표에 반영했습니다.");
    }

    void ChannelsText_TextChanged(object sender, TextChangedEventArgs e)
    {
        if (suppressRawChannelTextChanged)
            return;

        var normalized = NormalizeChannelText(ChannelsText.Text);
        if (string.Equals(normalized, lastProgrammaticRawChannelText, StringComparison.Ordinal))
            return;

        rawChannelTextDirty = !string.Equals(
            normalized,
            savedChannelTextSnapshot,
            StringComparison.Ordinal);
        SetChannelChangesDirty(rawChannelTextDirty || channelTableChangesDirty);
    }

    async Task<bool> ConfirmLeaveChannelsAsync()
    {
        var dialog = new ContentDialog
        {
            Title = "저장하지 않은 채널 변경",
            Content = "채널 변경 사항이 아직 파일에 저장되지 않았습니다.",
            PrimaryButtonText = "저장 후 이동",
            SecondaryButtonText = "저장하지 않고 이동",
            CloseButtonText = "취소",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };

        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary)
            return await SaveChannelsAsync(showSuccess: false);

        if (result == ContentDialogResult.Secondary)
        {
            ReloadChannelsFromDisk();
            return true;
        }

        return false;
    }

    void ReloadChannelsFromDisk()
    {
        if (File.Exists(channelPath))
        {
            SetRawChannelText(File.ReadAllText(channelPath, Encoding.UTF8), markDirty: false);
            LoadChannelItemsFromText(ChannelsText.Text);
        }
        else
        {
            SetRawChannelText(
                "# ENABLED|NAME|ACCOUNT|OUTDIR" + Environment.NewLine,
                markDirty: false);
            ChannelItems.Clear();
        }

        rawChannelTextDirty = false;
        channelTableChangesDirty = false;
        savedChannelTextSnapshot = NormalizeChannelText(ChannelsText.Text);
        lastProgrammaticRawChannelText = savedChannelTextSnapshot;
        ChannelFilePathText.Text = "채널 파일 : " + channelPath;
        SetChannelChangesDirty(false);
        ClearChannelSelection();
    }

    void SetChannelChangesDirty(bool dirty)
    {
        channelChangesDirty = dirty;

        if (ChannelDirtyStateText == null)
            return;

        ChannelDirtyStateText.Text = dirty
            ? "● 저장되지 않은 변경 사항"
            : "✓ 저장된 상태";
        ChannelDirtyStateText.Foreground = dirty
            ? MakeBrush("#F4C95D")
            : MakeBrush("#7F8A99");
        if (SaveChannelsButton != null)
            SaveChannelsButton.IsEnabled = dirty;
    }

    async Task<bool> EnsureRawChannelEditsAppliedAsync()
    {
        if (!rawChannelTextDirty)
            return true;

        var selectedAccounts = GetSelectedChannels()
            .Select(x => x.Account)
            .Where(x => !string.IsNullOrWhiteSpace(x))
            .ToList();

        var error = TryLoadChannelItemsFromRawText(ChannelsText.Text, out _);
        if (!string.IsNullOrWhiteSpace(error))
        {
            await ShowDialogAsync("원본 편집 확인", error);
            return false;
        }

        rawChannelTextDirty = false;
        RestoreChannelSelection(selectedAccounts);
        return true;
    }

    async void AddChannel_Click(object sender, RoutedEventArgs e)
    {
        if (!await EnsureRawChannelEditsAppliedAsync())
            return;

        var item = await ShowChannelEditorAsync(null);
        if (item == null) return;

        ChannelItems.Add(item);
        SyncRawChannelTextFromItems(markDirty: true);
    }

    async void EditChannel_Click(object sender, RoutedEventArgs e)
    {
        var selectedAccount = GetSelectedChannels().Count == 1
            ? GetSelectedChannels()[0].Account
            : null;

        if (!await EnsureRawChannelEditsAppliedAsync())
            return;

        var selectedItems = GetSelectedChannels();
        if (selectedItems.Count != 1 && !string.IsNullOrWhiteSpace(selectedAccount))
        {
            RestoreChannelSelection(new[] { selectedAccount });
            selectedItems = GetSelectedChannels();
        }
        if (selectedItems.Count != 1)
        {
            await ShowDialogAsync("채널 수정", "수정할 채널 하나만 선택하세요.");
            return;
        }

        var selected = selectedItems[0];

        var edited = await ShowChannelEditorAsync(selected);
        if (edited == null) return;

        selected.Enabled = edited.Enabled;
        selected.Name = edited.Name;
        selected.Account = edited.Account;
        selected.OutDir = edited.OutDir;
        SyncRawChannelTextFromItems(markDirty: true);
        RestoreChannelSelection(new[] { edited.Account });
    }

    async void DeleteChannel_Click(object sender, RoutedEventArgs e)
    {
        if (!await EnsureRawChannelEditsAppliedAsync())
            return;

        var selectedItems = GetSelectedChannels();
        if (selectedItems.Count == 0)
        {
            await ShowDialogAsync("채널 삭제", "삭제할 채널을 하나 이상 선택하세요.");
            return;
        }

        var preview = string.Join(
            Environment.NewLine,
            selectedItems.Take(8).Select(x => $"• {x.Name} ({x.Account})"));
        if (selectedItems.Count > 8)
            preview += $"{Environment.NewLine}• 외 {selectedItems.Count - 8}개";

        var dialog = new ContentDialog
        {
            Title = $"채널 {selectedItems.Count}개 삭제",
            Content = preview + "\n\n선택한 채널을 목록에서 삭제할까요?\n변경 저장 전까지 파일에는 반영되지 않습니다.",
            PrimaryButtonText = "삭제",
            CloseButtonText = "취소",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };

        if (await dialog.ShowAsync() != ContentDialogResult.Primary)
            return;

        suppressChannelCollectionRefresh = true;
        try
        {
            foreach (var selected in selectedItems)
                ChannelItems.Remove(selected);
        }
        finally
        {
            suppressChannelCollectionRefresh = false;
        }

        ClearChannelSelection();
        SyncRawChannelTextFromItems(markDirty: true);
    }

    async void EnableSelectedChannels_Click(object sender, RoutedEventArgs e)
    {
        await SetSelectedChannelsEnabledAsync(true);
    }

    async void DisableSelectedChannels_Click(object sender, RoutedEventArgs e)
    {
        await SetSelectedChannelsEnabledAsync(false);
    }

    async Task SetSelectedChannelsEnabledAsync(bool enabled)
    {
        if (!await EnsureRawChannelEditsAppliedAsync())
            return;

        var selectedItems = GetSelectedChannels();
        if (selectedItems.Count == 0)
        {
            await ShowDialogAsync(
                enabled ? "채널 활성화" : "채널 비활성화",
                "변경할 채널을 하나 이상 선택하세요.");
            return;
        }

        foreach (var selected in selectedItems)
            selected.Enabled = enabled;

        SyncRawChannelTextFromItems(markDirty: true);
    }

    List<EditableChannel> GetSelectedChannels()
    {
        return ChannelList.SelectedItems
            .OfType<EditableChannel>()
            .ToList();
    }

    void ChannelList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        UpdateChannelSelectionUi();
    }

    void SelectAllChannelsCheckBox_Checked(object sender, RoutedEventArgs e)
    {
        if (suppressChannelSelectionSync)
            return;

        ChannelList.SelectAll();
    }

    void SelectAllChannelsCheckBox_Unchecked(object sender, RoutedEventArgs e)
    {
        if (suppressChannelSelectionSync)
            return;

        ChannelList.SelectedItems.Clear();
    }

    void ClearChannelSelection()
    {
        if (ChannelList == null)
            return;

        ChannelList.SelectedItems.Clear();
        UpdateChannelSelectionUi();
    }

    void RestoreChannelSelection(IEnumerable<string> accounts)
    {
        if (ChannelList == null)
            return;

        var wanted = new HashSet<string>(
            accounts.Where(x => !string.IsNullOrWhiteSpace(x)),
            StringComparer.OrdinalIgnoreCase);

        ChannelList.SelectedItems.Clear();
        foreach (var item in VisibleChannelItems)
        {
            if (wanted.Contains(item.Account))
                ChannelList.SelectedItems.Add(item);
        }

        UpdateChannelSelectionUi();
    }

    void UpdateChannelSelectionUi()
    {
        if (ChannelList == null || SelectedChannelCountText == null)
            return;

        var selectedCount = ChannelList.SelectedItems.Count;
        var totalCount = VisibleChannelItems.Count;

        SelectedChannelCountText.Text = $"{selectedCount}개 선택";
        EditChannelButton.IsEnabled = selectedCount == 1;
        SelectedChannelActionsButton.IsEnabled = selectedCount > 0;
        DeleteChannelMenuItem.IsEnabled = selectedCount > 0;
        EnableSelectedChannelsMenuItem.IsEnabled = selectedCount > 0;
        DisableSelectedChannelsMenuItem.IsEnabled = selectedCount > 0;

        suppressChannelSelectionSync = true;
        try
        {
            SelectAllChannelsCheckBox.IsChecked = totalCount == 0 || selectedCount == 0
                ? false
                : selectedCount == totalCount
                    ? true
                    : null;
        }
        finally
        {
            suppressChannelSelectionSync = false;
        }
    }

    void RefreshChannelFilter()
    {
        if (ChannelList == null || ChannelSearchBox == null || ChannelFilterBox == null)
            return;

        var selectedAccounts = ChannelList.SelectedItems
            .OfType<EditableChannel>()
            .Select(x => x.Account)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        var query = ChannelSearchBox.Text?.Trim() ?? "";
        var filter = ChannelFilterBox.SelectedItem?.ToString() ?? "전체 채널";

        var filtered = ChannelItems.Where(item =>
        {
            var searchMatch = query.Length == 0 ||
                item.Name.Contains(query, StringComparison.CurrentCultureIgnoreCase) ||
                item.Account.Contains(query, StringComparison.OrdinalIgnoreCase);
            var filterMatch = filter switch
            {
                "활성 채널" => item.Enabled,
                "비활성 채널" => !item.Enabled,
                "개별 저장 경로" => !string.IsNullOrWhiteSpace(item.OutDir),
                _ => true
            };
            return searchMatch && filterMatch;
        }).ToList();

        VisibleChannelItems.Clear();
        foreach (var item in filtered)
            VisibleChannelItems.Add(item);

        ChannelList.SelectedItems.Clear();
        foreach (var item in VisibleChannelItems.Where(x => selectedAccounts.Contains(x.Account)))
            ChannelList.SelectedItems.Add(item);
        UpdateChannelSelectionUi();
    }

    async void SaveSettings_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var validationError = ValidateSettingsInputsFix36();
            if (!string.IsNullOrWhiteSpace(validationError))
            {
                await ShowDialogAsync("설정 입력 확인", validationError);
                return;
            }

            var current = IniService.Read(iniPath);

            string ExistingOrNewSecret(string key, string entered)
            {
                if (!string.IsNullOrWhiteSpace(entered))
                    return entered;

                return current.TryGetValue(key, out var oldValue) ? oldValue : "";
            }

            string YesNo(CheckBox box) => box.IsChecked == true ? "Y" : "N";
            string Num(NumberBox box, string fallback) =>
                double.IsNaN(box.Value) ? fallback : box.Value.ToString("0.##");

            var quality = QualityBox.SelectedItem?.ToString() switch
            {
                "원본/마스터 (master)" => "master",
                "1080p" => "1080p",
                "720p" => "720p",
                _ => "best"
            };

            var filePattern = FileNamePatternBox.SelectedItem?.ToString() switch
            {
                "제목 + 번호 — 260826_방송제목_01_BJ.ts" => "TITLE_NUMBER",
                "시간 + 제목 — 260826_153000_방송제목_BJ.ts" => "TIME_TITLE",
                "BJ + 제목 — 260826_BJ_방송제목.ts" => "BJ_TITLE",
                _ => "LEGACY"
            };

            var masterQuality = MasterQualityBox.SelectedItem?.ToString() switch
            {
                "master" => "master",
                "1080p" => "1080p",
                "720p" => "720p",
                _ => "auto"
            };

            var updates = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            {
                ["OUTPUT_DIR"] = OutputDirBox.Text?.Trim() ?? "",
                ["QUALITY"] = quality,
                ["FILE_NAME_PATTERN"] = filePattern,
                ["MIN_FREE_SPACE_GB"] = Num(MinDiskBox, "20"),

                ["SOOP_USERNAME"] = SoopUsernameBox.Text?.Trim() ?? "",
                ["SOOP_PASSWORD"] = clearSoopPassword ? "" : ExistingOrNewSecret("SOOP_PASSWORD", SoopPasswordBox.Password),
                ["SOOP_PURGE_CREDENTIALS"] = YesNo(SoopPurgeCredentialsCheck),

                ["CLOUDFLARE_WORKER_URL"] = CloudflareWorkerUrlBox.Text?.Trim() ?? "",
                ["CLOUDFLARE_API_KEY"] = clearCloudflareApiKey ? "" : ExistingOrNewSecret("CLOUDFLARE_API_KEY", CloudflareApiKeyBox.Password),
                ["MASTER_QUALITY"] = masterQuality,

                ["STREAMLINK_PATH"] = string.IsNullOrWhiteSpace(StreamlinkPathBox.Text) ? "AUTO" : StreamlinkPathBox.Text.Trim(),
                ["STREAMLINK_FALLBACK"] = StreamlinkFallbackBox.Text?.Trim() ?? "",

                ["CHECK_INTERVAL"] = Num(CheckIntervalBox, "30"),
                ["CHANNEL_RELOAD_INTERVAL"] = Num(ChannelReloadIntervalBox, "2"),
                ["RECORD_RETRY_INTERVAL"] = Num(RecordRetryIntervalBox, "5"),
                ["RECORD_STALL_TIMEOUT"] = Num(RecordStallTimeoutBox, "90"),
                ["RECORD_MONITOR_INTERVAL"] = Num(RecordMonitorIntervalBox, "5"),
                ["WORKER_MAX_RETRY"] = Num(WorkerMaxRetryBox, "3"),

                ["LOG_ENABLED"] = YesNo(LogEnabledCheck),
                ["LOG_DIR"] = LogDirBox.Text?.Trim() ?? @".\logs",
                ["LOG_RETENTION_DAYS"] = Num(LogRetentionDaysBox, "30"),

                ["CONSOLE_AUTO_FORMAT"] = YesNo(ConsoleAutoFormatCheck),
                ["CONSOLE_COLOR"] = YesNo(ConsoleColorCheck),
                ["CONSOLE_SHOW_PATH"] = YesNo(ConsoleShowPathCheck)
            };

            BackupFile(iniPath);
            UpdateIniFile(iniPath, updates);

            ApplyConsoleDisplayOptions(
                ConsoleAutoFormatCheck.IsChecked == true,
                ConsoleColorCheck.IsChecked == true,
                ConsoleShowPathCheck.IsChecked == true);

            settingsLoading = true;
            SoopPasswordBox.Password = "";
            CloudflareApiKeyBox.Password = "";
            clearSoopPassword = false;
            clearCloudflareApiKey = false;
            settingsLoading = false;
            UpdateSecretStatusFix36(IniService.Read(iniPath));
            SetSettingsDirtyFix36(false);

            await ShowDialogAsync(
                "설정 저장 완료",
                "SOOP_LIVE_SETTING.ini에 저장했습니다.\n\n" +
                "• 디스크·감시·로그·표시: 실행 중 반영\n" +
                "• 녹화 화질·파일명·저장 경로: 다음 녹화부터\n" +
                "• Streamlink 경로: Watcher 재시작 필요\n" +
                "• 인증정보: 재인증되며 Watcher 재시작 시에도 확실하게 적용");
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("설정 저장 실패", ex.Message);
        }
    }

    void LoadChannelItemsFromText(string text)
    {
        var error = TryLoadChannelItemsFromRawText(text, out _);
        if (!string.IsNullOrWhiteSpace(error))
            WriteStartupLog("Channel file parse warning: " + error.Replace(Environment.NewLine, " | "));
    }

    void SyncRawChannelTextFromItems(bool markDirty)
    {
        RefreshChannelFilter();
        var sb = new StringBuilder();
        sb.AppendLine("# ENABLED|NAME|ACCOUNT|OUTDIR");

        foreach (var item in ChannelItems)
        {
            sb.Append(item.Enabled ? "Y" : "N");
            sb.Append('|');
            sb.Append(item.Name?.Trim());
            sb.Append('|');
            sb.Append(item.Account?.Trim());
            sb.Append('|');
            sb.AppendLine(item.OutDir?.Trim() ?? "");
        }

        SetRawChannelText(sb.ToString(), markDirty: false);
        rawChannelTextDirty = false;
        channelTableChangesDirty = markDirty;
        SetChannelChangesDirty(markDirty);
    }

    void SetRawChannelText(string text, bool markDirty)
    {
        lastProgrammaticRawChannelText = NormalizeChannelText(text);
        suppressRawChannelTextChanged = true;
        try
        {
            ChannelsText.Text = text ?? "";
        }
        finally
        {
            suppressRawChannelTextChanged = false;
            rawChannelTextDirty = markDirty;
        }
    }

    static string NormalizeChannelText(string? text) =>
        string.Join("\n", SplitChannelLines(text)).TrimEnd('\n') + "\n";

    static IEnumerable<string> SplitChannelLines(string? text)
    {
        return ChannelFileParser.SplitLines(text);
    }

    string? TryLoadChannelItemsFromRawText(string text, out int parsedCount)
    {
        var error = ChannelFileParser.TryParse(text, out var parsed);
        if (!string.IsNullOrWhiteSpace(error))
        {
            parsedCount = 0;
            return error;
        }

        ReplaceChannelItemsFix38(parsed);
        parsedCount = parsed.Count;
        return null;
    }
    void ReplaceChannelItemsFix38(IEnumerable<EditableChannel> items)
    {
        suppressChannelCollectionRefresh = true;
        try
        {
            ChannelItems.Clear();
            foreach (var item in items)
                ChannelItems.Add(item);
        }
        finally
        {
            suppressChannelCollectionRefresh = false;
        }

        RefreshChannelFilter();
        UpdateDashboardEmptyState();
    }

    async Task<EditableChannel?> ShowChannelEditorAsync(EditableChannel? source)
    {
        var enabled = new CheckBox
        {
            Content = "활성",
            IsChecked = source?.Enabled ?? true
        };
        var account = new TextBox
        {
            Header = "SOOP 계정 ID 또는 채널 URL",
            Text = source?.Account ?? ""
        };
        var resolvedName = source?.Name ?? "";
        var resolvedAccount = source?.Account ?? "";
        var nameValue = new TextBlock
        {
            Text = string.IsNullOrWhiteSpace(resolvedName)
                ? "계정 확인 후 자동 입력됩니다."
                : resolvedName,
            Foreground = White,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            TextWrapping = TextWrapping.Wrap
        };
        var nameStatus = new TextBlock
        {
            Text = source == null ? "SOOP 방송국 정보에서 이름을 확인합니다." : "현재 저장된 이름",
            Foreground = Muted,
            FontSize = 12
        };
        var refreshName = ApplyButtonMetricsFix39(new Button
        {
            Content = source == null ? "계정 확인" : "SOOP 이름 다시 조회"
        }, 132);
        var outDir = new TextBox
        {
            Header = "개별 저장 경로 (선택)",
            Text = source?.OutDir ?? ""
        };

        var panel = new StackPanel { Spacing = 10 };
        panel.Children.Add(enabled);
        panel.Children.Add(account);
        panel.Children.Add(new TextBlock { Text = "방송인 이름", Foreground = Muted });
        panel.Children.Add(nameValue);
        panel.Children.Add(nameStatus);
        panel.Children.Add(refreshName);
        panel.Children.Add(outDir);

        var dialog = new ContentDialog
        {
            Title = source == null ? "채널 추가" : "채널 수정",
            Content = panel,
            PrimaryButtonText = "확인",
            CloseButtonText = "취소",
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };

        refreshName.Click += async (_, _) =>
        {
            var normalized = NormalizeSoopAccount(account.Text);
            if (string.IsNullOrWhiteSpace(normalized))
            {
                nameStatus.Text = "올바른 SOOP 계정 ID 또는 URL을 입력하세요.";
                nameStatus.Foreground = MakeBrush("#F08080");
                return;
            }

            if (IsDuplicateChannelAccount(normalized, source))
            {
                nameStatus.Text = "이미 등록된 SOOP 계정 ID입니다.";
                nameStatus.Foreground = MakeBrush("#F08080");
                return;
            }

            refreshName.IsEnabled = false;
            dialog.IsPrimaryButtonEnabled = false;
            nameStatus.Text = "SOOP 이름 확인 중…";
            nameStatus.Foreground = Muted;
            try
            {
                var lookup = await LookupSoopProfileAsync(normalized);
                if (!lookup.Exists)
                {
                    nameStatus.Text = lookup.Error ?? "존재하지 않는 SOOP 계정입니다.";
                    nameStatus.Foreground = MakeBrush("#F08080");
                    return;
                }

                resolvedAccount = lookup.Account;
                resolvedName = lookup.Name;
                account.Text = lookup.Account;
                nameValue.Text = lookup.Name;
                nameStatus.Text = "SOOP에서 확인됨";
                nameStatus.Foreground = Accent;
            }
            finally
            {
                refreshName.IsEnabled = true;
                dialog.IsPrimaryButtonEnabled = true;
            }
        };

        if (await dialog.ShowAsync() != ContentDialogResult.Primary)
            return null;

        var normalizedAccount = NormalizeSoopAccount(account.Text);
        if (string.IsNullOrWhiteSpace(normalizedAccount))
        {
            await ShowDialogAsync("채널 입력 확인", "올바른 SOOP 계정 ID 또는 채널 URL을 입력하세요.");
            return null;
        }

        if (IsDuplicateChannelAccount(normalizedAccount, source))
        {
            await ShowDialogAsync("채널 입력 확인", $"이미 등록된 SOOP 계정 ID입니다: {normalizedAccount}");
            return null;
        }

        if (!string.Equals(resolvedAccount, normalizedAccount, StringComparison.OrdinalIgnoreCase) ||
            string.IsNullOrWhiteSpace(resolvedName))
        {
            var lookup = await LookupSoopProfileAsync(normalizedAccount);
            if (!lookup.Exists && string.IsNullOrWhiteSpace(lookup.Error))
            {
                await ShowDialogAsync("채널 입력 확인", "존재하지 않는 SOOP 계정입니다.");
                return null;
            }

            if (lookup.Exists)
            {
                normalizedAccount = lookup.Account;
                resolvedName = lookup.Name;
            }
            else
            {
                resolvedName = normalizedAccount;
                await ShowDialogAsync(
                    "방송인 이름 조회 실패",
                    (lookup.Error ?? "SOOP 이름을 확인하지 못했습니다.") +
                    "\n\n계정 ID를 임시 이름으로 사용합니다. 나중에 수정 화면에서 다시 조회할 수 있습니다.");
            }
        }

        return new EditableChannel
        {
            Enabled = enabled.IsChecked == true,
            Name = resolvedName,
            Account = normalizedAccount,
            OutDir = outDir.Text?.Trim() ?? ""
        };
    }

    bool IsDuplicateChannelAccount(string account, EditableChannel? except)
    {
        return ChannelItems.Any(x =>
            !ReferenceEquals(x, except) &&
            string.Equals(x.Account, account, StringComparison.OrdinalIgnoreCase));
    }

    static string? NormalizeSoopAccount(string? input)
    {
        return ChannelFileParser.NormalizeAccount(input);
    }

    sealed record SoopProfileLookup(
        bool Exists,
        string Account,
        string Name,
        string? Error = null);

    static async Task<SoopProfileLookup> LookupSoopProfileAsync(string account)
    {
        try
        {
            var uri = "https://st.sooplive.com/api/get_station_status.php?szBjId=" +
                      Uri.EscapeDataString(account);
            using var response = await SoopProfileClient.GetAsync(uri);
            response.EnsureSuccessStatusCode();
            await using var stream = await response.Content.ReadAsStreamAsync();
            using var json = await JsonDocument.ParseAsync(stream);
            var root = json.RootElement;

            if (!root.TryGetProperty("RESULT", out var result) || result.GetInt32() != 1 ||
                !root.TryGetProperty("DATA", out var data))
            {
                return new SoopProfileLookup(false, account, "");
            }

            var resolvedAccount = data.TryGetProperty("user_id", out var userId)
                ? userId.GetString()?.Trim() ?? ""
                : "";
            var name = data.TryGetProperty("user_nick", out var userNick)
                ? userNick.GetString()?.Trim() ?? ""
                : "";

            if (string.IsNullOrWhiteSpace(resolvedAccount) ||
                !string.Equals(resolvedAccount, account, StringComparison.OrdinalIgnoreCase))
            {
                return new SoopProfileLookup(false, account, "");
            }

            if (string.IsNullOrWhiteSpace(name) && data.TryGetProperty("station_name", out var stationName))
                name = stationName.GetString()?.Trim() ?? "";

            if (string.IsNullOrWhiteSpace(name))
                name = resolvedAccount;

            return new SoopProfileLookup(true, resolvedAccount, name);
        }
        catch (Exception ex)
        {
            return new SoopProfileLookup(false, account, "", "SOOP 이름 조회 오류: " + ex.Message);
        }
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

    void UpdateSelectedRecordingActionButton()
    {
        if (StopSelectedRecordingButton == null)
            return;

        if (RecordingList?.SelectedItem is not ChannelStatus selected)
        {
            StopSelectedRecordingButton.Content = "■ 선택 채널 녹화 중지";
            StopSelectedRecordingButton.IsEnabled = false;
            return;
        }

        if (selected.Status == "● REC")
        {
            StopSelectedRecordingButton.Content = "■ 선택 채널 녹화 중지";
            StopSelectedRecordingButton.IsEnabled = true;
            return;
        }

        StopSelectedRecordingButton.Content = "녹화 상태 확인 중";
        StopSelectedRecordingButton.IsEnabled = false;
    }

    async void StopSelectedRecording_Click(object sender, RoutedEventArgs e)
    {
        if (RecordingList.SelectedItem is not ChannelStatus selected)
            return;

        var account = selected.Account;
        if (string.IsNullOrWhiteSpace(account))
            account = ChannelItems.FirstOrDefault(
                x => string.Equals(x.Name, selected.Name, StringComparison.OrdinalIgnoreCase)
            )?.Account ?? "";

        if (string.IsNullOrWhiteSpace(account))
        {
            await ShowDialogAsync(
                "채널 녹화 제어",
                "선택한 채널의 SOOP 계정 ID를 찾지 못했습니다.");
            return;
        }

        try
        {
            SendChannelControlCommand("STOP_ONCE", account);
            StopSelectedRecordingButton.Content = "중지 요청 중";
            StopSelectedRecordingButton.IsEnabled = false;
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("채널 녹화 제어 실패", ex.Message);
            UpdateSelectedRecordingActionButton();
        }
    }

    async void ResumeStopped_Click(object sender, RoutedEventArgs e)
    {
        if (StoppedFlyoutList.SelectedItem is not ChannelStatus selected)
            return;

        try
        {
            SendChannelControlCommand("RESUME_ONCE", selected.Account);
            selected.Status = "다시 시작 확인 중";
            selected.Detail = "방송 상태를 다시 확인하고 있습니다.";
            selected.IsSuspended = false;
            StoppedItems.Remove(selected);
            UpdateCounts();
            if (StoppedSummaryButton.Flyout is Flyout flyout)
                flyout.Hide();
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("녹화 다시 시작 실패", ex.Message);
        }
    }

    void SendChannelControlCommand(string action, string account)
    {
        if (string.IsNullOrWhiteSpace(account))
            throw new InvalidOperationException("SOOP 계정 ID를 찾지 못했습니다.");

        var controlDir = Path.Combine(backendDir, "control");
        Directory.CreateDirectory(controlDir);
        var id = Guid.NewGuid().ToString("N");
        var temp = Path.Combine(controlDir, id + ".tmp");
        var cmd = Path.Combine(controlDir, id + ".cmd");
        File.WriteAllText(temp, $"{action}|{account}", new UTF8Encoding(false));
        File.Move(temp, cmd);
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
        LogBox.Text="";
    }

    void OpenBackend_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            Process.Start(new ProcessStartInfo("explorer.exe", backendDir)
            {
                UseShellExecute = true
            });
        }
        catch { }
    }

    bool IsGuiEventLogLine(string line)
    {
        if (string.IsNullOrWhiteSpace(line)) return false;
        var text=line.Trim();
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
