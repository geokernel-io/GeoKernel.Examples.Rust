# SimilaritySearch — Rust

Pick a solar park, a lake, a town centre or a field (or click a place, or drag a box) and every
80 m cell of Flevoland is ranked by how much it looks like it, using AlphaEarth satellite
embeddings (64 values per cell). Rust port of the Qt `SimilaritySearch` example on top of
geokernel's `ViewerWindow::similarity_search` (C ABI `GeoKernelSimilarity_*`), with the native
viewer window and control panel.

## Run

The find-similar API ships after geokernel **1.5.40**. Until the next release, patch in the
GeoKernel.Rust sources and use a native runtime with the API:

```powershell
$env:GEOKERNEL_RUST_SOURCE = "D:\projects\GeoKernel.Rust"
$env:GEOKERNEL_BIN = "D:\projects\GeoKernel\outputs\build\Release"
./run.ps1 -Release
```

At startup the example downloads `flevoland_alphaearth_2024_80m_v1.zip` from GeoKernel.SampleData
v1 (31 MB, SHA-256 checked, with `curl.exe` and `tar.exe`, cached in
`%LOCALAPPDATA%\GeoKernel\Examples\GeoAI` and shared with the other GeoAI examples).
`./run.ps1 -Release --smoke` searches the solar park, prints the result and exits.

## Workflow

1. The map shows the embedding grid as muted false colour (its first three dimensions).
2. The preset buttons search **Solar park** (the mean embedding of Zonnepark Zuyderzon, Almere),
   **Open water**, **Town centre** or **Farmland** (one cell each); the view frames the query and
   its ten best matches.
3. **Find similar on the map** turns map search on or off: click a place (its cell) or drag a box
   (the mean of its cells). The match list follows these searches (`SimilaritySearch::revision`).
4. **Heat map ≥** sets the heat-map threshold; pick a match to zoom to it.

`SimilaritySearch` keeps the window's native handles alive without borrowing the window, so the
event loop keeps full use of the window while the search exists.

The AlphaEarth Foundations Satellite Embedding dataset is produced by Google and Google DeepMind
(CC BY 4.0).
