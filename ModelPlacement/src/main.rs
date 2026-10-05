mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::json;
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn run(runtime: &Runtime) -> Result<()> {
    let mut window =
        unsafe { ViewerWindow::new(runtime, "ModelPlacement — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title": "Terrain",
        "controls": [
            {"id":1,"type":"combo","label":"Terrain mesh","options":["256 — Fast","512 — Balanced","1024 — Detailed"],"value":"512 — Balanced"},
            {"id": 10, "type": "number", "label": "Longitude", "minimum": -180, "maximum": 180, "step": 1, "decimals": 9, "value": 2.174401283},
            {"id": 11, "type": "number", "label": "Latitude", "minimum": -90, "maximum": 90, "step": 1, "decimals": 9, "value": 41.403605046},
            {"id": 12, "type": "number", "label": "Above terrain (m)", "minimum": -500, "maximum": 1000, "step": 1, "decimals": 3, "value": 2.938},
            {"id": 13, "type": "number", "label": "Heading (degrees)", "minimum": -180, "maximum": 180, "step": 1, "decimals": 7, "value": 0.2373346},
            {"id": 14, "type": "number", "label": "Pitch (degrees)", "minimum": -180, "maximum": 180, "step": 1, "decimals": 2, "value": 0},
            {"id": 15, "type": "number", "label": "Roll (degrees)", "minimum": -180, "maximum": 180, "step": 1, "decimals": 2, "value": 0},
            {"id": 16, "type": "number", "label": "Scale", "minimum": 0.01, "maximum": 100, "step": 1, "decimals": 6, "value": 0.958139},
            {"id":2,"type":"button","text":"Apply placement / Reload"},
            {"id":3,"type":"button","text":"Cancel loading"},
            {"id":4,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":5,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":6,"type":"button","text":"Reset view"},
            {"id":7,"type":"combo","label":"Model","options":["Visible","Hidden"],"value":"Visible"},
            {"id":8,"type":"button","text":"Focus model"}
        ]
    }).to_string())?;
    window.add_log_panel("Terrain information")?;
    window.append_log("Sagrada Familia - textured model<br>Approximate demo placement; not a surveyed fit. Height is an offset above terrain at the model anchor. EGM08D595 correction is applied. Textures are retained.<br>Model: PeeJaa - La Sagrada Familia. Source and license: downloaded model/license.txt.<br>Left drag: pan | Wheel/right: zoom | Middle/Ctrl+left: orbit")?;
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
    let model = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_3d_model/model.gltf")
        .canonicalize()?;
    let geoid = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/egm08d595/EGM08D595/cat80000.gr")
        .canonicalize()?;
    let mut visible = true;
    let mut placement = [
        2.174401283,
        41.403605046,
        2.938,
        0.2373346,
        0.0,
        0.0,
        0.958139,
    ];
    let mut resolution = 512;
    let mut imagery = 1;
    let mut height = 1.0;
    terrain.load(&data, &photo, &model, &geoid, &placement, resolution)?;
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
                    match terrain.load(&data, &photo, &model, &geoid, &placement, resolution) {
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
                    visible = event.text != "Hidden";
                    terrain.model_visible(visible)?;
                }
                8 => terrain.focus_model()?,
                10..=16 => {
                    placement[(event.id - 10) as usize] = event.number;
                }
                _ => {}
            }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    terrain.model_visible(visible)?;
                    terrain.focus_model()?;
                    window.set_status_text(&format!(
                        "Model loaded - {} triangles; EGM08D595 corrected terrain",
                        terrain.model_vertex_count()? / 3
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
