using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

namespace SOOPLiveWinUI;

internal static class DiagnosticInfoService
{
    internal static string CreateReport(
        string version,
        bool watcherRunning,
        int recordingCount,
        int offlineCount,
        int pausedCount,
        int alertCount,
        bool backendPresent,
        bool settingsPresent,
        bool channelsPresent,
        IEnumerable<string> recentEvents)
    {
        var logSnapshot = string.Join(Environment.NewLine, recentEvents.TakeLast(50));
        return new StringBuilder()
            .AppendLine("SOOP LIVE Downloader diagnostic")
            .AppendLine("Version: " + version)
            .AppendLine("Time: " + DateTimeOffset.Now.ToString("o"))
            .AppendLine("OS: " + RuntimeInformation.OSDescription)
            .AppendLine("Runtime: " + RuntimeInformation.FrameworkDescription)
            .AppendLine("Process architecture: " + RuntimeInformation.ProcessArchitecture)
            .AppendLine("Watcher running: " + watcherRunning)
            .AppendLine($"Recording/Offline/Paused/Alert: {recordingCount}/{offlineCount}/{pausedCount}/{alertCount}")
            .AppendLine("Backend script present: " + backendPresent)
            .AppendLine("Settings present: " + settingsPresent)
            .AppendLine("Channels present: " + channelsPresent)
            .AppendLine()
            .AppendLine("Recent GUI events:")
            .AppendLine(Redact(logSnapshot))
            .ToString();
    }

    internal static string Redact(string text)
    {
        var safe = Regex.Replace(
            text, @"(?im)^.*(?:SOOP_PASSWORD|WORKER_API_KEY|API_KEY|AUTHORIZATION|COOKIE)\s*[:=].*$", "<redacted sensitive header>");
        safe = Regex.Replace(
            safe, @"(?i)(SOOP_PASSWORD|WORKER_API_KEY|API_KEY|AUTHORIZATION|COOKIE)\s*[:=]\s*[^\s;]+", "$1=<redacted>");
        safe = Regex.Replace(safe, @"(?i)(Bearer|Basic)\s+[A-Za-z0-9+/=_\-.]+", "$1 <redacted>");
        return Regex.Replace(
            safe, @"(?i)([?&](?:aid|token|key|apikey|api_key|worker_api_key|password|passwd)=)[^&\s]+", "$1<redacted>");
    }
}
