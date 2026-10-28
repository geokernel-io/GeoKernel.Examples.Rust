//! Worker-only RGB previews and prompt reprojection using the selected SDK's GDAL.
use crate::{assets, profiles::Profile};
use libloading::Library;
use serde_json::{json, Value};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc::Sender},
};
type Handle = *mut c_void;
type R<T> = assets::WorkerResult<T>;

macro_rules! api {
    ($lib:expr, $name:ident, $ty:ty) => {
        #[allow(non_snake_case)] // Keep the exact GDAL C ABI symbol spelling.
        let $name: $ty = *$lib
            .get::<$ty>(concat!(stringify!($name), "\0").as_bytes())
            .map_err(|e| e.to_string())?;
    };
}
fn string(path: &Path) -> R<CString> {
    let value = path.to_str().ok_or("Non-Unicode path")?;
    let value = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    };
    CString::new(value).map_err(|e| e.to_string())
}
fn checked(status: i32) -> R<()> {
    if status == 0 {
        Ok(())
    } else {
        Err("GDAL operation failed".into())
    }
}
unsafe fn library() -> R<Library> {
    let bin = std::env::var_os("GEOKERNEL_BIN").ok_or("SDK directory not configured")?;
    Library::new(PathBuf::from(bin).join("gdal.dll")).map_err(|e| e.to_string())
}

/// Prepare display coordinates without modifying the projected result used for export.
pub fn prepare_vector_preview(source: &Path, cancel: &AtomicBool) -> R<PathBuf> {
    let output_path = source.with_file_name("viewer-preview.geojson");
    let result = unsafe {
        let lib = library()?;
        api!(lib, GDALAllRegister, unsafe extern "C" fn());
        api!(
            lib,
            GDALOpenEx,
            unsafe extern "C" fn(
                *const c_char,
                u32,
                *const *const c_char,
                *const *const c_char,
                *const *const c_char,
            ) -> Handle
        );
        api!(lib, GDALClose, unsafe extern "C" fn(Handle));
        api!(
            lib,
            GDALVectorTranslateOptionsNew,
            unsafe extern "C" fn(*const *const c_char, Handle) -> Handle
        );
        api!(
            lib,
            GDALVectorTranslateOptionsFree,
            unsafe extern "C" fn(Handle)
        );
        api!(
            lib,
            GDALVectorTranslate,
            unsafe extern "C" fn(
                *const c_char,
                Handle,
                i32,
                *const Handle,
                Handle,
                *mut i32,
            ) -> Handle
        );
        assets::check(cancel)?;
        GDALAllRegister();
        let source_name = string(source)?;
        let output_name = string(&output_path)?;
        let args = ["-f", "GeoJSON", "-t_srs", "EPSG:4326", "-dim", "XY"]
            .map(|value| CString::new(value).unwrap());
        let mut argv: Vec<_> = args.iter().map(|value| value.as_ptr()).collect();
        argv.push(std::ptr::null());
        let dataset = GDALOpenEx(
            source_name.as_ptr(),
            4,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );
        if dataset.is_null() {
            return Err("Cannot open building polygons for display".into());
        }
        let options = GDALVectorTranslateOptionsNew(argv.as_ptr(), std::ptr::null_mut());
        if options.is_null() {
            GDALClose(dataset);
            return Err("Cannot configure building polygon display".into());
        }
        let mut usage_error = 0;
        let translated = GDALVectorTranslate(
            output_name.as_ptr(),
            std::ptr::null_mut(),
            1,
            &dataset,
            options,
            &mut usage_error,
        );
        let success = !translated.is_null() && usage_error == 0;
        if !translated.is_null() {
            GDALClose(translated);
        }
        GDALVectorTranslateOptionsFree(options);
        GDALClose(dataset);
        if success {
            assets::check(cancel)
        } else {
            Err("Cannot transform building polygons for display".into())
        }
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&output_path);
    }
    result?;
    Ok(output_path)
}

