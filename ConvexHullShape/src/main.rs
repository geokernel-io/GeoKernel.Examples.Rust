mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

const LEFT: &[[f64; 2]] = &[[-4.4, -1.6], [-3.4, 1.6], [-1.9, -0.7], [-0.4, 2.3], [0.8, -1.2], [2.0, 1.7], [3.9, -0.5], [2.5, -2.1], [0.4, -0.2], [-1.2, -2.0], [-2.7, 0.0], [-4.4, -1.6]];

fn calculate(window: &mut ViewerWindow) -> Result<()>
{
    window.viewer().clear_shapes()?;
    let result: Value = serde_json::from_str(&window.viewer().convex_hull_polygon_json(LEFT)?)?;
    let rings = result.as_array().ok_or("Expected polygon rings")?;
    if rings.is_empty() { return Err("The operation returned no geometry".into()); }
    let parts: Vec<Vec<[f64; 2]>> = rings.iter().map(|ring| {
        ring.as_array().ok_or("Expected ring")?.iter().map(|p| {
            Ok([p["x"].as_f64().ok_or("Missing x")?, p["y"].as_f64().ok_or("Missing y")?])
        }).collect::<Result<_>>()
    }).collect::<Result<_>>()?;
    window.viewer().add_polygon_shape(LEFT, &json!({"fillColor":"#BFD7EA","fillOpacity":100,"lineColor":"#2F80C2","lineWidth":2}).to_string())?;

    if !window.viewer().add_polygon_parts_shape(&parts, &json!({"fillColor":"#F9C74F","fillOpacity":155,"lineColor":"#D95D39","lineWidth":3}).to_string())? {
        return Err("Could not display result".into());
    }
    window.clear_log()?;
    window.append_log(&format!("<b>ConvexHullShape</b><pre>{}</pre>", serde_json::to_string_pretty(&result)?))?;
    window.set_status_text(&format!("ConvexHullShape: {} result rings, {} vertices", parts.len(), parts.iter().map(Vec::len).sum::<usize>()))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Result coordinates")?;
    let events = window.add_control_panel(&json!({"title":"ConvexHullShape","controls":[
        {"id":1,"type":"button","text":"Recalculate"},
        {"id":2,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    calculate(window)?;
    runtime::show(window)?;
    runtime::pump(window, 100);
    window.viewer().set_view_extent(Extent { x_min:-6.0, y_min:-4.0, x_max:6.0, y_max:4.0 })?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                1 => calculate(window)?,
                2 => window.viewer().set_view_extent(Extent { x_min:-6.0, y_min:-4.0, x_max:6.0, y_max:4.0 })?,
                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ConvexHullShape", "", run)
}

mod runtime
{
    use geokernel::{Runtime, ViewerWindow};

    use std::{
        error::Error,
        path::PathBuf,
        time::{Duration, Instant},
    };
    pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

    pub fn main(name: &str, default_data: &str, run: fn(&mut ViewerWindow, &str) -> Result<()>) -> Result<()>
    {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data").join(default_data);

        let path = if default_data.is_empty() {
            String::new()
        } else {
            path.canonicalize()?.to_str().ok_or("Invalid data path")?.to_owned()
        };
        // This standalone process owns the main thread and loads the trusted matching SDK.
        let runtime = unsafe { Runtime::from_env()? };
        let outcome = (|| {
            let mut window = unsafe {
                if matches!(name, "MultiWindowSync" | "LabelCollisionOff") {
                    ViewerWindow::new_dual(&runtime, name, 1280, 760)?
                } else {
                    ViewerWindow::new(&runtime, name, 1200, 800)?
                }
            };
            run(&mut window, &path)
        })();
        let shutdown = unsafe { runtime.shutdown_viewer_application() };
        outcome?;
        shutdown?;
        Ok(())
    }

    pub fn show(window: &mut ViewerWindow) -> Result<()>
    {
        window.show()?;
        window.process_events();
        Ok(())
    }

    pub fn pump(window: &mut ViewerWindow, millis: u64)
    {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            window.process_events();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }
}
