# GeoKernel Rust Examples

Official Rust examples for the GeoKernel native GIS SDK.

## Requirements

- Windows 10 or later
- Rust and Cargo
- A supported MSVC toolchain
- A valid GeoKernel license

## Getting Started

Choose an example directory and run it with the included Cargo configuration or build scripts.

## Resources

- [GeoKernel Website](https://geokernel.io)
- [Documentation](https://docs.geokernel.io)
- [Sample Data](https://github.com/geokernel-io/GeoKernel.SampleData)

## Support

For questions, contact [admin@geokernel.io](mailto:admin@geokernel.io).

## RoadsOnTerrain

Run `cargo run --locked -p roads-on-terrain --release`. The example automatically
prepares the published SDK and downloads the Sagrada Família DEM, orthophoto
and roads. It provides road visibility, color and opacity controls, terrain
resolution, vertical exaggeration and camera reset.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.

## Buildings3D

Run `cargo run --locked -p buildings-3d --release`. The example automatically
prepares the published SDK and downloads the Sagrada Família DEM, orthophoto
and building footprints. Buildings are clamped to terrain with an illustrative
9 metre default height. Change the height and select **Apply height / Reload
sample**. Visibility, color, opacity and terrain exaggeration are adjustable.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.

## ModelPlacement

Run `cargo run --locked -p model-placement --release`. The example automatically
prepares the published SDK and downloads the textured Sagrada Família model,
DEM, orthophoto and EGM08D595 geoid. Placement controls include longitude,
latitude, height above terrain, heading, pitch, roll and scale. Select **Apply
placement / Reload** after changes. Model visibility, focus and imagery controls
are included; no placeholder model is displayed.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.
Placement is approximate. The downloaded model retains its source and license.

## ElevationProfile

`ElevationProfile` downloads the SDK, DEM and orthophoto automatically. Draw a
terrain route to show its elevation graph and ascent/descent totals. Includes
sample spacing, finish, undo and clear controls. Uses the same native profile API
as Qt, .NET, Python and Electron; NoData gaps are not joined.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ElevationProfile\run.ps1" -Release
```

## ViewshedAnalysis

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ViewshedAnalysis\run.ps1" -Release
```

## LayerStylingAndFiltering

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

## ProjectSaveLoad

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.