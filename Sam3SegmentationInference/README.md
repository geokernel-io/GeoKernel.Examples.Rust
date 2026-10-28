# SAM3 segmentation — Rust

## SDK and launch

Windows x64 and Rust are required. This standalone workspace uses the published **geokernel 1.5.39** crates.io package through `GeoAiCommon`. First launch downloads the matching Windows SDK from GitHub Releases, verifies SHA-256 against the release manifest and caches it at `packages/GeoKernel/1.5.39/windows-x64` in this repository. No sibling SDK source repository or native build is needed.

Inside `Sam3SegmentationInference`:

```powershell
cargo run --locked --release
```

Alternatively use `powershell -ExecutionPolicy Bypass -File run.ps1 -Release`. The launcher locates Cargo in the user profile when it is absent from PATH. Models and sample data download automatically when needed. Input/model preparation uses Windows curl and PowerShell/.NET ZIP support; GDAL comes from the SDK. No Python or model-framework dependency is required.

`GEOKERNEL_BIN` is an explicit compatible-runtime override and skips SDK download. It is never pointed at a sibling SDK build automatically.


## Workflow

Startup displays only the fixed buildings RGB sample. Choose **Text segmentation** for a concept such as `building`; Run scans overlapping tiles and creates a draft. Text score defaults to **0.50**. The reference gives **700 instances over 18 tiles** on the CUDA validation runtime.

For **Point / box segmentation**, select Positive point, Negative point or Box in the map-tool control. Click an object, or click two opposite corners tightly around **one object**. A box is an object prompt, not a region in which all buildings are extracted. Selected points and the box are drawn on the map. Negative-only input and incomplete boxes are rejected. Clear prompts resets the selection. At most 64 prompt tokens are accepted, with two reserved for a box.

The viewer explicitly uses WGS84. A background worker transforms map prompts into the source raster CRS using traditional GIS axis order and all four box corners. Tracker mode sends no text score threshold. The tracker fixture produces one instance for either a point or a box.

Review the draft, then **Accept** to enable Save Shapefile or Save accepted GeoTIFF. Neither export is allowed before acceptance. **Discard** removes the overlay and releases the temporary result. Results never reload on startup. The source remains unchanged.

Text and tracker are separate, multi-GB packages, downloaded only for the selected mode. SHA-256 hashing and ZIP extraction stream from disk without loading whole archives/weights into Rust memory. Downloads require several GB of cache space. Retain the SAM License and all package notices; imagery source/license is unverified.

## Lifecycle and runtime

Startup prepares the input only; **Run** prepares the model and starts inference. There are no automatic predictions or result reloads. Results are session-only until explicitly saved. The native Windows save dialog selects a new destination; existing datasets are not overwritten. Move the opacity slider right for a more opaque overlay.

Download, SHA-256 verification, extraction, RGB/CRS preparation, task start, export and release run on background workers. The main thread handles the viewer and short status/event polls. Cancellation includes model/input preparation and native inference. Closing keeps pumping events until workers and task cleanup finish. Layers are detached before temporary workspaces are released. Diagnostics preserve native failures.

The shared cache is `%LOCALAPPDATA%/GeoKernel/Examples/GeoAI`, overridable with `GEOKERNEL_EXAMPLES_DATA_DIR`. Archives verified by the other language examples are reused. Owned partial downloads and extraction staging are cleaned up on cancellation/failure. Auto currently uses CPU. Explicit CUDA requires a compatible GPU ONNX Runtime deployment plus its supported CUDA/cuDNN dependencies; unavailable CUDA produces an error instead of silently falling back. No machine-specific CUDA directory is embedded.

No embedded license keys, smoke modes or test arguments are included. See [data-sources.md](data-sources.md) for the exact URLs, hashes, licenses and provenance. Build/run uses published SDK 1.5.39.
