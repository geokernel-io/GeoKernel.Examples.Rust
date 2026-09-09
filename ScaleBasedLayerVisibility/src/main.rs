mod bootstrap;
use geokernel::{ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use runtime::Result;

fn update(window: &mut ViewerWindow) -> Result<Vec<bool>>
{
    let (x0, _) = window.viewer().world_to_screen(0.0, 0.0)?;
    let (x1, _) = window.viewer().world_to_screen(1.0, 0.0)?;
    let scale = (x1 - x0).abs();
    let mut items = Vec::new();
    let mut states = Vec::new();
    for index in 0..3 {
        let info: Value = serde_json::from_str(&window.viewer().get_layer_info_json(index)?)?;
        let min = info["minVisibleScale"].as_f64().ok_or("Missing min scale")?;
        let max = info["maxVisibleScale"].as_f64().ok_or("Missing max scale")?;
        let visible = info["visible"] == true && (min <= 0.0 || scale >= min) && (max <= 0.0 || scale <= max);
        states.push(visible);
        items.push(json!({"shape":"none","label":format!("[{}] {} [{min} - {max}]",if visible {"x"} else {" "}, info["name"].as_str().unwrap_or("Layer"))}));
    }
    window.set_legend_items_json(&json!(items).to_string())?;
    window.set_status_text(&format!("Current scale: {scale:.2} px/map unit"))?;
    Ok(states)
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    runtime::show(window)?;
    window.viewer().use_tool(ViewerTool::Pan);
    window.add_navigation_toolbar();
    window.add_legend_panel("Visible scale ranges [min - max]")?;
    for which in 0..3 {
        layers::add(window, path, which, true)?;
    }
    window.viewer().set_view_extent(layers::INITIAL)?;
    let _initial = update(window)?;
    while window.is_visible()? {
        window.process_events();
        update(window)?;

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ScaleBasedLayerVisibility", "world_4326/world_4326.shp", run)
}

mod layers
{
    use crate::runtime::{self, Result};
    use geokernel::{Extent, ViewerWindow};
    use serde_json::{json, Value};

    pub const INITIAL: Extent = Extent {
        x_min: -151.2,
        y_min: 16.4,
        x_max: -41.6,
        y_max: 55.6,
    };
    pub const NAMES: [&str; 3] = ["World", "States", "Cities"];

    pub fn add(window: &mut ViewerWindow, world: &str, which: usize, scale: bool) -> Result<()>
    {
        let existing: Value = serde_json::from_str(&window.viewer().get_layer_info_by_name_json(NAMES[which])?)?;
        if existing["isValid"] == true {
            return Ok(());
        }
        let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
        let path = match which {
            0 => world.to_owned(),
            1 => data_path(data.join("usa_states/usa_states.shp")),
            _ => data_path(data.join("usa_cities/usa_cities.shp")),
        };
        let style = match which {
            0 => json!({"fillColor":"#D8E5E1","fillOpacity":if scale {225} else {220},"lineColor":"#7B918D","lineWidth":0.8}),
            1 => json!({"fillColor":"#A9C8DB","fillOpacity":if scale {135} else {115},"lineColor":"#356780","lineWidth":if scale {1.1} else {1.2}}),
            _ => json!({"pointColor":"#D95D39","pointSize":7.0,"lineColor":"#873A24","lineWidth":1.0}),
        };
        runtime::load(&mut window.viewer(), &path, NAMES[which], &style)?;
        if scale {
            let (min, max) = [(0.0, 11.0), (5.0, 45.0), (28.0, 0.0)][which];
            if !window.viewer().set_layer_visible_scale_range(0, min, max)? {
                return Err("Scale range rejected".into());
            }
        }
        window.viewer().refresh_layers()?;
        Ok(())
    }

    fn data_path(fallback: std::path::PathBuf) -> String
    {

        fallback.to_string_lossy().into_owned()
    }
}

mod runtime
{
    use geokernel::{Runtime, Viewer, ViewerWindow};
    use serde_json::Value;
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
}
