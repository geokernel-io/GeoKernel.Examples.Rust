mod bootstrap;
use geokernel::{Runtime, ViewerWindow, ViewerTool};
use serde_json::json;
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    let fill_colors = ["#D8E5E1", "#D9C7A5", "#C7D7EA", "#D7C5DE"];
    let outline_colors = ["#6F8883", "#A24A3D", "#356780", "#6F4D8C"];
    let opacities = [210, 160, 110, 235];
    let configuration = json!({"fillColors":fill_colors, "outlineColors":outline_colors, "opacities":opacities});

    if !window.add_layer_style_toolbar(&configuration.to_string())? {
        return Err("Could not create layer style toolbar".into());
    }

    window.show()?;
    window.process_events();

    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        let style = json!({"fillColor":fill_colors[0], "fillOpacity":opacities[0], "lineColor":outline_colors[0], "lineWidth":0.9});

        if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, "California")? || !viewer.set_layer_style_json(0, &style.to_string())? {
            return Err("Could not load or configure California".into());
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
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/california/california.shp");

    let path = data.canonicalize()?;

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "LayerRefresh", 1200, 800)? };
        run(&mut window, path.to_str().ok_or("Invalid California path")?)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
