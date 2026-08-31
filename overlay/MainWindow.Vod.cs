using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    readonly VodProcessService vodBackend = new();
    TextBox VodUrlBox = null!;
    TextBox VodOutputBox = null!;
    TextBox VodPartsBox = null!;
    ComboBox VodCookieModeBox = null!;
    TextBox VodCookieSourceBox = null!;
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

    FrameworkElement BuildVodView()
    {
        var root = new Grid { Padding = DesignTokens.PagePadding };
        var stack = new StackPanel { Spacing = DesignTokens.SpaceLg, MaxWidth = 980, HorizontalAlignment = HorizontalAlignment.Stretch };
        stack.Children.Add(new TextBlock { Text = "VOD 다운로드", FontSize = 26, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold });
        stack.Children.Add(new TextBlock { Text = "사용 권한이 있는 SOOP VOD를 분석하고 선택한 PART를 다운로드합니다. LIVE Watcher와 별도로 실행됩니다.", Foreground = Muted, TextWrapping = TextWrapping.Wrap });

        VodUrlBox = new TextBox { Header = "VOD URL", PlaceholderText = "https://vod.sooplive.com/player/204952073" };
        VodOutputBox = new TextBox { Header = "출력 폴더", Text = vodSettings.OutputDirectory, PlaceholderText = @"C:\Videos" };
        VodPartsBox = new TextBox { Header = "PART 선택", PlaceholderText = "비워 두면 전체 · 예: 1-5,8,10-12" };
        VodCookieModeBox = new ComboBox { Header = "Cookie 방식", HorizontalAlignment = HorizontalAlignment.Stretch };
        VodCookieModeBox.Items.Add("FILE");
        VodCookieModeBox.Items.Add("BROWSER");
        VodCookieModeBox.SelectedItem = vodSettings.CookieMode;
        VodCookieSourceBox = new TextBox
        {
            Header = "Cookie 파일 경로 또는 브라우저 이름",
            Text = vodSettings.CookieMode == "BROWSER" ? vodSettings.BrowserName : vodSettings.CookieFile,
            PlaceholderText = "FILE: cookies.txt 전체 경로 · BROWSER: firefox 또는 chrome"
        };
        VodCookieModeBox.SelectionChanged += (_, _) =>
        {
            var browser = (VodCookieModeBox.SelectedItem as string) == "BROWSER";
            VodCookieSourceBox.Text = browser ? vodSettings.BrowserName : vodSettings.CookieFile;
        };
        stack.Children.Add(VodUrlBox);
        stack.Children.Add(VodOutputBox);
        stack.Children.Add(VodPartsBox);
        stack.Children.Add(VodCookieModeBox);
        stack.Children.Add(VodCookieSourceBox);

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
        var jobDirectory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "SOOPLiveDownloader", "vod-jobs", jobId);
        Directory.CreateDirectory(jobDirectory);
        var requestPath = Path.Combine(jobDirectory, "request.json");
        var cookieMode = VodCookieModeBox.SelectedItem as string ?? "FILE";
        var cookieSource = VodCookieSourceBox.Text.Trim();
        var cookieFile = cookieMode == "FILE" ? cookieSource : vodSettings.CookieFile;
        var browserName = cookieMode == "BROWSER" ? cookieSource : vodSettings.BrowserName;
        var request = new VodJobRequest(1, jobId, VodUrlBox.Text.Trim(), parts, output,
            cookieMode, cookieFile, browserName, vodSettings.Merge, vodSettings.MaxRetries);
        WriteJsonAtomically(requestPath, request);
        vodSettings = vodSettings with { OutputDirectory = output, CookieMode = cookieMode, CookieFile = cookieFile, BrowserName = browserName };
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
        if (code != 0 && !VodStatusText.Text.Contains("취소", StringComparison.Ordinal)) VodStatusText.Text = $"VOD 작업 오류 종료 ({code})";
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
}
