#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{GlobeCamera, GlobeOptions, JobStatus, ModelPlacement, ModelRequest},
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
        let title: Vec<u16> = "GlobeModelPlacement"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

const LAYER: &str = "sagrada-familia";

fn camera(model: &ModelRequest) -> GlobeCamera {
    let scale = model.scale.unwrap_or(1.0);
    GlobeCamera {
        longitude: model.longitude,
        latitude: model.latitude,
        target_height_metres: model.height.unwrap_or(0.0) + 65.0 * scale,
        distance_metres: 550.0 * scale,
        pitch_degrees: 1.15_f64.to_degrees(),
        heading_degrees: 0.6_f64.to_degrees(),
    }
}

fn summary(window: &mut ViewerWindow, globe: &Globe, present: bool) -> Result<()> {
    let message = if present {
        "Approximate placement"
    } else {
        "Model removed - click Restore"
    };
    window.set_status_text(&format!(
        "Sagrada Familia | {message} | Model: PeeJaa | {}",
        globe.attribution()?
    ))?;
    Ok(())
}

fn enable_controls(window: &mut ViewerWindow, enabled: bool, present: bool) -> Result<()> {
    for id in 1..=8 {
        window.set_control_enabled(
            id,
            enabled
                && match id {
                    2..=5 => present,
                    6 => !present,
                    _ => true,
                },
        )?;
    }
    Ok(())
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| -> Result<()> {
        let mut window =
            unsafe { ViewerWindow::new(&runtime, "GlobeModelPlacement — GeoKernel", 1200, 800)? };
        let controls = window.add_control_panel(&json!({
            "title":"Model placement", "controls":[
                {"id":1,"type":"button","label":"","text":"Zoom to model"},
                {"id":2,"type":"button","label":"","text":"Rotate 30 degrees"},
                {"id":3,"type":"number","label":"Scale","minimum":0.01,"maximum":10,"decimals":2,"step":0.1,"value":1},
                {"id":4,"type":"number","label":"Anchor height (m)","minimum":-100,"maximum":10000,"decimals":3,"step":1,"value":14.089},
                {"id":5,"type":"button","label":"","text":"Remove"},
                {"id":6,"type":"button","label":"","text":"Restore"},
                {"id":7,"type":"combo","label":"Atmosphere","options":["Off","On"],"value":"On"},
                {"id":8,"type":"combo","label":"Esri","options":["Off","On"],"value":"On"}
            ]
        }).to_string())?;
        enable_controls(&mut window, false, false)?;
        window.set_status_text("Loading Sagrada Familia model…")?;
        window.show()?;
        window.process_events();
        let parent = window.viewer().get_native_handle()?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/model.gltf");
        let mut model = ModelRequest {
            id: LAYER.into(),
            path: path.to_string_lossy().into_owned(),
            longitude: 2.1744,
            latitude: 41.4036,
            height: Some(14.089),
            scale: Some(1.0),
            heading: Some(0.0),
            height_mode: Some("ellipsoidal".into()),
            name: Some("Sagrada Familia".into()),
            ..Default::default()
        };
        let home = camera(&model);
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
        let mut job = Some(globe.load_model(&model)?);
        let mut pending = model.clone();
        let mut present = false;
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
                        window.set_control_value(3, model.scale.unwrap_or(1.0), "")?;
                        window.set_control_value(4, model.height.unwrap_or(0.0), "")?;
                        while controls.try_recv().is_ok() {}
                        window.set_status_text(&format!("Operation failed: {}", state.error))?;
                        enable_controls(&mut window, true, present)?;
                    } else {
                        model = pending.clone();
                        present = true;
                        globe.set_camera(&camera(&model))?;
                        globe.set_home_camera(&camera(&model))?;
                        waiting_for_gpu = true;
                        window.set_status_text("Uploading model to the GPU…")?;
                    }
                }
            }
            if waiting_for_gpu {
                // Geometry completion precedes the incremental GPU upload.
                let render = globe.state()?;
                if let Some(error) = render.errors.get("scene").filter(|error| !error.is_empty()) {
                    waiting_for_gpu = false;
                    window.set_status_text(error)?;
                    enable_controls(&mut window, true, present)?;
                } else if !render.scene_upload_pending && render.scene_vertices_drawn > 0 {
                    waiting_for_gpu = false;
                    enable_controls(&mut window, true, present)?;
                    summary(&mut window, &globe, present)?;
                }
            }
            while let Ok(event) = controls.try_recv() {
                if job.is_some() || waiting_for_gpu {
                    continue;
                }
                let result = (|| -> Result<()> {
                    let mut next = model.clone();
                    match event.id {
                        1 => {
                            globe.set_camera(&camera(&model))?;
                            return Ok(());
                        }
                        2 if present => {
                            next.heading = Some((model.heading.unwrap_or(0.0) + 30.0) % 360.0)
                        }
                        3 if present => next.scale = Some(event.number.clamp(0.01, 10.0)),
                        4 if present => next.height = Some(event.number.clamp(-100.0, 10000.0)),
                        5 if present => {
                            globe.remove_layer(LAYER)?;
                            present = false;
                            enable_controls(&mut window, true, present)?;
                            summary(&mut window, &globe, present)?;
                            return Ok(());
                        }
                        6 if !present => {}
                        7 => {
                            let mut options = globe.options()?;
                            options.atmosphere_enabled = event.text == "On";
                            globe.set_options(&options)?;
                            return Ok(());
                        }
                        8 => {
                            globe.set_basemap(if event.text == "On" { esri } else { -1 })?;
                            summary(&mut window, &globe, present)?;
                            return Ok(());
                        }
                        _ => return Ok(()),
                    }
                    job = Some(if present {
                        globe.set_model_placement(&ModelPlacement {
                            id: LAYER.into(),
                            height: next.height,
                            heading: next.heading,
                            scale: next.scale,
                            ..Default::default()
                        })?
                    } else {
                        globe.load_model(&next)?
                    });
                    pending = next;
                    enable_controls(&mut window, false, present)?;
                    window.set_status_text("Updating Sagrada Familia model…")?;
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
