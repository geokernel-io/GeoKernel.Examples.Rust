#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{
        BuildingColors, BuildingsRequest, GlobeCamera, GlobeOptions, JobStatus, VectorRequest,
        VectorStyle,
    },
    Globe, Runtime, ViewerWindow,
};
use serde_json::json;
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
        let title: Vec<u16> = "GlobeBuildings".encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

fn summary(window: &mut ViewerWindow, globe: &Globe, height: f64) -> Result<()> {
    let layer = globe.layer_state("buildings")?;
    let stats = layer.statistics;
    let message = if layer.extruded {
        format!("{} extruded polygons | Display height {height} m (not measured) | Extrude Off for footprints", stats["extrudedPolygons"])
    } else {
        format!(
            "{} buildings | {} outline-only | Extrude On for 3D",
            stats["features"], stats["outlineOnly"]
        )
    };
    window.set_status_text(&format!("{message} | {}", globe.attribution()?))?;
    Ok(())
}

fn enable_controls(window: &mut ViewerWindow, enabled: bool, extruded: bool) -> Result<()> {
    for id in 1..=5 {
        window.set_control_enabled(id, enabled && (id != 2 || !extruded))?;
    }
    Ok(())
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| -> Result<()> {
        let mut window =
            unsafe { ViewerWindow::new(&runtime, "GlobeBuildings — GeoKernel", 1200, 800)? };
        let controls = window.add_control_panel(&json!({
            "title":"Buildings", "controls":[
                {"id":1,"type":"combo","label":"Extrude","options":["Off","On"],"value":"Off"},
                {"id":2,"type":"number","label":"Display height (m, not measured)","minimum":1,"maximum":300,"decimals":0,"step":1,"value":18},
                {"id":3,"type":"button","label":"","text":"Zoom to buildings"},
                {"id":4,"type":"combo","label":"Atmosphere","options":["Off","On"],"value":"On"},
                {"id":5,"type":"combo","label":"Esri","options":["Off","On"],"value":"On"}
            ]
        }).to_string())?;
        enable_controls(&mut window, false, false)?;
        window.set_status_text("Loading Sagrada Familia buildings…")?;
        window.show()?;
        window.process_events();
        let parent = window.viewer().get_native_handle()?;
        let mut home = GlobeCamera {
            longitude: 2.1744,
            latitude: 41.4036,
            target_height_metres: 0.0,
            distance_metres: 2500.0,
            pitch_degrees: 1.32_f64.to_degrees(),
            heading_degrees: 0.60_f64.to_degrees(),
        };
        let options = GlobeOptions {
            atmosphere_enabled: true,
            ..GlobeOptions::default()
        };
        let globe = unsafe { Globe::embedded(&runtime, parent, &options, Some(&home))? };
        let esri = globe
            .basemaps()?
            .into_iter()
            .find(|item| item.name == "Esri Satellite")
            .ok_or("Esri Satellite basemap is unavailable")?
            .id;
        globe.set_basemap(esri)?;
        globe.set_visible(true)?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/buildings.shp");
        let mut job = Some(globe.load_vector(&VectorRequest {
            id: "buildings".into(),
            path: path.to_string_lossy().into_owned(),
            source_crs: Some("EPSG:25831".into()),
            style: Some(VectorStyle {
                fill: Some(true),
                labels: Some(false),
                base_color: Some("#e8d6b4".into()),
                ..Default::default()
            }),
            ..VectorRequest::default()
        })?);
        let mut loaded = false;
        let mut extruded = false;
        let mut height = 18.0;
        let mut waiting_for_gpu = false;
        let mut size = (0, 0);
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
                        extruded = loaded && globe.layer_state("buildings")?.extruded;
                        window.set_control_value(1, 0.0, if extruded { "On" } else { "Off" })?;
                        while controls.try_recv().is_ok() {}
                        window.set_status_text(&format!("Operation failed: {}", state.error))?;
                        enable_controls(&mut window, loaded, extruded)?;
                    } else {
                        if !loaded {
                            globe.set_layer_opacity("buildings", 0.65)?;
                            globe.focus_layer("buildings")?;
                            home = globe.camera()?;
                            home.distance_metres *= 0.28;
                            home.pitch_degrees = 1.32_f64.to_degrees();
                            home.heading_degrees = 0.60_f64.to_degrees();
                            globe.set_camera(&home)?;
                            globe.set_home_camera(&home)?;
                            loaded = true;
                        }
                        extruded = globe.layer_state("buildings")?.extruded;
                        if extruded {
                            waiting_for_gpu = true;
                            window.set_status_text("Uploading 3D buildings to the GPU…")?;
                        } else {
                            enable_controls(&mut window, true, false)?;
                            summary(&mut window, &globe, height)?;
                        }
                    }
                }
            }
            if waiting_for_gpu {
                // Geometry completion precedes the incremental GPU upload.
                let render = globe.state()?;
                if let Some(error) = render.errors.get("scene").filter(|error| !error.is_empty()) {
                    waiting_for_gpu = false;
                    window.set_status_text(error)?;
                    enable_controls(&mut window, true, extruded)?;
                } else if !render.scene_upload_pending && render.scene_vertices_drawn > 0 {
                    waiting_for_gpu = false;
                    enable_controls(&mut window, true, extruded)?;
                    summary(&mut window, &globe, height)?;
                }
            }
            while let Ok(event) = controls.try_recv() {
                if !loaded || job.is_some() || waiting_for_gpu {
                    continue;
                }
                let result = (|| -> Result<()> {
                    match event.id {
                        1 => {
                            job = Some(globe.configure_buildings(&BuildingsRequest {
                                id: "buildings".into(),
                                enabled: Some(event.text == "On"),
                                default_height: Some(height),
                                base_mode: Some("referenceSphere".into()),
                                building_colors: Some(BuildingColors {
                                    mode: "uniform".into(),
                                    fallback: "#e8d6b4".into(),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            })?);
                            enable_controls(&mut window, false, extruded)?;
                            window.set_status_text(if event.text == "On" {
                                "Extruding buildings…"
                            } else {
                                "Restoring footprints…"
                            })?;
                        }
                        2 if !extruded => {
                            height = event.number.clamp(1.0, 300.0);
                            summary(&mut window, &globe, height)?;
                        }
                        3 => globe.set_camera(&home)?,
                        4 => {
                            let mut options = globe.options()?;
                            options.atmosphere_enabled = event.text == "On";
                            globe.set_options(&options)?;
                        }
                        5 => {
                            globe.set_basemap(if event.text == "On" { esri } else { -1 })?;
                            summary(&mut window, &globe, height)?;
                        }
                        _ => {}
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    window.set_status_text(&error.to_string())?;
                }
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
