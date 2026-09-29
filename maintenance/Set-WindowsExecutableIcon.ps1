param(
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,
    [Parameter(Mandatory = $true)]
    [string]$SourceImagePath,
    [Parameter(Mandatory = $true)]
    [string]$GeneratedIconPath
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

internal static class NativeResource {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr BeginUpdateResource(string fileName, bool deleteExistingResources);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern bool UpdateResource(
        IntPtr update,
        IntPtr type,
        IntPtr name,
        ushort language,
        byte[] data,
        uint dataSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    internal static extern bool EndUpdateResource(IntPtr update, bool discard);
}
'@

function Resource-Id([int]$Value) {
    return [IntPtr]::new($Value)
}

function New-MultiSizeIcon {
    param(
        [string]$ImagePath,
        [string]$OutputPath
    )

    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $source = [System.Drawing.Image]::FromFile((Resolve-Path -LiteralPath $ImagePath).Path)
    try {
        $images = @()
        foreach ($size in $sizes) {
            $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                try {
                    $graphics.Clear([System.Drawing.Color]::Transparent)
                    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                    $graphics.DrawImage($source, 0, 0, $size, $size)
                }
                finally {
                    $graphics.Dispose()
                }

                $stream = New-Object System.IO.MemoryStream
                try {
                    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
                    $images += ,$stream.ToArray()
                }
                finally {
                    $stream.Dispose()
                }
            }
            finally {
                $bitmap.Dispose()
            }
        }

        $directorySize = 6 + (16 * $images.Count)
        $offset = $directorySize
        $output = New-Object System.IO.MemoryStream
        $writer = New-Object System.IO.BinaryWriter $output
        try {
            $writer.Write([uint16]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]$images.Count)
            for ($i = 0; $i -lt $images.Count; $i++) {
                $size = $sizes[$i]
                $image = $images[$i]
                $writer.Write([byte]($(if ($size -eq 256) { 0 } else { $size })))
                $writer.Write([byte]($(if ($size -eq 256) { 0 } else { $size })))
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([uint16]1)
                $writer.Write([uint16]32)
                $writer.Write([uint32]$image.Length)
                $writer.Write([uint32]$offset)
                $offset += $image.Length
            }
            foreach ($image in $images) {
                $writer.Write($image)
            }
            $writer.Flush()

            $parent = Split-Path -Parent $OutputPath
            if (-not [string]::IsNullOrWhiteSpace($parent)) {
                New-Item -ItemType Directory -Path $parent -Force | Out-Null
            }
            [System.IO.File]::WriteAllBytes($OutputPath, $output.ToArray())
        }
        finally {
            $writer.Dispose()
            $output.Dispose()
        }
    }
    finally {
        $source.Dispose()
    }
}

$exe = (Resolve-Path -LiteralPath $ExecutablePath).Path
New-MultiSizeIcon -ImagePath $SourceImagePath -OutputPath $GeneratedIconPath
$ico = (Resolve-Path -LiteralPath $GeneratedIconPath).Path
$bytes = [System.IO.File]::ReadAllBytes($ico)

if ($bytes.Length -lt 6) { throw 'ICO file is truncated.' }
$reserved = [BitConverter]::ToUInt16($bytes, 0)
$type = [BitConverter]::ToUInt16($bytes, 2)
$count = [BitConverter]::ToUInt16($bytes, 4)
if ($reserved -ne 0 -or $type -ne 1 -or $count -lt 1) {
    throw 'ICO header is invalid.'
}
if ($bytes.Length -lt 6 + (16 * $count)) {
    throw 'ICO directory is truncated.'
}

$entries = @()
for ($i = 0; $i -lt $count; $i++) {
    $offset = 6 + (16 * $i)
    $size = [BitConverter]::ToUInt32($bytes, $offset + 8)
    $imageOffset = [BitConverter]::ToUInt32($bytes, $offset + 12)
    if ($size -lt 1 -or ($imageOffset + $size) -gt $bytes.Length) {
        throw "ICO image $i is out of bounds."
    }
    $image = New-Object byte[] $size
    [Array]::Copy($bytes, [int]$imageOffset, $image, 0, [int]$size)
    $entries += [pscustomobject]@{
        Width = $bytes[$offset]
        Height = $bytes[$offset + 1]
        ColorCount = $bytes[$offset + 2]
        Reserved = $bytes[$offset + 3]
        Planes = [BitConverter]::ToUInt16($bytes, $offset + 4)
        BitCount = [BitConverter]::ToUInt16($bytes, $offset + 6)
        Size = $size
        Image = $image
        ResourceId = $i + 1
    }
}

$update = [NativeResource]::BeginUpdateResource($exe, $false)
if ($update -eq [IntPtr]::Zero) {
    throw "BeginUpdateResource failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}

$committed = $false
try {
    foreach ($entry in $entries) {
        if (-not [NativeResource]::UpdateResource(
            $update,
            (Resource-Id 3),
            (Resource-Id $entry.ResourceId),
            0,
            $entry.Image,
            [uint32]$entry.Size)) {
            throw "UpdateResource RT_ICON failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
    }

    $group = New-Object byte[] (6 + (14 * $entries.Count))
    [BitConverter]::GetBytes([uint16]0).CopyTo($group, 0)
    [BitConverter]::GetBytes([uint16]1).CopyTo($group, 2)
    [BitConverter]::GetBytes([uint16]$entries.Count).CopyTo($group, 4)
    for ($i = 0; $i -lt $entries.Count; $i++) {
        $entry = $entries[$i]
        $offset = 6 + (14 * $i)
        $group[$offset] = $entry.Width
        $group[$offset + 1] = $entry.Height
        $group[$offset + 2] = $entry.ColorCount
        $group[$offset + 3] = $entry.Reserved
        [BitConverter]::GetBytes([uint16]$entry.Planes).CopyTo($group, $offset + 4)
        [BitConverter]::GetBytes([uint16]$entry.BitCount).CopyTo($group, $offset + 6)
        [BitConverter]::GetBytes([uint32]$entry.Size).CopyTo($group, $offset + 8)
        [BitConverter]::GetBytes([uint16]$entry.ResourceId).CopyTo($group, $offset + 12)
    }

    if (-not [NativeResource]::UpdateResource(
        $update,
        (Resource-Id 14),
        (Resource-Id 1),
        0,
        $group,
        [uint32]$group.Length)) {
        throw "UpdateResource RT_GROUP_ICON failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    if (-not [NativeResource]::EndUpdateResource($update, $false)) {
        throw "EndUpdateResource failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $committed = $true
}
finally {
    if (-not $committed -and $update -ne [IntPtr]::Zero) {
        [void][NativeResource]::EndUpdateResource($update, $true)
    }
}

Write-Host "Generated multi-size Windows icon: $ico"
Write-Host "Embedded Windows application icon: $exe"
