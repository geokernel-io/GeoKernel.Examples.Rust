# ElevationProfile — Rust

Automatically prepares the published SDK and Sagrada Família DEM/orthophoto. Click at least two terrain positions to draw a route. Includes finish, undo, clear, sample spacing, imagery visibility and vertical exaggeration.

The results panel shows elevation against horizontal distance, ascent/descent totals and sampling progress. A temporary BMP supplies the chart to the SDK rich-text panel; it is removed on normal shutdown. No extra graphics dependency is needed.

Uses the same incremental native profile API as Qt, .NET, Python and Electron: up to 64 route points and 4096 samples. Results describe the loaded terrain mesh with an unverified source vertical datum. Missing samples remain gaps; exaggeration does not change results.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

From CMD:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ElevationProfile\run.ps1" -Release
```
