using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Text;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    const int MaxRecentRecordings = 200;
    public ObservableCollection<RecentRecordingEntry> RecentRecordingItems { get; } = new();
    FrameworkElement RecentRecordingsView = null!;
    ListView RecentRecordingsList = null!;
    AppBarButton RecentOpenFolderButton = null!;
    AppBarButton RecentSelectFileButton = null!;
    AppBarButton RecentCopyPathButton = null!;
    Button AlertRetryButton = null!;
    Button AlertFolderButton = null!;
    RecentRecordingStore? recentRecordingStore;
    DataTemplate? recentWideTemplate;
    DataTemplate? recentCompactTemplate;

    string RecentRecordingsPath => Path.Combine(backendDir, "history", "recent-recordings.json");
    RecentRecordingStore RecentStore => recentRecordingStore ??= CreateRecentStoreFix59();

    RecentRecordingStore CreateRecentStoreFix59()
    {
        var store = new RecentRecordingStore(RecentRecordingsPath, MaxRecentRecordings);
        store.SaveFailed += ex => DispatcherQueue.TryEnqueue(() =>
            AppendLog("[WARN] 최근 녹화 내역 저장 실패: " + ex.Message));
        return store;
    }

    static DataTemplate BuildAlertFlyoutTemplateFix51()
    {
        const string xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="#20252C" CornerRadius="7" Padding="10" Margin="0,0,0,6">
    <StackPanel Spacing="3">
      <Grid ColumnSpacing="8">
        <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
        <StackPanel Orientation="Horizontal" Spacing="8">
          <TextBlock Text="{Binding Time}" Foreground="#8E99A8"/>
          <TextBlock Text="{Binding Name}" Foreground="White" FontWeight="SemiBold"/>
        </StackPanel>
        <TextBlock Grid.Column="1" Text="{Binding Status}" Foreground="{Binding StatusBrush}"/>
      </Grid>
      <TextBlock Text="{Binding Account}" Foreground="#8E99A8" FontSize="11"/>
      <TextBlock Text="{Binding Detail}" Foreground="#D7DEE8" TextWrapping="Wrap"/>
      <TextBlock Text="{Binding Title}" Foreground="#8E99A8" FontSize="11" TextWrapping="Wrap"/>
    </StackPanel>
  </Border>
</DataTemplate>
""";
        return (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }

    FrameworkElement BuildRecentRecordingsViewFix51()
    {
        var root = new Grid
        {
            Padding = DesignTokens.PagePadding,
            Visibility = Visibility.Collapsed,
            RequestedTheme = ElementTheme.Dark
        };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var header = new Grid { Margin = new Thickness(0, 0, 0, 12), ColumnSpacing = 8 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var title = new StackPanel();
        title.Children.Add(new TextBlock
        {
            Text = "최근 녹화",
            Foreground = White,
            FontSize = 22,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        title.Children.Add(new TextBlock
        {
            Text = "최근 200건의 완료·중단 내역입니다. 기록 삭제는 실제 녹화 파일을 삭제하지 않습니다.",
            Foreground = Muted,
            Margin = new Thickness(0, 4, 0, 0)
        });
        header.Children.Add(title);

        var actions = new CommandBar
        {
            Background = DesignTokens.Surface,
            DefaultLabelPosition = CommandBarDefaultLabelPosition.Right,
            IsDynamicOverflowEnabled = false
        };
        RecentOpenFolderButton = DesignTokens.Command("폴더 열기", Symbol.Folder, enabled: false);
        RecentSelectFileButton = DesignTokens.Command("파일 선택", Symbol.OpenFile, enabled: false);
        RecentCopyPathButton = DesignTokens.Command("경로 복사", Symbol.Copy, enabled: false);
        var clear = DesignTokens.Command("내역 지우기", Symbol.Delete);
        RecentOpenFolderButton.Click += async (_, _) => await OpenRecentRecordingAsync(selectFile: false);
        RecentSelectFileButton.Click += async (_, _) => await OpenRecentRecordingAsync(selectFile: true);
        RecentCopyPathButton.Click += async (_, _) => await CopyRecentRecordingPathAsync();
        clear.Click += ClearRecentRecordings_ClickFix51;
        actions.PrimaryCommands.Add(RecentOpenFolderButton);
        actions.PrimaryCommands.Add(RecentSelectFileButton);
        actions.SecondaryCommands.Add(RecentCopyPathButton);
        actions.SecondaryCommands.Add(clear);
        Grid.SetColumn(actions, 1);
        header.Children.Add(actions);

        RecentRecordingsList = new ListView
        {
            ItemsSource = RecentRecordingItems,
            SelectionMode = ListViewSelectionMode.Single,
            ItemTemplate = BuildRecentRecordingTemplateFix51(compact: false)
        };
        RecentRecordingsList.SelectionChanged += (_, _) => UpdateRecentRecordingActionsFix51();
        var recentCompactLayout = false;
        root.SizeChanged += (_, args) =>
        {
            var compact = args.NewSize.Width < DesignTokens.CompactRecentWidth;
            if (compact == recentCompactLayout) return;
            recentCompactLayout = compact;
            RecentRecordingsList.ItemTemplate = BuildRecentRecordingTemplateFix51(compact);
        };
        Grid.SetRow(RecentRecordingsList, 1);
        root.Children.Add(header);
        root.Children.Add(RecentRecordingsList);
        return root;
    }

    DataTemplate BuildRecentRecordingTemplateFix51(bool compact)
    {
        var cached = compact ? recentCompactTemplate : recentWideTemplate;
        if (cached != null) return cached;

        if (compact)
        {
            const string compactXaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="{ThemeResource CardBackgroundFillColorDefaultBrush}" BorderBrush="{ThemeResource CardStrokeColorDefaultBrush}" BorderThickness="1" CornerRadius="10" Padding="12" Margin="0,0,0,7">
    <Grid RowSpacing="5">
      <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="Auto"/><RowDefinition Height="Auto"/></Grid.RowDefinitions>
      <Grid ColumnSpacing="8">
        <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
        <StackPanel Orientation="Horizontal" Spacing="8">
          <TextBlock Text="{Binding Name}" Foreground="{ThemeResource TextFillColorPrimaryBrush}" FontWeight="SemiBold"/>
          <TextBlock Text="{Binding Account}" Foreground="{ThemeResource TextFillColorSecondaryBrush}" FontSize="11"/>
        </StackPanel>
        <TextBlock Grid.Column="1" Text="{Binding EndedAtText}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
      </Grid>
      <StackPanel Grid.Row="1">
        <TextBlock Text="{Binding Title}" Foreground="{ThemeResource TextFillColorPrimaryBrush}" TextTrimming="CharacterEllipsis"/>
        <TextBlock Text="{Binding FileName}" Foreground="{ThemeResource TextFillColorSecondaryBrush}" FontSize="11" TextTrimming="CharacterEllipsis"/>
      </StackPanel>
      <StackPanel Grid.Row="2" Orientation="Horizontal" Spacing="10">
        <TextBlock Text="{Binding Duration}"/><TextBlock Text="{Binding Size}"/>
        <TextBlock Text="{Binding Reason}" Foreground="{ThemeResource SystemFillColorCautionBrush}" FontWeight="SemiBold"/>
      </StackPanel>
    </Grid>
  </Border>
</DataTemplate>
""";
            return recentCompactTemplate =
                (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(compactXaml);
        }

        const string xaml = """
<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
  <Border Background="{ThemeResource CardBackgroundFillColorDefaultBrush}" BorderBrush="{ThemeResource CardStrokeColorDefaultBrush}" BorderThickness="1" CornerRadius="10" Padding="12" Margin="0,0,0,7">
    <Grid ColumnSpacing="12">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="150"/><ColumnDefinition Width="170"/>
        <ColumnDefinition Width="*"/><ColumnDefinition Width="100"/>
        <ColumnDefinition Width="110"/>
      </Grid.ColumnDefinitions>
      <TextBlock Grid.Column="0" Text="{Binding EndedAtText}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
      <StackPanel Grid.Column="1">
        <TextBlock Text="{Binding Name}" Foreground="{ThemeResource TextFillColorPrimaryBrush}" FontWeight="SemiBold"/>
        <TextBlock Text="{Binding Account}" Foreground="{ThemeResource TextFillColorSecondaryBrush}" FontSize="11"/>
      </StackPanel>
      <StackPanel Grid.Column="2">
        <TextBlock Text="{Binding Title}" Foreground="{ThemeResource TextFillColorPrimaryBrush}" TextTrimming="CharacterEllipsis"/>
        <TextBlock Text="{Binding FileName}" Foreground="{ThemeResource TextFillColorSecondaryBrush}" FontSize="11" TextTrimming="CharacterEllipsis"/>
      </StackPanel>
      <StackPanel Grid.Column="3">
        <TextBlock Text="{Binding Duration}" Foreground="#D7DEE8"/>
        <TextBlock Text="{Binding Size}" Foreground="#8E99A8" FontSize="11"/>
      </StackPanel>
      <StackPanel Grid.Column="4">
        <TextBlock Text="{Binding Reason}" Foreground="{ThemeResource SystemFillColorCautionBrush}" TextTrimming="CharacterEllipsis"/>
        <TextBlock Text="{Binding StateText}" Foreground="#8E99A8" FontSize="11"/>
      </StackPanel>
    </Grid>
  </Border>
</DataTemplate>
""";
        return recentWideTemplate =
            (DataTemplate)Microsoft.UI.Xaml.Markup.XamlReader.Load(xaml);
    }

    void LoadRecentRecordingsFix51()
    {
        RecentRecordingItems.Clear();
        try
        {
            var migratedReason = false;
            foreach (var item in RecentStore.Load())
            {
                var normalized = BackendEventParser.NormalizeRecordingFinishedReason(item.Reason);
                if (!string.Equals(item.Reason, normalized, StringComparison.Ordinal))
                {
                    item.Reason = normalized;
                    migratedReason = true;
                }
                RecentRecordingItems.Add(item);
            }
            if (migratedReason)
                SaveRecentRecordingsFix51();
        }
        catch (Exception ex)
        {
            AppendLog("[WARN] 최근 녹화 내역 읽기 실패: " + ex.Message);
        }
    }

    void AddRecentRecordingFix51(
        string account, string name, string title, string duration,
        string size, string reason, string file)
    {
        if (string.IsNullOrWhiteSpace(file)) return;
        RecentRecordingItems.Insert(0, new RecentRecordingEntry
        {
            EndedAt = DateTime.UtcNow,
            Account = account,
            Name = name,
            Title = title,
            Duration = duration,
            Size = size,
            Reason = reason,
            File = file
        });
        while (RecentRecordingItems.Count > MaxRecentRecordings)
            RecentRecordingItems.RemoveAt(RecentRecordingItems.Count - 1);
        SaveRecentRecordingsFix51();
    }

    void SaveRecentRecordingsFix51()
    {
        RecentStore.Save(RecentRecordingItems.ToArray());
    }

    async void ClearRecentRecordings_ClickFix51(object sender, RoutedEventArgs e)
    {
        var dialog = new ContentDialog
        {
            Title = "최근 녹화 내역 지우기",
            Content = "최근 녹화 기록만 지웁니다. 실제 녹화 파일은 삭제되지 않습니다.",
            PrimaryButtonText = "내역 지우기",
            CloseButtonText = "취소",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        RecentRecordingItems.Clear();
        SaveRecentRecordingsFix51();
        UpdateRecentRecordingActionsFix51();
    }

    void UpdateRecentRecordingActionsFix51()
    {
        var selected = RecentRecordingsList?.SelectedItem as RecentRecordingEntry;
        var hasPath = selected != null && !string.IsNullOrWhiteSpace(selected.File);
        RecentOpenFolderButton.IsEnabled = hasPath;
        RecentSelectFileButton.IsEnabled = hasPath && File.Exists(selected!.File);
        RecentCopyPathButton.IsEnabled = hasPath;
    }

    async Task OpenRecentRecordingAsync(bool selectFile)
    {
        if (RecentRecordingsList.SelectedItem is not RecentRecordingEntry selected) return;
        try
        {
            var fullPath = Path.GetFullPath(selected.File);
            var directory = Path.GetDirectoryName(fullPath);
            if (string.IsNullOrWhiteSpace(directory) || !Directory.Exists(directory))
                throw new DirectoryNotFoundException("녹화 폴더를 찾을 수 없습니다.\n" + directory);
            var info = new ProcessStartInfo("explorer.exe") { UseShellExecute = true };
            if (selectFile && File.Exists(fullPath))
            {
                info.ArgumentList.Add("/select," + fullPath);
            }
            else info.ArgumentList.Add(directory);
            Process.Start(info);
        }
        catch (Exception ex) { await ShowDialogAsync("최근 녹화 위치 열기 실패", ex.Message); }
    }

    async Task CopyRecentRecordingPathAsync()
    {
        if (RecentRecordingsList.SelectedItem is not RecentRecordingEntry selected) return;
        await CopyTextToClipboardFix51(selected.File, "최근 녹화 경로");
    }

    async Task CopyDiagnosticInfoFix51()
    {
        try
        {
            var text = DiagnosticInfoService.CreateReport(
                "1.2.0-preview1-fix75",
                backend.IsRunning,
                RecordingItems.Count,
                OfflineItems.Count,
                StoppedItems.Count,
                AlertItems.Count,
                File.Exists(Path.Combine(backendDir, "SOOP_LIVE.ps1")),
                File.Exists(iniPath),
                File.Exists(channelPath),
                logLines);
            if (await CopyTextToClipboardFix51(text, "진단 정보"))
                AppendLog("[GUI] 진단 정보를 클립보드에 복사했습니다.");
        }
        catch (Exception ex) { await ShowDialogAsync("진단 정보 복사 실패", ex.Message); }
    }

    async Task<bool> CopyTextToClipboardFix51(string text, string label)
    {
        try
        {
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(text);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            Windows.ApplicationModel.DataTransfer.Clipboard.Flush();
            return true;
        }
        catch (Exception ex)
        {
            await ShowDialogAsync(label + " 복사 실패", ex.Message);
            return false;
        }
    }

    void NavigateToViewFix51(string tag)
    {
        if (AlertSummaryButton?.Flyout is Flyout flyout) flyout.Hide();
        var target = Nav.MenuItems.OfType<NavigationViewItem>()
            .FirstOrDefault(x => string.Equals(x.Tag as string, tag, StringComparison.Ordinal));
        if (target != null) Nav.SelectedItem = target;
    }

    void UpdateAlertActionsFix51()
    {
        var selected = AlertFlyoutList?.SelectedItem as ChannelStatus;
        AlertRetryButton.IsEnabled = selected != null && !string.IsNullOrWhiteSpace(selected.Account);
        AlertFolderButton.IsEnabled = selected != null &&
            statusMap.TryGetValue(DashboardKey(selected.Account, selected.Name), out var status) &&
            !string.IsNullOrWhiteSpace(status.FilePath);
    }

    async void RetrySelectedAlertFix51(object sender, RoutedEventArgs e)
    {
        if (AlertFlyoutList.SelectedItem is not ChannelStatus selected) return;
        if (!backend.IsRunning)
        {
            await ShowDialogAsync("즉시 다시 확인", "Watcher가 중지되어 있습니다. Watcher를 먼저 시작해 주세요.");
            return;
        }
        try
        {
            SendChannelControlCommand("RECHECK", selected.Account);
            selected.Detail = "즉시 상태 확인을 요청했습니다. 정상 녹화가 확인되면 자동으로 해제됩니다.";
        }
        catch (Exception ex) { await ShowDialogAsync("즉시 다시 확인 실패", ex.Message); }
    }

    async void RefreshChannelNamesPreviewFix51_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button button || !await EnsureRawChannelEditsAppliedAsync()) return;
        var selected = GetSelectedChannels();
        var targets = (selected.Count > 0 ? selected : ChannelItems.ToList())
            .Where(x => !string.IsNullOrWhiteSpace(x.Account))
            .ToList();
        if (targets.Count == 0)
        {
            await ShowDialogAsync("채널명 일괄 확인", "확인할 채널이 없습니다.");
            return;
        }

        button.IsEnabled = false;
        var originalContent = button.Content;
        button.Content = $"이름 확인 중 0/{targets.Count}";
        var results = new List<(EditableChannel Channel, SoopProfileLookup Lookup)>();
        try
        {
            for (var index = 0; index < targets.Count; index += 4)
            {
                var batch = targets.Skip(index).Take(4).ToList();
                var lookups = await Task.WhenAll(batch.Select(async channel =>
                    (Channel: channel, Lookup: await LookupSoopProfileAsync(channel.Account))));
                results.AddRange(lookups);
                button.Content = $"이름 확인 중 {Math.Min(index + batch.Count, targets.Count)}/{targets.Count}";
            }
        }
        finally
        {
            button.Content = originalContent;
            button.IsEnabled = true;
        }

        var changed = results
            .Where(x => x.Lookup.Exists &&
                !string.Equals(x.Channel.Name, x.Lookup.Name, StringComparison.CurrentCulture))
            .ToList();
        var failed = results.Where(x => !x.Lookup.Exists).ToList();
        var preview = new StringBuilder();
        preview.AppendLine($"확인 {results.Count}개 · 변경 {changed.Count}개 · 실패 {failed.Count}개");
        preview.AppendLine();
        foreach (var entry in changed.Take(100))
            preview.AppendLine($"• {entry.Channel.Account}: {entry.Channel.Name} → {entry.Lookup.Name}");
        if (changed.Count > 100) preview.AppendLine($"• 외 {changed.Count - 100}개 변경");
        if (failed.Count > 0)
        {
            preview.AppendLine().AppendLine("조회 실패:");
            foreach (var entry in failed.Take(20))
                preview.AppendLine($"• {entry.Channel.Account}: {entry.Lookup.Error ?? "채널 확인 실패"}");
            if (failed.Count > 20) preview.AppendLine($"• 외 {failed.Count - 20}개 실패");
        }

        var previewBox = new TextBox
        {
            Text = preview.ToString().TrimEnd(),
            IsReadOnly = true,
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinWidth = 560,
            MaxHeight = 430,
            FontFamily = new FontFamily("Consolas")
        };
        var dialog = new ContentDialog
        {
            Title = "채널명 일괄 새로고침 미리보기",
            Content = previewBox,
            PrimaryButtonText = "변경 사항 반영",
            SecondaryButtonText = "확인",
            CloseButtonText = "취소",
            IsPrimaryButtonEnabled = changed.Count > 0,
            DefaultButton = changed.Count > 0 ? ContentDialogButton.Primary : ContentDialogButton.Secondary,
            XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
        };
        var result = await dialog.ShowAsync();
        if (result != ContentDialogResult.Primary) return;

        foreach (var entry in changed)
            entry.Channel.Name = entry.Lookup.Name;
        SyncRawChannelTextFromItems(markDirty: true);
        RefreshChannelFilter();
        await ShowDialogAsync(
            "채널명 변경 반영",
            $"{changed.Count}개의 이름을 목록에 반영했습니다.\n\n파일과 실행 중 Watcher에 적용하려면 '변경 저장'을 눌러 주세요.");
    }

    async void OpenSelectedAlertFolderFix51(object sender, RoutedEventArgs e)
    {
        if (AlertFlyoutList.SelectedItem is not ChannelStatus selected) return;
        if (statusMap.TryGetValue(DashboardKey(selected.Account, selected.Name), out var status))
            await OpenRecordingLocationAsync(status, selectFile: false);
    }
}
