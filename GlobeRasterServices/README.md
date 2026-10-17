# GlobeRasterServices

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeRasterServices
cargo run --locked --release
```

Uses published `geokernel` and `geokernel-sys` 1.5.35 crates. First launch downloads
the matching Windows x64 SDK and real Sagrada Familia orthophoto/terrain archives,
verifies SHA-256 checksums and caches the extracted files. The SDK is shared under
`packages`; this example owns its `data`, Cargo workspace, lockfile and `target`.

Matches the simplified Qt example: orthophoto and local DEM are enabled on startup,
with Esri imagery, atmosphere and elevation exaggeration 1x. DEM color shading is
explicitly disabled to keep the aerial photograph visible. The camera focuses on
the raster in an oblique view. No control panel or synthetic data is included.

Native loading uses asynchronous jobs. Includes the application icon, single-line
status bar, runtime error reporting and native cleanup, including during loading.
No console window, application command-line parameters or test modes.

Requires Windows x64, Rust/MSVC, a Vulkan-capable graphics driver, a valid license
and internet for initial downloads and imagery. No local SDK build is needed.
