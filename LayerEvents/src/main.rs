mod bootstrap;
use geokernel::{Extent, Runtime, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const INITIAL: Extent = Extent {
    x_min: -151.2,
    y_min: 16.4,
    x_max: -41.6,
    y_max: 55.6,
};

const NAMES: [&str; 3] = ["World", "States", "Cities"];

fn add_layer(window: &mut ViewerWindow, world: &str, which: usize) -> Result<()>
{
    let existing: Value = serde_json::from_str(&window.viewer().get_layer_info_by_name_json(NAMES[which])?)?;
    if existing["isValid"] == true {
        return Ok(());
    }
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let path = match which {
        0 => world.to_owned(),
        1 => data.join("usa_states/usa_states.shp").to_string_lossy().into_owned(),
        _ => data.join("usa_cities/usa_cities.shp").to_string_lossy().into_owned(),
    };
    let style = match which {
        0 => json!({"fillColor":"#D8E5E1","fillOpacity":220,"lineColor":"#7B918D","lineWidth":0.8}),
        1 => json!({"fillColor":"#A9C8DB","fillOpacity":115,"lineColor":"#356780","lineWidth":1.2}),
        _ => json!({"pointColor":"#D95D39","pointSize":7.0,"lineColor":"#873A24","lineWidth":1.0}),
    };
    let mut viewer = window.viewer();
    if !viewer.add_layer_file(&path)? || !viewer.set_layer_name(0, NAMES[which])? || !viewer.set_layer_style_json(0, &style.to_string())? {
        return Err(format!("Could not load {}", NAMES[which]).into());
    }
    viewer.refresh_layers()?;
    Ok(())
}

fn command(window: &mut ViewerWindow, path: &str, id: i32) -> Result<()>
{
    window.append_log(&format!("Action: command {id}"))?;
    let index = window.get_selected_layer_index()?;
    match id {
        1..=3 => add_layer(window, path, (id - 1) as usize)?,
        4 => {
            if index >= 0 {
                window.viewer().remove_layer(index)?;
            }
        }
        5 => window.viewer().clear_layers()?,
        6 => {
            if index >= 0 {
                let info: Value = serde_json::from_str(&window.viewer().get_layer_info_json(index)?)?;
                window.viewer().set_layer_visible(index, info["visible"] != true)?;
            }
        }
        9 => window.viewer().refresh_layers()?,
        10 => window.clear_log()?,
        _ => return Err("Unknown event command".into()),
    }
    Ok(())
}

fn log(window: &mut ViewerWindow, events: &std::sync::mpsc::Receiver<geokernel::ViewerEvent>) -> Result<Vec<i32>>
{
    let mut types = Vec::new();
    for event in events.try_iter() {
        let kind = event.data.event_type;
        let name = match kind {
            2 => "layersChanged",
            3 => "layerAdded",
            4 => "layerRemoved",
            5 => "layerVisibilityChanged",
            6 => "layerEditStateChanged",
            7 => "layerEditSessionStarted",
            8 => "layerEditSessionCommitted",
            9 => "layerEditSessionRolledBack",
            10 => "layerOrderChanged",
            23 => "layerAdding",
            24 => "layerRemoving",
            25 => "indexCreating",
            26 => "indexCreated",
            27 => "indexLoaded",
            28 => "renderBackendChanged",
            _ => continue,
        };
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() % 86400000;
        // Native strings can contain HTML; escape them for the Qt rich text log.
        let text = event.text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        window.append_log(&format!(
            "{:02}:{:02}:{:02}.{:03} Signal: {name}({text}) [{} / {}]",
            time / 3600000,
            time / 60000 % 60,
            time / 1000 % 60,
            time % 1000,
            event.data.int_value,
            event.data.int_value2
        ))?;
        types.push(kind);
    }
    Ok(types)
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.show()?;
    window.process_events();
    window.viewer().use_tool(ViewerTool::Pan);
    // The panel's move buttons keep the moved layer selected and disable
    // moves beyond the first/last row. Viewer events are logged below.
    window.add_layer_panel(true, false)?;
    window.add_log_panel("Layer Events")?;
    let commands: Vec<Value> = [
        (1, "Add World"),
        (2, "Add States"),
        (3, "Add Cities"),
        (4, "Remove Selected"),
        (5, "Clear Layers"),
        (6, "Toggle Visibility"),
        (9, "Refresh"),
        (10, "Clear Log"),
    ]
    .iter()
    .map(|(id, text)| json!({"id":id,"text":text}))
    .collect();
    let rx = window.add_command_toolbar(&json!({"commands":commands}).to_string())?;
    let events = window.subscribe_events();
    for id in 1..=3 {
        command(window, path, id)?;
    }
    window.viewer().set_view_extent(INITIAL)?;
    while window.is_visible()? {
        window.process_events();
        for id in rx.try_iter() {
            command(window, path, id)?;
        }
        log(window, &events)?;

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data/world_4326/world_4326.shp")
        .canonicalize()?;
    let path = path.to_str().ok_or("Invalid data path")?;
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "LayerEvents", 1200, 800)? };
        run(&mut window, path)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
