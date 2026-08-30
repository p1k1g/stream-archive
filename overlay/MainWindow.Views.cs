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
    DataTemplate? channelWideTemplate;
    DataTemplate? channelCompactTemplate;

    FrameworkElement BuildDashboard()
    {
        var scroll = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto
        };

        var stack = new StackPanel
        {
            Padding = DesignTokens.PagePadding,
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
            SelectionMode = ListViewSelectionMode.Single,
            MinWidth = 410,
            MaxHeight = 420,
            ItemTemplate = BuildAlertFlyoutTemplateFix51()
        };
        AlertFlyoutList.SelectionChanged += (_, _) => UpdateAlertActionsFix51();
        var alertPanel = new StackPanel { Spacing = 8, MinWidth = 430 };
        alertPanel.Children.Add(new TextBlock
        {
            Text = "확인이 필요한 채널",
            FontSize = 16,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        alertPanel.Children.Add(AlertFlyoutList);
        var alertActions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        AlertRetryButton = ApplyButtonMetricsFix39(new Button { Content = "즉시 다시 확인", IsEnabled = false }, 120);
        AlertFolderButton = ApplyButtonMetricsFix39(new Button { Content = "녹화 폴더", IsEnabled = false }, 104);
        var alertSettings = ApplyButtonMetricsFix39(new Button { Content = "설정" });
        var alertLogs = ApplyButtonMetricsFix39(new Button { Content = "로그" });
        AlertRetryButton.Click += RetrySelectedAlertFix51;
        AlertFolderButton.Click += OpenSelectedAlertFolderFix51;
        alertSettings.Click += (_, _) => NavigateToViewFix51("settings");
        alertLogs.Click += (_, _) => NavigateToViewFix51("logs");
        alertActions.Children.Add(AlertRetryButton);
        alertActions.Children.Add(AlertFolderButton);
        alertActions.Children.Add(alertSettings);
        alertActions.Children.Add(alertLogs);
        alertPanel.Children.Add(alertActions);
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

        OpenSelectedRecordingFolderButton = ApplyButtonMetricsFix39(new Button
        {
            Content = "폴더 열기",
            IsEnabled = false
        }, 112);
        ToolTipService.SetToolTip(
            OpenSelectedRecordingFolderButton,
            "선택한 채널이 현재 녹화 중인 폴더를 엽니다.");
        OpenSelectedRecordingFolderButton.Click += OpenSelectedRecordingFolder_Click;

        var recordingActions = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8
        };
        recordingActions.Children.Add(OpenSelectedRecordingFolderButton);
        recordingActions.Children.Add(StopSelectedRecordingButton);
        Grid.SetColumn(recordingActions, 1);

        recordingHeader.Children.Add(recordingTitle);
        recordingHeader.Children.Add(recordingActions);
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
        RecordingList.ContainerContentChanging += RecordingList_ContainerContentChanging;
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
            AlertFlyoutList.ItemTemplate = BuildAlertFlyoutTemplateFix51();

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
            Padding = DesignTokens.PagePadding,
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

        AddChannelButton = DesignTokens.Command("채널 추가", Symbol.Add);
        ImportChannelsButton = DesignTokens.Command("채널 가져오기", Symbol.Import);
        var refreshChannelNamesButton = DesignTokens.Command("채널명 일괄 확인", Symbol.Refresh);
        EditChannelButton = DesignTokens.Command("수정", Symbol.Edit);
        SelectedChannelActionsButton = DesignTokens.Command("선택 작업", Symbol.More, enabled: false);
        ApplyRawChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "목록에 반영" }, 110);
        ReloadChannelsButton = ApplyButtonMetricsFix39(new Button { Content = "저장본 다시 읽기" }, 132);
        SaveChannelsButton = DesignTokens.Command("변경 저장", Symbol.Save, enabled: false);

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
        refreshChannelNamesButton.Click += RefreshChannelNamesPreviewFix51_Click;
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

        var commandBar = new CommandBar
        {
            Margin = new Thickness(0, DesignTokens.SpaceMd, 0, 0),
            Background = DesignTokens.Surface,
            DefaultLabelPosition = CommandBarDefaultLabelPosition.Right,
            IsDynamicOverflowEnabled = false,
            HorizontalAlignment = HorizontalAlignment.Stretch
        };
        commandBar.PrimaryCommands.Add(AddChannelButton);
        commandBar.PrimaryCommands.Add(SaveChannelsButton);
        commandBar.PrimaryCommands.Add(SelectedChannelActionsButton);
        commandBar.SecondaryCommands.Add(ImportChannelsButton);
        commandBar.SecondaryCommands.Add(refreshChannelNamesButton);
        commandBar.SecondaryCommands.Add(EditChannelButton);
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
        ChannelList.ItemTemplate = BuildChannelTemplate(compact: false);
        ChannelList.SelectionChanged += ChannelList_SelectionChanged;
        var channelCompactLayout = false;
        root.SizeChanged += (_, args) =>
        {
            var compact = args.NewSize.Width < DesignTokens.CompactChannelWidth;
            if (compact == channelCompactLayout) return;
            channelCompactLayout = compact;
            ChannelList.ItemTemplate = BuildChannelTemplate(compact);
        };

        DesignTokens.AddAccelerator(AddChannelButton, Windows.System.VirtualKey.N, Windows.System.VirtualKeyModifiers.Control);
        DesignTokens.AddAccelerator(SaveChannelsButton, Windows.System.VirtualKey.S, Windows.System.VirtualKeyModifiers.Control);
        DesignTokens.AddAccelerator(
            root,
            Windows.System.VirtualKey.F,
            Windows.System.VirtualKeyModifiers.Control,
            () => ChannelSearchBox.Focus(FocusState.Keyboard));

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

    DataTemplate BuildChannelTemplate(bool compact)
    {
        var cached = compact ? channelCompactTemplate : channelWideTemplate;
        if (cached != null) return cached;

        if (compact)
        {
            const string compactXaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="{Binding CardBackground}" BorderBrush="{Binding CardBorder}" BorderThickness="1" CornerRadius="10" Padding="12" Margin="0,0,0,7" Opacity="{Binding RowOpacity}">
    <Grid ColumnSpacing="10" RowSpacing="5">
      <Grid.ColumnDefinitions><ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/></Grid.ColumnDefinitions>
      <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="Auto"/></Grid.RowDefinitions>
      <Border Grid.RowSpan="2" Background="{Binding BadgeBackground}" CornerRadius="999" Padding="8,3" VerticalAlignment="Top">
        <TextBlock Text="{Binding EnabledText}" Foreground="{Binding StateForeground}" FontWeight="SemiBold" AutomationProperties.Name="{Binding EnabledText}"/>
      </Border>
      <StackPanel Grid.Column="1" Orientation="Horizontal" Spacing="8">
        <TextBlock Text="{Binding Name}" Foreground="{Binding NameForeground}" FontWeight="SemiBold" TextTrimming="CharacterEllipsis"/>
        <TextBlock Text="{Binding Account}" Foreground="#A7B0BE" TextTrimming="CharacterEllipsis"/>
      </StackPanel>
      <TextBlock Grid.Row="1" Grid.Column="1" Text="{Binding OutputDisplay}" Foreground="#9AA5B4" FontSize="11" TextTrimming="CharacterEllipsis"/>
    </Grid>
  </Border>
</DataTemplate>
""";
            return channelCompactTemplate =
                (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(compactXaml);
        }

        const string xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="{Binding CardBackground}" BorderBrush="{Binding CardBorder}" BorderThickness="1" CornerRadius="7" Padding="10" Margin="0,0,0,5" Opacity="{Binding RowOpacity}">
    <Grid ColumnSpacing="12">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="80"/>
        <ColumnDefinition Width="160"/>
        <ColumnDefinition Width="220"/>
        <ColumnDefinition Width="*"/>
      </Grid.ColumnDefinitions>
      <Border Grid.Column="0" Background="{Binding BadgeBackground}" CornerRadius="10" Padding="8,3" HorizontalAlignment="Left">
        <TextBlock Text="{Binding EnabledText}" Foreground="{Binding StateForeground}" FontWeight="SemiBold" AutomationProperties.Name="{Binding EnabledText}"/>
      </Border>
      <TextBlock Grid.Column="1" Text="{Binding Name}" Foreground="{Binding NameForeground}" FontWeight="SemiBold"/>
      <TextBlock Grid.Column="2" Text="{Binding Account}" Foreground="#D7DEE8"/>
      <TextBlock Grid.Column="3" Text="{Binding OutputDisplay}" Foreground="#9AA5B4" TextTrimming="CharacterEllipsis"/>
    </Grid>
  </Border>
</DataTemplate>
""";
        return channelWideTemplate =
            (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }

    FrameworkElement BuildLogsView()
    {
        var root = new Grid
        {
            Padding = DesignTokens.PagePadding,
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
        var copyDiagnostic = ApplyButtonMetricsFix39(new Button { Content = "진단 정보 복사" }, 126);
        clear.Click += ClearLog_Click;
        open.Click += OpenBackend_Click;
        copyDiagnostic.Click += async (_, _) => await CopyDiagnosticInfoFix51();
        buttons.Children.Add(clear);
        buttons.Children.Add(open);
        buttons.Children.Add(copyDiagnostic);

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
}
