using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Text;
using WinRT.Interop;

namespace SOOPLiveWinUI;

public sealed partial class MainWindow
{
    async void ImportChannelsFix38_Click(object sender, RoutedEventArgs e)
    {
        if (!await EnsureRawChannelEditsAppliedAsync())
            return;

        try
        {
            var picker = new Windows.Storage.Pickers.FileOpenPicker
            {
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary,
                ViewMode = Windows.Storage.Pickers.PickerViewMode.List
            };
            picker.FileTypeFilter.Add(".txt");
            picker.FileTypeFilter.Add(".bak");
            picker.FileTypeFilter.Add(".csv");
            InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
            var file = await picker.PickSingleFileAsync();
            if (file == null)
                return;

            var text = await Windows.Storage.FileIO.ReadTextAsync(file);
            var error = ChannelFileParser.TryParse(text, out var imported);
            if (!string.IsNullOrWhiteSpace(error))
            {
                await ShowDialogAsync("채널 가져오기 실패", error);
                return;
            }

            if (imported.Count == 0)
            {
                await ShowDialogAsync("채널 가져오기", "가져올 수 있는 채널 행이 없습니다.");
                return;
            }

            var existing = ChannelItems.ToDictionary(x => x.Account, StringComparer.OrdinalIgnoreCase);
            var additions = imported.Where(x => !existing.ContainsKey(x.Account)).ToList();
            var duplicates = imported.Where(x => existing.ContainsKey(x.Account)).ToList();
            var changedDuplicates = duplicates.Where(x =>
            {
                var current = existing[x.Account];
                return current.Enabled != x.Enabled ||
                       !string.Equals(current.Name, x.Name, StringComparison.Ordinal) ||
                       !string.Equals(current.OutDir, x.OutDir, StringComparison.OrdinalIgnoreCase);
            }).ToList();

            var preview = new StringBuilder();
            preview.AppendLine(file.Path);
            preview.AppendLine();
            preview.AppendLine($"가져온 채널: {imported.Count}개");
            preview.AppendLine($"새 채널: {additions.Count}개");
            preview.AppendLine($"기존 계정: {duplicates.Count}개 (변경 가능 {changedDuplicates.Count}개)");
            preview.AppendLine();
            preview.AppendLine("새로 추가될 채널");
            AppendImportPreviewFix38(preview, additions);
            preview.AppendLine();
            preview.AppendLine("기존 목록과 계정 ID가 같은 채널");
            AppendDuplicatePreviewFix38(preview, duplicates, existing);
            preview.AppendLine();
            preview.Append("가져온 내용은 즉시 파일에 저장되지 않습니다. 확인 후 ‘변경 저장’을 눌러야 저장됩니다.");

            var previewBox = new TextBox
            {
                Text = preview.ToString(),
                IsReadOnly = true,
                AcceptsReturn = true,
                TextWrapping = TextWrapping.Wrap,
                MinWidth = 560,
                Height = 390,
                FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas")
            };
            var dialog = new ContentDialog
            {
                Title = "채널 가져오기 미리보기",
                Content = previewBox,
                PrimaryButtonText = "새 채널만 추가",
                SecondaryButtonText = "기존 채널도 갱신",
                CloseButtonText = "취소",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = Content is FrameworkElement fe ? fe.XamlRoot : null
            };
            var result = await dialog.ShowAsync();
            if (result == ContentDialogResult.None)
                return;

            var updateCount = result == ContentDialogResult.Secondary
                ? changedDuplicates.Count
                : 0;
            if (additions.Count == 0 && updateCount == 0)
            {
                await ShowDialogAsync("채널 가져오기", "현재 목록에 반영할 새로운 변경 사항이 없습니다.");
                return;
            }

            var selectedAccounts = GetSelectedChannels()
                .Select(x => x.Account)
                .ToList();
            suppressChannelCollectionRefresh = true;
            try
            {
                foreach (var item in additions)
                    ChannelItems.Add(CloneImportedChannelFix38(item));

                if (result == ContentDialogResult.Secondary)
                {
                    foreach (var importedItem in changedDuplicates)
                    {
                        var current = existing[importedItem.Account];
                        current.Enabled = importedItem.Enabled;
                        current.Name = importedItem.Name;
                        current.OutDir = importedItem.OutDir;
                    }
                }
            }
            finally
            {
                suppressChannelCollectionRefresh = false;
            }

            SyncRawChannelTextFromItems(markDirty: true);
            RestoreChannelSelection(selectedAccounts);
            await ShowDialogAsync(
                "채널 가져오기 완료",
                $"새 채널 {additions.Count}개를 추가하고 기존 채널 {updateCount}개를 갱신했습니다.\n" +
                "아직 파일에는 저장되지 않았습니다. 내용을 확인한 후 ‘변경 저장’을 눌러 주세요.");
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("채널 가져오기 실패", ex.Message);
        }
    }

    static EditableChannel CloneImportedChannelFix38(EditableChannel source) => new()
    {
        Enabled = source.Enabled,
        Name = source.Name,
        Account = source.Account,
        OutDir = source.OutDir
    };

    static void AppendImportPreviewFix38(StringBuilder preview, IReadOnlyList<EditableChannel> items)
    {
        if (items.Count == 0)
        {
            preview.AppendLine("  없음");
            return;
        }

        foreach (var item in items.Take(12))
            preview.AppendLine($"  • {item.Name} ({item.Account}) · {(item.Enabled ? "활성" : "비활성")}");
        if (items.Count > 12)
            preview.AppendLine($"  • 외 {items.Count - 12}개");
    }

    static void AppendDuplicatePreviewFix38(
        StringBuilder preview,
        IReadOnlyList<EditableChannel> items,
        IReadOnlyDictionary<string, EditableChannel> existing)
    {
        if (items.Count == 0)
        {
            preview.AppendLine("  없음");
            return;
        }

        foreach (var item in items.Take(12))
        {
            var current = existing[item.Account];
            var changes = new List<string>();
            if (!string.Equals(current.Name, item.Name, StringComparison.Ordinal))
                changes.Add($"이름: {current.Name} → {item.Name}");
            if (current.Enabled != item.Enabled)
                changes.Add($"상태: {(current.Enabled ? "활성" : "비활성")} → {(item.Enabled ? "활성" : "비활성")}");
            if (!string.Equals(current.OutDir, item.OutDir, StringComparison.OrdinalIgnoreCase))
                changes.Add("저장 경로 변경");
            preview.AppendLine($"  • {item.Name} ({item.Account}) · " +
                (changes.Count == 0 ? "변경 없음" : string.Join(", ", changes)));
        }
        if (items.Count > 12)
            preview.AppendLine($"  • 외 {items.Count - 12}개");
    }

}
