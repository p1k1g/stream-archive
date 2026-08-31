using System.Collections.Concurrent;
using SOOPLiveWinUI;

static class EventPipelineSoakRegression
{
    internal static void Run()
    {
        PriorityQueueStaysBounded();
        DriveCacheHonorsTtl();
        VirtualMultiChannelSoakStaysBounded();
        Console.WriteLine("Event pipeline 24-hour virtual soak tests passed.");
    }

    static void DriveCacheHonorsTtl()
    {
        var now = new DateTime(2026, 8, 31, 0, 0, 0, DateTimeKind.Utc);
        var queries = 0;
        var cache = new DriveSpaceCache(
            TimeSpan.FromSeconds(5),
            _ => { queries++; return (true, 123L); },
            () => now);

        Check(cache.TryGetAvailableBytes("C:\\", out var first) && first == 123L,
            "first drive lookup must query the provider");
        for (var repeat = 0; repeat < 100; repeat++)
            Check(cache.TryGetAvailableBytes("C:\\", out _), "cached drive lookup failed");
        Check(queries == 1, "same-root lookups inside TTL must use one provider query");

        now = now.AddMilliseconds(4_999);
        cache.TryGetAvailableBytes("C:\\", out _);
        Check(queries == 1, "drive cache expired before its TTL");
        now = now.AddMilliseconds(1);
        cache.TryGetAvailableBytes("C:\\", out _);
        Check(queries == 2, "drive cache did not refresh at TTL expiry");
    }

    static void PriorityQueueStaysBounded()
    {
        var queue = new BoundedConcurrentQueue<int>(512);
        for (var index = 0; index < 100_000; index++) queue.Enqueue(index);
        Check(queue.Count == 512, "priority queue exceeded its hard capacity");
        Check(queue.TakeDroppedCount() == 99_488, "priority overflow count is incorrect");
        Check(queue.TryDequeue(out var first) && first == 99_488,
            "priority queue must retain the newest bounded window");
    }

    static void VirtualMultiChannelSoakStaysBounded()
    {
        Check(typeof(ProgressSnapshot).IsValueType,
            "progress snapshot must remain a value type");
        var now = new DateTime(2026, 8, 31, 0, 0, 0, DateTimeKind.Utc);
        var driveQueries = 0;
        var cache = new DriveSpaceCache(
            TimeSpan.FromSeconds(5),
            _ => { driveQueries++; return (true, 500L * 1024 * 1024 * 1024); },
            () => now);
        var warnings = new WarningDeduplicator(TimeSpan.FromSeconds(30));
        var progress = new ConcurrentDictionary<string, ProgressSnapshot>(StringComparer.OrdinalIgnoreCase);
        var accounts = Enumerable.Range(0, 64).Select(index => "channel" + index).ToArray();
        var roots = new[] { "C:\\", "D:\\", "E:\\", "F:\\" };
        var emittedWarnings = 0;

        for (var second = 0; second < 24 * 60 * 60; second++)
        {
            var account = accounts[second % accounts.Length];
            progress[account] = new ProgressSnapshot(account, account, "1 GB", "01:00:00", "1 MB/s");
            var warning = $"[{now:HH:mm:ss}] [WARN] transient worker failure";
            if (!warnings.ShouldSuppress(warning, now)) emittedWarnings++;
            cache.TryGetAvailableBytes(roots[second % roots.Length], out _);
            now = now.AddSeconds(1);
        }

        Check(progress.Count == 64, "progress snapshots grew beyond channel cardinality");
        Check(emittedWarnings <= 2_881, "warning deduplication did not bound repeated output");
        Check(warnings.TakeSuppressedCount() >= 80_000, "warning deduplication suppressed too few repeats");
        Check(cache.Count == roots.Length, "drive cache grew beyond output-root cardinality");
        Check(driveQueries < 24 * 60 * 60,
            $"drive cache did not reduce repeated drive queries (queries={driveQueries})");
    }

    static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}
