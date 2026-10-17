# GlobeTerrainNavigation

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeTerrainNavigation
cargo run --locked --release
```

Uses the published `geokernel` and `geokernel-sys` 1.5.35 crates. First launch
automatically downloads the matching Windows x64 SDK from GitHub Releases and
verifies its SHA-256 checksum. Later launches reuse the shared SDK cache.
No local SDK build or manual data preparation is required.

Requires Windows x64, Rust/MSVC, a Vulkan-capable graphics driver and a valid
GeoKernel license. Internet access is needed for the SDK download and Esri data.

Opens above Sagrada Familia in Barcelona with Esri satellite imagery, streamed
DEM, atmosphere and sunlight enabled. Use the four Off/On selectors to toggle
these independently. There is no synthetic terrain.

The toolbar provides zoom in/out, rotate left/right, tilt up/down, top view,
north alignment and Overview. Overview restores the same camera as the other
platforms: longitude 2.1744, latitude 41.4036, target height 40 m, distance 2500 m,
pitch approximately 37.24 degrees.

Left drag pans; right drag or the wheel zooms; middle drag / Ctrl + left drag
orbits. Includes the shared application icon and a single-line status bar for
attribution and errors. Native resources are released before Qt shuts down.
The GUI opens without a console; startup errors appear in a dialog.

The example has its own Cargo workspace and lockfile, builds into its own
`target/release` directory, and includes no application CLI parameters or test modes.
