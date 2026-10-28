# HOTOSM building footprints — Rust

HOTOSM DINOv3-S building footprint segmentation on the fixed buildings RGB raster.

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `HotosmBuildingFootprints`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

Startup downloads/verifies only the fixed source archive and displays `buildings.tif`. Run prepares the fixed model and starts background segmentation. No predictions are loaded at startup or saved automatically.

Building threshold defaults to **0.50**. Input is the original 2503 × 1126 UInt8 RGB raster, EPSG:26911, 0.6 m pixels, without demo downsampling. Native inference uses mask, boundary and distance heads and filters small regions. At 0.50 the reference is 636 building instances/polygons over 78 tiles. This is a reproducibility count, not an accuracy score. Touching buildings and other model errors can remain.

The result is a red polygon overlay with yellow outlines. Move the opacity slider right for a more opaque result. Save Shapefile opens the native Windows destination dialog. Choose a new destination, since existing datasets are not overwritten. There are no instance-label path controls or saved `building-labels.tif` outputs. Results stay temporary until explicitly exported.

Input preparation, SHA-256 verification, task start, export and release run on background workers. The UI thread handles viewer operations, short task-status polls and queued control/events. Cancel is cooperative, including pending reruns during cleanup. Close keeps pumping viewer events until workers and renderer/task cleanup finish. Result layers are detached before their workspace is released.

Inputs share `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI` or `GEOKERNEL_EXAMPLES_DATA_DIR`, reusing verified .NET/Python/Electron archives. Interrupted downloads and owned extraction staging files are removed. Auto currently uses CPU. Explicit CUDA requires matching GPU ONNX Runtime/CUDA/cuDNN files, without silent CPU fallback.

Retain HOTOSM CC-BY-4.0, the separate Meta DINOv3 License, attribution and conversion notices. The imagery source/license remains unverified. See [data-sources.md](data-sources.md) for hashes and provenance. Predictions are not ground truth or cadastral boundaries. No embedded license keys, smoke modes or application test arguments are included.

Display polygons are converted in a background worker to temporary WGS84 GeoJSON. The original projected result remains the Shapefile export source. Results start at 100% opacity with red fill and yellow outlines; the slider applies opacity once at layer level. No predictions are restored at startup.
