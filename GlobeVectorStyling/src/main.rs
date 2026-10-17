#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{GlobeCamera, GlobeOptions, Interaction, JobStatus, VectorRequest, VectorStyle},
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
        let title: Vec<u16> = "GlobeVectorStyling".encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

fn summary(window: &mut ViewerWindow, globe: &Globe) -> Result<()> {
    let stats = globe.layer_state("buildings")?.statistics;
    window.set_status_text(&format!(
        "{} buildings | {} outline-only | Click to identify | {}",
        stats["features"],
        stats["outlineOnly"],
        globe.attribution()?
    ))?;
    Ok(())
}

fn enable_controls(window: &mut ViewerWindow, enabled: bool) -> Result<()> {
    for id in 1..=8 {
        window.set_control_enabled(id, enabled)?;
    }
    Ok(())
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| -> Result<()> {
        let mut window =
            unsafe { ViewerWindow::new(&runtime, "GlobeVectorStyling — GeoKernel", 1200, 800)? };
        let controls = window.add_control_panel(&json!({
            "title":"Building styles", "controls":[
                {"id":1,"type":"combo","label":"Fill","options":["Off","On"],"value":"On"},
                {"id":2,"type":"combo","label":"Labels","options":["Off","On"],"value":"Off"},
                {"id":3,"type":"combo","label":"Color by","options":["Uniform"]},
                {"id":4,"type":"combo","label":"Label field","options":[]},
                {"id":5,"type":"number","label":"Opacity (%)","minimum":15,"maximum":100,"decimals":0,"step":5,"value":60},
                {"id":6,"type":"color","label":"Base color","value":"#efb84c"},
                {"id":7,"type":"button","label":"","text":"Zoom to buildings"},
                {"id":8,"type":"combo","label":"Esri","options":["Off","On"],"value":"On"}
            ]
        }).to_string())?;
        enable_controls(&mut window, false)?;
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
        let mut style = VectorStyle {
            fill: Some(true),
            labels: Some(false),
            category_field: Some("type".into()),
            label_field: Some("name".into()),
            base_color: Some("#efb84c".into()),
            label_size: Some(12.0),
            ..VectorStyle::default()
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/buildings.shp");
        let mut job = Some(globe.load_vector(&VectorRequest {
            id: "buildings".into(),
            path: path.to_string_lossy().into_owned(),
            source_crs: Some("EPSG:25831".into()),
            style: Some(style.clone()),
            ..VectorRequest::default()
        })?);
        let mut loaded = false;
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
                        window.set_status_text(&format!("Operation failed: {}", state.error))?;
                        enable_controls(&mut window, loaded)?;
                    } else {
                        if !loaded {
                            let stats = globe.layer_state("buildings")?.statistics;
                            let fields: Vec<String> =
                                serde_json::from_value(stats["fields"].clone())?;
                            if !fields.iter().any(|s| s == "type")
                                || !fields.iter().any(|s| s == "name")
                            {
                                return Err(
                                    "Expected building fields 'type' and 'name' are missing".into(),
                                );
                            }
                            let mut categories = vec!["Uniform".to_string()];
                            categories.extend(fields.iter().cloned());
                            window.set_control_options_json(3, &json!(categories).to_string())?;
                            window.set_control_options_json(4, &json!(fields).to_string())?;
                            window.set_control_value(3, 0.0, "type")?;
                            window.set_control_value(4, 0.0, "name")?;
                            // Discard initialization callbacks before accepting user changes.
                            while controls.try_recv().is_ok() {}
                            globe.set_layer_opacity("buildings", 0.6)?;
                            globe.focus_layer("buildings")?;
                            home = globe.camera()?;
                            home.distance_metres *= 0.28;
                            home.pitch_degrees = 1.32_f64.to_degrees();
                            home.heading_degrees = 0.60_f64.to_degrees();
                            globe.set_camera(&home)?;
                            globe.set_home_camera(&home)?;
                            globe.set_interaction_mode(Interaction::Identify)?;
                            loaded = true;
                        }
                        enable_controls(&mut window, true)?;
                        summary(&mut window, &globe)?;
                    }
                }
            }
            while let Ok(event) = controls.try_recv() {
                if !loaded || job.is_some() {
                    continue;
                }
                let result = (|| -> Result<()> {
                    match event.id {
                        1 => style.fill = Some(event.text == "On"),
                        2 => style.labels = Some(event.text == "On"),
                        3 => {
                            style.category_field = Some(if event.text == "Uniform" {
                                String::new()
                            } else {
                                event.text
                            })
                        }
                        4 => style.label_field = Some(event.text),
                        5 => {
                            globe.set_layer_opacity("buildings", event.number / 100.0)?;
                            return Ok(());
                        }
                        6 => style.base_color = Some(event.text),
                        7 => {
                            globe.set_camera(&home)?;
                            return Ok(());
                        }
                        8 => {
                            globe.set_basemap(if event.text == "On" { esri } else { -1 })?;
                            summary(&mut window, &globe)?;
                            return Ok(());
                        }
                        _ => return Ok(()),
                    }
                    job = Some(globe.set_vector_style("buildings", &style)?);
                    enable_controls(&mut window, false)?;
                    window.set_status_text("Applying building style…")?;
                    Ok(())
                })();
                if let Err(error) = result {
                    window.set_status_text(&error.to_string())?;
                }
            }
            while let Some(event) = globe.poll_event()? {
                if event.r#type == "featurePicked" && event.data["hit"] == true {
                    let id = event.data["featureId"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| event.data["featureId"].to_string());
                    match globe.feature_info("buildings", &id) {
                        Ok(info) => {
                            let attributes: Vec<_> = info
                                .attributes
                                .iter()
                                .map(|(key, value)| format!("{key}: {value}"))
                                .collect();
                            window.set_status_text(&format!(
                                "Building {id}: {}",
                                attributes.join(" | ")
                            ))?;
                        }
                        Err(error) => {
                            window.set_status_text(&error.to_string())?;
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
