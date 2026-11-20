//! GeoKernel.AI raster inference through the SDK's JSON C ABI (`GeoKernelAI_RunRasterInference`,
//! loaded from the configured SDK's `GeoKernel.AI.dll`) and the pinned pix_grad model package.
//! The crate's `run_raster_inference` passes no progress callback, so this module binds the call
//! itself to report AI tiles. The reference is never an input to inference.
use crate::gdal::{self, Pixels, Raster, NODATA};
use crate::sample::{self, Assets, Options};
use crate::terrain;
use libloading::Library;
use serde_json::{json, Value};
use std::{
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, OnceLock},
};

type Result<T> = sample::Result<T>;
type Progress<'a> = &'a (dyn Fn(f64, &str) + Sync);
type TileCallback = unsafe extern "C" fn(i32, i32, i32, *const u16, *mut c_void);

pub const MODEL_ID: &str = "davpergan-dtm-dsm-gan-pix-grad";
pub const MODEL_SHA256: &str = "e400e27f6afc75bcf4e1a61432ee9557529683903996dd953d3e8e3fcbe90eb6";
const MODEL_HINT: &str = "Use the exact model ZIP or retry its public URL.";

struct Api {
    inspect: unsafe extern "C" fn(*const u16) -> *mut u16,
    run: unsafe extern "C" fn(*const u16, Option<TileCallback>, *mut c_void) -> *mut u16,
    last_error: unsafe extern "C" fn() -> *mut u16,
    free: unsafe extern "C" fn(*mut u16),
    _library: Library,
}

