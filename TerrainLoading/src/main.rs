mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::json;
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn run(runtime: &Runtime) -> Result<()> {
    let mut window =
        unsafe { ViewerWindow::new(runtime, "TerrainLoading — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title": "Terrain",
        "controls": [
            {"id":1,"type":"combo","label":"Terrain mesh","options":["256 — Fast","512 — Balanced","1024 — Detailed"],"value":"512 — Balanced"},
            {"id":2,"type":"button","text":"Reload terrain"},
            {"id":3,"type":"button","text":"Cancel loading"},
            {"id":4,"type":"combo","label":"Color scale","options":["Terrain","Grayscale","Viridis","Inferno","Ocean"],"value":"Terrain"},
            {"id":5,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":6,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Terrain information")?;
    window.append_log("Sagrada Família — DEM<br>Colors represent relative terrain relief.<br>Heights are treated as metres; the vertical datum is unverified.<br><br>Left drag: pan<br>Wheel / right drag: zoom<br>Middle / Ctrl + left drag: orbit")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    // This adapter is dropped before the owning window and Qt runtime.
    let mut terrain = unsafe { Terrain::new(parent, &bin.join("GeoKernel.Viewer3D.dll"))? };
    terrain.resize()?;
    terrain.ramp(1)?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_terrain/sagrada_familia_terrain.tif")
        .canonicalize()?;
    let mut resolution = 512;
    let mut ramp = 1;
    let mut height = 1.0;
    terrain.load(&data, resolution)?;
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
                2 if !loading => match terrain.load(&data, resolution) {
                    Ok(()) => {
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    Err(error) => window.set_status_text(&error.to_string())?,
                },
                3 if loading => terrain.cancel(),
                4 => {
                    ramp = match event.text.as_str() {
                        "Grayscale" => 2,
                        "Viridis" => 3,
                        "Inferno" => 4,
                        "Ocean" => 5,
                        _ => 1,
                    };
                    terrain.ramp(ramp)?;
                }
                5 => {
                    height = event.number as f32;
                    terrain.height(height)?;
                }
                6 => terrain.reset()?,
                _ => {}
            }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    terrain.ramp(ramp)?;
                    terrain.height(height)?;
                    window.set_status_text("Terrain loaded — sagrada_familia_terrain.tif")?;
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
