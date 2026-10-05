mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn display(window: &mut ViewerWindow, value: &Value) -> Result<()> {
    let points = value["points"].as_array().ok_or("Missing measurement points")?;
    let area = value["areaMode"].as_bool().unwrap_or(false);
    let minimum = if area { 3 } else { 2 };
    let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
    let mut text = format!("<b>{} points</b><br>", points.len());
    if points.len() < minimum { text += &format!("Add at least {minimum} points.<br>"); }
    if value["validArea"] == true { text += &format!("Plan area: {:.2} m²<br>", number("planArea")); }
    text += &format!("Horizontal {}: {:.2} m<br>3D segment total: {:.2} m<br>",
        if area { "perimeter" } else { "length" }, number("horizontal"), number("length3D"));
    if !area && points.len() >= 2 { text += &format!("End-to-start ΔUp: {:.2} m<br>", number("deltaUp")); }
    text += &escape(value["message"].as_str().unwrap_or(""));
    text += "<table><tr><th>East (m)</th><th>North (m)</th><th>Up (m)</th></tr>";
    for point in points {
        text += &format!("<tr><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td></tr>",
            point["east"].as_f64().unwrap_or(0.0), point["north"].as_f64().unwrap_or(0.0),
            point["up"].as_f64().unwrap_or(0.0));
    }
    text += "</table>";
    window.clear_log()?;
    window.append_log(&text)?;
    Ok(())
}

fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool, value: &Value) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, loading)?;
    for id in 3..=7 { window.set_control_enabled(id, ready && !loading)?; }
    let count = value["points"].as_array().map_or(0, Vec::len);
    let area = value["areaMode"] == true;
    let valid = count >= if area { 3 } else { 2 };
    window.set_control_enabled(8, ready && !loading && value["capture"] == true && valid &&
        (!area || value["validArea"] == true))?;
    for id in 9..=10 { window.set_control_enabled(id, ready && !loading && count > 0)?; }
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "Measurement3D — GeoKernel", 1300, 850)? };
    let controls = window.add_control_panel(&json!({
        "title":"Measurement",
        "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"combo","label":"Measurement mode","options":["Distance / polyline","Area / polygon"],"value":"Distance / polyline"},
            {"id":5,"type":"combo","label":"Add points on click","options":["Enabled","Disabled"],"value":"Enabled"},
            {"id":6,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":7,"type":"button","text":"Reset view"},
            {"id":8,"type":"button","text":"Finish measurement"},
            {"id":9,"type":"button","text":"Undo last point"},
            {"id":10,"type":"button","text":"Clear measurement"}
        ]
    }).to_string())?;
    window.add_log_panel("Measurement results and coordinates")?;
    window.append_log("Enable Add points and click terrain. Dragging navigates. Area closes after three points.<br>Local ENU metres; straight 3D segments, not terrain-following length. Area is horizontal plan area. Exaggeration does not change measurements. DEM datum is unverified.")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    // Drops before the owning window and Qt runtime.
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let dem = root.join("sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let image = root.join("sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif").canonicalize()?;
    let mut area = false;
    let mut capture = true;
    let mut imagery = 1;
    let mut height = 1.0;
    let mut ready = false;
    let mut loading = true;
    let mut snapshot = Value::Null;
    let mut next_poll = Instant::now();
    terrain.load(&dem, &image, 512)?;
    window.set_status_text("Loading terrain…")?;
    controls_state(&mut window, ready, loading, &snapshot)?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        terrain.resize()?;
        while let Ok(event) = controls.try_recv() {
            let result: Result<()> = (|| {
                match event.id {
                    1 if !loading => {
                        if ready { terrain.mode(area, false)?; }
                        if let Err(error) = terrain.load(&dem, &image, 512) {
                            if ready { terrain.mode(area, capture)?; }
                            return Err(error);
                        }
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    2 if loading => terrain.cancel(),
                    _ if !ready || loading => {}
                    3 => { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; }
                    4 => { area = event.text == "Area / polygon"; terrain.mode(area, capture)?; }
                    5 => { capture = event.text == "Enabled"; terrain.mode(area, capture)?; }
                    6 => { height = event.number as f32; terrain.height(height)?; }
                    7 => terrain.reset()?,
                    8 => {
                        capture = false;
                        terrain.mode(area, capture)?;
                        window.set_control_value(5, 0.0, "Disabled")?;
                    }
                    9 => terrain.edit(true)?,
                    10 => terrain.edit(false)?,
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
                    ready = true;
                    snapshot = Value::Null;
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    window.set_status_text("Ready — click terrain to measure.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
            // A cancelled or failed replacement retains the prior scene and points.
            if !loading && ready { terrain.mode(area, capture)?; }
        }
        if ready && !loading && Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(150);
            let current = terrain.measurement()?;
            if current != snapshot {
                display(&mut window, &current)?;
                snapshot = current;
            }
        }
        controls_state(&mut window, ready, loading, &snapshot)?;
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
