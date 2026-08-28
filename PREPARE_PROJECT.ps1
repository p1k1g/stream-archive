$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$generatedRoot = Join-Path $root '.generated'
$soopDir = Join-Path $generatedRoot 'SOOPLiveWinUI'
$overlayDir = Join-Path $root 'overlay'
$backendDir = Join-Path $root 'backend'

Write-Host '========================================'
Write-Host ' SOOP LIVE WinUI 3 fix47 Project Prep'
Write-Host ' UNPACKAGED / SINGLE PROJECT'
Write-Host '========================================'
Write-Host ''

if (-not (Get-Command dotnet.exe -ErrorAction SilentlyContinue)) {
    throw '.NET SDK not found.'
}

Write-Host '[1/5] Checking official WinUI template pack...'
$templateList = (& dotnet new list winui 2>&1) -join "`n"
if ($LASTEXITCODE -ne 0 -or $templateList -notmatch 'WinUI Blank App') {
    dotnet new install Microsoft.WindowsAppSDK.WinUI.CSharp.Templates
    if ($LASTEXITCODE -ne 0) {
        throw 'Failed to install WinUI template pack.'
    }
}
else {
    Write-Host '[OK] Installed WinUI template is ready.'
}

Write-Host '[2/5] Cleaning generated project...'
if (Test-Path $generatedRoot) {
    Remove-Item $generatedRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $generatedRoot -Force | Out-Null

Write-Host '[3/5] Creating official SOOP WinUI base only...'
dotnet new winui --dotnet-version net8.0 -n SOOPLiveWinUI -o $soopDir --force
if ($LASTEXITCODE -ne 0) {
    throw 'SOOP generation failed.'
}

function Set-ProjectProperties([string]$ProjectFile) {
    [xml]$xml = Get-Content $ProjectFile -Raw
    $pg = $xml.Project.PropertyGroup | Select-Object -First 1

    function Set-Prop([string]$Name, [string]$Value) {
        $node = $pg.SelectSingleNode($Name)
        if ($null -eq $node) {
            $node = $xml.CreateElement($Name)
            [void]$pg.AppendChild($node)
        }
        $node.InnerText = $Value
    }

    Set-Prop 'WindowsPackageType' 'None'
    Set-Prop 'EnableWinAppRunSupport' 'false'
    Set-Prop 'PublishTrimmed' 'false'
    Set-Prop 'ApplicationIcon' 'Assets\SOOPLiveDownloader.ico'
    Set-Prop 'Version' '1.2.0-preview1-fix47'
    Set-Prop 'InformationalVersion' '1.2.0-preview1-fix47'

    # Do NOT set UseWindowsForms=true in a WinUI project.
    # That imports WindowsDesktop/WPF XAML targets and causes App.xaml to be
    # compiled as WPF (MC6000: PresentationCore/PresentationFramework).
    #
    # We only need System.Windows.Forms types for NotifyIcon, so add the
    # WindowsForms shared-framework reference directly without enabling WPF.
    $existing = $xml.Project.ItemGroup.FrameworkReference |
        Where-Object { $_.Include -eq 'Microsoft.WindowsDesktop.App.WindowsForms' }

    if ($null -eq $existing) {
        $ig = $xml.CreateElement('ItemGroup')
        $fr = $xml.CreateElement('FrameworkReference')
        $fr.SetAttribute('Include', 'Microsoft.WindowsDesktop.App.WindowsForms')
        [void]$ig.AppendChild($fr)
        [void]$xml.Project.AppendChild($ig)
    }

    # Ensure custom tray/window assets are physically present beside the EXE.
    foreach ($asset in @(
        'Assets\SOOPLiveDownloader.ico',
        'Assets\SOOPLiveDownloader.png'
    )) {
        $exists = $false

        foreach ($ig0 in $xml.Project.ItemGroup) {
            foreach ($content0 in $ig0.Content) {
                if ($content0.Include -eq $asset) {
                    $exists = $true
                    break
                }
            }
            if ($exists) { break }
        }

        if (-not $exists) {
            $ig = $xml.CreateElement('ItemGroup')
            $content = $xml.CreateElement('Content')
            $content.SetAttribute('Include', $asset)

            $copyOut = $xml.CreateElement('CopyToOutputDirectory')
            $copyOut.InnerText = 'PreserveNewest'
            [void]$content.AppendChild($copyOut)

            $copyPub = $xml.CreateElement('CopyToPublishDirectory')
            $copyPub.InnerText = 'PreserveNewest'
            [void]$content.AppendChild($copyPub)

            [void]$ig.AppendChild($content)
            [void]$xml.Project.AppendChild($ig)
        }
    }


    # fix31: The GUI intentionally resolves only:
    #   AppContext.BaseDirectory\backend
    # Therefore backend files MUST be published beside the EXE.
    # Copying them into the generated project directory alone is not enough;
    # SDK publish can otherwise omit them.
    $backendContentExists = $false
    foreach ($ig0 in $xml.Project.ItemGroup) {
        foreach ($content0 in $ig0.Content) {
            if ($content0.Include -eq 'backend\**\*') {
                $backendContentExists = $true
                break
            }
        }
        if ($backendContentExists) { break }
    }

    if (-not $backendContentExists) {
        $ig = $xml.CreateElement('ItemGroup')
        $content = $xml.CreateElement('Content')
        $content.SetAttribute('Include', 'backend\**\*')

        $copyOut = $xml.CreateElement('CopyToOutputDirectory')
        $copyOut.InnerText = 'PreserveNewest'
        [void]$content.AppendChild($copyOut)

        $copyPub = $xml.CreateElement('CopyToPublishDirectory')
        $copyPub.InnerText = 'PreserveNewest'
        [void]$content.AppendChild($copyPub)

        [void]$ig.AppendChild($content)
        [void]$xml.Project.AppendChild($ig)
    }

    $xml.Save($ProjectFile)
}

Write-Host '[4/5] Applying unpackaged + tray/icon project settings...'
$projectFile = Join-Path $soopDir 'SOOPLiveWinUI.csproj'
Set-ProjectProperties $projectFile

# Keep Microsoft's official App.xaml/MainWindow.xaml.
Get-ChildItem $overlayDir -File | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $soopDir $_.Name) -Force
}

