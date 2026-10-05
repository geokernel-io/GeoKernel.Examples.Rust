# PointCloudViewer — Rust

Automatically downloads the published SDK dependencies and EuroSDR P4 40 × 40 m LAZ sample, then opens the SDK point-preview window. Uses the same reader and renderer as the Qt, WinForms, WPF, Python and Electron examples.

Choose RGB/height colours, a display sample limit of 50,000 / 100,000 / 250,000 points, cancel native loading, or reset the camera. The information panel shows source/display counts, source Z range and RGB availability. Cancellation or read failure retains the previous cloud.

The preview is locally normalized. Source coordinates remain EPSG:32630 with provisional heights. Source data is not modified. Point size is fixed at one pixel.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

From **CMD**:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "PointCloudViewer\run.ps1" -Release
```

The launcher records the extracted LAZ path in the ignored `pointcloud-path.txt`, so nested archive folders work. Native calls remain on the GUI thread, and the adapter cancels/joins its loader before its parent window is destroyed.
