mod bootstrap;
use geokernel::{Runtime, ViewerWindow, ViewerTool};
use serde_json::json;
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{

    window.show()?;
    window.process_events();

    let california_style = json!({"fillColor":"#D8E5E1", "fillOpacity":210, "lineColor":"#6F8883", "lineWidth":0.9});
    let rectangle_style = json!({"fillColor":"#FFFFFF", "fillOpacity":0, "lineColor":"#E2453D", "lineWidth":2.2});
    let rectangle;

    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);

        if !viewer.set_coordinate_system_preset("EPSG:4326")? || !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, "California")? || !viewer.set_layer_style_json(0, &california_style.to_string())? {
            return Err("Could not load or configure California".into());
        }

        let extent = viewer.layer_projected_extent(0)?;

        if extent.x_max <= extent.x_min || extent.y_max <= extent.y_min {
            return Err("California has an empty extent".into());
        }

        rectangle = [[extent.x_min, extent.y_min], [extent.x_max, extent.y_min], [extent.x_max, extent.y_max], [extent.x_min, extent.y_max], [extent.x_min, extent.y_min]];

        let overlay_index = viewer.add_polygon_layer("Layer Extent", &rectangle, &rectangle_style.to_string())?;

        if overlay_index < 0 || !viewer.set_layer_coordinate_system_preset(overlay_index, "EPSG:4326")? {
            return Err("Could not create layer extent rectangle".into());
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
        let mut window = unsafe { ViewerWindow::new(&runtime, "LayerExtent", 1200, 800)? };
        run(&mut window, path.to_str().ok_or("Invalid California path")?)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
