//! Float32 raster IO, CRS checks and the four pinned height grids, through the GDAL
//! runtime shipped in the published SDK (no system GDAL).
use crate::sample::{check, Result};
use libloading::Library;
use serde_json::Value;
use std::{
    ffi::{c_char, c_double, c_int, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, OnceLock},
};

type P = *mut c_void;

struct Api {
    _library: Library,
    open: unsafe extern "C" fn(*const c_char, c_int) -> P,
    close: unsafe extern "C" fn(P),
    width: unsafe extern "C" fn(P) -> c_int,
    height: unsafe extern "C" fn(P) -> c_int,
    band: unsafe extern "C" fn(P, c_int) -> P,
    nodata: unsafe extern "C" fn(P, *mut c_int) -> c_double,
    set_nodata: unsafe extern "C" fn(P, c_double) -> c_int,
    io: unsafe extern "C" fn(
        P,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
    ) -> c_int,
    get_transform: unsafe extern "C" fn(P, *mut c_double) -> c_int,
    set_transform: unsafe extern "C" fn(P, *const c_double) -> c_int,
    get_projection: unsafe extern "C" fn(P) -> *const c_char,
    set_projection: unsafe extern "C" fn(P, *const c_char) -> c_int,
    driver: unsafe extern "C" fn(*const c_char) -> P,
    create: unsafe extern "C" fn(P, *const c_char, c_int, c_int, c_int, c_int, P) -> P,
    last_error: unsafe extern "C" fn() -> *const c_char,
    new_srs: unsafe extern "C" fn(*const c_char) -> P,
    destroy_srs: unsafe extern "C" fn(P),
    from_epsg: unsafe extern "C" fn(P, c_int) -> c_int,
    from_user: unsafe extern "C" fn(P, *const c_char) -> c_int,
    same: unsafe extern "C" fn(P, P) -> c_int,
    proj_paths: unsafe extern "C" fn(*const *const c_char),
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
        let api = Api {
            open: f!("GDALOpen"),
            close: f!("GDALClose"),
            width: f!("GDALGetRasterXSize"),
            height: f!("GDALGetRasterYSize"),
            band: f!("GDALGetRasterBand"),
            nodata: f!("GDALGetRasterNoDataValue"),
            set_nodata: f!("GDALSetRasterNoDataValue"),
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
            from_epsg: f!("OSRImportFromEPSG"),
            from_user: f!("OSRSetFromUserInput"),
            same: f!("OSRIsSame"),
            proj_paths: f!("OSRSetPROJSearchPaths"),
            _library: library,
        };
        register();
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

pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub transform: [f64; 6],
    pub projection: String,
    pub nodata: Option<f64>,
    pub values: Vec<f32>,
}

pub fn read(path: &Path) -> Result<Raster> {
    let a = api()?;
    let name = c_path(path)?;
    let ds = unsafe { (a.open)(name.as_ptr(), 0) };
    if ds.is_null() {
        return Err(error(&format!("Cannot open raster {}.", path.display())));
    }
    let result = (|| unsafe {
        let (w, h) = ((a.width)(ds), (a.height)(ds));
        if w < 1 || h < 1 || i64::from(w) * i64::from(h) > 64_000_000 {
            return Err(format!(
                "Unsupported raster dimensions in {}.",
                path.display()
            ));
        }
        let mut transform = [0.0; 6];
        if (a.get_transform)(ds, transform.as_mut_ptr()) != 0 {
            return Err(format!(
                "Raster has no world transform: {}.",
                path.display()
            ));
        }
        let projection = CStr::from_ptr((a.get_projection)(ds))
            .to_string_lossy()
            .into_owned();
        let band = (a.band)(ds, 1);
        let mut values = vec![0f32; (w * h) as usize];
        if (a.io)(
            band,
            0,
            0,
            0,
            w,
            h,
            values.as_mut_ptr().cast(),
            w,
            h,
            6,
            0,
            0,
        ) != 0
        {
            return Err(error(&format!(
                "Could not read numeric values from {}.",
                path.display()
            )));
        }
        let mut has = 0;
        let nodata = (a.nodata)(band, &mut has);
        Ok(Raster {
            width: w as usize,
            height: h as usize,
            transform,
            projection,
            nodata: (has != 0).then_some(nodata),
            values,
        })
    })();
    unsafe { (a.close)(ds) };
    result
}

pub fn same_crs(projection: &str, epsg: i32) -> Result<bool> {
    let a = api()?;
    let text = c(projection)?;
    unsafe {
        let (left, right) = ((a.new_srs)(std::ptr::null()), (a.new_srs)(std::ptr::null()));
        let same = (a.from_user)(left, text.as_ptr()) == 0
            && (a.from_epsg)(right, epsg) == 0
            && (a.same)(left, right) != 0;
        (a.destroy_srs)(left);
        (a.destroy_srs)(right);
        Ok(same)
    }
}

