# Imagery attribution

The single demo raster is `buildings.tif` from:
https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/buildings.zip

Archive SHA-256: `f7cb0554b54c8e8bf31d576c38f988a8902382ad2ec3db3de6de05f5dbfb6f2b`.
The archive contains no source/license notice; provenance has not been verified.
Original imagery is preserved. Generated footprints are predictions, not ground truth.

Model credits: HOTOSM and Meta DINOv3. Preserve CC-BY-4.0, the separate DINOv3 License and ATTRIBUTION.txt from the model package. The SDK has separate license terms.


## Model package

- Model ID: `hotosm-dinov3s-buildings`; ONNX FP32, `geokernel-onnx-v1`.
- Upstream revision: `f7254545dfbbf10a40594d5a5fb62136902000ae`.
- [Download model ZIP](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/hotosm-dinov3s-buildings-geokernel-onnx-v1.zip).
- Archive SHA-256: `3a6cd11e013f81b46b3db700feddef2d98319780285b8320e1200573684b24bb`.
- RGB bands 1/2/3; source pixels and CRS are preserved, without demo downsampling.
- Default building probability threshold: 0.50. Mask, boundary and distance heads separate buildings;
  small regions are filtered. Touching buildings and other model errors can remain.
- Original imagery is not an accuracy benchmark. Its source/license must be verified before redistribution beyond this existing sample.
