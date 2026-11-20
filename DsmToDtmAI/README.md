# DsmToDtmAI — Rust

Live AI versus classical: run the experimental **pix_grad** model through **GeoKernel.AI**,
run the native classical filter independently on the same DSM, and compare the newly
generated heights. Three linked native GeoKernel **2D maps** keep the original DSM on the
left. Neither method receives the reference or the other method's terrain estimate. Rust
port of the Qt `DsmToDtmAI` example; inference is in [ai.rs](src/ai.rs), the comparison and
export in [compare.rs](src/compare.rs), the classical filter in [terrain.rs](src/terrain.rs).

Requires Windows x64, Rust/Cargo with the MSVC toolchain and a valid GeoKernel license.
Uses published **geokernel 1.5.40** and automatically downloads/verifies the matching native
SDK; terrain, projection support and the model package are downloaded and hash-verified on
first start. Its own Cargo workspace and lockfile.

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

Options: `-- --data-dir D:\cache` (cache for terrain, support and model files; default: the
repository's `data` folder), `--model-archive <zip>` / `--model-url <https-url>` (the exact
pinned model ZIP), `--sample-archive <zip>` / `--sample-url <https-url>` (the pinned Zurich v2
support ZIP, not the 1 km source). All hash pins still apply.

## Layout and workflow

The control panel holds the source, model and reference buttons, tile overlap, CPU threads,
the classical preset, **Run AI + classical**, Cancel, the compare and scope selectors, Save,
the navigation buttons and progress. The log panel shows the source, the model, the run state
and the probed pixel; the strip below the maps shows the legend, both methods' scores and the
last run's summary.

1. Startup verifies the Zurich v2 support ZIP for the matching PROJ database, then downloads
   **the original full 1 km DSM and reference** directly from swisstopo (2000 × 2000 at
   0.5 m, EPSG:2056; sizes and SHA-256 in [terrain-assets.json](terrain-assets.json)).
2. The model package is acquired and its archive plus all six package files are checked
   against [model-assets.json](model-assets.json). **Use model ZIP** supplies the same pinned
   package locally; it does not accept arbitrary weights.
3. Choose tile overlap (default **64 px**, 0–128), CPU threads (default 4) and a classical
   preset, then **Run AI + classical**. The classical result is computed first, then an
   independent inference from the same DSM on a worker thread; the status bar shows phases
   and AI tile counts.
4. Compare **terrain relief**, **removed heights** (one fixed 0–20 m scale) or **signed
   reference errors** (one fixed −2 to +2 m scale). Display limits never clip the scores.
5. **Full input** scores all common valid pixels; **Shared comparison crop** zooms to the
   250 × 250 m window of the earlier Zurich examples (column 200 / row 800, 500 × 500 cells)
   and dims everything outside it. Both methods still process all 4 million cells.
6. Pan/zoom is linked. With Pan active, click a cell to read original, classical, AI and
   reference heights; a marker shows it on every map. Nothing here reruns inference.
7. Changing processing settings keeps the last comparison, marked as needing a new run.
   **Cancel** keeps the completed comparison.
8. **Open DSM GeoTIFF** loads a custom source (crop = central 60% per axis) and clears the
   reference and estimates; **Open reference** attaches an exactly aligned evaluation-only
   raster without rerunning inference.
9. **Save comparison folder** stages `classical-dtm.tif`, `ai-dtm.tif`, `ai-raw.tif`,
   `classical-removed.tif`, `ai-removed.tif`, `classical-qa.tif`, the `comparison-*.tif`
   crops and `comparison.json` in the chosen parent, then renames the staging folder to a new
   `dsm-dtm-ai-<UTC time>-<id>` folder. Existing files are never overwritten.

## Results

On the full Zurich tile every output raster (all 4 million cells, the six display maps and
the crops) is **identical to the Qt build**, and so are the scores: full input classical
(Balanced) RMSE 1.300 / MAE 0.517 / bias 0.400 m, AI (overlap 64) 1.749 / 0.981 / −0.668 m;
shared crop classical 1.018 / 0.355 / 0.253 m, AI 0.659 / 0.407 / −0.277 m. 121 AI tiles,
33,253 AI upper-bound clamps. A small custom input with NoData holes also matched exactly.

Supported inputs: one north-up metre-height band, projected metre CRS, square pixels,
identity scale/offset, 8 × 8 to 4 million cells, at least 5% valid; the reference must share
the exact grid and an equivalent CRS.

## Implementation notes and differences from the Qt example

- Inference calls `GeoKernelAI_RunRasterInference` (the SDK's JSON raster inference C ABI)
  instead of the C++ `requestFromPackage`, binding it directly because the crate's
  `run_raster_inference` passes no progress callback. It has no separate context band,
  source-mask or padding options, so the example writes the context-filled DSM itself,
  edge-pads inputs smaller than one tile right/bottom (the package's padding mode) and crops
  the result, and re-applies the source mask. Results are the same as Qt's.
- `GeoKernel.AI.dll` is loaded with its own folder first in the dependency search, as the
  crate loads the SDK: Windows ships an unrelated `System32\onnxruntime.dll` that the
  default search order would pick instead.
- The inference call has no cancellation hook: **Cancel** and closing the window take effect
  when the current AI pass ends (a few seconds for the 1 km tile).
- Each map is a standalone viewer hosted through the GeoKernelViewer C ABI
  ([map2d.rs](src/map2d.rs)); views are linked by extent. The crop is dimmed with four
  translucent owned polygons and a solid outline (no dash style); the probe is an overlay
  point that is moved, never removed.
- Text that the Qt window shows in sidebar labels and the score table is in the log panel
  and the strip below the maps; the native control panel has no free text labels or tables.
- The model is prepared on the worker thread, not the GUI thread.

Model: David Perez, Zenodo 7973800, CC BY 4.0 (deterministic ONNX adaptation). Terrain:
Federal Office of Topography swisstopo, swissSURFACE3D Raster and swissALTI3D, LN02 heights,
no vertical datum conversion. Fixed development scene, not a held-out benchmark.
