using System.Text.Json;
using SOOPLiveWinUI;

static class RecentRecordingStoreRegression
{
    internal static void Run()
    {
        var directory = Path.Combine(Path.GetTempPath(), "soop-recent-test-" + Guid.NewGuid().ToString("N"));
        var path = Path.Combine(directory, "recent.json");
        try
        {
            var store = new RecentRecordingStore(path, 200);
            for (var index = 0; index < 100; index++)
            {
                store.Save(new[] { new RecentRecordingEntry
                {
                    Id = index,
                    Name = "channel",
                    EndedAt = DateTime.UtcNow
                }});
            }
            store.FlushAsync().GetAwaiter().GetResult();
            using var document = JsonDocument.Parse(File.ReadAllText(path));
            Check(document.RootElement[0].GetProperty("Id").GetInt32() == 99,
                "coalesced recent save must persist the latest snapshot");
            Check(store.CompletedWriteCount == 1,
                "a burst of recent saves must coalesce into one physical write");
            Console.WriteLine("Recent recording coalescing regression tests passed.");
        }
        finally
        {
            try { if (Directory.Exists(directory)) Directory.Delete(directory, true); } catch { }
        }
    }

    static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}

namespace SOOPLiveWinUI
{
    public sealed class RecentRecordingEntry
    {
        public int Id { get; set; }
        public DateTime EndedAt { get; set; }
        public string Name { get; set; } = "";
    }
}
