function ConvertTo-VodFfmpegConcatLine {
    param([string]$Path)
    $fullPath = [System.IO.Path]::GetFullPath($Path).Normalize([System.Text.NormalizationForm]::FormC)
    return "file '" + ($fullPath.Replace('\', '/').Replace("'", "'\''")) + "'"
}

function Merge-VodParts {
    param($Request, $Metadata, [string[]]$PartFiles, [string]$Ffmpeg, [string]$JobDirectory)
    if ([string]::IsNullOrWhiteSpace($Ffmpeg) -or -not (Test-Path -LiteralPath $Ffmpeg -PathType Leaf)) { throw 'ffmpeg를 찾을 수 없어 PART를 병합할 수 없습니다.' }
    $list = Join-Path $JobDirectory 'concat.txt'
    $lines = foreach ($file in $PartFiles) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "병합할 PART 파일이 없습니다: $file" }
        ConvertTo-VodFfmpegConcatLine -Path $file
    }
    [IO.File]::WriteAllLines($list, $lines, [Text.UTF8Encoding]::new($false))
    $directory = Split-Path -Parent $PartFiles[0]
    $base = '{0}_{1}' -f $Metadata.Date, (Get-SafeVodFileName $Metadata.Streamer)
    $target = Get-CollisionSafeVodPath -Directory $directory -BaseName $base -Extension '.mp4'
    Register-VodOwnedOutputPath -JobDirectory $JobDirectory -Path $target -DeleteTargetOnCleanup
    Write-VodEvent -Type 'merge_started' -Message '선택한 PART를 병합 중…'
    & $Ffmpeg '-hide_banner' '-loglevel' 'warning' '-f' 'concat' '-safe' '0' '-i' $list '-c' 'copy' '-n' $target 2>&1 | ForEach-Object { }
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $target -PathType Leaf) -or (Get-Item -LiteralPath $target).Length -lt 1MB) { throw 'FFmpeg 병합에 실패했습니다. 원본 PART 파일은 유지합니다.' }
    Complete-VodOwnedOutputPath -JobDirectory $JobDirectory -Path $target
    foreach ($file in $PartFiles) { Remove-Item -LiteralPath $file -Force }
    return $target
}
