# BuildingChangeReview — Rust

Compare Oosterwold in 2016 and 2025 using synchronized native GeoKernel map
panes. Analyze change candidates, edit, review and export approved GeoJSON.
The native panel uses a candidate dropdown, matching the Electron workflow.

Requires Windows x64, Rust/Cargo with the MSVC toolchain, a supported graphics
driver and a valid GeoKernel license. Uses published **geokernel 1.5.39** and
automatically downloads/verifies the matching native SDK and demo assets.
There is no local SDK build or system GDAL requirement. This example has its own
Cargo workspace and lockfile, preserving other examples' dependency versions.

From this directory:

```powershell
.\run.ps1 -Release
# or:
cargo run --locked --release
```

## Workflow

Startup prepares the two images and model; initially only imagery is shown.
The SDK's GDAL runtime creates WGS84 display copies in the background while
analysis retains the original EPSG:25831 rasters. Preparation errors appear in
the status bar; **Run analysis** retries loading if preparation failed.
Choose **Demo area (fixed)** (320 m square), **Current view**, or **Full dataset**
(1024 m square), then **Run analysis**. Default CPU inference uses threshold 0.4371.
Choose a Candidate and use Locate to center it on both dates.

Green denotes additions, red removals and gray rejections. Orange marks the
selected location on both dates; its reference on the old image does not mean
a building existed there. Edit vertices in the right pane, with undo/redo within
the current native edit session. The public Rust single-ring editing API edits
the exterior ring only: original hole rings are preserved in the resulting
geometry, and multipart geometry is not flattened for editing.

Selection highlights use persistent reference features, so changing candidates
does not reload the map layers. The editable layer is created only when
**Edit vertices** is chosen. Review decisions update styles without replacing
layers; geometry changes refresh the candidates when leaving the edit session.

Approve, Reject or Pending records reviewer and note. An edit recalculates
metric area and resets approval to pending. Export writes approved candidates
only, checking that the current geometry matches its approval hash. GeoJSON
includes WGS84 geometry, metric area, dates, scores and data/model/parameter
provenance. Decisions are session-only until export; a new analysis asks before
replacing them.

A background worker owns inference. Cancel retains previous published results.
Instance inference cancels cooperatively; the extra score pass finishes before
cancel/close completes. Closing joins the worker, releases viewers and then
removes session files. Map synchronization occurs outside native callbacks.

## Analysis and Qt comparison

The native `hotosm-buildings` task produces instances for each date. A second
`FloatBands` inference per date supplies matching scores because the public
1.5.39 API does not expose decoded scores from the instance task. Sigmoid is
applied after logit blending; Qt decodes before tile blending. Filters have the
same defaults, but results are not bit-identical and the extra inference costs time.

| Filter | Minimum area | Other-date coverage below | Tolerance | Own mean at least | Other mean at most |
|---|---:|---:|---:|---:|---:|
| Conservative | 50 m² | 0.10 | 2 m | 0.75 | 0.20 |
| Balanced | 20 m² | 0.20 | 1 m | 0.55 | 0.35 |

Coverage uses buffered other-date scores ≥ 0.25. Border-touching components are
excluded. Analysis/areas use EPSG:25831; viewing/export use WGS84.
CPU functional validation produced 0/68 instances and 42 addition candidates
in the fixed demo, matching the other three ports. These are model predictions;
no reference change labels are included, so counts are not accuracy measures.

## Data and model

[Imagery ZIP](https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/almere_oosterwold_change_2016_2025_v1.zip)
contains the two `oosterwold_YEAR-epsg25831-50cm.tif` images, README, attribution
and source manifest. Each image is 2048 × 2048 RGB Byte at 0.5 m. EPSG:25831
bounds: `657533,5800382,658557,5801406`; demo bounds:
`657885,5800734,658205,5801054`.

Imagery: Beeldmateriaal Nederland / PDOK,
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/);
[provider attribution](https://www.beeldmateriaal.nl/dataroom).
The package contains WMS JPEG renders of 0.25 m annual layers at 0.5 m,
converted without further pixel loss to GeoTIFF. Years identify annual layers,
not verified local flight dates. Shadows, vegetation and registration can cause
false changes that need review.

[HOTOSM DINOv3-S package](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/hotosm-dinov3s-buildings-geokernel-onnx-v1.zip)
retains attribution, model/dataset cards, CC BY 4.0 and the separate Meta DINOv3
license. Preserve those files and applicable terms when redistributing.

| Object | SHA-256 |
|---|---|
| Imagery ZIP | `ecc35ef2c6bbdd586adc6ecc812ce1c74aee62fbf8e404de1d63b75bf7137a88` |
| 2016 image | `db723783db4c8738a716fe8b898de2e3099bab9394e335bd489db2412ae0c5c3` |
| 2025 image | `7b1b12d8b27cd0714f140415004daf9fd0000495eda31366c7cd4aab218caa82` |
| Model ZIP | `3a6cd11e013f81b46b3db700feddef2d98319780285b8320e1200573684b24bb` |
| model.onnx | `9a28e28403accc06c70a3104aedb31eca50401ed23c8ba4314b26357c25e9043` |

The shared asset cache is `%LOCALAPPDATA%\GeoKernel\Examples\GeoAI`, indexed
by archive hash; override with `GEOKERNEL_EXAMPLES_DATA_DIR`. Original imagery
is unchanged. The published SDK is acquired through `GeoAiCommon`.

## Verification

```powershell
cargo test --locked
cargo run --locked --release --example verify
```

The optional verification executable runs real CPU inference in the fixed demo
and checks native dual-view publication, edit setup, approval/export gating and
edit invalidation. It needs the same runtime/model as the interactive app and
writes diagnostic artifacts to ignored `target/validation`. It does not establish accuracy.
