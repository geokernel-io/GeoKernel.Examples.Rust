# DsmOrthoToDtm — Rust

DSM + orthophoto to DTM: compare three newly computed terrain estimates on **the same full
DSM**: the native classical filter (DSM only), the experimental **pix_grad** model (DSM only)
and the experimental **rgb_grad_fm** model (DSM plus an aligned RGB orthophoto). Four linked
native GeoKernel **2D maps** keep the input context on the left, switchable between the real
orthophoto and the DSM relief. The reference is evaluation-only; the methods do not feed each
other. Rust port of the Qt `DsmOrthoToDtm` example; inference is in [ai.rs](src/ai.rs), the
orthophoto, comparison and export in [ortho.rs](src/ortho.rs), the classical filter in
[terrain.rs](src/terrain.rs).

Requires Windows x64, Rust/Cargo with the MSVC toolchain and a valid GeoKernel license. Uses
published **geokernel 1.5.40** and automatically downloads/verifies the matching native SDK;
the input ZIP and both model packages are downloaded and hash-verified on first start. Its own
Cargo workspace and lockfile.

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

Options: `-- --data-dir D:\cache` (default: the repository's `data` folder),
`--sample-archive <zip>` / `--sample-url <https-url>` (the pinned DSM + orthophoto ZIP),
`--pix-model-archive`, `--rgb-model-archive`, `--pix-model-url`, `--rgb-model-url` (the pinned
model ZIPs). All archive and file hashes stay enforced.

## Layout and workflow

The control panel holds the source, model and reference buttons, AI overlap, CPU threads, the
classical preset, **Run all three methods**, Cancel, the input-context, compare and scope
selectors, Save, navigation and progress. The log panel shows the source, the models, the run
state and the probed pixel; the strip below the maps shows the legend, the three methods'
scores and the last run's summary.

1. Startup verifies/downloads `zurich_dsm_ortho_to_dtm_v1.zip` (GeoKernel.SampleData v1;
   [pins](sample-assets.json)): full 1 km `dsm.tif`, aligned SWISSIMAGE `ortho_rgb.tif`,
   evaluation `reference.tif`, manifest, notices and PROJ database (2000 × 2000, 0.5 m,
   EPSG:2056). Then both model packages are verified ([pix](pix-model-assets.json),
   [rgb](rgb-model-assets.json)); the model buttons supply the same pinned ZIPs locally.
2. Choose AI overlap (64 px, 0–128), CPU threads (4) and the classical preset, then **Run all
   three methods**: classical, then pix_grad (DSM), then rgb_grad_fm (DSM, R, G, B), on a
   worker thread with phase and tile progress.
3. The estimates share one hillshade and switch together to removed heights (0–20 m) or signed
   reference errors (−2 to +2 m). **Input context** shows the orthophoto or the DSM relief.
4. The default **Shared comparison crop** (column 200 / row 800, 500 × 500 cells) dims the rest
   of each map; **Full input** scores the complete valid overlap. Both are saved.
5. Pan/zoom is linked; click a cell to read RGB bytes, DSM, all three estimates and reference.
6. **Open aligned DSM + RGB** loads custom sources (crop = central 60%); **Open reference**
   attaches an aligned evaluation surface without regenerating any heights.
7. **Save comparison folder** stages the three `<method>-dtm.tif` / `-removed.tif`, both
   `<AI>-raw.tif`, `classical-qa.tif`, the `comparison-*.tif` crops (heights and RGB) and
   `comparison.json`, then renames the folder to `dsm-ortho-dtm-<UTC time>-<id>`.

Custom RGB must be three RGB-ordered Byte bands on the DSM's exact grid covering every valid
DSM cell; RGB gaps outside valid DSM are context-filled from RGB only, DSM gaps from DSM only.

## Results

On the Zurich sample every output raster (heights, raw AI, QA, nine display maps, crops) is
**identical to the Qt build**, and so are the scores. Shared crop RMSE / MAE / bias: classical
1.018 / 0.355 / 0.253 m, pix_grad 0.659 / 0.407 / −0.277 m, rgb_grad_fm 0.663 / 0.399 /
−0.249 m; full input: 1.300 / 0.517 / 0.400, 1.749 / 0.981 / −0.668 and 1.615 / 0.877 /
−0.561 m. A small custom input with DSM and RGB gaps also matched exactly. Different
checkpoints and objectives mean this does not isolate the causal contribution of RGB.

## Implementation notes and differences from the Qt example

- Inference calls `GeoKernelAI_RunRasterInference` (the SDK's JSON raster inference C ABI)
  instead of the C++ `requestFromPackage`, binding it directly for tile progress. It takes one
  raster and a band list, so the example stacks the channels (context-filled DSM, then R, G, B)
  into one Float32 raster in the model's order, edge-pads inputs smaller than one tile and crops
  the result, and re-applies the source mask. The SDK reads every band as Float32, so inputs
  and results are the same as Qt's.
- `GeoKernel.AI.dll` is loaded with its own folder first in the dependency search, as the crate
  loads the SDK: Windows ships an unrelated `System32\onnxruntime.dll`.
- The inference call has no cancellation hook: **Cancel** and closing the window take effect
  when the running AI pass ends.
- Each map is a standalone viewer hosted through the GeoKernelViewer C ABI
  ([map2d.rs](src/map2d.rs)); views are linked by extent. The crop is dimmed with four
  translucent owned polygons and a solid outline; the probe is an overlay point that is moved,
  never removed. Sidebar labels and the score table are in the log panel and the strip below
  the maps. Models are prepared on the worker thread.

Data: Federal Office of Topography swisstopo. Model weights: David Perez, Zenodo 7973800 /
7973886, CC BY 4.0 (deterministic ONNX adaptations). Fixed development scene, not a held-out
benchmark.
