using System.Text.Json;

namespace SOOPLiveWinUI;

public static class VodEventParser
{
    public const string Prefix = "@@SOOP_VOD_EVENT@@";

    public static bool TryParse(string? line, out VodBackendEvent? value)
    {
        value = null;
        if (string.IsNullOrWhiteSpace(line) || !line.StartsWith(Prefix, StringComparison.Ordinal)) return false;
        try
        {
            using var document = JsonDocument.Parse(line[Prefix.Length..]);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || GetInt(root, "version") != 1) return false;
            var type = GetString(root, "type");
            if (string.IsNullOrWhiteSpace(type)) return false;
            value = new VodBackendEvent(1, type, GetString(root, "jobId"), GetString(root, "message"),
                GetString(root, "title"), GetString(root, "streamer"), GetInt(root, "part"),
                GetInt(root, "partCount"), GetDouble(root, "percent"), GetString(root, "outputFile"));
            return true;
        }
        catch (JsonException) { return false; }
    }

    static string GetString(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() ?? "" : "";
    static int GetInt(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.TryGetInt32(out var number) ? number : 0;
    static double GetDouble(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.TryGetDouble(out var number) ? number : 0;
}
