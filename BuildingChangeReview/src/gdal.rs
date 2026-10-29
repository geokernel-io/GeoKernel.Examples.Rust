pub use crate::gdal_bindings::{Gdal, Handle};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    path::Path,
};
pub type R<T> = Result<T, String>;
fn c(value: &str) -> R<CString> {
    CString::new(value).map_err(|e| e.to_string())
}
fn p(path: &Path) -> R<CString> {
    c(&path.to_string_lossy().replace(r"\\?\", ""))
}
pub fn check(value: i32) -> R<()> {
    if value == 0 {
        Ok(())
    } else {
        Err("GDAL operation failed".into())
    }
}
#[derive(Clone)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub gt: [f64; 6],
    pub crs: String,
}
impl Grid {
    pub fn same(&self, g: &Self) -> bool {
        self.width == g.width && self.height == g.height && self.gt == g.gt && self.crs == g.crs
    }
}
pub struct Polygon {
    pub id: usize,
    pub geometry: Value,
    pub area: f64,
}
pub struct Dataset<'a> {
    pub g: &'a Gdal,
    pub handle: Handle,
}
impl Drop for Dataset<'_> {
    fn drop(&mut self) {
        unsafe { (self.g.GDALClose)(self.handle) }
    }
}
impl Gdal {
    pub fn initialize(&self) {
        unsafe { (self.GDALAllRegister)() }
    }
    pub fn open(&self, path: &Path) -> R<Dataset<'_>> {
        unsafe {
            let handle = (self.GDALOpen)(p(path)?.as_ptr(), 0);
            if handle.is_null() {
                Err(format!("Cannot open {}", path.display()))
            } else {
                Ok(Dataset { g: self, handle })
            }
        }
    }
    pub fn metadata(&self, ds: Handle) -> R<Grid> {
        unsafe {
            let mut gt = [0.; 6];
            check((self.GDALGetGeoTransform)(ds, gt.as_mut_ptr()))?;
            Ok(Grid {
                width: (self.GDALGetRasterXSize)(ds) as usize,
                height: (self.GDALGetRasterYSize)(ds) as usize,
                gt,
                crs: CStr::from_ptr((self.GDALGetProjectionRef)(ds))
                    .to_string_lossy()
                    .into_owned(),
            })
        }
    }
    pub fn display_raster(&self, source: &Path, destination: &Path) -> R<()> {
        // Display pixels use WGS84; analysis retains the original metre grid.
        unsafe {
            let ds = self.open(source)?;
            let warped = (self.GDALAutoCreateWarpedVRT)(
                ds.handle,
                std::ptr::null(),
                c("EPSG:4326")?.as_ptr(),
                1,
                0.,
                std::ptr::null_mut(),
            );
            if warped.is_null() {
                return Err("Cannot reproject imagery for the map".into());
            }
            let warped = Dataset {
                g: self,
                handle: warped,
            };
            let output = (self.GDALCreateCopy)(
                (self.GDALGetDriverByName)(c("GTiff")?.as_ptr()),
                p(destination)?.as_ptr(),
                warped.handle,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if output.is_null() {
                return Err("Cannot create the map preview".into());
            }
            let output = Dataset {
                g: self,
                handle: output,
            };
            let grid = self.metadata(output.handle)?;
            if grid.width == 0
                || grid.height == 0
                || !(-180. ..=180.).contains(&grid.gt[0])
                || !(-90. ..=90.).contains(&grid.gt[3])
            {
                return Err("Map preview was not reprojected to WGS84".into());
            }
            Ok(())
        }
    }
    fn create(
        &self,
        driver: &str,
        name: &str,
        w: usize,
        h: usize,
        bands: i32,
        data_type: i32,
    ) -> R<Dataset<'_>> {
        unsafe {
            let handle = (self.GDALCreate)(
                (self.GDALGetDriverByName)(c(driver)?.as_ptr()),
                c(name)?.as_ptr(),
                w as i32,
                h as i32,
                bands,
                data_type,
                std::ptr::null_mut(),
            );
            if handle.is_null() {
                Err("Cannot create GDAL dataset".into())
            } else {
                Ok(Dataset { g: self, handle })
            }
        }
    }
    pub fn read_float(&self, ds: Handle, band: i32) -> R<Vec<f32>> {
        unsafe {
            let g = self.metadata(ds)?;
            let mut a = vec![0f32; g.width * g.height];
            check((self.GDALRasterIO)(
                (self.GDALGetRasterBand)(ds, band),
                0,
                0,
                0,
                g.width as i32,
                g.height as i32,
                a.as_mut_ptr().cast(),
                g.width as i32,
                g.height as i32,
                6,
                0,
                0,
            ))?;
            Ok(a)
        }
    }
    pub fn crop(&self, source: &Path, bounds: [f64; 4], target: &Path) -> R<()> {
        unsafe {
            let ds = self.open(source)?;
            let g = self.metadata(ds.handle)?;
            let t = g.gt;
            if t[1] != 0.5 || t[5] != -0.5 || t[2] != 0. || t[4] != 0. || !g.crs.contains("25831") {
                return Err("Expected pinned EPSG:25831 0.5m imagery".into());
            }
            let x = ((bounds[0] - t[0]) / 0.5).floor().max(0.) as usize;
            let y = ((t[3] - bounds[3]) / 0.5).floor().max(0.) as usize;
            let end_x = (((bounds[2] - t[0]) / 0.5).ceil().max(0.) as usize).min(g.width);
            let end_y = (((t[3] - bounds[1]) / 0.5).ceil().max(0.) as usize).min(g.height);
            if end_x <= x || end_y <= y {
                return Err("Analysis area does not intersect imagery".into());
            }
            let (w, h) = (end_x - x, end_y - y);
            let out = self.create("GTiff", &target.to_string_lossy(), w, h, 3, 6)?;
            let mut gt = [
                t[0] + x as f64 * 0.5,
                0.5,
                0.,
                t[3] - y as f64 * 0.5,
                0.,
                -0.5,
            ];
            check((self.GDALSetProjection)(out.handle, c(&g.crs)?.as_ptr()))?;
            check((self.GDALSetGeoTransform)(out.handle, gt.as_mut_ptr()))?;
            for band in 1..=3 {
                let mut a = vec![0f32; w * h];
                check((self.GDALRasterIO)(
                    (self.GDALGetRasterBand)(ds.handle, band),
                    0,
                    x as i32,
                    y as i32,
                    w as i32,
                    h as i32,
                    a.as_mut_ptr().cast(),
                    w as i32,
                    h as i32,
                    6,
                    0,
                    0,
                ))?;
                check((self.GDALRasterIO)(
                    (self.GDALGetRasterBand)(out.handle, band),
                    1,
                    0,
                    0,
                    w as i32,
                    h as i32,
                    a.as_mut_ptr().cast(),
                    w as i32,
                    h as i32,
                    6,
                    0,
                    0,
                ))?;
                check((self.GDALSetRasterColorInterpretation)(
                    (self.GDALGetRasterBand)(out.handle, band),
                    band + 2,
                ))?;
            }
            Ok(())
        }
    }
    unsafe fn json(&self, geom: Handle) -> R<Value> {
        let raw = (self.OGR_G_ExportToJson)(geom);
        if raw.is_null() {
            return Err("Cannot serialize geometry".into());
        }
        let result = serde_json::from_str(&CStr::from_ptr(raw.cast()).to_string_lossy())
            .map_err(|e| e.to_string());
        (self.VSIFree)(raw);
        result
    }
    pub fn geometry(&self, value: &Value, source: i32, target: i32) -> R<(Value, f64)> {
        unsafe {
            let geom = (self.OGR_G_CreateGeometryFromJson)(c(&value.to_string())?.as_ptr());
            if geom.is_null() {
                return Err("Invalid geometry JSON".into());
            }
            let (mut a, mut b, mut operation) = (
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            let result = (|| {
                if (self.OGR_G_IsValid)(geom) == 0 {
                    return Err("Invalid polygon; repair before approval".into());
                }
                if source != target {
                    a = (self.OSRNewSpatialReference)(std::ptr::null());
                    b = (self.OSRNewSpatialReference)(std::ptr::null());
                    check((self.OSRImportFromEPSG)(a, source))?;
                    check((self.OSRImportFromEPSG)(b, target))?;
                    (self.OSRSetAxisMappingStrategy)(a, 0);
                    (self.OSRSetAxisMappingStrategy)(b, 0);
                    operation = (self.OCTNewCoordinateTransformation)(a, b);
                    if operation.is_null() {
                        return Err("Cannot transform geometry".into());
                    }
                    check((self.OGR_G_Transform)(geom, operation))?;
                }
                Ok((self.json(geom)?, (self.OGR_G_Area)(geom)))
            })();
            (self.OGR_G_DestroyGeometry)(geom);
            if !operation.is_null() {
                (self.OCTDestroyCoordinateTransformation)(operation)
            }
            if !a.is_null() {
                (self.OSRDestroySpatialReference)(a)
            }
            if !b.is_null() {
                (self.OSRDestroySpatialReference)(b)
            }
            result
        }
    }
    pub fn polygons_and_labels(&self, path: &Path, g: &Grid) -> R<(Vec<Polygon>, Vec<i32>)> {
        unsafe {
            let handle = (self.GDALOpenEx)(
                p(path)?.as_ptr(),
                4,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if handle.is_null() {
                return Err("Cannot open predicted polygons".into());
            }
            let ds = Dataset { g: self, handle };
            let layer = (self.GDALDatasetGetLayer)(ds.handle, 0);
            if layer.is_null() {
                return Err("Prediction layer missing".into());
            }
            let mut polygons = vec![];
            loop {
                let f = (self.OGR_L_GetNextFeature)(layer);
                if f.is_null() {
                    break;
                }
                let record: R<Option<Polygon>> = (|| {
                    let geom = (self.OGR_F_GetGeometryRef)(f);
                    if geom.is_null() || (self.OGR_G_IsValid)(geom) == 0 {
                        return Ok(None);
                    }
                    let field = (self.OGR_F_GetFieldIndex)(f, c("instance_id")?.as_ptr());
                    if field < 0 {
                        return Err("Missing instance_id".into());
                    }
                    Ok(Some(Polygon {
                        id: (self.OGR_F_GetFieldAsInteger)(f, field) as usize,
                        geometry: self.json(geom)?,
                        area: (self.OGR_G_Area)(geom),
                    }))
                })();
                (self.OGR_F_Destroy)(f);
                if let Some(p) = record? {
                    polygons.push(p)
                }
            }
            let labels = self.create("MEM", "", g.width, g.height, 1, 5)?;
            let mut gt = g.gt;
            check((self.GDALSetProjection)(labels.handle, c(&g.crs)?.as_ptr()))?;
            check((self.GDALSetGeoTransform)(labels.handle, gt.as_mut_ptr()))?;
            if !polygons.is_empty() {
                (self.OGR_L_ResetReading)(layer);
                let mut band = [1i32];
                let mut layers = [layer];
                let attr = c("ATTRIBUTE=instance_id")?;
                let mut opts = [attr.as_ptr(), std::ptr::null()];
                check((self.GDALRasterizeLayers)(
                    labels.handle,
                    1,
                    band.as_mut_ptr().cast(),
                    1,
                    layers.as_mut_ptr().cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    opts.as_mut_ptr().cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ))?;
            }
            let mut data = vec![0i32; g.width * g.height];
            check((self.GDALRasterIO)(
                (self.GDALGetRasterBand)(labels.handle, 1),
                0,
                0,
                0,
                g.width as i32,
                g.height as i32,
                data.as_mut_ptr().cast(),
                g.width as i32,
                g.height as i32,
                5,
                0,
                0,
            ))?;
            Ok((polygons, data))
        }
    }
}
pub fn rectangle(b: [f64; 4]) -> Value {
    json!({"type":"Polygon","coordinates":[[[b[0],b[1]],[b[2],b[1]],[b[2],b[3]],[b[0],b[3]],[b[0],b[1]]]]})
}
pub fn envelope(geometry: &Value) -> R<[f64; 4]> {
    let mut points = vec![];
    fn walk(v: &Value, p: &mut Vec<[f64; 2]>) -> R<()> {
        let a = v.as_array().ok_or("Invalid coordinates")?;
        if a.first().is_some_and(Value::is_number) {
            p.push([
                a[0].as_f64().ok_or("Invalid X")?,
                a[1].as_f64().ok_or("Invalid Y")?,
            ]);
        } else {
            for child in a {
                walk(child, p)?;
            }
        }
        Ok(())
    }
    walk(&geometry["coordinates"], &mut points)?;
    if points.is_empty() {
        return Err("Empty polygon".into());
    }
    Ok([
        points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
        points
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max),
        points
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max),
    ])
}
