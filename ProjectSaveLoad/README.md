# ProjectSaveLoad

Windows Rust example using the shared GeoKernel Viewer3D project API.

The SDK and sample terrain, orthophoto, roads and buildings are prepared automatically.
Open project, Save project and Save project as use native Windows file dialogs.
Projects preserve camera, vertical exaggeration, imagery visibility, layer
appearance, attribute colors and filters. Source data is referenced, not embedded.
Keep the data cache. Save changes before opening a project or reloading the sample.
Enter or leave the filter text field to apply it before saving.

Opening restores the saved camera and display settings. Cancelled or failed
preparation retains the previous project path. The native API supports the
terrain/imagery/roads/buildings subset and rejects other scene types.
Sample building heights are illustrative (9 m); the DEM vertical datum is unverified.

## Run from CMD

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ProjectSaveLoad\run.ps1" -Release
```

Validation: change camera, layer colors, opacity, filters and exaggeration; save,
change the scene and reopen. Check restored state and source paths. Check file
dialog and load cancellation as well.

The example has not been compiled or run during preparation.
