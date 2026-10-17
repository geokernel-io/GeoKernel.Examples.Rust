param(
    [switch]$Release,
    [switch]$PrepareOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Example = 'GlobeTerrainNavigation'
if (!$PrepareOnly) {
    $cargoArgs = @('run', '--locked', '--manifest-path', (Join-Path $PSScriptRoot 'Cargo.toml'))
    if ($Release) { $cargoArgs += '--release' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "$Example failed with exit code $LASTEXITCODE" }
    return
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$version = '1.5.35'
$SdkBin = Join-Path $repositoryRoot "packages/GeoKernel/$version/windows-x64"
$ready = Join-Path $SdkBin '.complete'

if (!(Test-Path -LiteralPath $ready) -or
    !(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Viewer.dll')) -or
    !(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Globe.dll')) -or
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
    if (!(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Viewer.dll'))) { throw 'The downloaded SDK is incomplete' }
    if (!(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Globe.dll'))) { throw 'The published SDK does not contain Globe' }
    Set-Content -LiteralPath $ready -Value $expected
}
Write-Host "GeoKernel SDK: $SdkBin"
