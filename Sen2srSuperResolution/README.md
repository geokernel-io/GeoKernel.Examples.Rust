# SEN2SR super resolution — Rust

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `Sen2srSuperResolution`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

The fixed FTW Sentinel-2 TIFF supplies first-period RGB + NIR (the first four bands). Inference preserves the spectral source and generates **2312 × 1516, four-band Float32 reflectance** from the 578 × 379 input, using 28 tiles and a 4× finer grid.

The native task prepares source and enhanced RGB previews with the **same contrast stretch**. The opacity slider moves right toward the enhanced image. Save GeoTIFF exports the scientific four-band result, not a screenshot: CRS and footprint are retained, pixel size/rotation terms are divided by four, and AI provenance is included. The fourth band is NIR, not alpha. The result estimates detail; it is not a new sensor observation.

Both comparison previews belong to the job. Rerun detaches both and restores the persistent source preview before releasing the old workspace. Only GeoTIFF export is offered. Retain CC0-1.0 and inherited Apache-2.0 notices.

## Lifecycle and runtime

Startup prepares the input only; **Run** prepares the model and starts inference. There are no automatic predictions or result reloads. Results are session-only until explicitly saved. The native Windows save dialog selects a new destination; existing datasets are not overwritten. Move the opacity slider right for a more opaque overlay.

Download, SHA-256 verification, extraction, RGB/CRS preparation, task start, export and release run on background workers. The main thread handles the viewer and short status/event polls. Cancellation includes model/input preparation and native inference. Closing keeps pumping events until workers and task cleanup finish. Layers are detached before temporary workspaces are released. Diagnostics preserve native failures.

The shared cache is `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI`, overridable with `GEOKERNEL_EXAMPLES_DATA_DIR`. Archives verified by the other language examples are reused. Owned partial downloads and extraction staging are cleaned up on cancellation/failure. Auto currently uses CPU. Explicit CUDA requires a compatible GPU ONNX Runtime deployment plus its supported CUDA/cuDNN dependencies; unavailable CUDA produces an error instead of silently falling back. No machine-specific CUDA directory is embedded.

No embedded license keys, smoke modes or test arguments are included. See [data-sources.md](data-sources.md) for the exact URLs, hashes, licenses and provenance. Build/run uses published SDK 1.5.39.
