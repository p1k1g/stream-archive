using System.Text;

namespace SOOPLiveWinUI;

public static class IniService
{
    public static Dictionary<string, string> Read(string path)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        if (!File.Exists(path)) return result;

        foreach (var raw in File.ReadAllLines(path, Encoding.UTF8))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#') || line.StartsWith(';') || line.StartsWith('['))
                continue;

            var idx = line.IndexOf('=');
            if (idx <= 0) continue;

            result[line[..idx].Trim()] = line[(idx + 1)..].Trim();
        }
        return result;
    }
}
