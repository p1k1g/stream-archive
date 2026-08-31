using System.Text.Json;
using SOOPLiveWinUI;

static class VodFeatureRegression
{
    public static void Run()
    {
        Equal(VodSelectionParser.Parse("", 4), 1, 2, 3, 4);
        Equal(VodSelectionParser.Parse("1-3,2,5", 5), 1, 2, 3, 5);
        Equal(VodSelectionParser.Parse("3", 5), 3);
        Throws(() => VodSelectionParser.Parse("3-1", 5));
        Throws(() => VodSelectionParser.Parse("0", 5));
        Throws(() => VodSelectionParser.Parse("1,,2", 5));

        var line = VodEventParser.Prefix + JsonSerializer.Serialize(new
        {
            version = 1,
            type = "part_progress",
            jobId = "job[한글]|=",
            message = "제목[테스트]|= 다운로드",
            title = "방송 제목[1]|=",
            streamer = "문월:-)",
            part = 2,
            partCount = 10,
            percent = 42.5,
            outputFile = @"C:\VOD[한글]\제목|=.mp4"
        });
        if (!VodEventParser.TryParse(line, out var parsed) || parsed == null ||
            parsed.Percent != 42.5 || parsed.OutputFile != @"C:\VOD[한글]\제목|=.mp4")
            throw new InvalidOperationException("VOD JSON event special-character regression.");
        if (VodEventParser.TryParse(VodEventParser.Prefix + "{bad", out _))
            throw new InvalidOperationException("Malformed VOD JSON event was accepted.");
        if (VodEventParser.TryParse(VodEventParser.Prefix + "{\"version\":2,\"type\":\"completed\"}", out _))
            throw new InvalidOperationException("Unknown VOD event version was accepted.");

        var state = new VodSettings(MaxRetries: 100, CookieMode: "browser", BrowserName: "");
        // Store normalization is covered by source-level atomic-write checks; this
        // verifies the records remain dependency-free and serializable in CI.
        if (!JsonSerializer.Serialize(state).Contains("MaxRetries", StringComparison.Ordinal))
            throw new InvalidOperationException("VOD settings are not serializable.");
        var directory = Path.Combine(Path.GetTempPath(), "soop-vod-history-" + Guid.NewGuid().ToString("N"));
        var historyPath = Path.Combine(directory, "history.json");
        try
        {
            var history = new VodHistoryStore(historyPath);
            for (var index = 0; index < 205; index++)
                history.Append(new VodHistoryEntry(index.ToString(), DateTimeOffset.UtcNow, "https://vod.sooplive.com/player/1", "제목", "BJ", @"C:\VOD[한글]\제목|=.mp4", "COMPLETED"));
            if (history.Load().Count != 200 || Directory.GetFiles(directory, "*.tmp").Length != 0)
                throw new InvalidOperationException("VOD history bound/atomic cleanup regression.");
        }
        finally { try { Directory.Delete(directory, true); } catch { } }
        Console.WriteLine("VOD feature regression tests passed.");
    }

    static void Equal(IReadOnlyList<int> actual, params int[] expected)
    {
        if (!actual.SequenceEqual(expected)) throw new InvalidOperationException("VOD PART selection mismatch.");
    }

    static void Throws(Action action)
    {
        try { action(); }
        catch { return; }
        throw new InvalidOperationException("Invalid VOD PART selection was accepted.");
    }
}
