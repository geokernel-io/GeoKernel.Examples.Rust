# Data and model sources

## Model

- Name: Google Dynamic World, nine-class Sentinel-2 land-cover segmentation.
- Upstream: [google/dynamicworld](https://github.com/google/dynamicworld/tree/0dd288fae5fb5291d988e5ea3c12eb91df481ec5).
- Upstream revision: `0dd288fae5fb5291d988e5ea3c12eb91df481ec5`.
- GeoKernel model ID: `google-dynamicworld`; package `geokernel-onnx-v1`, FP32 ONNX.
- [Download model package](https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/google-dynamicworld-geokernel-onnx-v1.zip).
- Archive SHA-256: `326bb2899cdea6e620a02f5e16fd330ec848e0a5b76f72f576038d7bc1e0aa63`.

The pinned upstream model repository declares Apache-2.0. Preserve its LICENSE,
NOTICE, attribution and GeoKernel conversion/modification records in the model
package. Credits: Google, National Geographic and World Resources Institute.
GeoKernel SDK licensing is separate. This package contains the forward model,
not the separately licensed Earth Engine hosted Dynamic World dataset.

## Sample imagery

- [Download spain_sentinel2_l1c.zip](https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/spain_sentinel2_l1c.zip).
- Archive SHA-256: `07811e9600882c526185268c5b280134e5715c95042754e414d6736fa5647839`.
- Area: Alicante, Spain; acquisition date: 2019-09-18.
- Sensor/product: Copernicus Sentinel-2B MSI, L1C, processing baseline 02.08.
- Original product: `S2B_MSIL1C_20190918T105029_N0208_R051_T30SXH_20190918T143346.SAFE`.
- Input: `dynamicworld_spain_sentinel2_l1c.tif`, 512 x 512, nine Float32 bands,
  10 m pixels, EPSG:32630.
- Input TIFF SHA-256: `26fe16b99ea90e909587cbcf426806b823923ddf6be4a0421194583840e7cf0e`.
- Display: `dynamicworld_spain_rgb.tif` (UInt8 RGB; not the inference input).
- Validity: `dynamicworld_spain_valid_mask.tif` (0 nodata, 255 observed).
  This is not a cloud/shadow mask.

The source was retrieved from the ESA/Copernicus archive mirrored in the
[Google Cloud public Sentinel-2 archive](https://storage.googleapis.com/gcp-public-data-sentinel-2/tiles/30/S/XH/S2B_MSIL1C_20190918T105029_N0208_R051_T30SXH_20190918T143346.SAFE/GRANULE/L1C_T30SXH_A013233_20190918T110002/IMG_DATA/).
The ZIP contains `dataset.json` with individual source-band URLs, object hashes
and processing details, plus `SOURCE-METADATA.xml` and `SHA256SUMS.txt`.

Required imagery attribution: **Contains modified Copernicus Sentinel data 2019.**
Retain `ATTRIBUTION.txt` and `COPERNICUS-SENTINEL-LEGAL-NOTICE.txt` from the archive.
The included [Copernicus Sentinel Data Legal Notice](https://cds.climate.copernicus.eu/licences/ec-sentinel)
applies to the imagery; the model's Apache-2.0 license does not replace it.

## Input contract and changes

Band order is B2, B3, B4, B5, B6, B7, B8, B11, B12. Values are raw L1C DN
(TOA reflectance scaled by 10000). The ONNX package embeds the required nonlinear
normalization: do not additionally divide the input by 255 or 10000.

GeoKernel sample preparation cropped a 512px region, selected/reordered bands,
bilinearly aligned 20 m bands to the 10 m grid, converted the stack to Float32,
and generated the separate RGB preview and nodata mask. No training labels or
precomputed classification output are bundled. Class 0 is valid water, not nodata.
Generated classes are predictions, not ground truth or an accuracy benchmark.
