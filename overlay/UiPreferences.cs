using System.Text.Json;

namespace SOOPLiveWinUI;

public sealed class UiPreferences
{
    public string CloseAction { get; set; } = "ASK";

    public static string FilePath =>
        Path.Combine(AppContext.BaseDirectory, "SOOPLiveWinUI.user.json");

    public static UiPreferences Load()
    {
        try
        {
            if (!File.Exists(FilePath))
                return new UiPreferences();

            return JsonSerializer.Deserialize<UiPreferences>(
                File.ReadAllText(FilePath)) ?? new UiPreferences();
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
            File.WriteAllText(
                FilePath,
                JsonSerializer.Serialize(this, new JsonSerializerOptions
                {
                    WriteIndented = true
                }));
        }
        catch { }
    }
}
