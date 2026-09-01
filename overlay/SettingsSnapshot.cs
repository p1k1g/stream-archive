using System.Text.Json;

namespace SOOPLiveWinUI;

internal static class SettingsSnapshot
{
    internal static string Create(IEnumerable<KeyValuePair<string, string?>> values)
    {
        var normalized = values
            .OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .ToDictionary(
                pair => pair.Key,
                pair => pair.Value?.Trim() ?? "",
                StringComparer.Ordinal);
        return JsonSerializer.Serialize(normalized);
    }
}
