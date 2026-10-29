//! Published GDAL C ABI declarations. No separate GDAL installation.
#![allow(non_snake_case)]
use libloading::Library;
use std::{
    ffi::{c_char, c_int, c_void},
    path::Path,
};
pub type Handle = *mut c_void;
pub struct Gdal {
    pub GDALAllRegister: unsafe extern "C" fn() -> (),
    pub GDALOpen: unsafe extern "C" fn(*const c_char, c_int) -> Handle,
    pub GDALOpenEx: unsafe extern "C" fn(*const c_char, u32, Handle, Handle, Handle) -> Handle,
    pub GDALClose: unsafe extern "C" fn(Handle) -> (),
    pub GDALGetDriverByName: unsafe extern "C" fn(*const c_char) -> Handle,
    pub GDALAutoCreateWarpedVRT:
        unsafe extern "C" fn(Handle, *const c_char, *const c_char, c_int, f64, Handle) -> Handle,
    pub GDALCreateCopy: unsafe extern "C" fn(
        Handle,
        *const c_char,
        Handle,
        c_int,
        Handle,
        Handle,
        Handle,
    ) -> Handle,
    pub GDALCreate:
        unsafe extern "C" fn(Handle, *const c_char, c_int, c_int, c_int, c_int, Handle) -> Handle,
    pub GDALGetRasterXSize: unsafe extern "C" fn(Handle) -> c_int,
    pub GDALGetRasterYSize: unsafe extern "C" fn(Handle) -> c_int,
    pub GDALGetRasterCount: unsafe extern "C" fn(Handle) -> c_int,
    pub GDALGetRasterBand: unsafe extern "C" fn(Handle, c_int) -> Handle,
    pub GDALGetGeoTransform: unsafe extern "C" fn(Handle, *mut f64) -> c_int,
    pub GDALSetGeoTransform: unsafe extern "C" fn(Handle, *mut f64) -> c_int,
    pub GDALGetProjectionRef: unsafe extern "C" fn(Handle) -> *const c_char,
    pub GDALSetProjection: unsafe extern "C" fn(Handle, *const c_char) -> c_int,
    pub GDALRasterIO: unsafe extern "C" fn(
        Handle,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        Handle,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
    ) -> c_int,
    pub GDALSetRasterColorInterpretation: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub GDALDatasetGetLayer: unsafe extern "C" fn(Handle, c_int) -> Handle,
    pub OGR_L_ResetReading: unsafe extern "C" fn(Handle) -> (),
    pub OGR_L_GetNextFeature: unsafe extern "C" fn(Handle) -> Handle,
    pub OGR_F_Destroy: unsafe extern "C" fn(Handle) -> (),
    pub OGR_F_GetGeometryRef: unsafe extern "C" fn(Handle) -> Handle,
    pub OGR_F_GetFieldIndex: unsafe extern "C" fn(Handle, *const c_char) -> c_int,
    pub OGR_F_GetFieldAsInteger: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub OGR_G_ExportToJson: unsafe extern "C" fn(Handle) -> Handle,
    pub OGR_G_CreateGeometryFromJson: unsafe extern "C" fn(*const c_char) -> Handle,
    pub OGR_G_DestroyGeometry: unsafe extern "C" fn(Handle) -> (),
    pub OGR_G_Clone: unsafe extern "C" fn(Handle) -> Handle,
    pub OGR_G_Area: unsafe extern "C" fn(Handle) -> f64,
    pub OGR_G_IsValid: unsafe extern "C" fn(Handle) -> c_int,
    pub OGR_G_Transform: unsafe extern "C" fn(Handle, Handle) -> c_int,
    pub VSIFree: unsafe extern "C" fn(Handle) -> (),
    pub OSRNewSpatialReference: unsafe extern "C" fn(*const c_char) -> Handle,
    pub OSRImportFromEPSG: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub OSRSetAxisMappingStrategy: unsafe extern "C" fn(Handle, c_int) -> (),
    pub OSRDestroySpatialReference: unsafe extern "C" fn(Handle) -> (),
    pub OCTNewCoordinateTransformation: unsafe extern "C" fn(Handle, Handle) -> Handle,
    pub OCTDestroyCoordinateTransformation: unsafe extern "C" fn(Handle) -> (),
    pub OCTTransform: unsafe extern "C" fn(Handle, c_int, *mut f64, *mut f64, *mut f64) -> c_int,
    pub GDALRasterizeLayers: unsafe extern "C" fn(
        Handle,
        c_int,
        Handle,
        c_int,
        Handle,
        Handle,
        Handle,
        Handle,
        Handle,
        Handle,
        Handle,
    ) -> c_int,
    _library: Library,
}
impl Gdal {
    pub fn load(bin: &Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(bin.join("gdal.dll")).map_err(|e| e.to_string())?;
            Ok(Self {
                GDALAllRegister: *lib
                    .get(concat!("GDALAllRegister", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALOpen: *lib
                    .get(concat!("GDALOpen", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALOpenEx: *lib
                    .get(concat!("GDALOpenEx", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALClose: *lib
                    .get(concat!("GDALClose", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetDriverByName: *lib
                    .get(concat!("GDALGetDriverByName", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALAutoCreateWarpedVRT: *lib
                    .get(b"GDALAutoCreateWarpedVRT\0")
                    .map_err(|e| e.to_string())?,
                GDALCreateCopy: *lib.get(b"GDALCreateCopy\0").map_err(|e| e.to_string())?,
                GDALCreate: *lib
                    .get(concat!("GDALCreate", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetRasterXSize: *lib
                    .get(concat!("GDALGetRasterXSize", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetRasterYSize: *lib
                    .get(concat!("GDALGetRasterYSize", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetRasterCount: *lib
                    .get(concat!("GDALGetRasterCount", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetRasterBand: *lib
                    .get(concat!("GDALGetRasterBand", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetGeoTransform: *lib
                    .get(concat!("GDALGetGeoTransform", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALSetGeoTransform: *lib
                    .get(concat!("GDALSetGeoTransform", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALGetProjectionRef: *lib
                    .get(concat!("GDALGetProjectionRef", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALSetProjection: *lib
                    .get(concat!("GDALSetProjection", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALRasterIO: *lib
                    .get(concat!("GDALRasterIO", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALSetRasterColorInterpretation: *lib
                    .get(concat!("GDALSetRasterColorInterpretation", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALDatasetGetLayer: *lib
                    .get(concat!("GDALDatasetGetLayer", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_L_ResetReading: *lib
                    .get(concat!("OGR_L_ResetReading", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_L_GetNextFeature: *lib
                    .get(concat!("OGR_L_GetNextFeature", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_F_Destroy: *lib
                    .get(concat!("OGR_F_Destroy", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_F_GetGeometryRef: *lib
                    .get(concat!("OGR_F_GetGeometryRef", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_F_GetFieldIndex: *lib
                    .get(concat!("OGR_F_GetFieldIndex", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_F_GetFieldAsInteger: *lib
                    .get(concat!("OGR_F_GetFieldAsInteger", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_ExportToJson: *lib
                    .get(concat!("OGR_G_ExportToJson", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_CreateGeometryFromJson: *lib
                    .get(concat!("OGR_G_CreateGeometryFromJson", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_DestroyGeometry: *lib
                    .get(concat!("OGR_G_DestroyGeometry", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_Clone: *lib
                    .get(concat!("OGR_G_Clone", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_Area: *lib
                    .get(concat!("OGR_G_Area", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_IsValid: *lib
                    .get(concat!("OGR_G_IsValid", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OGR_G_Transform: *lib
                    .get(concat!("OGR_G_Transform", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                VSIFree: *lib
                    .get(concat!("VSIFree", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OSRNewSpatialReference: *lib
                    .get(concat!("OSRNewSpatialReference", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OSRImportFromEPSG: *lib
                    .get(concat!("OSRImportFromEPSG", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OSRSetAxisMappingStrategy: *lib
                    .get(concat!("OSRSetAxisMappingStrategy", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OSRDestroySpatialReference: *lib
                    .get(concat!("OSRDestroySpatialReference", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OCTNewCoordinateTransformation: *lib
                    .get(concat!("OCTNewCoordinateTransformation", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OCTDestroyCoordinateTransformation: *lib
                    .get(concat!("OCTDestroyCoordinateTransformation", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                OCTTransform: *lib
                    .get(concat!("OCTTransform", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                GDALRasterizeLayers: *lib
                    .get(concat!("GDALRasterizeLayers", "\0").as_bytes())
                    .map_err(|e| e.to_string())?,
                _library: lib,
            })
        }
    }
}
