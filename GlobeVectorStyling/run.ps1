param(
    [switch]$Release,
    [switch]$PrepareOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Example = 'GlobeVectorStyling'
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


$data = Join-Path $PSScriptRoot 'data'
$required = @('.shp', '.shx', '.dbf', '.prj')
$missing = @($required | Where-Object { !(Test-Path -LiteralPath (Join-Path $data ("buildings" + $_))) })
if ($missing.Count -gt 0) {
    New-Item -ItemType Directory -Path $data -Force | Out-Null
    $archive = Join-Path $data 'buildings.zip'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/sagrada_familia_buildings.zip' -OutFile $archive
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            foreach ($extension in ($required + '.cpg')) {
                $entries = @($zip.Entries | Where-Object { $_.Name -ieq ("buildings" + $extension) })
                if ($entries.Count -eq 0 -and $extension -eq '.cpg') { continue }
                if ($entries.Count -ne 1) { throw "Expected one buildings$extension in the archive" }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], (Join-Path $data ("buildings" + $extension)), $true)
            }
        } finally { $zip.Dispose() }
    } finally { if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive } }
}
