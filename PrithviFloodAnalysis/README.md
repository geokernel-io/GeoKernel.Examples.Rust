# Prithvi flood analysis — Rust

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `PrithviFloodAnalysis`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

The fixed India/Sen1Floods11 source has 13 Sentinel-2 L1C bands. The native task selects B2/B3/B4/B8A/B11/B12 (source indices 2/3/4/9/12/13) and preserves the scientific input. A worker creates a separate B4/B3/B2 RGB preview with a per-channel 2–98 percentile stretch and a validity alpha mask. All selected bands participate in validity; the original TIFF is opened read-only.

The reference produces **57 polygons over 9 tiles**. Class 0 is non-water and class 1 is water/flood; invalid input is excluded. The raster preview displays both classes, while Save Shapefile exports **only class 1**. A single acquisition cannot identify which water is permanent or newly flooded. Model weights use Apache-2.0; retain notices and consult the sample's processed-chip provenance restrictions.

## Lifecycle and runtime

Startup prepares the input only; **Run** prepares the model and starts inference. There are no automatic predictions or result reloads. Results are session-only until explicitly saved. The native Windows save dialog selects a new destination; existing datasets are not overwritten. Move the opacity slider right for a more opaque overlay.

Download, SHA-256 verification, extraction, RGB/CRS preparation, task start, export and release run on background workers. The main thread handles the viewer and short status/event polls. Cancellation includes model/input preparation and native inference. Closing keeps pumping events until workers and task cleanup finish. Layers are detached before temporary workspaces are released. Diagnostics preserve native failures.

The shared cache is `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI`, overridable with `GEOKERNEL_EXAMPLES_DATA_DIR`. Archives verified by the other language examples are reused. Owned partial downloads and extraction staging are cleaned up on cancellation/failure. Auto currently uses CPU. Explicit CUDA requires a compatible GPU ONNX Runtime deployment plus its supported CUDA/cuDNN dependencies; unavailable CUDA produces an error instead of silently falling back. No machine-specific CUDA directory is embedded.

No embedded license keys, smoke modes or test arguments are included. See [data-sources.md](data-sources.md) for the exact URLs, hashes, licenses and provenance. Build/run uses published SDK 1.5.39.
