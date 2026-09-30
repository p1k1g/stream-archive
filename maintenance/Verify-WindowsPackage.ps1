param(
    [string]$Root = ".\dist\stream-archive",
    [switch]$RequireCleanData,
    [string]$ArchivePath = "",
    [string]$ArchiveChecksumPath = ""
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

function Assert-Leaf {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Missing package file: $Path"
    }
}

function Assert-Directory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "Missing package directory: $Path"
    }
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

public sealed class EmbeddedIconFrame
{
    public int Width { get; set; }
    public int Height { get; set; }
    public ushort Planes { get; set; }
    public ushort BitCount { get; set; }
    public int ResourceId { get; set; }
    public byte[] ImageData { get; set; }
}

public static class WindowsIconResourceReader
{
    private const int RT_ICON = 3;
    private const int RT_GROUP_ICON = 14;
    private const uint LOAD_LIBRARY_AS_DATAFILE = 0x00000002;
    private const uint LOAD_LIBRARY_AS_IMAGE_RESOURCE = 0x00000020;

    private delegate bool EnumResNameProc(IntPtr module, IntPtr type, IntPtr name, IntPtr parameter);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryExW(string fileName, IntPtr file, uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool FreeLibrary(IntPtr module);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool EnumResourceNamesW(
        IntPtr module,
        IntPtr type,
        EnumResNameProc callback,
        IntPtr parameter);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr LoadResource(IntPtr module, IntPtr resourceInfo);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr LockResource(IntPtr resourceData);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint SizeofResource(IntPtr module, IntPtr resourceInfo);

    private sealed class ResourceName
    {
        public ushort? Id;
        public string Name;
    }

    public static EmbeddedIconFrame[] Read(string executablePath)
    {
        IntPtr module = LoadLibraryExW(
            executablePath,
            IntPtr.Zero,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE);
        if (module == IntPtr.Zero)
            throw new InvalidOperationException(
                "LoadLibraryExW failed for icon verification: " + Marshal.GetLastWin32Error());

        try
        {
            var groupNames = new List<ResourceName>();
            EnumResNameProc callback = delegate(IntPtr h, IntPtr t, IntPtr name, IntPtr p)
            {
                long raw = name.ToInt64();
                if ((((ulong)raw) >> 16) == 0)
                {
                    groupNames.Add(new ResourceName { Id = unchecked((ushort)raw) });
                }
                else
                {
                    groupNames.Add(new ResourceName { Name = Marshal.PtrToStringUni(name) });
                }
                return true;
            };

            bool enumerated = EnumResourceNamesW(
                module,
                new IntPtr(RT_GROUP_ICON),
                callback,
                IntPtr.Zero);
            GC.KeepAlive(callback);

            if (!enumerated)
                throw new InvalidOperationException(
                    "EnumResourceNamesW(RT_GROUP_ICON) failed: " + Marshal.GetLastWin32Error());
            if (groupNames.Count == 0)
                throw new InvalidDataException("Executable has no RT_GROUP_ICON resources.");

            var frames = new List<EmbeddedIconFrame>();
            foreach (ResourceName groupName in groupNames)
            {
                IntPtr allocatedName = IntPtr.Zero;
                try
                {
                    IntPtr namePtr;
                    if (groupName.Id.HasValue)
                    {
                        namePtr = new IntPtr(groupName.Id.Value);
                    }
                    else
                    {
                        allocatedName = Marshal.StringToHGlobalUni(groupName.Name);
                        namePtr = allocatedName;
                    }

                    byte[] group = ReadResource(module, namePtr, RT_GROUP_ICON);
                    ParseGroup(module, group, frames);
                }
                finally
                {
                    if (allocatedName != IntPtr.Zero)
                        Marshal.FreeHGlobal(allocatedName);
                }
            }

            return frames.ToArray();
        }
        finally
        {
            FreeLibrary(module);
        }
    }

    private static byte[] ReadResource(IntPtr module, IntPtr name, int type)
    {
        IntPtr info = FindResourceW(module, name, new IntPtr(type));
        if (info == IntPtr.Zero)
            throw new InvalidDataException(
                "FindResourceW failed for resource type " + type + ": " + Marshal.GetLastWin32Error());

        uint size = SizeofResource(module, info);
        if (size == 0)
            throw new InvalidDataException("Embedded icon resource is empty.");

        IntPtr loaded = LoadResource(module, info);
        if (loaded == IntPtr.Zero)
            throw new InvalidDataException(
                "LoadResource failed: " + Marshal.GetLastWin32Error());

        IntPtr locked = LockResource(loaded);
        if (locked == IntPtr.Zero)
            throw new InvalidDataException("LockResource returned null.");

        byte[] bytes = new byte[size];
        Marshal.Copy(locked, bytes, 0, checked((int)size));
        return bytes;
    }

    private static void ParseGroup(
        IntPtr module,
        byte[] group,
        List<EmbeddedIconFrame> frames)
    {
        if (group.Length < 6)
            throw new InvalidDataException("RT_GROUP_ICON header is truncated.");

        ushort reserved = ReadUInt16(group, 0);
        ushort type = ReadUInt16(group, 2);
        ushort count = ReadUInt16(group, 4);
        if (reserved != 0 || type != 1 || count == 0)
            throw new InvalidDataException("RT_GROUP_ICON header is invalid.");

        int expectedLength = checked(6 + count * 14);
        if (group.Length < expectedLength)
            throw new InvalidDataException("RT_GROUP_ICON directory is truncated.");

        for (int i = 0; i < count; i++)
        {
            int offset = 6 + i * 14;
            int width = group[offset] == 0 ? 256 : group[offset];
            int height = group[offset + 1] == 0 ? 256 : group[offset + 1];
            byte colorCount = group[offset + 2];
            byte entryReserved = group[offset + 3];
            ushort planes = ReadUInt16(group, offset + 4);
            ushort bitCount = ReadUInt16(group, offset + 6);
            uint bytesInRes = ReadUInt32(group, offset + 8);
            ushort resourceId = ReadUInt16(group, offset + 12);

            if (entryReserved != 0)
                throw new InvalidDataException("RT_GROUP_ICON entry reserved byte is non-zero.");
            if (width != height)
                throw new InvalidDataException(
                    "Embedded application icon frame is not square: " + width + "x" + height);

            byte[] image = ReadResource(module, new IntPtr(resourceId), RT_ICON);
            if (image.Length != bytesInRes)
                throw new InvalidDataException(
                    "RT_ICON size mismatch for " + width + "px frame: group=" +
                    bytesInRes + ", resource=" + image.Length);

            frames.Add(new EmbeddedIconFrame
            {
                Width = width,
                Height = height,
                Planes = planes,
                BitCount = bitCount,
                ResourceId = resourceId,
                ImageData = image
            });
        }
    }

    private static ushort ReadUInt16(byte[] data, int offset)
    {
        return (ushort)(data[offset] | (data[offset + 1] << 8));
    }

    private static uint ReadUInt32(byte[] data, int offset)
    {
        return (uint)(
            data[offset] |
            (data[offset + 1] << 8) |
            (data[offset + 2] << 16) |
            (data[offset + 3] << 24));
    }
}
'@

function Assert-IconBitmapSignal {
    param(
        [System.Drawing.Bitmap]$Bitmap,
        [int]$ExpectedSize
    )

    if ($Bitmap.Width -ne $ExpectedSize -or $Bitmap.Height -ne $ExpectedSize) {
        throw "StreamArchive.exe embedded icon frame decoded at $($Bitmap.Width)x$($Bitmap.Height); expected $($ExpectedSize)x$($ExpectedSize)."
    }

    $lowerSignal = 0
    $lowerPixels = 0
    for ($y = [int]($Bitmap.Height / 2); $y -lt $Bitmap.Height; $y++) {
        for ($x = 0; $x -lt $Bitmap.Width; $x++) {
            $pixel = $Bitmap.GetPixel($x, $y)
            $lowerPixels++
            if ($pixel.A -gt 0 -and
                ($pixel.R -lt 240 -or $pixel.G -lt 240 -or $pixel.B -lt 240)) {
                $lowerSignal++
            }
        }
    }

    if ($lowerPixels -eq 0 -or ($lowerSignal / [double]$lowerPixels) -lt 0.20) {
        throw "StreamArchive.exe $($ExpectedSize)px icon appears cropped/blank in the lower half: $lowerSignal / $lowerPixels visible pixels."
    }
}

function Assert-EmbeddedApplicationIcon {
    param([string]$ExecutablePath)

    $expectedSizes = @(16, 24, 32, 48, 64, 128, 256)
    $resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
    $frames = @([WindowsIconResourceReader]::Read($resolvedExecutable))

    if ($frames.Count -ne $expectedSizes.Count) {
        throw "StreamArchive.exe must embed exactly $($expectedSizes.Count) application icon frames; found $($frames.Count)."
    }

    $actualSizes = @($frames | ForEach-Object { $_.Width } | Sort-Object -Unique)
    $sizeDifference = @(Compare-Object -ReferenceObject $expectedSizes -DifferenceObject $actualSizes)
    if ($sizeDifference.Count -ne 0) {
        throw "StreamArchive.exe embedded icon sizes are invalid. Expected: $($expectedSizes -join ', '); actual: $($actualSizes -join ', ')."
    }

    foreach ($frame in $frames | Sort-Object Width) {
        if ($frame.Width -ne $frame.Height) {
            throw "StreamArchive.exe embedded icon frame is not square: $($frame.Width)x$($frame.Height)."
        }

        $imageBytes = $frame.ImageData
        $pngSignature = [byte[]](137, 80, 78, 71, 13, 10, 26, 10)
        if ($imageBytes.Length -lt $pngSignature.Length) {
            throw "StreamArchive.exe $($frame.Width)px RT_ICON resource is truncated."
        }
        for ($index = 0; $index -lt $pngSignature.Length; $index++) {
            if ($imageBytes[$index] -ne $pngSignature[$index]) {
                throw "StreamArchive.exe $($frame.Width)px RT_ICON resource is not the expected PNG frame."
            }
        }

        $stream = [System.IO.MemoryStream]::new($imageBytes, $false)
        try {
            $image = [System.Drawing.Image]::FromStream($stream, $true, $true)
            try {
                $bitmap = [System.Drawing.Bitmap]::new($image)
                try {
                    Assert-IconBitmapSignal -Bitmap $bitmap -ExpectedSize $frame.Width
                }
                finally {
                    $bitmap.Dispose()
                }
            }
            finally {
                $image.Dispose()
            }
        }
        catch {
            throw "StreamArchive.exe $($frame.Width)px embedded RT_ICON PNG could not be decoded: $($_.Exception.Message)"
        }
        finally {
            $stream.Dispose()
        }
    }
}

function Test-PackageTree {
    param(
        [string]$PackageRoot,
        [bool]$CleanData
    )

    $resolved = (Resolve-Path -LiteralPath $PackageRoot).Path
    $required = @(
        'StreamArchive.exe',
        'RUN.bat',
        'BACKUP_DATA.bat',
        'RESTORE_DATA.bat',
        'LICENSE',
        'THIRD_PARTY_NOTICES.md',
        'RELEASE_INFO.txt',
        'SHA256SUMS.txt',
        'maintenance\Backup-StreamArchiveData.ps1',
        'maintenance\Restore-StreamArchiveData.ps1',
        'docs\OPERATIONS.md'
    )
    foreach ($relative in $required) {
        Assert-Leaf (Join-Path $resolved $relative)
    }

    foreach ($relative in @('backend', 'backend\vod', 'data', 'maintenance', 'docs')) {
        Assert-Directory (Join-Path $resolved $relative)
    }

    foreach ($relative in @(
        'stream-archive-server.exe',
        'RUN_HEADLESS.bat',
        'stream-archive-icon.png',
        'stream-archive.ico',
        'stream-archive-launcher.exe',
        'RUN_WEB.bat',
        'RUN_SERVER_CONSOLE.bat',
        'Caddyfile.example',
        'docs\REVERSE_PROXY.md',
        'docs\LOCAL_LAUNCHER.md'
    )) {
        $path = Join-Path $resolved $relative
        if (Test-Path -LiteralPath $path) {
            throw "Forbidden Windows package file returned: $relative"
        }
    }

    $forbiddenNames = @(
        'streamlink.exe',
        'yt-dlp.exe',
        'ffmpeg.exe',
        'SOOP_LIVE_SETTING.ini',
        'SOOP_LIVE_CHANNELS.txt',
        'SOOP_VOD_SETTING.ini'
    )
    $forbidden = Get-ChildItem -LiteralPath $resolved -Recurse -Force -File |
        Where-Object {
            $_.Name -in $forbiddenNames -or
            $_.Name -like '*.stream-archive.claim' -or
            $_.Name -like '*.log'
        } |
        Select-Object -First 1
    if ($null -ne $forbidden) {
        throw "Forbidden bundled/runtime file: $($forbidden.FullName)"
    }

    if ($CleanData) {
        $dataRoot = Join-Path $resolved 'data'
        $dataEntry = Get-ChildItem -LiteralPath $dataRoot -Force | Select-Object -First 1
        if ($null -ne $dataEntry) {
            throw "Official package data directory must be empty: $($dataEntry.FullName)"
        }
        $database = Get-ChildItem -LiteralPath $resolved -Recurse -Force -File |
            Where-Object { $_.Name -match '\.db(?:-wal|-shm)?$' } |
            Select-Object -First 1
        if ($null -ne $database) {
            throw "Official package must not contain a runtime database: $($database.FullName)"
        }
    }

    $expected = @{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $resolved 'SHA256SUMS.txt')) {
        if ($line -match '^([0-9a-fA-F]{64})\s+(.+)$') {
            $expected[$matches[2].Trim()] = $matches[1].ToLowerInvariant()
        }
    }
    foreach ($name in @('StreamArchive.exe')) {
        if (-not $expected.ContainsKey($name)) {
            throw "checksum missing: $name"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolved $name)).Hash.ToLowerInvariant()
        if ($expected[$name] -ne $actual) {
            throw "$name checksum mismatch"
        }
    }

    Assert-EmbeddedApplicationIcon -ExecutablePath (Join-Path $resolved 'StreamArchive.exe')

    $run = Get-Content -LiteralPath (Join-Path $resolved 'RUN.bat') -Raw
    if ($run -notmatch 'StreamArchive\.exe') {
        throw 'RUN.bat does not use StreamArchive.exe'
    }
    if ($run -match 'launcher|RUN_WEB|127\.0\.0\.1|http') {
        throw 'RUN.bat must remain native-only'
    }

    $releaseInfo = Get-Content -LiteralPath (Join-Path $resolved 'RELEASE_INFO.txt') -Raw
    if ($releaseInfo -notmatch '(?m)^product=Stream Archive\s*$') {
        throw 'release metadata product missing'
    }
    if ($releaseInfo -notmatch '(?m)^version=\S+\s*$') {
        throw 'release metadata version missing'
    }

    $license = Get-Content -LiteralPath (Join-Path $resolved 'LICENSE') -Raw
    if ($license -notmatch 'GNU AFFERO GENERAL PUBLIC LICENSE') {
        throw 'AGPL license text missing from package'
    }

    Write-Host "Windows package tree verified: $resolved"
}

