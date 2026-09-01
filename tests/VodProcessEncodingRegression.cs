using System.Collections.Concurrent;
using System.Text;
using SOOPLiveWinUI;

static class VodProcessEncodingRegression
{
    public static void Run()
    {
        if (!OperatingSystem.IsWindows())
        {
            Console.WriteLine("VOD Windows PowerShell UTF-8 round-trip skipped on non-Windows.");
            return;
        }

        var root = Path.Combine(Path.GetTempPath(), "soop-vod-한글[테스트]-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var script = Path.Combine(root, "한글 VOD 테스트.ps1");
            var request = Path.Combine(root, "요청[테스트].json");
            var source = "param([string]$RequestFile)\r\n" +
                "Write-Output '@@SOOP_VOD_EVENT@@{\"version\":1,\"type\":\"test\",\"message\":\"한글[출력]|=정상\"}'\r\n" +
                "[Console]::Error.WriteLine('한글 stderr 정상')\r\n" +
                "[Console]::Error.WriteLine('API_KEY=secret')\r\n";
            File.WriteAllText(script, source, new UTF8Encoding(encoderShouldEmitUTF8Identifier: true));
            File.WriteAllText(request, "{}", new UTF8Encoding(false));

            var lines = new ConcurrentQueue<string>();
            var exited = new TaskCompletionSource<int>(TaskCreationOptions.RunContinuationsAsynchronously);
            using var service = new VodProcessService();
            service.Output += lines.Enqueue;
            service.Exited += code => exited.TrySetResult(code);
            service.Start(script, request);
            var completed = Task.WhenAny(exited.Task, Task.Delay(TimeSpan.FromSeconds(20))).GetAwaiter().GetResult();
            if (completed != exited.Task) throw new InvalidOperationException("VOD UTF-8 subprocess timed out.");
            if (exited.Task.GetAwaiter().GetResult() != 0) throw new InvalidOperationException("VOD UTF-8 subprocess failed.");
            var output = string.Join("\n", lines);
            if (!output.Contains("한글[출력]|=정상", StringComparison.Ordinal))
                throw new InvalidOperationException("VOD Korean stdout did not round-trip as UTF-8.");
            if (!output.Contains("한글 stderr", StringComparison.Ordinal) || output.Contains("secret", StringComparison.Ordinal))
                throw new InvalidOperationException("VOD Korean stderr round-trip/redaction failed.");
        }
        finally { try { Directory.Delete(root, true); } catch { } }
        Console.WriteLine("VOD Windows PowerShell UTF-8 round-trip tests passed.");
    }
}