pub fn prepare(
    profile: Profile,
    raster: &Path,
    cancel: &AtomicBool,
    progress: &Sender<(i32, String)>,
) -> R<PathBuf> {
    if !matches!(
        profile.task,
        "prithvi-flood" | "prue-fields" | "sen2sr-super-resolution"
    ) {
        return Ok(raster.with_file_name(profile.display_name));
    }
    let output = raster.with_file_name(profile.display_name);
    if output.is_file() {
        return Ok(output);
    }
    let temporary = output.with_extension(format!(
        "{}-{}.tif",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let _ = progress.send((0, "Preparing source RGB display…".into()));
    let result = unsafe { create_rgb(profile.task, raster, &temporary, cancel) };
    let result = result.and_then(|()| {
        assets::check(cancel)?;
        std::fs::rename(&temporary, &output).map_err(|e| e.to_string())
    });
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    Ok(output)
}

unsafe fn create_rgb(task: &str, source: &Path, output: &Path, cancel: &AtomicBool) -> R<()> {
    let lib = library()?;
    api!(lib, GDALAllRegister, unsafe extern "C" fn());
    api!(
        lib,
        GDALOpen,
        unsafe extern "C" fn(*const c_char, c_int) -> Handle
    );
    api!(lib, GDALClose, unsafe extern "C" fn(Handle));
    api!(lib, GDALGetRasterXSize, unsafe extern "C" fn(Handle) -> i32);
    api!(lib, GDALGetRasterYSize, unsafe extern "C" fn(Handle) -> i32);
    api!(lib, GDALGetRasterCount, unsafe extern "C" fn(Handle) -> i32);
    api!(
        lib,
        GDALGetRasterBand,
        unsafe extern "C" fn(Handle, i32) -> Handle
    );
    api!(
        lib,
        GDALGetRasterNoDataValue,
        unsafe extern "C" fn(Handle, *mut i32) -> f64
    );
    api!(
        lib,
        GDALRasterIO,
        unsafe extern "C" fn(
            Handle,
            i32,
            i32,
            i32,
            i32,
            i32,
            Handle,
            i32,
            i32,
            i32,
            i32,
            i32,
        ) -> i32
    );
    api!(
        lib,
        GDALGetGeoTransform,
        unsafe extern "C" fn(Handle, *mut f64) -> i32
    );
    api!(
        lib,
        GDALSetGeoTransform,
        unsafe extern "C" fn(Handle, *mut f64) -> i32
    );
    api!(
        lib,
        GDALGetProjectionRef,
        unsafe extern "C" fn(Handle) -> *const c_char
    );
    api!(
        lib,
        GDALSetProjection,
        unsafe extern "C" fn(Handle, *const c_char) -> i32
    );
    api!(
        lib,
        GDALGetDriverByName,
        unsafe extern "C" fn(*const c_char) -> Handle
    );
    api!(
        lib,
        GDALCreate,
        unsafe extern "C" fn(
            Handle,
            *const c_char,
            i32,
            i32,
            i32,
            i32,
            *const *const c_char,
        ) -> Handle
    );
    api!(
        lib,
        GDALSetRasterColorInterpretation,
        unsafe extern "C" fn(Handle, i32) -> i32
    );
    GDALAllRegister();
    let input = GDALOpen(string(source)?.as_ptr(), 0);
    if input.is_null() {
        return Err("Cannot open spectral source".into());
    }
    let mut destination = std::ptr::null_mut();
    let result = (|| -> R<()> {
        let (width, height, count) = (
            GDALGetRasterXSize(input),
            GDALGetRasterYSize(input),
            GDALGetRasterCount(input),
        );
        if width < 1 || height < 1 || i64::from(width) * i64::from(height) > 16_000_000 {
            return Err("Invalid source dimensions".into());
        }
        let pixels = width as usize * height as usize;
        let bands: Vec<i32> = match task {
            "prithvi-flood" if count == 13 => vec![2, 3, 4, 9, 12, 13],
            "prithvi-flood" if count == 6 => (1..=6).collect(),
            "prue-fields" if count == 8 => (1..=8).collect(),
            "sen2sr-super-resolution" if count == 4 || count == 8 => (1..=4).collect(),
            _ => return Err("Unexpected spectral band count".into()),
        };
        let mut valid = vec![true; pixels];
        let mut channels: Vec<Vec<f32>> = vec![vec![]; 3];
        for (index, number) in bands.into_iter().enumerate() {
            assets::check(cancel)?;
            let band = GDALGetRasterBand(input, number);
            let mut values = vec![0f32; pixels];
            checked(GDALRasterIO(
                band,
                0,
                0,
                0,
                width,
                height,
                values.as_mut_ptr().cast(),
                width,
                height,
                6,
                0,
                0,
            ))?;
            let mut declared = 0;
            let nodata = GDALGetRasterNoDataValue(band, &mut declared);
            for (i, &v) in values.iter().enumerate() {
                valid[i] &= v.is_finite() && v >= 0.0 && (declared == 0 || f64::from(v) != nodata);
            }
            if index < 3 {
                channels[if task == "prithvi-flood" {
                    2 - index
                } else {
                    index
                }] = values;
            }
        }
        let mut transform = [0.0; 6];
        checked(GDALGetGeoTransform(input, transform.as_mut_ptr()))?;
        destination = GDALCreate(
            GDALGetDriverByName(c"GTiff".as_ptr()),
            string(output)?.as_ptr(),
            width,
            height,
            4,
            1,
            std::ptr::null(),
        );
        if destination.is_null() {
            return Err("Cannot create source RGB preview".into());
        }
        checked(GDALSetGeoTransform(destination, transform.as_mut_ptr()))?;
        checked(GDALSetProjection(destination, GDALGetProjectionRef(input)))?;
        for index in 0..4 {
            assets::check(cancel)?;
            let mut bytes = vec![0u8; pixels];
            if index == 3 {
                for i in 0..pixels {
                    bytes[i] = if valid[i] { 255 } else { 0 };
                }
            } else {
                let values = &channels[index];
                let mut ordered: Vec<f32> = values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &v)| valid[i].then_some(v))
                    .collect();
                if ordered.is_empty() {
                    return Err("No valid source pixels".into());
                }
                ordered.sort_unstable_by(f32::total_cmp);
                let low = ordered[((ordered.len() - 1) as f64 * 0.02).floor() as usize];
                let high = ordered[((ordered.len() - 1) as f64 * 0.98).floor() as usize];
                let scale = 255.0f64 / f64::from((high - low).max(1.0));
                for i in 0..pixels {
                    if valid[i] {
                        bytes[i] = (f64::from(values[i] - low) * scale).clamp(0.0, 255.0) as u8;
                    }
                }
            }
            let band = GDALGetRasterBand(destination, index as i32 + 1);
            checked(GDALRasterIO(
                band,
                1,
                0,
                0,
                width,
                height,
                bytes.as_mut_ptr().cast(),
                width,
                height,
                1,
                0,
                0,
            ))?;
            checked(GDALSetRasterColorInterpretation(band, index as i32 + 3))?;
        }
        Ok(())
    })();
    if !destination.is_null() {
        GDALClose(destination);
    }
    GDALClose(input);
    result
}

