mod bootstrap;
use geokernel::{ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use runtime::Result;

const ZONES: [&str; 5] = ["Residential", "Commercial", "Industrial", "Park", "Mixed"];

fn apply_zone(window: &mut ViewerWindow, row: usize, zone: &str, names: &[String]) -> Result<()>
{
    if !window
        .viewer()
        .set_feature_attributes_in_edit_layer_json(0, 0, row as i32 + 1, &json!({"name":names[row],"zone":zone}).to_string())?
    {
        return Err("Feature attribute update failed".into());
    }
    runtime::refresh(window)?;
    window.set_status_text(&format!("{} - zone={zone}", names[row]))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    runtime::show(window)?;
    window.viewer().use_tool(ViewerTool::Pan);
    window.viewer().add_open_street_map_layer(true)?;
    let base = json!({"fillColor":"#AAE5E7EB","fillOpacity":170,"lineColor":"#EB6B7280","lineWidth":1.2});
    runtime::load(&mut window.viewer(), path, "California counties - style from zone attribute", &base)?;
    for field in ["name", "zone"] {
        if !window.viewer().add_layer_attribute_definition(0, field, 0, 120, 0)? {
            return Err("Attribute definition failed".into());
        }
    }
    let count = window.viewer().get_layer_feature_count(0)?;
    let mut names = Vec::new();
    for row in 0..count {
        let attrs: Value = serde_json::from_str(&window.viewer().get_layer_feature_attributes_json(0, row)?)?;
        names.push(attrs["NAME"].as_str().map(str::to_owned).unwrap_or_else(|| format!("Feature {}", row + 1)));
    }
    if names.is_empty() || !window.viewer().begin_edit_layer(0)? {
        return Err("No editable counties".into());
    }
    let mut zones: Vec<String> = (0..names.len()).map(|i| ZONES[i % 5].into()).collect();
    for (row, zone) in zones.iter().enumerate() {
        apply_zone(window, row, zone, &names)?;
    }
    let fills = ["#AAF5DFA1", "#AA9DD7F5", "#AAC4B5FD", "#AA9AD9A8", "#AAFDBA9A"];
    let lines = ["#EBA16207", "#EB0369A1", "#EB6D28D9", "#EB15803D", "#EBC2410C"];
    let rules:Vec<Value>=(0..5).map(|i|json!({"field":"zone","operator":"equals","value":ZONES[i],"label":ZONES[i],"enabled":true,"style":{"fillColor":fills[i],"fillOpacity":170,"lineColor":lines[i],"lineWidth":1.2}})).collect();
    if !window
        .viewer()
        .set_layer_symbol_renderer_json(0, &json!({"type":"ruleBased","defaultStyle":base,"rules":rules}).to_string())?
    {
        return Err("Zone renderer failed".into());
    }
    window.add_legend_panel("Zone styles")?;
    runtime::legend(window, false)?;
    let labels: Vec<String> = names.iter().enumerate().map(|(i, name)| format!("{}: {name}", i + 1)).collect();
    let rx = window.add_control_panel(
        &json!({"title":"Feature attributes","area":"right","controls":[
            {"id":1,"type":"combo","label":"Feature","options":labels,"value":labels[0]},
            {"id":2,"type":"combo","label":"Zone attribute","options":ZONES,"value":zones[0]},
            {"id":3,"type":"button","text":"Apply Feature Style"}
        ]})
        .to_string(),
    )?;
    window.viewer().zoom_to_layer(0)?;
    runtime::refresh(window)?;
    {
        let mut selected = 0;
        let mut zone = zones[0].clone();
        while window.is_visible()? {
            window.process_events();
            for event in rx.try_iter() {
                match event.id {
                    1 => {
                        selected = labels.iter().position(|s| *s == event.text).ok_or("Invalid feature")?;
                        zone = zones[selected].clone();
                        window.set_control_value(2, 0.0, &zone)?;
                    }
                    2 => zone = event.text,
                    3 => {
                        apply_zone(window, selected, &zone, &names)?;
                        zones[selected] = zone.clone();
                    }
                    _ => (),
                }
            }

        }
    }
    // Changes belong to this demonstration session; no save-to-file call is made.
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("StylePerFeature", "california/california.shp", run)
}

mod runtime
{
    use geokernel::{Runtime, Viewer, ViewerWindow};
    use serde_json::{json, Value};
    use std::{
        error::Error,
        path::PathBuf,
    };
    pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

    pub fn main(name: &str, default_data: &str, run: fn(&mut ViewerWindow, &str) -> Result<()>) -> Result<()>
    {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data").join(default_data);

        let path = if default_data.is_empty() {
            String::new()
        } else {
            path.canonicalize()?.to_str().ok_or("Invalid data path")?.to_owned()
        };
        // This standalone process owns the main thread and loads the trusted matching SDK.
        let runtime = unsafe { Runtime::from_env()? };
        let outcome = (|| {
            let mut window = unsafe {
                if matches!(name, "MultiWindowSync" | "LabelCollisionOff") {
                    ViewerWindow::new_dual(&runtime, name, 1280, 760)?
                } else {
                    ViewerWindow::new(&runtime, name, 1200, 800)?
                }
            };
            run(&mut window, &path)
        })();
        let shutdown = unsafe { runtime.shutdown_viewer_application() };
        outcome?;
        shutdown?;
        Ok(())
    }

    pub fn show(window: &mut ViewerWindow) -> Result<()>
    {
        window.show()?;
        window.process_events();
        Ok(())
    }

    pub fn load(viewer: &mut Viewer<'_>, path: &str, name: &str, style: &Value) -> Result<()>
    {
        if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &style.to_string())? {
            return Err(format!("Could not load {name}").into());
        }
        Ok(())
    }

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }

    pub fn renderer(window: &mut ViewerWindow) -> Result<Value>
    {
        let text = window.viewer().get_layer_symbol_renderer_json(0)?;
        Ok(if text.trim().is_empty() { json!({}) } else { serde_json::from_str(&text)? })
    }

    pub fn legend(window: &mut ViewerWindow, point: bool) -> Result<usize>
    {
        let value = renderer(window)?;
        let mut items = Vec::new();
        for key in ["categories", "ranges", "rules"] {
            if let Some(rows) = value[key].as_array() {
                for row in rows {
                    let mut item = row.clone();
                    item["shape"] = json!(if point { "point" } else { "polygon" });
                    if item["label"].as_str().is_none_or(str::is_empty) {
                        item["label"] = json!(row["value"].to_string());
                    }
                    items.push(item);
                }
            }
        }
        if !window.set_legend_items_json(&json!(items).to_string())? {
            return Err("Could not update legend".into());
        }
        Ok(items.len())
    }
}
