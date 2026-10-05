# FeaturePicking

Windows x64 example using the existing native Viewer3D selection API. The launcher
downloads the published SDK and Sagrada Família DEM, orthophoto, roads and buildings.
Click features to show attributes in the log panel. Controls include multiple
selection, active feature, zoom to selection, clear selection, visibility and
vertical exaggeration. Building heights are illustrative (9 metres).

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

If not already rebuilt after adding selection, build the native target in a fresh CMD:



Build and run from CMD:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "FeaturePicking\run.ps1" -Release
```

All native calls stay on the GUI thread. Selection snapshots are copied every
150 ms and only changed revisions update the panel. IDs retain their full integer
precision. Closing destroys the native host before its parent window and runtime.
