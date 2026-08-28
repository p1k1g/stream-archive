using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using System.Text.Json.Serialization;
using Windows.UI;

namespace SOOPLiveWinUI;

public sealed class ChannelStatus : INotifyPropertyChanged
{
    static readonly SolidColorBrush PausedBrush = new(Color.FromArgb(255, 240, 190, 70));
    static readonly SolidColorBrush ErrorBrush = new(Color.FromArgb(255, 240, 128, 128));
    static readonly SolidColorBrush RecordingBrush = new(Color.FromArgb(255, 66, 217, 135));
    static readonly SolidColorBrush PendingBrush = new(Color.FromArgb(255, 98, 174, 239));
    static readonly SolidColorBrush NeutralBrush = new(Color.FromArgb(255, 167, 176, 190));
    string _time = "--:--:--";
    string _name = "";
    string _status = "OFFLINE";
    string _detail = "";
    string _title = "";
    string _drive = "";
    string _displayDrive = "";
    string _filePath = "";
    string _fileName = "";
    string _account = "";
    string _sizeText = "-";
    string _elapsedText = "-";
    string _rateText = "-";
    double _rateBytesPerSecond;
    bool _isSuspended;

    public string Time { get => _time; set { if (Equals(_time, value)) return; _time = value; OnChanged(); } }
    public string Name { get => _name; set { if (Equals(_name, value)) return; _name = value; OnChanged(); } }
    public string Status
    {
        get => _status;
        set
        {
            if (Equals(_status, value)) return;
            _status = value;
            OnChanged();
            OnChanged(nameof(StatusBrush));
        }
    }

    public SolidColorBrush StatusBrush
    {
        get
        {
            if (_status.Contains("PAUSED", StringComparison.OrdinalIgnoreCase))
                return PausedBrush;

            if (_status.Contains("실패", StringComparison.OrdinalIgnoreCase) ||
                _status.Contains("ERROR", StringComparison.OrdinalIgnoreCase))
                return ErrorBrush;

            if (_status.Contains("DISK", StringComparison.OrdinalIgnoreCase) ||
                _status.Contains("주의", StringComparison.OrdinalIgnoreCase) ||
                _status.Contains("LOGIN", StringComparison.OrdinalIgnoreCase))
                return PausedBrush;

            if (_status.Contains("REC", StringComparison.OrdinalIgnoreCase))
                return RecordingBrush;

            if (_status.Contains("대기", StringComparison.OrdinalIgnoreCase))
                return PendingBrush;

            return NeutralBrush;
        }
    }
    public string Detail { get => _detail; set { if (Equals(_detail, value)) return; _detail = value; OnChanged(); } }
    public string Title { get => _title; set { if (Equals(_title, value)) return; _title = value; OnChanged(); } }
    public string Drive { get => _drive; set { if (Equals(_drive, value)) return; _drive = value; OnChanged(); } }
    public string DisplayDrive { get => _displayDrive; set { if (Equals(_displayDrive, value)) return; _displayDrive = value; OnChanged(); } }
    public string FilePath
    {
        get => _filePath;
        set
        {
            if (Equals(_filePath, value)) return;
            _filePath = value;
            OnChanged();
            var nextName = "";
            try { nextName = string.IsNullOrWhiteSpace(value) ? "" : Path.GetFileName(value); } catch { }
            if (!Equals(_fileName, nextName))
            {
                _fileName = nextName;
                OnChanged(nameof(FileName));
            }
        }
    }
    public string FileName => _fileName;
    public string Account { get => _account; set { if (Equals(_account, value)) return; _account = value; OnChanged(); } }
    public string SizeText { get => _sizeText; set { if (Equals(_sizeText, value)) return; _sizeText = value; OnChanged(); } }
    public string ElapsedText { get => _elapsedText; set { if (Equals(_elapsedText, value)) return; _elapsedText = value; OnChanged(); } }
    public string RateText { get => _rateText; set { if (Equals(_rateText, value)) return; _rateText = value; OnChanged(); } }
    public double RateBytesPerSecond { get => _rateBytesPerSecond; set { if (Equals(_rateBytesPerSecond, value)) return; _rateBytesPerSecond = value; } }
    public bool IsSuspended { get => _isSuspended; set { if (Equals(_isSuspended, value)) return; _isSuspended = value; OnChanged(); } }

    public event PropertyChangedEventHandler? PropertyChanged;
    void OnChanged([CallerMemberName] string? n = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(n));
}


public sealed class EditableChannel : INotifyPropertyChanged
{
    bool _enabled = true;
    string _name = "";
    string _account = "";
    string _outDir = "";

    public bool Enabled { get => _enabled; set { if (Equals(_enabled, value)) return; _enabled = value; OnChanged(); OnChanged(nameof(EnabledText)); } }
    public string Name { get => _name; set { if (Equals(_name, value)) return; _name = value; OnChanged(); } }
    public string Account { get => _account; set { if (Equals(_account, value)) return; _account = value; OnChanged(); } }
    public string OutDir { get => _outDir; set { if (Equals(_outDir, value)) return; _outDir = value; OnChanged(); OnChanged(nameof(OutputDisplay)); } }
    public string EnabledText => Enabled ? "● 활성" : "○ 비활성";
    public string OutputDisplay => string.IsNullOrWhiteSpace(OutDir) ? "기본 경로" : OutDir;

    public event PropertyChangedEventHandler? PropertyChanged;
    void OnChanged([CallerMemberName] string? n = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(n));
}

public sealed class RecentRecordingEntry
{
    public DateTime EndedAt { get; set; }
    public string Account { get; set; } = "";
    public string Name { get; set; } = "";
    public string Title { get; set; } = "";
    public string File { get; set; } = "";
    public string Duration { get; set; } = "";
    public string Size { get; set; } = "";
    public string Reason { get; set; } = "";

    [JsonIgnore] public string EndedAtText => EndedAt.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss");
    [JsonIgnore] public string FileName
    {
        get { try { return Path.GetFileName(File); } catch { return File; } }
    }
    [JsonIgnore] public string StateText => System.IO.File.Exists(File) ? "파일 있음" : "파일 없음";
}
