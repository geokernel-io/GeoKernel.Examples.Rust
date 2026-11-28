# GeoKernel Rust Examples

Official Rust examples for the GeoKernel native GIS SDK.

## Requirements

- Windows 10 or later
- Rust and Cargo
- A supported MSVC toolchain
- A valid GeoKernel license

## Getting Started

Choose an example directory and run it with the included Cargo configuration or build scripts.

## AgentMapAssistant

From `AgentMapAssistant`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run
`./run.ps1 -Release`. Ask for map operations in plain language; an LLM (or the offline demo's
prepared plans) proposes a workflow DSL v2 plan that GeoKernel validates, previews, runs after
confirmation when it changes data, undoes and records with hash-chained provenance. Needs
geokernel newer than 1.5.40. See [AgentMapAssistant instructions](AgentMapAssistant/README.md).

## AgentPlannerBasics

From `AgentPlannerBasics`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run `./run.ps1 -Release`.
Five lessons send map requests through the planner one stage at a time (proposal, validation, execution,
provenance): the same plan gives the same result, bad proposals never run, confirmation is bound to one
exact plan and the provenance record shows any change. Needs geokernel newer than 1.5.40.
See [AgentPlannerBasics instructions](AgentPlannerBasics/README.md).

## TextPromptChangeDetection

From `TextPromptChangeDetection`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run
`./run.ps1 -Release`. Type what to look for ("building", "greenhouse") and see where it appeared
and disappeared between two dates or over a 9-date series, with a timeline, a QA review queue and
GeoPackage export (SAM 3, objects tracked from date to date). Needs geokernel newer than 1.5.40.
See [TextPromptChangeDetection instructions](TextPromptChangeDetection/README.md).

## SimilaritySearch

From `SimilaritySearch`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run
`./run.ps1 -Release`. Pick a solar park, lake, town or field (or click the map) and rank every
80 m cell of Flevoland by AlphaEarth embedding similarity, with a heat map and ranked matches.
Needs geokernel newer than 1.5.40. See [SimilaritySearch instructions](SimilaritySearch/README.md).

## SpatialEmbeddingSearch

From `SpatialEmbeddingSearch`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run `./run.ps1 -Release`.
Search AlphaEarth embeddings of the Netherlands (3 years, 2 UTM zones) for places like a seed place, limited
to one CRS, extent and time span, with the records each filter rejected. Needs geokernel newer than 1.5.40.
See [SpatialEmbeddingSearch instructions](SpatialEmbeddingSearch/README.md).

## VlmImageQA

From `VlmImageQA`, set `GEOKERNEL_RUST_SOURCE` and `GEOKERNEL_BIN`, then run `.\run.ps1 -Release`.
Pick an area of an aerial photo; Florence-2 describes it and marks objects named in plain language on this
computer, and a remote vision model (optional) answers questions about it. Needs geokernel newer than 1.5.40.
See [VlmImageQA instructions](VlmImageQA/README.md).

## DsmToDtm

From `DsmToDtm`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Uses the published SDK 1.5.40. Ground beneath buildings: two linked native 3D views
of an original DSM and prepared classical/AI terrain, numeric height transition,
fixed transect profile and reference metrics. The pinned Zurich sample is
downloaded and SHA-256 verified automatically; no inference runs.
See [DsmToDtm instructions](DsmToDtm/README.md).

## DsmToDtmProfile

From `DsmToDtmProfile`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Uses the published SDK 1.5.40. Draw your own A → B transect on two linked native 2D
relief maps and compare DSM, classical, AI and reference heights on one chart, with an
error mode, hover inspection, line RMSE/MAE and CSV export. Reuses the verified
DsmToDtm sample. See [DsmToDtmProfile instructions](DsmToDtmProfile/README.md).

## DsmToDtmAccuracy

From `DsmToDtmAccuracy`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Uses the published SDK 1.5.40. Three linked native 2D maps of the DSM and shared-scale
classical/AI reference errors; area selection and presets, RMSE/MAE/bias/P95, histogram,
pixel probe and JSON report. See [DsmToDtmAccuracy instructions](DsmToDtmAccuracy/README.md).