pub fn write_float(
    path: &Path,
    width: usize,
    height: usize,
    transform: &[f64; 6],
    projection: &str,
    values: &[f32],
) -> Result<PathBuf> {
    let a = api()?;
    if values.len() != width * height {
        return Err("GeoTIFF band size mismatch.".into());
    }
    let (name, gtiff, wkt) = (c_path(path)?, c("GTiff")?, c(projection)?);
    let ds = unsafe {
        (a.create)(
            (a.driver)(gtiff.as_ptr()),
            name.as_ptr(),
            width as c_int,
            height as c_int,
            1,
            6,
            std::ptr::null_mut(),
        )
    };
    if ds.is_null() {
        return Err(error(&format!("Cannot create GeoTIFF {}.", path.display())));
    }
    let result = unsafe {
        if (a.set_transform)(ds, transform.as_ptr()) != 0
            || (a.set_projection)(ds, wkt.as_ptr()) != 0
        {
            Err(error("Cannot write GeoTIFF georeferencing."))
        } else {
            let band = (a.band)(ds, 1);
            (a.set_nodata)(band, -9999.0);
            let (w, h) = (width as c_int, height as c_int);
            if (a.io)(
                band,
                1,
                0,
                0,
                w,
                h,
                values.as_ptr().cast_mut().cast(),
                w,
                h,
                6,
                0,
                0,
            ) != 0
            {
                Err(error("Cannot write GeoTIFF values."))
            } else {
                Ok(())
            }
        }
    };
    unsafe { (a.close)(ds) };
    result.map(|()| path.to_path_buf())
}

const NAMES: [&str; 4] = [
    "dsm.tif",
    "dtm_classical.tif",
    "dtm_ai.tif",
    "dtm_reference.tif",
];

/// The four pinned 500 x 500, 0.5 m EPSG:2056 height rasters of the sample.
pub struct Grids {
    pub folder: PathBuf,
    pub width: usize,
    pub height: usize,
    pub transform: [f64; 6],
    pub projection: String,
    pub manifest: Value,
    pub profile_row: usize,
    pub z: [Vec<f32>; 4],
}

impl Grids {
    pub fn read(folder: &Path, cancel: &AtomicBool) -> Result<Self> {
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(folder.join("source-manifest.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let row = manifest["profile"]["row"].as_i64().unwrap_or(-1);
        let mut rasters: Vec<Raster> = Vec::new();
        for name in NAMES {
            check(cancel)?;
            let raster = read(&folder.join(name))?;
            if !same_crs(&raster.projection, 2056)? {
                return Err("The sample height CRS is not equivalent to EPSG:2056.".into());
            }
            let t = raster.transform;
            if raster.width != 500
                || raster.height != 500
                || t[1] != 0.5
                || t[5] != -0.5
                || t[2] != 0.0
                || t[4] != 0.0
            {
                return Err(format!("The sample requires the pinned 500 x 500, 0.5 m grid. Found {} x {}; pixel {}, {}; rotation {}, {}.", raster.width, raster.height, t[1], t[5], t[4], t[2]));
            }
            if let Some(first) = rasters.first() {
                if first.projection != raster.projection || first.transform != raster.transform {
                    return Err("Height rasters do not share exactly the same CRS and grid.".into());
                }
            }
            if raster
                .values
                .iter()
                .any(|v| !v.is_finite() || raster.nodata.is_some_and(|n| f64::from(*v) == n))
            {
                return Err(
                    "This prepared comparison requires valid heights in all four rasters.".into(),
                );
            }
            rasters.push(raster);
        }
        let first = &rasters[0];
        if row < 0 || row as usize >= first.height {
            return Err("Invalid sample profile row.".into());
        }
        let (width, height, transform, projection) = (
            first.width,
            first.height,
            first.transform,
            first.projection.clone(),
        );
        let mut z = rasters.into_iter().map(|r| r.values);
        Ok(Self {
            folder: folder.to_path_buf(),
            width,
            height,
            transform,
            projection,
            manifest,
            profile_row: row as usize,
            z: [
                z.next().unwrap(),
                z.next().unwrap(),
                z.next().unwrap(),
                z.next().unwrap(),
            ],
        })
    }

    /// 0% is exactly the DSM, 100% exactly the selected DTM; between: (1-t)*DSM + t*DTM.
    pub fn blended(&self, method: usize, percent: u32) -> Vec<f32> {
        let t = f64::from(percent) / 100.0;
        self.z[0]
            .iter()
            .zip(&self.z[method])
            .map(|(a, b)| ((1.0 - t) * f64::from(*a) + t * f64::from(*b)) as f32)
            .collect()
    }

    pub fn surface_file(&self, method: usize, percent: u32, temporary: &Path) -> Result<PathBuf> {
        match percent {
            0 => Ok(self.folder.join("dsm.tif")),
            100 => Ok(self.folder.join(if method == 1 {
                "dtm_classical.tif"
            } else {
                "dtm_ai.tif"
            })),
            _ => write_float(
                temporary,
                self.width,
                self.height,
                &self.transform,
                &self.projection,
                &self.blended(method, percent),
            ),
        }
    }

    /// Viewer3D maps local ENU metres to render units by 2 / horizontal span of the pixel-centre grid.
    pub fn render_scale(&self) -> f64 {
        2.0 / ((self.width - 1) as f64 * self.transform[1])
    }

    pub fn mean_dsm(&self) -> f64 {
        self.z[0].iter().map(|v| f64::from(*v)).sum::<f64>() / self.z[0].len() as f64
    }

    pub fn metric(&self, method: &str, name: &str) -> f64 {
        self.manifest["metrics"][method][name]
            .as_f64()
            .unwrap_or(0.0)
    }

    pub fn profile(&self) -> [&[f32]; 4] {
        let start = self.profile_row * self.width;
        [0, 1, 2, 3].map(|i| &self.z[i][start..start + self.width])
    }
}
