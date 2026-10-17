param(
    [switch]$Release,
    [switch]$PrepareOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '') }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}
$Example = 'GlobeModelPlacement'
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
    if (!(Test-Path -LiteralPath $archive) -or (Get-Sha256 $archive) -ne $expected) {
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$asset" -OutFile $archive
    }
    if ((Get-Sha256 $archive) -ne $expected) {
        throw "Downloaded SDK checksum does not match: $asset"
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $SdkBin -Force
    if (!(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Viewer.dll'))) { throw 'The downloaded SDK is incomplete' }
    if (!(Test-Path -LiteralPath (Join-Path $SdkBin 'GeoKernel.Globe.dll'))) { throw 'The published SDK does not contain Globe' }
    Set-Content -LiteralPath $ready -Value $expected
}
Write-Host "GeoKernel SDK: $SdkBin"


$data = Join-Path $PSScriptRoot 'data'
$model = Join-Path $data 'model.gltf'
$marker = Join-Path $data '.complete'
if (!(Test-Path -LiteralPath $marker) -or !(Test-Path -LiteralPath $model)) {
    New-Item -ItemType Directory -Path $data -Force | Out-Null
    $archive = Join-Path $data 'model.zip'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/sagrada_familia_3d_model.zip' -OutFile $archive
        if ((Get-Sha256 $archive) -ne 'd7f031059b2dc3fb22ce8d25dd3d7bd3d71b5f7675489c818309a78a068dd956') {
            throw 'The model archive checksum does not match'
        }
        Expand-Archive -LiteralPath $archive -DestinationPath $data -Force
        if (!(Test-Path -LiteralPath $model)) { throw 'model.gltf is missing from the archive' }
        Set-Content -LiteralPath $marker -Value '1'
    } finally { if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive } }
}

