using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

public static class VodSettingsStore
{
    public static string FilePath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "SOOPLiveDownloader", "vod-settings.json");

    public static VodSettings Load()
    {
        try { return File.Exists(FilePath) ? Normalize(JsonSerializer.Deserialize<VodSettings>(File.ReadAllText(FilePath, Encoding.UTF8)) ?? new()) : new(); }
        catch { return new(); }
    }

    public static void Save(VodSettings settings)
    {
        var normalized = Normalize(settings);
        var directory = Path.GetDirectoryName(FilePath)!;
        Directory.CreateDirectory(directory);
        var temporary = Path.Combine(directory, ".vod-settings." + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            File.WriteAllText(temporary, JsonSerializer.Serialize(normalized, new JsonSerializerOptions { WriteIndented = true }), new UTF8Encoding(false));
            using var _ = JsonDocument.Parse(File.ReadAllText(temporary, Encoding.UTF8));
            if (File.Exists(FilePath))
            {
                try { File.Replace(temporary, FilePath, null, true); }
                catch (PlatformNotSupportedException) { File.Move(temporary, FilePath, true); }
                catch (IOException) { File.Move(temporary, FilePath, true); }
            }
            else File.Move(temporary, FilePath);
        }
        finally { try { if (File.Exists(temporary)) File.Delete(temporary); } catch { } }
    }

    static VodSettings Normalize(VodSettings value) => value with
    {
        CookieMode = (value.CookieMode ?? "").ToUpperInvariant() switch
        {
            "FILE" => "FILE",
            "BROWSER" => "BROWSER",
            _ => "SOOP_LOGIN"
        },
        MaxRetries = Math.Clamp(value.MaxRetries, 1, 20),
        BrowserName = string.IsNullOrWhiteSpace(value.BrowserName) ? "firefox" : value.BrowserName.Trim()
    };
}
