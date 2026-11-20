# DsmToDtmAccuracy — Rust

Reference accuracy by area: compare **where classical and AI terrain estimates differ
from the same reference**. Three linked native GeoKernel **2D maps** keep the original
DSM on the left, with classical and AI error maps beside it on one shared colour scale.
Select an area to compare numeric errors, their distribution and individual source
pixels. **No inference runs here.** Rust port of the Qt `DsmToDtmAccuracy` example.

Requires Windows x64, Rust/Cargo with the MSVC toolchain and a valid GeoKernel license.
Uses published **geokernel 1.5.40** and automatically downloads/verifies the matching
native SDK and the pinned sample; its own Cargo workspace and lockfile.

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

Options: `-- --data-dir D:\cache`, `--sample-archive <zip>`, `--sample-url <https-url>`.
All hash pins still apply. The default cache is the repository's `data` folder.

## Layout

The control panel holds Select evaluation area (a toggle button), Evaluate current view,
Full sample, map comparison, display limit, large-error threshold, study-area preset,
navigation, report export, retry/cancel and loading progress. The log panel shows the
instructions, the lower RMSE/MAE verdict, the metric table, the evaluated scope and the
probed pixel. The central area holds three pane headings, three maps and a GDI panel with
the shared colour legend and the absolute-error histogram.

## Workflow

1. Startup reuses the verified Zurich v2 sample and prepares 19 shared-scale Byte RGB
   display maps (DSM relief; per method 4 signed, 4 absolute and 1 terrain relief).
2. **Signed reference errors**, **Absolute reference errors** or **Terrain estimates**;
   **Display limit** (1, 2, 5, 10 m) only sets colour saturation.
3. Choose a geographic preset or press **Select evaluation area** and drag a box on any
   map; both methods are evaluated on the same source cells and the outside is dimmed on
   all maps. **Evaluate current view** and **Full sample** set the area directly.
4. Read RMSE, MAE, bias, nearest-rank P95 of |error| and the share above **Large error >**
   (no overall winner is inferred) and the six-bin absolute-error histogram.
5. With Pan active, click a pixel to read DSM, reference, classical and AI heights and
   both signed errors; a marker shows the cell on every map.
6. **Export area report** writes the JSON metrics and provenance atomically.

Full sample: classical RMSE 1.006692 / MAE 0.350048 / bias 0.247517 m; AI RMSE 0.659430 /
MAE 0.406952 / bias −0.277471 m. Presets are geographic windows, not land-cover classes.

## Implementation notes and differences from the Qt example

- Each map is a standalone viewer hosted through the GeoKernelViewer C ABI
  ([map2d.rs](src/map2d.rs)), because the crate's `ViewerWindow` panes report mouse and
  selection events only for the primary pane.
- The native control panel has no checkbox, so area selection is a toggle button.
- The area is drawn with owned shapes (four translucent rectangles and an outline); the
  probe is an overlay point that is moved, never removed (removal is deferred to a Qt
  event loop this host does not run). Views are linked by extent; each map adopts its
  first layer's CRS.

## Data

500 × 500 cells at 0.5 m, EPSG:2056, LN02 heights (EPSG:5728), no vertical datum
conversion. Data: **Federal Office of Topography swisstopo** (swissSURFACE3D Raster,
swissALTI3D). AI: David Perez's [pix_grad checkpoint](https://doi.org/10.5281/zenodo.7973800),
CC BY 4.0 (derived output only). Fixed development crop, not a held-out benchmark.
