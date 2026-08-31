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
    void UpdateSelectedRecordingActionButton()
    {
        if (StopSelectedRecordingButton == null ||
            OpenSelectedRecordingFolderButton == null)
            return;

        if (RecordingList?.SelectedItem is not ChannelStatus selected)
        {
            StopSelectedRecordingButton.Content = "■ 선택 채널 녹화 중지";
            StopSelectedRecordingButton.IsEnabled = false;
            OpenSelectedRecordingFolderButton.IsEnabled = false;
            return;
        }

        OpenSelectedRecordingFolderButton.IsEnabled =
            !string.IsNullOrWhiteSpace(selected.FilePath);

        if (selected.Status == "● REC")
        {
            StopSelectedRecordingButton.Content = "■ 선택 채널 녹화 중지";
            StopSelectedRecordingButton.IsEnabled = true;
            return;
        }

        StopSelectedRecordingButton.Content = "녹화 상태 확인 중";
        StopSelectedRecordingButton.IsEnabled = false;
    }

    void RecordingList_ContainerContentChanging(
        ListViewBase sender,
        ContainerContentChangingEventArgs args)
    {
        if (args.InRecycleQueue ||
            args.ItemContainer is not ListViewItem container ||
            args.Item is not ChannelStatus item)
            return;

        var menu = new MenuFlyout();
        var openFolder = new MenuFlyoutItem { Text = "녹화 폴더 열기" };
        openFolder.Click += async (_, _) => await OpenRecordingLocationAsync(item, selectFile: false);
        var selectFile = new MenuFlyoutItem { Text = "파일 위치에서 선택" };
        selectFile.Click += async (_, _) => await OpenRecordingLocationAsync(item, selectFile: true);
        var copyPath = new MenuFlyoutItem { Text = "녹화 경로 복사" };
        copyPath.Click += async (_, _) => await CopyRecordingPathAsync(item);
        var stop = new MenuFlyoutItem
        {
            Text = "현재 방송 녹화 중지",
            IsEnabled = item.Status == "● REC"
        };
        stop.Click += (_, _) =>
        {
            RecordingList.SelectedItem = item;
            StopSelectedRecording_Click(StopSelectedRecordingButton, new RoutedEventArgs());
        };

        var hasPath = !string.IsNullOrWhiteSpace(item.FilePath);
        openFolder.IsEnabled = hasPath;
        selectFile.IsEnabled = hasPath;
        copyPath.IsEnabled = hasPath;
        menu.Items.Add(openFolder);
        menu.Items.Add(selectFile);
        menu.Items.Add(copyPath);
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(stop);
        container.ContextFlyout = menu;
    }

    async void OpenSelectedRecordingFolder_Click(object sender, RoutedEventArgs e)
    {
        if (RecordingList?.SelectedItem is not ChannelStatus selected)
            return;

        await OpenRecordingLocationAsync(selected, selectFile: false);
    }

    async Task OpenRecordingLocationAsync(ChannelStatus selected, bool selectFile)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(selected.FilePath))
                throw new InvalidOperationException("선택한 녹화의 파일 경로를 아직 확인하지 못했습니다.");

            var fullPath = Path.GetFullPath(selected.FilePath);
            var directory = Path.GetDirectoryName(fullPath);
            if (string.IsNullOrWhiteSpace(directory) || !Directory.Exists(directory))
                throw new DirectoryNotFoundException("녹화 폴더를 찾을 수 없습니다.\n" + directory);

            var startInfo = new ProcessStartInfo("explorer.exe")
            {
                UseShellExecute = true
            };
            if (selectFile && File.Exists(fullPath))
            {
                startInfo.ArgumentList.Add("/select,");
                startInfo.ArgumentList.Add(fullPath);
            }
            else
            {
                startInfo.ArgumentList.Add(directory);
            }
            Process.Start(startInfo);
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("녹화 폴더 열기 실패", ex.Message);
        }
    }

    async Task CopyRecordingPathAsync(ChannelStatus selected)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(selected.FilePath))
                throw new InvalidOperationException("선택한 녹화의 파일 경로를 아직 확인하지 못했습니다.");

            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(Path.GetFullPath(selected.FilePath));
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            Windows.ApplicationModel.DataTransfer.Clipboard.Flush();
            AppendLog($"[GUI] 녹화 경로 복사 channel={selected.Name}");
        }
        catch (Exception ex)
        {
            await ShowDialogAsync("녹화 경로 복사 실패", ex.Message);
        }
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
}
