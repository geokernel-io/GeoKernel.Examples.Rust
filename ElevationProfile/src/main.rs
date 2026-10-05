mod bootstrap;
mod chart;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn display(window: &mut ViewerWindow, value: &Value, chart: &mut chart::Chart) -> Result<()> {
    let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
    let samples = value["samples"].as_array().ok_or("Missing profile samples")?;
    let mut text = format!("<b>{} route points</b><br>Samples: {}/{}<br>Horizontal: {:.1} m<br>Ascent: {:.1} m | Descent: {:.1} m<br>Maximum interval: {:.1} m | NoData: {}<br>",
        value["pointCount"], value["processed"], samples.len(), number("horizontal"),
        number("ascent"), number("descent"), number("maxInterval"), value["missing"]);
    text += &escape(value["message"].as_str().unwrap_or(""));
    text += &chart.render(value)?;
    window.clear_log()?;
    window.append_log(&text)?;
    Ok(())
}

fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool, value: &Value) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, loading)?;
    for id in 3..=7 { window.set_control_enabled(id, ready && !loading)?; }
    let count = value["pointCount"].as_u64().unwrap_or(0);
    window.set_control_enabled(8, ready && !loading && value["capture"] == true && count >= 2)?;
    for id in 9..=10 { window.set_control_enabled(id, ready && !loading && count > 0)?; }
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "ElevationProfile — GeoKernel", 1300, 850)? };
    let controls = window.add_control_panel(&json!({
        "title":"Elevation profile",
        "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"number","label":"Sample spacing (m)","minimum":1,"maximum":100,"step":1,"decimals":0,"value":10},
            {"id":5,"type":"combo","label":"Add points on click","options":["Enabled","Disabled"],"value":"Enabled"},
            {"id":6,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":7,"type":"button","text":"Reset view"},
            {"id":8,"type":"button","text":"Finish profile"},
            {"id":9,"type":"button","text":"Undo last point"},
            {"id":10,"type":"button","text":"Clear profile"}
        ]
    }).to_string())?;
    window.add_log_panel("Elevation profile")?;
    window.append_log("Enable Add points and click at least two terrain positions. Dragging navigates.<br>Approximate mesh profile; maximum 64 points and 4096 samples. NoData gaps are not joined. Exaggeration does not change results. DEM datum is unverified.")?;
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
    let mut spacing = 10.0;
    let mut capture = true;
    let mut imagery = 1;
    let mut height = 1.0;
    let mut ready = false;
    let mut loading = true;
    let mut chart = chart::Chart::new()?;
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
                        if ready { terrain.mode(false, spacing)?; }
                        if let Err(error) = terrain.load(&dem, &image, 512) {
                            if ready { terrain.mode(capture, spacing)?; }
                            return Err(error);
                        }
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    2 if loading => terrain.cancel(),
                    _ if !ready || loading => {}
                    3 => { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; }
                    4 => { spacing = event.number; terrain.mode(capture, spacing)?; }
                    5 => { capture = event.text == "Enabled"; terrain.mode(capture, spacing)?; }
                    6 => { height = event.number as f32; terrain.height(height)?; }
                    7 => terrain.reset()?,
                    8 => {
                        capture = false;
                        terrain.mode(capture, spacing)?;
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
                    window.set_status_text("Ready — click terrain to draw a profile.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
            // A cancelled or failed replacement retains the prior scene and points.
            if !loading && ready { terrain.mode(capture, spacing)?; }
        }
        if ready && !loading && Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(150);
            let current = terrain.profile()?;
            if current != snapshot {
                display(&mut window, &current, &mut chart)?;
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
