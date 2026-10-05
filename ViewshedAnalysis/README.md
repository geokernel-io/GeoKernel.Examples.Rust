# ViewshedAnalysis — Rust

Automatically prepares the published SDK and Sagrada Família DEM/orthophoto. Select an observer on terrain and configure eye/target heights, radius, ray spacing and grid size. Includes calculate/resume, pause, clear, imagery/overlay visibility and vertical exaggeration.

The results panel shows a north-up visibility grid and statistics. The same result appears as a 55%-opacity overlay on the 3D terrain. The small grid uses a temporary BMP displayed by the existing SDK results panel; the file is removed on normal shutdown.

Uses the same native viewshed calculation and triangle-clipped overlay as Qt, .NET, Python and Electron. Analysis samples the 512-sample terrain mesh. Buildings, vegetation and refraction are excluded; small obstacles between samples may be missed. Vertical datum is unverified. Exaggeration changes only the view.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

From CMD:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ViewshedAnalysis\run.ps1" -Release
```
