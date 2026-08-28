using System.Text.RegularExpressions;

namespace SOOPLiveWinUI;

public static class ChannelFileParser
{
    public static string? TryParse(string? text, out List<EditableChannel> parsed)
    {
        parsed = new List<EditableChannel>();
        var seenAccounts = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var lineNumber = 0;

        foreach (var raw in SplitLines(text))
        {
            lineNumber++;
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith("#"))
                continue;

            var parts = line.Split('|', 4);
            if (parts.Length < 3)
                return $"{lineNumber}번째 줄 형식이 잘못되었습니다.\n내용: {line}\n\n형식: Y|이름|SOOP_ID|저장경로";

            var enabledText = parts[0].Trim();
            if (!enabledText.Equals("Y", StringComparison.OrdinalIgnoreCase) &&
                !enabledText.Equals("N", StringComparison.OrdinalIgnoreCase))
                return $"{lineNumber}번째 줄의 활성 값은 Y 또는 N이어야 합니다.\n내용: {line}";

            var account = NormalizeAccount(parts[2]);
            if (string.IsNullOrWhiteSpace(account))
                return $"{lineNumber}번째 줄의 SOOP 계정 ID 또는 URL이 잘못되었습니다.\n내용: {line}";
            if (!seenAccounts.Add(account))
                return $"{lineNumber}번째 줄에 중복된 SOOP 계정 ID가 있습니다: {account}";

            var name = parts[1].Trim();
            parsed.Add(new EditableChannel
            {
                Enabled = enabledText.Equals("Y", StringComparison.OrdinalIgnoreCase),
                Name = string.IsNullOrWhiteSpace(name) ? account : name,
                Account = account,
                OutDir = parts.Length >= 4 ? parts[3].Trim() : ""
            });
        }

        return null;
    }

    public static string? NormalizeAccount(string? input)
    {
        var value = input?.Trim() ?? "";
        if (value.Length == 0)
            return null;

        if (Uri.TryCreate(value, UriKind.Absolute, out var uri))
        {
            var host = uri.Host.ToLowerInvariant();
            var segments = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries);
            if (host.StartsWith("play.sooplive.") && segments.Length >= 1)
                value = segments[0];
            else if ((host == "www.sooplive.com" || host == "www.sooplive.co.kr") &&
                     segments.Length >= 2 &&
                     segments[0].Equals("station", StringComparison.OrdinalIgnoreCase))
                value = segments[1];
            else
                return null;
        }

        return Regex.IsMatch(value, "^[A-Za-z0-9_]+$") ? value : null;
    }

    public static IEnumerable<string> SplitLines(string? text) =>
        (text ?? "").Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');
}
