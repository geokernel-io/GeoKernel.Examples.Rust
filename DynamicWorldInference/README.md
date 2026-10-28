# Dynamic World inference — Rust

Google Dynamic World nine-class land-cover classification on the fixed Spain Sentinel-2 L1C sample.

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `DynamicWorldInference`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

Startup prepares only the source imagery and displays its RGB preview. Run downloads/verifies the fixed model and starts native asynchronous classification. No predictions are loaded at startup or saved automatically. The reference gives 1400 polygons; this is a reproducibility count, not an accuracy score.

The slider becomes more opaque to the right and controls the new prediction at native layer index 0. Save Shapefile opens the native Windows file dialog. Choose a new destination; the native exporter does not replace existing datasets. All valid classes are exported, including water class 0. NoData is excluded.

Inference uses the original 512 × 512, nine-band L1C DN stack at 10 m, EPSG:32630. Normalization is embedded in the ONNX package; do not pre-scale the input. RGB display and observed-pixel validity mask are separate files. The validity mask is not a cloud/shadow mask. Class names and attribution appear in diagnostics.

Downloads, hash checks, ZIP extraction, task start, export and release run on background workers. Native inference owns its asynchronous task. The UI thread only handles viewer operations, short status polls and queued control/events. Cancel is cooperative; a pending rerun can be cancelled during cleanup. Close continues pumping events until workers and renderer/task cleanup finish. Result layers are detached before the old job/workspace is released.

Inputs share `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI` or `GEOKERNEL_EXAMPLES_DATA_DIR`, reusing verified .NET/Python/Electron archives. ZIPs are SHA-256 checked, extracted through an owned staging directory and checked for traversal/symlink entries. Failed/cancelled partial downloads and staging files are removed. Predictions remain temporary for the session until explicitly exported.

Auto currently selects CPU. Explicit CUDA requires a compatible GPU ONNX Runtime/CUDA/cuDNN deployment; the example does not install it or silently fall back from an explicit CUDA selection. Download cancellation may wait briefly for the child process to stop; export is intentionally not cancellable.

Keep model Apache-2.0 notices and **Contains modified Copernicus Sentinel data 2019** imagery attribution. Full URLs, hashes, acquisition metadata, band order and notices are in [data-sources.md](data-sources.md). No embedded license keys, smoke modes or application test arguments are included.
