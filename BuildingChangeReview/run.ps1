param([switch]$Release)
$ErrorActionPreference = 'Stop'
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
$cargoPath = if ($cargoCommand) { $cargoCommand.Source } else { Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe' }
if (!(Test-Path -LiteralPath $cargoPath)) { throw 'Rust cargo is not installed or could not be located' }
$arguments = @('run', '--locked', '--manifest-path', (Join-Path $PSScriptRoot 'Cargo.toml'))
if ($Release) { $arguments += '--release' }
& $cargoPath @arguments
if ($LASTEXITCODE -ne 0) { throw "BuildingChangeReview failed: $LASTEXITCODE" }
