# GlobeTerrainAnalysis

Windows x64 example using the published GeoKernel 1.5.35 SDK.

```powershell
cd D:\projects\GeoKernel.Examples.Rust\GlobeTerrainAnalysis
cargo run --locked --release
```

The application automatically downloads the SDK and real Sagrada Familia DEM,
verifies archive checksums, and caches them locally. Barcelona opens with Esri
imagery, atmosphere and terrain at 1x elevation. DEM color shading is disabled.

Choose Line of sight or Measure, then click two terrain points. Viewshed and
Slope / aspect need one point. After a result, click again to start another
selection. Drag to navigate; Esc cancels. Clear removes the selection and
Barcelona restores the camera. The scrollable result panel stays 170 logical
pixels high.

Line of sight uses heights of 1.7 m above terrain and a maximum distance of 10 km.
The drawn line marks the ground path rather than the elevated sight ray.
Viewshed uses a 350 m radius and requests 15 m ray sampling; the SDK determines
the output grid spacing. These analyses use terrain, excluding buildings.
If terrain changes during calculation, wait for loading to settle and select
again; stale results are explicitly identified. Missing DEM samples are reported.

The normal GUI has no test or diagnostic command-line arguments.
