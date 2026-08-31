namespace SOOPLiveWinUI;

public enum VodJobState
{
    Idle, Analyzing, Ready, Downloading, RefreshingAuthentication,
    Retrying, Merging, Completed, Failed, Cancelling, Cancelled
}

public sealed record VodJobRequest(
    int Version,
    string JobId,
    string VodUrl,
    IReadOnlyList<int> Parts,
    string OutputDirectory,
    string CookieMode,
    string CookieFile,
    string BrowserName,
    bool Merge,
    int MaxRetries);

public sealed record VodBackendEvent(
    int Version,
    string Type,
    string JobId,
    string Message,
    string Title,
    string Streamer,
    int Part,
    int PartCount,
    double Percent,
    string OutputFile);

public sealed record VodSettings(
    string OutputDirectory = "",
    string YtDlpPath = "",
    string FfmpegPath = "",
    string CookieMode = "SOOP_LOGIN",
    string CookieFile = "",
    string BrowserName = "firefox",
    int MaxRetries = 5,
    bool Merge = true);

public sealed record VodHistoryEntry(
    string JobId,
    DateTimeOffset CompletedAt,
    string VodUrl,
    string Title,
    string Streamer,
    string OutputFile,
    string Result);
