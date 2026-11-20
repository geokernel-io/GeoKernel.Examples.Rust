# DsmToDtmProcessing — Rust

Live classical processing: **run a classical DSM-to-DTM conversion on the input DSM**,
change parameters, inspect the newly generated terrain and export numeric GeoTIFFs. Three
linked native GeoKernel **2D maps** show the original DSM, the generated DTM and a
selectable diagnostic map. Rust port of the Qt `DsmToDtmProcessing` example; the
algorithm is in [terrain.rs](src/terrain.rs).

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

## Layout and workflow

The control panel holds Load Zurich sample, Open DSM GeoTIFF, Open reference (optional),
the preset, the six parameters, Run DSM → DTM, Cancel, the right-map mode, Save result
GeoTIFFs, the navigation buttons and progress. The log panel shows the source, the run
state, reference scores and the probed pixel; the strip below the maps shows the legend
and the last run's summary.

1. Startup reuses the verified Zurich v2 sample and shows its original DSM. Nothing is
   converted until **Run DSM → DTM** is pressed.
2. Choose **Balanced**, **Remove more objects** or **Preserve more relief**, or edit the six
   parameters, then **Run**. Processing runs on a worker thread; **Cancel** keeps the last
   completed source/result.
3. The middle map shows the new DTM with the same hillshade as the original; the views are
   linked. With Pan active, click a pixel to read DSM, DTM, removed height, QA bits and the
   optional reference/error; a marker shows the cell on every map.
4. The right map shows **removed height** (0–20 m), **support / edge quality** or **signed
   reference error** (−2 to +2 m). Colours never clip the numeric results or scores.
5. Changing parameters keeps the last result on screen, marked as needing a new run.
6. **Open DSM GeoTIFF** loads another source; **Open reference (optional)** attaches an
   exactly aligned evaluation raster and rescores without recomputing.
7. **Save result GeoTIFFs** writes `<name>.tif` (Float32 DTM), `.removed.tif`, `.qa.tif`
   (Byte QA bits 1/2/4/8/16) and `.json` (parameters, statistics, scores, provenance), each
   atomically. Source/reference paths are rejected; existing companions need confirmation.

## Method and results

Method `native-multiscale-morphological-tin-v1`: block-minimum lower envelope keeping each
minimum's real source pixel, reflected-boundary openings at doubling windows with a capped
base/slope allowance plus a DSM-derived relief allowance reduced in rough areas, GDAL linear
TIN gridding at the source pixel centres (the SDK's `gdal.dll`), a grid-support fallback
outside the hull, source retention within the keep tolerance and a DSM upper bound. The
reference is never an input to processing.

Grids are `f32` and the C++ float operations are reproduced, so on the Zurich sample the
result is **identical to the Qt build** (DTM, removed heights and QA on all 250,000 cells).
Balanced: 10485 supports, 2582 hull-fallback pixels; RMSE 0.863 / MAE 0.324 / bias 0.200 m
over all cells and 0.532 / 0.220 / 0.098 m outside the 32 m edge band.

Inputs: single-band north-up GeoTIFF, square pixels, projected metre CRS, metre heights,
8 × 8 to 4 million cells with at least 5% valid; working grid 3 × 3 to 262,144 cells.

## Implementation notes and differences from the Qt example

- Each map is a standalone viewer hosted through the GeoKernelViewer C ABI
  ([map2d.rs](src/map2d.rs)), because the crate's `ViewerWindow` panes report mouse events
  only for the primary pane. Views are linked by extent; an empty map takes the linked view
  when it receives its first layer.
- The probe is an overlay point that is moved, never removed (removal is deferred to a Qt
  event loop this host does not run). Each map adopts its first layer's CRS.
- Text that the Qt window shows in sidebar labels is in the log panel and the strip below
  the maps; the native control panel has no free text labels.

Data: **Federal Office of Topography swisstopo**, 500 × 500 at 0.5 m, EPSG:2056, LN02 heights,
no vertical datum conversion. Fixed development crop, not a held-out benchmark.
