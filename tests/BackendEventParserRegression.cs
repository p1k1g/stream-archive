using System.Text.Json;
using SOOPLiveWinUI;

static class BackendEventParserRegression
{
    static int failures;

    static void Main()
    {
        JsonEvent("recording_started");
        JsonEvent("recording_finished");
        JsonEvent("recording_stalled");
        JsonEvent("low_disk");
        JsonEvent("worker_cooldown");
        JsonEvent("channel_removed");
        JsonEvent("channel_disabled");

        Legacy("[2026-08-28 16:00:00] [INFO] RECORD START channel=채널[테스트]|= account=test_01 bno=123 title=제목[가]|= hls=best cdn=cdn host=host file=C:\\녹화[임시]\\제목|=채널.ts", "recording_started");
        Legacy("[16:01:00] 채널[테스트]|= [account=test_01] : RECORD FINISHED | duration=00:01:00 | size=10.00 MB | reason=NORMAL | file=C:\\녹화[임시]\\제목=채널.ts", "recording_finished");
        Legacy("[16:01:01] 채널[테스트]|= [account=test_01] : RECORD STALLED (90s no growth)", "recording_stalled");
        Legacy("[16:01:02] 채널[테스트]|= [account=test_01] : LOW DISK SPACE - Free=1 GB", "low_disk");
        Legacy("[16:01:03] 채널[테스트]|= [account=test_01] : WORKER COOLDOWN - seconds=30 until=2026-08-28T16:02:00Z", "worker_cooldown");
        Legacy("[16:01:04] 채널[테스트]|= [account=test_01] : CHANNEL REMOVED", "channel_removed");
        Legacy("[16:01:05] 채널[테스트]|= [account=test_01] : CHANNEL DISABLED", "channel_disabled");

        Check(!BackendEventParser.TryParse("@@SOOP_EVENT@@{broken", out _), "malformed JSON must be rejected");
        Check(!BackendEventParser.TryParse("@@SOOP_EVENT@@[]", out _), "non-object JSON must be rejected");
        Check(!BackendEventParser.TryParse("@@SOOP_EVENT@@{\"version\":2,\"type\":\"recording_started\"}", out _), "unknown JSON version must be rejected");
        Check(BackendEventParser.NormalizeRecordingFinishedReason("") == "NORMAL", "empty recorder exit reason must be normal");
        Check(BackendEventParser.NormalizeRecordingFinishedReason("RECORDER EXIT CODE=") == "NORMAL", "missing recorder exit code must be normal");
        Check(BackendEventParser.NormalizeRecordingFinishedReason("RECORDER EXIT CODE=0") == "NORMAL", "zero recorder exit code must be normal");
        Check(BackendEventParser.NormalizeRecordingFinishedReason("RECORDER EXIT CODE=1") == "RECORDER EXIT CODE=1", "non-zero recorder exit code must remain actionable");

        if (OperatingSystem.IsWindows())
        {
            const string secret = "한글[secret]|=token";
            var protectedValue = SecretProtectionService.Protect(secret);
            Check(protectedValue.StartsWith(SecretProtectionService.Prefix, StringComparison.Ordinal), "DPAPI prefix must be present");
            Check(SecretProtectionService.Unprotect(protectedValue) == secret, "DPAPI secret must round-trip for current user");
            Check(SecretProtectionService.Unprotect(secret) == secret, "legacy plaintext must remain readable for migration");
            var protectedIni = SecretProtectionService.ProtectIniSecretsForCurrentUser(
                "SOOP_PASSWORD=legacy-password\r\nCLOUDFLARE_API_KEY=legacy-key\r\nQUALITY=best\r\n");
            Check(!protectedIni.Contains("legacy-password", StringComparison.Ordinal), "imported password must be migrated before write");
            Check(!protectedIni.Contains("legacy-key", StringComparison.Ordinal), "imported API key must be migrated before write");
            Check(protectedIni.Contains("\r\nQUALITY=best\r\n", StringComparison.Ordinal), "DPAPI import migration must preserve CRLF");
            var protectedPassword = protectedIni.Split("\r\n", StringSplitOptions.RemoveEmptyEntries)
                .Single(x => x.StartsWith("SOOP_PASSWORD=", StringComparison.OrdinalIgnoreCase))["SOOP_PASSWORD=".Length..];
            Check(SecretProtectionService.Unprotect(protectedPassword) == "legacy-password", "migrated imported password must decrypt");
        }

        if (failures != 0) throw new InvalidOperationException($"{failures} parser regression test(s) failed.");
        Console.WriteLine("BackendEventParser regression tests passed.");
    }

    static void JsonEvent(string type)
    {
        var source = new Dictionary<string, object?>
        {
            ["version"] = 1,
            ["type"] = type,
            ["account"] = "test_01",
            ["name"] = "채널[테스트]|=",
            ["title"] = "제목[가]|= 한글",
            ["file"] = @"C:\녹화[임시]\제목|=채널.ts",
            ["reason"] = "오류|원인=테스트",
            ["duration"] = "00:01:00",
            ["size"] = "10.00 MB",
            ["detail"] = "상세[내용]|=",
            ["cooldownSeconds"] = 30,
            ["until"] = "2026-08-28T16:02:00Z",
            ["action"] = type.StartsWith("channel_", StringComparison.Ordinal) ? type[8..] : ""
        };
        var line = BackendEventParser.Prefix + JsonSerializer.Serialize(source);
        Check(BackendEventParser.TryParse(line, out var parsed), $"JSON {type} must parse");
        Check(parsed?.Type == type, $"JSON {type} type must round-trip");
        Check(parsed?.Name == "채널[테스트]|=", $"JSON {type} Korean/special name must round-trip");
        Check(parsed?.File == @"C:\녹화[임시]\제목|=채널.ts", $"JSON {type} path must round-trip");
    }

    static void Legacy(string line, string expectedType)
    {
        Check(BackendEventParser.TryParse(line, out var parsed), $"legacy {expectedType} must parse");
        Check(parsed?.Type == expectedType, $"legacy {expectedType} type must match");
        Check(parsed?.Name == "채널[테스트]|=", $"legacy {expectedType} Korean/special name must survive");
    }

    static void Check(bool condition, string message)
    {
        if (condition) return;
        failures++;
        Console.Error.WriteLine("FAIL: " + message);
    }
}
