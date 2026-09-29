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

public static class NativeResource {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr BeginUpdateResource(string fileName, bool deleteExistingResources);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool UpdateResource(
        IntPtr update,
        IntPtr type,
        IntPtr name,
        ushort language,
        byte[] data,
        uint dataSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool EndUpdateResource(IntPtr update, bool discard);
}
'@

function Resource-Id([int]$Value) {
    return [IntPtr]::new($Value)
}

function New-IconDibData {
    param(
        [System.Drawing.Bitmap]$Bitmap,
        [int]$Size
    )

    $pixelBytes = $Size * $Size * 4
    $maskStride = [int]([Math]::Ceiling($Size / 32.0) * 4)
    $maskBytes = $maskStride * $Size

    $stream = New-Object System.IO.MemoryStream
    $writer = New-Object System.IO.BinaryWriter $stream
    try {
        # RT_ICON/ICO entries use a DIB. biHeight is doubled because the
        # XOR bitmap is followed by the 1-bpp AND mask.
        $writer.Write([uint32]40)
        $writer.Write([int32]$Size)
        $writer.Write([int32]($Size * 2))
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]0)
        $writer.Write([uint32]$pixelBytes)
        $writer.Write([int32]0)
        $writer.Write([int32]0)
        $writer.Write([uint32]0)
        $writer.Write([uint32]0)

        # DIB scanlines are bottom-up and pixels are BGRA.
        for ($y = $Size - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $Size; $x++) {
                $pixel = $Bitmap.GetPixel($x, $y)
                $writer.Write([byte]$pixel.B)
                $writer.Write([byte]$pixel.G)
                $writer.Write([byte]$pixel.R)
                $writer.Write([byte]$pixel.A)
            }
        }

        # A zero AND mask lets the 32-bpp alpha channel own transparency.
        $writer.Write([byte[]](New-Object byte[] $maskBytes))
        $writer.Flush()
        return $stream.ToArray()
    }
    finally {
        $writer.Dispose()
        $stream.Dispose()
    }
}

function New-MultiSizeIcon {
    param(
        [string]$ImagePath,
        [string]$OutputPath
    )

    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $source = [System.Drawing.Image]::FromFile((Resolve-Path -LiteralPath $ImagePath).Path)
    try {
        $entries = @()
        foreach ($size in $sizes) {
            $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                try {
                    $graphics.Clear([System.Drawing.Color]::Transparent)
                    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
                    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                    $graphics.DrawImage($source, 0, 0, $size, $size)
                }
                finally {
                    $graphics.Dispose()
                }

                $dib = New-IconDibData -Bitmap $bitmap -Size $size
                $entries += [pscustomobject]@{
                    Size = $size
                    Data = $dib
                }
            }
            finally {
                $bitmap.Dispose()
            }
        }

        $directorySize = 6 + (16 * $entries.Count)
        $dataOffset = $directorySize
        $output = New-Object System.IO.MemoryStream
        $writer = New-Object System.IO.BinaryWriter $output
        try {
            $writer.Write([uint16]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]$entries.Count)
            foreach ($entry in $entries) {
                $encodedSize = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
                $writer.Write([byte]$encodedSize)
                $writer.Write([byte]$encodedSize)
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([uint16]1)
                $writer.Write([uint16]32)
                $writer.Write([uint32]$entry.Data.Length)
                $writer.Write([uint32]$dataOffset)
                $dataOffset += $entry.Data.Length
            }
            foreach ($entry in $entries) {
                $writer.Write([byte[]]$entry.Data)
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

function Assert-GeneratedIconUsesDibFrames {
    param([string]$IconPath)

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $IconPath).Path)
    $expectedSizes = @(16, 24, 32, 48, 64, 128, 256)
    $count = [BitConverter]::ToUInt16($bytes, 4)
    if ($count -ne $expectedSizes.Count) {
        throw "ICO frame count mismatch: expected $($expectedSizes.Count), got $count"
    }

    for ($i = 0; $i -lt $count; $i++) {
        $entryOffset = 6 + (16 * $i)
        $dataSize = [BitConverter]::ToUInt32($bytes, $entryOffset + 8)
        $dataOffset = [BitConverter]::ToUInt32($bytes, $entryOffset + 12)
        if (($dataOffset + $dataSize) -gt $bytes.Length) {
            throw "ICO frame $i is out of bounds."
        }

        $size = $expectedSizes[$i]
        if ([BitConverter]::ToUInt32($bytes, $dataOffset) -ne 40) {
            throw "ICO frame $size must use a BITMAPINFOHEADER DIB, not a PNG payload."
        }
        if ([BitConverter]::ToInt32($bytes, $dataOffset + 4) -ne $size) {
            throw "ICO frame width mismatch for $size."
        }
        if ([BitConverter]::ToInt32($bytes, $dataOffset + 8) -ne ($size * 2)) {
            throw "ICO frame height/mask contract mismatch for $size."
        }
        if ([BitConverter]::ToUInt16($bytes, $dataOffset + 12) -ne 1 -or
            [BitConverter]::ToUInt16($bytes, $dataOffset + 14) -ne 32) {
            throw "ICO frame pixel format mismatch for $size."
        }
    }
}

