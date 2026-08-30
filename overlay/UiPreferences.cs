using System.Text;
using System.Text.Json;

namespace SOOPLiveWinUI;

public sealed class UiPreferences
{
    public string CloseAction { get; set; } = "ASK";
    public string LastView { get; set; } = "dashboard";
    public int WindowWidth { get; set; } = 1280;
    public int WindowHeight { get; set; } = 820;
    public string UiDensity { get; set; } = "NORMAL";

    public static string FilePath =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "SOOPLiveDownloader",
            "ui-state.json");

    static string LegacyFilePath =>
        Path.Combine(AppContext.BaseDirectory, "SOOPLiveWinUI.user.json");

    public static UiPreferences Load()
    {
        try
        {
            var source = File.Exists(FilePath) ? FilePath : LegacyFilePath;
            if (!File.Exists(source))
                return new UiPreferences();

            var loaded = JsonSerializer.Deserialize<UiPreferences>(
                File.ReadAllText(source, Encoding.UTF8)) ?? new UiPreferences();
            loaded.Normalize();
            if (!string.Equals(source, FilePath, StringComparison.OrdinalIgnoreCase))
                loaded.Save();
            return loaded;
        }
        catch
        {
            return new UiPreferences();
        }
    }

    public void Save()
    {
        try
        {
            Normalize();
            var directory = Path.GetDirectoryName(FilePath)!;
            Directory.CreateDirectory(directory);
            var temporary = Path.Combine(directory, ".ui-state." + Guid.NewGuid().ToString("N") + ".tmp");
            var json = JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true });
            try
            {
                File.WriteAllText(temporary, json, new UTF8Encoding(false));
                using var validation = JsonDocument.Parse(File.ReadAllText(temporary, Encoding.UTF8));
                if (File.Exists(FilePath))
                {
                    var backup = FilePath + ".replace.bak";
                    try
                    {
                        File.Replace(temporary, FilePath, backup, true);
                        try { File.Delete(backup); } catch { }
                    }
                    catch (PlatformNotSupportedException) { File.Move(temporary, FilePath, true); }
                    catch (IOException) { File.Move(temporary, FilePath, true); }
                }
                else File.Move(temporary, FilePath);
            }
            finally
            {
                try { if (File.Exists(temporary)) File.Delete(temporary); } catch { }
            }
        }
        catch { }
    }

    void Normalize()
    {
        if (WindowWidth is < 800 or > 7680) WindowWidth = 1280;
        if (WindowHeight is < 600 or > 4320) WindowHeight = 820;
        if (LastView is not ("dashboard" or "channels" or "settings" or "recent" or "logs"))
            LastView = "dashboard";
        var density = (UiDensity ?? "").ToUpperInvariant();
        UiDensity = density is "COMPACT" or "COMFORTABLE"
            ? density
            : "NORMAL";
    }
}
