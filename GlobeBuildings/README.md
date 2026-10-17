# GlobeBuildings

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeBuildings
cargo run --locked --release
```

Uses published `geokernel` and `geokernel-sys` 1.5.35 crates. First launch downloads
the matching Windows x64 SDK and verifies its published SHA-256 checksum. The real
Sagrada Familia building shapefile downloads automatically into this example's
`data` folder. Later launches reuse the SDK and data.

Starts with flat building footprints, Esri satellite imagery, atmosphere and the
same oblique camera as the Qt, WinForms, WPF, Python and Electron examples.
Set Extrude to On for 3D buildings or Off to restore footprints. Display height
is illustrative, not measured: it defaults to 18 metres and ranges from 1 to 300.
Turn extrusion off before changing height. Bases use the reference sphere without
DEM, matching the Qt example. Controls remain disabled during geometry generation
and GPU upload. Zoom to buildings, Atmosphere and Esri controls are also included.

Requires Windows x64, Rust/MSVC, a Vulkan-capable graphics driver and a valid
GeoKernel license. Internet is needed for initial downloads and satellite imagery.
Includes the application icon and single-line status bar. Native resources are
released before the parent window and Qt, including during an extrusion job.
No console window, application command-line parameters or test modes.

The example owns its Cargo workspace and lockfile and builds in its own `target`
folder. No local SDK build or manual data preparation is needed.
