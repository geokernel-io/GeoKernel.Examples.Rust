# Data and model sources

## Model

- Name: GeoAI USA building footprints, Mask R-CNN ResNet50 FPN.
- Upstream checkpoint: `building_footprints_usa.pth` from
  [giswqs/geoai](https://huggingface.co/giswqs/geoai/tree/aa2b25d27b2c78f0e92c7b6a4813a571f9bc2288).
- Upstream revision: `aa2b25d27b2c78f0e92c7b6a4813a571f9bc2288`.
- GeoKernel model ID: `giswqs-geoai-usa-maskrcnn-resnet50-fpn`;
  package `geokernel-onnx-v1`, FP32 ONNX.
- [Download model package](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/giswqs-geoai-usa-maskrcnn-resnet50-fpn-geokernel-onnx-v1.zip).
- Archive SHA-256: `055097781e125083201421c37c70d36336f2f968b69b1aa09350578d2c87ca32`.
- Original checkpoint SHA-256: `3aea5d0da7803be31ff4e2e1e5ac747f15cab2a13d6bb1a9319b6a6ba8bc7bca`.

The [pinned upstream model card](https://huggingface.co/giswqs/geoai/blob/aa2b25d27b2c78f0e92c7b6a4813a571f9bc2288/README.md)
declares MIT. Preserve the package's `LICENSE-MIT.txt`,
`LICENSE-TORCHVISION-BSD.txt`, `LICENSE-PYTORCH.txt`, `ATTRIBUTION.txt` and conversion
notices. Complete training-data and initialization provenance is not documented
by that model card; the MIT declaration is not a blanket statement about
undocumented third-party rights. GeoKernel SDK licensing is separate.

## Sample imagery

- [Download buildings.zip](https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/buildings.zip).
- Archive SHA-256: `f7cb0554b54c8e8bf31d576c38f988a8902382ad2ec3db3de6de05f5dbfb6f2b`.
- Raster: `buildings.tif`, 2503 x 1126, three UInt8 RGB bands, 0.6 m pixels.
- CRS: NAD83 / UTM zone 11N (EPSG:26911 equivalent).
- TIFF SHA-256: `a44d4c26ab51aa69daf0d648ed02ee8846ddd724e8b42f32f6199e0bc8943bb1`.

The download location is GeoKernel.SampleData, but the original imagery producer,
acquisition date and imagery license have not been verified. The ZIP contains no
source/license notice. Do not infer a CC-BY, MIT or public-domain imagery license
from the hosting repository or from the model license. Additional attribution or
redistribution terms cannot be completed until the original imagery source is
identified. The original raster is preserved; it is downloaded rather than
committed into this example's source repository.

## Input contract and outputs

This is a USA-trained RGB aerial checkpoint; transfer accuracy outside that domain
has not been established. It is not a Sentinel-2 multispectral land-cover model.

The manifest uses 512 x 512 RGB tiles with 128px overlap. Its input adapter scales
byte values by 1/255; ImageNet normalization and the model's internal resizing are
embedded in ONNX and must not be applied a second time. Associated outputs are
boxes, scores, classes and instance masks. The example's default confidence and
mask thresholds are 0.50. These outputs need the example's dedicated instance
postprocessing, not a semantic class-channel decoder.

Footprints are generated in the current session and can be explicitly exported
as Shapefile. They are predictions, not cadastral boundaries, ground truth or an
independent accuracy benchmark. Model packages do not contain the demo imagery.