## DsmToDtmProcessing

From `DsmToDtmProcessing`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Run the classical multiscale-morphological + TIN conversion live with presets and six
parameters; DSM, new DTM and removed-height/quality/reference-error maps, scores, pixel
probe and GeoTIFF/QA/JSON export. See [DsmToDtmProcessing instructions](DsmToDtmProcessing/README.md).

## DsmToDtmAI

From `DsmToDtmAI`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Run the experimental pix_grad model through GeoKernel.AI and the classical filter on the same
full 1 km Zurich DSM; relief/removed-height/error maps, full-input and shared-crop scores,
pixel probe and a staged comparison-folder export. Model and terrain are downloaded and
hash-verified on first start. See [DsmToDtmAI instructions](DsmToDtmAI/README.md).

## DsmOrthoToDtm

From `DsmOrthoToDtm`, run `cargo run --locked --release` or `./run.ps1 -Release`.
Compare classical, DSM-only pix_grad and DSM+RGB rgb_grad_fm terrain estimates on the same
full Zurich DSM; orthophoto/DSM input context, four linked 2D maps, full-input and shared-crop
scores, pixel probe and a staged comparison-folder export. See
[DsmOrthoToDtm instructions](DsmOrthoToDtm/README.md).

## BuildingChangeReview

From `BuildingChangeReview`, run `cargo run --locked --release` or `./run.ps1 -Release`.
The example uses the published SDK 1.5.39 and automatically prepares Oosterwold
2016/2025 imagery and the HOTOSM model. Includes synchronized maps, change
candidates, vertex editing, review decisions and approved-only GeoJSON export.
See [BuildingChangeReview instructions](BuildingChangeReview/README.md).

## GlobeHello

From `GlobeHello`, run `cargo run --locked --release`. The example automatically
downloads and verifies the published Windows x64 SDK 1.5.35. Its own Cargo
workspace and lockfile keep other examples' SDK versions unchanged.

Atmosphere, Esri Satellite, DEM and Sunlight can be enabled independently from
the Globe panel. All start off, matching the other GlobeHello examples. Includes
the application icon, attribution/status bar and normal native resource cleanup.
See [GlobeHello instructions](GlobeHello/README.md).

## Resources

