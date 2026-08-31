using System.Diagnostics;
using System.Text;

namespace SOOPLiveWinUI;

public sealed class VodProcessService : IDisposable
{
    readonly object gate = new();
    readonly Queue<string> standardErrorTail = new();
    Process? process;
    public bool IsRunning { get { lock (gate) return process is { HasExited: false }; } }
    public string LastErrorSummary
    {
        get { lock (gate) return string.Join(Environment.NewLine, standardErrorTail); }
    }
    public event Action<string>? Output;
    public event Action<int>? Exited;

    public void Start(string scriptPath, string requestPath)
    {
        lock (gate)
        {
            if (process is { HasExited: false }) throw new InvalidOperationException("VOD 작업이 이미 실행 중입니다.");
            if (!File.Exists(scriptPath)) throw new FileNotFoundException("VOD 백엔드를 찾을 수 없습니다.", scriptPath);
            standardErrorTail.Clear();
            var powershell = OperatingSystem.IsWindows() ? "powershell.exe" : "pwsh";
            var escapedScript = scriptPath.Replace("'", "''");
            var escapedRequest = requestPath.Replace("'", "''");
            var command =
                "[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false); " +
                "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); " +
                "$OutputEncoding=[System.Text.UTF8Encoding]::new($false); " +
                $"& '{escapedScript}' -RequestFile '{escapedRequest}'";
            var info = new ProcessStartInfo
            {
                FileName = powershell,
                WorkingDirectory = Path.GetDirectoryName(scriptPath) ?? AppContext.BaseDirectory,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = new UTF8Encoding(false),
                StandardErrorEncoding = new UTF8Encoding(false)
            };
            info.Environment["PYTHONUTF8"] = "1";
            info.Environment["PYTHONIOENCODING"] = "utf-8";
            info.ArgumentList.Add("-NoLogo");
            info.ArgumentList.Add("-NoProfile");
            info.ArgumentList.Add("-ExecutionPolicy");
            info.ArgumentList.Add("Bypass");
            info.ArgumentList.Add("-Command");
            info.ArgumentList.Add(command);
            var started = new Process { StartInfo = info, EnableRaisingEvents = true };
            started.OutputDataReceived += (_, e) => { if (e.Data != null) Output?.Invoke(e.Data); };
            started.ErrorDataReceived += (_, e) =>
            {
                if (e.Data == null) return;
                var safe = DiagnosticInfoService.Redact(e.Data).Trim();
                if (safe.Length == 0) return;
                lock (gate)
                {
                    standardErrorTail.Enqueue(safe);
                    while (standardErrorTail.Count > 8) standardErrorTail.Dequeue();
                }
                Output?.Invoke("[VOD STDERR] " + safe);
            };
            started.Exited += (_, _) =>
            {
                try { started.WaitForExit(); } catch { }
                int code;
                try { code = started.ExitCode; } catch { code = -1; }
                lock (gate) { if (ReferenceEquals(process, started)) process = null; }
                Exited?.Invoke(code);
                started.Dispose();
            };
            if (!started.Start()) throw new InvalidOperationException("VOD 백엔드를 시작하지 못했습니다.");
            process = started;
            started.BeginOutputReadLine();
            started.BeginErrorReadLine();
        }
    }

    public bool Stop()
    {
        Process? target;
        lock (gate) target = process;
        if (target == null || target.HasExited) return true;
        try
        {
            if (OperatingSystem.IsWindows())
            {
                using var taskkill = Process.Start(new ProcessStartInfo
                {
                    FileName = "taskkill.exe",
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    ArgumentList = { "/PID", target.Id.ToString(), "/T", "/F" }
                });
                taskkill?.WaitForExit(5000);
                if (target.WaitForExit(3000)) return true;
            }
            target.Kill(entireProcessTree: true);
            return target.WaitForExit(5000);
        }
        catch { return false; }
    }

    public void Dispose() { Stop(); lock (gate) { process?.Dispose(); process = null; } }
}
