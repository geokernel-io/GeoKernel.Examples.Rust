mod bootstrap;
mod navigation;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::json;
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn run(runtime: &Runtime) -> Result<()> {
    let mut window =
        unsafe { ViewerWindow::new(runtime, "CameraNavigation — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title": "Terrain",
        "controls": [
            {"id":1,"type":"combo","label":"Terrain mesh","options":["256 — Fast","512 — Balanced","1024 — Detailed"],"value":"512 — Balanced"},
            {"id":2,"type":"button","text":"Reload terrain"},
            {"id":3,"type":"button","text":"Cancel loading"},
            {"id":4,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":5,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":6,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Terrain information")?;
    window.append_log("Sagrada Familia - terrain and orthophoto<br>Heights are treated as metres; the vertical datum is unverified.<br><br>Left drag: pan<br>Wheel / right drag: zoom<br>Middle / Ctrl + left drag: orbit")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    // Use the local imagery API until it is published. The remaining runtime
    // dependencies still come from the downloaded SDK.
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../GeoKernel/outputs/build/Release/GeoKernel.Viewer3D.dll");
    let library = std::env::var_os("GEOKERNEL_VIEWER3D_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if local.is_file() {
                local
            } else {
                bin.join("GeoKernel.Viewer3D.dll")
            }
        });
    // This adapter is dropped before the owning window and Qt runtime.
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    terrain.imagery(1)?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_terrain/sagrada_familia_terrain.tif")
        .canonicalize()?;
    let photo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif")
        .canonicalize()?;
    let mut navigation = navigation::Navigation::new()?;
    let camera_controls = window.add_control_panel(&json!({
        "title":"Camera navigation", "controls":[
            {"id":10,"type":"combo","label":"Preset","options":["Overview","Top view","East view","West view","Close oblique"],"value":"Overview"},
            {"id":11,"type":"button","text":"Go to preset"},
            {"id":12,"type":"combo","label":"Transition","options":["Smooth (1.2 seconds)","Instant"],"value":"Smooth (1.2 seconds)"},
            {"id":13,"type":"button","text":"Stop transition"},
            {"id":14,"type":"button","text":"Zoom in"},
            {"id":15,"type":"button","text":"Zoom out"},
            {"id":16,"type":"button","text":"North up"},
            {"id":17,"type":"text","label":"Saved view name","placeholder":"Name to save, restore or delete"},
            {"id":18,"type":"button","text":"Save current view"},
            {"id":19,"type":"button","text":"Restore named view"},
            {"id":20,"type":"button","text":"Delete named view"}
        ]
    }).to_string())?;
    window.append_log(&navigation.list_html())?;
    let mut resolution = 512;
    let mut imagery = 1;
    let mut height = 1.0;
    terrain.load(&data, &photo, resolution)?;
    let mut loading = true;
    window.set_status_text("Loading terrain…")?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? {
            break;
        }
        terrain.resize()?;
        while let Ok(event) = controls.try_recv() {
            match event.id {
                1 => {
                    resolution = match event.text.as_str() {
                        "256 — Fast" => 256,
                        "1024 — Detailed" => 1024,
                        _ => 512,
                    }
                }
                2 if !loading => match terrain.load(&data, &photo, resolution) {
                    Ok(()) => {
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    Err(error) => window.set_status_text(&error.to_string())?,
                },
                3 if loading => terrain.cancel(),
                4 => {
                    imagery = if event.text == "Hidden" { 0 } else { 1 };
                    terrain.imagery(imagery)?;
                }
                5 => {
                    height = event.number as f32;
                    terrain.height(height)?;
                }
                6 => terrain.reset()?,
                _ => {}
            }
        }
        while let Ok(event) = camera_controls.try_recv() {
            match event.id {
                10 => navigation.preset = event.text,
                12 => {
                    terrain.stop_camera()?;
                    navigation.smooth = event.text != "Instant";
                }
                17 => navigation.name = event.text,
                _ if !loading && navigation.home.is_some() => {
                    match navigation.action(&terrain, event.id) {
                        Ok(()) => {
                            if event.id == 18 || event.id == 20 {
                                window.append_log(&navigation.list_html())?;
                            }
                        }
                        Err(error) => window.set_status_text(&error.to_string())?,
                    }
                }
                _ => {}
            }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    navigation.home = Some(terrain.camera()?);
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    window.set_status_text("Terrain and orthophoto loaded")?;
                }
                Ok(-2) => {
                    loading = false;
                    window.set_status_text("Loading cancelled.")?;
                }
                Ok(_) => {
                    loading = false;
                    window.set_status_text("Terrain load ended without a scene.")?;
                }
                Err(error) => {
                    loading = false;
                    window.set_status_text(&error.to_string())?;
                }
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

fn main() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let result = run(&runtime);
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    result?;
    shutdown?;
    Ok(())
}
