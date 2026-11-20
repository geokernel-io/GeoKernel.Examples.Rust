# DsmToDtmProfile — Rust

Interactive DSM / DTM elevation profile: draw your own **A → B transect** on two linked
native GeoKernel **2D maps** and compare the original DSM, classical DTM, AI DTM and
provider reference heights on one chart. **No inference runs here.** Rust port of the
Qt `DsmToDtmProfile` example.

Requires Windows x64, Rust/Cargo with the MSVC toolchain and a valid GeoKernel license.
Uses published **geokernel 1.5.40** and automatically downloads/verifies the matching
native SDK and the pinned sample. There is no local SDK build, system GDAL or Python
requirement. This example has its own Cargo workspace and lockfile.

From this directory:

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

Options: `cargo run --locked --release -- --data-dir D:\cache`, `--sample-archive <zip>`
or `--sample-url <https-url>`. All hash pins still apply. The default cache is the
repository's `data` folder, shared with `DsmToDtm`. `GEOKERNEL_BIN` may point at a
compatible runtime instead of the downloaded SDK.

## Layout

The native window's control panel holds Draw (a toggle button), Example and Clear, the
right-hand surface, sample spacing, chart mode, Pan / Zoom box / Zoom in / Zoom out /
Full extent, CSV export, retry/cancel and loading progress. The log panel shows the
instructions, this line's statistics, the hovered sample and the data notes. The central
area holds the two pane headings, both maps and the profile chart as Win32 child windows.

## Workflow

1. Startup reuses the verified Zurich v2 sample (archive and every file SHA-256 checked
   against [sample-assets.json](sample-assets.json)) and validates the four numeric
   height rasters (500 × 500, 0.5 m, EPSG:2056).
2. The left map is always the original DSM; choose **Classical / AI / Reference** for the
   right map. Both maps share position and scale; pan or zoom either one.
3. The fixed transect loads initially. Press **Draw transect**, click **A**, then **B** on
   either map. Dragging still pans; pressing Draw again cancels an unfinished line.
   Clicks outside the valid pixel centres are rejected; endpoints must be at least 0.5 m
   apart.
4. The chart shows gray DSM, teal classical, purple AI and dashed orange reference;
   **Compare errors against reference** plots both estimates minus the reference.
5. Hovering over the chart marks the sample on both maps and lists its distance,
   coordinates and four heights. **Sample spacing** (0.5–10 m) resamples the line.
6. **Export profile CSV** writes the numeric samples and errors, then
   `<name>.csv.metadata.json`; each file atomically.
7. Cancel stops preparation; closing during loading stops and joins the worker before
   the maps are released.

Profiles sample the Float32 arrays bilinearly between pixel centres (no extrapolation,
no datum conversion); RMSE/MAE describe this line only. Both maps use the same fixed
hillshade (azimuth 315°, altitude 40°), written as display-only RGB GeoTIFFs.

## Implementation notes and differences from the Qt example

- The crate's `ViewerWindow` panes report mouse events only for the primary pane, so
  [map2d.rs](src/map2d.rs) hosts each map as a standalone viewer through the published
  GeoKernelViewer C ABI (like the WinForms control): created, reparented into a Win32
  host window and given its own event callback. Events are copied into a channel and
  handled after the event pump returns.
- The native control panel has no checkbox, so Draw is a toggle button and its state is
  shown in the status bar and the log panel.
- The transect is drawn with the viewer's owned shapes; the hover sample is a native
  overlay point. A teal and B orange markers carry no letter tags, and the hover point
  is parked far outside the map instead of removed: the runtime removes text labels and
  overlay points with Qt's `deleteLater`, which never runs when the host pumps
  `process_events` instead of a Qt event loop, so removed items would reappear.
- The viewer has no EPSG:2056 preset, so each map adopts the CRS of its first relief
  layer.

## Data

500 × 500 pixels at 0.5 m, EPSG:2056, heights in the source-declared LN02 reference
(EPSG:5728), no vertical datum conversion. Data: **Federal Office of Topography
swisstopo** (swissSURFACE3D Raster, swissALTI3D,
[OGD terms](https://www.swisstopo.admin.ch/en/terms-of-use-free-geodata-and-geoservices)).
AI: David Perez's [pix_grad checkpoint](https://doi.org/10.5281/zenodo.7973800), CC BY
4.0; the archive contains derived output only, no weights. DSM/reference source years
2018/2019; fixed development scene, line errors are descriptive comparisons.