function Assert-EmbeddedExecutableIcon {
    param(
        [string]$ExecutablePath,
        [string]$SourceImagePath
    )

    $icon = [System.Drawing.Icon]::ExtractAssociatedIcon((Resolve-Path -LiteralPath $ExecutablePath).Path)
    if ($null -eq $icon) {
        throw 'Windows could not extract an associated icon from StreamArchive.exe.'
    }

    try {
        $bitmap = $icon.ToBitmap()
        try {
            $lowerSignal = 0
            $lowerPixels = 0
            for ($y = [int]($bitmap.Height / 2); $y -lt $bitmap.Height; $y++) {
                for ($x = 0; $x -lt $bitmap.Width; $x++) {
                    $pixel = $bitmap.GetPixel($x, $y)
                    $lowerPixels++
                    if ($pixel.A -gt 0 -and
                        ($pixel.R -lt 240 -or $pixel.G -lt 240 -or $pixel.B -lt 240)) {
                        $lowerSignal++
                    }
                }
            }

            if ($lowerPixels -eq 0 -or ($lowerSignal / [double]$lowerPixels) -lt 0.20) {
                throw "Embedded icon lower half appears blank/cropped: $lowerSignal of $lowerPixels pixels contain visible color."
            }

            $source = [System.Drawing.Image]::FromFile((Resolve-Path -LiteralPath $SourceImagePath).Path)
            try {
                $reference = New-Object System.Drawing.Bitmap $bitmap.Width, $bitmap.Height, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
                try {
                    $graphics = [System.Drawing.Graphics]::FromImage($reference)
                    try {
                        $graphics.Clear([System.Drawing.Color]::Transparent)
                        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
                        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                        $graphics.DrawImage($source, 0, 0, $reference.Width, $reference.Height)
                    }
                    finally {
                        $graphics.Dispose()
                    }

                    [long]$difference = 0
                    [long]$samples = 0
                    for ($y = 0; $y -lt $bitmap.Height; $y++) {
                        for ($x = 0; $x -lt $bitmap.Width; $x++) {
                            $actual = $bitmap.GetPixel($x, $y)
                            $expected = $reference.GetPixel($x, $y)
                            $difference += [Math]::Abs([int]$actual.R - [int]$expected.R)
                            $difference += [Math]::Abs([int]$actual.G - [int]$expected.G)
                            $difference += [Math]::Abs([int]$actual.B - [int]$expected.B)
                            $samples += 3
                        }
                    }
                    $meanDifference = $difference / [double]$samples
                    if ($meanDifference -gt 24.0) {
                        throw "Embedded icon does not match the canonical artwork closely enough: mean RGB difference $([Math]::Round($meanDifference, 2))."
                    }
                    Write-Host "Verified embedded icon source similarity: mean RGB difference $([Math]::Round($meanDifference, 2))"
                }
                finally {
                    $reference.Dispose()
                }
            }
            finally {
                $source.Dispose()
            }

            Write-Host "Verified embedded icon lower-half coverage: $lowerSignal / $lowerPixels"
        }
        finally {
            $bitmap.Dispose()
        }
    }
    finally {
        $icon.Dispose()
    }
}

$exe = (Resolve-Path -LiteralPath $ExecutablePath).Path
New-MultiSizeIcon -ImagePath $SourceImagePath -OutputPath $GeneratedIconPath
Assert-GeneratedIconUsesDibFrames -IconPath $GeneratedIconPath
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

Assert-EmbeddedExecutableIcon -ExecutablePath $exe -SourceImagePath $SourceImagePath
Write-Host "Generated multi-size Windows icon with native DIB frames: $ico"
Write-Host "Embedded and verified Windows application icon: $exe"
