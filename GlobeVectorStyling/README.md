# GlobeVectorStyling

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeVectorStyling
cargo run --locked --release
```

Uses the published `geokernel` and `geokernel-sys` 1.5.35 crates. First launch
automatically downloads the matching Windows x64 SDK and verifies its published
SHA-256 checksum. The real Sagrada Familia building shapefile downloads from
GeoKernel.SampleData into this example's `data` folder. Later launches reuse
the downloaded SDK and data.

Requires Windows x64, Rust/MSVC, a Vulkan-capable graphics driver and a valid
GeoKernel license. Internet is required for the initial downloads and Esri imagery.

Opens with atmosphere, Esri satellite imagery and the same oblique building view
as the Qt, WinForms, WPF, Python and Electron examples. The native control panel
provides Fill, Labels, Color by (attribute or Uniform), label field, opacity,
base color, Zoom to buildings and Esri on/off. Click a building to display its
attributes. SDK fill validation leaves rejected polygons outline-only; their
count appears in the status bar.

Loading and styling use asynchronous SDK jobs. Controls are disabled while a
job is pending. Closing releases the Globe before its parent window and Qt.
Includes the application icon and single-line status bar; no console window,
application command-line parameters or test modes.

The example owns its Cargo workspace and lockfile and builds in its own `target`
folder. No local SDK build or manual data preparation is required.
