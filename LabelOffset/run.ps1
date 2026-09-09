param(
    [switch]$Release,
    [switch]$PrepareOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Example = 'LabelOffset'
if (!$PrepareOnly) {
    $cargoArgs = @('run', '--locked', '--manifest-path', (Join-Path $PSScriptRoot 'Cargo.toml'))
    if ($Release) { $cargoArgs += '--release' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "$Example failed with exit code $LASTEXITCODE" }
    return
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$lock = Get-Content -LiteralPath (Join-Path $repositoryRoot 'Cargo.lock') -Raw
$match = [regex]::Match($lock, '(?m)^name = "geokernel"\r?\nversion = "([0-9.]+)"')
if (!$match.Success) { throw 'Missing geokernel version in Cargo.lock' }
$version = $match.Groups[1].Value
$SdkBin = Join-Path $repositoryRoot "packages/GeoKernel/$version/windows-x64"
$ready = Join-Path $SdkBin '.complete'

if (!(Test-Path -LiteralPath $ready) -or
    !(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Viewer.dll')) -or
    !(Test-Path -LiteralPath (Join-Path $SdkBin 'plugins/platforms/qwindows.dll'))) {
    $packageRoot = Split-Path -Parent $SdkBin
    New-Item -ItemType Directory -Force -Path $packageRoot | Out-Null
    $asset = "GeoKernel-$version-windows-x64.zip"
    $baseUrl = "https://github.com/geokernel-io/GeoKernel/releases/download/v$version"
    $archive = Join-Path $packageRoot $asset
    $checksums = Join-Path $packageRoot 'github-release-assets.sha256'
    Write-Host "Downloading GeoKernel SDK $version from GitHub Releases..."
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/github-release-assets.sha256" -OutFile $checksums
    $hashPattern = '^([a-fA-F0-9]{64})\s+\*?' + [regex]::Escape($asset) + '$'
    $hashLine = Get-Content -LiteralPath $checksums | Where-Object { $_ -match $hashPattern } | Select-Object -First 1
    if (!$hashLine) { throw "Published checksum is missing for $asset" }
    $expected = [regex]::Match($hashLine, $hashPattern).Groups[1].Value
    if (!(Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$asset" -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
        throw "Downloaded SDK checksum does not match: $asset"
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $SdkBin -Force
    $plugins = Join-Path $SdkBin 'plugins'
    # The 1.5.24 runtime archive carries Qt plugins alongside its .NET controls.
    # Install those published native plugins into the common runtime directory.
    if (!(Test-Path -LiteralPath (Join-Path $plugins 'platforms/qwindows.dll'))) {
        $platform = Get-ChildItem -LiteralPath $SdkBin -Filter qwindows.dll -Recurse | Select-Object -First 1
        if (!$platform) { throw 'The published SDK does not contain the Qt Windows platform plugin' }
        $pluginSource = Split-Path -Parent $platform.Directory.FullName
        New-Item -ItemType Directory -Force -Path $plugins | Out-Null
        foreach ($folder in 'platforms', 'tls', 'imageformats', 'iconengines') {
            $source = Join-Path $pluginSource $folder
            if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $plugins -Recurse -Force }
        }
    }
    if (!(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Viewer.dll'))) { throw 'The downloaded SDK is incomplete' }
    Set-Content -LiteralPath $ready -Value $expected
}
Write-Host "GeoKernel SDK: $SdkBin"
function Get-SampleFile([string]$Folder, [string]$File) {
    $dataRoot = Join-Path $PSScriptRoot '../data'
    $dataFolder = Join-Path $dataRoot $Folder
    $existing = Get-ChildItem -LiteralPath $dataFolder -Filter $File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if (!$existing) {
        New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
        $archive = Join-Path $dataRoot "$Folder.zip"
        Write-Host "Downloading GeoKernel sample: $Folder..."
        Invoke-WebRequest -Uri "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/$Folder.zip" -OutFile $archive
        Expand-Archive -LiteralPath $archive -DestinationPath $dataFolder -Force
        $existing = Get-ChildItem -LiteralPath $dataFolder -Filter $File -Recurse | Select-Object -First 1
    }
    if (!$existing) { throw "The sample archive does not contain $File" }
    return $existing.FullName
}
function Test-Shapefile([string]$File) {
    foreach ($extension in '.shp', '.shx', '.dbf') {
        $part = [System.IO.Path]::ChangeExtension($File, $extension)
        if (!(Test-Path -LiteralPath $part)) { throw "Incomplete Shapefile: $part" }
    }
}
$Data = Get-SampleFile 'world_4326' 'world_4326.shp'
Test-Shapefile $Data
