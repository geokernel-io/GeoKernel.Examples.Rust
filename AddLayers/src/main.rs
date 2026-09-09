mod bootstrap;
use geokernel::{Runtime, ViewerTool, ViewerWindow};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const COUNTRY_STYLE: &str = r##"{"fillColor":"#35475B","fillOpacity":172,"lineColor":"#B7E8FF","lineWidth":0.85}"##;
const CITY_STYLE: &str = r##"{"pointColor":"#1D8FC7","lineColor":"#74C3E8","lineWidth":0.9,"pointSize":4.2}"##;

fn run(window: &mut ViewerWindow, paths: &[PathBuf; 3],) -> Result<()>
{
    window.add_navigation_toolbar();

    window.show()?;
    window.process_events();

    window.viewer().clear_layers()?;

    for ((name, style), path) in [
        ("World raster", None),
        ("Countries", Some(COUNTRY_STYLE)),
        ("Cities", Some(CITY_STYLE)),
    ]
    .into_iter()
    .zip(paths)
    {
        window.set_status_text(&format!("Loading {name}..."))?;
        let mut viewer = window.viewer();
        if !viewer.add_layer_file(path.to_str().ok_or("Layer path is not valid Unicode")?)? {
            return Err(format!("{name} could not be loaded: {}", path.display()).into());
        }
        // SDK inserts each newly loaded layer at index zero.
        if !viewer.set_layer_name(0, name)? {
            return Err(format!("Could not name {name}").into());
        }
        if let Some(style) = style {
            if !viewer.set_layer_style_json(0, style)? {
                return Err(format!("Could not style {name}").into());
            }
        }
    }
    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        viewer.refresh_layers()?;
        viewer.full_extent()?;
    }

    window.set_status_text("Raster, country and city layers loaded.")?;

    while window.is_visible()? {
        window.process_events();

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let mut paths = [
        data.join("world_8km_png/world_8km.png"),
        data.join("world_4326/world_4326.shp"),
        data.join("world_cities_4326/world_cities_4326.shp"),
    ];

    for path in &mut paths {
        *path = path.canonicalize().map_err(|error| {
            format!(
                "{}: {error}. Run ./run.ps1 -Example AddLayers first.",
                path.display()
            )
        })?;
    }

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "AddLayers", 1200, 800)? };
        run(&mut window, &paths)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
