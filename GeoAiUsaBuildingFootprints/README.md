# GeoAI USA building footprints — Rust

USA-trained Mask R-CNN ResNet50 FPN building extraction on the fixed buildings RGB sample.

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `GeoAiUsaBuildingFootprints`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

Startup downloads/verifies the fixed source archive and displays only `buildings.tif`. Run prepares the fixed model and starts asynchronous instance segmentation. No predictions are loaded at startup or saved automatically.

Confidence threshold defaults to **0.50**. The native mask threshold and NMS IoU remain 0.50; the confidence control does not change those settings. The original 2503 × 1126 RGB pixels, EPSG:26911 and 0.6 m grid are preserved, without demo downsampling. Native inference uses 512px tiles with 128px overlap and filters masks smaller than 16 pixels.

The reference gives 746 building instances and 857 polygon features over 21 tiles. Disconnected polygon parts can produce multiple features per instance, so those counts need not match. This verifies reproducibility, not accuracy. The checkpoint is USA-trained and does not guarantee equivalent performance in other regions.

The result is a red polygon overlay with yellow outlines. Move the opacity slider right for a more opaque result. Save Shapefile opens the native Windows destination dialog; choose a new destination, since existing datasets are not overwritten. There are no instance-label path controls or saved `building-labels.tif` outputs. Results stay temporary until explicitly exported.

Input preparation, SHA-256 checks, task start, export and release run on background workers. The UI thread handles viewer operations, short status polls and queued control/events. Cancel is cooperative, including pending reruns during cleanup. Close keeps pumping viewer events until worker and renderer/task cleanup finish. Result layers are detached before the old workspace is released.

Inputs share `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI` or `GEOKERNEL_EXAMPLES_DATA_DIR`, reusing verified .NET/Python/Electron archives. Interrupted downloads and owned extraction staging files are removed. Auto currently uses CPU. Explicit CUDA requires matching GPU ONNX Runtime/CUDA/cuDNN files without silent CPU fallback.

Existing ONNX internal dynamic-mask shape warnings may appear in runtime logs. They are retained; output counts are checked separately. Other failures are reported in diagnostics.

Retain the package MIT model declaration, third-party attribution and conversion notices. Imagery source/license remains unverified. See [data-sources.md](data-sources.md) for exact URLs, hashes and provenance. Footprints are predictions, not ground truth or cadastral boundaries. No embedded license keys, smoke modes or application test arguments are included.

Display polygons are converted in a background worker to temporary WGS84 GeoJSON. The original projected result remains the Shapefile export source. Results start at 100% opacity with red fill and yellow outlines; the slider applies opacity once at layer level. No predictions are restored at startup.
