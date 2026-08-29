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
}
