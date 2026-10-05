mod bootstrap;
mod terrain;
mod visibility_map;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Settings { eye: f64, target: f64, radius: f64, spacing: f64, width: i32 }
impl Settings {
    fn apply(&self, terrain: &Terrain) -> Result<()> {
        terrain.configure(self.eye, self.target, self.radius, self.spacing, self.width)
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn display(window: &mut ViewerWindow, value: &Value, map: &mut visibility_map::VisibilityMap) -> Result<()> {
    let cells = value["cells"].as_array().ok_or("Missing viewshed cells")?;
    let count = |state: u64| cells.iter().filter(|v| v.as_u64() == Some(state)).count();
    let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
    let mut text = if value["hasObserver"] == true {
        format!("Observer: {:.6}° E, {:.6}° N<br>", number("longitude"), number("latitude"))
    } else { "No observer selected.<br>".into() };
    text += &format!("{}<br>Progress: {}/{}<br>Visible: {} | Blocked: {} | Unknown: {} | Pending: {}<br>Grid spacing: {:.1} m | Maximum ray spacing: {:.1} m<br>{}",
        escape(value["message"].as_str().unwrap_or("")), value["processed"], cells.len(),
        count(2), count(3), count(4), count(1), number("cellSpacing"), number("maxRaySpacing"),
        escape(value["overlayMessage"].as_str().unwrap_or("")));
    text += &map.render(value)?;
    text += "<p><font color='#239b56'>■ Visible</font> <font color='#d35445'>■ Blocked</font> <font color='#858585'>■ Unknown / NoData</font> <font color='#8795a6'>■ Pending</font></p>";
    window.clear_log()?;
    window.append_log(&text)?;
    window.set_control_value(4, 0.0, if value["capture"] == true { "Enabled" } else { "Disabled" })?;
    Ok(())
}
fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool, value: &Value) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, loading)?;
    for id in 3..=15 { window.set_control_enabled(id, ready && !loading)?; }
    let observer = value["hasObserver"] == true;
    let running = value["running"] == true;
    window.set_control_enabled(10, ready && !loading && observer && !running)?;
    window.set_control_enabled(11, ready && !loading && running)?;
    window.set_control_enabled(12, ready && !loading && observer)?;
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "ViewshedAnalysis — GeoKernel", 1400, 900)? };
    let controls = window.add_control_panel(&json!({
        "title":"Viewshed analysis",
        "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"combo","label":"Select observer on terrain","options":["Enabled","Disabled"],"value":"Enabled"},
            {"id":5,"type":"number","label":"Observer above terrain (m)","minimum":0.1,"maximum":100,"step":0.1,"decimals":2,"value":1.7},
            {"id":6,"type":"number","label":"Target above terrain (m)","minimum":0,"maximum":100,"step":0.1,"decimals":2,"value":1.7},
            {"id":7,"type":"number","label":"Radius (m)","minimum":10,"maximum":2000,"step":10,"decimals":2,"value":300},
            {"id":8,"type":"number","label":"Ray sample spacing (m)","minimum":1,"maximum":50,"step":1,"decimals":2,"value":5},
            {"id":9,"type":"combo","label":"Grid","options":["17 × 17","33 × 33","65 × 65"],"value":"33 × 33"},
            {"id":10,"type":"button","text":"Calculate / Resume / Recalculate"},
            {"id":11,"type":"button","text":"Pause calculation"},
            {"id":12,"type":"button","text":"Clear observer and result"},
            {"id":13,"type":"combo","label":"Result on terrain","options":["Visible","Hidden"],"value":"Visible"},
            {"id":14,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":15,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("North-up visibility grid and results")?;
    window.append_log("Select an observer on terrain, then calculate.<br>Terrain-only, sampled visibility on a 512-sample mesh. Buildings, vegetation and atmospheric refraction are excluded. Small obstacles may be missed. Vertical datum is unverified. Exaggeration changes only the view.")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let dem = root.join("sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let image = root.join("sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif").canonicalize()?;
    let mut settings = Settings { eye: 1.7, target: 1.7, radius: 300.0, spacing: 5.0, width: 33 };
    let mut imagery = 1;
    let mut overlay = true;
    let mut height = 1.0;
    let mut ready = false;
    let mut loading = true;
    let mut map = visibility_map::VisibilityMap::new()?;
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
                        if ready {
                            terrain.capture(false)?;
                            if terrain.viewshed()?["running"] == true { terrain.action(1)?; }
                        }
                        terrain.load(&dem, &image, 512)?;
                        loading = true;
                        window.set_status_text("Loading terrain…")?;
                    }
                    2 if loading => terrain.cancel(),
                    _ if !ready || loading => {}
                    3 => { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; }
                    4 => terrain.capture(event.text == "Enabled")?,
                    5..=9 => {
                        match event.id {
                            5 => settings.eye = event.number,
                            6 => settings.target = event.number,
                            7 => settings.radius = event.number,
                            8 => settings.spacing = event.number,
                            9 => settings.width = match event.text.as_str() { "17 × 17" => 17, "65 × 65" => 65, _ => 33 },
                            _ => {}
                        }
                        settings.apply(&terrain)?;
                    }
                    10 => terrain.action(0)?,
                    11 => terrain.action(1)?,
                    12 => terrain.action(2)?,
                    13 => { overlay = event.text != "Hidden"; terrain.overlay(overlay)?; }
                    14 => { height = event.number as f32; terrain.height(height)?; }
                    15 => terrain.reset()?,
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
                    settings.apply(&terrain)?;
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    terrain.overlay(overlay)?;
                    terrain.capture(true)?;
                    ready = true;
                    snapshot = Value::Null;
                    window.set_status_text("Ready — select an observer on terrain.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
        }
        if ready && !loading && Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(150);
            let current = terrain.viewshed()?;
            if current != snapshot {
                display(&mut window, &current, &mut map)?;
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
