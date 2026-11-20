//! Validated height reading, numeric/RGB GeoTIFF writing, CRS checks and GDAL's linear TIN
//! gridding, through the GDAL runtime shipped in the published SDK (no system GDAL).
use crate::sample::{check, hash_file, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_double, c_int, c_uint, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, OnceLock},
};

type P = *mut c_void;
type GridProgress = unsafe extern "C" fn(c_double, *const c_char, P) -> c_int;
type RasterIo = unsafe extern "C" fn(
    P,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    P,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
) -> c_int;

pub const NODATA: f32 = -9999.0;
pub const METHOD: &str = "native-multiscale-morphological-tin-v1";

struct Api {
    _library: Library,
    open: unsafe extern "C" fn(*const c_char, c_int) -> P,
    close: unsafe extern "C" fn(P),
    count: unsafe extern "C" fn(P) -> c_int,
    width: unsafe extern "C" fn(P) -> c_int,
    height: unsafe extern "C" fn(P) -> c_int,
    band: unsafe extern "C" fn(P, c_int) -> P,
    mask: unsafe extern "C" fn(P) -> P,
    data_type: unsafe extern "C" fn(P) -> c_int,
    color: unsafe extern "C" fn(P) -> c_int,
    scale: unsafe extern "C" fn(P, *mut c_int) -> c_double,
    offset: unsafe extern "C" fn(P, *mut c_int) -> c_double,
    units: unsafe extern "C" fn(P) -> *const c_char,
    set_units: unsafe extern "C" fn(P, *const c_char) -> c_int,
    nodata: unsafe extern "C" fn(P, *mut c_int) -> c_double,
    set_nodata: unsafe extern "C" fn(P, c_double) -> c_int,
    set_metadata: unsafe extern "C" fn(P, *const c_char, *const c_char, *const c_char) -> c_int,
    io: RasterIo,
    get_transform: unsafe extern "C" fn(P, *mut c_double) -> c_int,
    set_transform: unsafe extern "C" fn(P, *const c_double) -> c_int,
    get_projection: unsafe extern "C" fn(P) -> *const c_char,
    set_projection: unsafe extern "C" fn(P, *const c_char) -> c_int,
    driver: unsafe extern "C" fn(*const c_char) -> P,
    create: unsafe extern "C" fn(
        P,
        *const c_char,
        c_int,
        c_int,
        c_int,
        c_int,
        *const *const c_char,
    ) -> P,
    last_error: unsafe extern "C" fn() -> *const c_char,
    new_srs: unsafe extern "C" fn(*const c_char) -> P,
    destroy_srs: unsafe extern "C" fn(P),
    from_user: unsafe extern "C" fn(P, *const c_char) -> c_int,
    same: unsafe extern "C" fn(P, P) -> c_int,
    is_projected: unsafe extern "C" fn(P) -> c_int,
    linear_units: unsafe extern "C" fn(P, *mut *const c_char) -> c_double,
    proj_paths: unsafe extern "C" fn(*const *const c_char),
    grid_create: unsafe extern "C" fn(
        c_int,
        *const c_void,
        c_uint,
        *const f64,
        *const f64,
        *const f64,
        c_int,
    ) -> P,
    grid_process: unsafe extern "C" fn(
        P,
        f64,
        f64,
        f64,
        f64,
        c_uint,
        c_uint,
        c_int,
        P,
        Option<GridProgress>,
        P,
    ) -> c_int,
    grid_free: unsafe extern "C" fn(P),
}
// The function pointers are plain C entry points of a library that stays loaded.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api> {
    API.get_or_init(|| unsafe {
        let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
        let library = Library::new(bin.join("gdal.dll")).map_err(|e| e.to_string())?;
        macro_rules! f {
            ($name:literal) => {
                *library
                    .get(concat!($name, "\0").as_bytes())
                    .map_err(|e| e.to_string())?
            };
        }
        let register: unsafe extern "C" fn() = f!("GDALAllRegister");
        let version: unsafe extern "C" fn(*const c_char) -> *const c_char = f!("GDALVersionInfo");
        let api = Api {
            open: f!("GDALOpen"),
            close: f!("GDALClose"),
            count: f!("GDALGetRasterCount"),
            width: f!("GDALGetRasterXSize"),
            height: f!("GDALGetRasterYSize"),
            band: f!("GDALGetRasterBand"),
            mask: f!("GDALGetMaskBand"),
            data_type: f!("GDALGetRasterDataType"),
            color: f!("GDALGetRasterColorInterpretation"),
            scale: f!("GDALGetRasterScale"),
            offset: f!("GDALGetRasterOffset"),
            units: f!("GDALGetRasterUnitType"),
            set_units: f!("GDALSetRasterUnitType"),
            nodata: f!("GDALGetRasterNoDataValue"),
            set_nodata: f!("GDALSetRasterNoDataValue"),
            set_metadata: f!("GDALSetMetadataItem"),
            io: f!("GDALRasterIO"),
            get_transform: f!("GDALGetGeoTransform"),
            set_transform: f!("GDALSetGeoTransform"),
            get_projection: f!("GDALGetProjectionRef"),
            set_projection: f!("GDALSetProjection"),
            driver: f!("GDALGetDriverByName"),
            create: f!("GDALCreate"),
            last_error: f!("CPLGetLastErrorMsg"),
            new_srs: f!("OSRNewSpatialReference"),
            destroy_srs: f!("OSRDestroySpatialReference"),
            from_user: f!("OSRSetFromUserInput"),
            same: f!("OSRIsSame"),
            is_projected: f!("OSRIsProjected"),
            linear_units: f!("OSRGetLinearUnits"),
            proj_paths: f!("OSRSetPROJSearchPaths"),
            grid_create: f!("GDALGridContextCreate"),
            grid_process: f!("GDALGridContextProcess"),
            grid_free: f!("GDALGridContextFree"),
            _library: library,
        };
        register();
        let number = CStr::from_ptr(version(c"VERSION_NUM".as_ptr()))
            .to_string_lossy()
            .parse::<i64>()
            .unwrap_or(0);
        if number < 3_060_000 {
            return Err("The gridding bridge requires GDAL 3.6 or newer.".to_owned());
        }
        Ok(api)
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn error(message: &str) -> String {
    let detail = api()
        .ok()
        .map(|a| unsafe { (a.last_error)() })
        .filter(|p| !p.is_null())
        .map(|p| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{message} {detail}").trim().to_owned()
}

fn c(text: &str) -> Result<CString> {
    CString::new(text).map_err(|e| e.to_string())
}
fn c_path(path: &Path) -> Result<CString> {
    c(path.to_str().ok_or("Path is not UTF-8")?)
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

/// Prefer packaged proj-data, then the verified sample PROJ database. Process-local only.
pub fn configure_runtime(
    sample: &Path,
    set: &dyn Fn(&str, &std::ffi::OsStr) -> Result<()>,
) -> Result<()> {
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let proj = if bin.join("proj-data/proj.db").is_file() {
        bin.join("proj-data")
    } else {
        sample.join("proj-data")
    };
    if !proj.join("proj.db").is_file() {
        return Err("Sample projection database is missing.".into());
    }
    set("PROJ_DATA", proj.as_os_str())?;
    set("PROJ_LIB", proj.as_os_str())?;
    set("PROJ_NETWORK", std::ffi::OsStr::new("OFF"))?;
    let path = c_path(&proj)?;
    let list = [path.as_ptr(), std::ptr::null()];
    unsafe { (api()?.proj_paths)(list.as_ptr()) };
    Ok(())
}

/// A single-band north-up metre height raster with an explicit validity mask.
pub struct Raster {
    pub path: PathBuf,
    pub projection: String,
    pub sha256: String,
    pub width: usize,
    pub height: usize,
    pub pixel: f64,
    pub transform: [f64; 6],
    pub z: Vec<f32>,
    pub valid: Vec<u8>,
}

impl Raster {
    pub fn extent(&self) -> [f64; 4] {
        let t = &self.transform;
        [
            t[0],
            t[3] - self.height as f64 * self.pixel,
            t[0] + self.width as f64 * self.pixel,
            t[3],
        ]
    }
}

fn metre_projected(a: &Api, projection: &str) -> Result<bool> {
    let text = c(projection)?;
    unsafe {
        let srs = (a.new_srs)(std::ptr::null());
        let mut name = std::ptr::null();
        let ok = (a.from_user)(srs, text.as_ptr()) == 0
            && (a.is_projected)(srs) != 0
            && ((a.linear_units)(srs, &mut name) - 1.0).abs() < 1e-9;
        (a.destroy_srs)(srs);
        Ok(ok)
    }
}

/// Single-band north-up metre heights in a projected metre CRS, with an explicit validity mask.
pub fn read_raster(path: &Path, cancel: &AtomicBool) -> Result<Raster> {
    check(cancel)?;
    let a = api()?;
    let name = c_path(path)?;
    let ds = unsafe { (a.open)(name.as_ptr(), 0) };
    if ds.is_null() {
        return Err(error("Cannot open height raster."));
    }
    let read = (|| unsafe {
        require((a.count)(ds) == 1, "Use a single-band height raster.")?;
        let band = (a.band)(ds, 1);
        let mut present = 0;
        let scale = (a.scale)(band, &mut present);
        require(
            present == 0 || (scale - 1.0).abs() < 1e-12,
            "Apply raster scale to metre heights before opening.",
        )?;
        let offset = (a.offset)(band, &mut present);
        require(
            present == 0 || offset.abs() < 1e-12,
            "Apply raster offset before opening.",
        )?;
        let units = (a.units)(band);
        let units = if units.is_null() {
            String::new()
        } else {
            CStr::from_ptr(units).to_string_lossy().to_lowercase()
        };
        require(
            ["", "m", "metre", "meter", "metres", "meters"].contains(&units.as_str()),
            "Height units must be metres. Unspecified units are assumed to be metres.",
        )?;
        let (w, h) = ((a.width)(ds), (a.height)(ds));
        let mut t = [0.0; 6];
        let has_transform = (a.get_transform)(ds, t.as_mut_ptr()) == 0;
        let projection = CStr::from_ptr((a.get_projection)(ds))
            .to_string_lossy()
            .into_owned();
        require(
            metre_projected(a, &projection)?,
            "Use a projected CRS with metre XY units.",
        )?;
        require(
            has_transform && t[2] == 0.0 && t[4] == 0.0 && t[1] > 0.0 && (t[1] + t[5]).abs() < 1e-9,
            "Use a north-up raster with square pixels.",
        )?;
        require(
            w >= 8 && h >= 8 && i64::from(w) * i64::from(h) <= 4_000_000,
            "This interactive example supports 8 x 8 to 4 million source cells.",
        )?;
        let n = (w * h) as usize;
        let mut z = vec![0f32; n];
        if (a.io)(band, 0, 0, 0, w, h, z.as_mut_ptr().cast(), w, h, 6, 0, 0) != 0 {
            return Err(error("Cannot read complete heights."));
        }
        let mut has = 0;
        let nodata = (a.nodata)(band, &mut has);
        let mut mask = vec![0u8; n];
        if (a.io)(
            (a.mask)(band),
            0,
            0,
            0,
            w,
            h,
            mask.as_mut_ptr().cast(),
            w,
            h,
            1,
            0,
            0,
        ) != 0
        {
            return Err(error("Cannot read source validity pixels."));
        }
        Ok((
            w as usize,
            h as usize,
            t,
            projection,
            z,
            mask,
            (has != 0).then_some(nodata),
        ))
    })();
    unsafe { (a.close)(ds) };
    let (width, height, t, projection, z, mask, nodata) = read?;
    let valid: Vec<u8> = z
        .iter()
        .zip(&mask)
        .map(|(v, m)| {
            u8::from(*m != 0 && v.is_finite() && nodata.is_none_or(|n| f64::from(*v) != n))
        })
        .collect();
    let count = valid.iter().filter(|v| **v != 0).count();
    require(
        count >= 4 && count as f64 >= z.len() as f64 * 0.05,
        "Insufficient valid source coverage.",
    )?;
    check(cancel)?;
    Ok(Raster {
        path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
        projection,
        sha256: hash_file(path),
        width,
        height,
        pixel: t[1],
        transform: [t[0], t[1], 0.0, t[3], 0.0, t[5]],
        z,
        valid,
    })
}

/// True when two CRS definitions are equivalent (GDAL `OSRIsSame`).
pub fn same_projection(first: &str, second: &str) -> Result<bool> {
    let a = api()?;
    let (left, right) = (c(first)?, c(second)?);
    Ok(unsafe {
        let (x, y) = ((a.new_srs)(std::ptr::null()), (a.new_srs)(std::ptr::null()));
        let same = (a.from_user)(x, left.as_ptr()) == 0
            && (a.from_user)(y, right.as_ptr()) == 0
            && (a.same)(x, y) != 0;
        (a.destroy_srs)(x);
        (a.destroy_srs)(y);
        same
    })
}

pub fn validate_reference(dsm: &Raster, reference: &Raster) -> Result<()> {
    let same = same_projection(&dsm.projection, &reference.projection)?;
    require(
        same && dsm.width == reference.width && dsm.height == reference.height && dsm.transform == reference.transform,
        "Reference must share the DSM's exact dimensions, grid and equivalent CRS; no implicit resampling.",
    )
}

/// Numeric pixels for [`write_raster`]: Float32 heights or Byte QA bits.
pub enum Pixels<'a> {
    Heights(&'a [f32]),
    Quality(&'a [u8]),
}

/// Float32 heights (NoData -9999, metre units) or Byte QA (NoData 1) on the source grid.
pub fn write_raster(raster: &Raster, path: &Path, pixels: Pixels) -> Result<PathBuf> {
    let a = api()?;
    let (name, gtiff, wkt) = (c_path(path)?, c("GTiff")?, c(&raster.projection)?);
    let (compress, tiled) = (c("COMPRESS=DEFLATE")?, c("TILED=YES")?);
    let options = [compress.as_ptr(), tiled.as_ptr(), std::ptr::null()];
    let (w, h) = (raster.width as c_int, raster.height as c_int);
    let kind = if matches!(pixels, Pixels::Heights(_)) {
        6
    } else {
        1
    };
    let ds = unsafe {
        (a.create)(
            (a.driver)(gtiff.as_ptr()),
            name.as_ptr(),
            w,
            h,
            1,
            kind,
            options.as_ptr(),
        )
    };
    if ds.is_null() {
        return Err(error(&format!("Cannot create GeoTIFF: {}", path.display())));
    }
    let result = unsafe {
        if (a.set_transform)(ds, raster.transform.as_ptr()) != 0
            || (a.set_projection)(ds, wkt.as_ptr()) != 0
        {
            Err(error("Cannot write raster coordinates."))
        } else {
            let method = c(METHOD)?;
            (a.set_metadata)(
                ds,
                c"GEOKERNEL_TERRAIN_METHOD".as_ptr(),
                method.as_ptr(),
                std::ptr::null(),
            );
            let band = (a.band)(ds, 1);
            (a.set_nodata)(band, if kind == 1 { 1.0 } else { f64::from(NODATA) });
            let data: P = match pixels {
                Pixels::Heights(v) => {
                    (a.set_units)(band, c"metre".as_ptr());
                    v.as_ptr().cast_mut().cast()
                }
                Pixels::Quality(v) => v.as_ptr().cast_mut().cast(),
            };
            if (a.io)(band, 1, 0, 0, w, h, data, w, h, kind, 0, 0) != 0 {
                Err(error("Cannot write raster pixels."))
            } else {
                Ok(())
            }
        }
    };
    unsafe { (a.close)(ds) };
    result.map(|()| path.to_path_buf())
}

/// A Byte RGB GeoTIFF (display only). Byte RGB bypasses the viewer's statistical stretch of
/// non-Byte imagery, so colours keep their fixed, precomputed scale.
pub fn write_rgb(path: &Path, raster: &Raster, bands: &[Vec<u8>; 3]) -> Result<PathBuf> {
    let a = api()?;
    let (width, height) = (raster.width, raster.height);
    if bands.iter().any(|b| b.len() != width * height) {
        return Err("Display image size mismatch.".into());
    }
    let (name, gtiff, wkt) = (c_path(path)?, c("GTiff")?, c(&raster.projection)?);
    let (w, h) = (width as c_int, height as c_int);
    let ds = unsafe {
        (a.create)(
            (a.driver)(gtiff.as_ptr()),
            name.as_ptr(),
            w,
            h,
            3,
            1,
            std::ptr::null(),
        )
    };
    if ds.is_null() {
        return Err(error(&format!("Cannot create GeoTIFF {}.", path.display())));
    }
    let result = unsafe {
        if (a.set_transform)(ds, raster.transform.as_ptr()) != 0
            || (a.set_projection)(ds, wkt.as_ptr()) != 0
        {
            Err(error("Cannot write GeoTIFF georeferencing."))
        } else if (1..=3).any(|band| {
            let data = &bands[band as usize - 1];
            (a.io)(
                (a.band)(ds, band),
                1,
                0,
                0,
                w,
                h,
                data.as_ptr().cast_mut().cast(),
                w,
                h,
                1,
                0,
                0,
            ) != 0
        }) {
            Err(error("Cannot write GeoTIFF values."))
        } else {
            Ok(())
        }
    };
    unsafe { (a.close)(ds) };
    result.map(|()| path.to_path_buf())
}

/// GDALGridLinearOptions, versioned by size since GDAL 3.6.
#[repr(C)]
struct LinearOptions {
    size: usize,
    radius: f64,
    nodata: f64,
}

struct ProgressState<'a> {
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(f64, &str),
}

unsafe extern "C" fn grid_tick(fraction: c_double, _message: *const c_char, user: P) -> c_int {
    let state = &*(user as *const ProgressState);
    (state.progress)(
        65.0 + (fraction * 16.0).trunc(),
        "Interpolating native source pixel centres...",
    );
    c_int::from(!state.cancel.load(std::sync::atomic::Ordering::Relaxed))
}

/// GDAL linear (Delaunay TIN) gridding of the supports at every native pixel centre.
pub fn triangulate(
    raster: &Raster,
    x: &[f64],
    y: &[f64],
    z: &[f64],
    cancel: &AtomicBool,
    progress: &dyn Fn(f64, &str),
) -> Result<Vec<f32>> {
    let a = api()?;
    let options = LinearOptions {
        size: std::mem::size_of::<LinearOptions>(),
        radius: 0.0,
        nodata: f64::from(NODATA),
    };
    check(cancel)?;
    progress(
        62.0,
        &format!("Triangulating {} ground supports...", z.len()),
    );
    let context = unsafe {
        (a.grid_create)(
            10,
            (&options as *const LinearOptions).cast(),
            z.len() as c_uint,
            x.as_ptr(),
            y.as_ptr(),
            z.as_ptr(),
            1,
        )
    };
    check(cancel)?;
    if context.is_null() {
        return Err("GDAL could not triangulate ground supports; check candidate distribution and SDK Qhull support.".into());
    }
    let mut result = vec![NODATA; raster.width * raster.height];
    let state = ProgressState { cancel, progress };
    // Local x = column metres, y = row metres; output GeoTIFFs keep the real CRS.
    let status = unsafe {
        let status = (a.grid_process)(
            context,
            0.0,
            raster.width as f64 * raster.pixel,
            0.0,
            raster.height as f64 * raster.pixel,
            raster.width as c_uint,
            raster.height as c_uint,
            6,
            result.as_mut_ptr().cast(),
            Some(grid_tick),
            (&state as *const ProgressState).cast_mut().cast(),
        );
        (a.grid_free)(context);
        status
    };
    check(cancel)?;
    if status != 0 {
        return Err("TIN interpolation failed.".into());
    }
    Ok(result)
}

/// Three RGB-ordered Byte bands on the DSM's exact grid and their combined validity (all band masks).
pub fn read_rgb(dsm: &Raster, path: &Path, cancel: &AtomicBool) -> Result<([Vec<u8>; 3], Vec<u8>)> {
    check(cancel)?;
    let a = api()?;
    let name = c_path(path)?;
    let ds = unsafe { (a.open)(name.as_ptr(), 0) };
    if ds.is_null() {
        return Err("Cannot open RGB orthophoto.".into());
    }
    let n = dsm.width * dsm.height;
    let (w, h) = (dsm.width as c_int, dsm.height as c_int);
    let result = (|| -> Result<([Vec<u8>; 3], Vec<u8>)> {
        unsafe {
            require(
                (a.count)(ds) == 3,
                "Orthophoto must contain exactly three RGB Byte bands.",
            )?;
            let mut t = [0.0f64; 6];
            let has_transform = (a.get_transform)(ds, t.as_mut_ptr()) == 0;
            let raw = (a.get_projection)(ds);
            let projection = if raw.is_null() {
                String::new()
            } else {
                CStr::from_ptr(raw).to_string_lossy().into_owned()
            };
            require(
                same_projection(&dsm.projection, &projection)?
                    && has_transform
                    && (a.width)(ds) == w
                    && (a.height)(ds) == h
                    && t == dsm.transform,
                "RGB and DSM must have the same dimensions, exact affine grid and equivalent CRS. Align them before loading.",
            )?;
            let mut bands: [Vec<u8>; 3] = Default::default();
            let (mut valid, mut mask) = (vec![1u8; n], vec![0u8; n]);
            for (channel, pixels) in bands.iter_mut().enumerate() {
                check(cancel)?;
                let band = (a.band)(ds, channel as c_int + 1);
                require(
                    (a.data_type)(band) == 1 && (a.color)(band) == channel as c_int + 3,
                    "Use RGB-ordered Byte bands; no implicit spectral or color conversion is performed.",
                )?;
                let mut present = 0;
                let scale = (a.scale)(band, &mut present);
                require(
                    present == 0 || (scale - 1.0).abs() < 1e-12,
                    "Apply RGB scale before loading.",
                )?;
                let offset = (a.offset)(band, &mut present);
                require(
                    present == 0 || offset.abs() < 1e-12,
                    "Apply RGB offset before loading.",
                )?;
                *pixels = vec![0u8; n];
                if (a.io)(
                    band,
                    0,
                    0,
                    0,
                    w,
                    h,
                    pixels.as_mut_ptr().cast(),
                    w,
                    h,
                    1,
                    0,
                    0,
                ) != 0
                    || (a.io)(
                        (a.mask)(band),
                        0,
                        0,
                        0,
                        w,
                        h,
                        mask.as_mut_ptr().cast(),
                        w,
                        h,
                        1,
                        0,
                        0,
                    ) != 0
                {
                    return Err(error("Cannot read RGB pixels or validity."));
                }
                for (v, m) in valid.iter_mut().zip(&mask) {
                    if *m == 0 {
                        *v = 0;
                    }
                }
            }
            Ok((bands, valid))
        }
    })();
    unsafe { (a.close)(ds) };
    result
}

/// A multi-band Float32 GeoTIFF on the raster's grid without NoData (the model input stack).
pub fn write_float_bands(raster: &Raster, path: &Path, bands: &[Vec<f32>]) -> Result<PathBuf> {
    let a = api()?;
    let (name, gtiff, wkt) = (c_path(path)?, c("GTiff")?, c(&raster.projection)?);
    let (w, h) = (raster.width as c_int, raster.height as c_int);
    let ds = unsafe {
        (a.create)(
            (a.driver)(gtiff.as_ptr()),
            name.as_ptr(),
            w,
            h,
            bands.len() as c_int,
            6,
            std::ptr::null(),
        )
    };
    if ds.is_null() {
        return Err(error(&format!("Cannot create GeoTIFF {}.", path.display())));
    }
    let result = unsafe {
        if (a.set_transform)(ds, raster.transform.as_ptr()) != 0
            || (a.set_projection)(ds, wkt.as_ptr()) != 0
        {
            Err(error("Cannot write GeoTIFF georeferencing."))
        } else if bands.iter().enumerate().any(|(i, data)| {
            (a.io)(
                (a.band)(ds, i as c_int + 1),
                1,
                0,
                0,
                w,
                h,
                data.as_ptr().cast_mut().cast(),
                w,
                h,
                6,
                0,
                0,
            ) != 0
        }) {
            Err(error("Cannot write GeoTIFF values."))
        } else {
            Ok(())
        }
    };
    unsafe { (a.close)(ds) };
    result.map(|()| path.to_path_buf())
}
