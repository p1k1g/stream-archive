using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Diagnostics;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using WinRT.Interop;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    TextBlock SettingsDirtyStateText = null!;
    TextBlock OutputPathStatusText = null!;
    TextBlock LogPathStatusText = null!;
    TextBlock SoopSecretStatusText = null!;
    TextBlock CloudflareSecretStatusText = null!;
    TextBlock SoopTestStatusText = null!;
    TextBlock WorkerTestStatusText = null!;
    TextBlock StreamlinkTestStatusText = null!;
    Button DiscardSettingsButton = null!;
    bool settingsChangesDirty;
    bool settingsLoading;
    int settingsUiTransitionDepth;
    bool clearSoopPassword;
    bool clearCloudflareApiKey;

    static readonly HttpClient SettingsTestClient = new()
    {
        Timeout = TimeSpan.FromSeconds(12)
    };

    FrameworkElement BuildSettingsViewFix36()
    {
        var root = new Grid
        {
            Visibility = Visibility.Collapsed,
            RequestedTheme = ElementTheme.Dark
        };
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            HorizontalContentAlignment = HorizontalAlignment.Stretch
        };
        var stack = new StackPanel
        {
            Padding = DesignTokens.PagePadding,
            Spacing = 14,
            HorizontalAlignment = HorizontalAlignment.Stretch
        };

        stack.Children.Add(new TextBlock
        {
            Text = "설정",
            Foreground = White,
            FontSize = 24,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        stack.Children.Add(new TextBlock
        {
            Text = "자주 쓰는 설정만 먼저 표시합니다. 고급 설정은 아래에서 펼칠 수 있습니다.",
            Foreground = Muted,
            TextWrapping = TextWrapping.Wrap
        });

        var recording = SettingsCard(
            "녹화",
            "저장 위치, 최종 녹화 화질, 파일명 및 디스크 보호 기준입니다.");
        recording.Children.Add(ApplyHintFix36("경로·화질·파일명: 다음 녹화부터 · 디스크 기준: 즉시 적용"));
        recording.Children.Add(FieldLabel("기본 저장 경로"));
        OutputDirBox = WideTextBox();
        var outputRow = AdaptiveFieldRowFix40(
            OutputDirBox,
            ActionButtonFix36("찾아보기", async (_, _) => await PickFolderFix36(OutputDirBox)),
            ActionButtonFix36("폴더 열기", (_, _) => OpenFolderFix36(OutputDirBox.Text)));
        recording.Children.Add(outputRow);
        OutputPathStatusText = StatusTextFix36("경로를 확인하는 중입니다.");
        recording.Children.Add(OutputPathStatusText);
        recording.Children.Add(new TextBlock
        {
            Text = "채널 관리에서 개별 저장 경로를 지정한 채널은 해당 경로가 우선합니다.",
            Foreground = Muted,
            FontSize = 12,
            TextWrapping = TextWrapping.Wrap
        });
        recording.Children.Add(ActionButtonFix36("폴더 쓰기 테스트", async (_, _) => await TestFolderFix36(OutputDirBox.Text, OutputPathStatusText)));

        recording.Children.Add(FieldLabel("녹화 화질"));
        QualityBox = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var item in new[] { "최고 화질 (best)", "원본/마스터 (master)", "1080p", "720p" })
            QualityBox.Items.Add(item);
        recording.Children.Add(QualityBox);

        recording.Children.Add(FieldLabel("파일명 형식"));
        FileNamePatternBox = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var item in new[]
        {
            "기본 — 260826_153000_BJ.ts",
            "제목 + 번호 — 260826_방송제목_01_BJ.ts",
            "시간 + 제목 — 260826_153000_방송제목_BJ.ts",
            "BJ + 제목 — 260826_BJ_방송제목.ts"
        }) FileNamePatternBox.Items.Add(item);
        recording.Children.Add(FileNamePatternBox);
        recording.Children.Add(FieldLabel("최소 디스크 여유공간 (GB) · 권장 20GB"));
        MinDiskBox = NumberSetting(1, 10000, 180);
        recording.Children.Add(MinDiskBox);
        recording.Children.Add(ActionButtonFix36("녹화 설정 기본값 복원", (_, _) => ResetRecordingDefaultsFix36()));
        AddSettingsCardFix36(stack, recording);

        var soop = SettingsCard(
            "SOOP 로그인",
            "연령 제한 방송 등 로그인 쿠키가 필요할 때 사용합니다. 비워두면 비로그인으로 동작합니다.");
        soop.Children.Add(ApplyHintFix36("저장 후 재인증 · Watcher 재시작 시에도 확실하게 적용"));
        soop.Children.Add(FieldLabel("SOOP 아이디"));
        SoopUsernameBox = WideTextBox();
        soop.Children.Add(SoopUsernameBox);
        soop.Children.Add(FieldLabel("SOOP 비밀번호"));
        SoopPasswordBox = new PasswordBox
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            PlaceholderText = "새 값 입력 시 교체 · 비워두면 저장된 값 유지"
        };
        soop.Children.Add(SoopPasswordBox);
        SoopSecretStatusText = StatusTextFix36("저장 상태 확인 중");
        soop.Children.Add(SoopSecretStatusText);
        var soopActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        soopActions.Children.Add(ActionButtonFix36("저장된 비밀번호 삭제", (_, _) => StageSecretDeleteFix36(true)));
        soopActions.Children.Add(ActionButtonFix36("SOOP 로그인 테스트", async (_, _) => await TestSoopLoginFix36()));
        soop.Children.Add(soopActions);
        SoopTestStatusText = StatusTextFix36("");
        soop.Children.Add(SoopTestStatusText);
        SoopPurgeCredentialsCheck = new CheckBox
        {
            Content = "Watcher 시작 시 기존 인증정보를 정리하고 다시 로그인",
            Foreground = White
        };
        soop.Children.Add(SoopPurgeCredentialsCheck);
        soop.Children.Add(ActionButtonFix36("로그인 설정 기본값 복원", (_, _) => ResetSoopDefaultsFix36()));
        AddSettingsCardFix36(stack, soop);

        var advancedToggle = ApplyButtonMetricsFix39(new Button
        {
            Content = "고급 설정 펼치기",
            HorizontalAlignment = HorizontalAlignment.Left
        }, 142);
        stack.Children.Add(advancedToggle);
        var advanced = new StackPanel
        {
            Spacing = 14,
            Visibility = Visibility.Collapsed,
            HorizontalAlignment = HorizontalAlignment.Stretch
        };
        advancedToggle.Click += (_, _) =>
        {
            var wasDirty = settingsChangesDirty;
            settingsUiTransitionDepth++;
            var opening = advanced.Visibility != Visibility.Visible;
            advanced.Visibility = opening ? Visibility.Visible : Visibility.Collapsed;
            advancedToggle.Content = opening ? "고급 설정 접기" : "고급 설정 펼치기";
            // NumberBox can commit its display Text asynchronously while the
            // previously collapsed panel is first measured. Two dispatcher
            // turns were not sufficient after Settings import/save, so keep a
            // short, layout-only suppression window and preserve any dirty
            // state that existed before the panel transition.
            var settleTimer = DispatcherQueue.CreateTimer();
            settleTimer.Interval = TimeSpan.FromMilliseconds(500);
            settleTimer.IsRepeating = false;
            settleTimer.Tick += (_, _) =>
            {
                settingsUiTransitionDepth = Math.Max(0, settingsUiTransitionDepth - 1);
                if (!wasDirty && settingsUiTransitionDepth == 0)
                    SetSettingsDirtyFix36(false);
            };
            settleTimer.Start();
        };

        var cloudflare = SettingsCard(
            "Cloudflare Worker",
            "글로벌 master HLS 주소를 발급합니다. 최종 녹화 화질은 위의 녹화 화질에서 선택합니다.");
        cloudflare.Children.Add(ApplyHintFix36("저장 후 다음 Worker 요청부터 적용"));
        cloudflare.Children.Add(FieldLabel("Worker URL"));
        CloudflareWorkerUrlBox = WideTextBox();
        CloudflareWorkerUrlBox.PlaceholderText = "https://xxxx.workers.dev/soop/url";
        cloudflare.Children.Add(CloudflareWorkerUrlBox);
        cloudflare.Children.Add(FieldLabel("API Key"));
        CloudflareApiKeyBox = new PasswordBox
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            PlaceholderText = "새 값 입력 시 교체 · 비워두면 저장된 값 유지"
        };
        cloudflare.Children.Add(CloudflareApiKeyBox);
        CloudflareSecretStatusText = StatusTextFix36("저장 상태 확인 중");
        cloudflare.Children.Add(CloudflareSecretStatusText);
        var workerActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        workerActions.Children.Add(ActionButtonFix36("저장된 API Key 삭제", (_, _) => StageSecretDeleteFix36(false)));
        workerActions.Children.Add(ActionButtonFix36("Worker 연결 테스트", async (_, _) => await TestWorkerFix36()));
        cloudflare.Children.Add(workerActions);
        WorkerTestStatusText = StatusTextFix36("");
        cloudflare.Children.Add(WorkerTestStatusText);
        cloudflare.Children.Add(FieldLabel("Worker 요청 화질 · 권장: 자동"));
        MasterQualityBox = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var item in new[] { "자동 (auto)", "master", "1080p", "720p" }) MasterQualityBox.Items.Add(item);
        cloudflare.Children.Add(MasterQualityBox);
        cloudflare.Children.Add(ActionButtonFix36("요청 화질 자동으로 복원", (_, _) => ResetWorkerDefaultsFix36()));
        AddSettingsCardFix36(advanced, cloudflare);

        var streamlink = SettingsCard(
            "Streamlink",
            "기본은 자동 탐색입니다. 자동 탐색이 실패할 때만 직접 경로를 지정하세요.");
        streamlink.Children.Add(ApplyHintFix36("경로 변경: Watcher 재시작 필요"));
        streamlink.Children.Add(FieldLabel("Streamlink 경로"));
        StreamlinkPathBox = WideTextBox();
        StreamlinkPathBox.PlaceholderText = "AUTO";
        streamlink.Children.Add(StreamlinkPathBox);
        streamlink.Children.Add(FieldLabel("대체 경로"));
        StreamlinkFallbackBox = WideTextBox();
        streamlink.Children.Add(StreamlinkFallbackBox);
        var streamActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        streamActions.Children.Add(ActionButtonFix36("Streamlink 확인", async (_, _) => await TestStreamlinkFix36()));
        streamActions.Children.Add(ActionButtonFix36("기본값 복원", (_, _) => ResetStreamlinkDefaultsFix36()));
        streamlink.Children.Add(streamActions);
        StreamlinkTestStatusText = StatusTextFix36("");
        streamlink.Children.Add(StreamlinkTestStatusText);
        AddSettingsCardFix36(advanced, streamlink);

        var monitoring = SettingsCard("감시 · 재시도", "특별한 이유가 없으면 권장값을 사용하세요.");
        monitoring.Children.Add(ApplyHintFix36("Watcher 실행 중 hot reload로 적용"));
        var monitorGrid = new Grid { ColumnSpacing = 18, RowSpacing = 10 };
        monitorGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        monitorGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        monitorGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        monitorGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var left = new StackPanel { Spacing = 7 };
        left.Children.Add(FieldLabel("LIVE 확인 주기 (초) · 권장 30"));
        CheckIntervalBox = NumberSetting(1, 3600, 180); left.Children.Add(CheckIntervalBox);
        left.Children.Add(FieldLabel("채널 파일 재확인 (초) · 권장 2"));
        ChannelReloadIntervalBox = NumberSetting(1, 300, 180); left.Children.Add(ChannelReloadIntervalBox);
        left.Children.Add(FieldLabel("녹화 재시도 (초) · 권장 5"));
        RecordRetryIntervalBox = NumberSetting(1, 600, 180); left.Children.Add(RecordRetryIntervalBox);
        var right = new StackPanel { Spacing = 7 };
        right.Children.Add(FieldLabel("Stall 판정 (초) · 권장 90"));
        RecordStallTimeoutBox = NumberSetting(10, 3600, 180); right.Children.Add(RecordStallTimeoutBox);
        right.Children.Add(FieldLabel("녹화 상태 확인 (초) · 권장 5"));
        RecordMonitorIntervalBox = NumberSetting(1, 300, 180); right.Children.Add(RecordMonitorIntervalBox);
        right.Children.Add(FieldLabel("Worker 최대 재시도 · 권장 3"));
        WorkerMaxRetryBox = NumberSetting(1, 10, 180); right.Children.Add(WorkerMaxRetryBox);
        Grid.SetColumn(right, 1); monitorGrid.Children.Add(left); monitorGrid.Children.Add(right);
        monitorGrid.SizeChanged += (_, args) =>
        {
            var narrow = args.NewSize.Width < 620;
            Grid.SetColumn(right, narrow ? 0 : 1);
            Grid.SetRow(right, narrow ? 1 : 0);
        };
        monitoring.Children.Add(monitorGrid);
        monitoring.Children.Add(ActionButtonFix36("감시 설정 권장값 복원", (_, _) => ResetMonitoringDefaultsFix36()));
        AddSettingsCardFix36(advanced, monitoring);

        var logs = SettingsCard("로그 · 표시", "파일 로그와 GUI 대시보드 표시 형식을 설정합니다.");
        logs.Children.Add(ApplyHintFix36("로그·표시: 즉시 적용"));
        LogEnabledCheck = new CheckBox { Content = "파일 로그 저장", Foreground = White };
        logs.Children.Add(LogEnabledCheck);
        logs.Children.Add(FieldLabel("로그 폴더"));
        LogDirBox = WideTextBox();
        var logRow = AdaptiveFieldRowFix40(
            LogDirBox,
            ActionButtonFix36("찾아보기", async (_, _) => await PickFolderFix36(LogDirBox)),
            ActionButtonFix36("폴더 열기", (_, _) => OpenFolderFix36(ResolveLogPathFix36(LogDirBox.Text))));
        logs.Children.Add(logRow);
        LogPathStatusText = StatusTextFix36("");
        logs.Children.Add(LogPathStatusText);
        logs.Children.Add(ActionButtonFix36("로그 폴더 쓰기 테스트", async (_, _) => await TestFolderFix36(ResolveLogPathFix36(LogDirBox.Text), LogPathStatusText)));
        logs.Children.Add(FieldLabel("로그 보관일 · 권장 30일"));
        LogRetentionDaysBox = NumberSetting(1, 3650, 180); logs.Children.Add(LogRetentionDaysBox);
        ConsoleAutoFormatCheck = new CheckBox { Content = "채널 자동 정렬 (백엔드 + 대시보드)", Foreground = White };
        ConsoleColorCheck = new CheckBox { Content = "상태 색상 사용 (백엔드 + 대시보드)", Foreground = White };
        ConsoleShowPathCheck = new CheckBox { Content = "녹화 파일 경로 표시 (백엔드 + 대시보드)", Foreground = White };
        logs.Children.Add(ConsoleAutoFormatCheck); logs.Children.Add(ConsoleColorCheck); logs.Children.Add(ConsoleShowPathCheck);
        logs.Children.Add(FieldLabel("트레이 알림"));
        NotifyRecordStartCheck = new CheckBox { Content = "녹화 시작 알림", Foreground = White };
        NotifyRecordFinishCheck = new CheckBox { Content = "녹화 완료 알림", Foreground = White };
        NotifyWarningCheck = new CheckBox { Content = "녹화 실패·디스크·인증 경고 알림", Foreground = White };
        logs.Children.Add(NotifyRecordStartCheck);
        logs.Children.Add(NotifyRecordFinishCheck);
        logs.Children.Add(NotifyWarningCheck);
        logs.Children.Add(ActionButtonFix36("로그·표시 기본값 복원", (_, _) => ResetLogDefaultsFix36()));
        AddSettingsCardFix36(advanced, logs);
        stack.Children.Add(advanced);

        scroll.Content = stack;
        root.Children.Add(scroll);

        var saveBarGrid = new Grid
        {
            Padding = new Thickness(22, 12, 22, 12),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            ColumnSpacing = 12
        };
        saveBarGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        saveBarGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        SettingsDirtyStateText = new TextBlock
        {
            Text = "✓ 저장된 상태",
            Foreground = MakeBrush("#7F8A99"),
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis
        };
        saveBarGrid.Children.Add(SettingsDirtyStateText);
        var saveActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        saveActions.Children.Add(ActionButtonFix36("설정 내보내기", async (_, _) => await ExportSettingsFix36(), 112));
        saveActions.Children.Add(ActionButtonFix36("설정 가져오기", async (_, _) => await ImportSettingsFix36(), 112));
        DiscardSettingsButton = ApplyButtonMetricsFix39(new Button { Content = "변경 취소", IsEnabled = false }, 104);
        DiscardSettingsButton.Click += (_, _) => LoadStaticFiles();
        SaveSettingsButton = ApplyButtonMetricsFix39(new Button
        {
            Content = "설정 저장",
            IsEnabled = false,
            Background = Accent,
            Foreground = MakeBrush("#102118")
        }, 104);
        SaveSettingsButton.Click += SaveSettings_Click;
        DesignTokens.AddAccelerator(
            SaveSettingsButton,
            Windows.System.VirtualKey.S,
            Windows.System.VirtualKeyModifiers.Control);
        saveActions.Children.Add(DiscardSettingsButton); saveActions.Children.Add(SaveSettingsButton);
        Grid.SetColumn(saveActions, 1); saveBarGrid.Children.Add(saveActions);
        var saveBar = new Border { Background = MakeBrush("#1B2026"), BorderBrush = MakeBrush("#3A414A"), BorderThickness = new Thickness(0, 1, 0, 0), Child = saveBarGrid };
        Grid.SetRow(saveBar, 1); root.Children.Add(saveBar);

        HookSettingsDirtyTrackingFix36();
        return root;
    }

    static void AddSettingsCardFix36(Panel parent, StackPanel card) => parent.Children.Add(new Border
    {
        Background = DesignTokens.Surface,
        BorderBrush = DesignTokens.Border,
        BorderThickness = new Thickness(1),
        CornerRadius = DesignTokens.CardRadius,
        HorizontalAlignment = HorizontalAlignment.Stretch,
        Child = card
    });

    static Grid AdaptiveFieldRowFix40(FrameworkElement field, params Button[] actions)
    {
        var row = new Grid { ColumnSpacing = 8, HorizontalAlignment = HorizontalAlignment.Stretch };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        field.HorizontalAlignment = HorizontalAlignment.Stretch;
        row.Children.Add(field);

        for (var index = 0; index < actions.Length; index++)
        {
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            Grid.SetColumn(actions[index], index + 1);
            row.Children.Add(actions[index]);
        }

        return row;
    }

    TextBlock ApplyHintFix36(string text) => new()
    {
        Text = text,
        Foreground = MakeBrush("#75B9F2"),
        FontSize = 12,
        TextWrapping = TextWrapping.Wrap
    };

    TextBlock StatusTextFix36(string text) => new()
    {
        Text = text,
        Foreground = Muted,
        FontSize = 12,
        TextWrapping = TextWrapping.Wrap
    };

    Button ActionButtonFix36(string text, RoutedEventHandler handler, double minWidth = 104)
    {
        var button = ApplyButtonMetricsFix39(
            new Button { Content = text, HorizontalAlignment = HorizontalAlignment.Left },
            minWidth);
        button.Click += handler;
        return button;
    }

    void HookSettingsDirtyTrackingFix36()
    {
        foreach (var box in new[] { OutputDirBox, SoopUsernameBox, CloudflareWorkerUrlBox, StreamlinkPathBox, StreamlinkFallbackBox, LogDirBox })
            box.TextChanged += (_, _) => MarkSettingsDirtyFix36();
        SoopPasswordBox.PasswordChanged += (_, _) => { if (SoopPasswordBox.Password.Length > 0) clearSoopPassword = false; MarkSettingsDirtyFix36(); UpdateSecretStatusFix36(); };
        CloudflareApiKeyBox.PasswordChanged += (_, _) => { if (CloudflareApiKeyBox.Password.Length > 0) clearCloudflareApiKey = false; MarkSettingsDirtyFix36(); UpdateSecretStatusFix36(); };
        foreach (var box in new[] { QualityBox, FileNamePatternBox, MasterQualityBox }) box.SelectionChanged += (_, _) => MarkSettingsDirtyFix36();
        foreach (var box in new[] { MinDiskBox, CheckIntervalBox, ChannelReloadIntervalBox, RecordRetryIntervalBox, RecordStallTimeoutBox, RecordMonitorIntervalBox, WorkerMaxRetryBox, LogRetentionDaysBox })
        {
            box.ValueChanged += (_, _) => MarkSettingsDirtyFix36();
            // Text changes immediately for typing, paste, touch keyboard, and
            // accessibility input, while Value is committed later. Tracking
            // the dependency property avoids KeyUp false positives from
            // navigation/modifier keys and still enables Save before blur.
            box.RegisterPropertyChangedCallback(
                NumberBox.TextProperty,
                (_, _) => MarkSettingsDirtyFix36());
        }
        foreach (var box in new[] { SoopPurgeCredentialsCheck, LogEnabledCheck, ConsoleAutoFormatCheck, ConsoleColorCheck, ConsoleShowPathCheck, NotifyRecordStartCheck, NotifyRecordFinishCheck, NotifyWarningCheck })
        {
            box.Checked += (_, _) => MarkSettingsDirtyFix36();
            box.Unchecked += (_, _) => MarkSettingsDirtyFix36();
        }
        OutputDirBox.TextChanged += (_, _) => RefreshPathStatusFix36();
        MinDiskBox.ValueChanged += (_, _) => RefreshPathStatusFix36();
        LogDirBox.TextChanged += (_, _) => RefreshLogPathStatusFix36();
    }

    void MarkSettingsDirtyFix36()
    {
        if (!settingsLoading && settingsUiTransitionDepth == 0)
            SetSettingsDirtyFix36(true);
    }

    void SetSettingsDirtyFix36(bool dirty)
    {
        settingsChangesDirty = dirty;
        if (SettingsDirtyStateText == null) return;
        SettingsDirtyStateText.Text = dirty ? "● 저장되지 않은 설정 변경" : "✓ 저장된 상태";
        SettingsDirtyStateText.Foreground = dirty ? MakeBrush("#F4C95D") : MakeBrush("#7F8A99");
        SaveSettingsButton.IsEnabled = dirty;
        DiscardSettingsButton.IsEnabled = dirty;
    }

    void UpdateSecretStatusFix36(Dictionary<string, string>? cfg = null)
    {
        cfg ??= IniService.Read(iniPath);
        if (SoopSecretStatusText != null)
        {
            var exists = !clearSoopPassword && (SoopPasswordBox.Password.Length > 0 || (cfg.TryGetValue("SOOP_PASSWORD", out var value) && !string.IsNullOrWhiteSpace(value)));
            var protectedValue = cfg.TryGetValue("SOOP_PASSWORD", out var storedPassword) && SecretProtectionService.IsProtected(storedPassword);
            SoopSecretStatusText.Text = clearSoopPassword ? "● 저장 시 기존 비밀번호 삭제"
                : SoopPasswordBox.Password.Length > 0 ? "● 저장 시 Windows DPAPI로 보호"
                : protectedValue ? "✓ Windows DPAPI로 보호된 비밀번호 있음"
                : exists ? "⚠ 기존 평문 비밀번호 · 다음 저장 시 DPAPI 전환"
                : "저장된 비밀번호 없음";
            SoopSecretStatusText.Foreground = clearSoopPassword || (exists && !protectedValue)
                ? MakeBrush("#F4C95D") : exists ? Accent : Muted;
        }
        if (CloudflareSecretStatusText != null)
        {
            var exists = !clearCloudflareApiKey && (CloudflareApiKeyBox.Password.Length > 0 || (cfg.TryGetValue("CLOUDFLARE_API_KEY", out var value) && !string.IsNullOrWhiteSpace(value)));
            var protectedValue = cfg.TryGetValue("CLOUDFLARE_API_KEY", out var storedKey) && SecretProtectionService.IsProtected(storedKey);
            CloudflareSecretStatusText.Text = clearCloudflareApiKey ? "● 저장 시 기존 API Key 삭제"
                : CloudflareApiKeyBox.Password.Length > 0 ? "● 저장 시 Windows DPAPI로 보호"
                : protectedValue ? "✓ Windows DPAPI로 보호된 API Key 있음"
                : exists ? "⚠ 기존 평문 API Key · 다음 저장 시 DPAPI 전환"
                : "저장된 API Key 없음";
            CloudflareSecretStatusText.Foreground = clearCloudflareApiKey || (exists && !protectedValue)
                ? MakeBrush("#F4C95D") : exists ? Accent : Muted;
        }
    }

    void StageSecretDeleteFix36(bool soop)
    {
        if (soop) { clearSoopPassword = true; SoopPasswordBox.Password = ""; }
        else { clearCloudflareApiKey = true; CloudflareApiKeyBox.Password = ""; }
        SetSettingsDirtyFix36(true);
        UpdateSecretStatusFix36();
    }

    string EffectiveSecretFix36(string key, string entered, bool clear)
    {
        if (clear) return "";
        if (!string.IsNullOrWhiteSpace(entered)) return entered;
        var cfg = IniService.Read(iniPath);
        return cfg.TryGetValue(key, out var value)
            ? SecretProtectionService.Unprotect(value)
            : "";
    }

    async Task PickFolderFix36(TextBox target)
    {
        var picker = new Windows.Storage.Pickers.FolderPicker();
        picker.FileTypeFilter.Add("*");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var folder = await picker.PickSingleFolderAsync();
        if (folder != null) target.Text = folder.Path;
    }

    void OpenFolderFix36(string? path)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(path)) throw new InvalidOperationException("폴더 경로가 비어 있습니다.");
            Directory.CreateDirectory(path);
            Process.Start(new ProcessStartInfo("explorer.exe", $"\"{path}\"") { UseShellExecute = true });
        }
        catch (Exception ex) { _ = ShowDialogAsync("폴더 열기 실패", ex.Message); }
    }

    async Task TestFolderFix36(string? path, TextBlock status)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(path)) throw new InvalidOperationException("폴더 경로가 비어 있습니다.");
            Directory.CreateDirectory(path);
            var probe = Path.Combine(path, $".soop_write_test_{Guid.NewGuid():N}.tmp");
            await File.WriteAllTextAsync(probe, "SOOP LIVE write test", Encoding.UTF8);
            File.Delete(probe);
            status.Text = "✓ 폴더 생성·쓰기·삭제 테스트 성공";
            status.Foreground = Accent;
        }
        catch (Exception ex)
        {
            status.Text = "✕ 폴더 쓰기 실패: " + ex.Message;
            status.Foreground = MakeBrush("#F08080");
        }
    }

    void RefreshPathStatusFix36()
    {
        if (OutputPathStatusText == null) return;
        try
        {
            var path = OutputDirBox.Text?.Trim();
            if (string.IsNullOrWhiteSpace(path)) throw new InvalidOperationException("경로가 비어 있습니다.");
            var root = Path.GetPathRoot(Path.GetFullPath(path));
            if (string.IsNullOrWhiteSpace(root)) throw new InvalidOperationException("드라이브를 확인할 수 없습니다.");
            var drive = new DriveInfo(root);
            var freeGb = drive.AvailableFreeSpace / 1024d / 1024d / 1024d;
            var minimum = double.IsNaN(MinDiskBox.Value) ? 20 : MinDiskBox.Value;
            OutputPathStatusText.Text = $"{(freeGb >= minimum ? "✓" : "⚠")} {drive.Name} 여유 공간 {freeGb:0.0}GB · 최소 기준 {minimum:0.##}GB";
            OutputPathStatusText.Foreground = freeGb >= minimum ? Accent : MakeBrush("#F4C95D");
        }
        catch (Exception ex)
        {
            OutputPathStatusText.Text = "✕ " + ex.Message;
            OutputPathStatusText.Foreground = MakeBrush("#F08080");
        }
    }

    string ResolveLogPathFix36(string? path)
    {
        var value = string.IsNullOrWhiteSpace(path) ? @".\logs" : path.Trim();
        return Path.IsPathRooted(value) ? value : Path.GetFullPath(Path.Combine(backendDir, value));
    }

    void RefreshLogPathStatusFix36()
    {
        if (LogPathStatusText == null) return;
        try { LogPathStatusText.Text = "실제 경로: " + ResolveLogPathFix36(LogDirBox.Text); LogPathStatusText.Foreground = Muted; }
        catch (Exception ex) { LogPathStatusText.Text = "✕ " + ex.Message; LogPathStatusText.Foreground = MakeBrush("#F08080"); }
    }

    async Task TestSoopLoginFix36()
    {
        SoopTestStatusText.Text = "SOOP 로그인 확인 중…"; SoopTestStatusText.Foreground = Muted;
        try
        {
            var username = SoopUsernameBox.Text?.Trim() ?? "";
            var password = EffectiveSecretFix36("SOOP_PASSWORD", SoopPasswordBox.Password, clearSoopPassword);
            if (username.Length == 0 || password.Length == 0) throw new InvalidOperationException("아이디와 비밀번호를 입력하거나 저장된 값을 유지하세요.");
            var handler = new HttpClientHandler { UseCookies = true, CookieContainer = new CookieContainer(), AutomaticDecompression = DecompressionMethods.All };
            using var client = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(12) };
            using var content = new FormUrlEncodedContent(new Dictionary<string, string>
            {
                ["szWork"] = "login", ["szType"] = "json", ["szUid"] = username, ["szPassword"] = password,
                ["isSaveId"] = "true", ["isSavePw"] = "false", ["isSaveJoin"] = "false", ["isLoginRetain"] = "Y"
            });
            using var response = await client.PostAsync("https://login.sooplive.com/app/LoginAction.php", content);
            var text = await response.Content.ReadAsStringAsync();
            response.EnsureSuccessStatusCode();
            using var json = JsonDocument.Parse(text);
            if (!json.RootElement.TryGetProperty("RESULT", out var result) ||
                !((result.ValueKind == JsonValueKind.Number && result.TryGetInt32(out var numericResult) && numericResult == 1) ||
                  (result.ValueKind == JsonValueKind.String && result.GetString() == "1")))
                throw new InvalidOperationException("SOOP 로그인 응답이 실패했습니다.");
            SoopTestStatusText.Text = "✓ SOOP 로그인 성공: " + username;
            SoopTestStatusText.Foreground = Accent;
        }
        catch (Exception ex) { SoopTestStatusText.Text = "✕ " + ex.Message; SoopTestStatusText.Foreground = MakeBrush("#F08080"); }
    }

    async Task ExportSettingsFix36()
    {
        try
        {
            var choice = new ContentDialog
            {
                Title = "설정 내보내기",
                Content = "인증정보는 Windows DPAPI 암호문으로 저장됩니다. 다른 PC·Windows 사용자에게 공유할 파일은 인증정보 제외를 권장합니다.",
                PrimaryButtonText = "인증정보 제외",
                SecondaryButtonText = "DPAPI 암호문 포함",
                CloseButtonText = "취소",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
            };
            var result = await choice.ShowAsync();
            if (result == ContentDialogResult.None) return;

            var text = File.Exists(iniPath) ? await File.ReadAllTextAsync(iniPath, Encoding.UTF8) : "";
            if (result == ContentDialogResult.Primary)
            {
                text = Regex.Replace(
                    text,
                    @"^(SOOP_PASSWORD|CLOUDFLARE_API_KEY)=.*$",
                    "$1=",
                    RegexOptions.Multiline | RegexOptions.IgnoreCase);
            }

            var picker = new Windows.Storage.Pickers.FileSavePicker
            {
                SuggestedFileName = "SOOP_LIVE_SETTING_fix58",
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary
            };
            picker.FileTypeChoices.Add("INI 설정", new List<string> { ".ini" });
            InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
            var file = await picker.PickSaveFileAsync();
            if (file == null) return;
            await Windows.Storage.FileIO.WriteTextAsync(file, text);
            await ShowDialogAsync("설정 내보내기 완료", "저장 위치: " + file.Path);
        }
        catch (Exception ex) { await ShowDialogAsync("설정 내보내기 실패", ex.Message); }
    }

    async Task ImportSettingsFix36()
    {
        try
        {
            var picker = new Windows.Storage.Pickers.FileOpenPicker
            {
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary,
                ViewMode = Windows.Storage.Pickers.PickerViewMode.List
            };
            picker.FileTypeFilter.Add(".ini");
            InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
            var file = await picker.PickSingleFileAsync();
            if (file == null) return;

            var imported = IniService.Read(file.Path);
            if (imported.Count == 0 || !imported.ContainsKey("OUTPUT_DIR"))
                throw new InvalidDataException("SOOP LIVE 설정 파일로 확인할 수 없습니다.");

            var hasSecrets =
                (imported.TryGetValue("SOOP_PASSWORD", out var password) && !string.IsNullOrWhiteSpace(password)) ||
                (imported.TryGetValue("CLOUDFLARE_API_KEY", out var apiKey) && !string.IsNullOrWhiteSpace(apiKey));
            var dialog = new ContentDialog
            {
                Title = "설정 가져오기",
                Content = $"{file.Path}\n\n현재 설정을 이 파일로 교체할까요? 기존 설정은 .bak으로 보관됩니다." +
                          (hasSecrets ? "\n가져올 파일에 인증정보가 포함되어 있습니다." : "\n가져올 파일에 저장된 인증정보는 없습니다."),
                PrimaryButtonText = "가져오기",
                CloseButtonText = "취소",
                DefaultButton = ContentDialogButton.Close,
                XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;

            var text = SecretProtectionService.ProtectIniSecretsForCurrentUser(
                await Windows.Storage.FileIO.ReadTextAsync(file));
            BackupProtectedSettingsFix52();
            AtomicWriteAllText(iniPath, text, new UTF8Encoding(false));
            LoadStaticFiles();
            await ShowDialogAsync("설정 가져오기 완료", "설정을 불러왔습니다. Streamlink 경로 등 일부 항목은 Watcher 재시작 후 적용됩니다.");
        }
        catch (Exception ex) { await ShowDialogAsync("설정 가져오기 실패", ex.Message); }
    }

    async Task TestWorkerFix36()
    {
        WorkerTestStatusText.Text = "Worker 연결 확인 중…"; WorkerTestStatusText.Foreground = Muted;
        try
        {
            var rawUrl = CloudflareWorkerUrlBox.Text?.Trim() ?? "";
            if (!Uri.TryCreate(rawUrl, UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps)
                throw new InvalidOperationException("올바른 https Worker URL을 입력하세요.");
            var key = EffectiveSecretFix36("CLOUDFLARE_API_KEY", CloudflareApiKeyBox.Password, clearCloudflareApiKey);
            if (key.Length == 0) throw new InvalidOperationException("API Key가 없습니다.");
            var builder = new UriBuilder(uri) { Path = uri.AbsolutePath.EndsWith("/soop/url", StringComparison.OrdinalIgnoreCase) ? uri.AbsolutePath[..^9] + "/health" : uri.AbsolutePath.TrimEnd('/') + "/health", Query = "" };
            using var request = new HttpRequestMessage(HttpMethod.Get, builder.Uri);
            request.Headers.Add("X-API-Key", key);
            using var response = await SettingsTestClient.SendAsync(request);
            var body = await response.Content.ReadAsStringAsync();
            if (response.StatusCode == HttpStatusCode.Unauthorized) throw new InvalidOperationException("API Key 인증 실패(401)");
            response.EnsureSuccessStatusCode();
            using var json = JsonDocument.Parse(body);
            var colo = json.RootElement.TryGetProperty("colo", out var value) ? value.GetString() : null;
            WorkerTestStatusText.Text = "✓ Worker 연결·API Key 정상" + (string.IsNullOrWhiteSpace(colo) ? "" : $" · {colo}");
            WorkerTestStatusText.Foreground = Accent;
        }
        catch (Exception ex) { WorkerTestStatusText.Text = "✕ " + ex.Message; WorkerTestStatusText.Foreground = MakeBrush("#F08080"); }
    }

    async Task TestStreamlinkFix36()
    {
        StreamlinkTestStatusText.Text = "Streamlink 확인 중…"; StreamlinkTestStatusText.Foreground = Muted;
        try
        {
            var configured = StreamlinkPathBox.Text?.Trim() ?? "AUTO";
            string? executable = null;
            if (!configured.Equals("AUTO", StringComparison.OrdinalIgnoreCase) && File.Exists(configured)) executable = configured;
            if (executable == null && File.Exists(StreamlinkFallbackBox.Text?.Trim())) executable = StreamlinkFallbackBox.Text.Trim();
            if (executable == null)
            {
                using var where = Process.Start(new ProcessStartInfo("where.exe", "streamlink.exe") { UseShellExecute = false, RedirectStandardOutput = true, CreateNoWindow = true });
                var output = where == null ? "" : await where.StandardOutput.ReadToEndAsync();
                if (where != null) await where.WaitForExitAsync();
                executable = output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries).FirstOrDefault(File.Exists);
            }
            if (string.IsNullOrWhiteSpace(executable)) throw new FileNotFoundException("Streamlink 실행 파일을 찾지 못했습니다.");
            using var process = Process.Start(new ProcessStartInfo(executable, "--version") { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true });
            if (process == null) throw new InvalidOperationException("Streamlink를 실행하지 못했습니다.");
            var outputText = await process.StandardOutput.ReadToEndAsync();
            await process.WaitForExitAsync();
            if (process.ExitCode != 0) throw new InvalidOperationException("Streamlink 버전 확인 실패");
            StreamlinkTestStatusText.Text = $"✓ {outputText.Trim()} · {executable}";
            StreamlinkTestStatusText.Foreground = Accent;
        }
        catch (Exception ex) { StreamlinkTestStatusText.Text = "✕ " + ex.Message; StreamlinkTestStatusText.Foreground = MakeBrush("#F08080"); }
    }

    string? ValidateSettingsInputsFix36()
    {
        if (string.IsNullOrWhiteSpace(OutputDirBox.Text)) return "기본 저장 경로가 비어 있습니다.";
        if (!Uri.TryCreate(CloudflareWorkerUrlBox.Text?.Trim(), UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps) return "Cloudflare Worker URL은 올바른 https 주소여야 합니다.";
        if (EffectiveSecretFix36("CLOUDFLARE_API_KEY", CloudflareApiKeyBox.Password, clearCloudflareApiKey).Length == 0) return "Cloudflare API Key가 비어 있습니다.";

        foreach (var (box, name) in new (NumberBox Box, string Name)[]
        {
            (MinDiskBox, "최소 디스크 여유 공간"),
            (CheckIntervalBox, "방송 확인 주기"),
            (ChannelReloadIntervalBox, "채널 다시 읽기 주기"),
            (RecordRetryIntervalBox, "녹화 재시도 주기"),
            (RecordStallTimeoutBox, "녹화 정체 판정 시간"),
            (RecordMonitorIntervalBox, "녹화 상태 확인 주기"),
            (WorkerMaxRetryBox, "Worker 최대 재시도"),
            (LogRetentionDaysBox, "로그 보존 기간")
        })
        {
            if (double.IsNaN(box.Value))
                return $"{name}에 올바른 숫자를 입력하세요.";
            if (box.Value < box.Minimum || box.Value > box.Maximum)
                return $"{name}은(는) {box.Minimum:0.##}~{box.Maximum:0.##} 범위로 입력하세요.";
        }

        if (!double.IsNaN(RecordStallTimeoutBox.Value) && !double.IsNaN(RecordMonitorIntervalBox.Value) && RecordStallTimeoutBox.Value < RecordMonitorIntervalBox.Value * 2) return "Stall 판정 시간은 녹화 상태 확인 주기의 최소 2배 이상으로 설정하세요.";
        return null;
    }

    void ResetRecordingDefaultsFix36() { OutputDirBox.Text = @"C:\SOOP_LIVE"; QualityBox.SelectedItem = "최고 화질 (best)"; FileNamePatternBox.SelectedItem = "기본 — 260826_153000_BJ.ts"; MinDiskBox.Value = 20; }
    void ResetSoopDefaultsFix36() { SoopUsernameBox.Text = ""; SoopPurgeCredentialsCheck.IsChecked = true; StageSecretDeleteFix36(true); }
    void ResetWorkerDefaultsFix36() { MasterQualityBox.SelectedItem = "자동 (auto)"; }
    void ResetStreamlinkDefaultsFix36() { StreamlinkPathBox.Text = "AUTO"; StreamlinkFallbackBox.Text = @"C:\Program Files\Streamlink\bin\streamlink.exe"; }
    void ResetMonitoringDefaultsFix36() { CheckIntervalBox.Value = 30; ChannelReloadIntervalBox.Value = 2; RecordRetryIntervalBox.Value = 5; RecordStallTimeoutBox.Value = 90; RecordMonitorIntervalBox.Value = 5; WorkerMaxRetryBox.Value = 3; }
    void ResetLogDefaultsFix36() { LogEnabledCheck.IsChecked = true; LogDirBox.Text = @".\logs"; LogRetentionDaysBox.Value = 30; ConsoleAutoFormatCheck.IsChecked = true; ConsoleColorCheck.IsChecked = true; ConsoleShowPathCheck.IsChecked = false; NotifyRecordStartCheck.IsChecked = false; NotifyRecordFinishCheck.IsChecked = true; NotifyWarningCheck.IsChecked = true; }

    async Task<bool> ConfirmLeaveSettingsFix36Async()
    {
        var dialog = new ContentDialog
        {
            Title = "저장하지 않은 설정 변경",
            Content = "설정 변경 사항이 아직 저장되지 않았습니다.",
            PrimaryButtonText = "설정으로 돌아가기",
            SecondaryButtonText = "변경 버리고 이동",
            CloseButtonText = "취소",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };
        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Secondary) { LoadStaticFiles(); return true; }
        return false;
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

                return current.TryGetValue(key, out var oldValue)
                    ? SecretProtectionService.Unprotect(oldValue)
                    : "";
            }

            string ProtectedSecret(string key, string entered, bool clear) =>
                clear ? "" : SecretProtectionService.Protect(ExistingOrNewSecret(key, entered));

            string YesNo(CheckBox box) => box.IsChecked == true ? "Y" : "N";
            string Num(NumberBox box) =>
                box.Value.ToString("0.##", System.Globalization.CultureInfo.InvariantCulture);

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
                ["MIN_FREE_SPACE_GB"] = Num(MinDiskBox),

                ["SOOP_USERNAME"] = SoopUsernameBox.Text?.Trim() ?? "",
                ["SOOP_PASSWORD"] = ProtectedSecret("SOOP_PASSWORD", SoopPasswordBox.Password, clearSoopPassword),
                ["SOOP_PURGE_CREDENTIALS"] = YesNo(SoopPurgeCredentialsCheck),

                ["CLOUDFLARE_WORKER_URL"] = CloudflareWorkerUrlBox.Text?.Trim() ?? "",
                ["CLOUDFLARE_API_KEY"] = ProtectedSecret("CLOUDFLARE_API_KEY", CloudflareApiKeyBox.Password, clearCloudflareApiKey),
                ["MASTER_QUALITY"] = masterQuality,

                ["STREAMLINK_PATH"] = string.IsNullOrWhiteSpace(StreamlinkPathBox.Text) ? "AUTO" : StreamlinkPathBox.Text.Trim(),
                ["STREAMLINK_FALLBACK"] = StreamlinkFallbackBox.Text?.Trim() ?? "",

                ["CHECK_INTERVAL"] = Num(CheckIntervalBox),
                ["CHANNEL_RELOAD_INTERVAL"] = Num(ChannelReloadIntervalBox),
                ["RECORD_RETRY_INTERVAL"] = Num(RecordRetryIntervalBox),
                ["RECORD_STALL_TIMEOUT"] = Num(RecordStallTimeoutBox),
                ["RECORD_MONITOR_INTERVAL"] = Num(RecordMonitorIntervalBox),
                ["WORKER_MAX_RETRY"] = Num(WorkerMaxRetryBox),

                ["LOG_ENABLED"] = YesNo(LogEnabledCheck),
                ["LOG_DIR"] = LogDirBox.Text?.Trim() ?? @".\logs",
                ["LOG_RETENTION_DAYS"] = Num(LogRetentionDaysBox),

                ["CONSOLE_AUTO_FORMAT"] = YesNo(ConsoleAutoFormatCheck),
                ["CONSOLE_COLOR"] = YesNo(ConsoleColorCheck),
                ["CONSOLE_SHOW_PATH"] = YesNo(ConsoleShowPathCheck),
                ["GUI_NOTIFY_RECORD_START"] = YesNo(NotifyRecordStartCheck),
                ["GUI_NOTIFY_RECORD_FINISH"] = YesNo(NotifyRecordFinishCheck),
                ["GUI_NOTIFY_WARNING"] = YesNo(NotifyWarningCheck)
            };

            BackupProtectedSettingsFix52();
            UpdateIniFile(iniPath, updates);

            ApplyConsoleDisplayOptions(
                ConsoleAutoFormatCheck.IsChecked == true,
                ConsoleColorCheck.IsChecked == true,
                ConsoleShowPathCheck.IsChecked == true);
            uiNotifyRecordStart = NotifyRecordStartCheck.IsChecked == true;
            uiNotifyRecordFinish = NotifyRecordFinishCheck.IsChecked == true;
            uiNotifyWarning = NotifyWarningCheck.IsChecked == true;

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
                "• 인증정보: Windows DPAPI(CurrentUser)로 보호 후 재인증");
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("설정 저장 실패", ex.Message);
        }
    }

    void BackupProtectedSettingsFix52()
    {
        if (!File.Exists(iniPath)) return;
        var backupPath = iniPath + ".bak";
        var protectedText = SecretProtectionService.ProtectIniSecretsForCurrentUser(
            File.ReadAllText(iniPath, Encoding.UTF8));
        AtomicWriteAllText(backupPath, protectedText, new UTF8Encoding(false));
    }

}
