using SOOPLiveWinUI;

static class UiStateRegression
{
    internal static void Run()
    {
        var first = SettingsSnapshot.Create(new Dictionary<string, string?>
        {
            ["quality"] = " best ",
            ["interval"] = "30"
        });
        var reordered = SettingsSnapshot.Create(new Dictionary<string, string?>
        {
            ["interval"] = "30",
            ["quality"] = "best"
        });
        var changed = SettingsSnapshot.Create(new Dictionary<string, string?>
        {
            ["interval"] = "31",
            ["quality"] = "best"
        });

        Check(first == reordered, "settings snapshot must ignore key order and outer whitespace");
        Check(first != changed, "settings snapshot must detect a real value change");
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Check(Path.GetFullPath(UiPreferences.FilePath).StartsWith(Path.GetFullPath(local), StringComparison.OrdinalIgnoreCase),
            "UI preferences must be stored below LocalAppData");
        Console.WriteLine("UI state regression tests passed.");
    }

    static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}
