mod bootstrap;
use geokernel::{Runtime, Viewer, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn prepare_layers(directory: &std::path::Path) -> Result<Vec<Value>>
{
    let palette = ["#BFD6E5", "#C9D5C9", "#D8CDA7", "#D7B79B", "#D6C6E3", "#B9D8C5"];
    let mut paths = Vec::new();

    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();

        if path.is_file() && path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("shp")) {
            paths.push(path);
        }
    }

    paths.sort();

    if paths.is_empty() {
        return Err("No city Shapefiles found. Run ./run.ps1 -Example LayerZoomTo first.".into());
    }

    let mut layers = Vec::new();

    for (index, path) in paths.iter().enumerate() {
        let stem = path.file_stem().and_then(|value| value.to_str()).ok_or("Invalid city filename")?;
        let name = stem.split('_').filter(|word| !word.is_empty()).map(|word| {
            let mut chars = word.chars();
            let first = chars.next().expect("Nonempty word");
            format!("{}{}", first.to_uppercase(), chars.as_str())
        }).collect::<Vec<_>>().join(" ");

        layers.push(json!({"name":name, "path":path.to_str().ok_or("Invalid city path")?, "style":{
            "fillColor":palette[index % palette.len()], "fillOpacity":150, "lineColor":"#5F7772", "lineWidth":0.8,
            "showLabels":true, "labelFontSize":12.0, "labelAllowOverlap":true, "labelAvoidObstacles":false,
            "labelField":"NAME", "labelColor":"#000000", "labelHaloEnabled":true, "labelHaloColor":"#FFFF00", "labelHaloWidth":2.0
        }}));
    }

    Ok(layers)
}

fn add_layer(viewer: &mut Viewer<'_>, layer: &Value) -> Result<()>
{
    let name = layer["name"].as_str().ok_or("Missing layer name")?;
    let path = layer["path"].as_str().ok_or("Missing layer path")?;

    if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &layer["style"].to_string())? {
        return Err(format!("Could not load or configure {name}: {path}").into());
    }

    Ok(())
}

fn run(window: &mut ViewerWindow, layers: &[Value]) -> Result<()>
{
    if !window.add_layer_zoom_selector()? {
        return Err("Could not create layer zoom selector".into());
    }

    window.show()?;
    window.process_events();

    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);

        for layer in layers {
            add_layer(&mut viewer, layer)?;
        }

        viewer.refresh_layers()?;
        viewer.full_extent()?;
    }

    while window.is_visible()? {
        window.process_events();

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/california_cities");

    let layers = prepare_layers(&data.canonicalize()?)?;

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "Layer ZoomTo", 1200, 800)? };
        run(&mut window, &layers)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
