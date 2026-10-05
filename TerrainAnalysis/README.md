# TerrainAnalysis — Rust

Automatically prepares the published SDK dependencies and Sagrada Família DEM, then opens the terrain viewer. Uses the same native relief, slope and aspect display modes as Qt, .NET, Python and Electron.

Includes matching colour legends, 256/512/1024 mesh resolution, vertical exaggeration, native-load cancellation and camera reset. Analysis describes mesh faces before exaggeration, not an exported analysis raster. Mesh resolution affects results. Aspect is clockwise from north in local ENU, in eight sectors spanning ±22.5°; slopes below 0.01° are flat. Heights are metres with an unverified vertical reference.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

From **CMD**:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "TerrainAnalysis\run.ps1" -Release
```

Native calls stay on the GUI thread. The native loader is cancelled and joined before the parent window and runtime are destroyed.
