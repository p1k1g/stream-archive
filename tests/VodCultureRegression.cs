using System.Globalization;
using System.Text.Json;
using SOOPLiveWinUI;

static class VodCultureRegression
{
    public static void Run()
    {
        var originalCulture = CultureInfo.CurrentCulture;
        var originalUiCulture = CultureInfo.CurrentUICulture;
        try
        {
            foreach (var cultureName in new[] { "ko-KR", "en-US", "de-DE" })
            {
                var culture = CultureInfo.GetCultureInfo(cultureName);
                CultureInfo.CurrentCulture = culture;
                CultureInfo.CurrentUICulture = culture;

                var parts = VodSelectionParser.Parse("1-2,4", 4);
                if (!parts.SequenceEqual(new[] { 1, 2, 4 }))
                    throw new InvalidOperationException($"VOD selection changed under {cultureName}.");

                var line = VodEventParser.Prefix + JsonSerializer.Serialize(new
                {
                    version = 1,
                    type = "part_progress",
                    jobId = "culture",
                    percent = 12.5
                });
                if (!VodEventParser.TryParse(line, out var item) || item?.Percent != 12.5)
                    throw new InvalidOperationException($"Invariant VOD progress changed under {cultureName}.");
            }
        }
        finally
        {
            CultureInfo.CurrentCulture = originalCulture;
            CultureInfo.CurrentUICulture = originalUiCulture;
        }
        Console.WriteLine("VOD culture regression tests passed.");
    }
}
