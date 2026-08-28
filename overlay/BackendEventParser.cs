using System.Text.Json;
using System.Text.RegularExpressions;

namespace SOOPLiveWinUI;

internal sealed record BackendEvent(
    int Version,
    string Type,
    string Account,
    string Name,
    string Title,
    string File,
    string Reason,
    string Duration,
    string Size,
    string Detail,
    int? CooldownSeconds,
    string Until,
    string Action);

internal static partial class BackendEventParser
{
    internal const string Prefix = "@@SOOP_EVENT@@";

    static readonly Regex LegacyRecordStart = new(
        @"^\[(?<date>[^]]+)\] \[INFO\] RECORD START channel=(?<name>.+?) account=(?<account>[A-Za-z0-9_]+) bno=(?<bno>\S+) (?<rest>.+)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex LegacyRecordFinished = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)\s+\[account=(?<account>[A-Za-z0-9_]+)\]\s*:\s*RECORD FINISHED\s*\|\s*duration=(?<duration>[^|]*)\|\s*size=(?<size>[^|]*)\|\s*reason=(?<reason>[^|]*)\|\s*file=(?<file>.*)$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    static readonly Regex LegacyState = new(
        @"^\[(?<time>\d{2}:\d{2}:\d{2})\]\s+(?<name>.+?)\s+\[account=(?<account>[A-Za-z0-9_]+)\]\s*:\s*(?<type>RECORD STALLED|LOW DISK(?: SPACE)?|WORKER COOLDOWN|CHANNEL REMOVED|CHANNEL DISABLED)(?:\s*-\s*(?<detail>.*)|\s*\((?<detail2>.*)\))?$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    internal static bool TryParse(string line, out BackendEvent? backendEvent)
    {
        backendEvent = null;
        if (string.IsNullOrWhiteSpace(line)) return false;

        var text = line.Trim();
        if (text.StartsWith(Prefix, StringComparison.Ordinal))
            return TryParseJson(text[Prefix.Length..], out backendEvent);

        return TryParseLegacy(text, out backendEvent);
    }

    static bool TryParseJson(string json, out BackendEvent? backendEvent)
    {
        backendEvent = null;
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            var version = Int(root, "version") ?? 0;
            var type = Text(root, "type");
            if (version != 1 || string.IsNullOrWhiteSpace(type)) return false;

            backendEvent = new BackendEvent(
                version, type, Text(root, "account"), Text(root, "name"),
                Text(root, "title"), Text(root, "file"), Text(root, "reason"),
                Text(root, "duration"), Text(root, "size"), Text(root, "detail"),
                Int(root, "cooldownSeconds"), Text(root, "until"), Text(root, "action"));
            return true;
        }
        catch (Exception ex) when (ex is JsonException or InvalidOperationException)
        {
            return false;
        }
    }

    static bool TryParseLegacy(string text, out BackendEvent? backendEvent)
    {
        backendEvent = null;
        var finished = LegacyRecordFinished.Match(text);
        if (finished.Success)
        {
            backendEvent = Empty("recording_finished") with
            {
                Account = Value(finished, "account"), Name = Value(finished, "name"),
                File = Value(finished, "file"), Reason = Value(finished, "reason"),
                Duration = Value(finished, "duration"), Size = Value(finished, "size")
            };
            return true;
        }

        var start = LegacyRecordStart.Match(text);
        if (start.Success)
        {
            ParseStartRest(Value(start, "rest"), out var title, out var file);
            backendEvent = Empty("recording_started") with
            {
                Account = Value(start, "account"), Name = Value(start, "name"),
                Title = title, File = file
            };
            return true;
        }

        var state = LegacyState.Match(text);
        if (!state.Success) return false;
        var rawType = Value(state, "type").ToUpperInvariant();
        var type = rawType switch
        {
            "RECORD STALLED" => "recording_stalled",
            "LOW DISK" or "LOW DISK SPACE" => "low_disk",
            "WORKER COOLDOWN" => "worker_cooldown",
            "CHANNEL REMOVED" => "channel_removed",
            "CHANNEL DISABLED" => "channel_disabled",
            _ => ""
        };
        if (type.Length == 0) return false;
        var detail = state.Groups["detail"].Success
            ? Value(state, "detail") : Value(state, "detail2");
        backendEvent = Empty(type) with
        {
            Account = Value(state, "account"), Name = Value(state, "name"), Detail = detail,
            Action = type.StartsWith("channel_", StringComparison.Ordinal) ? type[8..] : ""
        };
        return true;
    }

    static BackendEvent Empty(string type) =>
        new(1, type, "", "", "", "", "", "", "", "", null, "", "");

    static string Text(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.ValueKind != JsonValueKind.Null
            ? value.ToString() : "";

    static int? Int(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.TryGetInt32(out var number)
            ? number : null;

    static string Value(Match match, string name) => match.Groups[name].Value.Trim();

    static void ParseStartRest(string rest, out string title, out string file)
    {
        title = rest;
        file = "";
        var fileIndex = rest.LastIndexOf(" file=", StringComparison.OrdinalIgnoreCase);
        if (fileIndex >= 0)
        {
            file = rest[(fileIndex + 6)..].Trim();
            rest = rest[..fileIndex];
        }
        var titleIndex = rest.IndexOf("title=", StringComparison.OrdinalIgnoreCase);
        title = titleIndex >= 0 ? rest[(titleIndex + 6)..].Trim() : rest.Trim();
        foreach (var marker in new[] { " hls=", " cdn=", " host=" })
        {
            var index = title.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
            if (index >= 0) title = title[..index].Trim();
        }
    }
}
