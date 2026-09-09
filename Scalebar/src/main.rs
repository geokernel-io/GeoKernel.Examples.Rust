mod bootstrap;
use geokernel::{Runtime, ViewerTool, ViewerWindow};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const BOTTOM_RIGHT: i32 = 4;

fn run(window: &mut ViewerWindow, layer: &str) -> Result<()>
{
    window.set_status_text("Loading world layer...")?;

    window.show()?;
    window.process_events();

    let feature_count = {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        viewer.set_scale_bar_anchor(BOTTOM_RIGHT)?;
        viewer.set_scale_bar_visible(true)?;

        if !viewer.add_layer_file(layer)? {
            return Err(format!("World layer could not be loaded: {layer}").into());
        }

        viewer.full_extent()?;

        if viewer.get_layer_count()? != 1 {
            return Err("Expected one world layer".into());
        }

        let count = viewer.get_layer_feature_count(0)?;

        if count <= 0 {
            return Err("World layer has no features".into());
        }

        count
    };

    window.set_status_text(&format!("World layer loaded — {feature_count} features"))?;

    while window.is_visible()? {
        window.process_events();
        // Drain copied native notifications without executing Rust in Qt callbacks.

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let layer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/world_4326/world_4326.shp");

    let layer = layer.canonicalize().map_err(|error| {
        format!(
            "Sample data is unavailable ({}): {error}. Run ./run.ps1 -Example Scalebar first.",
            layer.display()
        )
    })?;

    let layer = layer
        .to_str()
        .ok_or("Shapefile path is not valid Unicode")?;

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "Scalebar", 1200, 800)? };
        run(&mut window, layer)
        // Window drops here on success and on setup/loading errors.
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
