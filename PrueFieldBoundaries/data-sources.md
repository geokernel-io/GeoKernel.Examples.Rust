# Sources and licenses

Model: Fields of The World / PRUE EfficientNet-B5 CCBY, v3.1.
Pinned repository: https://github.com/fieldsoftheworld/ftw-baselines/tree/779b809813b301de7fb9f00f0c5db07086b23227
Model license declaration: https://huggingface.co/isaaccorley/ftw-prue-ccby/blob/59f3136ceb4904dba8eda40e0168b206071cbe6b/README.md
Model weights CC-BY-4.0; copied upstream code MIT. Retain package license, attribution and modification records. GeoKernel SDK proprietary licensing is separate; it must not restrict model rights granted by CC-BY.
Model package: https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/fieldsoftheworld-prue-efficientnet-b5-ccby-geokernel-onnx-v2.zip
SHA256 0035f723ae5753eb861e056bbb38b60605617485c9700cc16d3317856374da44

Input sample: upstream tests/data-files/inference-img.tif, 578 x 379, 8 bands, EPSG:32633.
URL: https://raw.githubusercontent.com/fieldsoftheworld/ftw-baselines/779b809813b301de7fb9f00f0c5db07086b23227/tests/data-files/inference-img.tif
SHA256 da482e7987b36898bd9f8381e7c7fe95c05dfc5ccbba19a58eb65e544d1af750
The sample is used directly in the upstream inference tests. It is downloaded, not committed or reuploaded by this example. Underlying imagery is Sentinel-2; no ground truth is bundled. Preserve source attribution. Model-weight licensing must not be assumed to resolve separately supplied imagery/annotation rights.

Citation: Muhawenayo et al., PRUE: A Practical Recipe for Field Boundary Segmentation at Scale, CVPR 2026, arXiv:2603.27101; Kerner et al., Fields of The World benchmark. Field units are image-derived agricultural divisions, not legal/cadastral parcels.


## Python input and display

All eight Float32 bands are used at their original resolution: B4_B, B3_B, B2_B, B8_B, B4_A, B3_A, B2_A, B8_A. Normalization is embedded in the ONNX package. The source remains unchanged. The display uses the first RGB triplet, 2–98% channel stretch and alpha for invalid pixels. NoData class -1 is transparent. Valid classes are background 0, field interior 1, boundary 2. Only field interiors are polygonized/exported; model outputs are not cadastral boundaries or ground truth.
