using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

public sealed class VodHistoryStore
{
    readonly object gate = new();
    readonly string path;
    public VodHistoryStore(string? path = null) => this.path = path ?? Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "SOOPLiveDownloader", "vod-history.json");

    public void Append(VodHistoryEntry entry)
    {
        lock (gate)
        {
            var entries = Load().Where(x => !string.Equals(x.JobId, entry.JobId, StringComparison.Ordinal)).ToList();
            entries.Insert(0, entry);
            if (entries.Count > 200) entries.RemoveRange(200, entries.Count - 200);
            AtomicReplace(JsonSerializer.Serialize(entries, new JsonSerializerOptions { WriteIndented = true }));
        }
    }

    public IReadOnlyList<VodHistoryEntry> Load()
    {
        try { return File.Exists(path) ? JsonSerializer.Deserialize<List<VodHistoryEntry>>(File.ReadAllText(path, Encoding.UTF8)) ?? [] : []; }
        catch { return []; }
    }

    void AtomicReplace(string content)
    {
        var directory = Path.GetDirectoryName(path)!;
        Directory.CreateDirectory(directory);
        var temporary = Path.Combine(directory, ".vod-history." + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            File.WriteAllText(temporary, content, new UTF8Encoding(false));
            using var _ = JsonDocument.Parse(File.ReadAllText(temporary, Encoding.UTF8));
            if (File.Exists(path))
            {
                try { File.Replace(temporary, path, null, true); }
                catch (PlatformNotSupportedException) { File.Move(temporary, path, true); }
                catch (IOException) { File.Move(temporary, path, true); }
            }
            else File.Move(temporary, path);
        }
        finally { try { if (File.Exists(temporary)) File.Delete(temporary); } catch { } }
    }
}
