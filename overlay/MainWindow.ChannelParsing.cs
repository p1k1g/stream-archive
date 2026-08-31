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
}
