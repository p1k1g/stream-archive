using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

internal sealed class RecentRecordingStore
{
    readonly string path;
    readonly int limit;

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
        var json = JsonSerializer.Serialize(
            entries.Take(limit).ToArray(),
            new JsonSerializerOptions { WriteIndented = true });
        AtomicReplace(json);
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
