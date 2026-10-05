mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;")
}

fn label(entry: &Value) -> String {
    let name = entry["attributes"]["name"].as_str().filter(|s| !s.is_empty())
        .or_else(|| entry["sourceId"].as_str()).unwrap_or("");
    format!("{}: {} / {}", entry["index"].as_str().unwrap_or(""),
        entry["layer"].as_str().unwrap_or(""), name)
}

fn show_selection(window: &mut ViewerWindow, snapshot: &Value) -> Result<()> {
    let entries = snapshot["entries"].as_array().ok_or("Invalid selection snapshot")?;
    let labels: Vec<String> = entries.iter().map(label).collect();
    window.set_control_options_json(10, &serde_json::to_string(&labels)?)?;
    let active = entries.iter().find(|e| e["index"] == snapshot["activeIndex"])
        .or_else(|| entries.first());
    if let Some(entry) = active {
        window.set_control_value(10, 0.0, &label(entry))?;
    }
    window.clear_log()?;
    window.append_log(&format!("<b>{} selected</b>", entries.len()))?;
    if let Some(entry) = active {
        let mut rows = vec![
            ("Layer".to_owned(), entry["layer"].as_str().unwrap_or("").to_owned()),
            ("Type".to_owned(), entry["type"].as_str().unwrap_or("").to_owned()),
            ("Scene feature ID".to_owned(), entry["featureId"].as_str().unwrap_or("").to_owned()),
        ];
        if let Some(fid) = entry["sourceFid"].as_str() { rows.push(("Source FID".into(), fid.into())); }
        if let Some(height) = entry["displayHeight"].as_f64() {
            rows.push(("Display height (estimated)".into(), format!("{height} m")));
        }
        if let Some(attributes) = entry["attributes"].as_object() {
            rows.extend(attributes.iter().map(|(key, value)|
                (key.clone(), value.as_str().unwrap_or("(null)").to_owned())));
        }
        let html = rows.iter().map(|(key, value)| format!(
            "<tr><td>{}</td><td>{}</td></tr>", escape(key), escape(value))).collect::<Vec<_>>().join("");
        window.append_log(&format!("<table>{html}</table>"))?;
    }
    Ok(())
}

fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool, selected: bool) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, loading)?;
    for id in 3..=8 { window.set_control_enabled(id, ready && !loading)?; }
    for id in 10..=12 { window.set_control_enabled(id, ready && !loading && selected)?; }
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "FeaturePicking — GeoKernel", 1200, 850)? };
    let controls = window.add_control_panel(&json!({
        "title":"Feature picking",
        "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"combo","label":"Buildings","options":["Visible","Hidden"],"value":"Visible"},
            {"id":5,"type":"combo","label":"Roads","options":["Visible","Hidden"],"value":"Visible"},
            {"id":6,"type":"combo","label":"Selection mode","options":["Single","Multiple"],"value":"Single"},
            {"id":7,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":8,"type":"button","text":"Reset view"},
            {"id":10,"type":"combo","label":"Active selection","options":[]},
            {"id":11,"type":"button","text":"Zoom to selection"},
            {"id":12,"type":"button","text":"Clear selection"}
        ]
    }).to_string())?;
    window.add_log_panel("Feature attributes")?;
    window.append_log("Click a building or road to inspect its attributes. Drag to navigate.<br>Building heights are illustrative (9 m). DEM heights are metres; the vertical datum is unverified.")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    // Terrain drops before its parent window and runtime.
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let dem = root.join("sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let image = root.join("sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif").canonicalize()?;
    let roads = root.join("sagrada_familia_roads/roads.shp").canonicalize()?;
    let buildings = root.join("sagrada_familia_buildings/buildings.shp").canonicalize()?;
    let mut loading = true;
    let mut ready = false;
    let mut snapshot = Value::Null;
    let mut imagery = 1;
    let mut buildings_visible = true;
    let mut roads_visible = true;
    let mut multiple = false;
    let mut height = 1.0;
    let mut next_selection = Instant::now();
    terrain.load(&dem, &image, &roads, &buildings, 9.0, 512)?;
    controls_state(&mut window, ready, loading, false)?;
    window.set_status_text("Loading terrain, buildings and roads…")?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        terrain.resize()?;
        while let Ok(event) = controls.try_recv() {
            let result: Result<()> = (|| {
                match event.id {
                    1 if !loading => {
                        terrain.load(&dem, &image, &roads, &buildings, 9.0, 512)?;
                        loading = true;
                        window.set_status_text("Loading sample…")?;
                    }
                    2 if loading => terrain.cancel(),
                    _ if !ready || loading => {}
                    3 => { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; }
                    4 => { buildings_visible = event.text != "Hidden"; terrain.building_style(buildings_visible, 1.0, "#d9c4a5")?; }
                    5 => { roads_visible = event.text != "Hidden"; terrain.roads(roads_visible)?; }
                    6 => { multiple = event.text == "Multiple"; terrain.multiple(multiple)?; }
                    7 => { height = event.number as f32; terrain.height(height)?; }
                    8 => terrain.reset()?,
                    10 => {
                        if let Some(entries) = snapshot["entries"].as_array() {
                            if let Some(entry) = entries.iter().find(|e| label(e) == event.text) {
                                if entry["index"] != snapshot["activeIndex"] {
                                    terrain.activate(snapshot["revision"].as_str().ok_or("Missing revision")?.parse()?,
                                        entry["index"].as_str().ok_or("Missing index")?.parse()?)?;
                                }
                            }
                        }
                    }
                    11 => terrain.focus()?,
                    12 => terrain.clear()?,
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
                    terrain.building_style(buildings_visible, 1.0, "#d9c4a5")?;
                    terrain.roads(roads_visible)?;
                    terrain.multiple(multiple)?;
                    terrain.height(height)?;
                    window.set_status_text("Ready — click a building or road.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
        }
        if ready && !loading && Instant::now() >= next_selection {
            next_selection = Instant::now() + Duration::from_millis(150);
            let current = terrain.selection()?;
            if current["revision"] != snapshot["revision"] {
                show_selection(&mut window, &current)?;
                snapshot = current;
            }
        }
        let selected = snapshot["entries"].as_array().is_some_and(|entries| !entries.is_empty());
        controls_state(&mut window, ready, loading, selected)?;
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
