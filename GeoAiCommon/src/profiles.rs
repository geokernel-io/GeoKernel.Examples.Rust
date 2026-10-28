use crate::assets::{Asset, DATA, MODEL};

#[derive(Clone, Copy)]
pub struct Profile {
    pub task: &'static str,
    pub title: &'static str,
    pub data: Asset,
    pub model: Asset,
    pub display_name: &'static str,
    pub source_name: &'static str,
    pub run_label: &'static str,
    pub export_name: &'static str,
    pub description: &'static str,
    pub legend: &'static str,
    pub buildings: bool,
    pub threshold_label: &'static str,
}

pub const DYNAMIC_WORLD: Profile = Profile {
    task: "dynamic-world", title: "Google Dynamic World — land cover", data: DATA, model: MODEL,
    display_name: "dynamicworld_spain_rgb.tif", source_name: "Spain Sentinel-2 RGB source",
    run_label: "Run land-cover classification", export_name: "dynamic-world.shp",
    description: "Spain / Alicante · 512 × 512 · nine Sentinel-2 L1C bands.",
    legend: "Water · Trees · Grass · Flooded vegetation · Crops · Shrub and scrub · Built · Bare · Snow and ice\n\nContains modified Copernicus Sentinel data 2019.\nModel: Google / National Geographic / WRI; Apache-2.0. Retain downloaded notices.",
    buildings: false,
    threshold_label: "",
};

pub const HOTOSM: Profile = Profile {
    task: "hotosm-buildings", title: "HOTOSM DINOv3-S — building footprints",
    data: Asset {
        url: "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/buildings.zip",
        sha: "f7cb0554b54c8e8bf31d576c38f988a8902382ad2ec3db3de6de05f5dbfb6f2b", required: "buildings.tif",
    },
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/hotosm-dinov3s-buildings-geokernel-onnx-v1.zip",
        sha: "3a6cd11e013f81b46b3db700feddef2d98319780285b8320e1200573684b24bb", required: "geokernel-model.json",
    },
    display_name: "buildings.tif", source_name: "Buildings RGB source",
    run_label: "Run building segmentation inference", export_name: "building-footprints.shp",
    description: "Buildings RGB sample · 2503 × 1126 · original resolution.",
    legend: "Building footprints are predictions, not ground truth or cadastral boundaries.\n\nModel: HOTOSM / Meta DINOv3. CC-BY-4.0 plus separate DINOv3 License; retain package notices.\nImagery source/license not verified.",
    buildings: true,
    threshold_label: "Building threshold",
};

pub const GEOAI_USA: Profile = Profile {
    task: "geoai-usa-buildings",
    title: "GeoAI USA — Mask R-CNN building footprints",
    threshold_label: "Confidence threshold",
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/giswqs-geoai-usa-maskrcnn-resnet50-fpn-geokernel-onnx-v1.zip",
        sha: "055097781e125083201421c37c70d36336f2f968b69b1aa09350578d2c87ca32", required: "geokernel-model.json",
    },
    description: "Buildings RGB sample · USA-trained Mask R-CNN · original resolution.",
    legend: "Building footprints are predictions, not ground truth or cadastral boundaries.\n\nModel: giswqs/geoai — Mask R-CNN ResNet50 FPN. MIT model declaration; retain package notices.\nImagery source/license not verified.",
    ..HOTOSM
};