- [GeoKernel Website](https://geokernel.io)
- [Documentation](https://docs.geokernel.io)
- [Sample Data](https://github.com/geokernel-io/GeoKernel.SampleData)

## Support

For questions, contact [admin@geokernel.io](mailto:admin@geokernel.io).

## RoadsOnTerrain

Run `cargo run --locked -p roads-on-terrain --release`. The example automatically
prepares the published SDK and downloads the Sagrada Família DEM, orthophoto
and roads. It provides road visibility, color and opacity controls, terrain
resolution, vertical exaggeration and camera reset.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.

## Buildings3D

Run `cargo run --locked -p buildings-3d --release`. The example automatically
prepares the published SDK and downloads the Sagrada Família DEM, orthophoto
and building footprints. Buildings are clamped to terrain with an illustrative
9 metre default height. Change the height and select **Apply height / Reload
sample**. Visibility, color, opacity and terrain exaggeration are adjustable.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.

## ModelPlacement

Run `cargo run --locked -p model-placement --release`. The example automatically
prepares the published SDK and downloads the textured Sagrada Família model,
DEM, orthophoto and EGM08D595 geoid. Placement controls include longitude,
latitude, height above terrain, heading, pitch, roll and scale. Select **Apply
placement / Reload** after changes. Model visibility, focus and imagery controls
are included; no placeholder model is displayed.

The example uses the published `GeoKernel 1.5.32 SDK` package. No local SDK build is required.
Placement is approximate. The downloaded model retains its source and license.

## ElevationProfile

`ElevationProfile` downloads the SDK, DEM and orthophoto automatically. Draw a
terrain route to show its elevation graph and ascent/descent totals. Includes
sample spacing, finish, undo and clear controls. Uses the same native profile API
as Qt, .NET, Python and Electron; NoData gaps are not joined.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ElevationProfile\run.ps1" -Release
```

## ViewshedAnalysis

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "ViewshedAnalysis\run.ps1" -Release
```

## LayerStylingAndFiltering

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

## ProjectSaveLoad

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

## GlobeTerrainNavigation

From `GlobeTerrainNavigation`, run `cargo run --locked --release`. The example
prepares published SDK 1.5.35 automatically and opens the Barcelona terrain view.
Includes Esri imagery, DEM, atmosphere, sunlight and nine camera controls.
See [example instructions](GlobeTerrainNavigation/README.md).

## GlobeVectorStyling

[GlobeVectorStyling](GlobeVectorStyling/README.md) downloads real Sagrada Familia
buildings and displays them over Esri imagery. Includes fills, category colors,
labels, opacity, base color, camera reset and identification. Uses the published
1.5.35 SDK with automatic download and checksum verification.

~~~powershell
cd GlobeVectorStyling
cargo run --locked --release
~~~

## GlobeBuildings

[GlobeBuildings](GlobeBuildings/README.md) displays real Sagrada Familia buildings
with Esri imagery and atmosphere. Switch Extrude on/off and choose an illustrative
height. It automatically downloads the published 1.5.35 SDK and sample data, and
waits for GPU upload before re-enabling controls.

```powershell
cd GlobeBuildings
cargo run --locked --release
```

## GlobeModelPlacement

[GlobeModelPlacement](GlobeModelPlacement/README.md) automatically downloads the
published 1.5.35 SDK and real Sagrada Familia model. Includes Esri, atmosphere,
rotation, scale/height and remove/restore controls. See its README for the known
SDK precision limitation affecting some placements.

```powershell
cd GlobeModelPlacement
cargo run --locked --release
```

## GlobeRasterServices

[GlobeRasterServices](GlobeRasterServices/README.md) automatically downloads SDK
1.5.35 and real Sagrada Familia orthophoto/DEM. Both layers open with Esri and
atmosphere; DEM color shading is disabled and there is no service toolbar.

```powershell
cd GlobeRasterServices
cargo run --locked --release
```

## GlobeTerrainAnalysis

[GlobeTerrainAnalysis](GlobeTerrainAnalysis/README.md) downloads SDK 1.5.35 and
the real Barcelona DEM. Includes interactive line of sight, viewshed, slope/aspect
and measurement with a fixed-height result panel.

```powershell
cd GlobeTerrainAnalysis
cargo run --locked --release
```

## GeoAI task examples (published SDK 1.5.39)

- [Dynamic World inference](DynamicWorldInference/README.md): fixed Sentinel-2 input, background classification, opacity slider and explicit Shapefile save dialog.
- [HOTOSM building footprints](HotosmBuildingFootprints/README.md): fixed RGB source, threshold 0.50, polygon overlays and explicit Shapefile save dialog.
- [GeoAI USA building footprints](GeoAiUsaBuildingFootprints/README.md): Mask R-CNN with confidence threshold 0.50, polygon overlays and explicit Shapefile save dialog.

- [Prithvi flood analysis](PrithviFloodAnalysis/README.md): fixed input/model, asynchronous inference and explicit result export.
- [PRUE field boundaries](PrueFieldBoundaries/README.md): fixed input/model, asynchronous inference and explicit result export.
- [SEN2SR super resolution](Sen2srSuperResolution/README.md): fixed input/model, asynchronous inference and explicit result export.
- [SAM3 segmentation](Sam3SegmentationInference/README.md): fixed input/model, asynchronous inference and explicit result export.

```powershell
cargo run --locked --release --manifest-path DynamicWorldInference/Cargo.toml
```

The seven GeoAI examples use standalone Cargo workspaces and `GeoAiCommon`, with geokernel 1.5.39 from crates.io. First launch downloads and SHA-256 verifies the matching Windows SDK from GitHub Releases. No sibling SDK checkout is required.
