using System.Diagnostics;
using System.Text;

namespace SOOPLiveWinUI;

public sealed class BackendProcessService : IDisposable
{
    Process? _process;
    readonly object _outputGate = new();
    readonly Queue<string> _recentOutput = new();
    TaskCompletionSource<bool>? _startupCompletion;

    public enum StartupState
    {
        Ready,
        StillStarting,
        Exited
    }

    public bool IsRunning
    {
        get
        {
            try { return _process is { HasExited: false }; }
            catch { return false; }
        }
    }
    public int? ProcessId => IsRunning ? _process?.Id : null;
    public int? LastExitCode { get; private set; }

    public event Action<string>? Output;
    public event Action<int>? Exited;

    public void Start(string backendDir)
    {
        if (IsRunning) return;

        try { _process?.Dispose(); } catch { }
        _process = null;

        var ps1 = Path.Combine(backendDir, "SOOP_LIVE.ps1");
        if (!File.Exists(ps1))
            throw new FileNotFoundException("SOOP_LIVE.ps1을 찾을 수 없습니다.", ps1);

        var escapedPs1 = ps1.Replace("'", "''");
        var command =
            "[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false); " +
            "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); " +
            "$OutputEncoding=[System.Text.UTF8Encoding]::new($false); " +
            $"& '{escapedPs1}'";

        lock (_outputGate)
            _recentOutput.Clear();
        LastExitCode = null;
        _startupCompletion = new TaskCompletionSource<bool>(
            TaskCreationOptions.RunContinuationsAsynchronously);

        var process = new Process
        {
            EnableRaisingEvents = true,
            StartInfo = new ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments = $"-NoLogo -NoProfile -ExecutionPolicy Bypass -Command \"{command}\"",
                WorkingDirectory = backendDir,
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
                StandardOutputEncoding = new UTF8Encoding(false),
                StandardErrorEncoding = new UTF8Encoding(false)
            }
        };

        process.OutputDataReceived += (_, e) =>
        {
            if (e.Data != null)
                CaptureOutput(e.Data);
        };
        process.ErrorDataReceived += (_, e) =>
        {
            if (e.Data != null)
                CaptureOutput("[ERROR] " + e.Data);
        };
        process.Exited += (_, _) =>
        {
            if (!ReferenceEquals(_process, process))
                return;
            var code = -1;
            try { code = process.ExitCode; } catch { }
            LastExitCode = code;
            _startupCompletion?.TrySetResult(false);
            Exited?.Invoke(code);
        };

        try
        {
            // Publish ownership before Start so an immediately exiting process
            // cannot outrun the Exited handler and lose its exit information.
            _process = process;
            process.Start();
            process.BeginOutputReadLine();
            process.BeginErrorReadLine();
        }
        catch
        {
            try
            {
                if (!process.HasExited)
                    process.Kill(entireProcessTree: true);
            }
            catch { }
            try { process.Dispose(); } catch { }
            if (ReferenceEquals(_process, process))
                _process = null;
            _startupCompletion?.TrySetResult(false);
            throw;
        }
    }

    void CaptureOutput(string line)
    {
        lock (_outputGate)
        {
            _recentOutput.Enqueue(line);
            while (_recentOutput.Count > 40)
                _recentOutput.Dequeue();
        }

        if (line.Contains("SOOP LIVE WATCHER", StringComparison.OrdinalIgnoreCase))
            _startupCompletion?.TrySetResult(true);

        Output?.Invoke(line);
    }

    public string GetRecentOutput(int maxLines = 10)
    {
        lock (_outputGate)
        {
            return string.Join(
                Environment.NewLine,
                _recentOutput.TakeLast(Math.Max(1, maxLines)));
        }
    }

    public async Task<StartupState> WaitForStartupAsync(TimeSpan timeout)
    {
        var completion = _startupCompletion;
        if (completion == null)
            return IsRunning ? StartupState.StillStarting : StartupState.Exited;

        var finished = await Task.WhenAny(completion.Task, Task.Delay(timeout));
        if (finished == completion.Task)
            return await completion.Task ? StartupState.Ready : StartupState.Exited;

        return IsRunning ? StartupState.StillStarting : StartupState.Exited;
    }

    public void StopNow()
    {
        var proc=_process;
        if(proc==null)return;
        try{if(proc.HasExited)return;}catch{return;}
        TryTerminateProcessTree(proc,7000);
    }

    public async Task StopAsync()
    {
        var proc=_process;
        if(proc==null)return;
        try{if(proc.HasExited)return;}catch{return;}
        await Task.Run(()=>TryTerminateProcessTree(proc,7000));
    }

    static bool TryTerminateProcessTree(Process proc,int timeoutMs)
    {
        int pid;
        try{proc.Refresh();if(proc.HasExited)return true;pid=proc.Id;}catch{return true;}
        var taskkillOk=false;
        try
        {
            using var killer=Process.Start(new ProcessStartInfo{
                FileName="taskkill.exe",Arguments=$"/PID {pid} /T /F",UseShellExecute=false,
                CreateNoWindow=true,RedirectStandardOutput=true,RedirectStandardError=true});
            if(killer!=null && killer.WaitForExit(timeoutMs)) taskkillOk=killer.ExitCode==0;
        }catch{}
        try{proc.Refresh();if(proc.HasExited)return true;}catch{return true;}
        try{proc.Kill(entireProcessTree:true);if(proc.WaitForExit(5000))return true;}catch{}
        try{proc.Refresh();return proc.HasExited;}catch{return taskkillOk;}
    }

    public void Dispose()
    {
        StopNow();
        try { _process?.Dispose(); } catch { }
        _process = null;
    }
}
