#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{
        AnalysisMode, AnalysisRequest, AnalysisStatus, DemRequest, GlobeCamera, GlobeOptions,
        GlobePoint, Interaction, JobStatus, MeasurementRequest,
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
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        text: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn MoveWindow(
        window: *mut c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        repaint: i32,
    ) -> i32;
    fn SetWindowTextW(window: *mut c_void, text: *const u16) -> i32;
    fn GetClientRect(window: *mut c_void, rect: *mut Rect) -> i32;
    fn MessageBoxW(window: *mut c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

fn main() {
    if let Err(error) = open_globe() {
        let text: Vec<u16> = error.to_string().encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "GlobeTerrainAnalysis"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

fn result_text(panel: *mut c_void, text: &str) {
    let text: Vec<u16> = text
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        SetWindowTextW(panel, text.as_ptr());
    }
}

#[derive(Default)]
struct Selection {
    mode: Option<usize>,
    points: Vec<GlobePoint>,
    analyzing: bool,
}
impl Selection {
    fn clear(&mut self, globe: &Globe) -> Result<()> {
        self.mode = None;
        self.points.clear();
        self.analyzing = false;
        globe.cancel_terrain_analysis()?;
        globe.clear_terrain_analysis()?;
        globe.clear_measurement()?;
        // ClearInteraction emits selectionFinished; avoid an event feedback loop.
        globe.set_interaction_mode(Interaction::Navigate)?;
        Ok(())
    }
    fn add_point(&mut self, globe: &Globe, point: GlobePoint, panel: *mut c_void) -> Result<()> {
        let Some(mode) = self.mode else {
            return Ok(());
        };
        let required = if mode == 0 || mode == 3 { 2 } else { 1 };
        if self.points.len() == required {
            self.analyzing = false;
            self.points.clear();
            globe.cancel_terrain_analysis()?;
            globe.clear_terrain_analysis()?;
            globe.clear_measurement()?;
        }
        if !self.points.is_empty() {
            globe.set_measurement_points(&MeasurementRequest {
                points: vec![self.points[0].clone(), point.clone()],
                ..Default::default()
            })?;
            let distance = globe
                .measurement()?
                .distance_metres
                .ok_or("Distance unavailable")?;
            if distance < 1.0 || (mode == 0 && distance > 10000.0) {
                globe.set_measurement_points(&MeasurementRequest {
                    points: self.points.clone(),
                    ..Default::default()
                })?;
                result_text(
                    panel,
                    if distance < 1.0 {
                        "Choose a point at least 1 metre away."
                    } else {
                        "Choose a target within 10 km."
                    },
                );
                return Ok(());
            }
        }
        self.points.push(point);
        globe.set_measurement_points(&MeasurementRequest {
            points: self.points.clone(),
            ..Default::default()
        })?;
        if self.points.len() < required {
            result_text(
                panel,
                &format!(
                    "Start/observer: {:.6}, {:.6}. Now click the end/target. Esc cancels.",
                    self.points[0].latitude, self.points[0].longitude
                ),
            );
        } else if mode == 3 {
            result_text(
                panel,
                &format!(
                    "Ground distance: {:.2} m.\nClick to start a new measurement.",
                    globe
                        .measurement()?
                        .distance_metres
                        .ok_or("Distance unavailable")?
                ),
            );
        } else {
            globe.start_terrain_analysis(&AnalysisRequest {
                mode: [
                    AnalysisMode::LineOfSight,
                    AnalysisMode::Viewshed,
                    AnalysisMode::Slope,
                ][mode],
                points: self.points.clone(),
                spacing_metres: Some(15.0),
                radius_metres: Some(350.0),
                observer_height_metres: Some(1.7),
                target_height_metres: Some(1.7),
            })?;
            self.analyzing = true;
            self.show_analysis(globe, panel)?;
        }
        Ok(())
    }
    fn show_analysis(&mut self, globe: &Globe, panel: *mut c_void) -> Result<()> {
        let a = globe.terrain_analysis()?;
        self.analyzing = a.status == AnalysisStatus::Calculating;
        if a.stale {
            result_text(panel, "Terrain changed while calculating. Wait for loading to settle, then click to start a new selection. Previous results are no longer current.");
            return Ok(());
        }
        let mut text = format!("Status: {:?}", a.status);
        if let Some(s) = a.slope.filter(|s| s.available) {
            text += &format!(
                "\nSlope: {:.2}° ({:.2}%)\nAspect: {}",
                s.degrees.ok_or("Missing slope")?,
                s.percent.ok_or("Missing slope percent")?,
                s.aspect_degrees
                    .map(|v| format!("{v:.1}° clockwise from north (downhill)"))
                    .unwrap_or_else(|| "Undefined: flat terrain".into())
            );
        } else if let Some(l) = a.line_of_sight {
            text += &format!("\nLine of sight: {} | Distance: {:.1} m\nSamples: {}/{}; missing: {}\nThe map line marks the ground path, not the elevated sight ray.",
                l.outcome, l.length_metres.unwrap_or_default(), l.processed.unwrap_or_default(), l.total.unwrap_or_default(), l.missing_samples.unwrap_or_default());
        } else if let Some(v) = a.viewshed {
            text += &format!(
                "\nVisible: {}; blocked: {}; unknown: {}\nProcessed: {}/{} | Radius: 350 m",
                v.clear.unwrap_or_default(),
                v.blocked.unwrap_or_default(),
                v.unknown.unwrap_or_default(),
                v.processed.unwrap_or_default(),
                v.total.unwrap_or_default()
            );
        } else {
            text += "\nResult unavailable; neighbouring DEM samples may be missing.";
        }
        if !a.error.is_empty() {
            text += &format!("\n{}", a.error);
        }
        text += "\nTerrain only. Click to start a new selection; Esc cancels.";
        result_text(panel, &text);
        Ok(())
    }
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| -> Result<()> {
        let mut window = unsafe {
            ViewerWindow::new(&runtime, "Globe — Barcelona Terrain Analysis", 1200, 800)?
        };
        let controls = window.add_control_panel(
            &json!({"title":"Terrain analysis", "controls":[
                {"id":1,"type":"button","text":"Line of sight"},
                {"id":2,"type":"button","text":"Viewshed"},
                {"id":3,"type":"button","text":"Slope / aspect"},
                {"id":4,"type":"button","text":"Measure"},
                {"id":5,"type":"button","text":"Clear"},
                {"id":6,"type":"button","text":"Barcelona"}
            ]})
            .to_string(),
        )?;
        for id in 1..=6 {
            window.set_control_enabled(id, false)?;
        }
        window.set_status_text("Loading Barcelona terrain…")?;
        window.show()?;
        window.process_events();
        let parent = window.viewer().get_native_handle()?;
        let class: Vec<u16> = "EDIT".encode_utf16().chain(Some(0)).collect();
        // A read-only, scrollable result panel stays 170 logical pixels high.
        let panel = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0x50200844,
                0,
                0,
                1,
                170,
                parent as *mut c_void,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if panel.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        result_text(
            panel,
            "Choose an analysis tool, then click the terrain. Drag to navigate; Esc cancels.",
        );
        let home = GlobeCamera {
            longitude: 2.1744,
            latitude: 41.4036,
            target_height_metres: 40.0,
            distance_metres: 2500.0,
            pitch_degrees: 0.85_f64.to_degrees(),
            heading_degrees: 0.6_f64.to_degrees(),
        };
        let options = GlobeOptions {
            atmosphere_enabled: true,
            ..Default::default()
        };
        let globe = unsafe { Globe::embedded(&runtime, parent, &options, Some(&home))? };
        let esri = globe
            .basemaps()?
            .into_iter()
            .find(|m| m.name == "Esri Satellite")
            .ok_or("Esri Satellite unavailable")?
            .id;
        globe.set_basemap(esri)?;
        globe.set_visible(true)?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data/sagrada_familia_terrain.tif");
        let mut job = Some(globe.load_dem(&DemRequest {
            id: "dem".into(),
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        })?);
        let mut loaded = false;
        let mut selection = Selection::default();
        let mut size = (0, 0);
        let mut last_analysis = std::time::Instant::now();
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
            if current != size && current.0 > 0 && current.1 > 170 {
                globe.resize(current.0, current.1 - 170)?;
                unsafe {
                    MoveWindow(
                        panel,
                        0,
                        ((current.1 - 170) as f64 * scale).round() as i32,
                        rect.right - rect.left,
                        (170.0 * scale).round() as i32,
                        1,
                    );
                }
                size = current;
            }
            if let Some(id) = job {
                let state = globe.job_state(id)?;
                if !matches!(state.status, JobStatus::Queued | JobStatus::Running) {
                    globe.forget_job(id)?;
                    job = None;
                    if state.status != JobStatus::Completed {
                        result_text(panel, &format!("DEM loading failed: {}", state.error));
                    } else {
                        let stats = globe.layer_state("dem")?.statistics;
                        globe.set_dem_style(
                            false,
                            stats["minimum"].as_f64().ok_or("Missing DEM minimum")?,
                            stats["maximum"].as_f64().ok_or("Missing DEM maximum")?,
                        )?;
                        let mut options = globe.options()?;
                        options.terrain_enabled = true;
                        options.terrain_exaggeration = 1.0;
                        globe.set_options(&options)?;
                        globe.set_camera(&home)?;
                        globe.set_home_camera(&home)?;
                        loaded = true;
                        for id in 1..=6 {
                            window.set_control_enabled(id, true)?;
                        }
                        window.set_status_text(&format!(
                            "Barcelona | Real DEM | Terrain only | {}",
                            globe.attribution()?
                        ))?;
                    }
                }
            }
            let action = (|| -> Result<()> {
                while let Ok(event) = controls.try_recv() {
                    if !loaded {
                        continue;
                    }
                    match event.id {
                        1..=4 => {
                            selection.clear(&globe)?;
                            let mode = (event.id - 1) as usize;
                            selection.mode = Some(mode);
                            globe.set_interaction_mode(Interaction::GroundSelection)?;
                            result_text(panel, [
                                "Line of sight: click observer and target within 10 km. Heights: 1.7 m above ground. Buildings excluded.",
                                "Viewshed: click an observer. Radius: 350 m; sample spacing: 15 m. Buildings excluded.",
                                "Slope / aspect: click a terrain point to see slope and downhill direction.",
                                "Measure: click a start point, then an end point."][mode]);
                        }
                        5 => {
                            selection.clear(&globe)?;
                            result_text(panel, "Selection cleared. Choose an analysis tool.");
                        }
                        6 => globe.set_camera(&home)?,
                        _ => {}
                    }
                }
                while let Some(event) = globe.poll_event()? {
                    if !loaded {
                        continue;
                    }
                    if event.r#type == "selectionFinished" {
                        selection.clear(&globe)?;
                        result_text(panel, "Selection cleared. Choose an analysis tool.");
                    } else if event.r#type == "groundClicked" && selection.mode.is_some() {
                        if event.data["heightAvailable"].as_bool() != Some(true) {
                            result_text(panel, "DEM not loaded here. Wait for terrain or return to Barcelona and try again.");
                        } else {
                            selection.add_point(
                                &globe,
                                GlobePoint {
                                    longitude: event.data["longitude"]
                                        .as_f64()
                                        .ok_or("Missing longitude")?,
                                    latitude: event.data["latitude"]
                                        .as_f64()
                                        .ok_or("Missing latitude")?,
                                },
                                panel,
                            )?;
                        }
                    }
                }
                if selection.analyzing && last_analysis.elapsed() >= Duration::from_millis(100) {
                    selection.show_analysis(&globe, panel)?;
                    last_analysis = std::time::Instant::now();
                }
                Ok(())
            })();
            if let Err(error) = action {
                selection.analyzing = false;
                result_text(panel, &error.to_string());
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
