param([switch]$Release)
# Runs the example against GeoKernel.Rust sources and a native runtime with the embedding index:
#   $env:GEOKERNEL_RUST_SOURCE = "D:\projects\GeoKernel.Rust"
#   $env:GEOKERNEL_BIN         = "D:\projects\GeoKernel\outputs\build\Release"
$ErrorActionPreference = 'Stop'
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
$cargoPath = if ($cargoCommand) { $cargoCommand.Source } else { Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe' }
if (!(Test-Path -LiteralPath $cargoPath)) { throw 'Rust cargo is not installed or could not be located' }
if (-not $env:GEOKERNEL_BIN) { throw 'Set GEOKERNEL_BIN to a native GeoKernel runtime with the embedding index (newer than 1.5.40)' }
$arguments = @('run', '--manifest-path', (Join-Path $PSScriptRoot 'Cargo.toml'))
if ($args.Count) { $runArguments = @("--") + $args } else { $runArguments = @() }
if ($env:GEOKERNEL_RUST_SOURCE) {
    $crate = (Join-Path $env:GEOKERNEL_RUST_SOURCE 'crates/geokernel').Replace('\', '/')
    $arguments = @('--config', "patch.crates-io.geokernel.path='$crate'") + $arguments
}
if ($Release) { $arguments += '--release' }
$arguments += $runArguments  # e.g. .\run.ps1 -Release --smoke
if (-not $env:QT_PLUGIN_PATH) { $env:QT_PLUGIN_PATH = $env:GEOKERNEL_BIN }
$env:Path = "$env:GEOKERNEL_BIN;$env:Path"
& $cargoPath @arguments
if ($LASTEXITCODE -ne 0) { throw "SpatialEmbeddingSearch failed: $LASTEXITCODE" }
