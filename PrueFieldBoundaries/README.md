# PRUE field boundaries — Rust

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `PrueFieldBoundaries`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

The fixed upstream TIFF is downloaded as a raw file and SHA-256 verified. It contains two aligned Sentinel-2 periods, eight bands in total (B4/B3/B2/B8 for each period). Inference uses all eight scientific bands. A worker creates a separate first-period RGB preview; all eight bands participate in its validity mask. The original Float32 values, EPSG:32633 CRS and grid are preserved.

The CC-BY checkpoint produces **259 polygons over 2 tiles**. Classes are background (0), field interior (1), boundary (2); Save Shapefile exports **only class-1 interiors**. The model package supplies normalization and native processing settings. These are image-derived field divisions, not legal cadastral parcels. Weights are CC-BY-4.0; upstream code is MIT. This example does not use the FULL checkpoint.

## Lifecycle and runtime

Startup prepares the input only; **Run** prepares the model and starts inference. There are no automatic predictions or result reloads. Results are session-only until explicitly saved. The native Windows save dialog selects a new destination; existing datasets are not overwritten. Move the opacity slider right for a more opaque overlay.

Download, SHA-256 verification, extraction, RGB/CRS preparation, task start, export and release run on background workers. The main thread handles the viewer and short status/event polls. Cancellation includes model/input preparation and native inference. Closing keeps pumping events until workers and task cleanup finish. Layers are detached before temporary workspaces are released. Diagnostics preserve native failures.

The shared cache is `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI`, overridable with `GEOKERNEL_EXAMPLES_DATA_DIR`. Archives verified by the other language examples are reused. Owned partial downloads and extraction staging are cleaned up on cancellation/failure. Auto currently uses CPU. Explicit CUDA requires a compatible GPU ONNX Runtime deployment plus its supported CUDA/cuDNN dependencies; unavailable CUDA produces an error instead of silently falling back. No machine-specific CUDA directory is embedded.

No embedded license keys, smoke modes or test arguments are included. See [data-sources.md](data-sources.md) for the exact URLs, hashes, licenses and provenance. Build/run uses published SDK 1.5.39.

PRUE model v2 uses the same CCBY checkpoint at native resolution with 384 × 384 tiles and 128-pixel overlap. It requires SDK 1.5.39. Larger context reduces the rectangular top-left omission in this sample; this is a visual regression check, not a general accuracy benchmark. Model v1 remains available under its original URL.