fn api() -> Result<&'static Api> {
    static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
        // Resolve dependencies beside the DLL first, as the crate does: Windows ships an unrelated
        // System32 onnxruntime.dll that the default search order would pick instead.
        #[cfg(windows)]
        let library: Library = libloading::os::windows::Library::load_with_flags(
            bin.join("GeoKernel.AI.dll"),
            libloading::os::windows::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                | libloading::os::windows::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .map_err(|e| e.to_string())?
        .into();
        #[cfg(not(windows))]
        let library = Library::new(bin.join("libGeoKernel.AI.so")).map_err(|e| e.to_string())?;
        macro_rules! f {
            ($name:literal) => {
                *library
                    .get(concat!("GeoKernelAI_", $name, "\0").as_bytes())
                    .map_err(|e| format!("GeoKernel.AI entry point {} missing: {e}", $name))?
            };
        }
        Ok(Api {
            inspect: f!("InspectModelPackageJson"),
            run: f!("RunRasterInference"),
            last_error: f!("GetLastError"),
            free: f!("FreeString"),
            _library: library,
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Read and free an SDK-owned wide string.
unsafe fn take(a: &Api, value: *mut u16) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut n = 0;
    while *value.add(n) != 0 {
        n += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(value, n));
    (a.free)(value);
    Some(text)
}

fn json_result(a: &Api, value: *mut u16) -> Result<Value> {
    let text = unsafe { take(a, value) }.ok_or_else(|| {
        unsafe { take(a, (a.last_error)()) }
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "GeoKernel AI operation failed.".into())
    })?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

// ----- acquisition --------------------------------------------------------------------------------
/// Verified pinned model package folder (archive and every package file hash-checked).
pub fn prepare_model(
    options: &Options,
    archive: Option<&Path>,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> Result<PathBuf> {
    let folder = sample::prepare(
        &Assets::model(),
        &options.data_root,
        archive.or(options.model_archive.as_deref()),
        options.model_url.as_deref(),
        cancel,
        &|text| progress(format!("Model: {text}")),
        MODEL_HINT,
    )?;
    sample::check(cancel)?;
    let a = api()?;
    let path = wide(&folder.display().to_string());
    let inspection = json_result(a, unsafe { (a.inspect)(path.as_ptr()) })?;
    if inspection["valid"] != json!(true)
        || inspection["manifest"]["id"] != json!(MODEL_ID)
        || inspection["modelSha256"] != json!(MODEL_SHA256)
    {
        let errors: Vec<&str> = inspection["validationErrors"]
            .as_array()
            .map(|e| e.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return Err(format!("Unexpected model package. {}", errors.join(" ")));
    }
    Ok(folder)
}

// ----- inference -----------------------------------------------------------------------------------
/// pix_grad output for one source: blended raw heights and the final DSM-bounded DTM.
pub struct AiHeight {
    pub raw: Vec<f32>,
    pub dtm: Vec<f32>,
    pub provenance: Value,
    pub tiles: i64,
    pub context_filled: usize,
    pub clamped: usize,
    pub padded: bool,
}

unsafe extern "C" fn tile(
    completed: i32,
    total: i32,
    percent: i32,
    message: *const u16,
    user: *mut c_void,
) {
    // Called on the SDK's inference thread while `infer` waits in the C call.
    let report = &*(user as *const &(dyn Fn(i32, i32, i32, String) + Sync));
    let mut n = 0;
    while !message.is_null() && *message.add(n) != 0 {
        n += 1;
    }
    let text = if message.is_null() {
        String::new()
    } else {
        String::from_utf16_lossy(std::slice::from_raw_parts(message, n))
    };
    report(completed, total, percent, text);
}

fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// pix_grad over the full DSM with the package tiling; the reference is not involved. Missing
/// cells are filled for input context only (four-neighbour DSM support) and inputs smaller than
/// one tile are edge-padded right/bottom, the package's padding mode, before the raster reaches
/// the SDK. The source mask is restored and the final DTM bounded above by the DSM; the blended
/// raw output is retained. The C call has no cancellation hook; cancellation is checked after it.
pub fn infer(
    dsm: &Raster,
    model: &Path,
    overlap: i32,
    threads: i32,
    folder: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<AiHeight> {
    let manifest = read_json(&model.join("geokernel-model.json"))?;
    if manifest["modelSha256"] != json!(MODEL_SHA256) {
        return Err("This example requires the pinned pix_grad adaptation.".into());
    }
    let tile_w = manifest["tiling"]["width"].as_u64().unwrap_or(256) as usize;
    let tile_h = manifest["tiling"]["height"].as_u64().unwrap_or(256) as usize;
    let (w, h) = (dsm.width, dsm.height);
    let (pw, ph) = (w.max(tile_w), h.max(tile_h));
    let context_filled = dsm.valid.iter().filter(|v| **v == 0).count();
    let padded = (pw, ph) != (w, h);
    let mut source = dsm.path.clone();
    if context_filled > 0 || padded {
        let mut z = dsm.z.clone();
        if context_filled > 0 {
            progress(38.0, "Filling missing DSM context from DSM support only...");
            let nearest = terrain::support_map(&dsm.valid, w, h, cancel)?;
            for i in 0..z.len() {
                if dsm.valid[i] == 0 {
                    z[i] = dsm.z[nearest[i]];
                }
            }
        }
        if padded {
            let mut grown = vec![0f32; pw * ph];
            for y in 0..ph {
                let row = y.min(h - 1) * w;
                for x in 0..pw {
                    grown[y * pw + x] = z[row + x.min(w - 1)];
                }
            }
            z = grown;
        }
        let context = Raster {
            path: dsm.path.clone(),
            projection: dsm.projection.clone(),
            sha256: dsm.sha256.clone(),
            width: pw,
            height: ph,
            pixel: dsm.pixel,
            transform: dsm.transform,
            z: Vec::new(),
            valid: Vec::new(),
        };
        source = gdal::write_raster(
            &context,
            &folder.join("dsm-context.tif"),
            Pixels::Heights(&z),
        )?;
    }
    sample::check(cancel)?;
    let output = folder.join("ai-inference.tif");
    let provenance = &manifest["provenance"];
    let request = json!({
        "modelPackagePath": model.display().to_string(),
        "rasterPath": source.display().to_string(),
        "outputPath": output.display().to_string(),
        "outputMode": "floatBands",
        "bands": [1],
        "provider": "cpu",
        "intraOpThreads": threads,
        "interOpThreads": 1,
        "tiling": {"overlapX": overlap, "overlapY": overlap},
        "modelRevision": provenance["revision"].as_str().filter(|r| !r.is_empty()).or(provenance["sourceUrl"].as_str()).unwrap_or_default(),
    })
    .to_string();
    let report = |completed: i32, total: i32, percent: i32, message: String| {
        progress(
            40.0 + (f64::from(percent) * 0.4).trunc(),
            &format!("AI tiles {completed} / {total} · {message}"),
        )
    };
    let report: &(dyn Fn(i32, i32, i32, String) + Sync) = &report;
    let a = api()?;
    let request = wide(&request);
    let result = json_result(a, unsafe {
        (a.run)(
            request.as_ptr(),
            Some(tile),
            &report as *const _ as *mut c_void,
        )
    })?;
    sample::check(cancel)?;
    let int = |key: &str| result[key].as_i64().unwrap_or(-1);
    if result["cancelled"] == json!(true)
        || int("width") != pw as i64
        || int("height") != ph as i64
        || int("bandCount") != 1
        || result["dataType"] != json!("Float32")
    {
        return Err("AI result dimensions or numeric type do not match the source.".into());
    }
    let transform: Vec<f64> = result["geoTransform"]
        .as_array()
        .map(|t| t.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    if transform != dsm.transform
        || !gdal::same_projection(
            &dsm.projection,
            result["projectionWkt"].as_str().unwrap_or_default(),
        )?
    {
        return Err("AI inference changed the source grid or CRS.".into());
    }
    let values = gdal::read_raster(&output, cancel)?;
    let mut raw = vec![0f32; w * h];
    for y in 0..h {
        raw[y * w..(y + 1) * w].copy_from_slice(&values.z[y * pw..y * pw + w]);
    }
    let mut dtm = raw.clone();
    let mut clamped = 0;
    for i in 0..raw.len() {
        if dsm.valid[i] == 0 {
            (raw[i], dtm[i]) = (NODATA, NODATA);
            continue;
        }
        let (x, y) = (i % w, i / w);
        if values.valid[y * pw + x] == 0 || !raw[i].is_finite() {
            return Err("AI returned a nonfinite valid-source height.".into());
        }
        if dtm[i] > dsm.z[i] {
            dtm[i] = dsm.z[i];
            clamped += 1;
        }
    }
    Ok(AiHeight {
        raw,
        dtm,
        provenance: result["provenance"].clone(),
        tiles: int("processedTiles"),
        context_filled,
        clamped,
        padded,
    })
}