$assetSrc = Join-Path $overlayDir 'Assets'
$assetDst = Join-Path $soopDir 'Assets'
if (Test-Path $assetSrc) {
    New-Item -ItemType Directory -Path $assetDst -Force | Out-Null
    Copy-Item (Join-Path $assetSrc '*') $assetDst -Force
}

Write-Host '[5/5] Copying backend...'
$destBackend = Join-Path $soopDir 'backend'
if (Test-Path $destBackend) {
    Remove-Item $destBackend -Recurse -Force
}
Copy-Item $backendDir $destBackend -Recurse -Force

foreach ($runtimeFile in @(
    @{ Target = 'SOOP_LIVE_SETTING.ini'; Example = 'SOOP_LIVE_SETTING.example.ini' },
    @{ Target = 'SOOP_LIVE_CHANNELS.txt'; Example = 'SOOP_LIVE_CHANNELS.example.txt' }
)) {
    $targetPath = Join-Path $destBackend $runtimeFile.Target
    $examplePath = Join-Path $destBackend $runtimeFile.Example
    if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf) -and
        (Test-Path -LiteralPath $examplePath -PathType Leaf)) {
        Copy-Item -LiteralPath $examplePath -Destination $targetPath
        Write-Host "[SAFE DEFAULT] Created $($runtimeFile.Target) from example."
    }
}

$requiredBackend = Join-Path $destBackend 'SOOP_LIVE.ps1'
if (-not (Test-Path -LiteralPath $requiredBackend -PathType Leaf)) {
    throw "Backend copy verification failed: $requiredBackend"
}

Write-Host ''
Write-Host 'Prepared:'
Write-Host " $soopDir"
Write-Host ''
Write-Host 'VanillaWinUI diagnostic project is no longer generated.'
Write-Host ''
