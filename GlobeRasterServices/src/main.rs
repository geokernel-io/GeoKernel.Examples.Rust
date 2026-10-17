#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{DemRequest, GlobeCamera, GlobeOptions, JobStatus, RasterRequest},
    Globe, Runtime, ViewerWindow,
};

use std::{error::Error, ffi::c_void, thread, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetClientRect(window: *mut c_void, rect: *mut Rect) -> i32;
    fn MessageBoxW(window: *mut c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

fn main() {
    if let Err(error) = open_globe() {
        let text: Vec<u16> = error.to_string().encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "GlobeRasterServices"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| -> Result<()> {
        let mut window =
            unsafe { ViewerWindow::new(&runtime, "Globe — Sagrada Ortho & DEM", 1200, 800)? };
        window.set_status_text("Loading Sagrada orthophoto…")?;
        window.show()?;
        window.process_events();
        let parent = window.viewer().get_native_handle()?;
        let camera = GlobeCamera {
            longitude: 2.1744,
            latitude: 41.4036,
            target_height_metres: 0.0,
            distance_metres: 2500.0,
            pitch_degrees: 1.32_f64.to_degrees(),
            heading_degrees: 0.6_f64.to_degrees(),
        };
        let options = GlobeOptions {
            atmosphere_enabled: true,
            ..Default::default()
        };
        let globe = unsafe { Globe::embedded(&runtime, parent, &options, Some(&camera))? };
        let esri = globe
            .basemaps()?
            .into_iter()
            .find(|map| map.name == "Esri Satellite")
            .ok_or("Esri Satellite basemap is unavailable")?
            .id;
        globe.set_basemap(esri)?;
        globe.set_visible(true)?;
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let mut job = Some(
            globe.load_raster(&RasterRequest {
                id: "ortho".into(),
                path: data
                    .join("sagrada_familia_ortophoto.tif")
                    .to_string_lossy()
                    .into_owned(),
                ..Default::default()
            })?,
        );
        let mut loading_dem = false;
        let mut loaded = false;
        let mut size = (0, 0);
        let mut last_errors = std::time::Instant::now();
        while window.is_visible()? {
            globe.process_events()?;
            if !window.is_visible()? {
                break;
            }
            let mut rect = Rect::default();
            if unsafe { GetClientRect(parent as *mut c_void, &mut rect) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let scale = globe.state()?.device_pixel_ratio;
            let current = (
                ((rect.right - rect.left) as f64 / scale).round() as i32,
                ((rect.bottom - rect.top) as f64 / scale).round() as i32,
            );
            if current != size && current.0 > 0 && current.1 > 0 {
                globe.resize(current.0, current.1)?;
                size = current;
            }
            if let Some(id) = job {
                let state = globe.job_state(id)?;
                if !matches!(state.status, JobStatus::Queued | JobStatus::Running) {
                    globe.forget_job(id)?;
                    job = None;
                    if state.status != JobStatus::Completed {
                        window.set_status_text(&format!("Operation failed: {}", state.error))?;
                    } else if !loading_dem {
                        window.set_status_text("Loading Sagrada DEM…")?;
                        job = Some(
                            globe.load_dem(&DemRequest {
                                id: "dem".into(),
                                path: data
                                    .join("sagrada_familia_terrain.tif")
                                    .to_string_lossy()
                                    .into_owned(),
                                ..Default::default()
                            })?,
                        );
                        loading_dem = true;
                    } else {
                        let stats = globe.layer_state("dem")?.statistics;
                        // DEM supplies elevation without coloring over the orthophoto.
                        globe.set_dem_style(
                            false,
                            stats["minimum"].as_f64().ok_or("Missing DEM minimum")?,
                            stats["maximum"].as_f64().ok_or("Missing DEM maximum")?,
                        )?;
                        let mut options = globe.options()?;
                        options.terrain_enabled = true;
                        options.terrain_exaggeration = 1.0;
                        globe.set_options(&options)?;
                        globe.focus_layer("ortho")?;
                        let mut camera = globe.camera()?;
                        camera.distance_metres *= 0.6;
                        camera.pitch_degrees = 1.32_f64.to_degrees();
                        camera.heading_degrees = 0.6_f64.to_degrees();
                        globe.set_camera(&camera)?;
                        globe.set_home_camera(&camera)?;
                        loaded = true;
                    }
                }
            }
            if loaded && last_errors.elapsed() >= Duration::from_millis(500) {
                let state = globe.state()?;
                if let Some(error) = state.errors.values().find(|error| !error.is_empty()) {
                    window.set_status_text(error)?;
                } else {
                    window.set_status_text(&format!(
                        "Sagrada Familia orthophoto + DEM | Elevation 1x | {}",
                        globe.attribution()?
                    ))?;
                }
                last_errors = std::time::Instant::now();
            }
            while globe.poll_event()?.is_some() {}
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
