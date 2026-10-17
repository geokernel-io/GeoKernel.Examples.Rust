# GlobeModelPlacement

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeModelPlacement
cargo run --locked --release
```

Uses published `geokernel` and `geokernel-sys` 1.5.35 crates. First launch downloads
the matching Windows x64 SDK and real Sagrada Familia model, verifies their SHA-256
checksums, and caches the SDK under `packages` and the complete model under this
example's `data` folder. No local SDK build or manual data preparation is needed.

Starts with Esri imagery, atmosphere and the Qt example's approximate placement at
longitude 2.1744 / latitude 41.4036, ellipsoidal anchor height 14.089 m, without DEM.
Model credit: PeeJaa. Controls: Zoom to model, Rotate 30 degrees, Scale (0.01–10),
Anchor height (-100–10000 m), Remove, Restore, Atmosphere and Esri. Number changes
apply immediately. Restore reloads the cached model with its last accepted placement.
Controls remain disabled during geometry generation and GPU upload.

Includes the application icon, single-line status bar, native cleanup on close,
and no console window, application command-line parameters or test modes.
Requires Windows x64, Rust/MSVC, a Vulkan-capable graphics driver, a valid license
and internet for initial downloads and Esri imagery. Builds in its own `target`
folder with an independent Cargo workspace and lockfile.

Known SDK 1.5.35 limitation: heading 30 degrees with scale 1.5 is rejected with
`Model is too small for scene precision.` Scale 2 succeeds. The example displays
the error, restores accepted control values and retains the previous model.
This precision limitation requires a separate native SDK fix/release.
