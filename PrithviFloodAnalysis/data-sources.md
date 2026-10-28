# Data and model sources

Model: IBM/NASA Prithvi EO2.0 300M-TL fine-tuned on Sen1Floods11.
Upstream: https://huggingface.co/ibm-nasa-geospatial/Prithvi-EO-2.0-300M-TL-Sen1Floods11/tree/91ce9d38086a80b078a192b374df758b8855b732
Model license: Apache-2.0 as declared by the pinned model card. Retain the package LICENSE, NOTICE, ATTRIBUTION.txt, MODIFICATIONS.txt and other source notices. GeoKernel SDK license is separate.

R2 model: https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/ibm-nasa-prithvi-eo2-300m-tl-sen1floods11-geokernel-onnx-v1.zip
SHA256:34711ae55d48fe265b19c18551aef4b4aa84045a07bffb407161570b19429c13

Raster archive: https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/flood_analysis.zip
SHA256:059e9a84bed08ea5da76941432e6e8409bd1352661ff72f750e47df5f740c38f
Contents: India_900498_S2Hand.tif from the upstream model examples,13bands,512x512,WGS84. Underlying Sentinel imagery is Copernicus data; the image chip is provided via Sen1Floods11 by Cloud to Street. Cite Bonafilia et al., Sen1Floods11, CVPR Workshops2020.

The user uploaded the raster ZIP. This example references that existing asset; it does not upload or include imagery in the source repository. Model Apache-2.0 does not automatically license the dataset. Sen1Floods11 repository has no clear dataset LICENSE file; redistribution rights for its processed chips need independent confirmation before broader release. See https://github.com/cloudtostreet/Sen1Floods11/issues/18 . No ground-truth labels or cloud mask are bundled. Do not present this demonstration as an independent accuracy evaluation.


## .NET input and output contract

The 13-band input is preserved. Inference selects B2/B3/B4/B8A/B11/B12; raw L1C DN normalization is embedded in the ONNX graph. Do not convert the source to RGB or normalize twice for inference.

A separate byte RGBA source display image uses B4/B3/B2 and a per-band 2–98% stretch, retaining the source CRS/affine. Its alpha excludes invalid pixels. This display image is cached as a derived input, not a prediction.

The task creates class 0 (non-water), class 1 (water/flood), and NoData -1. The preview displays both valid classes; Shapefile export contains only class-1 polygons. A single image does not distinguish permanent water from newly flooded land. Predictions are not ground truth.
