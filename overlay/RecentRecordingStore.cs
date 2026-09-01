using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

internal sealed class RecentRecordingStore
{
    readonly string path;
    readonly int limit;
    readonly object saveGate = new();
    RecentRecordingEntry[]? pendingEntries;
    Task? saveWorker;
    int completedWriteCount;

    internal event Action<Exception>? SaveFailed;
    internal int CompletedWriteCount => Volatile.Read(ref completedWriteCount);

    internal RecentRecordingStore(string path, int limit)
    {
        this.path = Path.GetFullPath(path);
        this.limit = Math.Max(1, limit);
    }

    internal IReadOnlyList<RecentRecordingEntry> Load()
    {
        if (!File.Exists(path)) return Array.Empty<RecentRecordingEntry>();
        return (JsonSerializer.Deserialize<List<RecentRecordingEntry>>(
                File.ReadAllText(path, Encoding.UTF8)) ?? new())
            .Where(x => !string.IsNullOrWhiteSpace(x.Name))
            .OrderByDescending(x => x.EndedAt)
            .Take(limit)
            .ToArray();
    }

    internal void Save(IEnumerable<RecentRecordingEntry> entries)
    {
        // Materialize the bounded collection on the UI thread; JSON encoding and
        // disk I/O are both performed by the single coalescing worker.
        var snapshot = entries.Take(limit).ToArray();
        lock (saveGate)
        {
            pendingEntries = snapshot;
            saveWorker ??= Task.Run(ProcessPendingSavesAsync);
        }
    }

    internal async Task FlushAsync()
    {
        while (true)
        {
            Task? worker;
            lock (saveGate) worker = saveWorker;
            if (worker == null) return;
            await worker.ConfigureAwait(false);
        }
    }

    async Task ProcessPendingSavesAsync()
    {
        while (true)
        {
            await Task.Delay(250).ConfigureAwait(false);
            RecentRecordingEntry[]? entries;
            lock (saveGate)
            {
                entries = pendingEntries;
                pendingEntries = null;
            }
            if (entries != null)
            {
                try
                {
                    var content = JsonSerializer.Serialize(
                        entries,
                        new JsonSerializerOptions { WriteIndented = true });
                    AtomicReplace(content);
                    Interlocked.Increment(ref completedWriteCount);
                }
                catch (Exception ex) { SaveFailed?.Invoke(ex); }
            }
            lock (saveGate)
            {
                if (pendingEntries != null) continue;
                saveWorker = null;
                return;
            }
        }
    }

    void AtomicReplace(string content)
    {
        var directory = Path.GetDirectoryName(path) ??
            throw new InvalidOperationException("최근 녹화 기록 폴더를 확인할 수 없습니다.");
        Directory.CreateDirectory(directory);
        var temporary = Path.Combine(directory, "." + Path.GetFileName(path) + "." + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            File.WriteAllText(temporary, content, new UTF8Encoding(false));
            using var validation = JsonDocument.Parse(File.ReadAllText(temporary, Encoding.UTF8));
            if (File.Exists(path))
            {
                var backup = path + ".replace.bak";
                try
                {
                    File.Replace(temporary, path, backup, true);
                    try { File.Delete(backup); } catch { }
                }
                catch (PlatformNotSupportedException) { File.Move(temporary, path, true); }
                catch (IOException) { File.Move(temporary, path, true); }
            }
            else File.Move(temporary, path);
        }
        finally
        {
            try { if (File.Exists(temporary)) File.Delete(temporary); } catch { }
        }
    }
}
