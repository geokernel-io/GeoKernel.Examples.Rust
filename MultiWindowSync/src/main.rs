mod bootstrap;
use geokernel::{Extent, Runtime, Viewer, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn load(viewer: &mut Viewer<'_>, path: &str, name: &str, style: &Value) -> Result<()>
{
    if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &style.to_string())? {
        return Err(format!("Could not load {name}").into());
    }
    Ok(())
}

fn extent_equal(a: Extent, b: Extent) -> bool
{
    [(a.x_min, b.x_min), (a.y_min, b.y_min), (a.x_max, b.x_max), (a.y_max, b.y_max)]
        .iter()
        .all(|(x, y)| (x - y).abs() <= 0.00001 * x.abs().max(1.0))
}

fn sync(window: &mut ViewerWindow, enabled: bool, last: &mut [Extent; 2]) -> Result<()>
{
    let a = window.pane(0)?.view_extent()?;
    let b = window.pane(1)?.view_extent()?;
    if enabled {
        if !extent_equal(a, last[0]) {
            window.pane(1)?.set_view_extent(a)?;
        } else if !extent_equal(b, last[1]) {
            window.pane(0)?.set_view_extent(b)?;
        }
    }
    *last = [window.pane(0)?.view_extent()?, window.pane(1)?.view_extent()?];
    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.show()?;
    window.process_events();
    let commands=window.add_command_toolbar(&json!({"commands":[{"id":1,"text":"Zoom In"},{"id":2,"text":"Zoom Out"},{"id":3,"text":"Full Extent"},{"id":4,"text":"Toggle Sync"},{"id":5,"text":"Zoom Box"},{"id":6,"text":"Pan"}]}).to_string())?;
    let initial = Extent {
        x_min: -151.2,
        y_min: 16.4,
        x_max: -41.6,
        y_max: 55.6,
    };
    for i in 0..2 {
        load(
            &mut window.pane(i)?,
            path,
            if i == 0 { "World A" } else { "World B" },
            &json!({"fillColor":"#D8E5E1","fillOpacity":220,"lineColor":"#6F8883","lineWidth":0.8}),
        )?;
        window.pane(i)?.set_view_extent(initial)?;
    }
    let mut enabled = true;
    let mut last = [window.pane(0)?.view_extent()?, window.pane(1)?.view_extent()?];
    window.set_status_text("Sync On")?;
    while window.is_visible()? {
        window.process_events();
        for id in commands.try_iter() {
            match id {
                1 => window.pane(0)?.zoom_in(1.5)?,
                2 => window.pane(0)?.zoom_out(1.5)?,
                3 => window.pane(0)?.full_extent()?,
                4 => {
                    enabled = !enabled;
                    if enabled {
                        let a = window.pane(0)?.view_extent()?;
                        window.pane(1)?.set_view_extent(a)?;
                    }
                    window.set_status_text(if enabled { "Sync On" } else { "Sync Off" })?;
                }
                5 | 6 => {
                    for i in 0..2 {
                        window.pane(i)?.use_tool(if id == 5 { ViewerTool::ZoomBox } else { ViewerTool::Pan });
                    }
                }
                _ => (),
            }
        }
        sync(window, enabled, &mut last)?;

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
        let mut window = unsafe { ViewerWindow::new_dual(&runtime, "MultiWindowSync", 1280, 760)? };
        run(&mut window, path)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
