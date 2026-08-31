using System.Collections.Concurrent;
using System.Text.RegularExpressions;

namespace SOOPLiveWinUI;

internal readonly record struct ProgressSnapshot(
    string Account,
    string Name,
    string Size,
    string Duration,
    string Rate);

internal sealed class BoundedConcurrentQueue<T>
{
    readonly ConcurrentQueue<T> queue = new();
    readonly int capacity;
    int count;
    long dropped;

    internal BoundedConcurrentQueue(int capacity) =>
        this.capacity = Math.Max(1, capacity);

    internal bool IsEmpty => Volatile.Read(ref count) == 0;
    internal int Count => Math.Max(0, Volatile.Read(ref count));

    internal void Enqueue(T item)
    {
        queue.Enqueue(item);
        var current = Interlocked.Increment(ref count);
        while (current > capacity && queue.TryDequeue(out _))
        {
            Interlocked.Decrement(ref count);
            Interlocked.Increment(ref dropped);
            current = Volatile.Read(ref count);
        }
    }

    internal bool TryDequeue(out T item)
    {
        if (!queue.TryDequeue(out var dequeued))
        {
            item = default!;
            return false;
        }
        item = dequeued;
        if (Interlocked.Decrement(ref count) < 0)
            Interlocked.Exchange(ref count, 0);
        return true;
    }

    internal long TakeDroppedCount() => Interlocked.Exchange(ref dropped, 0);

    internal void Clear()
    {
        while (queue.TryDequeue(out _)) { }
        Interlocked.Exchange(ref count, 0);
        Interlocked.Exchange(ref dropped, 0);
    }
}

internal sealed class WarningDeduplicator
{
    static readonly Regex LeadingTimestamp = new(
        @"^\[(?:\d{2}:\d{2}:\d{2}|\d{4}-\d{2}-\d{2}[^\]]*)\]\s*",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    readonly Dictionary<string, DateTime> lastSeen = new(StringComparer.Ordinal);
    readonly TimeSpan window;
    readonly int capacity;
    readonly object gate = new();
    long suppressed;

    internal WarningDeduplicator(TimeSpan window, int capacity = 512)
    {
        this.window = window;
        this.capacity = Math.Max(16, capacity);
    }

    internal bool ShouldSuppress(string line, DateTime now)
    {
        var key = LeadingTimestamp.Replace(line.Trim(), "");
        lock (gate)
        {
            if (lastSeen.TryGetValue(key, out var previous) &&
                now >= previous && now - previous < window)
            {
                Interlocked.Increment(ref suppressed);
                return true;
            }
            lastSeen[key] = now;
            if (lastSeen.Count > capacity)
            {
                var expiry = now - window;
                foreach (var expired in lastSeen.Where(pair => pair.Value < expiry)
                             .Select(pair => pair.Key).Take(lastSeen.Count - capacity).ToArray())
                    lastSeen.Remove(expired);
                while (lastSeen.Count > capacity)
                    lastSeen.Remove(lastSeen.First().Key);
            }
            return false;
        }
    }

    internal long TakeSuppressedCount() => Interlocked.Exchange(ref suppressed, 0);

    internal void Clear()
    {
        lock (gate) lastSeen.Clear();
        Interlocked.Exchange(ref suppressed, 0);
    }
}

internal sealed class DriveSpaceCache
{
    readonly record struct Entry(bool Success, long AvailableBytes, DateTime ExpiresAt);
    readonly ConcurrentDictionary<string, Entry> entries = new(StringComparer.OrdinalIgnoreCase);
    readonly Func<string, (bool Success, long AvailableBytes)> query;
    readonly Func<DateTime> clock;
    readonly TimeSpan lifetime;

    internal DriveSpaceCache(
        TimeSpan lifetime,
        Func<string, (bool Success, long AvailableBytes)> query,
        Func<DateTime>? clock = null)
    {
        this.lifetime = lifetime;
        this.query = query;
        this.clock = clock ?? (() => DateTime.UtcNow);
    }

    internal bool TryGetAvailableBytes(string root, out long availableBytes)
    {
        var key = root.Trim().TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        var now = clock();
        if (entries.TryGetValue(key, out var cached) && cached.ExpiresAt > now)
        {
            availableBytes = cached.AvailableBytes;
            return cached.Success;
        }
        var result = query(root);
        entries[key] = new Entry(result.Success, result.AvailableBytes, now + lifetime);
        availableBytes = result.AvailableBytes;
        return result.Success;
    }

    internal void Clear() => entries.Clear();
}
