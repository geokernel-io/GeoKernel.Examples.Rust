# SAM3 model and sample sources

## Models

Upstream: Meta `facebook/sam3`, revision `3c879f39826c281e95690f02c7821c4de09afae7`.
Specialized package schema: `geokernel.sam3-package/1` with `sam3-package.json`.
The native SDK verifies the bundle files and graph contracts.

- [Text package](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/meta-sam3-text-fp32-geokernel-onnx-v1.zip), FP32, 3,087,973,902 bytes.
  SHA-256: `95d9bc3ebdec6a6147a9bfa97325712bfbb77fccbd6bb3f20be453c381820931`.
- [Tracker package](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/meta-sam3-tracker-fp32-geokernel-onnx-v1.zip), FP32, 1,701,342,048 bytes.
  SHA-256: `c4309ed67b2292f64641d740f2558c2d4253476ca1ec40c17d49c4800c050896`.

Retain the original SAM License, attribution, package README and conversion notices from both bundles.
The SAM License has its own distribution, use and export conditions; adding a copyright line alone does not replace those conditions.
GeoKernel SDK licensing is separate. This example does not modify or republish model weights.

## Imagery

[buildings.zip](https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/buildings.zip).
Archive SHA-256: `f7cb0554b54c8e8bf31d576c38f988a8902382ad2ec3db3de6de05f5dbfb6f2b`.
`buildings.tif`: 2503 × 1126, UInt8 RGB, source CRS EPSG:26911, 0.6 m pixels.
TIFF SHA-256: `a44d4c26ab51aa69daf0d648ed02ee8846ddd724e8b42f32f6199e0bc8943bb1`.
The original imagery source, acquisition date and imagery license are not verified; the ZIP contains no source/license notice.
Do not infer imagery redistribution rights from the hosting repository or model license.
The raster is downloaded on demand and preserved; no predictions or imagery are committed to the example repository.

## Input/output semantics

Text scans overlapping 512px tiles with 96px overlap, at most 200 instances per tile, score threshold 0.50 by default.
Point/box prompts use a separate tracker profile and select an object mask. A large box is not a request to detect all buildings inside it.
The display map is explicitly EPSG:4326. Before inference GDAL transforms prompt points and all four box corners to the original raster CRS with traditional GIS axis order.
The source image is never reprojected/resampled for inference. Results retain source coordinates.

Results first appear as a draft. Accept enables explicit Shapefile or instance-ID GeoTIFF export. Discard releases temporary files.
The GeoTIFF contains object IDs, not semantic class IDs or confidence values. Predictions are not ground truth or cadastral boundaries.
