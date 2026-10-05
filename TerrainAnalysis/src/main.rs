mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::json;
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn legend(window: &mut ViewerWindow, mode: i32, loaded: Option<i32>) -> Result<()> {
    let rows: &[(&str, &str)] = match mode {
        1 => &[("#2c7bb6", "0 to &lt;5°"), ("#abd9e9", "5 to &lt;15°"),
               ("#ffffbf", "15 to &lt;30°"), ("#fdae61", "30 to &lt;45°"), ("#d7191c", "45 to 90°")],
        2 => &[("#e6194b", "N — 0°"), ("#f58230", "NE — 45°"), ("#ffe119", "E — 90°"),
               ("#3cb44b", "SE — 135°"), ("#46f0f0", "S — 180°"), ("#0082c8", "SW — 225°"),
               ("#911eb4", "W — 270°"), ("#f032e6", "NW — 315°"), ("#808080", "Flat (slope &lt;0.01°)")],
        _ => &[("#185b3f", "Low"), ("#68984c", "↓"), ("#cfbe7e", "↓"),
               ("#926b4c", "↓"), ("#fafafa", "High")],
    };
    let mut html = String::from("<table cellspacing='5'>");
    for (color, text) in rows {
        html.push_str(&format!("<tr><td bgcolor='{color}' width='24'>&nbsp;</td><td>{text}</td></tr>"));
    }
    html.push_str("</table>");
    if mode == 2 { html.push_str("Each direction spans ±22.5°.<br>"); }
    if mode == 0 { html.push_str("Relative ENU relief, with lighting.<br>"); }
    html.push_str("Slope/aspect describe loaded mesh faces before vertical exaggeration, not an exported analysis raster. Mesh resolution affects results. Aspect is clockwise from north in local ENU.<br>Heights: metres, unverified vertical reference.<br>");
    if let Some(resolution) = loaded {
        html.push_str(&format!("DEM: sagrada_familia_terrain.tif<br>Mesh: up to {resolution} samples per side.<br>"));
    } else { html.push_str("No terrain loaded.<br>"); }
    html.push_str("Left drag: pan | Wheel/right: zoom | Middle/Ctrl+left: orbit | Shift+left: look");
    window.clear_log()?;
    window.append_log(&html)?;
    Ok(())
}

fn busy(window: &mut ViewerWindow, loading: bool) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, !loading)?;
    window.set_control_enabled(3, loading)?;
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "TerrainAnalysis — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title":"Sagrada Família — Terrain analysis",
        "controls":[
            {"id":1,"type":"combo","label":"Mesh samples per side","options":["256 — Fast","512 — Balanced","1024 — Detailed"],"value":"512 — Balanced"},
            {"id":2,"type":"button","text":"Reload sample"},
            {"id":3,"type":"button","text":"Cancel loading"},
            {"id":4,"type":"combo","label":"Display","options":["Terrain relief","Slope (degrees)","Aspect (downhill direction)"],"value":"Slope (degrees)"},
            {"id":5,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":6,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Legend and terrain information")?;
    legend(&mut window, 1, None)?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    // Drops before the parent window and the Qt runtime.
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let mut resolution = 512;
    let mut pending_resolution = resolution;
    let mut loaded = None;
    let mut mode = 1;
    let mut height = 1.0;
    terrain.ramp(1)?;
    terrain.mode(mode)?;
    terrain.load(&data, resolution)?;
    let mut loading = true;
    busy(&mut window, loading)?;
    window.set_status_text("Loading terrain…")?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        terrain.resize()?;
        while let Ok(event) = controls.try_recv() {
            let result: Result<()> = (|| {
                match event.id {
                    1 if !loading => {
                        resolution = match event.text.as_str() {
                            "256 — Fast" => 256,
                            "1024 — Detailed" => 1024,
                            _ => 512,
                        };
                    }
                    2 if !loading => {
                        terrain.load(&data, resolution)?;
                        pending_resolution = resolution;
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    3 if loading => terrain.cancel(),
                    4 => {
                        let requested = match event.text.as_str() {
                            "Terrain relief" => 0,
                            "Aspect (downhill direction)" => 2,
                            _ => 1,
                        };
                        terrain.mode(requested)?;
                        mode = requested;
                        legend(&mut window, mode, loaded)?;
                    }
                    5 => {
                        let requested = event.number as f32;
                        terrain.height(requested)?;
                        height = requested;
                    }
                    6 => terrain.reset()?,
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result { window.set_status_text(&error.to_string())?; }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    loaded = Some(pending_resolution);
                    terrain.ramp(1)?;
                    terrain.mode(mode)?;
                    terrain.height(height)?;
                    legend(&mut window, mode, loaded)?;
                    window.set_status_text("Terrain loaded.")?;
                }
                Ok(-2) => {
                    loading = false;
                    window.set_status_text("Loading cancelled. Previous scene retained.")?;
                }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a terrain scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
        }
        busy(&mut window, loading)?;
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
