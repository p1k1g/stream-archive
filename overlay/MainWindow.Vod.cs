using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System.Text;
using System.Text.Json;
using WinRT.Interop;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    readonly VodProcessService vodBackend = new();
    TextBox VodUrlBox = null!;
    TextBox VodOutputBox = null!;
    TextBox VodPartsBox = null!;
    TextBox VodYtDlpPathBox = null!;
    TextBox VodFfmpegPathBox = null!;
    ComboBox VodCookieModeBox = null!;
    TextBox VodCookieSourceBox = null!;
    TextBlock VodLoginStatusText = null!;
    TextBlock VodStatusText = null!;
    ProgressBar VodProgress = null!;
    AppBarButton VodStartButton = null!;
    AppBarButton VodCancelButton = null!;
    VodSettings vodSettings = VodSettingsStore.Load();
    readonly VodHistoryStore vodHistory = new();
    string? activeVodJobDirectory;
    string activeVodJobId = "";
    string activeVodTitle = "";
    string activeVodStreamer = "";
    string activeVodOutputFile = "";
    string activeVodFailureDetail = "";

    FrameworkElement BuildVodView()
    {
        var root = new Grid { Padding = DesignTokens.PagePadding };
        var stack = new StackPanel { Spacing = DesignTokens.SpaceLg, MaxWidth = 980, HorizontalAlignment = HorizontalAlignment.Stretch };
        stack.Children.Add(new TextBlock { Text = "VOD 다운로드", FontSize = 26, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold });
        stack.Children.Add(new TextBlock { Text = "사용 권한이 있는 SOOP VOD를 분석하고 선택한 PART를 다운로드합니다. LIVE Watcher와 별도로 실행됩니다.", Foreground = Muted, TextWrapping = TextWrapping.Wrap });

        VodUrlBox = new TextBox { Header = "VOD URL", PlaceholderText = "https://vod.sooplive.com/player/204952073" };
        VodOutputBox = new TextBox { Header = "출력 폴더", Text = vodSettings.OutputDirectory, PlaceholderText = @"C:\Videos" };
        VodPartsBox = new TextBox { Header = "PART 선택", PlaceholderText = "비워 두면 전체 · 예: 1-5,8,10-12" };
        VodYtDlpPathBox = new TextBox
        {
            Header = "yt-dlp 경로 (선택)",
            Text = vodSettings.YtDlpPath,
            PlaceholderText = @"예: C:\Tools\yt-dlp.exe · 비워 두면 자동 검색"
        };
        VodFfmpegPathBox = new TextBox
        {
            Header = "ffmpeg 경로 (선택)",
            Text = vodSettings.FfmpegPath,
            PlaceholderText = @"예: C:\Tools\ffmpeg.exe · 비워 두면 자동 검색"
        };
        VodCookieModeBox = new ComboBox { Header = "Cookie 방식", HorizontalAlignment = HorizontalAlignment.Stretch };
        VodCookieModeBox.Items.Add(new ComboBoxItem { Content = "저장된 SOOP 로그인 (권장)", Tag = "SOOP_LOGIN" });
        VodCookieModeBox.Items.Add(new ComboBoxItem { Content = "Cookie 파일", Tag = "FILE" });
        VodCookieModeBox.Items.Add(new ComboBoxItem { Content = "브라우저 Cookie", Tag = "BROWSER" });
        VodCookieModeBox.SelectedItem = VodCookieModeBox.Items.OfType<ComboBoxItem>()
            .First(item => string.Equals(item.Tag as string, vodSettings.CookieMode, StringComparison.Ordinal));
        VodCookieSourceBox = new TextBox
        {
            Header = "Cookie 파일 경로 또는 브라우저 이름",
            Text = vodSettings.CookieMode == "BROWSER" ? vodSettings.BrowserName : vodSettings.CookieFile,
            PlaceholderText = "FILE: cookies.txt 전체 경로 · BROWSER: firefox 또는 chrome"
        };
        VodLoginStatusText = new TextBlock
        {
            Text = "저장된 SOOP 로그인은 설정 탭의 DPAPI 보호 자격증명을 사용합니다.",
            Foreground = Muted,
            TextWrapping = TextWrapping.Wrap
        };
        VodCookieModeBox.SelectionChanged += (_, _) =>
        {
            var mode = SelectedVodCookieMode();
            VodCookieSourceBox.Visibility = mode == "SOOP_LOGIN" ? Visibility.Collapsed : Visibility.Visible;
            VodLoginStatusText.Visibility = mode == "SOOP_LOGIN" ? Visibility.Visible : Visibility.Collapsed;
            VodCookieSourceBox.Text = mode == "BROWSER" ? vodSettings.BrowserName : vodSettings.CookieFile;
        };
        stack.Children.Add(VodUrlBox);
        stack.Children.Add(VodOutputBox);
        stack.Children.Add(VodPartsBox);
        stack.Children.Add(BuildVodExecutablePicker(VodYtDlpPathBox, "yt-dlp.exe 선택", "yt-dlp.exe"));
        stack.Children.Add(BuildVodExecutablePicker(VodFfmpegPathBox, "ffmpeg.exe 선택", "ffmpeg.exe"));
        stack.Children.Add(VodCookieModeBox);
        stack.Children.Add(VodCookieSourceBox);
        stack.Children.Add(VodLoginStatusText);
        VodCookieSourceBox.Visibility = vodSettings.CookieMode == "SOOP_LOGIN" ? Visibility.Collapsed : Visibility.Visible;
        VodLoginStatusText.Visibility = vodSettings.CookieMode == "SOOP_LOGIN" ? Visibility.Visible : Visibility.Collapsed;

        VodStartButton = DesignTokens.Command("분석 및 다운로드", Symbol.Download);
        VodCancelButton = DesignTokens.Command("취소", Symbol.Cancel, enabled: false);
        VodStartButton.Click += VodStartButton_Click;
        VodCancelButton.Click += VodCancelButton_Click;
        AutomationProperties.SetName(VodStartButton, "VOD 분석 및 다운로드");
        AutomationProperties.SetName(VodCancelButton, "VOD 다운로드 취소");
        var commandBar = new CommandBar { DefaultLabelPosition = CommandBarDefaultLabelPosition.Right };
        commandBar.PrimaryCommands.Add(VodStartButton);
        commandBar.PrimaryCommands.Add(VodCancelButton);
        stack.Children.Add(commandBar);

        VodProgress = new ProgressBar { Minimum = 0, Maximum = 100, Value = 0 };
        VodStatusText = new TextBlock { Text = "대기 중", Foreground = Muted, TextWrapping = TextWrapping.Wrap };
        stack.Children.Add(VodProgress);
        stack.Children.Add(VodStatusText);
        root.Children.Add(new ScrollViewer { Content = stack });

        vodBackend.Output += VodBackend_Output;
        vodBackend.Exited += VodBackend_Exited;
        return root;
    }

    FrameworkElement BuildVodExecutablePicker(TextBox target, string accessibleName, string expectedFileName)
    {
        var grid = new Grid { ColumnSpacing = DesignTokens.SpaceSm };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(target);

        var browse = new Button
        {
            Content = new SymbolIcon(Symbol.OpenFile),
            VerticalAlignment = VerticalAlignment.Bottom,
            MinWidth = 44,
            Height = 34
        };
        AutomationProperties.SetName(browse, accessibleName);
        ToolTipService.SetToolTip(browse, accessibleName);
        browse.Click += async (_, _) => await PickVodExecutableAsync(target, expectedFileName);
        Grid.SetColumn(browse, 1);
        grid.Children.Add(browse);
        return grid;
    }

    async Task PickVodExecutableAsync(TextBox target, string expectedFileName)
    {
        var picker = new Windows.Storage.Pickers.FileOpenPicker
        {
            SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads
        };
        picker.FileTypeFilter.Add(".exe");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var file = await picker.PickSingleFileAsync();
        if (file == null) return;
        target.Text = file.Path;
        if (!string.Equals(file.Name, expectedFileName, StringComparison.OrdinalIgnoreCase))
            VodStatusText.Text = $"선택한 파일명이 {expectedFileName}이 아닙니다. 실행 파일을 다시 확인해 주세요.";
    }

    async void VodStartButton_Click(object sender, RoutedEventArgs e)
    {
        if (vodBackend.IsRunning) return;
        if (!Uri.TryCreate(VodUrlBox.Text.Trim(), UriKind.Absolute, out var uri) ||
            uri.Scheme != Uri.UriSchemeHttps || !uri.Host.EndsWith("sooplive.com", StringComparison.OrdinalIgnoreCase) ||
            !uri.AbsolutePath.Contains("/player/", StringComparison.OrdinalIgnoreCase))
        {
            VodStatusText.Text = "올바른 SOOP VOD HTTPS URL을 입력하세요.";
            return;
        }
        var output = VodOutputBox.Text.Trim();
        if (string.IsNullOrWhiteSpace(output)) { VodStatusText.Text = "출력 폴더를 입력하세요."; return; }

        IReadOnlyList<int> parts;
        try { parts = string.IsNullOrWhiteSpace(VodPartsBox.Text) ? Array.Empty<int>() : VodSelectionParser.Parse(VodPartsBox.Text, 10000); }
        catch (Exception ex) { VodStatusText.Text = ex.Message; return; }

        var jobId = Guid.NewGuid().ToString("N");
        activeVodJobId = jobId;
        activeVodFailureDetail = "";
        var jobDirectory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "SOOPLiveDownloader", "vod-jobs", jobId);
        Directory.CreateDirectory(jobDirectory);
        var requestPath = Path.Combine(jobDirectory, "request.json");
        var cookieMode = SelectedVodCookieMode();
        var cookieSource = VodCookieSourceBox.Text.Trim();
        var cookieFile = cookieMode == "FILE" ? cookieSource : vodSettings.CookieFile;
        var browserName = cookieMode == "BROWSER" ? cookieSource : vodSettings.BrowserName;
        if (cookieMode == "SOOP_LOGIN")
        {
            cookieFile = "";
            browserName = "";
        }
        var request = new VodJobRequest(1, jobId, VodUrlBox.Text.Trim(), parts, output,
            cookieMode, cookieFile, browserName, VodYtDlpPathBox.Text.Trim(), VodFfmpegPathBox.Text.Trim(),
            vodSettings.Merge, vodSettings.MaxRetries);
        WriteJsonAtomically(requestPath, request);
        vodSettings = vodSettings with
        {
            OutputDirectory = output,
            CookieMode = cookieMode,
            CookieFile = cookieFile,
            BrowserName = browserName,
            YtDlpPath = VodYtDlpPathBox.Text.Trim(),
            FfmpegPath = VodFfmpegPathBox.Text.Trim()
        };
        VodSettingsStore.Save(vodSettings);

        try
        {
            activeVodJobDirectory = jobDirectory;
            vodBackend.Start(Path.Combine(backendDir, "vod", "SOOP_VOD.ps1"), requestPath);
            VodStartButton.IsEnabled = false;
            VodCancelButton.IsEnabled = true;
            VodProgress.IsIndeterminate = true;
            VodStatusText.Text = "VOD 분석 중…";
        }
        catch (Exception ex) { VodStatusText.Text = ex.Message; CleanupVodJobDirectory(); }
        await Task.CompletedTask;
    }

    void VodCancelButton_Click(object sender, RoutedEventArgs e)
    {
        VodStatusText.Text = "취소 중…";
        VodCancelButton.IsEnabled = false;
        if (!vodBackend.Stop()) VodStatusText.Text = "VOD 프로세스 종료를 확인하지 못했습니다.";
    }

    void VodBackend_Output(string line)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            if (VodEventParser.TryParse(line, out var item) && item != null)
            {
                VodStatusText.Text = string.IsNullOrWhiteSpace(item.Message) ? item.Type : item.Message;
                if (!string.IsNullOrWhiteSpace(item.Title)) activeVodTitle = item.Title;
                if (!string.IsNullOrWhiteSpace(item.Streamer)) activeVodStreamer = item.Streamer;
                if (!string.IsNullOrWhiteSpace(item.OutputFile)) activeVodOutputFile = item.OutputFile;
                if (item.Type == "failed") activeVodFailureDetail = item.Message;
                if (item.Type == "completed")
                    vodHistory.Append(new VodHistoryEntry(activeVodJobId, DateTimeOffset.Now, VodUrlBox.Text.Trim(), activeVodTitle, activeVodStreamer, activeVodOutputFile, "COMPLETED"));
                if (item.Percent > 0) { VodProgress.IsIndeterminate = false; VodProgress.Value = Math.Clamp(item.Percent, 0, 100); }
            }
            else if (!line.StartsWith("[VOD STDERR]", StringComparison.Ordinal)) VodStatusText.Text = line;
        });
    }

    void VodBackend_Exited(int code) => DispatcherQueue.TryEnqueue(() =>
    {
        VodStartButton.IsEnabled = true;
        VodCancelButton.IsEnabled = false;
        VodProgress.IsIndeterminate = false;
        if (code != 0 && !VodStatusText.Text.Contains("취소", StringComparison.Ordinal))
        {
            var detail = !string.IsNullOrWhiteSpace(activeVodFailureDetail)
                ? activeVodFailureDetail
                : vodBackend.LastErrorSummary;
            VodStatusText.Text = string.IsNullOrWhiteSpace(detail)
                ? $"VOD 작업 오류 종료 ({code})"
                : $"VOD 작업 오류 종료 ({code}) · {detail}";
        }
        CleanupVodJobDirectory();
    });

    static void WriteJsonAtomically<T>(string path, T value)
    {
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(value), new UTF8Encoding(false));
        using var _ = JsonDocument.Parse(File.ReadAllText(temporary, Encoding.UTF8));
        File.Move(temporary, path, true);
    }

    void CleanupVodJobDirectory()
    {
        var directory = activeVodJobDirectory;
        activeVodJobDirectory = null;
        if (string.IsNullOrWhiteSpace(directory)) return;
        try { Directory.Delete(directory, true); } catch { }
    }

    string SelectedVodCookieMode() =>
        (VodCookieModeBox.SelectedItem as ComboBoxItem)?.Tag as string ?? "SOOP_LOGIN";

    void RefreshVodLoginAvailability()
    {
        if (VodLoginStatusText == null) return;
        try
        {
            var config = IniService.Read(iniPath);
            var hasUser = config.TryGetValue("SOOP_USERNAME", out var username) && !string.IsNullOrWhiteSpace(username);
            var hasPassword = config.TryGetValue("SOOP_PASSWORD", out var password) && !string.IsNullOrWhiteSpace(password);
            VodLoginStatusText.Text = hasUser && hasPassword
                ? "✓ 설정 탭의 SOOP 로그인 사용 · 비밀번호는 요청 JSON에 포함되지 않습니다."
                : "⚠ 설정 탭에 SOOP 아이디와 비밀번호를 먼저 저장해 주세요.";
            VodLoginStatusText.Foreground = hasUser && hasPassword ? Accent : DesignTokens.Warning;
        }
        catch
        {
            VodLoginStatusText.Text = "⚠ SOOP 로그인 설정 상태를 확인하지 못했습니다.";
            VodLoginStatusText.Foreground = DesignTokens.Warning;
        }
    }
}
