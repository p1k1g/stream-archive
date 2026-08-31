using System.Diagnostics;

namespace SOOPLiveWinUI;

public sealed class VodProcessService : IDisposable
{
    readonly object gate = new();
    Process? process;
    public bool IsRunning { get { lock (gate) return process is { HasExited: false }; } }
    public event Action<string>? Output;
    public event Action<int>? Exited;

    public void Start(string scriptPath, string requestPath)
    {
        lock (gate)
        {
            if (process is { HasExited: false }) throw new InvalidOperationException("VOD 작업이 이미 실행 중입니다.");
            if (!File.Exists(scriptPath)) throw new FileNotFoundException("VOD 백엔드를 찾을 수 없습니다.", scriptPath);
            var powershell = OperatingSystem.IsWindows() ? "powershell.exe" : "pwsh";
            var info = new ProcessStartInfo
            {
                FileName = powershell,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            info.ArgumentList.Add("-NoProfile");
            info.ArgumentList.Add("-ExecutionPolicy");
            info.ArgumentList.Add("Bypass");
            info.ArgumentList.Add("-File");
            info.ArgumentList.Add(scriptPath);
            info.ArgumentList.Add("-RequestFile");
            info.ArgumentList.Add(requestPath);
            var started = new Process { StartInfo = info, EnableRaisingEvents = true };
            started.OutputDataReceived += (_, e) => { if (e.Data != null) Output?.Invoke(e.Data); };
            started.ErrorDataReceived += (_, e) => { if (e.Data != null) Output?.Invoke("[VOD STDERR] " + e.Data); };
            started.Exited += (_, _) =>
            {
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
