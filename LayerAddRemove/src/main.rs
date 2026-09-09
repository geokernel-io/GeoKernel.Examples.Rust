mod bootstrap;
use geokernel::{Runtime, Viewer, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn add_layer(viewer: &mut Viewer<'_>, layer: &Value) -> Result<()>
{
    let name = layer["name"].as_str().ok_or("Missing layer name")?;
    let existing: Value = serde_json::from_str(&viewer.get_layer_info_by_name_json(name)?)?;

    if existing["isValid"].as_bool() == Some(true) {
        return Ok(());
    }

    let path = layer["path"].as_str().ok_or("Missing layer path")?;

    if !viewer.add_layer_file(path)? {
        return Err(format!("{name} could not be loaded: {path}").into());
    }

    if !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &layer["style"].to_string())? {
        return Err(format!("Could not configure {name}").into());
    }

    viewer.refresh_layers()?;

    Ok(())
}

fn run(window: &mut ViewerWindow, layers: &[Value]) -> Result<()>
{
    if !window.add_layer_management_toolbar(&json!({"layers": layers}).to_string())? {
        return Err("Could not create layer management toolbar".into());
    }

    window.show()?;
    window.process_events();

    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        add_layer(&mut viewer, &layers[0])?;
        viewer.full_extent()?;
    }

    window.set_status_text("Add or remove World, States and Cities using the toolbar.")?;

    while window.is_visible()? {
        window.process_events();

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let mut paths = [data.join("world_4326/world_4326.shp"), data.join("usa_states/usa_states.shp"), data.join("usa_cities/usa_cities.shp")];

    for path in &mut paths {
        *path = path.canonicalize().map_err(|error| format!("{}: {error}. Run ./run.ps1 -Example LayerAddRemove first.", path.display()))?;
    }

    let layers = [
        json!({"name":"World", "path":paths[0].to_str().ok_or("Invalid World path")?, "style":{"fillColor":"#D8E5E1","fillOpacity":210,"lineColor":"#7B918D","lineWidth":0.8}}),
        json!({"name":"States", "path":paths[1].to_str().ok_or("Invalid States path")?, "style":{"fillColor":"#A9C8DB","fillOpacity":100,"lineColor":"#356780","lineWidth":1.2}}),
        json!({"name":"Cities", "path":paths[2].to_str().ok_or("Invalid Cities path")?, "style":{"pointColor":"#D95D39","pointSize":7.0}}),
    ];

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "LayerAddRemove", 1200, 800)? };
        run(&mut window, &layers)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