pub const PRITHVI: Profile = Profile {
    task: "prithvi-flood", title: "IBM/NASA Prithvi EO 2.0 — flood analysis",
    data: Asset {
        url: "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/flood_analysis.zip",
        sha: "059e9a84bed08ea5da76941432e6e8409bd1352661ff72f750e47df5f740c38f", required: "India_900498_S2Hand.tif",
    },
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/ibm-nasa-prithvi-eo2-300m-tl-sen1floods11-geokernel-onnx-v1.zip",
        sha: "34711ae55d48fe265b19c18551aef4b4aa84045a07bffb407161570b19429c13", required: "geokernel-model.json",
    },
    display_name: "prithvi-source-rgb-rust-v1.tif", source_name: "Sentinel-2 B4/B3/B2 RGB source",
    run_label: "Run flood analysis", export_name: "flood-extent.shp",
    description: "India · Sentinel-2 L1C · 512 × 512 · original spectral source.",
    legend: "Non-water (0) · Water / flood (1). Shapefile saves only class 1. A single image cannot distinguish permanent water from new flooding.\n\nModel: IBM/NASA Prithvi EO 2.0; Apache-2.0. Imagery: Copernicus / Sen1Floods11. Retain source/package notices.",
    buildings: false, threshold_label: "",
};
pub const PRUE: Profile = Profile {
    task: "prue-fields", title: "Fields of The World / PRUE — field boundaries",
    data: Asset {
        url: "https://raw.githubusercontent.com/fieldsoftheworld/ftw-baselines/779b809813b301de7fb9f00f0c5db07086b23227/tests/data-files/inference-img.tif",
        sha: "da482e7987b36898bd9f8381e7c7fe95c05dfc5ccbba19a58eb65e544d1af750", required: "inference-img.tif",
    },
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/fieldsoftheworld-prue-efficientnet-b5-ccby-geokernel-onnx-v2.zip",
        sha: "0035f723ae5753eb861e056bbb38b60605617485c9700cc16d3317856374da44", required: "geokernel-model.json",
    },
    display_name: "prue-source-rgb-rust-v1.tif", source_name: "Sentinel-2 first-period RGB source",
    run_label: "Run field boundary segmentation", export_name: "field-interiors.shp",
    description: "FTW Sentinel-2 sample · 578 × 379 · two periods, eight bands.",
    legend: "Background (0) · Field interior (1) · Boundary (2). Shapefile saves only class-1 interiors. Image-derived divisions are not cadastral parcels.\n\nModel: FTW / PRUE EfficientNet-B5 CC-BY. Weights: CC-BY-4.0; code: MIT. Imagery: Sentinel-2 / FTW test sample. Retain notices.",
    buildings: false, threshold_label: "",
};
pub const SEN2SR: Profile = Profile {
    task: "sen2sr-super-resolution", title: "ESAOpenSR SEN2SRLite — super resolution",
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/esaopensr-sen2srlite-rgbn-x4-geokernel-onnx-v1.zip",
        sha: "bcad3673c73429fddc6f6277fff823b01bdc77a268aee8b09540566d510d1017", required: "geokernel-model.json",
    },
    display_name: "sen2sr-source-rgb-rust-v1.tif", run_label: "Run 4× super resolution", export_name: "sen2sr-reflectance.tif",
    description: "Sentinel-2 RGB + NIR · 578 × 379 · 4× enhanced output.",
    legend: "Estimated detail on a finer grid, not new sensor measurements.\nModel: ESAOpenSR / SEN2SRLite; CC0-1.0 and inherited Apache-2.0 notices. Imagery: Sentinel-2 / FTW sample. Retain notices.",
    ..PRUE
};
pub const SAM_TEXT: Profile = Profile {
    task: "sam3-text", title: "Meta SAM3 — text / point / box segmentation",
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/meta-sam3-text-fp32-geokernel-onnx-v1.zip",
        sha: "95d9bc3ebdec6a6147a9bfa97325712bfbb77fccbd6bb3f20be453c381820931", required: "sam3-package.json",
    },
    run_label: "Generate draft", export_name: "sam3-instances.shp", buildings: false,
    description: "Text scans the image. Point / box selects one object, not an area scan.",
    legend: "Review a draft, then Accept or Discard. Save explicitly to retain it.\nModel: Meta SAM3; SAM License and package notices apply. Imagery source/license not verified.",
    ..HOTOSM
};
pub const SAM_TRACKER: Profile = Profile {
    task: "sam3-tracker",
    model: Asset {
        url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/meta-sam3-tracker-fp32-geokernel-onnx-v1.zip",
        sha: "c4309ed67b2292f64641d740f2558c2d4253476ca1ec40c17d49c4800c050896", required: "sam3-package.json",
    },
    ..SAM_TEXT
};