/// WGS84 UI prompts to source CRS; transform all four box corners with traditional GIS axis order.
pub fn prompts(source: &Path, points: &[[f64; 3]], bounds: Option<[f64; 4]>) -> R<Value> {
    unsafe {
        let lib = library()?;
        api!(lib, GDALAllRegister, unsafe extern "C" fn());
        api!(
            lib,
            GDALOpen,
            unsafe extern "C" fn(*const c_char, i32) -> Handle
        );
        api!(lib, GDALClose, unsafe extern "C" fn(Handle));
        api!(
            lib,
            GDALGetProjectionRef,
            unsafe extern "C" fn(Handle) -> *const c_char
        );
        api!(
            lib,
            OSRNewSpatialReference,
            unsafe extern "C" fn(*const c_char) -> Handle
        );
        api!(
            lib,
            OSRImportFromEPSG,
            unsafe extern "C" fn(Handle, i32) -> i32
        );
        api!(
            lib,
            OSRSetAxisMappingStrategy,
            unsafe extern "C" fn(Handle, i32)
        );
        api!(
            lib,
            OSRDestroySpatialReference,
            unsafe extern "C" fn(Handle)
        );
        api!(
            lib,
            OCTNewCoordinateTransformation,
            unsafe extern "C" fn(Handle, Handle) -> Handle
        );
        api!(
            lib,
            OCTDestroyCoordinateTransformation,
            unsafe extern "C" fn(Handle)
        );
        api!(
            lib,
            OCTTransform,
            unsafe extern "C" fn(Handle, i32, *mut f64, *mut f64, *mut f64) -> i32
        );
        GDALAllRegister();
        let dataset = GDALOpen(string(source)?.as_ptr(), 0);
        if dataset.is_null() {
            return Err("Cannot read source CRS".into());
        }
        let wkt = CStr::from_ptr(GDALGetProjectionRef(dataset)).to_owned();
        GDALClose(dataset);
        if wkt.as_bytes().is_empty() {
            return Err("Source has no CRS".into());
        }
        let src = OSRNewSpatialReference(std::ptr::null());
        let dst = OSRNewSpatialReference(wkt.as_ptr());
        let mut transform = std::ptr::null_mut();
        let result = (|| -> R<Value> {
            if src.is_null() || dst.is_null() {
                return Err("Cannot initialize prompt CRS".into());
            }
            checked(OSRImportFromEPSG(src, 4326))?;
            OSRSetAxisMappingStrategy(src, 0);
            OSRSetAxisMappingStrategy(dst, 0);
            transform = OCTNewCoordinateTransformation(src, dst);
            if transform.is_null() {
                return Err("Cannot transform prompt CRS".into());
            }
            let convert = |mut x: f64, mut y: f64| -> R<[f64; 2]> {
                let mut z = 0.0;
                if OCTTransform(transform, 1, &mut x, &mut y, &mut z) == 0
                    || !x.is_finite()
                    || !y.is_finite()
                {
                    return Err("Invalid prompt coordinate".into());
                }
                Ok([x, y])
            };
            let mut converted = Vec::new();
            for p in points {
                let [x, y] = convert(p[0], p[1])?;
                converted.push(json!({"x":x,"y":y,"positive":p[2]>0.0}));
            }
            let mut result = json!({"coordinateSpace":"source-crs","points":converted});
            if let Some([a, b, c, d]) = bounds {
                let corners = [
                    convert(a, b)?,
                    convert(a, d)?,
                    convert(c, b)?,
                    convert(c, d)?,
                ];
                result["box"] = json!([
                    corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                    corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
                    corners
                        .iter()
                        .map(|p| p[0])
                        .fold(f64::NEG_INFINITY, f64::max),
                    corners
                        .iter()
                        .map(|p| p[1])
                        .fold(f64::NEG_INFINITY, f64::max)
                ]);
            }
            Ok(result)
        })();
        if !transform.is_null() {
            OCTDestroyCoordinateTransformation(transform);
        }
        if !src.is_null() {
            OSRDestroySpatialReference(src);
        }
        if !dst.is_null() {
            OSRDestroySpatialReference(dst);
        }
        result
    }
}
