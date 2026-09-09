mod bootstrap;
use geokernel::{Runtime, ViewerTool, ViewerWindow};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(window: &mut ViewerWindow) -> Result<()>
{
    let layer = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/world_4326/world_4326.shp");

    window.add_navigation_toolbar();
    window.show()?;
    window.process_events();

    let mut viewer = window.viewer();
    viewer.use_tool(ViewerTool::Pan);

    if !viewer.add_layer_file(layer)? {
        return Err(format!("World layer could not be loaded: {layer}").into());
    }

    viewer.full_extent()?;

    window.set_status_text("World layer loaded.")?;

    while window.is_visible()? {
        window.process_events();
    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };

    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "HelloMap", 1200, 800)? };
        run(&mut window)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
