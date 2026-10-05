mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::json;
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "Buildings3D — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title": "Terrain",
        "controls": [
            {"id":1,"type":"combo","label":"Terrain mesh","options":["256 — Fast","512 — Balanced","1024 — Detailed"],"value":"512 — Balanced"},
            {"id":10,"type":"number","label":"Default building height (m)","minimum":1,"maximum":300,"step":1,"decimals":1,"value":9},
            {"id":2,"type":"button","text":"Apply height / Reload sample"},
            {"id":3,"type":"button","text":"Cancel loading"},
            {"id":4,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":5,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":6,"type":"button","text":"Reset view"},
            {"id":7,"type":"combo","label":"Buildings","options":["Visible","Hidden"],"value":"Visible"},
            {"id":8,"type":"color","label":"Building color","value":"#d9c4a5"},
            {"id":9,"type":"number","label":"Building opacity (%)","minimum":0,"maximum":100,"step":1,"decimals":0,"value":100}
        ]
    }).to_string())?;
    window.add_log_panel("Terrain information")?;
    window.append_log("Sagrada Familia - terrain, orthophoto and buildings<br>Building heights are illustrative, not measured. Change the default height and select Apply height / Reload sample.<br>DEM heights are treated as metres; the vertical datum is unverified.<br><br>Left drag: pan<br>Wheel / right drag: zoom<br>Middle / Ctrl + left drag: orbit")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
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
    let buildings = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_buildings/buildings.shp")
        .canonicalize()?;
    let mut buildings_visible = true;
    let mut building_color = String::from("#d9c4a5");
    let mut building_opacity = 1.0;
    let mut buildings_ready = false;
    let mut building_height = 9.0;
    let mut resolution = 512;
    let mut imagery = 1;
    let mut height = 1.0;
    terrain.load(&data, &photo, &buildings, building_height, resolution)?;
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
                2 if !loading => {
                    match terrain.load(&data, &photo, &buildings, building_height, resolution) {
                        Ok(()) => {
                            loading = true;
                            window.set_status_text("Loading terrain…")?;
                        }
                        Err(error) => window.set_status_text(&error.to_string())?,
                    }
                }
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
                7 => {
                    buildings_visible = event.text != "Hidden";
                }
                8 => {
                    building_color = event.text.clone();
                }
                9 => {
                    building_opacity = (event.number / 100.0) as f32;
                }
                10 => {
                    building_height = event.number as f32;
                }
                _ => {}
            }
            if buildings_ready && matches!(event.id, 7..=9) {
                if let Err(error) =
                    terrain.building_style(buildings_visible, building_opacity, &building_color)
                {
                    window.set_status_text(&error.to_string())?;
                }
            }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    buildings_ready = true;
                    terrain.building_style(buildings_visible, building_opacity, &building_color)?;
                    window.set_status_text(&format!(
                        "Terrain, orthophoto and buildings loaded — {} building vertices",
                        terrain.building_vertex_count()?
                    ))?;
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
