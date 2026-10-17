# GlobeHello

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeHello
cargo run --locked --release
```

This standalone example uses the published `geokernel` and `geokernel-sys`
1.5.35 crates. On first launch it downloads the matching Windows x64 SDK from
GitHub Releases and verifies its published SHA-256 checksum. Subsequent launches
reuse `../packages/GeoKernel/1.5.35/windows-x64`. No local SDK build is needed.

Requirements: Windows x64, Rust with the MSVC toolchain, a Vulkan-capable graphics
driver, and a valid GeoKernel license. First launch requires internet access.

The globe starts with Atmosphere, Esri Satellite, DEM and Sunlight switched off.
Use the four Off/On selectors in the Globe panel to enable them independently.
Imagery and elevation stream from the real Esri services; no synthetic or local
sample data is generated. Attribution and errors appear in the status bar.

- Left drag: pan.
- Right drag or mouse wheel: zoom.
- Middle drag or Ctrl + left drag: orbit.

The window has the shared GeoKernel icon, a single-line status bar and no console.
Startup errors are shown in a dialog. Native resources are released before Qt
shuts down. There are no application command-line options or test modes.

`cargo build --locked --release` builds to `GlobeHello/target/release`.
This directory has its own lockfile so older Rust examples keep their existing
SDK versions.
