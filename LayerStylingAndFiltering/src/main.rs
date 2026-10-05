mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::Duration};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn text<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().unwrap_or("") }
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;")
}
fn layer_label(v: &Value) -> String { format!("{} [{}]", text(v, "name"), text(v, "id")) }
fn options(window: &mut ViewerWindow, id: i32, values: &[String], selected: &str) -> Result<()> {
    window.set_control_options_json(id, &serde_json::to_string(values)?)?;
    if !values.is_empty() { window.set_control_value(id, 0.0, selected)?; }
    Ok(())
}
fn sync(window: &mut ViewerWindow, layers: &[Value], active: &mut String) -> Result<()> {
    let Some(item) = layers.iter().find(|v| text(v, "id") == active.as_str()).or(layers.first()) else {
        active.clear();
        return Ok(());
    };
    *active = text(item, "id").to_owned();
    options(window, 4, &layers.iter().map(layer_label).collect::<Vec<_>>(), &layer_label(item))?;
    window.set_control_value(5, 0.0, if item["visible"].as_bool().unwrap_or(false) { "Visible" } else { "Hidden" })?;
    window.set_control_value(6, item["opacity"].as_f64().unwrap_or(1.0) * 100.0, "")?;
    window.set_control_value(7, 0.0, if text(item, "color").is_empty() { "#ffffff" } else { text(item, "color") })?;
    let attributes = item["attributes"].as_object().ok_or("Missing attributes")?;
    let mut fields: Vec<String> = attributes.keys().cloned().collect();
    fields.sort_by_key(|s| s.to_lowercase());
    let mut theme = vec!["(Uniform / source color)".to_owned()];
    theme.extend(fields.clone());
    let mut filter = vec!["(All attributes / source IDs)".to_owned()];
    filter.extend(fields);
    options(window, 9, &theme, if text(item, "theme").is_empty() { &theme[0] } else { text(item, "theme") })?;
    options(window, 10, &filter, if text(item, "field").is_empty() { &filter[0] } else { text(item, "field") })?;
    window.set_control_value(11, 0.0, text(item, "text"))?;
    let mut values = vec!["(Choose a value)".to_owned()];
    if let Some(items) = attributes.get(text(item, "field")).and_then(Value::as_array) {
        values.extend(items.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    options(window, 12, &values, &values[0])?;
    window.set_control_enabled(12, values.len() > 1)?;
    window.set_control_enabled(14, item["drawableRanges"].as_u64().unwrap_or(0) > 0
        && item["visible"].as_bool().unwrap_or(false) && item["opacity"].as_f64().unwrap_or(0.0) > 0.0)?;
    window.clear_log()?;
    window.append_log(&format!(
        "<b>{}</b><br>Matching records: {} / {}<br>Drawable matching ranges: {}<br>Color: {}<br><br>        Filters use literal, case-insensitive substring matching and affect drawing and selection.         Enter or leave the text field to apply. Example lists contain up to 200 values; filters search all records.         Counts include records without drawable geometry.<br><br>        Changes are session-only. Building heights are illustrative (9 m). DEM vertical datum is unverified.",
        escape(text(item, "name")), item["matchingRecords"], item["records"], item["drawableRanges"],
        if text(item, "color").is_empty() { "Source".to_owned() } else { escape(text(item, "color")) }))?;
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "LayerStylingAndFiltering — GeoKernel", 1250, 850)? };
    let controls = window.add_control_panel(&json!({
        "title":"Layer styling and filtering", "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"combo","label":"Layer","options":[]},
            {"id":5,"type":"combo","label":"Visibility","options":["Visible","Hidden"],"value":"Visible"},
            {"id":6,"type":"number","label":"Opacity (%)","minimum":0,"maximum":100,"step":1,"decimals":0,"value":100},
            {"id":7,"type":"color","label":"Uniform color","value":"#ffffff"},
            {"id":8,"type":"button","text":"Restore source color"},
            {"id":9,"type":"combo","label":"Color by attribute","options":[]},
            {"id":10,"type":"combo","label":"Filter attribute","options":[]},
            {"id":11,"type":"text","label":"Contains","value":"","placeholder":"Enter or leave field to apply"},
            {"id":12,"type":"combo","label":"Example values","options":[]},
            {"id":13,"type":"button","text":"Clear this filter"},
            {"id":14,"type":"button","text":"Zoom to matching features"},
            {"id":15,"type":"button","text":"Reset all styles and filters"},
            {"id":16,"type":"number","label":"Vertical exaggeration","minimum":0.25,"maximum":10,"step":0.25,"decimals":2,"value":1},
            {"id":17,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Layer details")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let dem = root.join("sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let image = root.join("sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif").canonicalize()?;
    let roads = root.join("sagrada_familia_roads/roads.shp").canonicalize()?;
    let buildings = root.join("sagrada_familia_buildings/buildings.shp").canonicalize()?;
    let mut layers: Vec<Value> = Vec::new();
    let mut active = String::new();
    let mut loading = true;
    let mut ready = false;
    let mut imagery = 1;
    let mut height = 1.0;
    terrain.resize()?;
    terrain.load(&dem, &image, &roads, &buildings, 9.0, 512)?;
    window.set_status_text("Loading terrain, roads and buildings…")?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        terrain.resize()?;
        // Take user events before synchronizing widgets. Programmatic widget events
        // produced by sync below are drained separately and never applied as edits.
        let events: Vec<_> = controls.try_iter().collect();
        let mut refresh = false;
        for event in events {
            let result: Result<()> = (|| {
                if event.id == 1 && !loading {
                    terrain.load(&dem, &image, &roads, &buildings, 9.0, 512)?;
                    loading = true;
                    window.set_status_text("Loading sample…")?;
                    return Ok(());
                }
                if event.id == 2 && loading { terrain.cancel(); return Ok(()); }
                if !ready || loading { return Ok(()); }
                if event.id == 3 { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; return Ok(()); }
                if event.id == 16 { height = event.number as f32; terrain.height(height)?; return Ok(()); }
                if event.id == 17 { terrain.reset()?; return Ok(()); }
                if event.id == 4 {
                    if let Some(item) = layers.iter().find(|v| layer_label(v) == event.text) {
                        active = text(item, "id").to_owned();
                        refresh = true;
                    }
                    return Ok(());
                }
                let item = layers.iter().find(|v| text(v, "id") == active.as_str()).ok_or("No layer selected")?;
                let command = match event.id {
                    5 => json!({"action":"appearance","visible":event.text == "Visible","opacity":item["opacity"]}),
                    6 => json!({"action":"appearance","visible":item["visible"],"opacity":event.number / 100.0}),
                    7 => json!({"action":"color","color":event.text}),
                    8 => json!({"action":"color","color":""}),
                    9 => json!({"action":"theme","field":if event.text == "(Uniform / source color)" { "" } else { &event.text }}),
                    10 => json!({"action":"filter","field":if event.text == "(All attributes / source IDs)" { "" } else { &event.text },"text":item["text"]}),
                    11 => json!({"action":"filter","field":item["field"],"text":event.text}),
                    12 if event.text != "(Choose a value)" => json!({"action":"filter","field":item["field"],"text":event.text}),
                    13 => json!({"action":"filter","field":item["field"],"text":""}),
                    14 => json!({"action":"focus"}),
                    15 => json!({"action":"reset"}),
                    _ => return Ok(()),
                };
                let mut command = command;
                command["id"] = json!(active);
                terrain.update(command)?;
                layers = terrain.layers()?;
                refresh = true;
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
                    terrain.imagery(imagery)?;
                    terrain.height(height)?;
                    layers = terrain.layers()?;
                    refresh = true;
                    window.set_status_text("Ready — choose a layer to style or filter.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
            if !loading && ready { refresh = true; }
        }
        window.set_control_enabled(1, !loading)?;
        window.set_control_enabled(2, loading)?;
        for id in 3..=17 { window.set_control_enabled(id, ready && !loading)?; }
        if ready && !loading {
            if refresh {
                sync(&mut window, &layers, &mut active)?;
                while controls.try_recv().is_ok() {}
            } else if let Some(item) = layers.iter().find(|v| text(v, "id") == active.as_str()) {
                window.set_control_enabled(12, item["attributes"][text(item, "field")].as_array().is_some_and(|v| !v.is_empty()))?;
                window.set_control_enabled(14, item["drawableRanges"].as_u64().unwrap_or(0) > 0
                    && item["visible"].as_bool().unwrap_or(false) && item["opacity"].as_f64().unwrap_or(0.0) > 0.0)?;
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
