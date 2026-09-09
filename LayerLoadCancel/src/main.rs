mod bootstrap;
use geokernel::{Runtime, ViewerWindow, ViewerTool};
use serde_json::json;
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    let style = json!({"fillColor":"#D8E5E1", "fillOpacity":210, "lineColor":"#607D78", "lineWidth":0.9});
    let configuration = json!({"filePath":path, "layerName":"One Million Points", "defaultStyle":style});

    if !window.add_cancellable_layer_load_toolbar(&configuration.to_string())? {
        return Err("Could not create cancellable load toolbar".into());
    }

    window.show()?;
    window.process_events();

    window.viewer().use_tool(ViewerTool::Pan);

    while window.is_visible()? {
        window.process_events();

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/output_1m_points/output_1m_points.shp");

    let path = data.canonicalize()?;

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "LayerLoadCancel", 1200, 800)? };
        run(&mut window, path.to_str().ok_or("Invalid point path")?)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
