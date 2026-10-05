# Measurement3D — Rust

Automatically prepares the published SDK, Sagrada Família DEM and orthophoto.
Enable Add points and click terrain for distance or horizontal polygon area.
The native overlay shows the measurement and the results panel lists coordinates.
Finish stops capture; Undo removes the last point; Clear resets the measurement.

Uses the same native measurement API as the other language examples. Results use
physical ENU metres, independent of exaggeration. 3D length is straight segments,
not terrain-following length. Area is horizontal plan area. Maximum 64 points;
invalid polygons report an error and the DEM datum is unverified.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "Measurement3D\run.ps1" -Release
```

Native calls stay on the GUI thread. Copied snapshots are read every 150 ms;
only changed values update the results panel. The adapter is destroyed before
the parent window and runtime. No automatic local SDK fallback is used.
