# DsmToDtm — Rust

Ground beneath buildings: an original DSM stays visible beside a prepared terrain
estimate in two linked native GeoKernel 3D viewers. Choose the classical
morphological/TIN or experimental DSM-only pix_grad estimate and move the surface
slider; heights are interpolated numerically between both endpoints. **No inference
runs here.** Rust port of the Qt `DsmToDtm` example.

Requires Windows x64, Rust/Cargo with the MSVC toolchain, Vulkan and a valid
GeoKernel license. Uses published **geokernel 1.5.40** and automatically
downloads/verifies the matching native SDK (GitHub release SHA-256) and the pinned
sample. There is no local SDK build, system GDAL or Python requirement. This example
has its own Cargo workspace and lockfile, preserving other examples' versions.

From this directory:

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

Options: `cargo run --locked --release -- --data-dir D:\cache`, `--sample-archive <zip>`
(exact local copy of the pinned ZIP) or `--sample-url <https-url>` (mirror). All
hash pins still apply. The default cache is the repository's `data` folder.
`GEOKERNEL_BIN` may point at a compatible runtime instead of the downloaded SDK.

## Layout

The native window's control panel holds the estimate, surface slider, endpoint
buttons, vertical exaggeration, camera actions, profile visibility, retry/cancel and
the data folder. The log panel shows the workflow, current surface and crop scores.
The central area holds the two pane headings, both Viewer3D hosts and the profile
chart as Win32 child windows; the chart is drawn with GDI.

## Workflow

1. Startup downloads the fixed Zurich v2 sample from the `GeoKernel.SampleData` v1
   release (`curl.exe`), checks the ZIP SHA-256, extracts it with a path-safe
   PowerShell helper and checks every file against
   [sample-assets.json](sample-assets.json). A complete valid cache works offline.
2. The left view always shows the original DSM; the right starts with the classical
   DTM at 100%. Pan, orbit or zoom either view; the other follows.
3. **0% is exactly the DSM, 100% exactly the selected DTM.** Intermediate values
   write a temporary Float32 GeoTIFF (`(1-t)*DSM+t*DTM`) through the SDK's GDAL and
   rebuild the right-hand terrain after a 200 ms pause in slider changes (the native
   slider reports value changes, not release).
4. The profile chart samples all four rasters on the fixed building transect: gray
   DSM, teal classical, purple AI, dashed orange reference.
5. Vertical exaggeration applies to both views. Cancel keeps the previous complete
   views and restores the last committed selection. Closing during preparation stops
   the worker; destroying each viewer cancels and joins its native load.

The reference DTM is used only for the profile and the stored crop scores.

## Differences from the Qt example

The published Viewer3D C API has no exact ENU camera mapping and no analysis-line
overlay. Render-space cameras are copied directly (both scenes share one grid and
grid-centre origin, so they correspond to within 1e-5), the transect is shown only
in the profile chart, and native terrain preparation reports no percentage.

## Data

500 × 500 pixels at 0.5 m, EPSG:2056, heights in the source-declared LN02 reference
(EPSG:5728), no vertical datum conversion. Crop reference errors: classical RMSE
1.01 m / MAE 0.35 m, AI RMSE 0.66 m / MAE 0.41 m; not a held-out benchmark.
Data: **Federal Office of Topography swisstopo** (swissSURFACE3D Raster, swissALTI3D,
[OGD terms](https://www.swisstopo.admin.ch/en/terms-of-use-free-geodata-and-geoservices)).
AI: David Perez's [pix_grad checkpoint](https://doi.org/10.5281/zenodo.7973800),
CC BY 4.0; the archive contains derived output only, no weights.