Test-PackageTree -PackageRoot $Root -CleanData $RequireCleanData.IsPresent

if (-not [string]::IsNullOrWhiteSpace($ArchivePath)) {
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        throw "Archive does not exist: $ArchivePath"
    }
    if ([string]::IsNullOrWhiteSpace($ArchiveChecksumPath)) {
        $ArchiveChecksumPath = "$ArchivePath.sha256"
    }
    if (-not (Test-Path -LiteralPath $ArchiveChecksumPath -PathType Leaf)) {
        throw "Archive checksum does not exist: $ArchiveChecksumPath"
    }

    $checksumLine = Get-Content -LiteralPath $ArchiveChecksumPath | Select-Object -First 1
    if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+(.+)$') {
        throw 'Archive checksum format is invalid'
    }
    $expectedArchiveHash = $matches[1].ToLowerInvariant()
    $actualArchiveHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
    if ($expectedArchiveHash -ne $actualArchiveHash) {
        throw 'Archive checksum mismatch'
    }

    $tempRoot = if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
        [System.IO.Path]::GetTempPath()
    }
    else {
        $env:RUNNER_TEMP
    }
    $scratch = Join-Path $tempRoot ("Stream Archive Release 테스트-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch -Force | Out-Null
    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $scratch -Force
        Test-PackageTree -PackageRoot $scratch -CleanData $true
    }
    finally {
        Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Windows archive verified: $ArchivePath"
}
