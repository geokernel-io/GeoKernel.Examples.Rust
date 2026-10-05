# Tiles3DStreaming — Rust

Automatically prepares the published SDK, Sagrada Família DEM, orthophoto,
3D Tiles and EGM08D595 geoid. Uses the same native camera-driven streamer as Qt.
Includes geometry budget, start/stop, visibility, focus and live diagnostics.

Vertical exaggeration stays at 1×. Terrain receives the regional geoid correction;
alignment is approximate. Stop retains tiles; hiding clears them and showing
restarts streaming. Cache counters do not represent total RAM/VRAM usage.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "Tiles3DStreaming\run.ps1" -Release
```

All native calls stay on the GUI thread. Copied state is read every 500 ms;
only changes update the diagnostics panel. The adapter drops before its parent
window and runtime. No automatic source-checkout fallback is used.
